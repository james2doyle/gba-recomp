//! `aot-compatible` packages (SPEC-062): interpreted profile → native runs →
//! one executable linked with the `gba-native-rt` runtime.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use gba_core::{Gba, GbaConfig};
use gba_llvm::Optimization;
use gba_llvm::native::{Entry, RamEntry, TargetSpec, emit_native_objects};

use crate::platform::{self, PackageInputs};
use sha2::{Digest, Sha256};

pub struct ProfileOptions {
    pub frames: u64,
    pub inputs: Vec<(u64, u64, u16)>,
    pub follow_direct_branches: bool,
    /// Compile code observed in EWRAM/IWRAM with content guards (SPEC-065).
    pub ram_code: bool,
    /// Embed the ROM in the executable (`bundle`, SPEC-066).
    pub embed_rom: bool,
    /// Package target (SPEC-068), runtime library and SDL3 location for it.
    pub target: TargetSpec,
    pub runtime_lib: Option<PathBuf>,
    pub sdl_dir: Option<PathBuf>,
}

const RAM_SNAPSHOT: usize = 4096;
const RAM_VARIANTS: usize = 4;

pub struct Profile {
    pub entries: Vec<Entry>,
    pub ram_entries: Vec<RamEntry>,
    pub instructions: u64,
    pub rom_instructions: u64,
    pub outside_rom_instructions: u64,
    pub fault: Option<String>,
    pub seconds: f64,
}

/// Locate the host runtime static library (NB-62-07, SPEC-068).
pub fn runtime_library() -> Result<PathBuf, String> {
    platform::default_runtime(&TargetSpec::host())
}

pub fn parse_keys(text: &str) -> Result<u16, String> {
    use gba_core::keys;
    let mut mask = 0;
    for k in text.split(',').filter(|k| !k.is_empty()) {
        mask |= match k.to_ascii_uppercase().as_str() {
            "A" => keys::A,
            "B" => keys::B,
            "SELECT" => keys::SELECT,
            "START" => keys::START,
            "RIGHT" => keys::RIGHT,
            "LEFT" => keys::LEFT,
            "UP" => keys::UP,
            "DOWN" => keys::DOWN,
            "R" => keys::R,
            "L" => keys::L,
            other => return Err(format!("unknown key `{other}`")),
        };
    }
    Ok(mask)
}

pub fn parse_input(text: &str) -> Result<(u64, u64, u16), String> {
    let parts: Vec<&str> = text.splitn(3, ':').collect();
    if parts.len() != 3 {
        return Err(format!("bad --profile-input `{text}`; use FRAME:LEN:KEYS"));
    }
    Ok((
        parts[0].parse().map_err(|_| "bad profile input frame")?,
        parts[1].parse().map_err(|_| "bad profile input length")?,
        parse_keys(parts[2])?,
    ))
}

/// Run the interpreted core and collect ROM entry points (NB-62-06): every
/// executed instruction that is not the sequential successor of the previous
/// executed instruction in the same ISA state.
pub fn profile(rom: &[u8], options: &ProfileOptions) -> Profile {
    let started = Instant::now();
    let mut gba = Gba::new(GbaConfig {
        rom,
        bios: None,
        backup: None,
        save: None,
    });
    let mut entries = BTreeSet::new();
    let mut ram: std::collections::BTreeMap<(u32, bool), Vec<Vec<u8>>> = Default::default();
    let mut prev: Option<(u32, bool)> = None;
    let mut rom_count = 0u64;
    let mut other = 0u64;
    let mut fault = None;
    'frames: for frame in 0..options.frames {
        let pressed = options
            .inputs
            .iter()
            .filter(|(s, l, _)| frame >= *s && frame < s + l)
            .fold(0, |m, (_, _, k)| m | k);
        gba.set_keys(pressed);
        gba.bus.frame_ready = false;
        while !gba.bus.frame_ready {
            let before = gba.instructions;
            if let Err(f) = gba.step() {
                fault = Some(format!("{f:?}"));
                break 'frames;
            }
            if gba.instructions == before {
                continue;
            }
            let pc = gba.cpu.current_pc;
            let thumb = gba.cpu.current_thumb;
            let size = if thumb { 2 } else { 4 };
            let sequential = prev == Some((pc.wrapping_sub(size), thumb));
            if (0x0800_0000..0x0A00_0000).contains(&pc) {
                rom_count += 1;
                if !sequential {
                    entries.insert(Entry { pc, thumb });
                }
            } else {
                other += 1;
                if options.ram_code && !sequential && matches!(pc >> 24, 0x02 | 0x03) {
                    record_ram_variant(&gba, &mut ram, pc, thumb);
                }
            }
            prev = Some((pc, thumb));
        }
        gba.take_audio();
    }
    let ram_entries = ram
        .into_iter()
        .flat_map(|((pc, thumb), variants)| {
            variants
                .into_iter()
                .map(move |bytes| RamEntry { pc, thumb, bytes })
        })
        .collect();
    Profile {
        entries: entries.into_iter().collect(),
        ram_entries,
        instructions: gba.instructions,
        rom_instructions: rom_count,
        outside_rom_instructions: other,
        fault,
        seconds: started.elapsed().as_secs_f64(),
    }
}

/// RAM-65-01: snapshot the bytes at a RAM entry the first time each
/// distinct content is seen (up to RAM_VARIANTS per entry).
fn record_ram_variant(
    gba: &Gba,
    ram: &mut std::collections::BTreeMap<(u32, bool), Vec<Vec<u8>>>,
    pc: u32,
    thumb: bool,
) {
    let variants = ram.entry((pc, thumb)).or_default();
    let region_size: u32 = if pc >> 24 == 0x02 { 0x4_0000 } else { 0x8000 };
    let offset = pc & (region_size - 1);
    let available = (region_size - offset) as usize;
    let Some(probe) = gba.bus.ram_bytes(pc, available.min(16)) else {
        return;
    };
    if variants.iter().any(|v| v.starts_with(probe)) || variants.len() >= RAM_VARIANTS {
        return;
    }
    if let Some(snapshot) = gba.bus.ram_bytes(pc, available.min(RAM_SNAPSHOT)) {
        variants.push(snapshot.to_vec());
    }
}

pub struct PackageReport {
    pub path: PathBuf,
    pub manifest: String,
}

/// Build an `aot-compatible` package directory atomically (no partial
/// package on failure).
pub fn build_package(
    rom_path: &Path,
    output: &Path,
    optimization: Optimization,
    options: &ProfileOptions,
) -> Result<PackageReport, String> {
    let runtime = match &options.runtime_lib {
        Some(p) => p.clone(),
        None => platform::default_runtime(&options.target)?,
    };
    if !runtime.is_file() {
        return Err(format!("runtime library not found: {}", runtime.display()));
    }
    let sdl_dir = options
        .sdl_dir
        .clone()
        .unwrap_or_else(|| platform::default_sdl_dir(&options.target));
    if output.exists() {
        return Err(format!(
            "output path already exists: {}; choose a new package path",
            output.display()
        ));
    }
    let rom =
        fs::read(rom_path).map_err(|e| format!("cannot read ROM {}: {e}", rom_path.display()))?;
    let sha: [u8; 32] = Sha256::digest(&rom).into();
    let sha_hex: String = sha.iter().map(|b| format!("{b:02x}")).collect();
    let profile = profile(&rom, options);
    if let Some(f) = &profile.fault {
        return Err(format!(
            "interpreted profile stopped with a fault ({f}); fix the core before packaging"
        ));
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let name = output
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("output path must have a UTF-8 directory name")?;
    let temporary = parent.join(format!(".{name}.tmp-{}-{stamp}", std::process::id()));
    fs::create_dir(&temporary).map_err(|e| format!("cannot create temporary package: {e}"))?;
    let result = (|| {
        let started = Instant::now();
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(10);
        let (objects, emitted) = emit_native_objects(
            &rom,
            sha,
            &profile.entries,
            &profile.ram_entries,
            options.follow_direct_branches,
            options.embed_rom,
            &temporary,
            optimization,
            threads,
            &options.target,
        )?;
        let emit_seconds = started.elapsed().as_secs_f64();
        let started = Instant::now();
        let packaged = platform::package(&PackageInputs {
            spec: &options.target,
            objects: &objects,
            runtime: &runtime,
            sdl_dir: &sdl_dir,
            dir: &temporary,
            rom: &rom,
            sha_hex: &sha_hex,
            embed: options.embed_rom,
        })?;
        let sdl_name = packaged.sdl.clone();
        let settings_app = packaged.companions;
        let link_seconds = started.elapsed().as_secs_f64();
        for object in &objects {
            fs::remove_file(object).map_err(|e| format!("cannot remove build object: {e}"))?;
        }
        let s = &emitted.stats;
        let inputs: Vec<String> = options
            .inputs
            .iter()
            .map(|(f, l, k)| format!("\"{f}:{l}:0x{k:03X}\""))
            .collect();
        let manifest = format!(
            "{{\n  \"format_version\": 2,\n  \"execution_mode\": \"aot-compatible\",\n  \"rom_sha256\": \"{sha_hex}\",\n  \"rom_size\": {},\n  \"core_version\": {},\n  \"hle_bios_version\": {},\n  \"native_abi_version\": 1,\n  \"llvm_version\": \"{}\",\n  \"target_triple\": \"{}\",\n  \"optimization\": \"{}\",\n  \"interpreter\": \"gba-core (linked, used for shells, misses and non-ROM code)\",\n  \"jit\": false,\n  \"profile\": {{\"frames\": {}, \"inputs\": [{}], \"instructions\": {}, \"rom_instructions\": {}, \"outside_rom_instructions\": {}, \"entries\": {}, \"seconds\": {:.3}}},\n  \"rom_embedded\": {}, \"native\": {{\"runs\": {}, \"instructions\": {}, \"ram_runs\": {}, \"ram_instructions\": {}, \"ram_entries\": {}, \"shell_sites\": {}, \"static_entries\": {}, \"follow_direct_branches\": {}, \"emit_seconds\": {:.3}, \"link_seconds\": {:.3}}},\n  \"frontend\": \"sdl3 window by default (bundled {sdl_name}); --headless for scripted runs\",\n  \"settings_app\": {settings_app},\n  \"coverage_limits\": \"only ROM code is native; RAM code, shells and misses run in the interpreter and are counted at runtime\"\n}}\n",
            rom.len(),
            gba_core::CORE_VERSION,
            gba_core::bios::HLE_VERSION,
            emitted.info.llvm_version,
            emitted.info.triple,
            optimization.as_str(),
            options.frames,
            inputs.join(", "),
            profile.instructions,
            profile.rom_instructions,
            profile.outside_rom_instructions,
            profile.entries.len(),
            profile.seconds,
            options.embed_rom,
            s.runs,
            s.instructions,
            s.ram_runs,
            s.ram_instructions,
            profile.ram_entries.len(),
            s.shells,
            s.entries_static,
            options.follow_direct_branches,
            emit_seconds,
            link_seconds,
        );
        fs::write(temporary.join("manifest.json"), &manifest)
            .map_err(|e| format!("cannot write manifest: {e}"))?;
        // Platform finishing step: e.g. macOS/iOS `.app`, Android APK.
        let published = platform::published_path(&options.target, output, options.embed_rom);
        if published != output && published.exists() {
            return Err(format!(
                "output path already exists: {}",
                published.display()
            ));
        }
        platform::finalize(
            &options.target,
            &temporary,
            &rom,
            &sha_hex,
            options.embed_rom,
        )?;
        fs::rename(&temporary, &published)
            .map_err(|e| format!("cannot publish package {}: {e}", published.display()))?;
        Ok(PackageReport {
            path: published,
            manifest,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

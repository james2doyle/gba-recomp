//! `aot-compatible` packages (SPEC-062): interpreted profile → native runs →
//! one executable linked with the `gba-native-rt` runtime.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use gba_core::{Gba, GbaConfig};
use gba_llvm::Optimization;
use gba_llvm::native::{Entry, emit_native_object};
use sha2::{Digest, Sha256};

pub struct ProfileOptions {
    pub frames: u64,
    pub inputs: Vec<(u64, u64, u16)>,
    pub follow_direct_branches: bool,
}

pub struct Profile {
    pub entries: Vec<Entry>,
    pub instructions: u64,
    pub rom_instructions: u64,
    pub outside_rom_instructions: u64,
    pub fault: Option<String>,
    pub seconds: f64,
}

/// Locate the runtime static library (NB-62-07). An explicit path wins;
/// otherwise it must sit next to the running `gba-recomp` executable.
pub fn runtime_library() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("GBA_NATIVE_RT_LIB") {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(format!(
                "GBA_NATIVE_RT_LIB does not exist: {}",
                path.display()
            ))
        };
    }
    let exe = env::current_exe().map_err(|e| format!("cannot locate gba-recomp: {e}"))?;
    let dir = exe.parent().ok_or("gba-recomp has no parent directory")?;
    for candidate in [
        dir.join("libgba_native_rt.a"),
        dir.join("../libgba_native_rt.a"),
    ] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "aot-compatible needs the runtime library libgba_native_rt.a next to {} \
         (build it with `cargo build -p gba-native-rt`) or GBA_NATIVE_RT_LIB; no package was produced",
        exe.display()
    ))
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
            }
            prev = Some((pc, thumb));
        }
        gba.take_audio();
    }
    Profile {
        entries: entries.into_iter().collect(),
        instructions: gba.instructions,
        rom_instructions: rom_count,
        outside_rom_instructions: other,
        fault,
        seconds: started.elapsed().as_secs_f64(),
    }
}

fn link(object: &Path, runtime: &Path, executable: &Path) -> Result<(), String> {
    let prefix = env::var_os("LLVM_SYS_231_PREFIX")
        .ok_or("set LLVM_SYS_231_PREFIX to the locked LLVM 23.1.3 installation")?;
    let clang = PathBuf::from(prefix).join("bin/clang");
    let mut command = Command::new(&clang);
    command
        .arg("-fuse-ld=lld")
        .arg(object)
        .arg(runtime)
        .arg("-o")
        .arg(executable);
    #[cfg(target_os = "macos")]
    {
        let sdk = Command::new("xcrun")
            .args(["--sdk", "macosx", "--show-sdk-path"])
            .output()
            .map_err(|e| format!("cannot query macOS SDK: {e}"))?;
        let sdk = String::from_utf8_lossy(&sdk.stdout).trim().to_owned();
        command
            .arg("-isysroot")
            .arg(sdk)
            .arg("-mmacosx-version-min=14.0.0")
            .args(["-lSystem", "-lc", "-lm"]);
    }
    #[cfg(target_os = "linux")]
    command.args(["-lc", "-lm", "-lpthread", "-ldl", "-lgcc_s"]);
    let output = command
        .output()
        .map_err(|e| format!("cannot run {}: {e}", clang.display()))?;
    if !output.status.success() {
        return Err(format!(
            "native link failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
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
    let runtime = runtime_library()?;
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
        let object = temporary.join("game.o");
        let started = Instant::now();
        let emitted = emit_native_object(
            &rom,
            sha,
            &profile.entries,
            options.follow_direct_branches,
            &object,
            optimization,
        )?;
        let emit_seconds = started.elapsed().as_secs_f64();
        let executable = temporary.join("game");
        let started = Instant::now();
        link(&object, &runtime, &executable)?;
        let link_seconds = started.elapsed().as_secs_f64();
        fs::remove_file(&object).map_err(|e| format!("cannot remove build object: {e}"))?;
        let s = &emitted.stats;
        let inputs: Vec<String> = options
            .inputs
            .iter()
            .map(|(f, l, k)| format!("\"{f}:{l}:0x{k:03X}\""))
            .collect();
        let manifest = format!(
            "{{\n  \"format_version\": 2,\n  \"execution_mode\": \"aot-compatible\",\n  \"rom_sha256\": \"{sha_hex}\",\n  \"rom_size\": {},\n  \"core_version\": {},\n  \"hle_bios_version\": {},\n  \"native_abi_version\": 1,\n  \"llvm_version\": \"{}\",\n  \"target_triple\": \"{}\",\n  \"optimization\": \"{}\",\n  \"interpreter\": \"gba-core (linked, used for shells, misses and non-ROM code)\",\n  \"jit\": false,\n  \"profile\": {{\"frames\": {}, \"inputs\": [{}], \"instructions\": {}, \"rom_instructions\": {}, \"outside_rom_instructions\": {}, \"entries\": {}, \"seconds\": {:.3}}},\n  \"native\": {{\"runs\": {}, \"instructions\": {}, \"shell_sites\": {}, \"static_entries\": {}, \"follow_direct_branches\": {}, \"emit_seconds\": {:.3}, \"link_seconds\": {:.3}}},\n  \"coverage_limits\": \"only ROM code is native; RAM code, shells and misses run in the interpreter and are counted at runtime\"\n}}\n",
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
            s.runs,
            s.instructions,
            s.shells,
            s.entries_static,
            options.follow_direct_branches,
            emit_seconds,
            link_seconds,
        );
        fs::write(temporary.join("manifest.json"), &manifest)
            .map_err(|e| format!("cannot write manifest: {e}"))?;
        fs::rename(&temporary, output)
            .map_err(|e| format!("cannot publish package {}: {e}", output.display()))?;
        Ok(PackageReport {
            path: output.to_path_buf(),
            manifest,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

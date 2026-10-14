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
use gba_llvm::native::{Entry, RamEntry, emit_native_objects};
use sha2::{Digest, Sha256};

pub struct ProfileOptions {
    pub frames: u64,
    pub inputs: Vec<(u64, u64, u16)>,
    pub follow_direct_branches: bool,
    /// Compile code observed in EWRAM/IWRAM with content guards (SPEC-065).
    pub ram_code: bool,
    /// Embed the ROM in the executable (`bundle`, SPEC-066).
    pub embed_rom: bool,
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

/// SDL3 shared library bundled into the package (FR-63-02).
fn sdl_library() -> Result<(PathBuf, String), String> {
    let dir = env::var_os("SDL3_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                PathBuf::from("/opt/homebrew/lib")
            } else {
                PathBuf::from("/usr/lib/x86_64-linux-gnu")
            }
        });
    let name = if cfg!(target_os = "macos") {
        "libSDL3.0.dylib"
    } else {
        "libSDL3.so.0"
    };
    let path = dir.join(name);
    if !path.exists() {
        return Err(format!(
            "SDL3 runtime library not found at {} (set SDL3_LIB_DIR); no package was produced",
            path.display()
        ));
    }
    Ok((dir, name.to_owned()))
}

fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("cannot run {what}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{what} failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Copy SDL3 next to the executable and make the executable load it from
/// there (`@executable_path` / `$ORIGIN`), so the package does not depend on
/// the build machine's SDL installation.
fn bundle_sdl(
    package: &Path,
    executable: &Path,
    sdl_dir: &Path,
    sdl_name: &str,
) -> Result<(), String> {
    let source = fs::canonicalize(sdl_dir.join(sdl_name))
        .map_err(|e| format!("cannot resolve SDL3 library: {e}"))?;
    let bundled = package.join(sdl_name);
    fs::copy(&source, &bundled).map_err(|e| format!("cannot bundle SDL3: {e}"))?;
    #[cfg(target_os = "macos")]
    {
        let prefix =
            PathBuf::from(env::var_os("LLVM_SYS_231_PREFIX").ok_or("LLVM_SYS_231_PREFIX unset")?);
        let tool = prefix.join("bin/llvm-install-name-tool");
        let otool = Command::new(prefix.join("bin/llvm-otool"))
            .arg("-L")
            .arg(executable)
            .output()
            .map_err(|e| format!("cannot run llvm-otool: {e}"))?;
        let listing = String::from_utf8_lossy(&otool.stdout);
        let old = listing
            .lines()
            .map(str::trim)
            .find(|l| l.contains("libSDL3"))
            .and_then(|l| l.split(" (").next())
            .ok_or("the executable does not reference SDL3")?
            .to_owned();
        let new = format!("@rpath/{sdl_name}");
        let mut perms = fs::metadata(&bundled)
            .map_err(|e| e.to_string())?
            .permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(&bundled, perms).map_err(|e| e.to_string())?;
        run(
            Command::new(&tool).args(["-id", &new]).arg(&bundled),
            "llvm-install-name-tool -id",
        )?;
        run(
            Command::new(&tool)
                .args(["-change", &old, &new])
                .arg(executable),
            "llvm-install-name-tool -change",
        )?;
        // Rewriting load commands invalidates signatures; re-sign ad hoc.
        for file in [&bundled, &executable.to_path_buf()] {
            run(
                Command::new("codesign")
                    .args(["--force", "--sign", "-"])
                    .arg(file),
                "codesign",
            )?;
        }
    }
    let _ = executable;
    Ok(())
}

/// Application icon (`assets/logo-ico.png`), turned into `AppIcon.icns`.
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/logo-ico.png");

fn game_title(rom: &[u8]) -> String {
    let raw = rom.get(0xA0..0xAC).unwrap_or_default();
    let title: String = String::from_utf8_lossy(raw)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-')
        .collect();
    let title = title.trim();
    if title.is_empty() {
        "GBA Game".to_owned()
    } else {
        title.to_owned()
    }
}

/// Rearrange a flat package directory into `Contents/{MacOS,Resources}`
/// with an Info.plist and icon, then sign it ad hoc as a whole.
fn make_app_bundle(dir: &Path, rom: &[u8], sha_hex: &str) -> Result<(), String> {
    let contents = dir.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos).map_err(|e| e.to_string())?;
    fs::create_dir_all(&resources).map_err(|e| e.to_string())?;
    for name in ["game", "gba-settings", "gba-party", "libSDL3.0.dylib"] {
        let from = dir.join(name);
        if from.exists() {
            fs::rename(&from, macos.join(name)).map_err(|e| format!("cannot move {name}: {e}"))?;
        }
    }
    fs::rename(dir.join("manifest.json"), resources.join("manifest.json"))
        .map_err(|e| format!("cannot move manifest: {e}"))?;
    // Icon set via the system tools (sips/iconutil ship with macOS).
    let iconset = dir.join("AppIcon.iconset");
    fs::create_dir_all(&iconset).map_err(|e| e.to_string())?;
    let source = dir.join("icon-source.png");
    fs::write(&source, APP_ICON_PNG).map_err(|e| e.to_string())?;
    for size in [16u32, 32, 128, 256, 512] {
        for (scale, suffix) in [(1, ""), (2, "@2x")] {
            let px = size * scale;
            let out = iconset.join(format!("icon_{size}x{size}{suffix}.png"));
            run(
                Command::new("sips")
                    .args(["-z", &px.to_string(), &px.to_string()])
                    .arg(&source)
                    .arg("--out")
                    .arg(&out),
                "sips",
            )?;
        }
    }
    run(
        Command::new("iconutil")
            .args(["-c", "icns"])
            .arg(&iconset)
            .arg("-o")
            .arg(resources.join("AppIcon.icns")),
        "iconutil",
    )?;
    fs::remove_dir_all(&iconset).map_err(|e| e.to_string())?;
    fs::remove_file(&source).map_err(|e| e.to_string())?;
    let title = game_title(rom);
    let xml = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>pt-BR</string>
  <key>CFBundleDisplayName</key><string>{t}</string>
  <key>CFBundleName</key><string>{t}</string>
  <key>CFBundleExecutable</key><string>game</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleIdentifier</key><string>dev.gba-recomp.game.{id}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{v}</string>
  <key>CFBundleVersion</key><string>{v}</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
</dict>
</plist>
"#,
        t = xml(&title),
        id = &sha_hex[..12],
        v = env!("CARGO_PKG_VERSION"),
    );
    fs::write(contents.join("Info.plist"), plist).map_err(|e| e.to_string())?;
    run(
        Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(dir),
        "codesign (app)",
    )?;
    Ok(())
}

/// Copy `gba-settings` (EXT-64-03) when it was built next to gba-recomp.
fn bundle_settings_app(package: &Path, sdl_name: &str) -> Result<bool, String> {
    let settings = bundle_companion(package, sdl_name, "gba-settings")?;
    // Party app (SPEC-067); optional like the settings app.
    bundle_companion(package, sdl_name, "gba-party")?;
    Ok(settings)
}

fn bundle_companion(package: &Path, sdl_name: &str, name: &str) -> Result<bool, String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let Some(source) = exe.parent().map(|d| d.join(name)).filter(|p| p.is_file()) else {
        return Ok(false);
    };
    let target = package.join(name);
    fs::copy(&source, &target).map_err(|e| format!("cannot bundle {name}: {e}"))?;
    #[cfg(target_os = "macos")]
    {
        let prefix =
            PathBuf::from(env::var_os("LLVM_SYS_231_PREFIX").ok_or("LLVM_SYS_231_PREFIX unset")?);
        let tool = prefix.join("bin/llvm-install-name-tool");
        let otool = Command::new(prefix.join("bin/llvm-otool"))
            .arg("-L")
            .arg(&target)
            .output()
            .map_err(|e| format!("cannot run llvm-otool: {e}"))?;
        let listing = String::from_utf8_lossy(&otool.stdout);
        if let Some(old) = listing
            .lines()
            .map(str::trim)
            .find(|l| l.contains("libSDL3"))
            .and_then(|l| l.split(" (").next())
        {
            let new = format!("@executable_path/{sdl_name}");
            run(
                Command::new(&tool)
                    .args(["-change", old, &new])
                    .arg(&target),
                "llvm-install-name-tool -change",
            )?;
            run(
                Command::new("codesign")
                    .args(["--force", "--sign", "-"])
                    .arg(&target),
                "codesign",
            )?;
        }
    }
    let _ = sdl_name;
    Ok(true)
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

fn link(
    objects: &[PathBuf],
    runtime: &Path,
    executable: &Path,
    sdl_dir: &Path,
) -> Result<(), String> {
    let prefix = env::var_os("LLVM_SYS_231_PREFIX")
        .ok_or("set LLVM_SYS_231_PREFIX to the locked LLVM 23.1.3 installation")?;
    let clang = PathBuf::from(prefix).join("bin/clang");
    let mut command = Command::new(&clang);
    command
        .arg("-fuse-ld=lld")
        .args(objects)
        .arg(runtime)
        .arg("-o")
        .arg(executable)
        .arg("-L")
        .arg(sdl_dir)
        .arg("-lSDL3");
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
            .args(["-lobjc", "-framework", "AppKit", "-lSystem", "-lc", "-lm"])
            .arg("-Wl,-rpath,@executable_path");
    }
    #[cfg(target_os = "linux")]
    command
        .args(["-lc", "-lm", "-lpthread", "-ldl", "-lgcc_s"])
        .arg("-Wl,-rpath,$ORIGIN");
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
    let (sdl_dir, sdl_name) = sdl_library()?;
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
        )?;
        let emit_seconds = started.elapsed().as_secs_f64();
        let executable = temporary.join("game");
        let started = Instant::now();
        link(&objects, &runtime, &executable, &sdl_dir)?;
        bundle_sdl(&temporary, &executable, &sdl_dir, &sdl_name)?;
        let settings_app = bundle_settings_app(&temporary, &sdl_name)?;
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
        // macOS bundles become a real application (.app) so the menu bar,
        // Dock and Finder show the game's name and icon (SPEC-066 BND-66-05).
        let published = if cfg!(target_os = "macos") && options.embed_rom {
            let app = if output.extension().is_some_and(|e| e == "app") {
                output.to_path_buf()
            } else {
                output.with_extension("app")
            };
            if app.exists() {
                return Err(format!("output path already exists: {}", app.display()));
            }
            make_app_bundle(&temporary, &rom, &sha_hex)?;
            app
        } else {
            output.to_path_buf()
        };
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

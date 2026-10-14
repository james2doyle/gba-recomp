//! macOS adapter (SPEC-068): link with the macOS SDK, bundle SDL3 and the
//! companion apps via `@rpath`, and turn bundles into signed `.app`s.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{PackageInputs, Packaged, run};

pub(super) fn package(i: &PackageInputs<'_>) -> Result<Packaged, String> {
    let sdl_name = "libSDL3.0.dylib";
    let sdl_path = i.sdl_dir.join(sdl_name);
    if !sdl_path.exists() {
        return Err(format!(
            "SDL3 not found at {} (pass --sdl-dir or set SDL3_LIB_DIR)",
            sdl_path.display()
        ));
    }
    let executable = i.dir.join("game");
    link(i, &executable)?;
    bundle_sdl(i.dir, &executable, i.sdl_dir, sdl_name)?;
    let companions = bundle_companions(i.dir, sdl_name)?;
    Ok(Packaged {
        sdl: sdl_name.to_owned(),
        companions,
    })
}

/// Bundles become `<output>.app` (SPEC-066 BND-66-05).
pub(super) fn finalize(dir: &Path, rom: &[u8], sha_hex: &str, embed: bool) -> Result<(), String> {
    if embed {
        make_app_bundle(dir, rom, sha_hex)?;
    }
    Ok(())
}

fn link(i: &PackageInputs<'_>, executable: &Path) -> Result<(), String> {
    let clang = super::llvm_tool("clang")?;
    let sdk = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .map_err(|e| format!("cannot query macOS SDK: {e}"))?;
    let sdk = String::from_utf8_lossy(&sdk.stdout).trim().to_owned();
    let arch = if i.spec.triple.starts_with("x86_64") {
        "x86_64"
    } else {
        "arm64"
    };
    let mut command = Command::new(&clang);
    command
        .args(["-target", &format!("{arch}-apple-macos14.0")])
        .arg("-fuse-ld=lld")
        .args(i.objects)
        .arg(i.runtime)
        .arg("-o")
        .arg(executable)
        .arg("-L")
        .arg(i.sdl_dir)
        .arg("-lSDL3")
        .arg("-isysroot")
        .arg(sdk)
        .args(["-lobjc", "-framework", "AppKit", "-lSystem", "-lc", "-lm"])
        .arg("-Wl,-rpath,@executable_path");
    run(&mut command, "native link (macOS)")
}

/// Copy SDL3 next to the executable and make the executable load it from
/// there (`@executable_path` / `$ORIGIN`), so the package does not depend on
/// the build machine's SDL installation.
pub(super) fn bundle_sdl(
    package: &Path,
    executable: &Path,
    sdl_dir: &Path,
    sdl_name: &str,
) -> Result<(), String> {
    let source = fs::canonicalize(sdl_dir.join(sdl_name))
        .map_err(|e| format!("cannot resolve SDL3 library: {e}"))?;
    let bundled = package.join(sdl_name);
    fs::copy(&source, &bundled).map_err(|e| format!("cannot bundle SDL3: {e}"))?;
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
const APP_ICON_PNG: &[u8] = include_bytes!("../../../../assets/logo-ico.png");

pub(super) fn game_title(rom: &[u8]) -> String {
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
pub(super) fn make_app_bundle(dir: &Path, rom: &[u8], sha_hex: &str) -> Result<(), String> {
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
pub(super) fn bundle_companions(package: &Path, sdl_name: &str) -> Result<bool, String> {
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

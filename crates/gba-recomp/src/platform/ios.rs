//! iOS adapter (SPEC-068 TGT-68-02): `aarch64-apple-ios` (device) and
//! `aarch64-apple-ios-sim` (simulator on Apple silicon).
//!
//! The executable links the generated objects, the runtime built for the
//! target (whose `main` hands the main thread to SDL3 via `SDL_RunApp`) and
//! a static `libSDL3.a` built for the same SDK. The package becomes a flat
//! iOS `.app` (executable `game`, `Info.plist`, icons, `manifest.json`).
//! Simulator apps are signed ad hoc; device apps stay without a bundle
//! signature and are also zipped into an unsigned `.ipa` (`Payload/`), to be
//! signed with the user's own certificate and provisioning profile.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use gba_llvm::native::TargetSpec;

use super::apple::{APP_ICON_PNG, game_title};
use super::{PackageInputs, Packaged, run};

/// SDL3 frameworks for iOS, as listed by SDL 3.4's `sdl3.pc` (static build).
const FRAMEWORKS: &[&str] = &[
    "CoreMedia",
    "CoreVideo",
    "CoreAudio",
    "AudioToolbox",
    "AVFoundation",
    "CoreBluetooth",
    "CoreGraphics",
    "CoreMotion",
    "Foundation",
    "GameController",
    "Metal",
    "OpenGLES",
    "QuartzCore",
    "UIKit",
];

/// Lowest iOS version of the package (matches `TargetSpec` LLVM triples).
const MINIMUM_OS: &str = "14.0";

fn is_simulator(spec: &TargetSpec) -> bool {
    spec.triple.ends_with("-sim")
}

fn sdk_name(spec: &TargetSpec) -> &'static str {
    if is_simulator(spec) {
        "iphonesimulator"
    } else {
        "iphoneos"
    }
}

fn xcrun_sdk(sdk: &str, query: &str) -> Result<String, String> {
    let output = Command::new("xcrun")
        .args(["--sdk", sdk, query])
        .output()
        .map_err(|e| format!("cannot run xcrun (Xcode is required for iOS): {e}"))?;
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || value.is_empty() {
        return Err(format!(
            "the {sdk} SDK is not available (`xcrun --sdk {sdk} {query}` failed); install Xcode with the iOS platform"
        ));
    }
    Ok(value)
}

pub(super) fn package(i: &PackageInputs<'_>) -> Result<Packaged, String> {
    if !i.embed {
        return Err(
            "iOS packages need the embedded ROM (no command line on iOS): use `gba-recomp bundle`"
                .to_owned(),
        );
    }
    let sdl = i.sdl_dir.join("libSDL3.a");
    if !sdl.is_file() {
        return Err(format!(
            "static SDL3 for {} not found at {} (build it with scripts/ci/setup-sdl3-ios.sh and pass --sdl-dir <dest>/{}/lib)",
            i.spec.triple,
            sdl.display(),
            sdk_name(i.spec)
        ));
    }
    let sdk = xcrun_sdk(sdk_name(i.spec), "--show-sdk-path")?;
    let target = if is_simulator(i.spec) {
        format!("arm64-apple-ios{MINIMUM_OS}-simulator")
    } else {
        format!("arm64-apple-ios{MINIMUM_OS}")
    };
    let clang = super::llvm_tool("clang")?;
    let mut command = Command::new(&clang);
    command
        .args(["-target", &target])
        .arg("-fuse-ld=lld")
        .arg("-isysroot")
        .arg(&sdk)
        .args(i.objects)
        .arg(i.runtime)
        .arg(&sdl)
        .arg("-o")
        .arg(i.dir.join("game"))
        .arg("-Wl,-dead_strip");
    for framework in FRAMEWORKS {
        command.args(["-framework", framework]);
    }
    command
        .args(["-weak_framework", "CoreHaptics"])
        .args(["-lobjc", "-lc++", "-lSystem", "-lc", "-lm"]);
    run(&mut command, "native link (iOS)")?;
    Ok(Packaged {
        sdl: "libSDL3.a (static, linked into game)".to_owned(),
        companions: false,
    })
}

/// Lay the flat package out as an iOS `.app`: `Info.plist`, icons and, for
/// the simulator, an ad hoc signature.
pub(super) fn finalize(
    spec: &TargetSpec,
    dir: &Path,
    rom: &[u8],
    sha_hex: &str,
    embed: bool,
) -> Result<(), String> {
    if !embed {
        return Err("iOS packages need the embedded ROM: use `gba-recomp bundle`".to_owned());
    }
    let icons = write_icons(dir)?;
    let sdk = sdk_name(spec);
    let sdk_version = xcrun_sdk(sdk, "--show-sdk-version")?;
    let plist = info_plist(
        &game_title(rom),
        &sha_hex[..12],
        is_simulator(spec),
        &sdk_version,
        &icons,
    );
    fs::write(dir.join("Info.plist"), plist)
        .map_err(|e| format!("cannot write Info.plist: {e}"))?;
    fs::write(dir.join("PkgInfo"), "APPL????").map_err(|e| e.to_string())?;
    if is_simulator(spec) {
        run(
            Command::new("codesign")
                .args(["--force", "--sign", "-", "--timestamp=none"])
                .arg(dir),
            "codesign (iOS simulator app)",
        )?;
    }
    Ok(())
}

/// Device builds: zip the published `.app` as `Payload/<name>.app` into an
/// unsigned `<name>.ipa` next to it.
pub(super) fn after_publish(spec: &TargetSpec, app: &Path) -> Result<Option<PathBuf>, String> {
    if is_simulator(spec) {
        return Ok(None);
    }
    let ipa = app.with_extension("ipa");
    let name = app
        .file_name()
        .ok_or("the published app has no file name")?
        .to_owned();
    let parent = app
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let staging = parent.join(format!(".{}.ipa-staging", name.to_string_lossy()));
    let _ = fs::remove_dir_all(&staging);
    let payload = staging.join("Payload");
    fs::create_dir_all(&payload).map_err(|e| e.to_string())?;
    let result = (|| {
        run(
            Command::new("ditto").arg(app).arg(payload.join(&name)),
            "ditto (Payload)",
        )?;
        if ipa.exists() {
            fs::remove_file(&ipa).map_err(|e| format!("cannot replace {}: {e}", ipa.display()))?;
        }
        run(
            Command::new("ditto")
                .args(["-c", "-k", "--norsrc", "--noextattr", "--keepParent"])
                .arg(&payload)
                .arg(&ipa),
            "ditto (ipa)",
        )
    })();
    let _ = fs::remove_dir_all(&staging);
    result.map(|()| Some(ipa))
}

/// iPhone/iPad icon PNGs (no asset catalog): `AppIcon<size>@<n>x.png`,
/// referenced by `CFBundleIconFiles`. Returns the base names.
fn write_icons(dir: &Path) -> Result<Vec<(&'static str, bool)>, String> {
    let source = dir.join(".icon-source.png");
    fs::write(&source, APP_ICON_PNG).map_err(|e| e.to_string())?;
    // (file name, pixels)
    let files = [
        ("AppIcon60x60@2x.png", 120),
        ("AppIcon60x60@3x.png", 180),
        ("AppIcon76x76@2x~ipad.png", 152),
        ("AppIcon83.5x83.5@2x~ipad.png", 167),
    ];
    let result = files.iter().try_for_each(|(file, px)| {
        run(
            Command::new("sips")
                .args(["-z", &px.to_string(), &px.to_string()])
                .arg(&source)
                .arg("--out")
                .arg(dir.join(file)),
            "sips",
        )
    });
    fs::remove_file(&source).map_err(|e| e.to_string())?;
    result?;
    // (base name, iPad only)
    Ok(vec![
        ("AppIcon60x60", false),
        ("AppIcon76x76", true),
        ("AppIcon83.5x83.5", true),
    ])
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn info_plist(
    title: &str,
    id: &str,
    simulator: bool,
    sdk_version: &str,
    icons: &[(&str, bool)],
) -> String {
    let (platform, dt_platform) = if simulator {
        ("iPhoneSimulator", "iphonesimulator")
    } else {
        ("iPhoneOS", "iphoneos")
    };
    let icon_list = |ipad: bool| -> String {
        icons
            .iter()
            .filter(|(_, only_ipad)| ipad || !only_ipad)
            .map(|(name, _)| format!("<string>{name}</string>"))
            .collect()
    };
    let icon_dict = |ipad: bool| {
        format!(
            "<dict><key>CFBundlePrimaryIcon</key><dict><key>CFBundleIconFiles</key><array>{}</array></dict></dict>",
            icon_list(ipad)
        )
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>pt-BR</string>
  <key>CFBundleDisplayName</key><string>{t}</string>
  <key>CFBundleName</key><string>{t}</string>
  <key>CFBundleExecutable</key><string>game</string>
  <key>CFBundleIdentifier</key><string>dev.gba-recomp.game.{id}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{v}</string>
  <key>CFBundleVersion</key><string>{v}</string>
  <key>CFBundleSupportedPlatforms</key><array><string>{platform}</string></array>
  <key>CFBundleIcons</key>{icons_iphone}
  <key>CFBundleIcons~ipad</key>{icons_ipad}
  <key>DTPlatformName</key><string>{dt_platform}</string>
  <key>DTSDKName</key><string>{dt_platform}{sdk}</string>
  <key>MinimumOSVersion</key><string>{min}</string>
  <key>LSRequiresIPhoneOS</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
  <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
  <key>UILaunchScreen</key><dict/>
  <key>UIRequiresFullScreen</key><true/>
  <key>UIStatusBarHidden</key><true/>
  <key>UIViewControllerBasedStatusBarAppearance</key><false/>
  <key>UISupportedInterfaceOrientations</key>
  <array>
    <string>UIInterfaceOrientationLandscapeLeft</string>
    <string>UIInterfaceOrientationLandscapeRight</string>
    <string>UIInterfaceOrientationPortrait</string>
  </array>
  <key>UISupportedInterfaceOrientations~ipad</key>
  <array>
    <string>UIInterfaceOrientationLandscapeLeft</string>
    <string>UIInterfaceOrientationLandscapeRight</string>
    <string>UIInterfaceOrientationPortrait</string>
    <string>UIInterfaceOrientationPortraitUpsideDown</string>
  </array>
  <key>GCSupportsControllerUserInteraction</key><true/>
  <key>ITSAppUsesNonExemptEncryption</key><false/>
</dict>
</plist>
"#,
        t = xml(title),
        v = env!("CARGO_PKG_VERSION"),
        sdk = sdk_version,
        min = MINIMUM_OS,
        icons_iphone = icon_dict(false),
        icons_ipad = icon_dict(true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_names_platform_and_identifier() {
        let device = info_plist(
            "A<B",
            "0123456789ab",
            false,
            "18.5",
            &[("AppIcon60x60", false)],
        );
        assert!(device.contains("<string>iPhoneOS</string>"));
        assert!(device.contains("dev.gba-recomp.game.0123456789ab"));
        assert!(device.contains("<string>A&lt;B</string>"));
        assert!(device.contains("<string>iphoneos18.5</string>"));
        let sim = info_plist("X", "0123456789ab", true, "18.5", &[]);
        assert!(sim.contains("<string>iPhoneSimulator</string>"));
    }

    #[test]
    fn simulator_is_detected_by_triple() {
        let sim = TargetSpec::for_triple("aarch64-apple-ios-sim").unwrap();
        let device = TargetSpec::for_triple("aarch64-apple-ios").unwrap();
        assert!(is_simulator(&sim));
        assert!(!is_simulator(&device));
    }
}

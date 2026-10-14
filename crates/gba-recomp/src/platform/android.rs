//! Android adapter (SPEC-068 TGT-68-02): `aarch64-linux-android` → APK.
//!
//! The LLVM objects and the runtime staticlib (built for
//! `aarch64-linux-android`) are linked into `libmain.so` with the NDK clang;
//! a minimal Gradle project (templates in `android/`) wraps it with SDL3's
//! `SDLActivity`, which loads `libSDL3.so` + `libmain.so` and calls the
//! generated `SDL_main`. Gradle builds a release APK signed with the debug
//! key; only `arm64-v8a` is packaged.
//!
//! Inputs: `--sdl-dir` holds the Android `libSDL3.so` (arm64-v8a); SDL's
//! Java sources are looked up in `SDL3_ANDROID_SOURCE` or next to the build
//! directory (`scripts/ci/setup-sdl3-android.sh` lays this out). The NDK
//! comes from `ANDROID_NDK_HOME`/`ANDROID_NDK_LATEST_HOME`/`ANDROID_NDK_ROOT`
//! or `$ANDROID_HOME/ndk/<version>`; the SDK from `ANDROID_HOME`
//! (`ANDROID_SDK_ROOT`); Gradle from `GBA_GRADLE` or `gradle` on PATH.
//! `GBA_APP_NAME` overrides the launcher label (default: ROM header title).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{PackageInputs, Packaged, run};

/// Minimum Android API level (matches the LLVM triple `aarch64-linux-android24`).
const API_LEVEL: u32 = 24;
const ABI: &str = "arm64-v8a";
const APK_NAME: &str = "game.apk";
const SDL_JAVA: &str = "org/libsdl/app";

const SETTINGS_GRADLE: &str = include_str!("android/settings.gradle");
const ROOT_BUILD_GRADLE: &str = include_str!("android/build.gradle");
const GRADLE_PROPERTIES: &str = include_str!("android/gradle.properties");
const APP_BUILD_GRADLE: &str = include_str!("android/app.build.gradle");
const MANIFEST: &str = include_str!("android/AndroidManifest.xml");
const ACTIVITY: &str = include_str!("android/GameActivity.java");
const STRINGS: &str = include_str!("android/strings.xml");
/// Launcher icons: `assets/logo-ico.png` resized per density.
const ICONS: [(&str, &[u8]); 5] = [
    (
        "mdpi",
        include_bytes!("android/res/mipmap-mdpi/ic_launcher.png"),
    ),
    (
        "hdpi",
        include_bytes!("android/res/mipmap-hdpi/ic_launcher.png"),
    ),
    (
        "xhdpi",
        include_bytes!("android/res/mipmap-xhdpi/ic_launcher.png"),
    ),
    (
        "xxhdpi",
        include_bytes!("android/res/mipmap-xxhdpi/ic_launcher.png"),
    ),
    (
        "xxxhdpi",
        include_bytes!("android/res/mipmap-xxxhdpi/ic_launcher.png"),
    ),
];

/// Tools found before any work starts, so a missing piece fails early.
struct Toolchain {
    clang: PathBuf,
    sdk: PathBuf,
    gradle: PathBuf,
    sdl_lib: PathBuf,
    sdl_java: PathBuf,
}

pub(super) fn package(i: &PackageInputs<'_>) -> Result<Packaged, String> {
    if !i.embed {
        return Err(
            "Android packages need the embedded ROM (an APK has no command line): use `gba-recomp bundle`"
                .to_owned(),
        );
    }
    let tools = locate(i.sdl_dir)?;
    let project = i.dir.join("android-project");
    let jni = project.join("app/src/main/jniLibs").join(ABI);
    fs::create_dir_all(&jni).map_err(|e| format!("cannot create {}: {e}", jni.display()))?;
    // libmain.so links against the copy that ships in the APK.
    fs::copy(&tools.sdl_lib, jni.join("libSDL3.so"))
        .map_err(|e| format!("cannot copy libSDL3.so: {e}"))?;
    link(i, &tools.clang, &jni.join("libmain.so"))?;
    write_project(
        &project,
        &app_name(i.rom),
        i.sha_hex,
        &tools.sdk,
        &tools.sdl_java,
    )?;
    gradle(&tools.gradle, &project, &tools.sdk)?;
    let apk = project.join("app/build/outputs/apk/release/app-release.apk");
    if !apk.is_file() {
        return Err(format!("Gradle finished without {}", apk.display()));
    }
    fs::copy(&apk, i.dir.join(APK_NAME)).map_err(|e| format!("cannot copy the APK: {e}"))?;
    fs::remove_dir_all(&project).map_err(|e| format!("cannot remove the Gradle project: {e}"))?;
    Ok(Packaged {
        sdl: "libSDL3.so (inside the APK)".to_owned(),
        companions: false,
    })
}

pub(super) fn finalize(
    _dir: &Path,
    _rom: &[u8],
    _sha_hex: &str,
    _embed: bool,
) -> Result<(), String> {
    Ok(())
}

fn locate(sdl_dir: &Path) -> Result<Toolchain, String> {
    let sdl_lib = [sdl_dir.join("libSDL3.so"), sdl_dir.join("lib").join("libSDL3.so")]
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| {
            format!(
                "Android libSDL3.so ({ABI}) not found in {} (build it with scripts/ci/setup-sdl3-android.sh and pass --sdl-dir)",
                sdl_dir.display()
            )
        })?;
    let sdl_java = sdl_java_dir(sdl_dir)?;
    let ndk = ndk_root()?;
    let clang = ndk_clang(&ndk)?;
    let sdk = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .into_iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .find(|p| p.is_dir())
        .ok_or("Android SDK not found: set ANDROID_HOME to the SDK (platforms;android-35, build-tools) to build the APK")?;
    let gradle = gradle_command()?;
    Ok(Toolchain {
        clang,
        sdk,
        gradle,
        sdl_lib,
        sdl_java,
    })
}

/// SDL's `org/libsdl/app` Java sources (the `android-project` of the SDL
/// release the library was built from).
fn sdl_java_dir(sdl_dir: &Path) -> Result<PathBuf, String> {
    let rel = Path::new("android-project/app/src/main/java").join(SDL_JAVA);
    let mut candidates = Vec::new();
    if let Some(src) = env::var_os("SDL3_ANDROID_SOURCE") {
        candidates.push(PathBuf::from(src).join(&rel));
    }
    candidates.push(sdl_dir.join("java").join(SDL_JAVA));
    let mut dir = Some(sdl_dir);
    while let Some(d) = dir {
        candidates.push(d.join(&rel));
        dir = d.parent();
    }
    candidates
        .into_iter()
        .find(|p| p.join("SDLActivity.java").is_file())
        .ok_or_else(|| {
            format!(
                "SDL3 Java sources ({SDL_JAVA}/SDLActivity.java) not found near {}; set SDL3_ANDROID_SOURCE to the SDL source tree",
                sdl_dir.display()
            )
        })
}

fn ndk_root() -> Result<PathBuf, String> {
    let has_toolchain = |p: &Path| p.join("toolchains/llvm/prebuilt").is_dir();
    for var in [
        "ANDROID_NDK_HOME",
        "ANDROID_NDK_LATEST_HOME",
        "ANDROID_NDK_ROOT",
        "ANDROID_NDK",
    ] {
        if let Some(p) = env::var_os(var).map(PathBuf::from)
            && has_toolchain(&p)
        {
            return Ok(p);
        }
    }
    // Side-by-side NDKs in the SDK: pick the highest version.
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        let Some(sdk) = env::var_os(var).map(PathBuf::from) else {
            continue;
        };
        let Ok(entries) = fs::read_dir(sdk.join("ndk")) else {
            continue;
        };
        let best = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| has_toolchain(p))
            .max_by_key(|p| version_key(p));
        if let Some(p) = best {
            return Ok(p);
        }
    }
    Err("Android NDK not found: set ANDROID_NDK_HOME (or ANDROID_NDK_LATEST_HOME / ANDROID_HOME with ndk/<version>)".to_owned())
}

fn version_key(p: &Path) -> Vec<u64> {
    p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .split('.')
        .map(|s| s.parse().unwrap_or(0))
        .collect()
}

/// `aarch64-linux-android<API>-clang` from the NDK's prebuilt LLVM.
fn ndk_clang(ndk: &Path) -> Result<PathBuf, String> {
    let prebuilt = ndk.join("toolchains/llvm/prebuilt");
    let hosts = fs::read_dir(&prebuilt).map_err(|e| format!("{}: {e}", prebuilt.display()))?;
    let name = if cfg!(windows) {
        format!("aarch64-linux-android{API_LEVEL}-clang.cmd")
    } else {
        format!("aarch64-linux-android{API_LEVEL}-clang")
    };
    hosts
        .filter_map(|e| e.ok().map(|e| e.path().join("bin").join(&name)))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("{name} not found under {}", prebuilt.display()))
}

fn gradle_command() -> Result<PathBuf, String> {
    if let Some(g) = env::var_os("GBA_GRADLE") {
        return Ok(PathBuf::from(g));
    }
    let name = if cfg!(windows) {
        "gradle.bat"
    } else {
        "gradle"
    };
    env::var_os("PATH")
        .into_iter()
        .flat_map(|p| env::split_paths(&p).collect::<Vec<_>>())
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            "Gradle not found: install Gradle 8.9+ (with JDK 17) on PATH or set GBA_GRADLE to build the APK".to_owned()
        })
}

/// Link the game objects and the runtime into the shared object that
/// `SDLActivity` loads (`libmain.so`, entry `SDL_main`).
fn link(i: &PackageInputs<'_>, clang: &Path, out: &Path) -> Result<(), String> {
    let jni = out.parent().ok_or("libmain.so has no parent")?;
    let mut command = Command::new(clang);
    command
        .arg("-shared")
        .arg("-o")
        .arg(out)
        .arg("-Wl,-soname,libmain.so")
        .args(i.objects)
        .arg(i.runtime)
        .arg("-L")
        .arg(jni)
        .args(["-lSDL3", "-llog", "-landroid", "-lm", "-ldl", "-lc"])
        .args([
            "-Wl,--no-undefined",
            "-Wl,--gc-sections",
            "-Wl,--strip-debug",
            // 16 KB pages (required for new apps on Android 15+ devices).
            "-Wl,-z,max-page-size=16384",
        ]);
    run(&mut command, "native link (Android libmain.so)")
}

/// Launcher label: `GBA_APP_NAME` when set (ROMs with an empty header
/// title, e.g. homebrew), else the ROM header title.
fn app_name(rom: &[u8]) -> String {
    env::var("GBA_APP_NAME")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| super::apple::game_title(rom))
}

/// `dev.gbarecomp.game.g<sha12>`: one application id per ROM.
fn application_id(sha_hex: &str) -> String {
    format!("dev.gbarecomp.game.g{}", &sha_hex[..12])
}

/// Android string resource escaping (XML plus `'`, `"`, leading `@`/`?`).
fn android_string(s: &str) -> String {
    let mut out = s
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"");
    if out.starts_with('@') || out.starts_with('?') {
        out.insert(0, '\\');
    }
    out
}

/// Lay out the Gradle project (everything except jniLibs).
fn write_project(
    project: &Path,
    title: &str,
    sha_hex: &str,
    sdk: &Path,
    sdl_java: &Path,
) -> Result<(), String> {
    let write = |rel: &str, bytes: &[u8]| -> Result<(), String> {
        let path = project.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
    };
    write("settings.gradle", SETTINGS_GRADLE.as_bytes())?;
    write("build.gradle", ROOT_BUILD_GRADLE.as_bytes())?;
    write("gradle.properties", GRADLE_PROPERTIES.as_bytes())?;
    // Gradle properties files use `\` as escape: forward slashes are safe.
    let sdk_dir = sdk.to_string_lossy().replace('\\', "/");
    write(
        "local.properties",
        format!("sdk.dir={sdk_dir}\n").as_bytes(),
    )?;
    write(
        "app/build.gradle",
        APP_BUILD_GRADLE
            .replace("{{APPLICATION_ID}}", &application_id(sha_hex))
            .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
            .as_bytes(),
    )?;
    write("app/src/main/AndroidManifest.xml", MANIFEST.as_bytes())?;
    write(
        "app/src/main/java/dev/gbarecomp/game/GameActivity.java",
        ACTIVITY.as_bytes(),
    )?;
    write(
        "app/src/main/res/values/strings.xml",
        STRINGS
            .replace("{{APP_NAME}}", &android_string(title))
            .as_bytes(),
    )?;
    for (density, png) in ICONS {
        write(
            &format!("app/src/main/res/mipmap-{density}/ic_launcher.png"),
            png,
        )?;
    }
    copy_dir(sdl_java, &project.join("app/src/main/java").join(SDL_JAVA))
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            fs::copy(&path, &target).map_err(|e| format!("cannot copy {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

fn gradle(gradle: &Path, project: &Path, sdk: &Path) -> Result<(), String> {
    run(
        Command::new(gradle)
            .arg("--no-daemon")
            .arg("--console=plain")
            .arg("-p")
            .arg(project)
            .arg("assembleRelease")
            .env("ANDROID_HOME", sdk),
        "Gradle (assembleRelease)",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_with_title(title: &[u8]) -> Vec<u8> {
        let mut rom = vec![0u8; 0xC0];
        rom[0xA0..0xA0 + title.len()].copy_from_slice(title);
        rom
    }

    #[test]
    fn project_layout_and_substitutions() {
        let base = env::temp_dir().join(format!("gba-android-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let java = base
            .join("sdl/android-project/app/src/main/java")
            .join(SDL_JAVA);
        fs::create_dir_all(&java).unwrap();
        fs::write(java.join("SDLActivity.java"), "class SDLActivity {}").unwrap();
        let sdl_build = base.join("sdl/build-android");
        fs::create_dir_all(&sdl_build).unwrap();
        // The Java sources are found next to the SDL build directory.
        let found = sdl_java_dir(&sdl_build).unwrap();
        assert_eq!(found, java);

        let project = base.join("project");
        let sha = "0123456789abcdef".repeat(4);
        write_project(
            &project,
            &crate::platform::apple::game_title(&rom_with_title(b"RKANOID\0\0\0\0\0")),
            &sha,
            Path::new("/opt/android-sdk"),
            &found,
        )
        .unwrap();
        let read = |rel: &str| fs::read_to_string(project.join(rel)).unwrap();
        let app = read("app/build.gradle");
        assert!(app.contains("applicationId \"dev.gbarecomp.game.g0123456789ab\""));
        assert!(!app.contains("{{"));
        assert!(read("app/src/main/res/values/strings.xml").contains(">RKANOID<"));
        let manifest = read("app/src/main/AndroidManifest.xml");
        assert!(manifest.contains("dev.gbarecomp.game.GameActivity"));
        assert!(manifest.contains("sensorLandscape"));
        assert!(manifest.contains("Fullscreen"));
        let activity = read("app/src/main/java/dev/gbarecomp/game/GameActivity.java");
        assert!(activity.contains("extends SDLActivity"));
        assert!(activity.contains("{\"SDL3\", \"main\"}"));
        assert!(
            project
                .join("app/src/main/java/org/libsdl/app/SDLActivity.java")
                .is_file()
        );
        assert_eq!(read("local.properties"), "sdk.dir=/opt/android-sdk\n");
        for (density, _) in ICONS {
            let icon = fs::read(
                project.join(format!("app/src/main/res/mipmap-{density}/ic_launcher.png")),
            )
            .unwrap();
            assert_eq!(&icon[..8], b"\x89PNG\r\n\x1a\n");
        }
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn string_resource_escaping() {
        assert_eq!(android_string("Tom's <Game>"), "Tom\\'s &lt;Game&gt;");
        assert_eq!(android_string("@home"), "\\@home");
        assert_eq!(android_string("RKANOID"), "RKANOID");
    }

    #[test]
    fn ndk_versions_sort_numerically() {
        let a = version_key(Path::new("/sdk/ndk/27.2.12479018"));
        let b = version_key(Path::new("/sdk/ndk/28.0.13004108"));
        let c = version_key(Path::new("/sdk/ndk/9.0.1"));
        assert!(b > a && a > c);
    }
}

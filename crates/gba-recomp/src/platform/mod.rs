//! Per-platform link and packaging adapters (SPEC-068 TGT-68-01/02).
//!
//! `compat` profiles the ROM and emits LLVM objects for the target triple;
//! the adapter for the target's platform links them with the runtime built
//! for that target, lays out the package directory and finalises it.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use gba_llvm::native::TargetSpec;

mod android;
mod apple;
mod ios;
mod linux;
mod web;
mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
    Ios,
    Android,
    Web,
}

impl Platform {
    pub fn of(spec: &TargetSpec) -> Self {
        let t = spec.triple.as_str();
        if t.contains("apple-ios") {
            Platform::Ios
        } else if t.contains("apple") {
            Platform::MacOs
        } else if t.contains("windows") {
            Platform::Windows
        } else if t.contains("android") {
            Platform::Android
        } else if t.starts_with("wasm32") {
            Platform::Web
        } else {
            Platform::Linux
        }
    }

    pub fn runtime_lib_name(self) -> &'static str {
        match self {
            Platform::Windows => "gba_native_rt.lib",
            _ => "libgba_native_rt.a",
        }
    }
}

/// Everything an adapter needs to link and lay out a package in `dir`.
#[allow(dead_code)] // rom/sha_hex/embed are used by the iOS, Android and Web adapters.
pub struct PackageInputs<'a> {
    pub spec: &'a TargetSpec,
    pub objects: &'a [PathBuf],
    pub runtime: &'a Path,
    pub sdl_dir: &'a Path,
    pub dir: &'a Path,
    pub rom: &'a [u8],
    pub sha_hex: &'a str,
    pub embed: bool,
}

pub struct Packaged {
    /// SDL3 library file shipped in the package (or a note for static SDL).
    pub sdl: String,
    /// Whether gba-settings/gba-party were included.
    pub companions: bool,
}

pub fn package(i: &PackageInputs<'_>) -> Result<Packaged, String> {
    match Platform::of(i.spec) {
        Platform::MacOs => apple::package(i),
        Platform::Linux => linux::package(i),
        Platform::Windows => windows::package(i),
        Platform::Ios => ios::package(i),
        Platform::Android => android::package(i),
        Platform::Web => web::package(i),
    }
}

/// Platform checks that must pass before profiling or emitting anything
/// (SPEC-068 TGT-68-01): e.g. the Web target needs `emcc` and an embedded ROM.
pub fn preflight(spec: &TargetSpec, embed: bool) -> Result<(), String> {
    match Platform::of(spec) {
        Platform::Web => web::preflight(embed),
        _ => Ok(()),
    }
}

/// Final path of the published package for `output`.
pub fn published_path(spec: &TargetSpec, output: &Path, embed: bool) -> PathBuf {
    let platform = Platform::of(spec);
    let wants_app = (platform == Platform::MacOs && embed) || platform == Platform::Ios;
    if wants_app && !output.extension().is_some_and(|e| e == "app") {
        // Append (names like `game-v3.00` contain dots).
        let mut name = output.as_os_str().to_owned();
        name.push(".app");
        PathBuf::from(name)
    } else {
        output.to_path_buf()
    }
}

/// Platform-specific last step on the complete package directory.
pub fn finalize(
    spec: &TargetSpec,
    dir: &Path,
    rom: &[u8],
    sha_hex: &str,
    embed: bool,
) -> Result<(), String> {
    match Platform::of(spec) {
        Platform::MacOs => apple::finalize(dir, rom, sha_hex, embed),
        Platform::Ios => ios::finalize(spec, dir, rom, sha_hex, embed),
        Platform::Android => android::finalize(dir, rom, sha_hex, embed),
        Platform::Web => web::finalize(dir, rom, sha_hex, embed),
        Platform::Linux | Platform::Windows => Ok(()),
    }
}

/// Step run on the published package; returns an extra distributable
/// (the unsigned `.ipa` for iOS devices), if any.
pub fn after_publish(spec: &TargetSpec, published: &Path) -> Result<Option<PathBuf>, String> {
    match Platform::of(spec) {
        Platform::Ios => ios::after_publish(spec, published),
        _ => Ok(None),
    }
}

/// Default runtime library for `spec`: `GBA_NATIVE_RT_LIB`, else next to
/// gba-recomp (host target) or in cargo's `target/<triple>/release`.
pub fn default_runtime(spec: &TargetSpec) -> Result<PathBuf, String> {
    let name = Platform::of(spec).runtime_lib_name();
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
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("gba-recomp has no parent directory")?;
    let candidates = runtime_candidates(dir, spec, name);
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        format!(
            "runtime library {name} for {} not found; build it with `cargo build --release -p gba-native-rt --target {}` or pass --runtime-lib",
            spec.triple, spec.triple
        )
    })
}

/// Where [`default_runtime`] looks for the library, in order.
///
/// `dir` is the directory holding the `gba-recomp` binary: `target/release`
/// for a normal build. Cargo puts a cross-compiled staticlib in
/// `target/<triple>/release`, which is *one* level above that binary, not
/// two: walking up twice looked in `<repo>/<triple>/release` and so always
/// missed. Every workflow and `scripts/ci/package.sh` pass `--runtime-lib`
/// explicitly, which is what kept the mistake hidden.
fn runtime_candidates(dir: &Path, spec: &TargetSpec, name: &str) -> Vec<PathBuf> {
    let mut candidates = vec![dir.join("..").join(&spec.triple).join("release").join(name)];
    if *spec == TargetSpec::host() {
        candidates.insert(0, dir.join(name));
        candidates.push(dir.join("..").join(name));
    }
    candidates
}

/// Default SDL3 location for `spec`: `SDL3_LIB_DIR` or the usual install
/// directory for the platform.
pub fn default_sdl_dir(spec: &TargetSpec) -> PathBuf {
    // iOS: static SDL3 per SDK (scripts/ci/setup-sdl3-ios.sh).
    if Platform::of(spec) == Platform::Ios {
        let var = if spec.triple.ends_with("-sim") {
            "SDL3_IOS_SIM_DIR"
        } else {
            "SDL3_IOS_DIR"
        };
        if let Some(dir) = env::var_os(var) {
            return PathBuf::from(dir);
        }
    }
    if let Some(dir) = env::var_os("SDL3_LIB_DIR") {
        return PathBuf::from(dir);
    }
    match Platform::of(spec) {
        Platform::MacOs => PathBuf::from("/opt/homebrew/lib"),
        Platform::Linux => {
            // Probe the layouts in use rather than assuming one. Debian and
            // Ubuntu use a multiarch directory; Arch, Fedora and most others
            // install straight into /usr/lib. `pkg-config` knows best when the
            // `.pc` file is present, so try it before guessing paths.
            let mut candidates: Vec<PathBuf> = Vec::new();
            if let Some(dir) = pkg_config_libdir("sdl3") {
                candidates.push(dir);
            }
            let multiarch = if spec.triple.starts_with("aarch64") {
                "aarch64-linux-gnu"
            } else if spec.triple.starts_with("armv7") {
                "arm-linux-gnueabihf"
            } else {
                "x86_64-linux-gnu"
            };
            candidates.push(PathBuf::from("/usr/local/lib"));
            candidates.push(PathBuf::from(format!("/usr/lib/{multiarch}")));
            candidates.push(PathBuf::from("/usr/lib64"));
            candidates.push(PathBuf::from("/usr/lib"));
            candidates
                .into_iter()
                .find(|dir| dir.join("libSDL3.so.0").exists())
                // Nothing present: keep the previous default so the error
                // message still names a conventional location.
                .unwrap_or_else(|| PathBuf::from(format!("/usr/lib/{multiarch}")))
        }
        _ => PathBuf::from("sdl3"),
    }
}

/// `pkg-config --variable=libdir <package>`, when the tool and `.pc` file
/// exist. Used to find SDL3 on distributions that place it somewhere other
/// than the Debian multiarch directory.
fn pkg_config_libdir(package: &str) -> Option<PathBuf> {
    let output = Command::new("pkg-config")
        .args(["--variable=libdir", package])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let dir = String::from_utf8(output.stdout).ok()?;
    let dir = PathBuf::from(dir.trim());
    dir.is_absolute().then_some(dir)
}

/// A tool from the pinned LLVM (clang, lld, llvm-otool…).
pub fn llvm_tool(name: &str) -> Result<PathBuf, String> {
    let prefix = env::var_os("LLVM_SYS_231_PREFIX")
        .ok_or("set LLVM_SYS_231_PREFIX to the locked LLVM 23.1.3 installation")?;
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    Ok(PathBuf::from(prefix).join("bin").join(exe))
}

pub fn run(command: &mut Command, what: &str) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// The Linux default used to assume Debian's multiarch directory, which
    /// broke `bundle` on Arch and Fedora where SDL3 lives in `/usr/lib`
    /// (EV-011-14). When SDL3 is installed on this host, the resolver MUST
    /// return the directory that actually contains it — a conventional-looking
    /// path is not good enough, since that is what failed before.
    #[test]
    fn linux_sdl_dir_contains_sdl3_when_installed() {
        let spec = TargetSpec::host();
        if Platform::of(&spec) != Platform::Linux {
            return; // other platforms have their own defaults
        }
        let installed = pkg_config_libdir("sdl3").is_some()
            || ["/usr/lib", "/usr/local/lib", "/usr/lib64"]
                .iter()
                .any(|d| PathBuf::from(d).join("libSDL3.so.0").exists());
        if !installed {
            return; // nothing to find; the fallback is all we can offer
        }
        let dir = default_sdl_dir(&spec);
        assert!(
            dir.join("libSDL3.so.0").exists(),
            "SDL3 is installed but default_sdl_dir returned {dir:?}, which has no libSDL3.so.0"
        );
    }

    /// A cross-compiled runtime lives in `target/<triple>/release`, one level
    /// above `target/release/gba-recomp`. The resolver used to walk up twice,
    /// so `bundle --target aarch64-linux-android` without `--runtime-lib`
    /// reported the library as missing even when it had been built. Every
    /// workflow passes `--runtime-lib`, which is why CI never noticed.
    #[test]
    fn cross_runtime_is_found_one_level_above_the_binary() {
        let base = env::temp_dir().join(format!("gba-rt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let triple = "aarch64-linux-android";
        let spec = TargetSpec::for_triple(triple).unwrap();
        let lib = base
            .join("target")
            .join(triple)
            .join("release")
            .join("libgba_native_rt.a");
        fs::create_dir_all(lib.parent().unwrap()).unwrap();
        fs::write(&lib, b"stub").unwrap();
        // `dir` is where the binary lives, as in a real `target/release`.
        let dir = base.join("target").join("release");
        fs::create_dir_all(&dir).unwrap();

        let candidates = runtime_candidates(&dir, &spec, "libgba_native_rt.a");
        let found = candidates
            .iter()
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("no candidate exists; tried {candidates:?}"));
        assert_eq!(
            fs::canonicalize(found).unwrap(),
            fs::canonicalize(&lib).unwrap(),
            "resolved to {found:?} instead of {lib:?}"
        );
        fs::remove_dir_all(&base).unwrap();
    }

    /// The host target keeps its own lookup order: next to the binary first,
    /// because `cargo build` copies the staticlib there.
    #[test]
    fn host_runtime_is_found_next_to_the_binary() {
        let base = env::temp_dir().join(format!("gba-rt-host-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let dir = base.join("target").join("release");
        fs::create_dir_all(&dir).unwrap();
        let name = Platform::of(&TargetSpec::host()).runtime_lib_name();
        fs::write(dir.join(name), b"stub").unwrap();

        let candidates = runtime_candidates(&dir, &TargetSpec::host(), name);
        assert_eq!(candidates.first(), Some(&dir.join(name)));
        assert!(candidates[0].is_file());
        fs::remove_dir_all(&base).unwrap();
    }

    /// Every candidate path the resolver tries must be absolute, so a wrong
    /// guess can never resolve relative to the current directory.
    #[test]
    fn pkg_config_libdir_is_absolute_or_none() {
        if let Some(dir) = super::pkg_config_libdir("sdl3") {
            assert!(dir.is_absolute(), "{dir:?}");
        }
        // A package that cannot exist must yield None, not a bogus path.
        assert_eq!(super::pkg_config_libdir("no-such-package-xyz"), None);
    }
}

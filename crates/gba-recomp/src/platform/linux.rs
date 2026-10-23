//! Linux adapter (SPEC-068): x86-64, ARM64 (Raspberry Pi 4/5, Orange Pi)
//! and ARMv7. The executable and companions load `libSDL3.so.0` from the
//! package directory (`$ORIGIN`).

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::{PackageInputs, Packaged, run};

pub(super) fn package(i: &PackageInputs<'_>) -> Result<Packaged, String> {
    let sdl_name = "libSDL3.so.0";
    let source = i.sdl_dir.join(sdl_name);
    if !source.exists() {
        return Err(format!(
            "SDL3 not found at {} (pass --sdl-dir or set SDL3_LIB_DIR)",
            source.display()
        ));
    }
    let executable = i.dir.join("game");
    let clang = super::llvm_tool("clang")?;
    let mut command = Command::new(&clang);
    command
        .arg(format!("--target={}", i.spec.triple))
        .arg("-fuse-ld=lld")
        .args(i.objects)
        .arg(i.runtime)
        .arg("-o")
        .arg(&executable)
        .arg("-L")
        .arg(i.sdl_dir)
        .arg("-l:libSDL3.so.0")
        .args(["-lc", "-lm", "-lpthread", "-ldl", "-lgcc_s"])
        .arg("-Wl,-rpath,$ORIGIN");
    // Cross builds pass a target sysroot (glibc, crt files).
    if let Some(sysroot) = env::var_os("GBA_SYSROOT") {
        command.arg("--sysroot").arg(sysroot);
    }
    run(&mut command, "native link (Linux)")?;
    let resolved = fs::canonicalize(&source).map_err(|e| e.to_string())?;
    fs::copy(&resolved, i.dir.join(sdl_name)).map_err(|e| format!("cannot bundle SDL3: {e}"))?;
    let companions = bundle_companions(i.dir)?;
    Ok(Packaged {
        sdl: sdl_name.to_owned(),
        companions,
    })
}

/// Copy companion apps built for the host when the package targets the
/// host; point them at the bundled SDL with `patchelf` when available.
///
/// Companions are picked up next to `gba-recomp`, so a plain
/// `cargo build -p gba-recomp` packages whatever `gba-settings`/`gba-party`
/// were built earlier. A companion older than this executable is stale and
/// is reported loudly: shipping one silently gave a package whose settings
/// window could not switch language (SPEC-069 L10N-69-03).
fn bundle_companions(dir: &Path) -> Result<bool, String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let mut any = false;
    for name in ["gba-settings", "gba-party"] {
        let Some(source) = exe.parent().map(|d| d.join(name)).filter(|p| p.is_file()) else {
            continue;
        };
        warn_if_stale(&exe, &source, name);
        let target = dir.join(name);
        fs::copy(&source, &target).map_err(|e| format!("cannot bundle {name}: {e}"))?;
        let _ = Command::new("patchelf")
            .args(["--set-rpath", "$ORIGIN"])
            .arg(&target)
            .status();
        any = true;
    }
    Ok(any)
}

/// Warn when a companion is older than the executable that packages it.
///
/// Not an error: a companion can legitimately predate the last change if that
/// change did not touch it. But the user must know, because the package looks
/// complete either way.
fn warn_if_stale(exe: &Path, companion: &Path, name: &str) {
    if let Some(gap) = staleness(exe, companion) {
        println!(
            "bundle: warning: {name} is {gap}s older than gba-recomp; it may not match this \
             build. Rebuild it with `cargo build -p {name}` (add `--release` for a release \
             package)."
        );
    }
}

/// How much older `companion` is than `exe`, when that gap is meaningful.
///
/// A single `cargo build` links `gba-recomp` after the companions, so seconds
/// always separate them; only a gap of a minute means the companion was last
/// built by an earlier command.
fn staleness(exe: &Path, companion: &Path) -> Option<u64> {
    const GRACE: std::time::Duration = std::time::Duration::from_secs(60);
    let built = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).ok();
    let (a, b) = (built(exe)?, built(companion)?);
    a.duration_since(b)
        .ok()
        .filter(|gap| *gap > GRACE)
        .map(|gap| gap.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn touch(dir: &Path, name: &str, age_secs: u64) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, b"x").unwrap();
        let when = SystemTime::now() - Duration::from_secs(age_secs);
        let f = fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(when).unwrap();
        p
    }

    /// The bug this guards: `cargo build -p gba-recomp` packages whatever
    /// companions were built earlier, and the package looks complete.
    #[test]
    fn an_old_companion_is_reported_and_a_fresh_one_is_not() {
        let dir = std::env::temp_dir().join(format!("gba-stale-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = touch(&dir, "gba-recomp", 0);
        let old = touch(&dir, "gba-settings", 3600);
        let fresh = touch(&dir, "gba-party", 0);
        assert!(
            staleness(&exe, &old).is_some(),
            "an hour-old companion must be reported"
        );
        assert!(
            staleness(&exe, &fresh).is_none(),
            "a companion from the same build must not be"
        );
        fs::remove_dir_all(&dir).ok();
    }
}

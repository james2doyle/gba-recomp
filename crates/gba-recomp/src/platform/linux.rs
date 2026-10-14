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
        .args(["--target", &i.spec.triple])
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
fn bundle_companions(dir: &Path) -> Result<bool, String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let mut any = false;
    for name in ["gba-settings", "gba-party"] {
        let Some(source) = exe.parent().map(|d| d.join(name)).filter(|p| p.is_file()) else {
            continue;
        };
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

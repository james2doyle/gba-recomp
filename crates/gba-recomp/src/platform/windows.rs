//! Windows adapter (SPEC-068): x86-64 MSVC. Links `game.exe` with clang +
//! lld-link against the runtime `gba_native_rt.lib` and `SDL3.lib`, and
//! ships `SDL3.dll` and the companion `.exe`s next to it. Must run in an
//! MSVC developer environment (vcvars) so the CRT and Windows SDK libraries
//! are found.

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::{PackageInputs, Packaged, run};

/// System libraries required by the Rust standard library and SDL3 import
/// library in a static MSVC link (`rustc --print native-static-libs`).
const SYSTEM_LIBS: &[&str] = &[
    "kernel32.lib",
    "advapi32.lib",
    "ntdll.lib",
    "userenv.lib",
    "ws2_32.lib",
    "bcrypt.lib",
    "dbghelp.lib",
    "synchronization.lib",
    "user32.lib",
    "shell32.lib",
    "ole32.lib",
    "msvcrt.lib",
];

pub(super) fn package(i: &PackageInputs<'_>) -> Result<Packaged, String> {
    let import_lib = i.sdl_dir.join("SDL3.lib");
    let dll = i.sdl_dir.join("SDL3.dll");
    if !import_lib.exists() || !dll.exists() {
        return Err(format!(
            "SDL3.lib/SDL3.dll not found in {} (pass --sdl-dir or set SDL3_LIB_DIR to the SDL3 VC lib/x64 folder)",
            i.sdl_dir.display()
        ));
    }
    let executable = i.dir.join("game.exe");
    let clang = super::llvm_tool("clang")?;
    let mut command = Command::new(&clang);
    command
        .arg(format!("--target={}", i.spec.triple))
        .arg("-fuse-ld=lld")
        .args(i.objects)
        .arg(i.runtime)
        .arg(&import_lib)
        .args(SYSTEM_LIBS)
        .arg("-o")
        .arg(&executable)
        .arg("-Wl,/subsystem:console")
        .arg("-Wl,/nodefaultlib:libcmt");
    run(&mut command, "native link (Windows)")?;
    fs::copy(&dll, i.dir.join("SDL3.dll")).map_err(|e| format!("cannot bundle SDL3.dll: {e}"))?;
    let companions = bundle_companions(i.dir)?;
    Ok(Packaged {
        sdl: "SDL3.dll".to_owned(),
        companions,
    })
}

fn bundle_companions(dir: &Path) -> Result<bool, String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let mut any = false;
    for name in ["gba-settings.exe", "gba-party.exe"] {
        if let Some(source) = exe.parent().map(|d| d.join(name)).filter(|p| p.is_file()) {
            fs::copy(&source, dir.join(name)).map_err(|e| format!("cannot bundle {name}: {e}"))?;
            any = true;
        }
    }
    Ok(any)
}

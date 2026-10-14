//! Links the system SDL3 library (SPEC-061 FE-61-01). No crates.io
//! dependency: the FFI surface is declared by hand in `src/sdl.rs`.
//! `SDL3_LIB_DIR` overrides the library search path.

use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=SDL3_LIB_DIR");
    let dir = env::var("SDL3_LIB_DIR").ok().or_else(|| {
        (env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos"))
            .then(|| "/opt/homebrew/lib".to_owned())
    });
    if let Some(dir) = dir {
        println!("cargo:rustc-link-search=native={dir}");
    }
    println!("cargo:rustc-link-lib=dylib=SDL3");
}

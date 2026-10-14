//! Web (WebAssembly) adapter (SPEC-068 TGT-68-02): `emcc` links the LLVM
//! wasm objects with the runtime built for `wasm32-unknown-emscripten` and
//! the Emscripten SDL3 port into `game.js` + `game.wasm`; `index.html`
//! (embedded template) hosts the canvas. The ROM is always embedded, so the
//! site is static and needs no upload step.
//!
//! The SDL frame loop blocks, so the link uses ASYNCIFY and the runtime
//! yields to the browser with `emscripten_sleep` every frame. Saves and
//! settings live in an IDBFS mount (`/save`, IndexedDB) synchronised by the
//! page.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{PackageInputs, Packaged, run};

const TEMPLATE: &str = include_str!("web/index.html");
const LOGO: &[u8] = include_bytes!("../../../../assets/logo-alpha.png");

/// Files of the published site.
pub const SITE_FILES: [&str; 4] = ["index.html", "game.js", "game.wasm", "logo.png"];

/// Locate `emcc`: `EMCC`, then `PATH`, then `$EMSDK/upstream/emscripten`.
pub(super) fn emcc() -> Result<PathBuf, String> {
    let name = if cfg!(windows) { "emcc.bat" } else { "emcc" };
    if let Some(path) = env::var_os("EMCC") {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(format!("EMCC does not exist: {}", path.display()))
        };
    }
    let on_path = env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    });
    let from_emsdk = || {
        env::var_os("EMSDK")
            .map(|root| PathBuf::from(root).join("upstream").join("emscripten").join(name))
            .filter(|p| p.is_file())
    };
    on_path.or_else(from_emsdk).ok_or_else(|| {
        "emcc (Emscripten) not found: the Web target links with Emscripten; install emsdk \
         (`./emsdk install latest && ./emsdk activate latest && source emsdk_env.sh`) \
         or set EMSDK/EMCC"
            .to_owned()
    })
}

/// Checks done before any work (SPEC-068 TGT-68-01: no partial package).
pub(super) fn preflight(embed: bool) -> Result<(), String> {
    if !embed {
        return Err(
            "the Web target needs the ROM embedded in the site: use `gba-recomp bundle` (or --embed-rom)"
                .to_owned(),
        );
    }
    emcc().map(|_| ())
}

pub(super) fn package(i: &PackageInputs<'_>) -> Result<Packaged, String> {
    preflight(i.embed)?;
    let emcc = emcc()?;
    let mut command = Command::new(&emcc);
    command
        .args(i.objects)
        .arg(i.runtime)
        .arg("-o")
        .arg(i.dir.join("game.js"))
        .args(link_flags());
    run(&mut command, "emcc link (Web)")?;
    for file in ["game.js", "game.wasm"] {
        if !i.dir.join(file).is_file() {
            return Err(format!("emcc did not produce {file}"));
        }
    }
    Ok(Packaged {
        sdl: "SDL3 (Emscripten port, linked into game.wasm)".to_owned(),
        companions: false,
    })
}

/// `emcc` flags of the final link.
pub fn link_flags() -> Vec<&'static str> {
    vec![
        "-O2",
        "--use-port=sdl3",
        // Rust's wasm32-unknown-emscripten std unwinds with wasm exceptions.
        "-fwasm-exceptions",
        // The SDL frame loop blocks; emscripten_sleep yields to the browser.
        "-sASYNCIFY",
        "-sASYNCIFY_STACK_SIZE=65536",
        "-sALLOW_MEMORY_GROWTH=1",
        "-sINITIAL_MEMORY=64MB",
        "-sSTACK_SIZE=4MB",
        "-sENVIRONMENT=web",
        "-sEXPORTED_RUNTIME_METHODS=FS,addRunDependency,removeRunDependency",
        "-lidbfs.js",
    ]
}

/// Title from the ROM header (0xA0..0xAC), HTML-escaped.
fn rom_title(rom: &[u8]) -> String {
    let raw = rom
        .get(0xA0..0xAC)
        .map(|t| {
            String::from_utf8_lossy(t)
                .trim_end_matches('\0')
                .trim()
                .to_owned()
        })
        .unwrap_or_default();
    let title = if raw.is_empty() { "GBA" } else { &raw };
    title
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| match c {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '"' => "&quot;".to_owned(),
            '\'' => "&#39;".to_owned(),
            c => c.to_string(),
        })
        .collect()
}

/// Render `index.html`: title, save name bound to the ROM hash.
pub fn render_index(rom: &[u8], sha_hex: &str) -> String {
    TEMPLATE
        .replace("{{TITLE}}", &rom_title(rom))
        .replace("{{SHA}}", sha_hex)
        .replace("{{SAVE}}", &format!("/save/{}.sav", &sha_hex[..16.min(sha_hex.len())]))
}

pub(super) fn finalize(dir: &Path, rom: &[u8], sha_hex: &str, _embed: bool) -> Result<(), String> {
    fs::write(dir.join("index.html"), render_index(rom, sha_hex))
        .map_err(|e| format!("cannot write index.html: {e}"))?;
    fs::write(dir.join("logo.png"), LOGO).map_err(|e| format!("cannot write logo.png: {e}"))?;
    for file in SITE_FILES {
        if !dir.join(file).is_file() {
            return Err(format!("incomplete Web package: {file} is missing"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_is_rendered_with_escaped_title_and_hash_bound_save() {
        let mut rom = vec![0u8; 0xC0];
        rom[0xA0..0xA7].copy_from_slice(b"A<B>&C\0");
        let sha = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let html = render_index(&rom, sha);
        assert!(html.contains("<title>A&lt;B&gt;&amp;C</title>"));
        assert!(html.contains("/save/0123456789abcdef.sav"));
        assert!(!html.contains("{{"), "every placeholder is replaced");
        assert!(html.contains("game.js"));
        assert!(html.contains("id=\"canvas\""));
    }

    #[test]
    fn link_uses_sdl3_port_asyncify_and_idbfs() {
        let flags = link_flags();
        for f in ["--use-port=sdl3", "-sASYNCIFY", "-sALLOW_MEMORY_GROWTH=1", "-lidbfs.js"] {
            assert!(flags.contains(&f), "{f}");
        }
    }

    #[test]
    fn web_requires_an_embedded_rom() {
        assert!(preflight(false).unwrap_err().contains("bundle"));
    }
}

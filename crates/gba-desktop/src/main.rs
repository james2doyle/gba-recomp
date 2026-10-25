//! Interpreted desktop runner (SPEC-061 FE-61-01).

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use gba_core::BackupKind;
use gba_desktop::{AppOptions, run_app};

fn usage() -> String {
    "usage: gba-desktop --rom <rom.gba> [--save <file.sav>] [--bios <bios.bin>] [--backup KIND] \
     [--settings <settings.toml>] [--frames N] [--view-width 240..=384]\n\
     Esc/F1 or right click: menu  F11: fullscreen  Tab (hold): fast-forward"
        .to_owned()
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<AppOptions, String> {
    let mut args = args.into_iter();
    let mut rom = None;
    let mut o = AppOptions {
        rom: PathBuf::new(),
        rom_bytes: None,
        bios: None,
        save: None,
        backup: None,
        frames: None,
        settings_path: None,
        title: "gba-recomp — interpreted".to_owned(),
        splash: true,
        view_width: None,
    };
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(usage);
        match flag.as_str() {
            "--rom" => rom = Some(PathBuf::from(value()?)),
            "--bios" => o.bios = Some(PathBuf::from(value()?)),
            "--save" => o.save = Some(PathBuf::from(value()?)),
            "--settings" => o.settings_path = Some(PathBuf::from(value()?)),
            "--no-splash" => o.splash = false,
            // SPEC-069 WS-69-18: experimental, per-invocation, never persisted.
            "--view-width" => {
                let w: u16 = value()?
                    .parse()
                    .map_err(|_| "--view-width must be an integer")?;
                if !(240..=384).contains(&w) {
                    return Err(format!(
                        "--view-width {w} outside 240..=384 (experimental wide surface)"
                    ));
                }
                o.view_width = Some(w);
            }
            "--backup" => {
                let v = value()?;
                o.backup = Some(BackupKind::parse(&v).ok_or(format!("unknown backup `{v}`"))?);
            }
            "--frames" => {
                o.frames = Some(
                    value()?
                        .parse()
                        .map_err(|_| "--frames must be an integer")?,
                )
            }
            "-h" | "--help" => return Err(usage()),
            _ => return Err(format!("unknown argument `{flag}`\n{}", usage())),
        }
    }
    o.rom = rom.ok_or_else(usage)?;
    Ok(o)
}

fn main() -> ExitCode {
    let result = parse_args(env::args().skip(1)).and_then(|o| run_app(&o, None));
    match result {
        Ok(r) => {
            eprintln!(
                "gba-desktop: frames={} presented={} audio={} underruns={} overruns={} saves={} view_width={}",
                r.frames,
                r.presented,
                if r.audio_on { "on" } else { "off" },
                r.audio_underruns,
                r.audio_overruns,
                r.saves,
                r.view_width
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("gba-desktop: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_flags() {
        let o = parse_args(args(&[
            "--rom",
            "r.gba",
            "--frames",
            "10",
            "--settings",
            "s.toml",
        ]))
        .unwrap();
        assert_eq!(o.frames, Some(10));
        assert_eq!(o.settings_path, Some(PathBuf::from("s.toml")));
        assert!(parse_args(args(&["--frames", "10"])).is_err());
        assert!(parse_args(args(&["--rom", "r", "--bogus"])).is_err());
    }
}

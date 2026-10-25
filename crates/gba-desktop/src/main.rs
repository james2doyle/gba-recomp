//! Interpreted desktop runner (SPEC-061 FE-61-01).

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use gba_core::BackupKind;
use gba_desktop::{AppOptions, run_app};

fn usage() -> String {
    gba_i18n::tr!("cli.usage")
}

/// Picks the CLI language from the environment before SDL is initialised,
/// where `$LANG` is all there is (SPEC-069 L10N-69-04).
fn detect_cli_language() {
    let tags = gba_i18n::posix_locale_vars();
    if let Some(lang) = gba_i18n::detect_from(&tags) {
        gba_i18n::set_lang(lang);
    }
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
    };
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(usage);
        match flag.as_str() {
            "--rom" => rom = Some(PathBuf::from(value()?)),
            "--bios" => o.bios = Some(PathBuf::from(value()?)),
            "--save" => o.save = Some(PathBuf::from(value()?)),
            "--settings" => o.settings_path = Some(PathBuf::from(value()?)),
            "--no-splash" => o.splash = false,
            "--backup" => {
                let v = value()?;
                o.backup = Some(
                    BackupKind::parse(&v).ok_or(gba_i18n::tr!("cli.unknown_backup", name = v))?,
                );
            }
            "--frames" => {
                o.frames = Some(
                    value()?
                        .parse()
                        .map_err(|_| gba_i18n::tr!("cli.frames_integer"))?,
                )
            }
            "-h" | "--help" => return Err(usage()),
            _ => {
                return Err(format!(
                    "{}\n{}",
                    gba_i18n::tr!("cli.unknown_argument", flag = flag),
                    usage()
                ));
            }
        }
    }
    o.rom = rom.ok_or_else(usage)?;
    Ok(o)
}

fn main() -> ExitCode {
    detect_cli_language();
    let result = parse_args(env::args().skip(1)).and_then(|o| run_app(&o, None));
    match result {
        Ok(r) => {
            eprintln!(
                "gba-desktop: frames={} presented={} audio={} underruns={} overruns={} saves={}",
                r.frames,
                r.presented,
                if r.audio_on { "on" } else { "off" },
                r.audio_underruns,
                r.audio_overruns,
                r.saves
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

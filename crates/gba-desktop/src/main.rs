//! Playable desktop frontend for the interpreted core (SPEC-061 FE-61-01).
//!
//! Host concerns only: SDL3 window/renderer, audio stream, keyboard and
//! gamepad input, guest-rate pacing and atomic save persistence. Host
//! preferences (scale, fast-forward, audio availability) never change the
//! simulation (VID-03/04, AUD-02/03).

#![deny(unsafe_code)]

mod sdl;

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use gba_core::{BackupKind, Gba, GbaConfig, SCREEN_HEIGHT, SCREEN_WIDTH, keys};
use sdl::{Event, button, scancode};

/// Guest clock and frame length: 16777216 Hz / 280896 cycles ≈ 59.7275 fps.
const GUEST_HZ: u64 = 16_777_216;
const CYCLES_PER_FRAME: u64 = 280_896;
const AUDIO_HZ: i32 = 32_768;
/// Bound on queued host audio (~100 ms of stereo S16).
const AUDIO_MAX_QUEUED: usize = (AUDIO_HZ as usize) * 2 * 2 / 10;
const SAVE_INTERVAL: Duration = Duration::from_secs(10);
/// Frames the host may fall behind before pacing resynchronises.
const MAX_LAG_FRAMES: u32 = 5;

#[derive(Debug, PartialEq)]
struct Options {
    rom: PathBuf,
    bios: Option<PathBuf>,
    save: Option<PathBuf>,
    backup: Option<BackupKind>,
    scale: i32,
    frames: Option<u64>,
}

fn usage() -> String {
    "usage: gba-desktop --rom <rom.gba> [--save <file.sav>] [--bios <bios.bin>] [--backup KIND] \
     [--scale N] [--frames N]\n\
     keys: arrows=D-pad Z=A X=B Enter=Start Backspace/RShift=Select A=L S=R \
     Tab(hold)=fast-forward Esc=quit"
        .to_owned()
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut rom = None;
    let mut o = Options {
        rom: PathBuf::new(),
        bios: None,
        save: None,
        backup: None,
        scale: 3,
        frames: None,
    };
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(usage);
        match flag.as_str() {
            "--rom" => rom = Some(PathBuf::from(value()?)),
            "--bios" => o.bios = Some(PathBuf::from(value()?)),
            "--save" => o.save = Some(PathBuf::from(value()?)),
            "--backup" => {
                let v = value()?;
                o.backup = Some(BackupKind::parse(&v).ok_or(format!("unknown backup `{v}`"))?);
            }
            "--scale" => {
                o.scale = value()?
                    .parse()
                    .ok()
                    .filter(|s| (1..=16).contains(s))
                    .ok_or("--scale must be an integer in 1..=16")?;
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

/// Converts a BGR555 guest pixel to host XRGB8888 (presentation only).
fn bgr555_to_xrgb(c: u16) -> u32 {
    let expand = |v: u16| {
        let v = u32::from(v & 0x1F);
        (v << 3) | (v >> 2)
    };
    0xFF00_0000 | (expand(c) << 16) | (expand(c >> 5) << 8) | expand(c >> 10)
}

const KEYBOARD_MAP: &[(usize, u16)] = &[
    (scancode::UP, keys::UP),
    (scancode::DOWN, keys::DOWN),
    (scancode::LEFT, keys::LEFT),
    (scancode::RIGHT, keys::RIGHT),
    (scancode::Z, keys::A),
    (scancode::X, keys::B),
    (scancode::RETURN, keys::START),
    (scancode::BACKSPACE, keys::SELECT),
    (scancode::RSHIFT, keys::SELECT),
    (scancode::A, keys::L),
    (scancode::S, keys::R),
];

const GAMEPAD_MAP: &[(i32, u16)] = &[
    (button::SOUTH, keys::A),
    (button::EAST, keys::B),
    (button::START, keys::START),
    (button::BACK, keys::SELECT),
    (button::LEFT_SHOULDER, keys::L),
    (button::RIGHT_SHOULDER, keys::R),
    (button::DPAD_UP, keys::UP),
    (button::DPAD_DOWN, keys::DOWN),
    (button::DPAD_LEFT, keys::LEFT),
    (button::DPAD_RIGHT, keys::RIGHT),
];

fn key_mask(pressed: impl Fn(usize) -> bool) -> u16 {
    KEYBOARD_MAP
        .iter()
        .filter(|(sc, _)| pressed(*sc))
        .fold(0, |m, (_, k)| m | k)
}

fn pad_mask(pressed: impl Fn(i32) -> bool) -> u16 {
    GAMEPAD_MAP
        .iter()
        .filter(|(b, _)| pressed(*b))
        .fold(0, |m, (_, k)| m | k)
}

/// Writes `data` to `path` atomically (temporary file + fsync + rename).
fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let mut tmp_name = path.file_name().unwrap_or_default().to_owned();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    let mut f =
        fs::File::create(&tmp).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    f.write_all(data)
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    drop(f);
    fs::rename(&tmp, path).map_err(|e| format!("cannot commit {}: {e}", path.display()))
}

/// Persists the backup only when dirty and clears the flag on success.
fn persist_save(gba: &mut Gba, path: Option<&Path>) -> Result<bool, String> {
    let Some(path) = path else { return Ok(false) };
    if !gba.bus.backup.dirty {
        return Ok(false);
    }
    write_atomic(path, gba.bus.backup.data())?;
    gba.bus.backup.dirty = false;
    Ok(true)
}

#[derive(Default)]
struct Stats {
    frames: u64,
    presented: u64,
    audio_underruns: u64,
    audio_overruns: u64,
    saves: u64,
}

fn run() -> Result<(), String> {
    let o = parse_args(env::args().skip(1))?;
    let rom = fs::read(&o.rom).map_err(|e| format!("cannot read ROM: {e}"))?;
    let bios = match &o.bios {
        Some(p) => Some(fs::read(p).map_err(|e| format!("cannot read BIOS: {e}"))?),
        None => None,
    };
    let save = match &o.save {
        Some(p) if p.exists() => Some(fs::read(p).map_err(|e| format!("cannot read save: {e}"))?),
        _ => None,
    };
    let mut gba = Gba::new(GbaConfig {
        rom: &rom,
        bios: bios.as_deref(),
        backup: o.backup,
        save: save.as_deref(),
    });

    // Declaration order matters: owners drop before `ctx` (SDL_Quit).
    let ctx = sdl::Context::init()?;
    let (w, h) = (SCREEN_WIDTH as i32, SCREEN_HEIGHT as i32);
    let mut video = ctx.create_video("gba-desktop", w, h, o.scale)?;
    let mut audio = match ctx.open_audio(AUDIO_HZ) {
        Ok(a) => Some(a),
        Err(e) => {
            eprintln!("gba-desktop: audio disabled ({e}); continuing without sound");
            None
        }
    };
    let mut pads: Vec<sdl::Gamepad<'_>> = Vec::new();

    let frame_time = Duration::from_nanos(CYCLES_PER_FRAME * 1_000_000_000 / GUEST_HZ);
    let mut stats = Stats::default();
    let mut pixels = vec![0u32; SCREEN_WIDTH * SCREEN_HEIGHT];
    let mut deadline = Instant::now();
    let mut last_present = Instant::now() - frame_time;
    let mut last_save = Instant::now();
    let mut fault = None;

    'main: loop {
        if o.frames.is_some_and(|n| stats.frames >= n) {
            break;
        }
        while let Some(ev) = ctx.poll_event() {
            match ev {
                Event::Quit => break 'main,
                Event::GamepadAdded(id) => {
                    if !pads.iter().any(|p| p.id() == id) {
                        match ctx.open_gamepad(id) {
                            Ok(p) => pads.push(p),
                            Err(e) => eprintln!("gba-desktop: {e}"),
                        }
                    }
                }
                Event::GamepadRemoved(id) => pads.retain(|p| p.id() != id),
                Event::Other => {}
            }
        }
        if ctx.key_down(scancode::ESCAPE) {
            break;
        }
        let fast_forward = ctx.key_down(scancode::TAB);
        let mask = key_mask(|sc| ctx.key_down(sc))
            | pads.iter().fold(0, |m, p| m | pad_mask(|b| p.button(b)));
        gba.set_keys(mask);

        if let Err(f) = gba.run_frame() {
            fault = Some(f);
            break;
        }
        stats.frames += 1;

        let samples = gba.take_audio();
        if let Some(a) = audio.as_mut()
            && !fast_forward
        {
            let queued = a.queued_bytes();
            if queued == 0 && stats.frames > 1 {
                stats.audio_underruns += 1;
            }
            if queued > AUDIO_MAX_QUEUED {
                stats.audio_overruns += 1; // drop this frame's samples
            } else if let Err(e) = a.put(&samples) {
                eprintln!("gba-desktop: audio disabled ({e})");
                audio = None;
            }
        }

        let now = Instant::now();
        if !fast_forward || now.duration_since(last_present) >= frame_time {
            for (dst, &src) in pixels.iter_mut().zip(gba.framebuffer()) {
                *dst = bgr555_to_xrgb(src);
            }
            video.present(&pixels)?;
            stats.presented += 1;
            last_present = now;
        }

        if last_save.elapsed() >= SAVE_INTERVAL {
            last_save = Instant::now();
            match persist_save(&mut gba, o.save.as_deref()) {
                Ok(true) => stats.saves += 1,
                Ok(false) => {}
                Err(e) => eprintln!("gba-desktop: {e}"),
            }
        }

        // Pacing by guest time on a monotonic clock, never by vsync.
        let now = Instant::now();
        if fast_forward {
            deadline = now;
        } else {
            deadline += frame_time;
            if deadline > now {
                thread::sleep(deadline - now);
            } else if now - deadline > frame_time * MAX_LAG_FRAMES {
                deadline = now;
            }
        }
    }

    let save_result = persist_save(&mut gba, o.save.as_deref());
    if let Ok(true) = save_result {
        stats.saves += 1;
    }
    eprintln!(
        "gba-desktop: frames={} presented={} audio={} underruns={} overruns={} saves={}",
        stats.frames,
        stats.presented,
        if audio.is_some() { "on" } else { "off" },
        stats.audio_underruns,
        stats.audio_overruns,
        stats.saves,
    );
    drop(pads);
    drop(audio);
    drop(video);
    drop(ctx);
    save_result?;
    if let Some(f) = fault {
        return Err(format!(
            "guest fault at frame {}: {f:?} (pc=0x{:08X})",
            stats.frames + 1,
            gba.cpu.pc()
        ));
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
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
    fn parses_defaults_and_flags() {
        let o = parse_args(args(&["--rom", "r.gba"])).unwrap();
        assert_eq!(o.scale, 3);
        assert_eq!(o.frames, None);
        let o = parse_args(args(&["--rom", "r.gba", "--scale", "5", "--frames", "10"])).unwrap();
        assert_eq!((o.scale, o.frames), (5, Some(10)));
        assert!(parse_args(args(&["--scale", "0", "--rom", "r"])).is_err());
        assert!(parse_args(args(&["--frames", "10"])).is_err());
        assert!(parse_args(args(&["--rom", "r", "--bogus"])).is_err());
    }

    #[test]
    fn converts_bgr555_extremes() {
        assert_eq!(bgr555_to_xrgb(0x0000), 0xFF00_0000);
        assert_eq!(bgr555_to_xrgb(0x7FFF), 0xFFFF_FFFF);
        assert_eq!(bgr555_to_xrgb(0x001F), 0xFFFF_0000); // red
        assert_eq!(bgr555_to_xrgb(0x03E0), 0xFF00_FF00); // green
        assert_eq!(bgr555_to_xrgb(0x7C00), 0xFF00_00FF); // blue
    }

    #[test]
    fn maps_keyboard_and_gamepad() {
        assert_eq!(key_mask(|sc| sc == scancode::Z), keys::A);
        assert_eq!(
            key_mask(|sc| sc == scancode::RSHIFT || sc == scancode::UP),
            keys::SELECT | keys::UP
        );
        assert_eq!(pad_mask(|b| b == button::EAST), keys::B);
        assert_eq!(pad_mask(|_| true), 0x03FF);
    }

    #[test]
    fn guest_frame_rate() {
        let fps = GUEST_HZ as f64 / CYCLES_PER_FRAME as f64;
        assert!((fps - 59.7275).abs() < 1e-3);
    }

    #[test]
    fn atomic_write_replaces_file() {
        let dir = env::temp_dir().join(format!("gba-desktop-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("game.sav");
        fs::write(&path, b"old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert!(!dir.join("game.sav.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}

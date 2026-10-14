//! Headless runner for the interpreted core (SPEC-061): runs a ROM for a
//! fixed number of frames with scripted input and reports a JSON summary.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use gba_core::{BackupKind, Gba, GbaConfig, SCREEN_HEIGHT, SCREEN_WIDTH, keys};

struct Options {
    rom: PathBuf,
    bios: Option<PathBuf>,
    save: Option<PathBuf>,
    backup: Option<BackupKind>,
    frames: u64,
    input: Vec<(u64, u64, u16)>,
    screenshot: Option<PathBuf>,
    screenshot_every: Option<u64>,
    trace_hashes: bool,
}

fn usage() -> String {
    "usage: gba-run --rom <rom.gba> [--bios <bios.bin>] [--save <file.sav>] [--backup KIND] \
     [--frames N] [--input FRAME:LEN:KEYS ...] [--ppm out.ppm] [--ppm-every N] [--hashes]\n\
     KEYS: comma-separated A,B,SELECT,START,RIGHT,LEFT,UP,DOWN,R,L"
        .to_owned()
}

fn parse_keys(text: &str) -> Result<u16, String> {
    let mut mask = 0;
    for k in text.split(',').filter(|k| !k.is_empty()) {
        mask |= match k.to_ascii_uppercase().as_str() {
            "A" => keys::A,
            "B" => keys::B,
            "SELECT" => keys::SELECT,
            "START" => keys::START,
            "RIGHT" => keys::RIGHT,
            "LEFT" => keys::LEFT,
            "UP" => keys::UP,
            "DOWN" => keys::DOWN,
            "R" => keys::R,
            "L" => keys::L,
            other => return Err(format!("unknown key `{other}`")),
        };
    }
    Ok(mask)
}

fn parse() -> Result<Options, String> {
    let mut args = env::args().skip(1);
    let mut o = Options {
        rom: PathBuf::new(),
        bios: None,
        save: None,
        backup: None,
        frames: 60,
        input: Vec::new(),
        screenshot: None,
        screenshot_every: None,
        trace_hashes: false,
    };
    let mut rom = None;
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
            "--frames" => {
                o.frames = value()?
                    .parse()
                    .map_err(|_| "--frames must be an integer")?
            }
            "--input" => {
                let v = value()?;
                let parts: Vec<&str> = v.splitn(3, ':').collect();
                if parts.len() != 3 {
                    return Err(format!("bad --input `{v}`"));
                }
                let start: u64 = parts[0].parse().map_err(|_| "bad input frame")?;
                let len: u64 = parts[1].parse().map_err(|_| "bad input length")?;
                o.input.push((start, len, parse_keys(parts[2])?));
            }
            "--ppm" => o.screenshot = Some(PathBuf::from(value()?)),
            "--ppm-every" => {
                o.screenshot_every = Some(value()?.parse().map_err(|_| "bad --ppm-every")?)
            }
            "--hashes" => o.trace_hashes = true,
            _ => return Err(format!("unknown argument `{flag}`\n{}", usage())),
        }
    }
    o.rom = rom.ok_or_else(usage)?;
    Ok(o)
}

fn write_ppm(path: &PathBuf, gba: &Gba) -> Result<(), String> {
    let mut data = format!("P6\n{SCREEN_WIDTH} {SCREEN_HEIGHT}\n255\n").into_bytes();
    for &c in gba.framebuffer() {
        for shift in [0, 5, 10] {
            let v = ((c >> shift) & 0x1F) as u8;
            data.push((v << 3) | (v >> 2));
        }
    }
    fs::write(path, data).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn run() -> Result<(), String> {
    let o = parse()?;
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
    let started = Instant::now();
    let mut fault = None;
    let mut frame = 0;
    while frame < o.frames {
        let pressed = o
            .input
            .iter()
            .filter(|(s, l, _)| frame >= *s && frame < s + l)
            .fold(0, |m, (_, _, k)| m | k);
        gba.set_keys(pressed);
        if let Err(f) = gba.run_frame() {
            fault = Some(f);
            break;
        }
        gba.take_audio();
        frame += 1;
        if o.trace_hashes {
            eprintln!("frame {frame} hash {:016x}", gba.frame_hash());
        }
        if let (Some(every), Some(path)) = (o.screenshot_every, &o.screenshot)
            && frame % every == 0
        {
            let p = path.with_extension(format!("{frame}.ppm"));
            write_ppm(&p, &gba)?;
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    if let Some(path) = &o.screenshot {
        write_ppm(path, &gba)?;
    }
    if let Some(path) = &o.save
        && gba.bus.backup.dirty
    {
        let tmp = path.with_extension("sav.tmp");
        fs::write(&tmp, gba.bus.backup.data()).map_err(|e| format!("cannot write save: {e}"))?;
        fs::rename(&tmp, path).map_err(|e| format!("cannot commit save: {e}"))?;
    }
    let unsupported: Vec<String> = gba
        .bus
        .unsupported_io
        .iter()
        .map(|(a, n)| format!("\"0x{:03X}\":{n}", a))
        .collect();
    let swis: Vec<String> = gba
        .bus
        .hle
        .calls
        .iter()
        .map(|(n, c)| format!("\"0x{n:02X}\":{c}"))
        .collect();
    println!(
        "{{\"mode\":\"interpreted\",\"frames\":{frame},\"instructions_interpreted\":{},\"guest_cycles\":{},\"frame_hash\":\"{:016x}\",\"pc\":\"0x{:08X}\",\"cpsr\":\"0x{:08X}\",\"backup\":\"{}\",\"swi_calls\":{{{}}},\"unsupported_io\":{{{}}},\"fault\":{},\"host_seconds\":{:.3},\"fps\":{:.1}}}",
        gba.instructions,
        gba.bus.cycles,
        gba.frame_hash(),
        gba.cpu.pc(),
        gba.cpu.cpsr,
        gba.bus.backup.kind().as_str(),
        swis.join(","),
        unsupported.join(","),
        fault
            .as_ref()
            .map_or("null".to_owned(), |f| format!("\"{f:?}\"")),
        elapsed,
        frame as f64 / elapsed.max(1e-9),
    );
    if fault.is_some() {
        return Err("guest stopped with a fault".to_owned());
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gba-run: {e}");
            ExitCode::FAILURE
        }
    }
}

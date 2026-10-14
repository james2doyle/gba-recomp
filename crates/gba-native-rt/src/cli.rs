//! Command line of an `aot-compatible` package (NB-62-07/08/09).

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use gba_core::{BackupKind, Gba, GbaConfig, SCREEN_HEIGHT, SCREEN_WIDTH, keys};
use sha2::{Digest, Sha256};

use crate::{AotEntry, AotGuard, NativeProgram};

struct Options {
    rom: Option<PathBuf>,
    bios: Option<PathBuf>,
    save: Option<PathBuf>,
    backup: Option<BackupKind>,
    frames: u64,
    input: Vec<(u64, u64, u16)>,
    ppm: Option<PathBuf>,
    ppm_every: Option<u64>,
    interpret_only: bool,
    verify: bool,
    hashes: bool,
    headless: bool,
    no_splash: bool,
    settings: Option<PathBuf>,
    frames_given: bool,
}

const USAGE: &str = "usage: <package>/game --rom <rom.gba> [--save F] [--settings S] [--backup KIND] [--bios B]\n\
  window (default): Esc/F1 or right click = menu, F11 = fullscreen, Tab = fast-forward\n\
  --headless [--frames N] [--input FRAME:LEN:KEYS]... [--ppm P] [--ppm-every N] [--hashes] \
[--interpret-only] [--verify-interpreter]";

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

fn parse(args: &[String]) -> Result<Options, String> {
    let mut it = args.iter().skip(1);
    let mut o = Options {
        rom: None,
        bios: None,
        save: None,
        backup: None,
        frames: 60,
        input: Vec::new(),
        ppm: None,
        ppm_every: None,
        interpret_only: false,
        verify: false,
        hashes: false,
        headless: false,
        no_splash: false,
        settings: None,
        frames_given: false,
    };
    let mut rom = None;
    while let Some(flag) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| USAGE.to_owned());
        match flag.as_str() {
            "--rom" => rom = Some(PathBuf::from(value()?)),
            "--bios" => o.bios = Some(PathBuf::from(value()?)),
            "--save" => o.save = Some(PathBuf::from(value()?)),
            "--backup" => {
                let v = value()?;
                o.backup = Some(BackupKind::parse(&v).ok_or(format!("unknown backup `{v}`"))?);
            }
            "--frames" => {
                o.frames = value()?.parse().map_err(|_| "bad --frames")?;
                o.frames_given = true;
            }
            "--headless" => o.headless = true,
            "--no-splash" => o.no_splash = true,
            "--settings" => o.settings = Some(PathBuf::from(value()?)),
            "--input" => {
                let v = value()?;
                let parts: Vec<&str> = v.splitn(3, ':').collect();
                if parts.len() != 3 {
                    return Err(format!("bad --input `{v}`"));
                }
                o.input.push((
                    parts[0].parse().map_err(|_| "bad input frame")?,
                    parts[1].parse().map_err(|_| "bad input length")?,
                    parse_keys(parts[2])?,
                ));
            }
            "--ppm" => o.ppm = Some(PathBuf::from(value()?)),
            "--ppm-every" => o.ppm_every = Some(value()?.parse().map_err(|_| "bad --ppm-every")?),
            "--interpret-only" => o.interpret_only = true,
            "--verify-interpreter" => o.verify = true,
            "--hashes" => o.hashes = true,
            "--help" | "-h" => return Err(USAGE.to_owned()),
            _ => return Err(format!("unknown argument `{flag}`\n{USAGE}")),
        }
    }
    o.rom = rom;
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

pub fn run(
    args: &[String],
    entries: &[AotEntry],
    guards: &[AotGuard],
    embedded: Option<&'static [u8]>,
    sha256: [u8; 32],
) -> Result<(), String> {
    let o = parse(args)?;
    // SPEC-066: a bundled package carries its ROM; `--rom` still overrides.
    let rom = match (&o.rom, embedded) {
        (Some(path), _) => fs::read(path).map_err(|e| format!("cannot read ROM: {e}"))?,
        (None, Some(bytes)) => bytes.to_vec(),
        (None, None) => {
            return Err(format!(
                "this package needs the game ROM: --rom <file.gba>\n{USAGE}"
            ));
        }
    };
    let digest: [u8; 32] = Sha256::digest(&rom).into();
    if digest != sha256 {
        return Err(format!(
            "ROM SHA-256 {} does not match the package ({}); native code is bound to one ROM",
            hex(&digest),
            hex(&sha256)
        ));
    }
    if !o.headless {
        return run_window(&o, entries, guards, rom, sha256);
    }
    let bios = match &o.bios {
        Some(p) => Some(fs::read(p).map_err(|e| format!("cannot read BIOS: {e}"))?),
        None => None,
    };
    let save = match &o.save {
        Some(p) if p.exists() => Some(fs::read(p).map_err(|e| format!("cannot read save: {e}"))?),
        _ => None,
    };
    let make = || {
        Gba::new(GbaConfig {
            rom: &rom,
            bios: bios.as_deref(),
            backup: o.backup,
            save: save.as_deref(),
        })
    };
    let mut gba = make();
    let mut reference = if o.verify { Some(make()) } else { None };
    let mut program = NativeProgram::new(entries, guards);
    let started = Instant::now();
    let mut fault = None;
    let mut divergence = None;
    let mut frame = 0;
    while frame < o.frames {
        let pressed = o
            .input
            .iter()
            .filter(|(s, l, _)| frame >= *s && frame < s + l)
            .fold(0, |m, (_, _, k)| m | k);
        gba.set_keys(pressed);
        let result = if o.interpret_only {
            gba.run_frame()
        } else {
            gba.run_frame_with(&mut program)
        };
        if let Err(f) = result {
            fault = Some(f);
            break;
        }
        gba.take_audio();
        frame += 1;
        if let Some(r) = reference.as_mut() {
            r.set_keys(pressed);
            if let Err(f) = r.run_frame() {
                divergence = Some(format!("reference faulted at frame {frame}: {f:?}"));
                break;
            }
            r.take_audio();
            if r.bus.cycles != gba.bus.cycles
                || r.cpu.r != gba.cpu.r
                || r.cpu.cpsr != gba.cpu.cpsr
                || r.frame_hash() != gba.frame_hash()
            {
                divergence = Some(format!(
                    "frame {frame}: native cycles={} pc=0x{:08X} hash={:016x} vs interpreter cycles={} pc=0x{:08X} hash={:016x}",
                    gba.bus.cycles,
                    gba.cpu.pc(),
                    gba.frame_hash(),
                    r.bus.cycles,
                    r.cpu.pc(),
                    r.frame_hash()
                ));
                break;
            }
        }
        if o.hashes {
            eprintln!("frame {frame} hash {:016x}", gba.frame_hash());
        }
        if let (Some(every), Some(path)) = (o.ppm_every, &o.ppm)
            && frame % every == 0
        {
            write_ppm(&path.with_extension(format!("{frame}.ppm")), &gba)?;
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    if let Some(path) = &o.ppm {
        write_ppm(path, &gba)?;
    }
    if let Some(path) = &o.save
        && gba.bus.backup.dirty
    {
        let tmp = path.with_extension("sav.tmp");
        fs::write(&tmp, gba.bus.backup.data()).map_err(|e| format!("cannot write save: {e}"))?;
        fs::rename(&tmp, path).map_err(|e| format!("cannot commit save: {e}"))?;
    }
    let c = &program.counters;
    let total = c.retired + gba.instructions;
    println!(
        "{{\"execution_mode\":\"aot-compatible\",\"native_abi\":{},\"core\":{},\"hle_bios\":{},\"entries\":{},\"frames\":{frame},\"instructions_native\":{},\"instructions_interpreted\":{},\"native_ratio\":{:.6},\"dispatcher_misses\":{},\"shells\":{},\"ram_guards\":{},\"guard_compares\":{},\"guard_failures\":{},\"exits_seq\":{},\"exits_branch\":{},\"exits_interpret\":{},\"guest_cycles\":{},\"frame_hash\":\"{:016x}\",\"verified_against_interpreter\":{},\"divergence\":{},\"fault\":{},\"host_seconds\":{:.3},\"fps\":{:.1}}}",
        crate::NATIVE_ABI_VERSION,
        gba_core::CORE_VERSION,
        gba_core::bios::HLE_VERSION,
        program.len(),
        c.retired,
        gba.instructions,
        if total == 0 {
            0.0
        } else {
            c.retired as f64 / total as f64
        },
        gba.native.misses,
        gba.native.shells,
        program.guard_count(),
        c.guard_compares,
        c.guard_failures,
        c.exits_seq,
        c.exits_branch,
        c.exits_interpret,
        gba.bus.cycles,
        gba.frame_hash(),
        o.verify && divergence.is_none(),
        divergence
            .as_ref()
            .map_or("null".to_owned(), |d| format!("\"{d}\"")),
        fault
            .as_ref()
            .map_or("null".to_owned(), |f| format!("\"{f:?}\"")),
        elapsed,
        frame as f64 / elapsed.max(1e-9),
    );
    if divergence.is_some() {
        return Err("native execution diverged from the interpreter".to_owned());
    }
    if fault.is_some() {
        return Err("guest stopped with a fault".to_owned());
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Playable window (SPEC-063 FR-63-02): same native program, SDL frontend.
fn run_window(
    o: &Options,
    entries: &[AotEntry],
    guards: &[AotGuard],
    rom: Vec<u8>,
    _sha256: [u8; 32],
) -> Result<(), String> {
    let mut program = NativeProgram::new(entries, guards);
    let header_title: String = Some(&rom)
        .and_then(|r| {
            r.get(0xA0..0xAC).map(|t| {
                String::from_utf8_lossy(t)
                    .trim_end_matches('\0')
                    .trim()
                    .to_owned()
            })
        })
        .unwrap_or_default();
    let options = gba_desktop::AppOptions {
        rom: o.rom.clone().unwrap_or_default(),
        rom_bytes: Some(rom),
        bios: o.bios.clone(),
        save: o.save.clone(),
        backup: o.backup,
        frames: o.frames_given.then_some(o.frames),
        settings_path: o.settings.clone(),
        title: format!("{header_title} — gba-recomp aot-compatible (nativo)"),
        splash: !o.no_splash,
    };
    let r = gba_desktop::run_app(&options, Some(&mut program))?;
    let c = &program.counters;
    let total = c.retired + r.interpreted_instructions;
    eprintln!(
        "{{\"execution_mode\":\"aot-compatible\",\"frontend\":\"sdl3\",\"frames\":{},\"instructions_native\":{},\"instructions_interpreted\":{},\"native_ratio\":{:.6},\"dispatcher_misses\":{},\"shells\":{},\"ram_guards\":{},\"guard_compares\":{},\"guard_failures\":{},\"audio\":{},\"audio_underruns\":{},\"audio_overruns\":{},\"saves\":{},\"guest_cycles\":{},\"frame_hash\":\"{:016x}\"}}",
        r.frames,
        c.retired,
        r.interpreted_instructions,
        if total == 0 {
            0.0
        } else {
            c.retired as f64 / total as f64
        },
        r.native_misses,
        r.native_shells,
        program.guard_count(),
        c.guard_compares,
        c.guard_failures,
        r.audio_on,
        r.audio_underruns,
        r.audio_overruns,
        r.saves,
        r.guest_cycles,
        r.frame_hash,
    );
    Ok(())
}

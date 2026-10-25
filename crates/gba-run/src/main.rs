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
    /// SPEC-069 WS-69-18: experimental session override, never persisted.
    /// `--ppm` output stays canonical (WS-69-01).
    view_width: Option<u16>,
    /// SPEC-070 DIAG-70-01: classify the margin of the last frame.
    margin_report: bool,
}

fn usage() -> String {
    "usage: gba-run --rom <rom.gba> [--bios <bios.bin>] [--save <file.sav>] [--backup KIND] \
     [--frames N] [--input FRAME:LEN:KEYS ...] [--ppm out.ppm] [--ppm-every N] [--hashes] \\
     [--view-width 240..=384] [--margin-report]\n\
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
        view_width: None,
        margin_report: false,
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
            "--margin-report" => o.margin_report = true,
            "--view-width" => {
                let w: u16 = value()?
                    .parse()
                    .map_err(|_| "--view-width must be an integer")?;
                if !(240..=384).contains(&w) {
                    return Err(format!("--view-width {w} outside 240..=384"));
                }
                o.view_width = Some(w);
            }
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

/// SPEC-070 DIAG-70-01..05: inventory of the layers and a structural verdict
/// on what the margin actually contains. Read-only: it inspects the state of
/// the last rendered frame and never changes it.
fn print_margin_report(gba: &Gba) {
    use gba_core::ppu::MarginKind;
    let io = &gba.bus.io;
    let r = |o: usize| u16::from_le_bytes([io[o], io[o + 1]]);
    let dispcnt = r(0);
    let mode = dispcnt & 7;
    let view = gba.view();
    println!("margin-report:");
    println!(
        "  view: width={} left={} right={} (canonical 240 columns occupy output {}..{})",
        view.width(),
        view.left,
        view.right,
        usize::from(view.left),
        usize::from(view.left) + 240
    );
    println!("  display: mode={mode} dispcnt=0x{dispcnt:04X}");
    for bg in 0..4 {
        let cnt = r(0x08 + bg * 2);
        let on = dispcnt & (0x100 << bg) != 0;
        let kind = match (mode, bg) {
            (0, _) | (1, 0) | (1, 1) => "text",
            (1, 2) | (2, _) => "affine",
            (3..=5, 2) => "bitmap",
            _ => "unused",
        };
        let map = gba.bg_map_width(bg);
        println!(
            "  BG{bg}: enabled={on} kind={kind} cnt=0x{cnt:04X} map_width={} \
             char_block={} screen_block={}",
            map.map_or_else(|| "-".to_owned(), |w| format!("{w}")),
            (cnt >> 2) & 3,
            (cnt >> 8) & 0x1F
        );
        if matches!(kind, "affine") && on {
            let base = 0x20 + (bg - 2) * 0x10;
            let p = |o: usize| i32::from(r(o) as i16);
            println!(
                "    affine: PA={} PB={} PC={} PD={} ref=({},{})",
                p(base),
                p(base + 2),
                p(base + 4),
                p(base + 6),
                p(0x28 + (bg - 2) * 0x10),
                p(0x2C + (bg - 2) * 0x10)
            );
        }
    }
    let (left, right) = gba.margin_counts();
    let describe = |c: gba_core::ppu::MarginCounts| {
        format!(
            "backdrop={} ring={} authored={} verdict={}",
            c.backdrop,
            c.ring,
            c.authored,
            match c.verdict() {
                MarginKind::Authored => "authored",
                MarginKind::Ring => "ring",
                MarginKind::Backdrop => "backdrop",
                MarginKind::Native => "native",
            }
        )
    };
    println!("  left margin:  {}", describe(left));
    println!("  right margin: {}", describe(right));
    let total = left.total() + right.total();
    if total > 0 {
        println!(
            "  conclusion: {} of {total} margin columns are authored content; \
             the rest is ring wrap or backdrop, which no patch can widen",
            left.authored + right.authored
        );
    } else {
        println!("  conclusion: no margin (native 240)");
    }
}

fn run() -> Result<(), String> {
    let o = parse()?;
    let mut rom = fs::read(&o.rom).map_err(|e| format!("cannot read ROM: {e}"))?;
    // SPEC-070: profile patches are applied before the machine sees the image;
    // a byte mismatch aborts the run instead of patching partially.
    let prepared = gba_profile::prepare_rom(&mut rom)?;
    for patch in &prepared.applied {
        eprintln!(
            "gba-recomp: {patch} ({})",
            prepared.profile_id.as_deref().unwrap_or("?")
        );
    }
    let profile_ceiling = prepared.view_ceiling;
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
    // SPEC-070/SPEC-069: a profile ceiling or an explicit --view-width widens
    // the presentation surface; the override wins (WS-69-18). PPM stays
    // canonical. Nothing here reaches frame_hash.
    if let Some(width) = o.view_width.or(profile_ceiling) {
        let extra = width - SCREEN_WIDTH as u16;
        gba.set_view(gba_core::View {
            left: extra / 2,
            right: extra - extra / 2,
        })?;
    }
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
    if o.margin_report {
        print_margin_report(&gba);
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
        "{{\"mode\":\"interpreted\",\"frames\":{frame},\"instructions_interpreted\":{},\"guest_cycles\":{},\"view_width\":{},\"frame_hash\":\"{:016x}\",\"pc\":\"0x{:08X}\",\"cpsr\":\"0x{:08X}\",\"backup\":\"{}\",\"swi_calls\":{{{}}},\"unsupported_io\":{{{}}},\"fault\":{},\"host_seconds\":{:.3},\"fps\":{:.1}}}",
        gba.instructions,
        gba.bus.cycles,
        gba.view().width(),
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

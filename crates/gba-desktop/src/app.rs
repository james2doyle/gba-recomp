//! Desktop application loop shared by the interpreted binary and the
//! recompiled package runtime (SPEC-061 FE-61-01, SPEC-063).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use gba_core::{BackupKind, Gba, GbaConfig, NativeExec, SCREEN_HEIGHT, SCREEN_WIDTH};

use crate::ctxmenu::{self, CtxItem, action, submenu};
use crate::gpio::GpioInput;
use crate::input::{Bindings, JoyControl, JoyState, PadState};
use crate::menu::{Command, Menu, Nav, Page};
use crate::sdl::{self, Event, button, scancode};
use crate::settings::{self, BUTTONS, Filter, Settings};

/// Guest clock and frame length: 16777216 Hz / 280896 cycles ≈ 59.7275 fps.
pub const GUEST_HZ: u64 = 16_777_216;
pub const CYCLES_PER_FRAME: u64 = 280_896;
const AUDIO_HZ: i32 = 32_768;
const SAVE_INTERVAL: Duration = Duration::from_secs(10);
const MAX_LAG_FRAMES: u32 = 5;

pub struct AppOptions {
    pub rom: PathBuf,
    /// ROM already in memory (embedded in a bundle, SPEC-066); wins over `rom`.
    pub rom_bytes: Option<Vec<u8>>,
    pub bios: Option<PathBuf>,
    pub save: Option<PathBuf>,
    pub backup: Option<BackupKind>,
    /// Exit after this many guest frames (automated smoke tests).
    pub frames: Option<u64>,
    pub settings_path: Option<PathBuf>,
    /// Window title, including the execution mode (FR-63-01).
    pub title: String,
    /// Show the logo splash before the game starts (FR-63-03).
    pub splash: bool,
}

#[derive(Debug, Default)]
pub struct AppReport {
    pub frames: u64,
    pub presented: u64,
    pub audio_on: bool,
    pub audio_underruns: u64,
    pub audio_overruns: u64,
    pub saves: u64,
    pub interpreted_instructions: u64,
    pub native_misses: u64,
    pub native_shells: u64,
    pub guest_cycles: u64,
    pub frame_hash: u64,
}

/// Converts a BGR555 guest pixel to host XRGB8888 (presentation only).
pub fn bgr555_to_xrgb(c: u16) -> u32 {
    let expand = |v: u16| {
        let v = u32::from(v & 0x1F);
        (v << 3) | (v >> 2)
    };
    0xFF00_0000 | (expand(c) << 16) | (expand(c >> 5) << 8) | expand(c >> 10)
}

/// LCD colour correction table (presentation only, SET-63-02): a gamma
/// model of the GBA screen mixing neighbouring channels.
fn color_correction_lut() -> Vec<u32> {
    let mut lut = vec![0u32; 0x8000];
    for (c, out) in lut.iter_mut().enumerate() {
        let ch = |shift: usize| ((c >> shift) & 0x1F) as f64 / 31.0;
        let (r, g, b) = (ch(0).powf(4.0), ch(5).powf(4.0), ch(10).powf(4.0));
        let mix = |lr: f64, lg: f64, lb: f64| {
            let v = ((lr * r + lg * g + lb * b) / 255.0).powf(1.0 / 2.2) * 255.0 * 255.0 / 280.0;
            v.round().clamp(0.0, 255.0) as u32
        };
        let (ro, go, bo) = (
            mix(255.0, 50.0, 0.0),
            mix(10.0, 230.0, 30.0),
            mix(50.0, 10.0, 220.0),
        );
        *out = 0xFF00_0000 | (ro << 16) | (go << 8) | bo;
    }
    lut
}

/// Writes `data` to `path` atomically (temporary file + fsync + rename).
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
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

/// Resolve SDL names to codes; unknown names are skipped with a warning.
fn resolve(input: &settings::Input) -> (Bindings, Vec<String>) {
    let mut warnings = Vec::new();
    let mut b = Bindings {
        deadzone: input.deadzone,
        ..Bindings::default()
    };
    for i in 0..BUTTONS.len() {
        let mask = 1u16 << i;
        for name in &input.keyboard[i] {
            match sdl::scancode_from_name(name) {
                Some(sc) => b.keyboard.push((sc, mask)),
                None => warnings.push(format!("unknown key `{name}` for {}", BUTTONS[i])),
            }
        }
        for name in &input.joystick[i] {
            match JoyControl::parse(name) {
                Some(c) => b.joystick.push((c, mask)),
                None => warnings.push(format!(
                    "invalid joystick control `{name}` for {}",
                    BUTTONS[i]
                )),
            }
        }
        for name in &input.gamepad[i] {
            match sdl::pad_button_from_name(name) {
                Some(btn) => b.gamepad.push((btn, mask)),
                None => warnings.push(format!(
                    "unknown gamepad button `{name}` for {}",
                    BUTTONS[i]
                )),
            }
        }
    }
    (b, warnings)
}

// Context menu action ids (CTX-63-01).
const A_SCALE: u32 = 1; // 1..=8
const A_FULLSCREEN: u32 = 20;
const A_LINEAR: u32 = 21;
const A_INTEGER: u32 = 22;
const A_VSYNC: u32 = 23;
const A_COLOR: u32 = 24;
const A_MUTE: u32 = 30;
const A_VOLUME: u32 = 31; // 31..=35 → 0,25,50,75,100
const A_LATENCY: u32 = 40; // 40..=43
const LATENCIES: [u16; 4] = [40, 60, 100, 150];
const A_DEADZONE: u32 = 50; // 50..=52
const DEADZONES: [f32; 3] = [0.10, 0.25, 0.40];
const A_REMAP: u32 = 53;
const A_INPUT_DEFAULTS: u32 = 54;
const A_PAUSE: u32 = 60;
const A_RESET: u32 = 61;
const A_SAVE: u32 = 62;
const A_MENU: u32 = 63;
const A_QUIT: u32 = 64;
const A_EXTERNAL: u32 = 65;

/// Launch `gba-settings` (EXT-64-02): next to this executable, then PATH.
fn open_external_settings(settings_path: Option<&Path>) -> Result<(), String> {
    let path = settings_path.ok_or("no settings file location")?;
    let exe_name = if cfg!(windows) {
        "gba-settings.exe"
    } else {
        "gba-settings"
    };
    let local = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(exe_name)))
        .filter(|p| p.is_file());
    let program = local.unwrap_or_else(|| PathBuf::from(exe_name));
    std::process::Command::new(&program)
        .arg("--settings")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start {} ({e})", program.display()))
}

fn context_items(s: &Settings, paused: bool) -> Vec<CtxItem> {
    let v = &s.video;
    let a = &s.audio;
    vec![
        submenu(
            "Vídeo",
            vec![
                submenu(
                    "Escala da janela",
                    (1..=8u8)
                        .map(|n| action(A_SCALE + u32::from(n) - 1, format!("{n}×"), v.scale == n))
                        .collect(),
                ),
                action(A_FULLSCREEN, "Tela cheia", v.fullscreen),
                ctxmenu::CtxItem::Separator,
                action(
                    A_LINEAR,
                    "Filtro suave (linear)",
                    v.filter == Filter::Linear,
                ),
                action(A_INTEGER, "Escala inteira", v.integer_scale),
                action(A_VSYNC, "VSync", v.vsync),
                action(A_COLOR, "Correção de cor do LCD", v.color_correction),
            ],
        ),
        submenu(
            "Som",
            vec![
                action(A_MUTE, "Mudo", a.mute),
                submenu(
                    "Volume",
                    [0u8, 25, 50, 75, 100]
                        .iter()
                        .enumerate()
                        .map(|(i, &vol)| {
                            action(A_VOLUME + i as u32, format!("{vol}%"), a.volume == vol)
                        })
                        .collect(),
                ),
                submenu(
                    "Latência",
                    LATENCIES
                        .iter()
                        .enumerate()
                        .map(|(i, &ms)| {
                            action(A_LATENCY + i as u32, format!("{ms} ms"), a.latency_ms == ms)
                        })
                        .collect(),
                ),
            ],
        ),
        submenu(
            "Controles",
            vec![
                action(A_REMAP, "Remapear botões…", false),
                submenu(
                    "Zona morta do analógico",
                    DEADZONES
                        .iter()
                        .enumerate()
                        .map(|(i, &dz)| {
                            action(
                                A_DEADZONE + i as u32,
                                format!("{:.0}%", dz * 100.0),
                                (s.input.deadzone - dz).abs() < 0.001,
                            )
                        })
                        .collect(),
                ),
                action(A_INPUT_DEFAULTS, "Restaurar controles padrão", false),
            ],
        ),
        ctxmenu::CtxItem::Separator,
        action(A_PAUSE, if paused { "Continuar" } else { "Pausar" }, false),
        action(A_RESET, "Reiniciar jogo", false),
        action(A_SAVE, "Salvar agora", false),
        action(A_EXTERNAL, "Configurações…", false),
        action(A_MENU, "Menu na janela…", false),
        ctxmenu::CtxItem::Separator,
        action(A_QUIT, "Sair", false),
    ]
}

enum Effect {
    None,
    Settings(Settings),
    OpenMenu(Page),
    TogglePause,
    OpenExternal,
    Reset,
    SaveNow,
    Quit,
}

fn context_effect(id: u32, s: &Settings) -> Effect {
    let mut n = s.clone();
    match id {
        A_SCALE..=8 => n.video.scale = id as u8,
        A_FULLSCREEN => n.video.fullscreen = !n.video.fullscreen,
        A_LINEAR => {
            n.video.filter = if n.video.filter == Filter::Linear {
                Filter::Nearest
            } else {
                Filter::Linear
            }
        }
        A_INTEGER => n.video.integer_scale = !n.video.integer_scale,
        A_VSYNC => n.video.vsync = !n.video.vsync,
        A_COLOR => n.video.color_correction = !n.video.color_correction,
        A_MUTE => n.audio.mute = !n.audio.mute,
        31..=35 => n.audio.volume = ((id - A_VOLUME) * 25) as u8,
        40..=43 => n.audio.latency_ms = LATENCIES[(id - A_LATENCY) as usize],
        50..=52 => n.input.deadzone = DEADZONES[(id - A_DEADZONE) as usize],
        A_INPUT_DEFAULTS => n.input = settings::Input::default(),
        A_REMAP => return Effect::OpenMenu(Page::Controls),
        A_PAUSE => return Effect::TogglePause,
        A_RESET => return Effect::Reset,
        A_SAVE => return Effect::SaveNow,
        A_MENU => return Effect::OpenMenu(Page::Main),
        A_QUIT => return Effect::Quit,
        A_EXTERNAL => return Effect::OpenExternal,
        _ => return Effect::None,
    }
    Effect::Settings(n)
}

struct Presentation {
    lut: Option<Vec<u32>>,
}

impl Presentation {
    fn convert(&self, src: &[u16], dst: &mut [u32]) {
        match &self.lut {
            Some(lut) => {
                for (d, &c) in dst.iter_mut().zip(src) {
                    *d = lut[usize::from(c & 0x7FFF)];
                }
            }
            None => {
                for (d, &c) in dst.iter_mut().zip(src) {
                    *d = bgr555_to_xrgb(c);
                }
            }
        }
    }
}

/// Run the game in a window until the user quits (or `frames` elapse).
pub fn run_app(
    o: &AppOptions,
    mut native: Option<&mut dyn NativeExec>,
) -> Result<AppReport, String> {
    let rom = match &o.rom_bytes {
        Some(bytes) => bytes.clone(),
        None => fs::read(&o.rom).map_err(|e| format!("cannot read ROM: {e}"))?,
    };
    let bios = match &o.bios {
        Some(p) => Some(fs::read(p).map_err(|e| format!("cannot read BIOS: {e}"))?),
        None => None,
    };
    let save = match &o.save {
        Some(p) if p.exists() => Some(fs::read(p).map_err(|e| format!("cannot read save: {e}"))?),
        _ => None,
    };
    let new_gba = |save: Option<&[u8]>| {
        Gba::new(GbaConfig {
            rom: &rom,
            bios: bios.as_deref(),
            backup: o.backup,
            save,
        })
    };
    let mut gba = new_gba(save.as_deref());

    let settings_path = o.settings_path.clone().or_else(settings::default_path);
    let loaded = settings_path
        .as_deref()
        .map(settings::load)
        .unwrap_or(settings::Loaded {
            settings: Settings::default(),
            warning: None,
            invalid_on_disk: false,
        });
    let mut settings_invalid_on_disk = loaded.invalid_on_disk;
    let mut current = loaded.settings;
    let mut status: Option<(String, Instant)> = loaded.warning.map(|w| {
        eprintln!("gba-desktop: {w}");
        (
            "Configuracoes invalidas: usando padroes".to_owned(),
            Instant::now(),
        )
    });

    // Declaration order matters: owners drop before `ctx` (SDL_Quit).
    let ctx = sdl::Context::init()?;
    let (w, h) = (SCREEN_WIDTH as i32, SCREEN_HEIGHT as i32);
    let mut video = ctx.create_video(&o.title, w, h, i32::from(current.video.scale))?;
    let mut audio = match ctx.open_audio(AUDIO_HZ) {
        Ok(a) => Some(a),
        Err(e) => {
            eprintln!("gba-desktop: audio disabled ({e}); continuing without sound");
            None
        }
    };
    let mut pads: Vec<sdl::Gamepad<'_>> = Vec::new();
    let mut joys: Vec<sdl::Joystick<'_>> = Vec::new();
    let mut gpio: Option<GpioInput> = None;

    let mut bindings = Bindings::default();
    let mut presentation = Presentation { lut: None };
    let mut applied: Option<Settings> = None;
    let mut apply = |s: &Settings,
                     video: &mut sdl::Video<'_>,
                     audio: &mut Option<sdl::Audio<'_>>,
                     bindings: &mut Bindings,
                     presentation: &mut Presentation,
                     gpio: &mut Option<GpioInput>| {
        let prev = applied.clone();
        let v = &s.video;
        video.set_presentation(v.filter == Filter::Linear, v.integer_scale, v.vsync);
        if prev.as_ref().map(|p| p.video.fullscreen) != Some(v.fullscreen) {
            video.set_fullscreen(v.fullscreen);
        }
        if prev.as_ref().map(|p| p.video.scale) != Some(v.scale) && !v.fullscreen {
            video.set_scale(i32::from(v.scale));
        }
        presentation.lut = v.color_correction.then(color_correction_lut);
        if let Some(a) = audio.as_mut() {
            a.set_gain(if s.audio.mute {
                0.0
            } else {
                f32::from(s.audio.volume) / 100.0
            });
        }
        if prev.as_ref().map(|p| &p.gpio) != Some(&s.gpio) {
            // Release lines before requesting the new configuration.
            *gpio = None;
            match GpioInput::open(&s.gpio) {
                Ok(g) => *gpio = g,
                Err(e) => eprintln!("gba-desktop: GPIO disabled ({e})"),
            }
        }
        let (b, warnings) = resolve(&s.input);
        for w in warnings {
            eprintln!("gba-desktop: {w}");
        }
        *bindings = b;
        applied = Some(s.clone());
    };
    // Branding (FR-63-03): window/Dock icon and a 2.4 s logo splash.
    match sdl::Surface::from_png(crate::assets::ICON_PNG) {
        Ok(icon) => video.set_icon(&icon),
        Err(e) => eprintln!("gba-desktop: icon unavailable ({e})"),
    }
    apply(
        &current,
        &mut video,
        &mut audio,
        &mut bindings,
        &mut presentation,
        &mut gpio,
    );

    let mtime = |path: Option<&Path>| {
        path.and_then(|p| fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
    };
    let mut known_mtime = mtime(settings_path.as_deref());
    let mut last_settings_check = Instant::now();
    let persist_settings = |s: &Settings,
                            invalid: &mut bool,
                            known: &mut Option<std::time::SystemTime>|
     -> Option<String> {
        let path = settings_path.as_deref()?;
        match settings::save(path, s, *invalid) {
            Ok(()) => {
                *invalid = false;
                *known = mtime(Some(path));
                None
            }
            Err(e) => {
                eprintln!("gba-desktop: {e}");
                Some("Nao foi possivel salvar as configuracoes".into())
            }
        }
    };

    if o.splash {
        match sdl::Surface::from_png(crate::assets::SPLASH_PNG) {
            Ok(logo) => {
                let keep = video.splash(&logo, crate::assets::SPLASH_DURATION, || {
                    while let Some(ev) = ctx.poll_event() {
                        if matches!(ev, Event::Quit) {
                            return false;
                        }
                    }
                    true
                });
                // Restore the user's presentation (filter/scaling/vsync).
                let v = &current.video;
                video.set_presentation(v.filter == Filter::Linear, v.integer_scale, v.vsync);
                if !keep {
                    drop(pads);
                    drop(joys);
                    drop(audio);
                    drop(video);
                    drop(ctx);
                    return Ok(AppReport::default());
                }
            }
            Err(e) => eprintln!("gba-desktop: splash unavailable ({e})"),
        }
    }

    let frame_time = Duration::from_nanos(CYCLES_PER_FRAME * 1_000_000_000 / GUEST_HZ);
    let mut report = AppReport {
        audio_on: audio.is_some(),
        ..AppReport::default()
    };
    let mut pixels = vec![0u32; SCREEN_WIDTH * SCREEN_HEIGHT];
    let mut deadline = Instant::now();
    let mut last_present = Instant::now() - frame_time;
    let mut last_save = Instant::now();
    let mut fault = None;
    let mut menu = Menu::new(current.clone());
    let mut paused = false;

    'main: loop {
        if o.frames.is_some_and(|n| report.frames >= n) {
            break;
        }
        let was_suspended = menu.open || paused;
        let mut effect = Effect::None;
        let mut nav: Vec<Nav> = Vec::new();
        while let Some(ev) = ctx.poll_event() {
            match ev {
                Event::Quit => break 'main,
                Event::FocusLost => {}
                Event::GamepadAdded(id) => {
                    if !pads.iter().any(|p| p.id() == id) {
                        match ctx.open_gamepad(id) {
                            Ok(p) => pads.push(p),
                            Err(e) => eprintln!("gba-desktop: {e}"),
                        }
                    }
                }
                Event::GamepadRemoved(id) => pads.retain(|p| p.id() != id),
                Event::JoystickAdded(id) => {
                    if !ctx.is_gamepad(id) && !joys.iter().any(|j| j.id() == id) {
                        match ctx.open_joystick(id) {
                            Ok(j) => joys.push(j),
                            Err(e) => eprintln!("gba-desktop: {e}"),
                        }
                    }
                }
                Event::JoystickRemoved(id) => joys.retain(|j| j.id() != id),
                Event::JoyButtonDown { .. } | Event::JoyHat { .. } | Event::JoyAxis { .. } => {}
                Event::KeyDown {
                    scancode: sc,
                    repeat,
                } => {
                    if menu.open && menu.remap.is_some_and(|r| !r.pad) && sc != scancode::ESCAPE {
                        // Ignore auto-repeat of the key that started the remap.
                        if !repeat && let Some(s) = menu.bind(sdl::scancode_name(sc), false) {
                            effect = Effect::Settings(s);
                        }
                        continue;
                    }
                    if sc == scancode::F11 && !repeat {
                        let mut n = current.clone();
                        n.video.fullscreen = !n.video.fullscreen;
                        effect = Effect::Settings(n);
                        continue;
                    }
                    if menu.open {
                        nav.extend(match sc {
                            scancode::UP => Some(Nav::Up),
                            scancode::DOWN => Some(Nav::Down),
                            scancode::LEFT => Some(Nav::Left),
                            scancode::RIGHT => Some(Nav::Right),
                            scancode::RETURN | scancode::SPACE | scancode::Z => Some(Nav::Confirm),
                            scancode::ESCAPE | scancode::BACKSPACE | scancode::X => Some(Nav::Back),
                            _ => None,
                        });
                    } else if (sc == scancode::ESCAPE || sc == scancode::F1) && !repeat {
                        effect = Effect::OpenMenu(Page::Main);
                    }
                }
                Event::PadDown { button: b } => {
                    if menu.open && menu.remap.is_some_and(|r| r.pad) {
                        if b == button::GUIDE {
                            nav.push(Nav::Back);
                        } else {
                            if let Some(s) = menu.bind(sdl::pad_button_name(b), true) {
                                effect = Effect::Settings(s);
                            }
                        }
                        continue;
                    }
                    let back_held = pads.iter().any(|p| p.button(button::BACK));
                    let start_held = pads.iter().any(|p| p.button(button::START));
                    if menu.open {
                        nav.extend(match b {
                            button::DPAD_UP => Some(Nav::Up),
                            button::DPAD_DOWN => Some(Nav::Down),
                            button::DPAD_LEFT => Some(Nav::Left),
                            button::DPAD_RIGHT => Some(Nav::Right),
                            button::SOUTH => Some(Nav::Confirm),
                            button::EAST | button::GUIDE => Some(Nav::Back),
                            _ => None,
                        });
                    } else if b == button::GUIDE || (back_held && start_held) {
                        effect = Effect::OpenMenu(Page::Main);
                    }
                }
                Event::MouseDown { button: 3 } => {
                    if ctxmenu::available() {
                        if let Some(id) = ctxmenu::popup(&context_items(&current, paused)) {
                            effect = context_effect(id, &current);
                        }
                        // The modal menu consumed host time: do not count it
                        // as emulation debt (UI-06).
                        deadline = Instant::now();
                    } else {
                        effect = Effect::OpenMenu(Page::Main);
                    }
                }
                Event::MouseDown { .. } | Event::Other => {}
            }
        }
        for n in nav {
            match menu.handle(n) {
                Command::None => {}
                Command::Resume => {}
                Command::Reset => effect = Effect::Reset,
                Command::SaveNow => effect = Effect::SaveNow,
                Command::Quit => effect = Effect::Quit,
                Command::OpenExternal => effect = Effect::OpenExternal,
                Command::Apply(s) => effect = Effect::Settings(s),
            }
        }
        match effect {
            Effect::None => {}
            Effect::Settings(s) => {
                apply(
                    &s,
                    &mut video,
                    &mut audio,
                    &mut bindings,
                    &mut presentation,
                    &mut gpio,
                );
                let failure = persist_settings(&s, &mut settings_invalid_on_disk, &mut known_mtime);
                status = Some((
                    failure.unwrap_or_else(|| "Configuracoes salvas".into()),
                    Instant::now(),
                ));
                current = s;
            }
            Effect::OpenMenu(page) => menu.open_at(page, &current),
            Effect::TogglePause => paused = !paused,
            Effect::OpenExternal => {
                let msg = match open_external_settings(settings_path.as_deref()) {
                    Ok(()) => "Janela de configuracoes aberta".to_owned(),
                    Err(e) => {
                        eprintln!("gba-desktop: {e}");
                        "gba-settings nao encontrado".to_owned()
                    }
                };
                status = Some((msg, Instant::now()));
            }
            Effect::Reset => {
                let backup = gba.bus.backup.data().to_vec();
                let dirty = gba.bus.backup.dirty;
                gba = new_gba(Some(&backup));
                gba.bus.backup.dirty = dirty;
                status = Some(("Jogo reiniciado".into(), Instant::now()));
            }
            Effect::SaveNow => {
                let msg = match persist_save(&mut gba, o.save.as_deref()) {
                    Ok(true) => {
                        report.saves += 1;
                        "Save gravado".to_owned()
                    }
                    Ok(false) if o.save.is_none() => "Sem arquivo de save (--save)".to_owned(),
                    Ok(false) => "Nada novo para gravar".to_owned(),
                    Err(e) => {
                        eprintln!("gba-desktop: {e}");
                        "Falha ao gravar o save".to_owned()
                    }
                };
                status = Some((msg, Instant::now()));
            }
            Effect::Quit => break 'main,
        }

        // Live reload when another process (gba-settings) changed the file.
        if last_settings_check.elapsed() >= Duration::from_millis(500) {
            last_settings_check = Instant::now();
            let now_mtime = mtime(settings_path.as_deref());
            if now_mtime.is_some() && now_mtime != known_mtime {
                known_mtime = now_mtime;
                if let Some(path) = settings_path.as_deref() {
                    let loaded = settings::load(path);
                    match loaded.warning {
                        None => {
                            apply(
                                &loaded.settings,
                                &mut video,
                                &mut audio,
                                &mut bindings,
                                &mut presentation,
                                &mut gpio,
                            );
                            current = loaded.settings;
                            if !menu.open {
                                menu = Menu::new(current.clone());
                            }
                            status = Some(("Configuracoes recarregadas".into(), Instant::now()));
                        }
                        Some(w) => eprintln!("gba-desktop: {w}"),
                    }
                }
            }
        }

        let suspended = menu.open || paused;
        if suspended != was_suspended
            && let Some(a) = audio.as_mut()
        {
            if suspended { a.pause() } else { a.resume() }
        }
        if suspended {
            // Guest paused (UI-06): redraw the last frame with the overlay.
            presentation.convert(gba.framebuffer(), &mut pixels);
            let rows = if menu.open { menu.rows() } else { Vec::new() };
            let (title, cursor, open) = (menu.title(), menu.cursor, menu.open);
            let remap_hint = menu.remap.map(|r| {
                if r.pad {
                    "Pressione um botao (Guide cancela)"
                } else {
                    "Pressione uma tecla (Esc cancela)"
                }
            });
            let msg = status.as_ref().map(|(m, _)| m.clone());
            video.present_with(&pixels, |p| {
                if open {
                    draw_menu(p, title, &rows, cursor, remap_hint, msg.as_deref());
                } else {
                    p.fill(0.0, 0.0, 240.0, 14.0, [0, 0, 0, 180]);
                    p.text(4.0, 3.0, "PAUSADO - botao direito: menu", [255, 255, 255]);
                }
            })?;
            report.presented += 1;
            thread::sleep(frame_time);
            deadline = Instant::now();
            continue;
        }

        let pad_states: Vec<(i16, i16)> = pads.iter().map(|p| p.left_stick()).collect();
        let pad_buttons: Vec<Box<dyn Fn(i32) -> bool + '_>> = pads
            .iter()
            .map(|p| Box::new(move |b| p.button(b)) as Box<dyn Fn(i32) -> bool>)
            .collect();
        let states: Vec<PadState<'_>> = pad_buttons
            .iter()
            .zip(&pad_states)
            .map(|(f, &(x, y))| PadState {
                button: f.as_ref(),
                left_x: x,
                left_y: y,
            })
            .collect();
        let joy_fns: Vec<_> = joys
            .iter()
            .map(|j| {
                (
                    Box::new(move |b| j.button(b)) as Box<dyn Fn(i32) -> bool + '_>,
                    Box::new(move |a| j.axis(a)) as Box<dyn Fn(i32) -> i16 + '_>,
                    Box::new(move |h| j.hat(h)) as Box<dyn Fn(i32) -> u8 + '_>,
                )
            })
            .collect();
        let joy_states: Vec<JoyState<'_>> = joy_fns
            .iter()
            .map(|(b, a, h)| JoyState {
                button: b.as_ref(),
                axis: a.as_ref(),
                hat: h.as_ref(),
            })
            .collect();
        let fast_forward = ctx.key_down(scancode::TAB);
        let gpio_mask = gpio.as_ref().map_or(0, GpioInput::mask);
        gba.set_keys(bindings.mask_all(|sc| ctx.key_down(sc), &states, &joy_states, gpio_mask));
        drop(joy_states);
        drop(joy_fns);
        drop(states);
        drop(pad_buttons);

        let result = match native.as_deref_mut() {
            Some(n) => gba.run_frame_with(n),
            None => gba.run_frame(),
        };
        if let Err(f) = result {
            fault = Some(f);
            break;
        }
        report.frames += 1;

        let samples = gba.take_audio();
        if let Some(a) = audio.as_mut()
            && !fast_forward
        {
            let max_queued = (AUDIO_HZ as usize) * 4 * usize::from(current.audio.latency_ms) / 1000;
            let queued = a.queued_bytes();
            if queued == 0 && report.frames > 1 {
                report.audio_underruns += 1;
            }
            if queued > max_queued {
                report.audio_overruns += 1; // drop this frame's samples
            } else if let Err(e) = a.put(&samples) {
                eprintln!("gba-desktop: audio disabled ({e})");
                audio = None;
            }
        }

        let now = Instant::now();
        if !fast_forward || now.duration_since(last_present) >= frame_time {
            presentation.convert(gba.framebuffer(), &mut pixels);
            if let Some((msg, at)) = &status
                && at.elapsed() < Duration::from_secs(3)
            {
                let msg = msg.clone();
                video.present_with(&pixels, |p| {
                    p.fill(0.0, 146.0, 240.0, 14.0, [0, 0, 0, 180]);
                    p.text(4.0, 149.0, &msg, [255, 255, 160]);
                })?;
            } else {
                video.present(&pixels)?;
            }
            report.presented += 1;
            last_present = now;
        }

        if last_save.elapsed() >= SAVE_INTERVAL {
            last_save = Instant::now();
            match persist_save(&mut gba, o.save.as_deref()) {
                Ok(true) => report.saves += 1,
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
        report.saves += 1;
    }
    report.audio_on = audio.is_some();
    report.interpreted_instructions = gba.instructions;
    report.native_misses = gba.native.misses;
    report.native_shells = gba.native.shells;
    report.guest_cycles = gba.bus.cycles;
    report.frame_hash = gba.frame_hash();
    drop(pads);
    drop(joys);
    drop(audio);
    drop(video);
    drop(ctx);
    save_result?;
    if let Some(f) = fault {
        return Err(format!(
            "guest fault at frame {}: {f:?} (pc=0x{:08X})",
            report.frames + 1,
            gba.cpu.pc()
        ));
    }
    Ok(report)
}

fn draw_menu(
    p: &mut sdl::Painter<'_>,
    title: &str,
    rows: &[crate::menu::Row],
    cursor: usize,
    remap_hint: Option<&str>,
    status: Option<&str>,
) {
    p.fill(0.0, 0.0, 240.0, 160.0, [0, 0, 24, 200]);
    p.text(6.0, 4.0, title, [255, 220, 120]);
    const VISIBLE: usize = 15;
    let start = cursor.saturating_sub(VISIBLE - 1);
    for (i, row) in rows.iter().enumerate().skip(start).take(VISIBLE) {
        let y = 16.0 + (i - start) as f32 * 9.0;
        if i == cursor {
            p.fill(2.0, y - 1.0, 236.0, 9.0, [80, 80, 160, 255]);
        }
        p.text(8.0, y, &row.label, [255, 255, 255]);
        if !row.value.is_empty() {
            let x = 236.0 - 8.0 * row.value.len().min(14) as f32;
            let value: String = row.value.chars().take(14).collect();
            p.text(x.max(120.0), y, &value, [170, 220, 255]);
        }
    }
    let footer = remap_hint
        .or(status)
        .unwrap_or("Setas/Enter/Esc  ou  D-pad/A/B");
    p.fill(0.0, 150.0, 240.0, 10.0, [0, 0, 0, 220]);
    p.text(4.0, 151.0, footer, [200, 200, 200]);
}

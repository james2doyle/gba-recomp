//! Desktop application loop shared by the interpreted binary and the
//! recompiled package runtime (SPEC-061 FE-61-01, SPEC-063).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gba_core::{BackupKind, Gba, GbaConfig, NativeExec, SCREEN_HEIGHT, SCREEN_WIDTH};

use gba_i18n::tr;

use crate::ctxmenu::{self, CtxItem, action, submenu};
use crate::gpio::GpioInput;
use crate::input::{Bindings, JoyControl, JoyState, PadState};
use crate::menu::{Command, Menu, Nav, Page};
use crate::sdl::{self, Event, button, scancode};
use crate::settings::{self, BUTTONS, Filter, Settings, TouchMode};
use crate::touch;

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
const A_PARTY: u32 = 66;
/// 70.. = one action per language, in `Lang::all()` order.
const A_LANGUAGE: u32 = 70;

/// Launch `gba-settings` (EXT-64-02): next to this executable, then PATH.
fn open_external_settings(settings_path: Option<&Path>) -> Result<(), String> {
    let path = settings_path.ok_or("no settings file location")?;
    open_companion(
        "gba-settings",
        &[std::ffi::OsStr::new("--settings"), path.as_os_str()],
    )
}

/// Launch a companion app shipped next to this executable (settings,
/// party), falling back to PATH.
fn open_companion(name: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    // SPEC-068 TGT-68-03: Android ships no companion apps.
    if cfg!(target_os = "android") {
        return Err(format!("{name} is not available on Android"));
    }
    let exe_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    let exe_name = exe_name.as_str();
    let local = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(exe_name)))
        .filter(|p| p.is_file());
    let program = local.unwrap_or_else(|| PathBuf::from(exe_name));
    std::process::Command::new(&program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start {} ({e})", program.display()))
}

/// Adopts the language from a settings file that another process wrote, and
/// reports whether it changed (SPEC-069 L10N-69-03).
///
/// The menu's draft and effective settings move with it, or the next `Apply`
/// would silently revert a language the user did not touch.
fn adopt_language(loaded: &Settings, menu: &mut Menu) -> bool {
    if loaded.language == gba_i18n::lang() {
        return false;
    }
    gba_i18n::set_lang(loaded.language);
    menu.set_language(loaded.language);
    // Rare host event, and otherwise invisible when no menu is open: say so,
    // like the audio/GPIO diagnostics next to it.
    eprintln!("gba-desktop: language changed to {}", loaded.language.tag());
    true
}

/// macOS menu bar (CTX-63-02): app menu entries and top-level menus. Built
/// per refresh rather than declared as a constant because the bar is rebuilt
/// whenever settings change, and a language change is a settings change
/// (SPEC-069 L10N-69-03).
fn app_menu_items() -> Vec<(u32, String, &'static str)> {
    vec![
        (A_EXTERNAL, tr!("app.settings"), ","),
        (A_PARTY, tr!("app.party"), "p"),
    ]
}

/// Menu navigation for a keyboard key, including the Android back button,
/// which is the only way to reach the menu on a touch device
/// (SPEC-069 L10N-69-06).
fn nav_for_key(sc: usize) -> Option<Nav> {
    match sc {
        scancode::UP => Some(Nav::Up),
        scancode::DOWN => Some(Nav::Down),
        scancode::LEFT => Some(Nav::Left),
        scancode::RIGHT => Some(Nav::Right),
        scancode::RETURN | scancode::SPACE | scancode::Z => Some(Nav::Confirm),
        scancode::ESCAPE | scancode::BACKSPACE | scancode::X | scancode::AC_BACK => Some(Nav::Back),
        _ => None,
    }
}

/// Whether a key press may be recorded as a game binding during a keyboard
/// remap. Esc and the Android back button only cancel (SPEC-063 MENU-63-02,
/// SPEC-069) — capturing the back button would make the menu unreachable.
fn captures_key(sc: usize) -> bool {
    !matches!(sc, scancode::ESCAPE | scancode::AC_BACK)
}

/// Whether a key opens the menu when it is closed: Esc/F1 on desktop, the
/// Android back button on touch devices.
fn opens_menu(sc: usize) -> bool {
    matches!(sc, scancode::ESCAPE | scancode::F1 | scancode::AC_BACK)
}

/// Pause toggles between two verbs depending on state.
fn pause_label(paused: bool) -> String {
    if paused {
        tr!("ctx.resume")
    } else {
        tr!("ctx.pause")
    }
}

fn menubar_items(s: &Settings, paused: bool) -> Vec<CtxItem> {
    let mut items: Vec<CtxItem> = context_items(s, paused)
        .into_iter()
        .filter(|i| matches!(i, CtxItem::Submenu { .. }))
        .collect();
    items.push(submenu(
        tr!("ctx.system"),
        vec![
            action(A_PAUSE, pause_label(paused), false),
            action(A_RESET, tr!("ctx.reset"), false),
            action(A_SAVE, tr!("ctx.save_now"), false),
            action(A_MENU, tr!("ctx.in_window_menu"), false),
        ],
    ));
    items
}

fn context_items(s: &Settings, paused: bool) -> Vec<CtxItem> {
    let v = &s.video;
    let a = &s.audio;
    vec![
        submenu(
            tr!("ctx.video"),
            vec![
                submenu(
                    tr!("ctx.scale"),
                    (1..=8u8)
                        .map(|n| action(A_SCALE + u32::from(n) - 1, format!("{n}×"), v.scale == n))
                        .collect(),
                ),
                action(A_FULLSCREEN, tr!("ctx.fullscreen"), v.fullscreen),
                ctxmenu::CtxItem::Separator,
                action(
                    A_LINEAR,
                    tr!("ctx.filter_linear"),
                    v.filter == Filter::Linear,
                ),
                action(A_INTEGER, tr!("ctx.integer_scale"), v.integer_scale),
                action(A_VSYNC, tr!("menu.video.vsync"), v.vsync),
                action(A_COLOR, tr!("ctx.color_correction"), v.color_correction),
            ],
        ),
        submenu(
            tr!("ctx.sound"),
            vec![
                action(A_MUTE, tr!("ctx.mute"), a.mute),
                submenu(
                    tr!("ctx.volume"),
                    [0u8, 25, 50, 75, 100]
                        .iter()
                        .enumerate()
                        .map(|(i, &vol)| {
                            action(A_VOLUME + i as u32, format!("{vol}%"), a.volume == vol)
                        })
                        .collect(),
                ),
                submenu(
                    tr!("ctx.latency"),
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
            tr!("ctx.controls"),
            vec![
                action(A_REMAP, tr!("ctx.remap"), false),
                submenu(
                    tr!("ctx.deadzone"),
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
                action(A_INPUT_DEFAULTS, tr!("ctx.restore_defaults"), false),
            ],
        ),
        ctxmenu::CtxItem::Separator,
        submenu(
            tr!("ctx.language"),
            gba_i18n::Lang::all()
                .iter()
                .enumerate()
                .map(|(i, lang)| {
                    action(
                        A_LANGUAGE + i as u32,
                        lang.display_name(),
                        *lang == s.language,
                    )
                })
                .collect(),
        ),
        action(A_PAUSE, pause_label(paused), false),
        action(A_RESET, tr!("ctx.reset"), false),
        action(A_SAVE, tr!("ctx.save_now"), false),
        action(A_EXTERNAL, tr!("ctx.external"), false),
        action(A_PARTY, tr!("ctx.party"), false),
        action(A_MENU, tr!("ctx.in_window_menu"), false),
        ctxmenu::CtxItem::Separator,
        action(A_QUIT, tr!("menu.quit"), false),
    ]
}

enum Effect {
    None,
    Settings(Settings),
    OpenMenu(Page),
    TogglePause,
    OpenExternal,
    OpenParty,
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
        A_PARTY => return Effect::OpenParty,
        // One action per language, in `Lang::all()` order. Selecting one in
        // the native menu is an explicit choice: it applies at once and the
        // caller persists the returned settings.
        A_LANGUAGE.. => {
            if let Some(lang) = gba_i18n::Lang::all().get((id - A_LANGUAGE) as usize) {
                n.language = *lang;
                gba_i18n::set_lang(*lang);
            }
        }
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
    // A saved `language` line is an explicit choice and wins over the OS;
    // without one the OS language decides, including later changes to it
    // (SPEC-069 L10N-69-04).
    let (initial_lang, mut language_chosen) = settings::initial_language(
        settings_path.as_deref(),
        &loaded.settings,
        &sdl::preferred_locales(),
    );
    gba_i18n::set_lang(initial_lang);
    let mut current = loaded.settings;
    current.language = initial_lang;
    let mut status: Option<(String, Instant)> = loaded.warning.map(|w| {
        eprintln!("gba-desktop: {w}");
        (tr!("status.settings_invalid"), Instant::now())
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
    // Touch screens (SPEC-068 TOUCH-68-01): active fingers in logical coords.
    let mut fingers: std::collections::HashMap<u64, (f32, f32)> = Default::default();
    // Long press on empty screen area opens the menu on touch devices, where
    // there is no Esc/F1/right click (SPEC-069 L10N-69-06).
    let mut long_press = touch::LongPress::new();
    let has_touchscreen = ctx.has_touchscreen();
    let mut touch_seen = false;

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

    let mtime = settings::modified;
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
                Some(tr!("status.settings_unwritable"))
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

    let mut menubar_state: Option<(Settings, bool)> = None;

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
                // The system language changed. Only follow it when the user
                // has not chosen a language explicitly, so a saved preference
                // is never overridden (SPEC-069 L10N-69-04).
                Event::LocaleChanged => {
                    if !language_chosen {
                        let tags = sdl::preferred_locales();
                        if let Some(detected) = gba_i18n::detect_from(&tags) {
                            gba_i18n::set_lang(detected);
                            current.language = detected;
                            menu.draft.language = detected;
                            menu.set_language(detected);
                        }
                    }
                }
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
                    if menu.open && menu.remap.is_some_and(|r| !r.pad) && captures_key(sc) {
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
                        nav.extend(nav_for_key(sc));
                    } else if opens_menu(sc) && !repeat {
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
                Event::FingerDown { finger, x, y } => {
                    touch_seen = true;
                    match video.window_to_logical(x, y) {
                        Some(p) => {
                            fingers.insert(finger, p);
                            // Only the menu being closed arms the gesture; while
                            // it is open a tap belongs to navigation.
                            if !menu.open {
                                long_press.press(finger, p.0, p.1, Instant::now());
                            }
                        }
                        None => {
                            fingers.remove(&finger);
                            long_press.release(finger);
                        }
                    }
                }
                // Motion only updates the position; it never starts a press.
                Event::FingerMotion { finger, x, y } => {
                    if let Some(p) = video.window_to_logical(x, y) {
                        if let Some(slot) = fingers.get_mut(&finger) {
                            *slot = p;
                        }
                        long_press.moved(finger, p.0, p.1);
                    }
                }
                Event::FingerUp { finger } => {
                    fingers.remove(&finger);
                    long_press.release(finger);
                }
                Event::MouseDown { .. } | Event::Other => {}
            }
        }
        // Menu bar selections (macOS) and refresh of its check marks.
        if let Some(id) = ctxmenu::take_selected() {
            effect = context_effect(id, &current);
            deadline = Instant::now();
        }
        if menubar_state.as_ref() != Some(&(current.clone(), paused)) {
            ctxmenu::install_menubar(&app_menu_items(), &menubar_items(&current, paused));
            menubar_state = Some((current.clone(), paused));
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
        // A long press on free screen area opens the menu. Polled once per
        // frame so the threshold is independent of event batching.
        if !menu.open && long_press.poll(Instant::now()) {
            effect = Effect::OpenMenu(Page::Main);
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
                // Saving settings records an explicit language preference.
                language_chosen = true;
                let failure = persist_settings(&s, &mut settings_invalid_on_disk, &mut known_mtime);
                status = Some((
                    failure.unwrap_or_else(|| tr!("settings.saved")),
                    Instant::now(),
                ));
                current = s;
            }
            Effect::OpenMenu(page) => {
                // Any route that opens the menu consumes a pending gesture.
                long_press.cancel();
                menu.open_at(page, &current);
            }
            Effect::TogglePause => paused = !paused,
            Effect::OpenParty => {
                let msg = match open_companion("gba-party", &[]) {
                    Ok(()) => tr!("status.party_opened"),
                    Err(e) => {
                        eprintln!("gba-desktop: {e}");
                        tr!("status.party_missing")
                    }
                };
                status = Some((msg, Instant::now()));
            }
            Effect::OpenExternal => {
                let msg = match open_external_settings(settings_path.as_deref()) {
                    Ok(()) => tr!("status.settings_opened"),
                    Err(e) => {
                        eprintln!("gba-desktop: {e}");
                        tr!("status.settings_missing")
                    }
                };
                status = Some((msg, Instant::now()));
            }
            Effect::Reset => {
                let backup = gba.bus.backup.data().to_vec();
                let dirty = gba.bus.backup.dirty;
                gba = new_gba(Some(&backup));
                gba.bus.backup.dirty = dirty;
                status = Some((tr!("status.game_reset"), Instant::now()));
            }
            Effect::SaveNow => {
                let msg = match persist_save(&mut gba, o.save.as_deref()) {
                    Ok(true) => {
                        report.saves += 1;
                        tr!("status.save_written")
                    }
                    Ok(false) if o.save.is_none() => tr!("status.save_no_file"),
                    Ok(false) => tr!("status.save_unchanged"),
                    Err(e) => {
                        eprintln!("gba-desktop: {e}");
                        tr!("status.save_failed")
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
                            // A language chosen in the settings window must
                            // take effect here too, not only on the next
                            // launch (SPEC-069 L10N-69-03).
                            if adopt_language(&loaded.settings, &mut menu) {
                                language_chosen = true;
                            }
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
                            status = Some((tr!("status.settings_reloaded"), Instant::now()));
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
                    tr!("menu.controls.press_pad")
                } else {
                    tr!("menu.controls.press_key")
                }
            });
            let msg = status.as_ref().map(|(m, _)| m.clone());
            video.present_with(&pixels, |p| {
                if open {
                    draw_menu(
                        p,
                        &title,
                        &rows,
                        cursor,
                        remap_hint.as_deref(),
                        msg.as_deref(),
                        has_touchscreen || touch_seen,
                    );
                } else {
                    p.fill(0.0, 0.0, 240.0, 14.0, [0, 0, 0, 180]);
                    p.text(4.0, 3.0, &tr!("banner.paused"), [255, 255, 255]);
                }
            })?;
            report.presented += 1;
            crate::pace::sleep(frame_time);
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
        let show_touch = match current.input.touch_controls {
            TouchMode::On => true,
            TouchMode::Off => false,
            TouchMode::Auto => has_touchscreen || touch_seen,
        };
        let touch_mask = if show_touch {
            touch::mask(fingers.values().copied())
        } else {
            0
        };
        gba.set_keys(bindings.mask_all(
            |sc| ctx.key_down(sc),
            &states,
            &joy_states,
            gpio_mask | touch_mask,
        ));
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
            let msg = status
                .as_ref()
                .filter(|(_, at)| at.elapsed() < Duration::from_secs(3))
                .map(|(m, _)| m.clone());
            if msg.is_some() || show_touch {
                video.present_with(&pixels, |p| {
                    if show_touch {
                        draw_touch(p, touch_mask);
                    }
                    if let Some(msg) = &msg {
                        p.fill(0.0, 146.0, 240.0, 14.0, [0, 0, 0, 180]);
                        p.text(4.0, 149.0, msg, [255, 255, 160]);
                    }
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
            crate::pace::yield_now();
        } else {
            deadline += frame_time;
            if deadline > now {
                crate::pace::sleep(deadline - now);
            } else {
                // Browser (SPEC-068): yield every frame even when late.
                crate::pace::yield_now();
                if now - deadline > frame_time * MAX_LAG_FRAMES {
                    deadline = now;
                }
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
    touch_only: bool,
) {
    p.fill(0.0, 0.0, 240.0, 160.0, [0, 0, 24, 200]);
    p.text(6.0, 4.0, title, [255, 220, 120]);
    // PixAntiqua is 12 px tall, so the old 15 rows at 9 px no longer fit the
    // 16..150 area: 11 rows of 12 px fill it exactly (SPEC-069 L10N-69-05).
    const ROW: f32 = crate::font::LINE_HEIGHT;
    const VISIBLE: usize = 11;
    let start = cursor.saturating_sub(VISIBLE - 1);
    for (i, row) in rows.iter().enumerate().skip(start).take(VISIBLE) {
        let y = 16.0 + (i - start) as f32 * ROW;
        if i == cursor {
            p.fill(2.0, y, 236.0, ROW, [80, 80, 160, 255]);
        }
        p.text(8.0, y, &row.label, [255, 255, 255]);
        if !row.value.is_empty() {
            // Right-align by measured width, not by character count: the
            // menu font is proportional (SPEC-069 L10N-69-05).
            let mut value = String::new();
            let mut width = 0.0;
            for ch in row.value.chars() {
                let advance = f32::from(crate::font::glyph(ch).advance);
                if width + advance > 108.0 {
                    break;
                }
                value.push(ch);
                width += advance;
            }
            p.text((236.0 - width).max(120.0), y, &value, [170, 220, 255]);
        }
    }
    // Touch devices have no Esc or right click: the footer tells them how the
    // menu is reached instead of naming keys they do not have
    // (SPEC-069 L10N-69-06).
    let default_footer = if touch_only {
        tr!("menu.hint_touch")
    } else {
        tr!("menu.hint")
    };
    let footer = remap_hint.or(status).unwrap_or(&default_footer);
    p.fill(0.0, 148.0, 240.0, 12.0, [0, 0, 0, 220]);
    p.text(4.0, 148.5, footer, [200, 200, 200]);
}

/// Semi-transparent on-screen gamepad; pressed controls are highlighted.
fn draw_touch(p: &mut sdl::Painter<'_>, pressed: u16) {
    use gba_core::keys;
    let alpha = |on: bool| if on { 150 } else { 60 };
    let (cx, cy, r) = touch::DPAD;
    let arm = r * 0.55;
    for (key, x, y) in [
        (keys::UP, cx - arm / 2.0, cy - r),
        (keys::DOWN, cx - arm / 2.0, cy + r - arm),
        (keys::LEFT, cx - r, cy - arm / 2.0),
        (keys::RIGHT, cx + r - arm, cy - arm / 2.0),
    ] {
        p.fill(x, y, arm, arm, [255, 255, 255, alpha(pressed & key != 0)]);
    }
    for b in &touch::BUTTONS {
        let on = pressed & b.key != 0;
        let (x, y, w, h) = match b.shape {
            touch::Shape::Circle { x, y, r } => (x - r, y - r, r * 2.0, r * 2.0),
            touch::Shape::Rect { x, y, w, h } => (x, y, w, h),
        };
        p.fill(x, y, w, h, [255, 255, 255, alpha(on)]);
        let tw = p.text_width(b.label);
        p.text(
            x + (w - tw) / 2.0,
            y + (h - crate::font::LINE_HEIGHT) / 2.0,
            b.label,
            [20, 20, 40],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The menu must fit the guest screen with the current font. This guards
    /// the 15-rows-at-9px layout that stopped fitting at 12 px.
    #[test]
    fn menu_rows_fit_the_guest_screen() {
        let _locale = crate::locale_guard::lock();
        const VISIBLE: f32 = 11.0;
        let top = 16.0;
        let footer_top = 148.0;
        let bottom = top + VISIBLE * crate::font::LINE_HEIGHT;
        assert!(
            bottom <= footer_top,
            "menu rows end at {bottom}, over the footer at {footer_top}"
        );
        assert_eq!(crate::font::LINE_HEIGHT, 12.0);
        // Every row of every page, in every language, must fit between the
        // 8 px left margin and the right-aligned value column.
        for lang in gba_i18n::Lang::all() {
            gba_i18n::set_lang(*lang);
            for page in [Page::Main, Page::Video, Page::Audio, Page::Controls] {
                let mut m = crate::menu::Menu::new(Settings::default());
                m.open_at(page, &Settings::default());
                for row in m.rows() {
                    let w = crate::font::width(&row.label);
                    assert!(
                        w < 240.0 - 8.0,
                        "{} row {:?} is {w} px wide ({})",
                        lang.tag(),
                        row.label,
                        page as u8
                    );
                }
            }
        }
    }

    /// Collects every label in the native menu tree.
    fn titles(items: &[CtxItem]) -> Vec<String> {
        let mut out = Vec::new();
        for item in items {
            match item {
                CtxItem::Action { title, .. } => out.push(title.clone()),
                CtxItem::Submenu { title, items } => {
                    out.push((*title).to_owned());
                    out.extend(titles(items));
                }
                CtxItem::Separator => {}
            }
        }
        out
    }

    #[test]
    fn context_menu_is_translated() {
        let _locale = crate::locale_guard::lock();
        let s = Settings::default();
        for (lang, wants) in [
            (
                gba_i18n::Lang::PtBr,
                ["Vídeo", "Som", "Controles", "Tela cheia", "Mudo", "Sair"],
            ),
            (
                gba_i18n::Lang::EnUs,
                ["Video", "Sound", "Controls", "Fullscreen", "Mute", "Quit"],
            ),
        ] {
            gba_i18n::set_lang(lang);
            let all = titles(&context_items(&s, false));
            // "Sistema"/"System" is a menu-bar entry, not a right-click one.
            for want in wants {
                assert!(all.iter().any(|t| t == want), "missing {want} in {all:?}");
            }
        }
        // Accented text is fine here: NSMenu renders full Unicode.
        gba_i18n::set_lang(gba_i18n::Lang::PtBr);
        let all = titles(&context_items(&s, false));
        assert!(all.iter().any(|t| t == "Correção de cor do LCD"));
    }

    #[test]
    fn pause_verb_follows_the_state() {
        let _locale = crate::locale_guard::lock();
        let s = Settings::default();
        let paused = titles(&context_items(&s, true));
        assert!(paused.iter().any(|t| t == "Continuar"));
        assert!(
            titles(&context_items(&s, false))
                .iter()
                .any(|t| t == "Pausar")
        );
        gba_i18n::set_lang(gba_i18n::Lang::EnUs);
        assert!(
            titles(&context_items(&s, true))
                .iter()
                .any(|t| t == "Resume")
        );
        assert!(
            titles(&context_items(&s, false))
                .iter()
                .any(|t| t == "Pause")
        );
    }

    #[test]
    fn menubar_has_a_system_menu() {
        let _locale = crate::locale_guard::lock();
        let all = titles(&menubar_items(&Settings::default(), false));
        assert!(all.iter().any(|t| t == "Sistema"));
        // "..." is three ASCII dots on purpose: PixAntiqua has no U+2026.
        assert!(all.iter().any(|t| t == "Menu na janela..."));
        gba_i18n::set_lang(gba_i18n::Lang::EnUs);
        let all = titles(&menubar_items(&Settings::default(), false));
        assert!(all.iter().any(|t| t == "System"));
        assert!(all.iter().any(|t| t == "In-window menu..."));
    }

    /// Selecting a language in the native menu must change the language and
    /// return settings for the caller to persist, like the in-window picker.
    #[test]
    fn native_menu_language_selection_applies_and_persists() {
        let _locale = crate::locale_guard::lock();
        let s = Settings::default();
        // The language submenu lists every language, in `Lang::all()` order.
        let all = titles(&context_items(&s, false));
        assert!(all.iter().any(|t| t == "Idioma"));
        assert!(all.iter().any(|t| t == "English (US)"));
        let id = A_LANGUAGE + 1;
        match context_effect(id, &s) {
            Effect::Settings(n) => assert_eq!(n.language, gba_i18n::Lang::EnUs),
            _ => panic!("selecting a language must return the settings to persist"),
        }
        assert_eq!(gba_i18n::lang(), gba_i18n::Lang::EnUs, "applied at once");
    }

    /// The settings window writes `language = "en-US"`; the running game must
    /// switch without a restart, and keep the change when the menu is applied
    /// (SPEC-069 L10N-69-03).
    #[test]
    fn a_language_written_by_the_settings_window_is_adopted() {
        let _locale = crate::locale_guard::lock();
        let mut m = crate::menu::Menu::new(Settings::default());
        m.open_at(Page::Main, &Settings::default());
        let mut loaded = Settings::default();
        assert_eq!(
            gba_i18n::lang(),
            gba_i18n::Lang::PtBr,
            "starts in the default"
        );

        loaded.language = gba_i18n::Lang::EnUs;
        assert!(adopt_language(&loaded, &mut m), "the change is reported");
        assert_eq!(gba_i18n::lang(), gba_i18n::Lang::EnUs);
        assert_eq!(m.draft.language, gba_i18n::Lang::EnUs, "draft follows");
        assert_eq!(m.effective_language(), gba_i18n::Lang::EnUs);

        // The same file again must be a no-op, not a repeated "changed".
        assert!(!adopt_language(&loaded, &mut m));
    }

    #[test]
    fn android_back_opens_the_menu_and_navigates_back() {
        assert!(opens_menu(scancode::AC_BACK));
        assert_eq!(nav_for_key(scancode::AC_BACK), Some(Nav::Back));
        // Desktop routes keep working.
        assert!(opens_menu(scancode::ESCAPE) && opens_menu(scancode::F1));
        assert!(
            !opens_menu(scancode::Z),
            "a play key must not open the menu"
        );
    }

    #[test]
    fn menu_navigation_covers_both_devices() {
        assert_eq!(nav_for_key(scancode::UP), Some(Nav::Up));
        assert_eq!(nav_for_key(scancode::RETURN), Some(Nav::Confirm));
        assert_eq!(nav_for_key(scancode::ESCAPE), Some(Nav::Back));
        assert_eq!(nav_for_key(9999), None);
    }

    /// The Android back button must never be captured as a game binding; it
    /// behaves like Esc during remap (cancel). Capturing it would leave the
    /// menu unreachable on a touch device.
    #[test]
    fn back_button_is_not_remappable() {
        assert!(!captures_key(scancode::AC_BACK));
        assert!(!captures_key(scancode::ESCAPE));
        assert!(captures_key(scancode::Z), "ordinary keys are capturable");
        assert!(captures_key(scancode::RETURN));
    }
}

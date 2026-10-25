//! Desktop application loop shared by the interpreted binary and the
//! recompiled package runtime (SPEC-061 FE-61-01, SPEC-063).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gba_core::{BackupKind, Gba, GbaConfig, NativeExec, SCREEN_HEIGHT};

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
    /// Experimental session override of the presentation width (SPEC-069
    /// WS-69-18). Never persisted.
    pub view_width: Option<u16>,
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
    /// Surface width actually presented at exit (SPEC-069 WS-69-17).
    pub view_width: u16,
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
// 25..=28 → 240, 288, 320, 384 (SPEC-069).
const A_VIEW_WIDTH: u32 = 25;
const VIEW_WIDTHS: [u16; 4] = [240, 288, 320, 384];
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

/// Launch `gba-settings` (EXT-64-02): next to this executable, then PATH.
fn open_external_settings(
    settings_path: Option<&Path>,
    view_override: Option<u16>,
) -> Result<(), String> {
    let path = settings_path.ok_or("no settings file location")?;
    // The settings window is a separate process with no access to this
    // invocation's flags, so pass the session override along for display
    // (SPEC-069 WS-69-15). It is advisory: the file itself is unchanged.
    let width = view_override.map(|w| w.to_string());
    let mut args: Vec<&std::ffi::OsStr> =
        vec![std::ffi::OsStr::new("--settings"), path.as_os_str()];
    if let Some(w) = width.as_deref() {
        args.push(std::ffi::OsStr::new("--session-view-width"));
        args.push(std::ffi::OsStr::new(w));
    }
    open_companion("gba-settings", &args)
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

/// macOS menu bar (CTX-63-02): app menu entries and top-level menus.
const APP_MENU: [(u32, &str, &str); 2] = [
    (A_EXTERNAL, "Configurações…", ","),
    (A_PARTY, "Party em rede…", "p"),
];

fn menubar_items(s: &Settings, paused: bool) -> Vec<CtxItem> {
    let mut items: Vec<CtxItem> = context_items(s, paused)
        .into_iter()
        .filter(|i| matches!(i, CtxItem::Submenu { .. }))
        .collect();
    items.push(submenu(
        "Sistema",
        vec![
            action(A_PAUSE, if paused { "Continuar" } else { "Pausar" }, false),
            action(A_RESET, "Reiniciar jogo", false),
            action(A_SAVE, "Salvar agora", false),
            action(A_MENU, "Menu na janela…", false),
        ],
    ));
    items
}

fn context_items(s: &Settings, paused: bool) -> Vec<CtxItem> {
    context_items_with(s, paused, None)
}

/// `override_width` is the session override, if any (SPEC-069 WS-69-18).
fn context_items_with(s: &Settings, paused: bool, override_width: Option<u16>) -> Vec<CtxItem> {
    let v = &s.video;
    let shown_width = override_width.unwrap_or(v.view_width);
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
                submenu(
                    "Largura da imagem",
                    VIEW_WIDTHS
                        .iter()
                        .enumerate()
                        .map(|(i, w)| {
                            let label = if *w == 240 {
                                "240 px (nativo)".to_owned()
                            } else if Some(*w) == override_width {
                                format!("{w} px (experimental, forcado)")
                            } else {
                                format!("{w} px (experimental)")
                            };
                            action(A_VIEW_WIDTH + i as u32, label, shown_width == *w)
                        })
                        .collect(),
                ),
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
        action(A_PARTY, "Party em rede…", false),
        action(A_MENU, "Menu na janela…", false),
        ctxmenu::CtxItem::Separator,
        action(A_QUIT, "Sair", false),
    ]
}

enum Effect {
    None,
    /// Settings to apply, and whether the presentation width row was edited
    /// explicitly (which supersedes the session override, WS-69-18).
    Settings(Settings, bool),
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
    let mut width_edited = false;
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
        25..=28 => {
            n.video.view_width = VIEW_WIDTHS[(id - A_VIEW_WIDTH) as usize];
            width_edited = true;
        }
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
        _ => return Effect::None,
    }
    Effect::Settings(n, width_edited)
}

struct Presentation {
    lut: Option<Vec<u32>>,
}

/// Settings as the UI should present them: the persisted preference with the
/// session override folded in, so menu rows and the actual surface agree
/// (SPEC-069 WS-69-15). Never written to disk on its own.
fn effective_settings(settings: &Settings, override_width: Option<u16>) -> Settings {
    let mut s = settings.clone();
    if let Some(w) = override_width {
        s.video.view_width = w;
    }
    s
}

/// The surface width a settings snapshot produces (SPEC-069 WS-69-06/18).
fn surface_width(settings: &Settings, ceiling: Option<u16>, session_override: Option<u16>) -> u16 {
    settings::effective_view_width(settings.video.view_width, ceiling, session_override).0
}

/// Retarget the guest renderer to `width`, the surface the host presents.
///
/// Called after any settings change so a live "Largura" edit takes effect
/// without a restart (WS-69-13), and after a reset so the rebuilt machine
/// keeps the active surface. A failure is reported, never fatal.
fn apply_view(gba: &mut Gba, width: u16) {
    let extra = width.saturating_sub(240);
    let view = gba_core::View {
        left: extra / 2,
        right: extra - extra / 2,
    };
    if gba.view() != view {
        if let Err(e) = gba.set_view(view) {
            eprintln!("gba-desktop: view not applied ({e})");
        }
    }
}

/// Keep the host pixel buffer matched to the active surface.
///
/// The presentation width can change while the game runs, so this must not be
/// allocated only once (WS-69-13).
fn ensure_pixels(pixels: &mut Vec<u32>, gba: &Gba) {
    let need = surface(gba).len();
    if pixels.len() != need {
        pixels.resize(need, 0);
    }
}

/// Current presentation surface: the widened buffer when margins are active,
/// otherwise the canonical framebuffer (SPEC-069 WS-69-01/WS-69-13).
fn surface(gba: &Gba) -> &[u16] {
    if gba.view().is_none() {
        gba.framebuffer()
    } else {
        gba.view_framebuffer()
    }
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
    let mut rom = match &o.rom_bytes {
        Some(bytes) => bytes.clone(),
        None => fs::read(&o.rom).map_err(|e| format!("cannot read ROM: {e}"))?,
    };
    // SPEC-070: resolve the profile for this exact image and apply its
    // verified patches before the machine is built. A mismatch aborts here.
    let profile_ceiling = settings::prepare_rom(&mut rom)?;
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
    // SPEC-069: resolve the presentation width from the persisted preference,
    // the per-game ceiling and the session override, then apply it to the
    // guest renderer. Nothing here reaches `frame_hash` (WS-69-04).
    // WS-69-19/PATCH-70-10: the ceiling comes from the profile matched by the
    // exact ROM SHA-256; absent profile means native 240.
    let ceiling = profile_ceiling;
    let mut session_override = o.view_width;
    let (view_width, view_source) =
        settings::effective_view_width(loaded.settings.video.view_width, ceiling, session_override);
    if session_override.is_none()
        && let Some(c) = ceiling
        && c < loaded.settings.video.view_width
    {
        eprintln!("gba-desktop: view width capped by game profile: {c}");
    }
    let view = |width: u16| gba_core::View {
        left: (width - 240) / 2,
        right: width - 240 - (width - 240) / 2,
    };
    let requested_view = view(view_width);
    gba.set_view(requested_view)
        .map_err(|e| format!("invalid view width: {e}"))?;
    if view_source == settings::ViewWidthSource::SessionOverride && view_width > 240 {
        eprintln!(
            "gba-desktop: EXPERIMENTAL wide surface {} px (session override; margins are \
             hardware wrap/backdrop, not new game content)",
            requested_view.width()
        );
    }
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
    // The texture is the presentation surface; guest coordinates stay 240x160.
    let (w, h) = (requested_view.width() as i32, SCREEN_HEIGHT as i32);
    // The painter draws in guest coordinates, so it needs to know where guest
    // column 0 sits on the presented surface (WS-69-13).
    let mut video = ctx.create_video(
        &o.title,
        w,
        h,
        i32::from(current.video.scale),
        i32::from(requested_view.left),
    )?;
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
    let has_touchscreen = ctx.has_touchscreen();
    let mut touch_seen = false;

    let mut bindings = Bindings::default();
    let mut presentation = Presentation { lut: None };
    let mut applied: Option<Settings> = None;
    let mut applied_surface: Option<u16> = None;
    let mut apply = |s: &Settings,
                     width: u16,
                     video: &mut sdl::Video<'_>,
                     audio: &mut Option<sdl::Audio<'_>>,
                     bindings: &mut Bindings,
                     presentation: &mut Presentation,
                     gpio: &mut Option<GpioInput>|
     -> bool {
        let prev = applied.clone();
        let v = &s.video;
        video.set_presentation(v.filter == Filter::Linear, v.integer_scale, v.vsync);
        if prev.as_ref().map(|p| p.video.fullscreen) != Some(v.fullscreen) {
            video.set_fullscreen(v.fullscreen);
        }
        if prev.as_ref().map(|p| p.video.scale) != Some(v.scale) && !v.fullscreen {
            video.set_scale(i32::from(v.scale));
        }
        // Presentation width (SPEC-069): recreate the surface in place so the
        // change is live; the guest view is retargeted by the caller only when
        // the texture accepted the new width, so both stay matched.
        if applied_surface != Some(width) {
            let extra = width.saturating_sub(240);
            match video.set_view_width(i32::from(width), i32::from(extra / 2)) {
                Ok(()) => applied_surface = Some(width),
                Err(e) => eprintln!("gba-desktop: view width not applied ({e})"),
            }
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
        applied_surface == Some(width)
    };
    // Branding (FR-63-03): window/Dock icon and a 2.4 s logo splash.
    match sdl::Surface::from_png(crate::assets::ICON_PNG) {
        Ok(icon) => video.set_icon(&icon),
        Err(e) => eprintln!("gba-desktop: icon unavailable ({e})"),
    }
    apply(
        &current,
        view_width,
        &mut video,
        &mut audio,
        &mut bindings,
        &mut presentation,
        &mut gpio,
    );
    apply_view(&mut gba, view_width);

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

    let mut menubar_state: Option<(Settings, bool)> = None;

    let frame_time = Duration::from_nanos(CYCLES_PER_FRAME * 1_000_000_000 / GUEST_HZ);
    let mut report = AppReport {
        audio_on: audio.is_some(),
        ..AppReport::default()
    };
    let mut pixels = vec![0u32; requested_view.width() * SCREEN_HEIGHT];
    let mut deadline = Instant::now();
    let mut last_present = Instant::now() - frame_time;
    let mut last_save = Instant::now();
    let mut fault = None;
    let mut menu = Menu::new(effective_settings(&current, session_override));
    menu.set_view_override(session_override, current.video.view_width);
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
                            effect = Effect::Settings(s, false);
                        }
                        continue;
                    }
                    if sc == scancode::F11 && !repeat {
                        let mut n = current.clone();
                        n.video.fullscreen = !n.video.fullscreen;
                        effect = Effect::Settings(n, false);
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
                                effect = Effect::Settings(s, false);
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
                        let items = context_items_with(&current, paused, session_override);
                        if let Some(id) = ctxmenu::popup(&items) {
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
                        }
                        None => {
                            fingers.remove(&finger);
                        }
                    }
                }
                Event::FingerUp { finger } => {
                    fingers.remove(&finger);
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
            ctxmenu::install_menubar(&APP_MENU, &menubar_items(&current, paused));
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
                Command::Apply(s, width_edited) => effect = Effect::Settings(s, width_edited),
            }
        }
        match effect {
            Effect::None => {}
            Effect::Settings(s, width_edited) => {
                // An explicit edit of the width row supersedes the session
                // override for the rest of the session (WS-69-18).
                if width_edited {
                    session_override = None;
                }
                let width = surface_width(&s, ceiling, session_override);
                if apply(
                    &s,
                    width,
                    &mut video,
                    &mut audio,
                    &mut bindings,
                    &mut presentation,
                    &mut gpio,
                ) {
                    apply_view(&mut gba, width);
                }
                let failure = persist_settings(&s, &mut settings_invalid_on_disk, &mut known_mtime);
                status = Some((
                    failure.unwrap_or_else(|| "Configuracoes salvas".into()),
                    Instant::now(),
                ));
                current = s;
            }
            Effect::OpenMenu(page) => {
                menu.open_at(page, &current);
                menu.set_view_override(session_override, current.video.view_width);
            }
            Effect::TogglePause => paused = !paused,
            Effect::OpenParty => {
                let msg = match open_companion("gba-party", &[]) {
                    Ok(()) => "Party aberta".to_owned(),
                    Err(e) => {
                        eprintln!("gba-desktop: {e}");
                        "gba-party nao encontrado".to_owned()
                    }
                };
                status = Some((msg, Instant::now()));
            }
            Effect::OpenExternal => {
                let msg = match open_external_settings(settings_path.as_deref(), session_override) {
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
                // The rebuilt machine starts with `View::NONE`; keep it on the
                // surface that is actually being presented.
                let width = u16::try_from(gba.view().width()).unwrap_or(240);
                gba = new_gba(Some(&backup));
                gba.bus.backup.dirty = dirty;
                apply_view(&mut gba, width);
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
                            let width = surface_width(&loaded.settings, ceiling, session_override);
                            if apply(
                                &loaded.settings,
                                width,
                                &mut video,
                                &mut audio,
                                &mut bindings,
                                &mut presentation,
                                &mut gpio,
                            ) {
                                apply_view(&mut gba, width);
                            }
                            current = loaded.settings;
                            if !menu.open {
                                menu = Menu::new(effective_settings(&current, session_override));
                                menu.set_view_override(session_override, current.video.view_width);
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
            ensure_pixels(&mut pixels, &gba);
            presentation.convert(surface(&gba), &mut pixels);
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
                    p.fill_surface(0.0, 14.0, [0, 0, 0, 180]);
                    p.text(4.0, 3.0, "PAUSADO - botao direito: menu", [255, 255, 255]);
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
            ensure_pixels(&mut pixels, &gba);
            presentation.convert(surface(&gba), &mut pixels);
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
    report.view_width = u16::try_from(gba.view().width()).unwrap_or(240);
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
    // Cover the whole presented surface: leaving the margins undimmed makes
    // the menu look off-centre on a widened surface (WS-69-13).
    p.fill_surface(0.0, 160.0, [0, 0, 24, 200]);
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
    p.fill_surface(150.0, 10.0, [0, 0, 0, 220]);
    p.text(4.0, 151.0, footer, [200, 200, 200]);
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
        let tx = x + (w - 8.0 * b.label.len() as f32) / 2.0;
        p.text(tx, y + (h - 8.0) / 2.0, b.label, [20, 20, 40]);
    }
}

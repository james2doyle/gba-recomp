//! Host settings (SPEC-063 SET-63-01). Pure data: no SDL here. Settings
//! never reach the guest (SET-63-02).

use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 1;

/// GBA buttons in KEYINPUT bit order.
pub const BUTTONS: [&str; 10] = [
    "a", "b", "select", "start", "right", "left", "up", "down", "r", "l",
];

/// Display names for the in-window menu (ASCII: the SDL debug font).
pub const BUTTON_LABELS: [&str; 10] = [
    "A", "B", "Select", "Start", "Direita", "Esquerda", "Cima", "Baixo", "R", "L",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Linear,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Video {
    pub scale: u8,
    pub fullscreen: bool,
    pub filter: Filter,
    pub integer_scale: bool,
    pub vsync: bool,
    pub color_correction: bool,
    /// Requested presentation width (SPEC-069). 240 = native, no margins.
    /// The effective width is clamped by the per-game ceiling and may be
    /// overridden for one session; see [`effective_view_width`].
    pub view_width: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    pub volume: u8,
    pub mute: bool,
    pub latency_ms: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    /// SDL scancode names per GBA button (empty = unbound).
    pub keyboard: [Vec<String>; 10],
    /// SDL gamepad button strings per GBA button.
    pub gamepad: [Vec<String>; 10],
    /// Generic (non-gamepad) joystick controls: `bN`, `hN<up|down|left|right>`,
    /// `aN+`/`aN-` (DEV-64-02).
    pub joystick: [Vec<String>; 10],
    pub deadzone: f32,
    /// On-screen gamepad (SPEC-068 TOUCH-68-01).
    pub touch_controls: TouchMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchMode {
    /// Shown when a touch screen is present.
    Auto,
    On,
    Off,
}

impl TouchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            TouchMode::Auto => "auto",
            TouchMode::On => "on",
            TouchMode::Off => "off",
        }
    }
    /// Default: automatic, except on the Web where it is opt-in.
    pub fn platform_default() -> Self {
        if cfg!(target_os = "emscripten") {
            TouchMode::Off
        } else {
            TouchMode::Auto
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
    PullUp,
    PullDown,
    Disabled,
}

impl Bias {
    pub fn as_str(self) -> &'static str {
        match self {
            Bias::PullUp => "pull-up",
            Bias::PullDown => "pull-down",
            Bias::Disabled => "disabled",
        }
    }
}

/// Buttons wired to GPIO lines (Linux gpiochip uAPI v2, GPIO-64-01).
#[derive(Clone, Debug, PartialEq)]
pub struct Gpio {
    pub enabled: bool,
    pub chip: String,
    pub active_low: bool,
    pub bias: Bias,
    pub debounce_ms: u32,
    /// Line offset per GBA button (`None` = no pin).
    pub lines: [Option<u32>; 10],
}

impl Default for Gpio {
    fn default() -> Self {
        Self {
            enabled: false,
            chip: "/dev/gpiochip0".into(),
            active_low: true,
            bias: Bias::PullUp,
            debounce_ms: 10,
            lines: [None; 10],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub video: Video,
    pub audio: Audio,
    pub input: Input,
    pub gpio: Gpio,
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

impl Default for Input {
    fn default() -> Self {
        Self {
            keyboard: [
                strings(&["Z"]),
                strings(&["X"]),
                strings(&["Backspace", "Right Shift"]),
                strings(&["Return"]),
                strings(&["Right"]),
                strings(&["Left"]),
                strings(&["Up"]),
                strings(&["Down"]),
                strings(&["S"]),
                strings(&["A"]),
            ],
            gamepad: [
                strings(&["a"]),
                strings(&["b"]),
                strings(&["back"]),
                strings(&["start"]),
                strings(&["dpright"]),
                strings(&["dpleft"]),
                strings(&["dpup"]),
                strings(&["dpdown"]),
                strings(&["rightshoulder"]),
                strings(&["leftshoulder"]),
            ],
            joystick: [
                strings(&["b0"]),
                strings(&["b1"]),
                strings(&["b8"]),
                strings(&["b9"]),
                strings(&["h0right", "a0+"]),
                strings(&["h0left", "a0-"]),
                strings(&["h0up", "a1-"]),
                strings(&["h0down", "a1+"]),
                strings(&["b5"]),
                strings(&["b4"]),
            ],
            deadzone: 0.25,
            touch_controls: TouchMode::platform_default(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            video: Video {
                scale: 3,
                fullscreen: false,
                filter: Filter::Nearest,
                integer_scale: true,
                vsync: true,
                color_correction: false,
                view_width: VIEW_WIDTH_RANGE.0,
            },
            audio: Audio {
                volume: 100,
                mute: false,
                latency_ms: 60,
            },
            input: Input::default(),
            gpio: Gpio::default(),
        }
    }
}

pub const SCALE_RANGE: (u8, u8) = (1, 8);
pub const LATENCY_RANGE: (u16, u16) = (20, 200);
/// Accepted `[video] view_width` range (SPEC-069 WS-69-05/WS-69-14).
pub const VIEW_WIDTH_RANGE: (u16, u16) = (240, 384);

/// Profile that applies to this image, if one is installed (PROF-02).
pub fn find_profile(rom_sha256: &str, rom_len: usize) -> Option<gba_profile::Profile> {
    gba_profile::find_profile(rom_sha256, rom_len)
}

/// Per-game presentation ceiling (SPEC-069 WS-69-19). `None` => native 240.
pub fn view_ceiling(rom_sha256: &str, rom_len: usize) -> Option<u16> {
    find_profile(rom_sha256, rom_len).and_then(|p| p.view_ceiling())
}

/// Apply this image's profile patches and return its ceiling (SPEC-070).
///
/// Logs every applied patch; a byte mismatch aborts the caller.
pub fn prepare_rom(rom: &mut [u8]) -> Result<Option<u16>, String> {
    let prepared = gba_profile::prepare_rom(rom)?;
    for patch in &prepared.applied {
        eprintln!(
            "gba-recomp: {} ({})",
            patch,
            prepared.profile_id.as_deref().unwrap_or("?")
        );
    }
    if let (Some(id), Some(ceiling)) = (&prepared.profile_id, prepared.view_ceiling) {
        eprintln!("gba-recomp: profile {id} declares a presentation ceiling of {ceiling} px");
    }
    Ok(prepared.view_ceiling)
}

/// Which input decided the effective width, for diagnostics and the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewWidthSource {
    /// No margins: the game has no ceiling and nothing was overridden.
    Native,
    /// `min(preference, ceiling)`.
    Preference,
    /// An explicit, non-persisted experimental override (WS-69-18).
    SessionOverride,
}

/// Effective presentation width and why (SPEC-069 WS-69-06/WS-69-18).
///
/// `preference` is the persisted `[video] view_width`; `session_override` is
/// the explicit per-invocation value; `ceiling` is [`view_ceiling`]'s answer.
pub fn effective_view_width(
    preference: u16,
    ceiling: Option<u16>,
    session_override: Option<u16>,
) -> (u16, ViewWidthSource) {
    let (lo, hi) = VIEW_WIDTH_RANGE;
    let clamp_range = |w: u16| w.clamp(lo, hi);
    if let Some(forced) = session_override {
        return (clamp_range(forced), ViewWidthSource::SessionOverride);
    }
    let allowed = ceiling.map_or(lo, clamp_range);
    let wanted = clamp_range(preference).min(allowed);
    if wanted == lo {
        (lo, ViewWidthSource::Native)
    } else {
        (wanted, ViewWidthSource::Preference)
    }
}
pub const DEADZONE_MAX: f32 = 0.95;

/// Default location (SET-63-01).
pub fn default_path() -> Option<PathBuf> {
    // Android (SPEC-068): no HOME/XDG; use the app's internal storage.
    if cfg!(target_os = "android") {
        return android_pref_path().map(|d| d.join("settings.toml"));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    // On iOS `HOME` is the app sandbox container; Library is writable there.
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        home.map(|h| h.join("Library/Application Support/gba-recomp/settings.toml"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.map(|h| h.join(".config")))
            .map(|d| d.join("gba-recomp/settings.toml"))
    }
}

/// App-private writable directory on Android (`SDL_GetPrefPath`); `None`
/// elsewhere, where the HOME/XDG rules above apply.
pub fn android_pref_path() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        crate::sdl::pref_path()
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

// ------------------------------------------------------------------ format

#[derive(Debug, PartialEq)]
enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    List(Vec<String>),
}

fn parse_string(s: &str) -> Result<(String, &str), String> {
    let rest = s.strip_prefix('"').ok_or("expected string")?;
    let mut out = String::new();
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((out, &rest[i + 1..])),
            '\\' => match chars.next() {
                Some((_, '"')) => out.push('"'),
                Some((_, '\\')) => out.push('\\'),
                _ => return Err("unsupported escape".into()),
            },
            c => out.push(c),
        }
    }
    Err("unterminated string".into())
}

fn parse_value(text: &str) -> Result<Value, String> {
    let t = text.trim();
    if t.starts_with('"') {
        let (s, rest) = parse_string(t)?;
        if !rest.trim().is_empty() {
            return Err(format!("trailing text after string: {rest}"));
        }
        return Ok(Value::Str(s));
    }
    if let Some(inner) = t.strip_prefix('[') {
        let inner = inner.strip_suffix(']').ok_or("unterminated list")?;
        let mut items = Vec::new();
        let mut rest = inner.trim();
        while !rest.is_empty() {
            let (s, r) = parse_string(rest)?;
            items.push(s);
            rest = r.trim();
            if let Some(r) = rest.strip_prefix(',') {
                rest = r.trim();
            } else if !rest.is_empty() {
                return Err("expected `,` in list".into());
            }
        }
        return Ok(Value::List(items));
    }
    match t {
        "true" => return Ok(Value::Bool(true)),
        "false" => return Ok(Value::Bool(false)),
        _ => {}
    }
    if let Ok(i) = t.parse::<i64>() {
        return Ok(Value::Int(i));
    }
    if let Ok(f) = t.parse::<f64>() {
        return Ok(Value::Float(f));
    }
    Err(format!("invalid value `{t}`"))
}

fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

impl Settings {
    /// Parse `settings.toml`. Any error rejects the whole file (UI-07).
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut s = Settings::default();
        let mut section = String::new();
        let mut version = None;
        for (n, raw) in text.lines().enumerate() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            let err = |m: String| format!("line {}: {m}", n + 1);
            if let Some(name) = line.strip_prefix('[') {
                section = name
                    .strip_suffix(']')
                    .ok_or_else(|| err("unterminated section".into()))?
                    .trim()
                    .to_owned();
                if !matches!(section.as_str(), "video" | "audio" | "input" | "gpio") {
                    return Err(err(format!("unknown section [{section}]")));
                }
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| err("expected key = value".into()))?;
            let key = key.trim();
            let value = parse_value(value).map_err(err)?;
            s.set(&section, key, value, &mut version).map_err(err)?;
        }
        match version {
            Some(VERSION) => Ok(s),
            Some(v) => Err(format!("unsupported settings version {v}")),
            None => Err("missing `version`".into()),
        }
    }

    fn set(
        &mut self,
        section: &str,
        key: &str,
        value: Value,
        version: &mut Option<u32>,
    ) -> Result<(), String> {
        let int = |v: &Value, lo: i64, hi: i64| match v {
            Value::Int(i) if (lo..=hi).contains(i) => Ok(*i),
            _ => Err(format!("`{key}` must be an integer in {lo}..={hi}")),
        };
        let boolean = |v: &Value| match v {
            Value::Bool(b) => Ok(*b),
            _ => Err(format!("`{key}` must be true or false")),
        };
        match (section, key) {
            ("", "version") => *version = Some(int(&value, 0, 1_000)? as u32),
            ("video", "scale") => {
                self.video.scale = int(&value, SCALE_RANGE.0.into(), SCALE_RANGE.1.into())? as u8
            }
            ("video", "fullscreen") => self.video.fullscreen = boolean(&value)?,
            ("video", "filter") => {
                self.video.filter = match value {
                    Value::Str(ref s) if s == "nearest" => Filter::Nearest,
                    Value::Str(ref s) if s == "linear" => Filter::Linear,
                    _ => return Err("`filter` must be \"nearest\" or \"linear\"".into()),
                }
            }
            ("video", "integer_scale") => self.video.integer_scale = boolean(&value)?,
            ("video", "vsync") => self.video.vsync = boolean(&value)?,
            ("video", "color_correction") => self.video.color_correction = boolean(&value)?,
            ("video", "view_width") => {
                self.video.view_width =
                    int(&value, VIEW_WIDTH_RANGE.0.into(), VIEW_WIDTH_RANGE.1.into())? as u16
            }
            ("audio", "volume") => self.audio.volume = int(&value, 0, 100)? as u8,
            ("audio", "mute") => self.audio.mute = boolean(&value)?,
            ("audio", "latency_ms") => {
                self.audio.latency_ms =
                    int(&value, LATENCY_RANGE.0.into(), LATENCY_RANGE.1.into())? as u16
            }
            ("input", "deadzone") => {
                let d = match value {
                    Value::Float(f) => f as f32,
                    Value::Int(i) => i as f32,
                    _ => return Err("`deadzone` must be a number".into()),
                };
                if !(0.0..=DEADZONE_MAX).contains(&d) {
                    return Err(format!("`deadzone` must be in 0..={DEADZONE_MAX}"));
                }
                self.input.deadzone = d;
            }
            ("gpio", "enabled") => self.gpio.enabled = boolean(&value)?,
            ("gpio", "chip") => match value {
                Value::Str(s) if s.starts_with("/dev/") => self.gpio.chip = s,
                _ => return Err("`chip` must be a /dev/gpiochipN path".into()),
            },
            ("gpio", "active_low") => self.gpio.active_low = boolean(&value)?,
            ("gpio", "bias") => {
                self.gpio.bias = match value {
                    Value::Str(ref s) if s == "pull-up" => Bias::PullUp,
                    Value::Str(ref s) if s == "pull-down" => Bias::PullDown,
                    Value::Str(ref s) if s == "disabled" => Bias::Disabled,
                    _ => {
                        return Err(
                            "`bias` must be \"pull-up\", \"pull-down\" or \"disabled\"".into()
                        );
                    }
                }
            }
            ("gpio", "debounce_ms") => self.gpio.debounce_ms = int(&value, 0, 100)? as u32,
            ("gpio", k) => {
                let button = k
                    .strip_prefix("line_")
                    .ok_or_else(|| format!("unknown gpio key `{k}`"))?;
                let index = BUTTONS
                    .iter()
                    .position(|b| *b == button)
                    .ok_or_else(|| format!("unknown GBA button `{button}`"))?;
                self.gpio.lines[index] = Some(int(&value, 0, 1023)? as u32);
            }
            ("input", "touch_controls") => {
                self.input.touch_controls = match value {
                    Value::Str(ref s) if s == "auto" => TouchMode::Auto,
                    Value::Str(ref s) if s == "on" => TouchMode::On,
                    Value::Str(ref s) if s == "off" => TouchMode::Off,
                    _ => return Err("`touch_controls` must be \"auto\", \"on\" or \"off\"".into()),
                }
            }
            ("input", k) => {
                let (kind, button) = k
                    .split_once('_')
                    .ok_or_else(|| format!("unknown input key `{k}`"))?;
                let index = BUTTONS
                    .iter()
                    .position(|b| *b == button)
                    .ok_or_else(|| format!("unknown GBA button `{button}`"))?;
                let list = match value {
                    Value::List(l) => l,
                    Value::Str(s) => vec![s],
                    _ => return Err(format!("`{k}` must be a list of strings")),
                };
                match kind {
                    "key" => self.input.keyboard[index] = list,
                    "pad" => self.input.gamepad[index] = list,
                    "joy" => {
                        if let Some(bad) = list
                            .iter()
                            .find(|c| crate::input::JoyControl::parse(c).is_none())
                        {
                            return Err(format!("invalid joystick control `{bad}`"));
                        }
                        self.input.joystick[index] = list
                    }
                    _ => return Err(format!("unknown input key `{k}`")),
                }
            }
            _ => return Err(format!("unknown key `{key}` in [{section}]")),
        }
        Ok(())
    }

    pub fn serialize(&self) -> String {
        let mut o = String::new();
        let list = |l: &[String]| {
            let items: Vec<String> = l
                .iter()
                .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
                .collect();
            format!("[{}]", items.join(", "))
        };
        let v = &self.video;
        let a = &self.audio;
        let _ = writeln!(
            o,
            "# gba-recomp host settings (SPEC-063). Presentation only: never changes the game."
        );
        let _ = writeln!(o, "version = {VERSION}\n");
        let _ = writeln!(o, "[video]");
        let _ = writeln!(o, "scale = {}", v.scale);
        let _ = writeln!(o, "fullscreen = {}", v.fullscreen);
        let _ = writeln!(
            o,
            "filter = \"{}\"",
            if v.filter == Filter::Linear {
                "linear"
            } else {
                "nearest"
            }
        );
        let _ = writeln!(o, "integer_scale = {}", v.integer_scale);
        let _ = writeln!(o, "vsync = {}", v.vsync);
        let _ = writeln!(o, "color_correction = {}", v.color_correction);
        let _ = writeln!(o, "view_width = {}\n", v.view_width);
        let _ = writeln!(o, "[audio]");
        let _ = writeln!(o, "volume = {}", a.volume);
        let _ = writeln!(o, "mute = {}", a.mute);
        let _ = writeln!(o, "latency_ms = {}\n", a.latency_ms);
        let _ = writeln!(o, "[input]");
        let _ = writeln!(o, "deadzone = {:.2}", self.input.deadzone);
        let _ = writeln!(
            o,
            "touch_controls = \"{}\"",
            self.input.touch_controls.as_str()
        );
        for (i, b) in BUTTONS.iter().enumerate() {
            let _ = writeln!(o, "key_{b} = {}", list(&self.input.keyboard[i]));
        }
        for (i, b) in BUTTONS.iter().enumerate() {
            let _ = writeln!(o, "pad_{b} = {}", list(&self.input.gamepad[i]));
        }
        for (i, b) in BUTTONS.iter().enumerate() {
            let _ = writeln!(o, "joy_{b} = {}", list(&self.input.joystick[i]));
        }
        let g = &self.gpio;
        let _ = writeln!(o, "\n[gpio]");
        let _ = writeln!(o, "enabled = {}", g.enabled);
        let _ = writeln!(o, "chip = \"{}\"", g.chip.replace('"', ""));
        let _ = writeln!(o, "active_low = {}", g.active_low);
        let _ = writeln!(o, "bias = \"{}\"", g.bias.as_str());
        let _ = writeln!(o, "debounce_ms = {}", g.debounce_ms);
        for (i, b) in BUTTONS.iter().enumerate() {
            if let Some(line) = g.lines[i] {
                let _ = writeln!(o, "line_{b} = {line}");
            }
        }
        o
    }
}

/// Outcome of loading settings from disk.
pub struct Loaded {
    pub settings: Settings,
    /// Diagnostic to show when the file was invalid (UI-07).
    pub warning: Option<String>,
    /// The file on disk is invalid and must be backed up before saving.
    pub invalid_on_disk: bool,
}

pub fn load(path: &Path) -> Loaded {
    match fs::read_to_string(path) {
        Ok(text) => match Settings::parse(&text) {
            Ok(settings) => Loaded {
                settings,
                warning: None,
                invalid_on_disk: false,
            },
            Err(e) => Loaded {
                settings: Settings::default(),
                warning: Some(format!(
                    "{} is invalid ({e}); using defaults, the file was kept",
                    path.display()
                )),
                invalid_on_disk: true,
            },
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Loaded {
            settings: Settings::default(),
            warning: None,
            invalid_on_disk: false,
        },
        Err(e) => Loaded {
            settings: Settings::default(),
            warning: Some(format!(
                "cannot read {} ({e}); using defaults",
                path.display()
            )),
            invalid_on_disk: false,
        },
    }
}

/// Persist atomically. An invalid file is first preserved as
/// `settings.toml.invalid` (UI-07).
pub fn save(path: &Path, settings: &Settings, invalid_on_disk: bool) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    if invalid_on_disk && path.exists() {
        let backup = path.with_extension("toml.invalid");
        fs::copy(path, &backup).map_err(|e| {
            format!(
                "cannot preserve invalid settings as {}: {e}",
                backup.display()
            )
        })?;
    }
    let tmp = path.with_extension("toml.tmp");
    let mut f =
        fs::File::create(&tmp).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    f.write_all(settings.serialize().as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    drop(f);
    fs::rename(&tmp, path).map_err(|e| format!("cannot commit {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let s = Settings::default();
        assert_eq!(Settings::parse(&s.serialize()).unwrap(), s);
    }

    #[test]
    fn custom_values_round_trip() {
        let mut s = Settings::default();
        s.video.scale = 5;
        s.video.fullscreen = true;
        s.video.filter = Filter::Linear;
        s.audio.volume = 40;
        s.audio.mute = true;
        s.audio.latency_ms = 150;
        s.input.deadzone = 0.4;
        s.input.keyboard[0] = vec!["Space".into(), "Left Ctrl".into()];
        s.input.gamepad[3] = vec![];
        s.input.joystick[2] = vec!["b7".into(), "a2-".into()];
        s.gpio.enabled = true;
        s.gpio.bias = Bias::PullDown;
        s.gpio.debounce_ms = 25;
        s.gpio.lines[0] = Some(17);
        s.gpio.lines[9] = Some(26);
        assert_eq!(Settings::parse(&s.serialize()).unwrap(), s);
    }

    #[test]
    fn rejects_out_of_range_and_unknown() {
        let base = "version = 1\n";
        for bad in [
            "[video]\nscale = 0",
            "[video]\nscale = 9",
            "[audio]\nvolume = 101",
            "[audio]\nlatency_ms = 10",
            "[input]\ndeadzone = 0.99",
            "[input]\nkey_x = [\"Z\"]",
            "[video]\nfilter = \"bicubic\"",
            "[bogus]",
            "[video]\nscale = \"3\"",
            "[input]\njoy_a = [\"x3\"]",
            "[gpio]\nline_q = 3",
            "[gpio]\nchip = \"gpiochip0\"",
            "[gpio]\ndebounce_ms = 500",
        ] {
            assert!(Settings::parse(&format!("{base}{bad}")).is_err(), "{bad}");
        }
        assert!(
            Settings::parse("[video]\nscale = 3").is_err(),
            "missing version"
        );
        assert!(Settings::parse("version = 2").is_err());
    }

    #[test]
    fn partial_file_keeps_defaults_and_comments_are_ignored() {
        let s = Settings::parse("version = 1 # c\n[audio]\nmute = true # m\n").unwrap();
        assert!(s.audio.mute);
        assert_eq!(s.video, Settings::default().video);
    }

    #[test]
    fn invalid_file_is_preserved_on_save() {
        let dir = std::env::temp_dir().join(format!("gba-settings-{}", std::process::id()));
        let path = dir.join("settings.toml");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "garbage").unwrap();
        let loaded = load(&path);
        assert!(loaded.warning.is_some() && loaded.invalid_on_disk);
        assert_eq!(fs::read_to_string(&path).unwrap(), "garbage");
        save(&path, &loaded.settings, true).unwrap();
        assert_eq!(
            fs::read_to_string(path.with_extension("toml.invalid")).unwrap(),
            "garbage"
        );
        assert_eq!(
            Settings::parse(&fs::read_to_string(&path).unwrap()).unwrap(),
            Settings::default()
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod spec069 {
    use super::*;

    /// T-25: a file written before SPEC-069 loads with the native default.
    #[test]
    fn file_without_view_width_defaults_to_native() {
        let text = "version = 1\n\n[video]\nscale = 3\n";
        let s = Settings::parse(text).unwrap();
        assert_eq!(s.video.view_width, 240);
        assert_eq!(
            effective_view_width(s.video.view_width, None, None),
            (240, ViewWidthSource::Native)
        );
    }

    /// T-26: out-of-range values are refused, on both sides.
    #[test]
    fn out_of_range_view_width_is_refused() {
        for bad in ["385", "200", "0", "-240"] {
            let text = format!("version = 1\n\n[video]\nview_width = {bad}\n");
            assert!(
                Settings::parse(&text).is_err(),
                "view_width = {bad} must be refused"
            );
        }
    }

    /// T-27: a wide preference with no declared ceiling stays native.
    #[test]
    fn preference_without_ceiling_stays_native() {
        // No installed profile for this hash => no ceiling => native 240.
        assert_eq!(view_ceiling("00ff", 16), None, "no profile installed");
        assert_eq!(
            effective_view_width(384, view_ceiling("00ff", 16), None),
            (240, ViewWidthSource::Native)
        );
    }

    /// T-30: the ceiling clamps the preference and the reason is reported.
    #[test]
    fn ceiling_clamps_preference() {
        assert_eq!(
            effective_view_width(320, Some(288), None),
            (288, ViewWidthSource::Preference)
        );
        // A ceiling below the native minimum cannot shrink the screen.
        assert_eq!(
            effective_view_width(320, Some(100), None),
            (240, ViewWidthSource::Native)
        );
    }

    /// T-29: a session override wins over the ceiling, and never persists.
    #[test]
    fn session_override_bypasses_ceiling() {
        assert_eq!(
            effective_view_width(240, Some(240), Some(320)),
            (320, ViewWidthSource::SessionOverride)
        );
        // The override is still bounded by the hard maximum.
        assert_eq!(
            effective_view_width(240, None, Some(9999)),
            (384, ViewWidthSource::SessionOverride)
        );
    }

    /// Round-trip: the new key survives save/parse.
    #[test]
    fn view_width_round_trips() {
        let mut s = Settings::default();
        s.video.view_width = 320;
        let text = s.serialize();
        assert!(text.contains("view_width = 320"), "{text}");
        let back = Settings::parse(&text).unwrap();
        assert_eq!(back.video.view_width, 320);
        assert_eq!(back, s);
    }

    /// SPEC-070: a profile declaring a ceiling is discovered by exact hash and
    /// exposes that ceiling through the SPEC-069 resolver.
    #[test]
    fn profile_ceiling_is_discovered_by_hash() {
        let dir = std::env::temp_dir().join(format!("gba-profiles-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let sha = "99ae825ffb1b7e8b48b33cd2715ceb8ca524858f99fee4b22938489460083d94";
        fs::write(
            dir.join("rk.toml"),
            format!(
                "format = \"gba-recomp-profile\"\nversion = 1\nid = \"rk\"\nstatus = \"draft\"\n\n\
                 [image]\nsha256 = \"{sha}\"\nsize = 183248\n\n[presentation]\nview_ceiling = 288\n"
            ),
        )
        .unwrap();
        let found = gba_profile::find_profile_in(&dir, sha, 183248);
        assert_eq!(
            found.as_ref().and_then(|p| p.view_ceiling()),
            Some(288),
            "profile discovered in an explicit directory"
        );
        assert_eq!(view_ceiling(sha, 183248), None, "no installed profile");
        assert_eq!(
            gba_profile::find_profile_in(&dir, sha, 1),
            None,
            "size mismatch is not a match"
        );
        assert_eq!(
            gba_profile::find_profile_in(&dir, &"0".repeat(64), 183248),
            None,
            "different hash is not a match"
        );
        let ceiling = found.and_then(|p| p.view_ceiling());
        assert_eq!(
            effective_view_width(320, ceiling, None),
            (288, ViewWidthSource::Preference),
            "the ceiling clamps the preference"
        );
        assert_eq!(
            effective_view_width(320, ceiling, Some(320)),
            (320, ViewWidthSource::SessionOverride),
            "--view-width still wins (WS-69-18)"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}

//! External settings window (SPEC-064 EXT-64-01): edits the same
//! `settings.toml` as the game, which reloads it live. egui/eframe UI; SDL is
//! used only for input-device discovery and capture.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use gba_desktop::gpio;
use gba_desktop::input::JoyControl;
use gba_desktop::sdl::{self, Event};
use gba_desktop::settings::{self, BUTTON_LABEL_KEYS, Bias, Filter, Settings, button_label};
use gba_i18n::{Lang, tr};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Video,
    Audio,
    Controls,
    Gpio,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Device {
    Keyboard,
    Gamepad,
    Joystick,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Capture {
    button: usize,
    device: Device,
}

struct App {
    path: PathBuf,
    effective: Settings,
    draft: Settings,
    invalid_on_disk: bool,
    /// Whether the file records an explicit `language` choice. While it does
    /// not, saves must not write the key (SPEC-069 L10N-69-04): its absence
    /// is what makes both windows follow the system language.
    language_explicit: bool,
    tab: Tab,
    status: Option<(String, Instant)>,
    sdl: Option<&'static sdl::Context>,
    pads: Vec<sdl::Gamepad<'static>>,
    joys: Vec<sdl::Joystick<'static>>,
    capture: Option<Capture>,
    gpio_test: bool,
    gpio_values: Vec<(usize, bool)>,
    gpio_error: Option<String>,
    /// Mtime of the file as this window last knew it, so a change written by
    /// the game is noticed (SPEC-069 L10N-69-03, EXT-64-02).
    known_mtime: Option<std::time::SystemTime>,
    last_check: Instant,
}

/// egui key → SDL scancode name (physical key positions).
fn sdl_key_name(key: egui::Key) -> Option<String> {
    use egui::Key as K;
    let name = match key {
        K::Enter => "Return",
        K::Comma => ",",
        K::Minus => "-",
        K::Period => ".",
        K::Equals => "=",
        K::Semicolon => ";",
        K::Backslash => "\\",
        K::Slash => "/",
        K::OpenBracket => "[",
        K::CloseBracket => "]",
        K::Backtick => "`",
        K::Quote => "'",
        K::ShiftLeft => "Left Shift",
        K::ShiftRight => "Right Shift",
        K::ControlLeft => "Left Ctrl",
        K::ControlRight => "Right Ctrl",
        K::AltLeft => "Left Alt",
        K::AltRight => "Right Alt",
        K::SuperLeft => "Left GUI",
        K::SuperRight => "Right GUI",
        K::Escape => return None,
        other => {
            let n = other.name();
            let known = n.len() <= 3
                || matches!(
                    n,
                    "Up" | "Down"
                        | "Left"
                        | "Right"
                        | "Tab"
                        | "Backspace"
                        | "Insert"
                        | "Delete"
                        | "Home"
                        | "End"
                        | "PageUp"
                        | "PageDown"
                        | "Space"
                );
            if !known {
                return None;
            }
            n
        }
    };
    Some(name.to_owned())
}

impl App {
    fn new(path: PathBuf, page: Option<Tab>, sdl: Option<&'static sdl::Context>) -> Self {
        let loaded = settings::load(&path);
        // This window is often the first frontend a user opens, so it must
        // pick the same language `gba-desktop` would: the saved choice if
        // there is one, otherwise the OS language (SPEC-069 L10N-69-04).
        let (lang, explicit) =
            settings::initial_language(Some(&path), &loaded.settings, &sdl::preferred_locales());
        gba_i18n::set_lang(lang);
        let status = loaded.warning.map(|w| (w, Instant::now()));
        let known_mtime = settings::modified(Some(&path));
        let mut draft = loaded.settings;
        draft.language = lang;
        Self {
            path,
            effective: draft.clone(),
            draft,
            invalid_on_disk: loaded.invalid_on_disk,
            language_explicit: explicit,
            tab: page.unwrap_or(Tab::Video),
            status,
            sdl,
            pads: Vec::new(),
            joys: Vec::new(),
            capture: None,
            gpio_test: false,
            gpio_values: Vec::new(),
            gpio_error: None,
            known_mtime,
            last_check: Instant::now(),
        }
    }

    /// Adopts changes written by another process (the running game, whose
    /// in-window menu also switches language).
    fn reload_if_changed(&mut self) {
        if self.last_check.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_check = Instant::now();
        let now = settings::modified(Some(&self.path));
        if now == self.known_mtime {
            return;
        }
        self.known_mtime = now;
        self.adopt(settings::load(&self.path));
    }

    /// Merges a file read from disk into this window.
    ///
    /// Split from [`Self::reload_if_changed`] so the rule can be tested
    /// without egui or a clock: unsaved edits are never discarded, but the
    /// language always follows, because the user did not type it here.
    fn adopt(&mut self, loaded: settings::Loaded) {
        if loaded.warning.is_some() {
            // Mid-write or invalid: keep what we have rather than fighting
            // over a file that is momentarily incomplete.
            return;
        }
        // The same rule as startup (SPEC-069 L10N-69-04): an explicit
        // `language` line is a choice; without one — including a deleted
        // file — the system language decides, so this window never flips to
        // the pt-BR default by accident.
        let (lang, explicit) = settings::initial_language(
            Some(&self.path),
            &loaded.settings,
            &sdl::preferred_locales(),
        );
        self.language_explicit = explicit;
        let mut s = loaded.settings;
        s.language = lang;
        let dirty = self.draft != self.effective;
        if !dirty {
            self.draft = s.clone();
        } else {
            // Keep the edits, but the language is not an edit to protect.
            self.draft.language = s.language;
        }
        self.effective = s;
        self.invalid_on_disk = loaded.invalid_on_disk;
        adopt_language(lang);
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    fn apply(&mut self) {
        match settings::save(
            &self.path,
            &self.draft,
            self.invalid_on_disk,
            self.language_explicit,
        ) {
            Ok(()) => {
                self.invalid_on_disk = false;
                self.effective = self.draft.clone();
                // Record our own write, so the poll below does not mistake it
                // for a change made by the game.
                self.known_mtime = settings::modified(Some(&self.path));
                self.say(tr!("settings.saved"));
            }
            Err(e) => self.say(tr!("settings.save_failed", error = e)),
        }
    }

    /// Applies a language pick (SPEC-069 L10N-69-03): only the `language` key
    /// is written. Saving the whole draft here would persist edits the user
    /// never confirmed — and a half-typed GPIO field would make the file
    /// invalid, losing the very switch this exists for.
    fn apply_language(&mut self) {
        match settings::save_language(&self.path, self.draft.language) {
            Ok(()) => {
                self.language_explicit = true;
                self.effective.language = self.draft.language;
                // Record our own write, like `apply` does.
                self.known_mtime = settings::modified(Some(&self.path));
                self.say(tr!("settings.saved"));
            }
            Err(e) => self.say(tr!("settings.save_failed", error = e)),
        }
    }

    /// Hotplug and capture of controller input (DEV-64-01).
    fn poll_devices(&mut self) {
        let Some(ctx) = self.sdl else { return };
        while let Some(ev) = ctx.poll_event() {
            match ev {
                Event::GamepadAdded(id) => {
                    if !self.pads.iter().any(|p| p.id() == id)
                        && let Ok(p) = ctx.open_gamepad(id)
                    {
                        self.pads.push(p);
                    }
                }
                Event::GamepadRemoved(id) => self.pads.retain(|p| p.id() != id),
                Event::JoystickAdded(id) => {
                    if !ctx.is_gamepad(id)
                        && !self.joys.iter().any(|j| j.id() == id)
                        && let Ok(j) = ctx.open_joystick(id)
                    {
                        self.joys.push(j);
                    }
                }
                Event::JoystickRemoved(id) => self.joys.retain(|j| j.id() != id),
                Event::PadDown { button } => {
                    if let Some(c) = self.capture
                        && c.device == Device::Gamepad
                    {
                        self.bind(c, sdl::pad_button_name(button));
                    }
                }
                Event::JoyButtonDown { which, button } => {
                    if self.is_generic(which) {
                        self.capture_joy(JoyControl::Button(button));
                    }
                }
                Event::JoyHat { which, hat, value } => {
                    if self.is_generic(which) && value != 0 {
                        let bit = [1u8, 2, 4, 8]
                            .into_iter()
                            .find(|b| value & b != 0)
                            .unwrap_or(1);
                        self.capture_joy(JoyControl::Hat(hat, bit));
                    }
                }
                Event::JoyAxis { which, axis, value } => {
                    if self.is_generic(which) && value.unsigned_abs() > 24000 {
                        self.capture_joy(JoyControl::Axis(axis, value > 0));
                    }
                }
                _ => {}
            }
        }
    }

    fn is_generic(&self, which: u32) -> bool {
        self.joys.iter().any(|j| j.id() == which)
    }

    fn capture_joy(&mut self, control: JoyControl) {
        if let Some(c) = self.capture
            && c.device == Device::Joystick
        {
            self.bind(c, control.name());
        }
    }

    fn bind(&mut self, c: Capture, name: String) {
        if name.is_empty() {
            return;
        }
        let lists = match c.device {
            Device::Keyboard => &mut self.draft.input.keyboard,
            Device::Gamepad => &mut self.draft.input.gamepad,
            Device::Joystick => &mut self.draft.input.joystick,
        };
        for l in lists.iter_mut() {
            l.retain(|n| n != &name);
        }
        lists[c.button] = vec![name.clone()];
        self.capture = None;
        self.say(tr!(
            "settings.bound",
            button = button_label(c.button),
            name = name
        ));
    }

    fn keyboard_capture(&mut self, ui: &egui::Ui) {
        let Some(c) = self.capture else { return };
        let events = ui.input(|i| i.events.clone());
        for ev in events {
            if let egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                repeat: false,
                ..
            } = ev
            {
                if key == egui::Key::Escape {
                    self.capture = None;
                    self.say(tr!("settings.capture_cancelled"));
                    return;
                }
                if c.device == Device::Keyboard
                    && let Some(name) = sdl_key_name(physical_key.unwrap_or(key))
                {
                    self.bind(c, name);
                    return;
                }
            }
        }
    }

    fn video_tab(&mut self, ui: &mut egui::Ui) {
        // Language first: it is the one setting that changes this window's
        // own text, so it must be findable without reading the rest
        // (SPEC-069 L10N-69-03).
        ui.horizontal(|ui| {
            ui.label(tr!("settings.language"));
            let mut chosen = self.draft.language;
            egui::ComboBox::from_id_salt("language")
                .selected_text(chosen.display_name())
                .show_ui(ui, |ui| {
                    for lang in Lang::all() {
                        ui.selectable_value(&mut chosen, *lang, lang.display_name());
                    }
                });
            if chosen != self.draft.language {
                self.draft.language = chosen;
                // Live: the game reloads settings.toml and picks this up.
                gba_i18n::set_lang(chosen);
                self.apply_language();
            }
        });
        ui.weak(tr!("settings.language_hint"));
        ui.separator();

        let v = &mut self.draft.video;
        egui::Grid::new("video")
            .num_columns(2)
            .spacing([24.0, 10.0])
            .show(ui, |ui| {
                ui.label(tr!("settings.video.scale"));
                ui.add(
                    egui::Slider::new(
                        &mut v.scale,
                        settings::SCALE_RANGE.0..=settings::SCALE_RANGE.1,
                    )
                    .suffix("×"),
                );
                ui.end_row();
                ui.label(tr!("settings.video.fullscreen"));
                ui.checkbox(&mut v.fullscreen, "");
                ui.end_row();
                ui.label(tr!("settings.video.filter"));
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut v.filter,
                        Filter::Nearest,
                        tr!("settings.video.filter_nearest"),
                    );
                    ui.selectable_value(
                        &mut v.filter,
                        Filter::Linear,
                        tr!("settings.video.filter_linear"),
                    );
                });
                ui.end_row();
                ui.label(tr!("settings.video.integer_scale"));
                ui.checkbox(
                    &mut v.integer_scale,
                    tr!("settings.video.integer_scale_hint"),
                );
                ui.end_row();
                ui.label(tr!("settings.video.vsync"));
                ui.checkbox(&mut v.vsync, "");
                ui.end_row();
                ui.label(tr!("settings.video.color_correction"));
                ui.checkbox(
                    &mut v.color_correction,
                    tr!("settings.video.color_correction_hint"),
                );
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.weak(tr!("settings.video.note"));
    }

    fn audio_tab(&mut self, ui: &mut egui::Ui) {
        let a = &mut self.draft.audio;
        egui::Grid::new("audio")
            .num_columns(2)
            .spacing([24.0, 10.0])
            .show(ui, |ui| {
                ui.label(tr!("settings.audio.volume"));
                ui.add(egui::Slider::new(&mut a.volume, 0..=100).suffix("%"));
                ui.end_row();
                ui.label(tr!("settings.audio.mute"));
                ui.checkbox(&mut a.mute, "");
                ui.end_row();
                ui.label(tr!("settings.audio.latency"));
                ui.add(
                    egui::Slider::new(
                        &mut a.latency_ms,
                        settings::LATENCY_RANGE.0..=settings::LATENCY_RANGE.1,
                    )
                    .step_by(10.0)
                    .suffix(" ms"),
                );
                ui.end_row();
            });
    }

    fn controls_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("settings.controls.devices"));
        if self.sdl.is_none() {
            ui.colored_label(egui::Color32::YELLOW, tr!("settings.controls.no_sdl"));
        }
        if self.pads.is_empty() && self.joys.is_empty() {
            ui.weak(tr!("settings.controls.none"));
        }
        for p in &self.pads {
            ui.label(tr!(
                "settings.controls.pad_line",
                name = p.name(),
                connection = p.connection().label()
            ));
        }
        for j in &self.joys {
            let (b, a, h) = j.counts();
            ui.label(tr!(
                "settings.controls.joystick_line",
                name = j.name(),
                buttons = b,
                axes = a,
                hats = h,
                connection = j.connection().label()
            ));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(tr!("settings.controls.deadzone"));
            ui.add(
                egui::Slider::new(&mut self.draft.input.deadzone, 0.0..=settings::DEADZONE_MAX)
                    .fixed_decimals(2),
            );
        });
        ui.horizontal(|ui| {
            use gba_desktop::settings::TouchMode;
            ui.label(tr!("settings.controls.touch"));
            let t = &mut self.draft.input.touch_controls;
            ui.selectable_value(t, TouchMode::Auto, tr!("settings.controls.touch_auto"));
            ui.selectable_value(t, TouchMode::On, tr!("settings.controls.touch_always"));
            ui.selectable_value(t, TouchMode::Off, tr!("settings.controls.touch_never"));
        });
        ui.add_space(8.0);
        ui.heading(tr!("settings.controls.buttons"));
        let capture = self.capture;
        let mut start: Option<Capture> = None;
        let mut clear: Option<Capture> = None;
        egui::ScrollArea::vertical()
            .max_height(330.0)
            .show(ui, |ui| {
                egui::Grid::new("bindings")
                    .striped(true)
                    .num_columns(4)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong(tr!("settings.controls.col_button"));
                        ui.strong(tr!("settings.controls.col_keyboard"));
                        ui.strong(tr!("settings.controls.col_gamepad"));
                        ui.strong(tr!("settings.controls.col_joystick"));
                        ui.end_row();
                        for i in 0..BUTTON_LABEL_KEYS.len() {
                            ui.label(button_label(i));
                            for (device, list) in [
                                (Device::Keyboard, &self.draft.input.keyboard[i]),
                                (Device::Gamepad, &self.draft.input.gamepad[i]),
                                (Device::Joystick, &self.draft.input.joystick[i]),
                            ] {
                                let this = Capture { button: i, device };
                                ui.horizontal(|ui| {
                                    if capture == Some(this) {
                                        let hint = match device {
                                            Device::Keyboard => tr!("settings.controls.press_key"),
                                            Device::Gamepad => tr!("settings.controls.press_pad"),
                                            Device::Joystick => {
                                                tr!("settings.controls.press_joystick")
                                            }
                                        };
                                        ui.colored_label(egui::Color32::LIGHT_BLUE, hint);
                                    } else {
                                        let text = if list.is_empty() {
                                            "—".to_owned()
                                        } else {
                                            list.join(", ")
                                        };
                                        if ui
                                            .button(text)
                                            .on_hover_text(tr!("settings.controls.click_to_bind"))
                                            .clicked()
                                        {
                                            start = Some(this);
                                        }
                                        if !list.is_empty()
                                            && ui
                                                .small_button("✕")
                                                .on_hover_text(tr!("settings.controls.remove"))
                                                .clicked()
                                        {
                                            clear = Some(this);
                                        }
                                    }
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(c) = start {
            self.capture = Some(c);
            self.say(tr!("settings.capture_waiting"));
        }
        if let Some(c) = clear {
            let lists = match c.device {
                Device::Keyboard => &mut self.draft.input.keyboard,
                Device::Gamepad => &mut self.draft.input.gamepad,
                Device::Joystick => &mut self.draft.input.joystick,
            };
            lists[c.button].clear();
        }
        ui.add_space(6.0);
        if ui.button(tr!("settings.controls.restore")).clicked() {
            self.draft.input = settings::Input::default();
        }
    }

    fn gpio_tab(&mut self, ui: &mut egui::Ui) {
        if !gpio::available() {
            ui.label(tr!("settings.gpio.unavailable"));
            ui.weak(tr!("settings.gpio.unavailable_hint"));
        }
        let g = &mut self.draft.gpio;
        egui::Grid::new("gpio")
            .num_columns(2)
            .spacing([24.0, 8.0])
            .show(ui, |ui| {
                ui.label(tr!("settings.gpio.use"));
                ui.checkbox(&mut g.enabled, "");
                ui.end_row();
                ui.label(tr!("settings.gpio.chip"));
                ui.text_edit_singleline(&mut g.chip);
                ui.end_row();
                ui.label(tr!("settings.gpio.active_low"));
                ui.checkbox(&mut g.active_low, "");
                ui.end_row();
                ui.label(tr!("settings.gpio.bias"));
                let bias_label = |b: Bias| match b {
                    Bias::PullUp => tr!("settings.gpio.pull_up"),
                    Bias::PullDown => tr!("settings.gpio.pull_down"),
                    Bias::Disabled => tr!("settings.gpio.disabled"),
                };
                egui::ComboBox::from_id_salt("bias")
                    .selected_text(bias_label(g.bias))
                    .show_ui(ui, |ui| {
                        for b in [Bias::PullUp, Bias::PullDown, Bias::Disabled] {
                            ui.selectable_value(&mut g.bias, b, bias_label(b));
                        }
                    });
                ui.end_row();
                ui.label(tr!("settings.gpio.debounce"));
                ui.add(egui::Slider::new(&mut g.debounce_ms, 0..=100).suffix(" ms"));
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.heading(tr!("settings.gpio.lines"));
        ui.weak(tr!("settings.gpio.lines_hint"));
        egui::Grid::new("lines")
            .striped(true)
            .num_columns(3)
            .show(ui, |ui| {
                for i in 0..BUTTON_LABEL_KEYS.len() {
                    ui.label(button_label(i));
                    let mut on = g.lines[i].is_some();
                    if ui.checkbox(&mut on, tr!("settings.gpio.on")).changed() {
                        g.lines[i] = if on { Some(0) } else { None };
                    }
                    if let Some(line) = g.lines[i].as_mut() {
                        ui.add(egui::DragValue::new(line).range(0..=1023));
                        if let Some((_, v)) = self.gpio_values.iter().find(|(b, _)| *b == i) {
                            ui.label(if *v {
                                tr!("settings.gpio.pressed")
                            } else {
                                tr!("settings.gpio.released")
                            });
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        ui.add_space(6.0);
        if gpio::available() {
            ui.checkbox(&mut self.gpio_test, tr!("settings.gpio.live_test"));
            if let Some(e) = &self.gpio_error {
                ui.colored_label(egui::Color32::LIGHT_RED, e);
            }
        }
    }

    fn sample_gpio(&mut self) {
        if !self.gpio_test {
            self.gpio_values.clear();
            self.gpio_error = None;
            return;
        }
        let used: Vec<(usize, u32)> = self
            .draft
            .gpio
            .lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.map(|l| (i, l)))
            .collect();
        let lines: Vec<u32> = used.iter().map(|(_, l)| *l).collect();
        match gpio::probe(&self.draft.gpio, &lines) {
            Ok(v) => {
                self.gpio_values = used.iter().map(|(i, _)| *i).zip(v).collect();
                self.gpio_error = None;
            }
            Err(e) => {
                self.gpio_values.clear();
                self.gpio_error = Some(e);
            }
        }
    }
}

/// Follow a language chosen elsewhere (the game's in-window menu), so the two
/// windows never disagree (SPEC-069 L10N-69-03). Returns whether it changed.
fn adopt_language(lang: Lang) -> bool {
    if gba_i18n::lang() == lang {
        return false;
    }
    gba_i18n::set_lang(lang);
    true
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_devices();
        self.reload_if_changed();
        self.keyboard_capture(ui);
        self.sample_gpio();
        // Keep polling controllers and GPIO even without UI input.
        ui.ctx().request_repaint_after(Duration::from_millis(50));

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Video, tr!("settings.tab.video"));
                ui.selectable_value(&mut self.tab, Tab::Audio, tr!("settings.tab.audio"));
                ui.selectable_value(&mut self.tab, Tab::Controls, tr!("settings.tab.controls"));
                ui.selectable_value(&mut self.tab, Tab::Gpio, tr!("settings.tab.gpio"));
            });
            ui.add_space(4.0);
        });
        egui::Panel::bottom("actions").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let dirty = self.draft != self.effective;
                if ui
                    .add_enabled(dirty, egui::Button::new(tr!("settings.apply")))
                    .clicked()
                {
                    self.apply();
                }
                if ui
                    .add_enabled(dirty, egui::Button::new(tr!("settings.cancel")))
                    .clicked()
                {
                    self.draft = self.effective.clone();
                    self.capture = None;
                    self.say(tr!("settings.discarded"));
                }
                if ui.button(tr!("settings.restore_tab")).clicked() {
                    let d = Settings::default();
                    match self.tab {
                        Tab::Video => self.draft.video = d.video,
                        Tab::Audio => self.draft.audio = d.audio,
                        Tab::Controls => self.draft.input = d.input,
                        Tab::Gpio => self.draft.gpio = d.gpio,
                    }
                }
                if dirty {
                    ui.colored_label(egui::Color32::YELLOW, tr!("settings.unsaved"));
                }
            });
            if let Some((msg, at)) = &self.status
                && at.elapsed() < Duration::from_secs(8)
            {
                ui.label(msg.as_str());
            }
            ui.weak(self.path.display().to_string());
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(8.0);
            match self.tab {
                Tab::Video => self.video_tab(ui),
                Tab::Audio => self.audio_tab(ui),
                Tab::Controls => self.controls_tab(ui),
                Tab::Gpio => self.gpio_tab(ui),
            }
        });
    }
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut page = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--settings" => path = args.next().map(PathBuf::from),
            "--page" => {
                page = match args.next().as_deref() {
                    Some("video") => Some(Tab::Video),
                    Some("audio") => Some(Tab::Audio),
                    Some("controls") => Some(Tab::Controls),
                    Some("gpio") => Some(Tab::Gpio),
                    other => return Err(format!("unknown page {other:?}")),
                }
            }
            "-h" | "--help" => {
                println!(
                    "usage: gba-settings [--settings settings.toml] [--page video|audio|controls|gpio]"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument `{flag}`")),
        }
    }
    let path = path
        .or_else(settings::default_path)
        .ok_or("cannot determine the settings location; pass --settings")?;
    // SDL must be initialised before the winit event loop starts: on macOS
    // HIDAPI enumeration runs the Cocoa run loop, which winit forbids inside
    // its handlers. HIDAPI is disabled here; IOKit/GameController still
    // report USB and Bluetooth controllers (private run-loop mode).
    if cfg!(target_os = "macos") {
        sdl::set_hint("SDL_JOYSTICK_HIDAPI", "0");
    }
    let sdl: Option<&'static sdl::Context> = match sdl::Context::init_input() {
        Ok(ctx) => Some(Box::leak(Box::new(ctx))),
        Err(e) => {
            eprintln!("gba-settings: controllers unavailable ({e})");
            None
        }
    };
    // Pick the language before the window exists: the title bar is drawn by
    // the OS from a string we pass once, so it must already be translated
    // (SPEC-069 L10N-69-03).
    let startup = settings::load(&path);
    let (lang, _) =
        settings::initial_language(Some(&path), &startup.settings, &sdl::preferred_locales());
    gba_i18n::set_lang(lang);
    // Window/Dock icon from the embedded logo (decoded by SDL, no extra crate).
    let icon = gba_desktop::sdl::Surface::from_png(gba_desktop::assets::ICON_PNG)
        .and_then(|s| s.to_rgba())
        .map(|(width, height, rgba)| egui::IconData {
            rgba,
            width,
            height,
        });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(tr!("settings.title"))
        .with_inner_size([720.0, 620.0]);
    match icon {
        Ok(icon) => viewport = viewport.with_icon(std::sync::Arc::new(icon)),
        Err(e) => eprintln!("gba-settings: icon unavailable ({e})"),
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "gba-settings",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(path, page, sdl)))),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// The locale is process-global and tests run in parallel, so any test
    /// that sets or reads it must hold this (SPEC-069).
    static LOCALE: Mutex<()> = Mutex::new(());

    struct Locale(#[allow(dead_code)] MutexGuard<'static, ()>);

    impl Drop for Locale {
        fn drop(&mut self) {
            gba_i18n::set_lang(Lang::default());
        }
    }

    fn locale() -> Locale {
        let guard = LOCALE.lock().unwrap_or_else(|e| e.into_inner());
        gba_i18n::set_lang(Lang::default());
        Locale(guard)
    }

    /// The game's in-window menu writes `language`; an open settings window
    /// must follow it rather than showing stale text (SPEC-069 L10N-69-03).
    #[test]
    fn a_language_written_by_the_game_is_adopted() {
        let _locale = locale();
        let dir = std::env::temp_dir().join(format!("gba-set-lang-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        std::fs::write(&path, "version = 1\nlanguage = \"pt-BR\"\n").unwrap();

        let mut app = App::new(path.clone(), Some(Tab::Video), None);
        assert_eq!(gba_i18n::lang(), Lang::PtBr);

        // The game switches to English and saves.
        std::fs::write(&path, "version = 1\nlanguage = \"en-US\"\n").unwrap();
        app.adopt(settings::load(&path));
        assert_eq!(gba_i18n::lang(), Lang::EnUs, "window follows the game");
        assert_eq!(app.draft.language, Lang::EnUs);
        assert_eq!(app.effective.language, Lang::EnUs);

        gba_i18n::set_lang(Lang::PtBr);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Unsaved edits must survive an external write, but the language is not
    /// an edit the user made in this window, so it still follows.
    #[test]
    fn external_change_keeps_unsaved_edits() {
        let _locale = locale();
        let dir = std::env::temp_dir().join(format!("gba-set-dirty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        std::fs::write(&path, "version = 1\n[video]\nscale = 3\n").unwrap();

        let mut app = App::new(path.clone(), Some(Tab::Video), None);
        app.draft.video.scale = 7; // an edit the user has not applied

        std::fs::write(
            &path,
            "version = 1\nlanguage = \"en-US\"\n[video]\nscale = 2\n",
        )
        .unwrap();
        app.adopt(settings::load(&path));

        assert_eq!(app.draft.video.scale, 7, "unsaved edit kept");
        assert_eq!(app.effective.video.scale, 2, "disk state still tracked");
        assert_eq!(app.draft.language, Lang::EnUs, "language follows anyway");

        gba_i18n::set_lang(Lang::PtBr);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The real `App` end to end, but on a clock we control: the poll must
    /// notice an external write and switch the window's language.
    #[test]
    fn polling_adopts_an_external_change() {
        let _locale = locale();
        let dir = std::env::temp_dir().join(format!("gba-set-poll-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        std::fs::write(&path, "version = 1\nlanguage = \"pt-BR\"\n").unwrap();

        let mut app = App::new(path.clone(), Some(Tab::Video), None);
        assert_eq!(gba_i18n::lang(), Lang::PtBr);

        // Before the interval elapses nothing is read, even if the file moved.
        std::fs::write(&path, "version = 1\nlanguage = \"en-US\"\n").unwrap();
        app.reload_if_changed();
        assert_eq!(
            gba_i18n::lang(),
            Lang::PtBr,
            "throttled by the poll interval"
        );

        // Once it elapses, the change is adopted.
        app.last_check = Instant::now() - Duration::from_secs(1);
        app.reload_if_changed();
        assert_eq!(gba_i18n::lang(), Lang::EnUs);
        assert_eq!(app.draft.language, Lang::EnUs);

        // Our own write must not look like a foreign change: after `apply`,
        // what the window believes about the file must be what is on disk.
        app.apply();
        assert_eq!(
            app.known_mtime,
            settings::modified(Some(&path)),
            "apply records its own write"
        );
        app.last_check = Instant::now() - Duration::from_secs(1);
        app.reload_if_changed();
        assert_eq!(gba_i18n::lang(), Lang::EnUs, "no self-triggered churn");
        assert_eq!(app.draft, app.effective, "no spurious edit appeared");

        gba_i18n::set_lang(Lang::PtBr);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn egui_keys_map_to_sdl_scancode_names() {
        assert_eq!(sdl_key_name(egui::Key::Enter).as_deref(), Some("Return"));
        assert_eq!(sdl_key_name(egui::Key::Z).as_deref(), Some("Z"));
        assert_eq!(sdl_key_name(egui::Key::ArrowUp).as_deref(), Some("Up"));
        assert_eq!(
            sdl_key_name(egui::Key::ShiftRight).as_deref(),
            Some("Right Shift")
        );
        assert_eq!(sdl_key_name(egui::Key::F12).as_deref(), Some("F12"));
        assert_eq!(sdl_key_name(egui::Key::Num7).as_deref(), Some("7"));
        assert_eq!(sdl_key_name(egui::Key::Escape), None);
    }

    /// The language picker writes only the `language` key: pending edits the
    /// user never confirmed stay out of the file (SPEC-069 L10N-69-03).
    #[test]
    fn the_language_combo_writes_only_the_language_key() {
        let _locale = locale();
        let dir = std::env::temp_dir().join(format!("gba-set-combo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        std::fs::write(&path, "version = 1\n[audio]\nvolume = 50\n").unwrap();

        let mut app = App::new(path.clone(), Some(Tab::Video), None);
        app.draft.audio.volume = 30; // an edit the user has not applied
        app.draft.language = Lang::EnUs;
        app.apply_language();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("language = \"en-US\""), "{text}");
        assert!(text.contains("volume = 50"), "no unapplied edit is written");
        assert_eq!(app.draft.audio.volume, 30, "the edit is still pending");
        assert_eq!(app.effective.language, Lang::EnUs);
        assert!(app.language_explicit, "a pick is an explicit choice");

        gba_i18n::set_lang(Lang::PtBr);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A deleted file is "no explicit choice": the window follows the system
    /// language rather than flipping to the pt-BR default (L10N-69-04).
    #[test]
    fn a_deleted_file_returns_to_system_following() {
        let _locale = locale();
        let dir = std::env::temp_dir().join(format!("gba-set-del-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        std::fs::write(&path, "version = 1\nlanguage = \"en-US\"\n").unwrap();

        let mut app = App::new(path.clone(), Some(Tab::Video), None);
        assert_eq!(gba_i18n::lang(), Lang::EnUs);
        assert!(app.language_explicit);

        std::fs::remove_file(&path).unwrap();
        app.adopt(settings::load(&path));
        let detected = gba_i18n::detect_from(&sdl::preferred_locales()).unwrap_or_default();
        assert!(!app.language_explicit, "the key is gone");
        assert_eq!(gba_i18n::lang(), detected, "the system decides again");

        gba_i18n::set_lang(Lang::PtBr);
        std::fs::remove_dir_all(&dir).ok();
    }
}

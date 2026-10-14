//! External settings window (SPEC-064 EXT-64-01): edits the same
//! `settings.toml` as the game, which reloads it live. egui/eframe UI; SDL is
//! used only for input-device discovery and capture.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use gba_desktop::gpio;
use gba_desktop::input::JoyControl;
use gba_desktop::sdl::{self, Event};
use gba_desktop::settings::{self, BUTTON_LABELS, Bias, Filter, Settings};

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
    tab: Tab,
    status: Option<(String, Instant)>,
    sdl: Option<&'static sdl::Context>,
    pads: Vec<sdl::Gamepad<'static>>,
    joys: Vec<sdl::Joystick<'static>>,
    capture: Option<Capture>,
    gpio_test: bool,
    gpio_values: Vec<(usize, bool)>,
    gpio_error: Option<String>,
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
        let status = loaded.warning.map(|w| (w, Instant::now()));
        Self {
            path,
            effective: loaded.settings.clone(),
            draft: loaded.settings,
            invalid_on_disk: loaded.invalid_on_disk,
            tab: page.unwrap_or(Tab::Video),
            status,
            sdl,
            pads: Vec::new(),
            joys: Vec::new(),
            capture: None,
            gpio_test: false,
            gpio_values: Vec::new(),
            gpio_error: None,
        }
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    fn apply(&mut self) {
        match settings::save(&self.path, &self.draft, self.invalid_on_disk) {
            Ok(()) => {
                self.invalid_on_disk = false;
                self.effective = self.draft.clone();
                self.say("Configurações salvas — o jogo aplica em até meio segundo");
            }
            Err(e) => self.say(format!("Falha ao salvar: {e}")),
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
        self.say(format!(
            "{} → {name} (clique em Aplicar para salvar)",
            BUTTON_LABELS[c.button]
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
                    self.say("Captura cancelada; o binding anterior foi mantido");
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
        let v = &mut self.draft.video;
        egui::Grid::new("video")
            .num_columns(2)
            .spacing([24.0, 10.0])
            .show(ui, |ui| {
                ui.label("Escala da janela");
                ui.add(
                    egui::Slider::new(
                        &mut v.scale,
                        settings::SCALE_RANGE.0..=settings::SCALE_RANGE.1,
                    )
                    .suffix("×"),
                );
                ui.end_row();
                ui.label("Tela cheia");
                ui.checkbox(&mut v.fullscreen, "");
                ui.end_row();
                ui.label("Filtro");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut v.filter, Filter::Nearest, "Nítido (nearest)");
                    ui.selectable_value(&mut v.filter, Filter::Linear, "Suave (linear)");
                });
                ui.end_row();
                ui.label("Escala inteira");
                ui.checkbox(&mut v.integer_scale, "evita pixels de tamanhos diferentes");
                ui.end_row();
                ui.label("VSync");
                ui.checkbox(&mut v.vsync, "");
                ui.end_row();
                ui.label("Correção de cor do LCD");
                ui.checkbox(&mut v.color_correction, "simula a tela do GBA");
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.weak("Estas opções só mudam a apresentação; o jogo roda igual (SET-63-02).");
    }

    fn audio_tab(&mut self, ui: &mut egui::Ui) {
        let a = &mut self.draft.audio;
        egui::Grid::new("audio")
            .num_columns(2)
            .spacing([24.0, 10.0])
            .show(ui, |ui| {
                ui.label("Volume");
                ui.add(egui::Slider::new(&mut a.volume, 0..=100).suffix("%"));
                ui.end_row();
                ui.label("Mudo");
                ui.checkbox(&mut a.mute, "");
                ui.end_row();
                ui.label("Latência máxima");
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
        ui.heading("Dispositivos conectados");
        if self.sdl.is_none() {
            ui.colored_label(
                egui::Color32::YELLOW,
                "SDL indisponível: só o teclado pode ser configurado.",
            );
        }
        if self.pads.is_empty() && self.joys.is_empty() {
            ui.weak("Nenhum controle detectado. Conecte por USB ou pareie por Bluetooth; a lista atualiza sozinha.");
        }
        for p in &self.pads {
            ui.label(format!(
                "🎮 {} — gamepad, {}",
                p.name(),
                p.connection().label()
            ));
        }
        for j in &self.joys {
            let (b, a, h) = j.counts();
            ui.label(format!(
                "🕹 {} — joystick genérico ({b} botões, {a} eixos, {h} hats), {}",
                j.name(),
                j.connection().label()
            ));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Zona morta do analógico");
            ui.add(
                egui::Slider::new(&mut self.draft.input.deadzone, 0.0..=settings::DEADZONE_MAX)
                    .fixed_decimals(2),
            );
        });
        ui.add_space(8.0);
        ui.heading("Botões do GBA");
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
                        ui.strong("Botão");
                        ui.strong("Teclado");
                        ui.strong("Gamepad");
                        ui.strong("Joystick genérico");
                        ui.end_row();
                        for (i, label) in BUTTON_LABELS.iter().enumerate() {
                            ui.label(*label);
                            for (device, list) in [
                                (Device::Keyboard, &self.draft.input.keyboard[i]),
                                (Device::Gamepad, &self.draft.input.gamepad[i]),
                                (Device::Joystick, &self.draft.input.joystick[i]),
                            ] {
                                let this = Capture { button: i, device };
                                ui.horizontal(|ui| {
                                    if capture == Some(this) {
                                        let hint = match device {
                                            Device::Keyboard => "pressione uma tecla…",
                                            Device::Gamepad => "pressione um botão…",
                                            Device::Joystick => "mova/pressione o joystick…",
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
                                            .on_hover_text("Clique para definir")
                                            .clicked()
                                        {
                                            start = Some(this);
                                        }
                                        if !list.is_empty()
                                            && ui
                                                .small_button("✕")
                                                .on_hover_text("Remover")
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
            self.say("Aguardando entrada… (Esc cancela)");
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
        if ui.button("Restaurar controles padrão").clicked() {
            self.draft.input = settings::Input::default();
        }
    }

    fn gpio_tab(&mut self, ui: &mut egui::Ui) {
        if !gpio::available() {
            ui.label("GPIO só está disponível no Linux (Raspberry Pi e similares), pela interface /dev/gpiochipN.");
            ui.weak("As configurações abaixo são salvas e valem quando o jogo roda no Linux.");
        }
        let g = &mut self.draft.gpio;
        egui::Grid::new("gpio")
            .num_columns(2)
            .spacing([24.0, 8.0])
            .show(ui, |ui| {
                ui.label("Usar botões em GPIO");
                ui.checkbox(&mut g.enabled, "");
                ui.end_row();
                ui.label("Chip");
                ui.text_edit_singleline(&mut g.chip);
                ui.end_row();
                ui.label("Botão ligado ao GND (ativo em 0)");
                ui.checkbox(&mut g.active_low, "");
                ui.end_row();
                ui.label("Resistor interno");
                egui::ComboBox::from_id_salt("bias")
                    .selected_text(match g.bias {
                        Bias::PullUp => "pull-up",
                        Bias::PullDown => "pull-down",
                        Bias::Disabled => "desligado",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut g.bias, Bias::PullUp, "pull-up");
                        ui.selectable_value(&mut g.bias, Bias::PullDown, "pull-down");
                        ui.selectable_value(&mut g.bias, Bias::Disabled, "desligado");
                    });
                ui.end_row();
                ui.label("Debounce");
                ui.add(egui::Slider::new(&mut g.debounce_ms, 0..=100).suffix(" ms"));
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.heading("Linha (offset no chip) de cada botão");
        ui.weak("No Raspberry Pi o offset é o número BCM (ex.: GPIO17 = 17).");
        egui::Grid::new("lines")
            .striped(true)
            .num_columns(3)
            .show(ui, |ui| {
                for (i, label) in BUTTON_LABELS.iter().enumerate() {
                    ui.label(*label);
                    let mut on = g.lines[i].is_some();
                    if ui.checkbox(&mut on, "ligado").changed() {
                        g.lines[i] = if on { Some(0) } else { None };
                    }
                    if let Some(line) = g.lines[i].as_mut() {
                        ui.add(egui::DragValue::new(line).range(0..=1023));
                        if let Some((_, v)) = self.gpio_values.iter().find(|(b, _)| *b == i) {
                            ui.label(if *v { "● pressionado" } else { "○ solto" });
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        ui.add_space(6.0);
        if gpio::available() {
            ui.checkbox(
                &mut self.gpio_test,
                "Testar pinos ao vivo (feche o jogo antes: o chip é exclusivo)",
            );
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

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_devices();
        self.keyboard_capture(ui);
        self.sample_gpio();
        // Keep polling controllers and GPIO even without UI input.
        ui.ctx().request_repaint_after(Duration::from_millis(50));

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Video, "🖥  Vídeo");
                ui.selectable_value(&mut self.tab, Tab::Audio, "🔊  Áudio");
                ui.selectable_value(&mut self.tab, Tab::Controls, "🎮  Controles");
                ui.selectable_value(&mut self.tab, Tab::Gpio, "📌  GPIO");
            });
            ui.add_space(4.0);
        });
        egui::Panel::bottom("actions").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let dirty = self.draft != self.effective;
                if ui
                    .add_enabled(dirty, egui::Button::new("Aplicar"))
                    .clicked()
                {
                    self.apply();
                }
                if ui
                    .add_enabled(dirty, egui::Button::new("Cancelar"))
                    .clicked()
                {
                    self.draft = self.effective.clone();
                    self.capture = None;
                    self.say("Alterações descartadas");
                }
                if ui.button("Restaurar padrões desta aba").clicked() {
                    let d = Settings::default();
                    match self.tab {
                        Tab::Video => self.draft.video = d.video,
                        Tab::Audio => self.draft.audio = d.audio,
                        Tab::Controls => self.draft.input = d.input,
                        Tab::Gpio => self.draft.gpio = d.gpio,
                    }
                }
                if dirty {
                    ui.colored_label(egui::Color32::YELLOW, "alterações não salvas");
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("gba-recomp — Configurações")
            .with_inner_size([720.0, 620.0]),
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
}

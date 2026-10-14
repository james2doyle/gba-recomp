//! In-window settings menu (SPEC-063 MENU-63-01/02). Pure model: the app
//! feeds navigation and draws `rows()`. Labels are ASCII (SDL debug font).

use crate::settings::{BUTTON_LABELS, DEADZONE_MAX, Filter, LATENCY_RANGE, SCALE_RANGE, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Main,
    Video,
    Audio,
    Controls,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    None,
    /// Close the menu and resume the guest.
    Resume,
    Reset,
    SaveNow,
    Quit,
    /// Make the draft effective and persist it (UI-02).
    Apply(Settings),
}

/// Pending remap: GBA button index and whether it is a gamepad binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Remap {
    pub button: usize,
    pub pad: bool,
}

pub struct Row {
    pub label: String,
    pub value: String,
}

pub struct Menu {
    pub open: bool,
    pub page: Page,
    pub cursor: usize,
    pub draft: Settings,
    effective: Settings,
    pub remap: Option<Remap>,
    pub status: Option<String>,
}

const MAIN: [&str; 7] = [
    "Continuar",
    "Video",
    "Audio",
    "Controles",
    "Reiniciar jogo",
    "Salvar agora",
    "Sair",
];

fn yes(b: bool) -> String {
    if b { "Sim".into() } else { "Nao".into() }
}

impl Menu {
    pub fn new(effective: Settings) -> Self {
        Self {
            open: false,
            page: Page::Main,
            cursor: 0,
            draft: effective.clone(),
            effective,
            remap: None,
            status: None,
        }
    }

    pub fn open_at(&mut self, page: Page, effective: &Settings) {
        self.open = true;
        self.page = page;
        self.cursor = 0;
        self.effective = effective.clone();
        self.draft = effective.clone();
        self.remap = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.remap = None;
        self.draft = self.effective.clone();
    }

    pub fn title(&self) -> &'static str {
        match self.page {
            Page::Main => "Menu",
            Page::Video => "Video",
            Page::Audio => "Audio",
            Page::Controls => "Controles",
        }
    }

    pub fn rows(&self) -> Vec<Row> {
        let row = |l: &str, v: String| Row {
            label: l.into(),
            value: v,
        };
        let d = &self.draft;
        let mut rows = match self.page {
            Page::Main => MAIN.iter().map(|l| row(l, String::new())).collect(),
            Page::Video => vec![
                row("Escala", format!("{}x", d.video.scale)),
                row("Tela cheia", yes(d.video.fullscreen)),
                row(
                    "Filtro",
                    if d.video.filter == Filter::Linear {
                        "Suave".into()
                    } else {
                        "Nitido".into()
                    },
                ),
                row("Escala inteira", yes(d.video.integer_scale)),
                row("VSync", yes(d.video.vsync)),
                row("Correcao de cor", yes(d.video.color_correction)),
            ],
            Page::Audio => vec![
                row("Volume", format!("{}%", d.audio.volume)),
                row("Mudo", yes(d.audio.mute)),
                row("Latencia", format!("{} ms", d.audio.latency_ms)),
            ],
            Page::Controls => {
                let mut v = Vec::new();
                for (i, label) in BUTTON_LABELS.iter().enumerate() {
                    let waiting = |pad| self.remap == Some(Remap { button: i, pad });
                    v.push(row(
                        &format!("{label} teclado"),
                        if waiting(false) {
                            "<tecla...>".into()
                        } else {
                            d.input.keyboard[i].join(", ")
                        },
                    ));
                    v.push(row(
                        &format!("{label} controle"),
                        if waiting(true) {
                            "<botao...>".into()
                        } else {
                            d.input.gamepad[i].join(", ")
                        },
                    ));
                }
                v.push(row("Zona morta", format!("{:.2}", d.input.deadzone)));
                v.push(row("Restaurar padroes", String::new()));
                v
            }
        };
        if self.page != Page::Main {
            rows.push(row("Aplicar", String::new()));
            rows.push(row("Cancelar", String::new()));
        }
        rows
    }

    pub fn handle(&mut self, nav: Nav) -> Command {
        if self.remap.is_some() {
            if nav == Nav::Back {
                // Cancel keeps the previous binding (UI-01).
                self.remap = None;
            }
            return Command::None;
        }
        let count = self.rows().len();
        match nav {
            Nav::Up => {
                self.cursor = (self.cursor + count - 1) % count;
                Command::None
            }
            Nav::Down => {
                self.cursor = (self.cursor + 1) % count;
                Command::None
            }
            Nav::Back => {
                if self.page == Page::Main {
                    self.close();
                    Command::Resume
                } else {
                    self.draft = self.effective.clone();
                    self.go(Page::Main);
                    Command::None
                }
            }
            Nav::Left | Nav::Right | Nav::Confirm => self.activate(nav),
        }
    }

    fn go(&mut self, page: Page) {
        let back_cursor = match self.page {
            Page::Video => 1,
            Page::Audio => 2,
            Page::Controls => 3,
            Page::Main => 0,
        };
        self.page = page;
        self.cursor = if page == Page::Main { back_cursor } else { 0 };
    }

    fn activate(&mut self, nav: Nav) -> Command {
        let delta: i32 = match nav {
            Nav::Left => -1,
            _ => 1,
        };
        let confirm = nav == Nav::Confirm;
        let count = self.rows().len();
        if self.page != Page::Main && self.cursor >= count - 2 {
            if !confirm {
                return Command::None;
            }
            if self.cursor == count - 2 {
                self.effective = self.draft.clone();
                self.status = Some("Configuracoes aplicadas".into());
                self.go(Page::Main);
                return Command::Apply(self.draft.clone());
            }
            self.draft = self.effective.clone();
            self.go(Page::Main);
            return Command::None;
        }
        let d = &mut self.draft;
        match self.page {
            Page::Main => {
                if !confirm {
                    return Command::None;
                }
                match self.cursor {
                    0 => {
                        self.close();
                        Command::Resume
                    }
                    1 => {
                        self.go(Page::Video);
                        Command::None
                    }
                    2 => {
                        self.go(Page::Audio);
                        Command::None
                    }
                    3 => {
                        self.go(Page::Controls);
                        Command::None
                    }
                    4 => {
                        self.close();
                        Command::Reset
                    }
                    5 => Command::SaveNow,
                    _ => Command::Quit,
                }
            }
            Page::Video => {
                let v = &mut d.video;
                match self.cursor {
                    0 => {
                        v.scale = step(
                            v.scale as i32,
                            delta,
                            SCALE_RANGE.0 as i32,
                            SCALE_RANGE.1 as i32,
                            confirm,
                        ) as u8
                    }
                    1 => v.fullscreen = !v.fullscreen,
                    2 => {
                        v.filter = if v.filter == Filter::Linear {
                            Filter::Nearest
                        } else {
                            Filter::Linear
                        }
                    }
                    3 => v.integer_scale = !v.integer_scale,
                    4 => v.vsync = !v.vsync,
                    _ => v.color_correction = !v.color_correction,
                }
                Command::None
            }
            Page::Audio => {
                let a = &mut d.audio;
                match self.cursor {
                    0 => a.volume = step(a.volume as i32, delta * 10, 0, 100, confirm) as u8,
                    1 => a.mute = !a.mute,
                    _ => {
                        a.latency_ms = step(
                            a.latency_ms as i32,
                            delta * 20,
                            LATENCY_RANGE.0 as i32,
                            LATENCY_RANGE.1 as i32,
                            confirm,
                        ) as u16
                    }
                }
                Command::None
            }
            Page::Controls => {
                let buttons = BUTTON_LABELS.len() * 2;
                if self.cursor < buttons {
                    if confirm {
                        self.remap = Some(Remap {
                            button: self.cursor / 2,
                            pad: self.cursor % 2 == 1,
                        });
                    }
                } else if self.cursor == buttons {
                    let dz = (d.input.deadzone * 20.0).round() as i32;
                    let max = (DEADZONE_MAX * 20.0).round() as i32;
                    d.input.deadzone = step(dz, delta, 0, max, confirm) as f32 / 20.0;
                } else if confirm {
                    d.input = crate::settings::Input::default();
                }
                Command::None
            }
        }
    }

    /// Complete a pending remap with an SDL scancode name / gamepad string.
    /// Returns the settings to apply when a binding was recorded: remaps
    /// take effect and persist immediately so they cannot be lost by leaving
    /// the page (fixes the "binding not kept" bug).
    pub fn bind(&mut self, name: String, pad: bool) -> Option<Settings> {
        let r = self.remap?;
        if r.pad != pad || name.is_empty() {
            return None;
        }
        {
            let list = if pad {
                &mut self.draft.input.gamepad
            } else {
                &mut self.draft.input.keyboard
            };
            // A control maps to one GBA button: remove it from the others.
            for other in list.iter_mut() {
                other.retain(|n| n != &name);
            }
            list[r.button] = vec![name];
            self.remap = None;
        }
        self.effective = self.draft.clone();
        self.status = Some("Controle salvo".into());
        Some(self.draft.clone())
    }
}

/// Adjust a value by `delta`; `Confirm` wraps around, Left/Right clamp.
fn step(value: i32, delta: i32, lo: i32, hi: i32, wrap: bool) -> i32 {
    let next = value + delta;
    if wrap && next > hi {
        lo
    } else {
        next.clamp(lo, hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_is_discarded_on_cancel_and_back() {
        let mut m = Menu::new(Settings::default());
        m.open_at(Page::Main, &Settings::default());
        m.cursor = 1;
        assert_eq!(m.handle(Nav::Confirm), Command::None);
        assert_eq!(m.page, Page::Video);
        m.handle(Nav::Right); // scale 3 → 4
        assert_eq!(m.draft.video.scale, 4);
        assert_eq!(m.handle(Nav::Back), Command::None);
        assert_eq!(m.page, Page::Main);
        assert_eq!(m.draft.video.scale, 3);
    }

    #[test]
    fn apply_returns_draft_and_becomes_effective() {
        let mut m = Menu::new(Settings::default());
        m.open_at(Page::Audio, &Settings::default());
        m.handle(Nav::Left); // volume 100 → 90
        m.cursor = 1;
        m.handle(Nav::Confirm); // mute
        let n = m.rows().len();
        m.cursor = n - 2;
        match m.handle(Nav::Confirm) {
            Command::Apply(s) => {
                assert_eq!(s.audio.volume, 90);
                assert!(s.audio.mute);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(m.handle(Nav::Back), Command::Resume);
    }

    #[test]
    fn ranges_are_clamped() {
        let mut m = Menu::new(Settings::default());
        m.open_at(Page::Video, &Settings::default());
        for _ in 0..20 {
            m.handle(Nav::Right);
        }
        assert_eq!(m.draft.video.scale, SCALE_RANGE.1);
        for _ in 0..20 {
            m.handle(Nav::Left);
        }
        assert_eq!(m.draft.video.scale, SCALE_RANGE.0);
    }

    #[test]
    fn remap_waits_and_cancel_keeps_previous_binding() {
        let mut m = Menu::new(Settings::default());
        m.open_at(Page::Controls, &Settings::default());
        m.handle(Nav::Confirm); // A keyboard
        assert_eq!(
            m.remap,
            Some(Remap {
                button: 0,
                pad: false
            })
        );
        assert_eq!(
            m.handle(Nav::Down),
            Command::None,
            "navigation blocked while waiting"
        );
        m.handle(Nav::Back);
        assert_eq!(m.remap, None);
        assert_eq!(m.draft.input.keyboard[0], vec!["Z".to_owned()]);
        m.handle(Nav::Confirm);
        assert!(m.bind("X".into(), true).is_none(), "wrong device is ignored");
        assert!(m.remap.is_some());
        let applied = m.bind("X".into(), false).expect("binding applies immediately");
        assert_eq!(applied.input.keyboard[0], vec!["X".to_owned()]);
        // Leaving the page no longer discards the binding.
        m.handle(Nav::Back);
        assert_eq!(m.draft.input.keyboard[0], vec!["X".to_owned()]);
        assert_eq!(m.draft.input.keyboard[0], vec!["X".to_owned()]);
        // X was B's key: moved, not duplicated.
        assert!(m.draft.input.keyboard[1].is_empty());
    }

    #[test]
    fn main_actions() {
        let mut m = Menu::new(Settings::default());
        m.open_at(Page::Main, &Settings::default());
        m.cursor = 4;
        assert_eq!(m.handle(Nav::Confirm), Command::Reset);
        m.open_at(Page::Main, &Settings::default());
        m.cursor = 6;
        assert_eq!(m.handle(Nav::Confirm), Command::Quit);
    }
}

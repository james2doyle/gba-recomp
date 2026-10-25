//! In-window settings menu (SPEC-063 MENU-63-01/02). Pure model: the app
//! feeds navigation and draws `rows()`. Labels come from `gba-i18n`, so the
//! menu follows the selected language live (SPEC-069 L10N-69-03).

use gba_i18n::{Lang, tr};

use crate::settings::{
    BUTTON_LABEL_KEYS, DEADZONE_MAX, Filter, LATENCY_RANGE, SCALE_RANGE, Settings, button_label,
};

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
    /// Open the external settings window (EXT-64-02).
    OpenExternal,
    /// Make the draft effective and persist it (UI-02).
    Apply(Settings),
}

/// Pending remap: GBA button index and whether it is a gamepad binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Remap {
    pub button: usize,
    pub pad: bool,
}

#[derive(Debug)]
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

/// Rows of the main page, in display order. Named so inserting a row cannot
/// silently shift the `activate` arms.
mod main_row {
    pub const RESUME: usize = 0;
    pub const VIDEO: usize = 1;
    pub const AUDIO: usize = 2;
    pub const CONTROLS: usize = 3;
    pub const EXTERNAL: usize = 4;
    pub const RESET: usize = 5;
    pub const SAVE: usize = 6;
    pub const QUIT: usize = 7;
    pub const LANGUAGE: usize = 8;
}

fn yes(b: bool) -> String {
    if b { tr!("yes") } else { tr!("no") }
}

/// Language label shown on the main page: the language's own name, so a user
/// who cannot read the current one can still find their language.
fn language_name(lang: Lang) -> String {
    lang.display_name().to_owned()
}

/// The language `delta` steps away in the picker, wrapping both ways.
fn next_language(current: Lang, delta: i32) -> Lang {
    let all = Lang::all();
    let n = all.len() as i32;
    let at = all.iter().position(|l| *l == current).unwrap_or(0) as i32;
    // Rem_euclid keeps Right on the last entry going to the first and Left on
    // the first going to the last, for any number of languages.
    all[((at + delta).rem_euclid(n)) as usize]
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

    /// Adopts a language chosen outside the menu (settings window, or the OS
    /// language changing), so the menu and the draft stay consistent
    /// (SPEC-069 L10N-69-04).
    pub fn set_language(&mut self, lang: Lang) {
        self.draft.language = lang;
        self.effective.language = lang;
    }

    /// Adopts settings written outside this menu (SPEC-069 L10N-69-03).
    ///
    /// Unapplied edits here are never discarded, but the language always
    /// follows: it is host state, not an edit made in this window. Without
    /// the effective refresh, the next `Apply` would write stale values back
    /// and silently revert the external change.
    pub fn adopt_settings(&mut self, s: Settings) {
        if self.draft == self.effective {
            self.draft = s.clone();
        } else {
            self.draft.language = s.language;
        }
        self.effective = s;
    }

    /// The language the menu will fall back to when reopened, for tests that
    /// check it followed an external change.
    #[cfg(test)]
    pub fn effective_language(&self) -> Lang {
        self.effective.language
    }

    pub fn close(&mut self) {
        self.open = false;
        self.remap = None;
        self.draft = self.effective.clone();
    }

    pub fn title(&self) -> String {
        match self.page {
            Page::Main => tr!("menu.title"),
            Page::Video => tr!("menu.page.video"),
            Page::Audio => tr!("menu.page.audio"),
            Page::Controls => tr!("menu.page.controls"),
        }
    }

    pub fn rows(&self) -> Vec<Row> {
        let row = |l: String, v: String| Row { label: l, value: v };
        let d = &self.draft;
        let mut rows = match self.page {
            Page::Main => vec![
                row(tr!("menu.resume"), String::new()),
                row(tr!("menu.page.video"), String::new()),
                row(tr!("menu.page.audio"), String::new()),
                row(tr!("menu.page.controls"), String::new()),
                row(tr!("menu.external"), String::new()),
                row(tr!("menu.reset"), String::new()),
                row(tr!("menu.save_now"), String::new()),
                row(tr!("menu.quit"), String::new()),
                // Last, and available on every platform: the settings window
                // does not exist on Android or the Web (SPEC-069 L10N-69-03).
                row(tr!("menu.language"), language_name(d.language)),
            ],
            Page::Video => vec![
                row(tr!("menu.video.scale"), format!("{}x", d.video.scale)),
                row(tr!("menu.video.fullscreen"), yes(d.video.fullscreen)),
                row(
                    tr!("menu.video.filter"),
                    if d.video.filter == Filter::Linear {
                        tr!("menu.video.filter_linear")
                    } else {
                        tr!("menu.video.filter_nearest")
                    },
                ),
                row(tr!("menu.video.integer_scale"), yes(d.video.integer_scale)),
                row(tr!("menu.video.vsync"), yes(d.video.vsync)),
                row(
                    tr!("menu.video.color_correction"),
                    yes(d.video.color_correction),
                ),
            ],
            Page::Audio => vec![
                row(tr!("menu.audio.volume"), format!("{}%", d.audio.volume)),
                row(tr!("menu.audio.mute"), yes(d.audio.mute)),
                row(
                    tr!("menu.audio.latency"),
                    format!("{} ms", d.audio.latency_ms),
                ),
            ],
            Page::Controls => {
                let mut v = Vec::new();
                for i in 0..BUTTON_LABEL_KEYS.len() {
                    // Two-level translation: the button's own name, then the
                    // row template, so English gets "Right keyboard" and
                    // Portuguese "Direita teclado".
                    let label = button_label(i);
                    let waiting = |pad| self.remap == Some(Remap { button: i, pad });
                    v.push(row(
                        tr!("menu.controls.keyboard", button = label),
                        if waiting(false) {
                            tr!("menu.controls.waiting_key")
                        } else {
                            d.input.keyboard[i].join(", ")
                        },
                    ));
                    v.push(row(
                        tr!("menu.controls.gamepad", button = label),
                        if waiting(true) {
                            tr!("menu.controls.waiting_pad")
                        } else {
                            d.input.gamepad[i].join(", ")
                        },
                    ));
                }
                v.push(row(
                    tr!("menu.controls.deadzone"),
                    format!("{:.2}", d.input.deadzone),
                ));
                v.push(row(
                    tr!("menu.controls.touch"),
                    match d.input.touch_controls {
                        crate::settings::TouchMode::Auto => tr!("auto"),
                        crate::settings::TouchMode::On => tr!("yes"),
                        crate::settings::TouchMode::Off => tr!("no"),
                    },
                ));
                v.push(row(tr!("menu.controls.restore"), String::new()));
                v
            }
        };
        if self.page != Page::Main {
            rows.push(row(tr!("menu.apply"), String::new()));
            rows.push(row(tr!("menu.cancel"), String::new()));
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

    /// Steps the language and applies it at once, so the menu re-renders in
    /// the new language immediately (SPEC-069 L10N-69-03).
    ///
    /// Returns `Apply` so the choice is persisted: on Android and the Web
    /// this row is the only language picker, and a pick that vanished on exit
    /// would be worse than no picker at all.
    fn cycle_language(&mut self, delta: i32) -> Command {
        let next = next_language(self.draft.language, delta);
        self.draft.language = next;
        self.effective.language = next;
        gba_i18n::set_lang(next);
        self.status = Some(language_name(next));
        Command::Apply(self.draft.clone())
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
                self.status = Some(tr!("menu.status.applied"));
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
                // The language row cycles with Left/Right as well as Confirm,
                // like the other enumerations (SPEC-069 L10N-69-03).
                if self.cursor == main_row::LANGUAGE {
                    return self.cycle_language(delta);
                }
                if !confirm {
                    return Command::None;
                }
                match self.cursor {
                    main_row::RESUME => {
                        self.close();
                        Command::Resume
                    }
                    main_row::VIDEO => {
                        self.go(Page::Video);
                        Command::None
                    }
                    main_row::AUDIO => {
                        self.go(Page::Audio);
                        Command::None
                    }
                    main_row::CONTROLS => {
                        self.go(Page::Controls);
                        Command::None
                    }
                    main_row::EXTERNAL => Command::OpenExternal,
                    main_row::RESET => {
                        self.close();
                        Command::Reset
                    }
                    main_row::SAVE => Command::SaveNow,
                    main_row::QUIT => Command::Quit,
                    // Unreachable: every index is handled above.
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
                let buttons = BUTTON_LABEL_KEYS.len() * 2;
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
                } else if self.cursor == buttons + 1 {
                    use crate::settings::TouchMode::*;
                    d.input.touch_controls = match (d.input.touch_controls, delta > 0) {
                        (Auto, true) => On,
                        (On, true) => Off,
                        (Off, true) => Auto,
                        (Auto, false) => Off,
                        (On, false) => Auto,
                        (Off, false) => On,
                    };
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
        self.status = Some(tr!("menu.status.bound"));
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
        assert!(
            m.bind("X".into(), true).is_none(),
            "wrong device is ignored"
        );
        assert!(m.remap.is_some());
        let applied = m
            .bind("X".into(), false)
            .expect("binding applies immediately");
        assert_eq!(applied.input.keyboard[0], vec!["X".to_owned()]);
        // Leaving the page no longer discards the binding.
        m.handle(Nav::Back);
        assert_eq!(m.draft.input.keyboard[0], vec!["X".to_owned()]);
        assert_eq!(m.draft.input.keyboard[0], vec!["X".to_owned()]);
        // X was B's key: moved, not duplicated.
        assert!(m.draft.input.keyboard[1].is_empty());
    }

    #[test]
    fn labels_are_translated_and_accented() {
        let _locale = crate::locale_guard::lock();
        let mut m = Menu::new(Settings::default());
        m.open_at(Page::Main, &Settings::default());
        let rows = m.rows();
        assert_eq!(rows[0].label, "Continuar");
        assert_eq!(rows[1].label, "Vídeo", "accented, now that the font has it");
        assert_eq!(m.title(), "Menu");
        // The video page renders accented values too.
        m.open_at(Page::Video, &Settings::default());
        let rows = m.rows();
        assert_eq!(rows[0].label, "Escala");
        assert_eq!(rows[2].value, "Nítido");
        assert_eq!(rows[1].value, "Não");
        assert_eq!(m.title(), "Vídeo");
    }

    #[test]
    fn every_page_has_a_translated_title() {
        let _locale = crate::locale_guard::lock();
        for (lang, wants) in [
            (Lang::PtBr, ["Menu", "Vídeo", "Áudio", "Controles"]),
            (Lang::EnUs, ["Menu", "Video", "Audio", "Controls"]),
        ] {
            gba_i18n::set_lang(lang);
            for (page, want) in [Page::Main, Page::Video, Page::Audio, Page::Controls]
                .into_iter()
                .zip(wants)
            {
                let mut m = Menu::new(Settings::default());
                m.open_at(page, &Settings::default());
                assert_eq!(m.title(), want, "{page:?} in {}", lang.tag());
                assert!(!m.rows().is_empty(), "{page:?} has no rows");
            }
        }
    }

    /// The language row is the only picker on Android and the Web, where the
    /// settings window does not exist, so it must be reachable.
    #[test]
    fn main_page_exposes_a_language_picker() {
        let _locale = crate::locale_guard::lock();
        let mut m = Menu::new(Settings::default());
        m.open_at(Page::Main, &Settings::default());
        let rows = m.rows();
        let at = rows
            .iter()
            .position(|r| r.label == "Idioma")
            .expect("main page must offer a language row");
        assert_eq!(rows[at].value, Lang::PtBr.display_name());
        // Cycling wraps without changing the draft's other fields.
        let before = m.draft.clone();
        m.cursor = at;
        // Changing language must persist, or the pick is lost on exit.
        match m.handle(Nav::Right) {
            Command::Apply(s) => assert_eq!(s.language, next_language(Lang::PtBr, 1)),
            other => panic!("language must apply, got {other:?}"),
        }
        assert_eq!(m.draft.language, next_language(Lang::PtBr, 1));
        assert_eq!(m.draft.video, before.video, "other settings untouched");
        assert_eq!(gba_i18n::lang(), m.draft.language, "applied immediately");
    }

    #[test]
    fn language_cycling_wraps_both_ways() {
        let _locale = crate::locale_guard::lock();
        // Two languages: each direction reaches the other, and continues.
        assert_eq!(next_language(Lang::PtBr, 1), Lang::EnUs);
        assert_eq!(next_language(Lang::EnUs, 1), Lang::PtBr);
        assert_eq!(next_language(Lang::PtBr, -1), Lang::EnUs);
        assert_eq!(next_language(Lang::EnUs, -1), Lang::PtBr);
        // The wrap arithmetic must work for any count, so check it directly.
        for n in 1..5i32 {
            for at in 0..n {
                assert_eq!((at + 1).rem_euclid(n), (at + 1) % n);
                assert_eq!((at - 1).rem_euclid(n), if at == 0 { n - 1 } else { at - 1 });
            }
        }
    }

    /// A picker that changes the saved language but not the drawn text would
    /// look broken to the user; this is the whole point of SPEC-069.
    #[test]
    fn switching_to_english_retranslates_every_row() {
        let _locale = crate::locale_guard::lock();
        let mut m = Menu::new(Settings::default());
        m.open_at(Page::Main, &Settings::default());
        let at = m
            .rows()
            .iter()
            .position(|r| r.label == "Idioma")
            .expect("language row");
        m.cursor = at;
        match m.handle(Nav::Right) {
            Command::Apply(s) => assert_eq!(s.language, Lang::EnUs),
            other => panic!("{other:?}"),
        }
        let rows = m.rows();
        // The label itself is retranslated, and so is every other row.
        assert_eq!(rows[at].label, "Language");
        assert_eq!(rows[at].value, "English (US)");
        assert_eq!(rows[0].label, "Resume");
        assert_eq!(rows[1].label, "Video", "no accent left over from pt-BR");

        // And the controls page, where the button names are themselves
        // translated before being interpolated.
        m.open_at(Page::Controls, &Settings::default());
        let rows = m.rows();
        assert!(rows.iter().any(|r| r.label == "Right keyboard"), "{rows:?}");
        assert!(rows.iter().any(|r| r.label == "Select gamepad"), "{rows:?}");

        // Back to Portuguese: same rows, Portuguese again.
        gba_i18n::set_lang(Lang::PtBr);
        let rows = m.rows();
        assert!(
            rows.iter().any(|r| r.label == "Direita teclado"),
            "{rows:?}"
        );
    }

    #[test]
    fn main_actions() {
        let mut m = Menu::new(Settings::default());
        m.open_at(Page::Main, &Settings::default());
        m.cursor = 4;
        assert_eq!(m.handle(Nav::Confirm), Command::OpenExternal);
        m.cursor = 5;
        assert_eq!(m.handle(Nav::Confirm), Command::Reset);
        m.open_at(Page::Main, &Settings::default());
        m.cursor = 7;
        assert_eq!(m.handle(Nav::Confirm), Command::Quit);
    }

    /// An external write is tracked but never overwrites unapplied edits,
    /// and the language follows regardless (SPEC-069 L10N-69-03).
    #[test]
    fn adopt_settings_keeps_edits_and_follows_the_file() {
        let mut m = Menu::new(Settings::default());
        m.open_at(Page::Main, &Settings::default());
        // No pending edits: the draft follows the external write whole.
        let mut s = Settings::default();
        s.audio.volume = 50;
        s.language = gba_i18n::Lang::EnUs;
        m.adopt_settings(s.clone());
        assert_eq!(m.draft, s, "a clean draft tracks the file");
        assert_eq!(m.effective, s);

        // With a pending edit, only the language follows.
        m.draft.audio.volume = 42;
        let mut t = s;
        t.audio.volume = 30;
        t.language = gba_i18n::Lang::PtBr;
        m.adopt_settings(t.clone());
        assert_eq!(m.draft.audio.volume, 42, "unsaved edit kept");
        assert_eq!(m.draft.language, gba_i18n::Lang::PtBr, "language follows");
        assert_eq!(m.effective, t, "the file state is tracked, not clobbered");
    }
}

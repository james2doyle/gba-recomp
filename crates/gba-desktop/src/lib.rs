//! Desktop frontend library (SPEC-061 FE-61-01, SPEC-063): SDL3 window,
//! audio and input, settings, in-window menu and native context menu.
//! Used by the interpreted `gba-desktop` binary and by recompiled packages
//! through `gba-native-rt`. Host preferences never change the simulation.

#![deny(unsafe_code)]

pub mod app;
pub mod assets;
pub mod ctxmenu;
pub mod font;
pub mod gpio;
pub mod input;
pub mod menu;
pub mod pace;
pub mod sdl;
pub mod settings;
pub mod touch;

pub use app::{AppOptions, AppReport, run_app};

/// Serializes tests that read translated text (SPEC-069).
///
/// The rust-i18n locale is process-global and tests run in parallel, so a
/// test that switches language would change what a sibling test reads. Hold
/// this for the duration of such a test; the guard restores the default on
/// drop, so a panic cannot leak a language into the next test.
#[cfg(test)]
pub(crate) mod locale_guard {
    use std::sync::{Mutex, MutexGuard};

    static LOCALE: Mutex<()> = Mutex::new(());

    pub struct Guard(#[allow(dead_code)] MutexGuard<'static, ()>);

    pub fn lock() -> Guard {
        let guard = LOCALE.lock().unwrap_or_else(|e| e.into_inner());
        gba_i18n::set_lang(gba_i18n::Lang::default());
        Guard(guard)
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            gba_i18n::set_lang(gba_i18n::Lang::default());
        }
    }
}

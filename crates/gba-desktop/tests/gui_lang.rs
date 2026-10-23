//! How the two GUI frontends pick their language at startup (SPEC-069
//! L10N-69-04). `gba-settings` and `gba-party` ask for the OS language before
//! any SDL context exists, because the window title is drawn once.

use gba_desktop::{sdl, settings};

#[test]
fn first_run_follows_the_environment_language() {
    let dir = std::env::temp_dir().join(format!("gba-gui-lang-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.toml");
    // No file: nothing saved, so the system decides.
    let loaded = settings::load(&path);
    let (lang, chosen) = settings::initial_language(Some(&path), &loaded.settings, &[]);
    assert!(!chosen, "nothing was saved");
    // Whatever the system reports, we agree with it.
    let system = sdl::preferred_locales();
    assert_eq!(lang, gba_i18n::detect_from(&system).unwrap_or_default());
    // On a machine with an English locale the answer must be English, not
    // the pt-BR default. The environment decides whether this applies, so a
    // machine without one checks only the agreement above.
    if system
        .iter()
        .any(|t| t.to_ascii_lowercase().starts_with("en"))
    {
        assert_eq!(lang, gba_i18n::Lang::EnUs, "system={system:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Both companion apps call this before `Context::init`, where the window
/// title is translated. Asking early must give the same answer, not nothing.
#[test]
fn the_os_language_can_be_read_before_sdl_is_initialised() {
    let before = sdl::preferred_locales();
    let ctx = sdl::Context::init().expect("SDL init");
    let after = sdl::preferred_locales();
    drop(ctx);
    assert_eq!(before, after, "asking before init changed the answer");
}

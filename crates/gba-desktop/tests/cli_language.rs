//! The CLI picks its language from `$LANG` before SDL starts
//! (SPEC-069 L10N-69-04). Both shipped languages must reach `--help`, and an
//! unsupported one must fall back to the pt-BR default rather than print
//! nothing.

use std::process::Command;

fn run(lang: Option<&str>) -> (String, String, bool) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_gba-desktop"));
    cmd.arg("--help");
    cmd.env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANG");
    if let Some(l) = lang {
        cmd.env("LANG", l);
    }
    let out = cmd.output().expect("run gba-desktop");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

#[test]
fn help_lists_the_flags_and_is_accented() {
    let (stdout, stderr, ok) = run(Some("pt_BR.UTF-8"));
    let text = format!("{stdout}{stderr}");
    assert!(!ok, "--help exits nonzero by design");
    for flag in [
        "--rom",
        "--save",
        "--bios",
        "--backup",
        "--settings",
        "--frames",
    ] {
        assert!(text.contains(flag), "help must mention {flag}: {text}");
    }
    // The usage line is accented now that the menu font has the glyphs.
    assert!(text.contains("uso:"), "{text}");
    assert!(text.contains("botão direito"), "{text}");
}

#[test]
fn english_system_language_prints_english_help() {
    let (stdout, stderr, _) = run(Some("en_US.UTF-8"));
    let text = format!("{stdout}{stderr}");
    assert!(text.contains("usage:"), "{text}");
    assert!(text.contains("right click"), "{text}");
    assert!(
        !text.contains("botão direito"),
        "no Portuguese left in the English help: {text}"
    );
}

#[test]
fn an_unsupported_system_language_falls_back_to_portuguese() {
    // Japanese has no translation, so it must fall back to the default.
    let (pt, _, _) = run(Some("pt_BR.UTF-8"));
    let (ja, _, _) = run(Some("ja_JP.UTF-8"));
    let (none, _, _) = run(None);
    let join = |t: &str| t.trim().to_owned();
    assert_eq!(join(&pt), join(&ja), "ja must fall back to pt-BR");
    assert_eq!(join(&pt), join(&none), "unset LANG must also fall back");
}

#[test]
fn regional_english_and_portuguese_do_not_fall_back() {
    // `en_GB` and `pt_PT` have no translation of their own, but the language
    // does; the user must get it rather than the unrelated default.
    let text = |t: (String, String, bool)| format!("{}{}", t.0, t.1);
    let en_us = text(run(Some("en_US.UTF-8")));
    let en_gb = text(run(Some("en_GB.UTF-8")));
    assert_eq!(en_us.trim(), en_gb.trim());
    assert!(en_gb.contains("usage:"), "{en_gb}");
    let pt_pt = text(run(Some("pt_PT.UTF-8")));
    assert!(pt_pt.contains("uso:"), "{pt_pt}");
    assert_ne!(
        en_gb.trim(),
        pt_pt.trim(),
        "the two regions must not collapse into one language"
    );
}

#[test]
fn unknown_arguments_are_reported_in_the_selected_language() {
    for (lang, want) in [
        ("pt_BR.UTF-8", "argumento desconhecido"),
        ("en_US.UTF-8", "unknown argument"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_gba-desktop"))
            .arg("--bogus")
            .env("LANG", lang)
            .env_remove("LC_ALL")
            .env_remove("LC_MESSAGES")
            .output()
            .expect("run");
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(text.contains(want), "LANG={lang}: {text}");
        assert!(text.contains("--bogus"), "LANG={lang}: {text}");
    }
}

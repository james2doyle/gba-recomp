//! O macro `tr!` precisa atravessar a fronteira de crates (SPEC-069
//! L10N-69-01): `gba-desktop`, `gba-settings` e `gba-party` o usam de fora.

use gba_i18n::{Lang, tr};

#[test]
fn macro_works_outside_the_defining_crate() {
    gba_i18n::set_lang(Lang::PtBr);
    assert_eq!(tr!("yes"), "Sim");
    assert_eq!(tr!("no"), "Não");
    assert_eq!(tr!("menu.page.video"), "Vídeo");
}

#[test]
fn interpolation_works_outside_the_defining_crate() {
    gba_i18n::set_lang(Lang::PtBr);
    assert_eq!(tr!("menu.controls.keyboard", button = "A"), "A teclado");
    assert_eq!(tr!("menu.controls.gamepad", button = "L"), "L controle");
}

#[test]
fn missing_keys_degrade_to_the_key() {
    // The raw key, not empty and not "pt-BR.no.such.key": what the user sees
    // must name the missing key exactly (SPEC-069 "Erros, limites").
    assert_eq!(tr!("no.such.key"), "no.such.key");
}

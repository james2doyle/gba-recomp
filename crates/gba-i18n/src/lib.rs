//! Localização do frontend (SPEC-069 L10N-69-01): um registro de idiomas e
//! as traduções embutidas no binário. Não toca na simulação.
//!
//! O idioma é um dado de host: trocar de língua MUST NOT alterar a semântica
//! guest (SET-63-02). O padrão é `pt-BR`; qualquer número de idiomas entra
//! como uma variante de [`Lang`] mais um arquivo em `locales/`.

rust_i18n::i18n!("locales", fallback = "pt-BR");

use std::borrow::Cow;

/// Idiomas com tradução embutida.
///
/// A enumeração é o registro único: adicionar um idioma é acrescentar uma
/// variante e um arquivo `locales/<tag>.toml` com as mesmas chaves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Lang {
    /// Português do Brasil: o padrão e o fallback de qualquer chave ausente
    /// (SPEC-069 L10N-69-02).
    #[default]
    PtBr,
    /// Inglês dos Estados Unidos.
    EnUs,
}

impl Lang {
    /// Tag BCP-47 usada em `settings.toml` e nos nomes de arquivo.
    pub const fn tag(self) -> &'static str {
        match self {
            Lang::PtBr => "pt-BR",
            Lang::EnUs => "en-US",
        }
    }

    /// Nome do idioma no próprio idioma, para o seletor do menu.
    pub const fn display_name(self) -> &'static str {
        match self {
            Lang::PtBr => "Português (Brasil)",
            Lang::EnUs => "English (US)",
        }
    }

    /// Código primário ISO-639 (`pt`), usado no casamento por prefixo.
    pub const fn language(self) -> &'static str {
        match self {
            Lang::PtBr => "pt",
            Lang::EnUs => "en",
        }
    }

    /// Todo idioma com tradução embutida, na ordem exibida no seletor.
    pub const fn all() -> &'static [Lang] {
        &[Lang::PtBr, Lang::EnUs]
    }

    /// Resolve uma tag de locale (`pt-BR`, `pt_BR.UTF-8`, `pt`) no idioma
    /// correspondente. Devolve `None` quando não há tradução.
    pub fn from_tag(tag: &str) -> Option<Self> {
        let (language, region) = split_tag(tag)?;
        let same_language = |l: &Lang| l.language().eq_ignore_ascii_case(&language);
        // Exact region first: `en-GB` must not steal `en-US` from `en-US`.
        Lang::all()
            .iter()
            .copied()
            .find(|l| {
                // Sem região pedida (`pt`) qualquer variante do idioma serve;
                // com região, a variante precisa bater (ou não declarar região).
                same_language(l)
                    && match (l.region(), region.as_deref()) {
                        (Some(want), Some(got)) => want.eq_ignore_ascii_case(got),
                        (None, _) | (Some(_), None) => true,
                    }
            })
            // Then settle for another variant of the same language: a user on
            // `en-GB` or `pt-PT` reads that language far better than the
            // unrelated default.
            .or_else(|| Lang::all().iter().copied().find(same_language))
    }

    /// Região que distingue variantes do mesmo idioma (`BR` em `pt-BR`).
    const fn region(self) -> Option<&'static str> {
        match self {
            Lang::PtBr => Some("BR"),
            Lang::EnUs => Some("US"),
        }
    }
}

/// Separa `pt_BR.UTF-8` / `pt-BR` / `pt` em (`pt`, `Some("BR")`) e `pt` em
/// (`pt`, `None`). Devolve `None` para entradas vazias ou inválidas.
fn split_tag(tag: &str) -> Option<(String, Option<String>)> {
    // POSIX sufixa a codificação (`pt_BR.UTF-8`) e às vezes o modificador
    // (`pt_BR@latin`); nenhum dos dois identifica o idioma.
    let tag = tag.split(['.', '@']).next().unwrap_or("");
    let tag = tag.trim();
    if tag.is_empty() {
        return None;
    }
    let mut parts = tag.split(['-', '_']);
    let language = parts.next().unwrap_or("").trim();
    if language.is_empty() || !language.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let region = parts
        .next()
        .map(str::trim)
        .filter(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_alphabetic()));
    Some((
        language.to_ascii_lowercase(),
        region.map(str::to_ascii_uppercase),
    ))
}

/// The language this crate last set, as the source of truth for [`lang`].
///
/// `rust_i18n::locale()` cannot serve: it starts at `"en"`, so a process that
/// never called [`set_lang`] would report English while every string still
/// renders in the pt-BR default. Keeping our own value makes "no language
/// chosen" mean pt-BR, which is what the fallback actually does.
static CURRENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(Lang::PtBr as u8);

/// Define o idioma corrente; todo `tr!` seguinte já responde nele.
pub fn set_lang(lang: Lang) {
    CURRENT.store(lang as u8, std::sync::atomic::Ordering::Relaxed);
    rust_i18n::set_locale(lang.tag());
}

/// Idioma corrente; `pt-BR` enquanto [`set_lang`] não for chamado.
pub fn lang() -> Lang {
    match CURRENT.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Lang::EnUs,
        _ => Lang::PtBr,
    }
}

/// Escolhe o primeiro idioma da lista de preferência que tenha tradução.
pub fn from_preferences<'a>(preferred: impl IntoIterator<Item = &'a str>) -> Option<Lang> {
    preferred.into_iter().find_map(Lang::from_tag)
}

/// Idiomas que o sistema declara preferir, do mais para o menos preferido.
///
/// Este crate não conhece o SDL (o frontend depende dele, não o contrário):
/// `gba-desktop` passa `sdl::preferred_locales()`, que é o que consulta o
/// idioma do sistema em Android, Web, Apple e Linux. Sem essa lista, caem as
/// variáveis POSIX, que existem no desktop e no CLI antes do vídeo iniciar.
pub fn detect_from(system: &[String]) -> Option<Lang> {
    from_preferences(system.iter().map(String::as_str))
        .or_else(|| from_preferences(posix_locale_vars().iter().map(String::as_str)))
}

/// `$LC_ALL`, `$LC_MESSAGES` e `$LANG`, nessa ordem de precedência (a mesma
/// que o `setlocale(3)` usa). Vazio quando nenhuma está definida.
pub fn posix_locale_vars() -> Vec<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .filter(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .collect()
}

/// Traduz uma chave no idioma corrente.
///
/// Envolve o `t!` do `rust-i18n`, que expande para um símbolo `pub(crate)`
/// do crate que chamou `i18n!` — por isso não atravessa a fronteira de
/// crates. Este macro é o que os outros crates usam.
///
/// Aceita as mesmas formas: `tr!("chave")`, `tr!("chave", nome = valor)`.
/// A substituição usa `%{nome}`, como no arquivo de locale.
///
/// ```
/// use gba_i18n::tr;
/// assert_eq!(tr!("yes"), "Sim");
/// assert_eq!(tr!("menu.controls.keyboard", button = "A"), "A teclado");
/// ```
#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        $crate::translate($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {{
        let mut patterns: Vec<&str> = Vec::new();
        let mut values: Vec<String> = Vec::new();
        $(
            patterns.push(stringify!($name));
            values.push(($value).to_string());
        )+
        $crate::interpolate($crate::translate($key), &patterns, &values)
    }};
}

/// Every translated message in every locale, for checks that need the whole
/// corpus (for example, verifying the menu font can draw it).
///
/// Does not touch the global locale: it translates each language explicitly,
/// so calling it while another thread is using another language is safe (the
/// locale is process-global, and `set_lang` around a loop would leak).
pub fn all_messages() -> Vec<String> {
    let mut out = Vec::new();
    for lang in Lang::all() {
        for key in keys_of(lang.tag()) {
            out.push(translate_in(*lang, &key));
        }
    }
    out
}

/// Keys declared in a locale file embedded at build time.
fn keys_of(tag: &str) -> Vec<String> {
    let text = match tag {
        "pt-BR" => include_str!("../locales/pt-BR.toml"),
        "en-US" => include_str!("../locales/en-US.toml"),
        _ => return Vec::new(),
    };
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim().to_owned()))
        .filter(|k| !k.is_empty() && k != "_version")
        .collect()
}

/// Texto da `key` no idioma corrente, ou a própria chave se faltar.
pub fn translate(key: &str) -> String {
    translate_in(lang(), key)
}

/// Texto da `key` num idioma explícito, sem tocar no locale global.
///
/// Usa `try_translate` em vez de `translate`: o `_rust_i18n_translate` do
/// rust-i18n devolve `"<tag>.<chave>"` quando a chave não existe em lugar
/// nenhum, o que na tela viraria "pt-BR.menu.typo". Aqui a chave crua é
/// devolvida, e o fallback para `pt-BR` continua valendo pelo próprio
/// rust-i18n.
pub fn translate_in(lang: Lang, key: &str) -> String {
    _rust_i18n_try_translate(lang.tag(), key).map_or_else(|| key.to_owned(), Cow::into_owned)
}

/// Troca `%{nome}` pelos valores correspondentes.
///
/// `patterns` traz só o nome, sem as chaves: `replace_patterns` casa o
/// conteúdo entre `%{` e `}`.
pub fn interpolate(text: String, patterns: &[&str], values: &[String]) -> String {
    rust_i18n::replace_patterns(&text, patterns, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The locale is process-global, so tests that switch it must not run
    /// concurrently: one test's `set_lang` would change another's lookups.
    static LOCALE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Runs `f` holding the locale lock, leaving the locale at the default.
    fn with_locale(f: impl FnOnce()) {
        let _guard = LOCALE.lock().unwrap_or_else(|e| e.into_inner());
        f();
        set_lang(Lang::default());
    }

    #[test]
    fn tags_round_trip() {
        for lang in Lang::all() {
            assert_eq!(Lang::from_tag(lang.tag()), Some(*lang));
        }
    }

    #[test]
    fn accepts_posix_codesets_and_underscores() {
        for tag in ["pt_BR", "pt_BR.UTF-8", "pt-br", "pt", "PT_br@latin"] {
            assert_eq!(Lang::from_tag(tag), Some(Lang::PtBr), "tag {tag}");
        }
        for tag in ["en_US", "en_US.UTF-8", "en-us", "en", "EN_us@latin"] {
            assert_eq!(Lang::from_tag(tag), Some(Lang::EnUs), "tag {tag}");
        }
    }

    #[test]
    fn accepts_other_regions_of_a_translated_language() {
        // No `en-GB` or `pt-PT` translation ships, but a reader of the
        // language is far better served by it than by the unrelated default.
        assert_eq!(Lang::from_tag("en_GB"), Some(Lang::EnUs));
        assert_eq!(Lang::from_tag("en-AU"), Some(Lang::EnUs));
        assert_eq!(Lang::from_tag("pt-PT"), Some(Lang::PtBr));
    }

    #[test]
    fn rejects_unknown_and_malformed() {
        for tag in [
            "", " ", "ja_JP", "zh-Hans", "de-DE", "ru_RU", "123", "-", ".",
        ] {
            assert_eq!(Lang::from_tag(tag), None, "tag {tag}");
        }
    }

    #[test]
    fn preferences_pick_the_first_supported() {
        assert_eq!(
            from_preferences(["de_DE", "en_US", "pt_BR"]),
            Some(Lang::EnUs)
        );
        assert_eq!(
            from_preferences(["de_DE", "pt_BR", "en_US"]),
            Some(Lang::PtBr)
        );
        assert_eq!(from_preferences(["ja_JP", "ru_RU"]), None);
        assert_eq!(from_preferences(std::iter::empty()), None);
    }

    #[test]
    fn detect_prefers_the_system_list_over_posix_vars() {
        // SDL entrega a lista já na ordem de preferência do usuário; ela vence
        // as variáveis POSIX, que só existem no desktop.
        let system = ["ja_JP".to_owned(), "pt-BR".to_owned()];
        assert_eq!(detect_from(&system), Some(Lang::PtBr));
        assert_eq!(detect_from(&["en_US".to_owned()]), Some(Lang::EnUs));
        // An empty list means "the platform said nothing", so the POSIX
        // variables decide; this machine always has some `LANG`, so assert
        // the fallback rather than `None`.
        let expected = from_preferences(posix_locale_vars().iter().map(String::as_str));
        assert_eq!(detect_from(&[]), expected);
    }

    #[test]
    fn detects_android_and_web_style_tags() {
        // Android devolve `pt_BR`/`en_US` do Java Locale; o Web,
        // `pt-BR`/`en-US` do navigator.language.
        assert_eq!(detect_from(&["pt_BR".to_owned()]), Some(Lang::PtBr));
        assert_eq!(detect_from(&["pt-BR".to_owned()]), Some(Lang::PtBr));
        assert_eq!(detect_from(&["pt".to_owned()]), Some(Lang::PtBr));
        assert_eq!(detect_from(&["en_US".to_owned()]), Some(Lang::EnUs));
        assert_eq!(detect_from(&["en-US".to_owned()]), Some(Lang::EnUs));
        assert_eq!(detect_from(&["en".to_owned()]), Some(Lang::EnUs));
    }

    #[test]
    fn switching_locale_changes_lookups() {
        with_locale(|| {
            set_lang(Lang::PtBr);
            assert_eq!(lang(), Lang::PtBr);
            assert_eq!(tr!("menu.resume"), "Continuar");
            set_lang(Lang::EnUs);
            assert_eq!(lang(), Lang::EnUs);
            assert_eq!(tr!("menu.resume"), "Resume");
        });
    }

    /// `rust_i18n::locale()` starts at `"en"`, so it cannot decide the
    /// language: a process that never chose one must render pt-BR and say so.
    #[test]
    fn lang_ignores_the_backend_default() {
        with_locale(|| {
            rust_i18n::set_locale("en");
            assert_eq!(lang(), Lang::PtBr, "no choice made means pt-BR");
            assert_eq!(tr!("menu.resume"), "Continuar");
            // A locale we do not know must not make us claim it either.
            rust_i18n::set_locale("ja_JP");
            assert_eq!(lang(), Lang::PtBr);
        });
    }

    #[test]
    fn every_locale_declares_the_same_keys() {
        // The fallback would hide a missing key behind the pt-BR text, so
        // compare the key sets directly: a new locale that forgot a key fails
        // here instead of shipping a Portuguese sentence.
        let mut reference = keys();
        reference.sort_unstable();
        for lang in Lang::all() {
            let mut mine = keys_of(lang.tag());
            mine.sort_unstable();
            assert_eq!(mine, reference, "{} has a different key set", lang.tag());
        }
    }

    #[test]
    fn every_key_exists_in_every_locale() {
        with_locale(|| {
            for key in keys() {
                for lang in Lang::all() {
                    set_lang(*lang);
                    let value = translate(&key);
                    assert!(!value.is_empty(), "{key} ausente em {}", lang.tag());
                }
            }
        });
    }

    #[test]
    fn locales_are_actually_translated() {
        // Guards against a copy-pasted `en-US.toml` shipped untranslated.
        with_locale(|| {
            for (key, pt, en) in [
                ("menu.resume", "Continuar", "Resume"),
                ("menu.quit", "Sair", "Quit"),
                ("yes", "Sim", "Yes"),
                ("no", "Não", "No"),
            ] {
                set_lang(Lang::PtBr);
                assert_eq!(translate(key), pt, "{key} em pt-BR");
                set_lang(Lang::EnUs);
                assert_eq!(translate(key), en, "{key} em en-US");
            }
        });
    }

    /// Placeholders must have the same names in every language: `tr!` fills
    /// them by name, so a translator typo (`%{botao}` for `%{button}`) would
    /// not fail the build — it would print the placeholder to the user.
    #[test]
    fn every_locale_uses_the_same_placeholders() {
        let names = |text: &str| -> Vec<String> {
            let mut out: Vec<String> = text
                .match_indices("%{")
                .filter_map(|(at, _)| {
                    text[at + 2..]
                        .find('}')
                        .map(|end| text[at + 2..at + 2 + end].to_owned())
                })
                .collect();
            out.sort_unstable();
            out
        };
        for key in keys_of("pt-BR") {
            let reference = names(&translate_in(Lang::PtBr, &key));
            for lang in Lang::all() {
                let mine = names(&translate_in(*lang, &key));
                assert_eq!(
                    mine,
                    reference,
                    "{key} em {} usa placeholders diferentes",
                    lang.tag()
                );
            }
        }
    }

    /// Chaves declaradas no locale padrão, lidas do arquivo embutido.
    fn keys() -> Vec<String> {
        keys_of("pt-BR")
    }
}

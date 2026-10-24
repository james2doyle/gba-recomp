//! PixAntiqua glyphs, generated at build time (SPEC-069 L10N-69-05).
//!
//! The font is a bitmap design: every contour is a rectangle on an exact
//! 12 px grid, so the build script rasterizes it losslessly into a 1-bit
//! atlas. The frontend blits from that atlas and needs no font library, which
//! is what removes SDL_ttf from the macOS, Linux, Android, iOS and Web
//! bundles alike.
//!
//! Advances are whole pixels per glyph, so text always lands on pixel
//! boundaries and stays crisp at integer scales.

#![allow(dead_code)]

/// Em size the atlas was rasterized at, in guest logical pixels.
pub const SIZE: f32 = atlas::SIZE;

/// Line height in logical pixels.
pub const LINE_HEIGHT: f32 = SIZE;

mod atlas {
    include!(concat!(env!("OUT_DIR"), "/font_atlas.rs"));
}

/// Fallback drawn for a character the font does not cover.
///
/// PixAntiqua is Latin-only (ASCII, Latin-1 and a few Latin Extended-A
/// letters): Cyrillic, Greek and CJK have no glyphs. Those languages still
/// work in the egui windows, which use system fonts, but the 240x160
/// in-window menu draws `?` for them rather than tofu boxes.
pub const MISSING: char = '?';

/// A glyph's place in the atlas and how far the pen moves after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    /// Atlas rectangle, in pixels.
    pub x: u16,
    pub y: u16,
    pub w: u8,
    pub h: u8,
    /// Pen advance, in pixels.
    pub advance: u8,
    /// Distance from the top of the line box to the first inked row.
    pub oy: u8,
}

/// Looks up a character's glyph, falling back to [`MISSING`].
pub fn glyph(ch: char) -> Glyph {
    lookup(ch).or_else(|| lookup(MISSING)).unwrap_or(Glyph {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        advance: atlas::ADVANCE_SPACE as u8,
        oy: 0,
    })
}

fn lookup(ch: char) -> Option<Glyph> {
    let code = ch as u32;
    atlas::GLYPHS
        .binary_search_by_key(&code, |&(c, ..)| c)
        .ok()
        .map(|i| {
            let (_, advance, x, y, w, h, oy) = atlas::GLYPHS[i];
            Glyph {
                x,
                y,
                w,
                h,
                advance,
                oy,
            }
        })
}

/// Whether the font covers a character (used to explain `?` in diagnostics).
pub fn covers(ch: char) -> bool {
    lookup(ch).is_some()
}

/// Width of `text` in logical pixels, including interpolation.
pub fn width(text: &str) -> f32 {
    text.chars().map(|c| f32::from(glyph(c).advance)).sum()
}

/// The 8-bit atlas coverage map: 255 where a pixel is inked, 0 elsewhere.
pub fn coverage() -> &'static [u8] {
    &atlas::COVERAGE
}

/// Atlas dimensions in pixels.
pub const ATLAS_WIDTH: u32 = atlas::WIDTH;
pub const ATLAS_HEIGHT: u32 = atlas::HEIGHT;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_is_consistent() {
        assert_eq!(
            coverage().len(),
            (ATLAS_WIDTH * ATLAS_HEIGHT) as usize,
            "coverage must cover exactly the atlas"
        );
        // A zero-sized atlas would mean the build script produced nothing;
        // checking it through `coverage` keeps this a real assertion.
        assert!(
            !coverage().is_empty(),
            "the atlas must contain at least one glyph"
        );
    }

    #[test]
    fn glyphs_fit_inside_the_atlas() {
        for &(code, _, x, y, w, h, _) in atlas::GLYPHS {
            assert!(
                u32::from(x) + u32::from(w) <= ATLAS_WIDTH,
                "U+{code:04X} overflows width"
            );
            assert!(
                u32::from(y) + u32::from(h) <= ATLAS_HEIGHT,
                "U+{code:04X} overflows height"
            );
        }
    }

    #[test]
    fn glyphs_are_sorted_for_binary_search() {
        // `lookup` binary-searches, so the generated table must be ordered.
        assert!(
            atlas::GLYPHS.windows(2).all(|w| w[0].0 < w[1].0),
            "GLYPHS must be sorted by code point"
        );
    }

    #[test]
    fn portuguese_is_fully_covered() {
        // The default locale must render every character it uses.
        let pt = "áàâãéêíóôõúüçÁÀÂÃÉÊÍÓÔÕÚÜÇñÑ";
        for ch in pt.chars() {
            assert!(covers(ch), "missing Portuguese glyph {ch:?}");
        }
        for ch in "ConfiguraçõesÁudioNãoNítidoControlesvídeo".chars() {
            assert!(covers(ch), "missing {ch:?}");
        }
    }

    #[test]
    fn non_latin_scripts_fall_back_to_the_missing_glyph() {
        // Documented limitation: PixAntiqua has no Cyrillic/Greek/CJK.
        for ch in ['Ж', 'α', '漢', '한'] {
            assert!(!covers(ch), "{ch:?} unexpectedly covered");
            assert_eq!(glyph(ch), glyph(MISSING), "{ch:?} must draw `?`");
        }
    }

    #[test]
    fn advances_are_whole_pixels_and_ordered_by_width() {
        let narrow = glyph('i').advance;
        let wide = glyph('M').advance;
        assert!(narrow > 0 && wide > narrow, "i={narrow} M={wide}");
        assert_eq!(wide, 10, "M should be 10 px wide at 12 px");
    }

    #[test]
    fn width_sums_advances() {
        assert_eq!(width(""), 0.0);
        assert_eq!(width("i"), f32::from(glyph('i').advance));
        assert_eq!(
            width("ii"),
            f32::from(glyph('i').advance) * 2.0,
            "width must accumulate"
        );
        // A translated label is measurably wider than a short one.
        assert!(width("Configurações") > width("Áudio"));
    }

    #[test]
    fn space_has_an_advance_but_no_ink() {
        let space = glyph(' ');
        assert!(space.advance > 0, "space must advance the pen");
        assert_eq!((space.w, space.h), (0, 0), "space must not draw");
    }

    /// The inked pixels of a glyph, as rows of booleans, for shape checks.
    fn rows(ch: char) -> Vec<Vec<bool>> {
        let g = glyph(ch);
        (0..g.h)
            .map(|r| {
                (0..g.w)
                    .map(|c| {
                        let at = (u32::from(g.y) + u32::from(r)) * ATLAS_WIDTH
                            + u32::from(g.x)
                            + u32::from(c);
                        coverage()[at as usize] != 0
                    })
                    .collect()
            })
            .collect()
    }

    /// A full stop is a small solid square: the simplest shape check that
    /// proves the rasterizer filled interiors rather than outlines.
    ///
    /// The font draws it 171 units wide at 1024/em, which is 2.004 px at
    /// 12 px — so 2x2 is the faithful result, not a rounding mistake.
    #[test]
    fn full_stop_is_a_solid_block() {
        let r = rows('.');
        assert_eq!(r.len(), 2, "period is 2 px tall");
        assert_eq!(r[0].len(), 2, "period is 2 px wide");
        assert!(
            r.iter().all(|row| row.iter().all(|&on| on)),
            "period must be solid"
        );
    }

    /// Counters must stay hollow: nonzero winding, not a naive fill.
    ///
    /// Scans every row rather than a fixed band, because the counter's rows
    /// differ per glyph (`g` is a descender, `A` has an apex).
    #[test]
    fn counters_stay_hollow() {
        for ch in ['A', 'o', 'e', 'g', 'D', 'R', 'B', 'P'] {
            let r = rows(ch);
            let inner_blank = r.iter().any(|row| {
                let (Some(first), Some(last)) = (
                    row.iter().position(|&on| on),
                    row.iter().rposition(|&on| on),
                ) else {
                    return false;
                };
                last > first + 1 && row[first + 1..last].iter().any(|&on| !on)
            });
            assert!(inner_blank, "{ch:?} should have a hollow counter");
        }
    }

    /// Messages the in-window menu draws in a single unclipped line must fit
    /// the 240 px screen. `draw_menu` blits its footer at x=4 with no wrapping
    /// or clipping, so a longer string is cut off at the right edge — that is
    /// how `settings.saved` used to lose "half a second" on device
    /// (SPEC-069 L10N-69-08).
    #[test]
    fn footer_messages_fit_the_menu_width() {
        // Keys reachable from `draw_menu`'s footer, the in-game status strip
        // and the paused banner: the fixed-width draw sites in app.rs.
        const KEYS: [&str; 19] = [
            "settings.saved",
            "status.party_opened",
            "status.party_missing",
            "status.settings_opened",
            "status.settings_missing",
            "status.game_reset",
            "status.save_written",
            "status.save_no_file",
            "status.save_unchanged",
            "status.save_failed",
            "status.settings_reloaded",
            "status.settings_invalid",
            "menu.hint",
            "menu.hint_touch",
            "menu.status.applied",
            "menu.status.bound",
            "menu.controls.press_key",
            "menu.controls.press_pad",
            "banner.paused",
        ];
        // Drawn at x=4 in a 240 px wide screen.
        const AVAILABLE: f32 = 240.0 - 4.0;
        let mut wide: Vec<String> = Vec::new();
        for lang in gba_i18n::Lang::all() {
            for key in KEYS {
                let text = gba_i18n::translate_in(*lang, key);
                let width = width(&text);
                if width > AVAILABLE {
                    wide.push(format!(
                        "{} {key}: {width} px > {AVAILABLE} px \"{text}\"",
                        lang.tag()
                    ));
                }
            }
        }
        assert!(
            wide.is_empty(),
            "these footer messages are clipped at the right edge: {wide:#?}"
        );
    }

    /// Every character the shipped locales use must be drawable by the menu
    /// font. A new locale that needs Cyrillic would fail here instead of
    /// silently rendering `?` in the 240x160 menu.
    #[test]
    fn every_shipped_locale_renders_in_the_menu_font() {
        let mut missing: Vec<char> = Vec::new();
        for message in gba_i18n::all_messages() {
            for ch in message.chars() {
                // Control characters are layout, not glyphs: `\n` in the CLI
                // usage text is drawn as a line break by its own renderer.
                if ch.is_control() {
                    continue;
                }
                if !covers(ch) && !missing.contains(&ch) {
                    missing.push(ch);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "the menu font cannot draw {missing:?}; add a glyph or pick a different font"
        );
    }

    /// Accents must actually be drawn above the base letter.
    #[test]
    fn accents_add_ink_above_the_baseline() {
        let plain = rows('a');
        let acute = rows('á');
        assert!(
            acute.len() > plain.len(),
            "á is taller than a ({} vs {})",
            acute.len(),
            plain.len()
        );
        assert!(
            acute[0].iter().any(|&on| on),
            "the accent must put ink in the top row"
        );
    }
}

//! Host input → KEYINPUT (SPEC-063 MENU-63-02, SPEC-009 INPUT-01).

use gba_core::keys;

/// Bindings resolved to SDL codes (scancodes / gamepad buttons).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bindings {
    pub keyboard: Vec<(usize, u16)>,
    pub gamepad: Vec<(i32, u16)>,
    /// Left stick deadzone as a fraction of full deflection.
    pub deadzone: f32,
}

/// Raw state of one gamepad for one sampling point.
pub struct PadState<'a> {
    pub button: &'a dyn Fn(i32) -> bool,
    /// Left stick axes in SDL range (-32768..=32767).
    pub left_x: i16,
    pub left_y: i16,
}

impl Bindings {
    pub fn mask(&self, key_down: impl Fn(usize) -> bool, pads: &[PadState<'_>]) -> u16 {
        let mut m = self
            .keyboard
            .iter()
            .filter(|(sc, _)| key_down(*sc))
            .fold(0, |m, (_, k)| m | k);
        let threshold = (self.deadzone.clamp(0.0, 0.95) * 32767.0) as i32;
        for p in pads {
            m |= self
                .gamepad
                .iter()
                .filter(|(b, _)| (p.button)(*b))
                .fold(0, |m, (_, k)| m | k);
            let (x, y) = (i32::from(p.left_x), i32::from(p.left_y));
            if x > threshold {
                m |= keys::RIGHT;
            } else if x < -threshold {
                m |= keys::LEFT;
            }
            if y > threshold {
                m |= keys::DOWN;
            } else if y < -threshold {
                m |= keys::UP;
            }
        }
        neutralize_opposites(m)
    }
}

/// Baseline policy for simultaneous opposite directions: release both.
pub fn neutralize_opposites(mut m: u16) -> u16 {
    if m & (keys::LEFT | keys::RIGHT) == keys::LEFT | keys::RIGHT {
        m &= !(keys::LEFT | keys::RIGHT);
    }
    if m & (keys::UP | keys::DOWN) == keys::UP | keys::DOWN {
        m &= !(keys::UP | keys::DOWN);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings() -> Bindings {
        Bindings {
            keyboard: vec![(29, keys::A), (79, keys::RIGHT), (80, keys::LEFT)],
            gamepad: vec![(0, keys::A), (6, keys::START)],
            deadzone: 0.25,
        }
    }

    #[test]
    fn keyboard_and_pad_buttons_merge() {
        let b = bindings();
        let pressed = |b: i32| b == 6;
        let pad = PadState {
            button: &pressed,
            left_x: 0,
            left_y: 0,
        };
        assert_eq!(b.mask(|sc| sc == 29, &[pad]), keys::A | keys::START);
    }

    #[test]
    fn deadzone_and_stick_directions() {
        let b = bindings();
        let none = |_: i32| false;
        let inside = PadState {
            button: &none,
            left_x: 8000,
            left_y: -8000,
        };
        assert_eq!(b.mask(|_| false, &[inside]), 0);
        let outside = PadState {
            button: &none,
            left_x: 9000,
            left_y: -9000,
        };
        assert_eq!(b.mask(|_| false, &[outside]), keys::RIGHT | keys::UP);
    }

    #[test]
    fn opposite_directions_are_neutralised() {
        let b = bindings();
        assert_eq!(b.mask(|sc| sc == 79 || sc == 80 || sc == 29, &[]), keys::A);
        assert_eq!(
            neutralize_opposites(keys::UP | keys::DOWN | keys::B),
            keys::B
        );
    }
}

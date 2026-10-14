//! Host input → KEYINPUT (SPEC-063 MENU-63-02, SPEC-009 INPUT-01).

use gba_core::keys;

/// One control of a generic (non-gamepad) joystick (DEV-64-02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoyControl {
    Button(i32),
    /// Hat index and SDL_HAT_* direction bit.
    Hat(i32, u8),
    /// Axis index and direction (true = positive).
    Axis(i32, bool),
}

impl JoyControl {
    /// `bN`, `hN<up|down|left|right>`, `aN+`, `aN-`.
    pub fn parse(text: &str) -> Option<Self> {
        let num = |s: &str| s.parse::<i32>().ok().filter(|n| (0..256).contains(n));
        if let Some(rest) = text.strip_prefix('b') {
            return num(rest).map(JoyControl::Button);
        }
        if let Some(rest) = text.strip_prefix('h') {
            for (name, bit) in [("up", 1u8), ("right", 2), ("down", 4), ("left", 8)] {
                if let Some(n) = rest.strip_suffix(name) {
                    return num(n).map(|n| JoyControl::Hat(n, bit));
                }
            }
            return None;
        }
        if let Some(rest) = text.strip_prefix('a') {
            if let Some(n) = rest.strip_suffix('+') {
                return num(n).map(|n| JoyControl::Axis(n, true));
            }
            if let Some(n) = rest.strip_suffix('-') {
                return num(n).map(|n| JoyControl::Axis(n, false));
            }
        }
        None
    }

    pub fn name(self) -> String {
        match self {
            JoyControl::Button(n) => format!("b{n}"),
            JoyControl::Hat(n, bit) => {
                let dir = match bit {
                    1 => "up",
                    2 => "right",
                    4 => "down",
                    _ => "left",
                };
                format!("h{n}{dir}")
            }
            JoyControl::Axis(n, pos) => format!("a{n}{}", if pos { '+' } else { '-' }),
        }
    }
}

/// Raw state of one generic joystick.
pub struct JoyState<'a> {
    pub button: &'a dyn Fn(i32) -> bool,
    pub axis: &'a dyn Fn(i32) -> i16,
    pub hat: &'a dyn Fn(i32) -> u8,
}

/// Bindings resolved to SDL codes (scancodes / gamepad buttons).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bindings {
    pub keyboard: Vec<(usize, u16)>,
    pub gamepad: Vec<(i32, u16)>,
    pub joystick: Vec<(JoyControl, u16)>,
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
        self.mask_all(key_down, pads, &[], 0)
    }

    /// Keyboard, gamepads, generic joysticks and an extra mask (GPIO).
    pub fn mask_all(
        &self,
        key_down: impl Fn(usize) -> bool,
        pads: &[PadState<'_>],
        joys: &[JoyState<'_>],
        extra: u16,
    ) -> u16 {
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
        for j in joys {
            for (control, k) in &self.joystick {
                let on = match *control {
                    JoyControl::Button(b) => (j.button)(b),
                    JoyControl::Hat(h, bit) => (j.hat)(h) & bit != 0,
                    JoyControl::Axis(a, pos) => {
                        let v = i32::from((j.axis)(a));
                        if pos { v > threshold } else { v < -threshold }
                    }
                };
                if on {
                    m |= k;
                }
            }
        }
        neutralize_opposites(m | extra)
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

    #[test]
    fn joystick_controls_parse_and_round_trip() {
        for text in ["b0", "b12", "h0up", "h1left", "a0+", "a3-"] {
            assert_eq!(JoyControl::parse(text).unwrap().name(), text);
        }
        for bad in ["", "b", "bx", "h0", "h0north", "a1", "a1*", "c3", "b300"] {
            assert!(JoyControl::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn generic_joystick_and_gpio_contribute() {
        let b = Bindings {
            joystick: vec![
                (JoyControl::Button(3), keys::A),
                (JoyControl::Hat(0, 1), keys::UP),
                (JoyControl::Axis(0, true), keys::RIGHT),
            ],
            deadzone: 0.25,
            ..Bindings::default()
        };
        let button = |n: i32| n == 3;
        let axis = |n: i32| if n == 0 { 20000 } else { 0 };
        let hat = |_: i32| 1u8;
        let joy = JoyState {
            button: &button,
            axis: &axis,
            hat: &hat,
        };
        assert_eq!(
            b.mask_all(|_| false, &[], &[joy], keys::START),
            keys::A | keys::UP | keys::RIGHT | keys::START
        );
    }

    fn bindings() -> Bindings {
        Bindings {
            keyboard: vec![(29, keys::A), (79, keys::RIGHT), (80, keys::LEFT)],
            gamepad: vec![(0, keys::A), (6, keys::START)],
            deadzone: 0.25,
            ..Bindings::default()
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

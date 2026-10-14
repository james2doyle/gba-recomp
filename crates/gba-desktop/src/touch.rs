//! On-screen gamepad for touch screens (SPEC-068 TOUCH-68-01). Pure layout
//! and hit testing in guest logical coordinates (240×160); the app feeds
//! finger positions and draws the overlay.

use gba_core::keys;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// Circle: centre and radius.
    Circle { x: f32, y: f32, r: f32 },
    /// Rectangle: origin and size.
    Rect { x: f32, y: f32, w: f32, h: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Button {
    pub label: &'static str,
    pub key: u16,
    pub shape: Shape,
}

/// Centre and radius of the 8-way D-pad.
pub const DPAD: (f32, f32, f32) = (34.0, 116.0, 30.0);

pub const BUTTONS: [Button; 6] = [
    Button {
        label: "A",
        key: keys::A,
        shape: Shape::Circle {
            x: 218.0,
            y: 108.0,
            r: 15.0,
        },
    },
    Button {
        label: "B",
        key: keys::B,
        shape: Shape::Circle {
            x: 190.0,
            y: 128.0,
            r: 15.0,
        },
    },
    Button {
        label: "L",
        key: keys::L,
        shape: Shape::Rect {
            x: 0.0,
            y: 0.0,
            w: 44.0,
            h: 18.0,
        },
    },
    Button {
        label: "R",
        key: keys::R,
        shape: Shape::Rect {
            x: 196.0,
            y: 0.0,
            w: 44.0,
            h: 18.0,
        },
    },
    Button {
        label: "SEL",
        key: keys::SELECT,
        shape: Shape::Rect {
            x: 88.0,
            y: 146.0,
            w: 30.0,
            h: 13.0,
        },
    },
    Button {
        label: "START",
        key: keys::START,
        shape: Shape::Rect {
            x: 122.0,
            y: 146.0,
            w: 30.0,
            h: 13.0,
        },
    },
];

fn hit(shape: Shape, px: f32, py: f32) -> bool {
    match shape {
        Shape::Circle { x, y, r } => {
            // A little slack makes buttons easier to hit than they look.
            let (dx, dy) = (px - x, py - y);
            dx * dx + dy * dy <= (r * 1.25) * (r * 1.25)
        }
        Shape::Rect { x, y, w, h } => px >= x && px < x + w && py >= y && py < y + h,
    }
}

/// D-pad directions for a touch at (px, py): 8-way with a small dead centre.
fn dpad(px: f32, py: f32) -> u16 {
    let (cx, cy, r) = DPAD;
    let (dx, dy) = (px - cx, py - cy);
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < r * 0.2 || dist > r * 1.4 {
        return 0;
    }
    // tan(22.5°) ≈ 0.414 splits each quadrant into straight and diagonal.
    let mut m = 0;
    if dx > dist * 0.38 {
        m |= keys::RIGHT;
    } else if dx < -dist * 0.38 {
        m |= keys::LEFT;
    }
    if dy > dist * 0.38 {
        m |= keys::DOWN;
    } else if dy < -dist * 0.38 {
        m |= keys::UP;
    }
    m
}

/// KEYINPUT bits pressed by the given touch points (logical coordinates).
pub fn mask(points: impl IntoIterator<Item = (f32, f32)>) -> u16 {
    let mut m = 0;
    for (x, y) in points {
        m |= dpad(x, y);
        for b in &BUTTONS {
            if hit(b.shape, x, y) {
                m |= b.key;
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_and_dpad_map_to_keys() {
        assert_eq!(mask([(218.0, 108.0)]), keys::A);
        assert_eq!(mask([(190.0, 128.0)]), keys::B);
        assert_eq!(mask([(10.0, 5.0)]), keys::L);
        assert_eq!(mask([(230.0, 5.0)]), keys::R);
        assert_eq!(mask([(130.0, 150.0)]), keys::START);
        assert_eq!(mask([(95.0, 150.0)]), keys::SELECT);
        let (cx, cy, _) = DPAD;
        assert_eq!(mask([(cx + 20.0, cy)]), keys::RIGHT);
        assert_eq!(mask([(cx, cy - 20.0)]), keys::UP);
        assert_eq!(mask([(cx - 15.0, cy + 15.0)]), keys::LEFT | keys::DOWN);
    }

    #[test]
    fn dead_centre_and_empty_screen_press_nothing() {
        let (cx, cy, _) = DPAD;
        assert_eq!(mask([(cx, cy)]), 0);
        assert_eq!(mask([(120.0, 80.0)]), 0);
        assert_eq!(mask(std::iter::empty()), 0);
    }

    #[test]
    fn multitouch_combines_buttons() {
        let (cx, cy, _) = DPAD;
        assert_eq!(
            mask([(cx + 20.0, cy), (218.0, 108.0)]),
            keys::RIGHT | keys::A
        );
    }
}

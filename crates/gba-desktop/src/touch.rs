//! On-screen gamepad for touch screens (SPEC-068 TOUCH-68-01). Pure layout
//! and hit testing in guest logical coordinates (240×160); the app feeds
//! finger positions and draws the overlay.

use gba_core::keys;

use crate::menu::Nav;

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

/// Whether `(px, py)` falls on the on-screen gamepad, using the same slop as
/// [`mask`]. A long press here is a held button, never a menu gesture
/// (SPEC-069 L10N-69-07).
pub fn on_control(px: f32, py: f32) -> bool {
    let (cx, cy, r) = DPAD;
    let (dx, dy) = (px - cx, py - cy);
    if dx * dx + dy * dy <= (r * 1.4) * (r * 1.4) {
        return true;
    }
    BUTTONS.iter().any(|b| hit(b.shape, px, py))
}

/// Hold time that opens the menu on a touch screen.
pub const LONG_PRESS: std::time::Duration = std::time::Duration::from_millis(500);

/// Movement that cancels a pending long press, in guest logical pixels.
pub const LONG_PRESS_SLOP: f32 = 12.0;

/// Long-press detection for the in-window menu (SPEC-069 L10N-69-06).
///
/// Pure model with injected time so it is testable without a clock or a
/// device. A press on the on-screen gamepad is ignored, so holding a
/// direction for half a second never opens the menu mid-game; a press that
/// drifts more than [`LONG_PRESS_SLOP`] is a drag, not a hold.
#[derive(Default)]
pub struct LongPress {
    /// Finger being timed, with where it started and when.
    active: Option<(u64, f32, f32, std::time::Instant)>,
    /// Set once the gesture fired, so it fires at most once per press.
    fired: bool,
}

impl LongPress {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the start of a press. Ignored when it lands on a control.
    pub fn press(&mut self, finger: u64, x: f32, y: f32, now: std::time::Instant) {
        if self.active.is_some() || on_control(x, y) {
            return;
        }
        self.active = Some((finger, x, y, now));
        self.fired = false;
    }

    /// Updates the position of the tracked finger, cancelling on drift.
    pub fn moved(&mut self, finger: u64, x: f32, y: f32) {
        if let Some((tracked, x0, y0, _)) = self.active
            && tracked == finger
            && (x - x0).hypot(y - y0) > LONG_PRESS_SLOP
        {
            self.active = None;
        }
    }

    /// Ends the press without firing.
    pub fn release(&mut self, finger: u64) {
        if self.active.is_some_and(|(tracked, ..)| tracked == finger) {
            self.active = None;
        }
    }

    /// Whether the tracked press has been held long enough. Fires once.
    pub fn poll(&mut self, now: std::time::Instant) -> bool {
        let Some((_, _, _, start)) = self.active else {
            return false;
        };
        if self.fired || now.saturating_duration_since(start) < LONG_PRESS {
            return false;
        }
        self.fired = true;
        true
    }

    /// Drops the gesture without firing (menu opened by another route, or the
    /// guest was suspended).
    pub fn cancel(&mut self) {
        self.active = None;
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }
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

/// Navigation a touch on the virtual gamepad means while the menu is open
/// (SPEC-068 TOUCH-68-02). `None` means the touch is not a menu control.
///
/// The D-pad is 8-way for gameplay but the menu only moves along the list, so
/// a diagonal prefers the vertical direction: rows are what a finger is
/// aiming at.
pub fn menu_key(x: f32, y: f32) -> Option<Nav> {
    let bit = |key: u16| BUTTONS.iter().any(|b| b.key == key && hit(b.shape, x, y));
    if bit(keys::A) {
        return Some(Nav::Confirm);
    }
    if bit(keys::B) {
        return Some(Nav::Back);
    }
    if bit(keys::START) {
        return Some(Nav::Last);
    }
    if bit(keys::SELECT) {
        return Some(Nav::First);
    }
    let d = dpad(x, y);
    if d & keys::UP != 0 {
        Some(Nav::Up)
    } else if d & keys::DOWN != 0 {
        Some(Nav::Down)
    } else if d & keys::LEFT != 0 {
        Some(Nav::Left)
    } else if d & keys::RIGHT != 0 {
        Some(Nav::Right)
    } else {
        None
    }
}

/// Edge detection for touch navigation: one [`Nav`] per press, never a repeat.
///
/// Holding a direction to scroll would be convenient on a 25-row page, but a
/// finger resting on a 30 px control while reading the list would then walk
/// straight through "Sair". A press is what emits, and the same control has
/// to be released and touched again to emit twice.
#[derive(Default)]
pub struct NavPad {
    /// Finger owning the current press, and the control it selected.
    held: Option<(u64, Nav)>,
}

impl NavPad {
    pub fn new() -> Self {
        Self::default()
    }

    /// A finger went down at (x, y). Returns the navigation it selects, if any.
    pub fn press(&mut self, finger: u64, x: f32, y: f32) -> Option<Nav> {
        self.held = None;
        let nav = menu_key(x, y)?;
        self.held = Some((finger, nav));
        Some(nav)
    }

    /// A finger went up; the control it held stops being active.
    pub fn release(&mut self, finger: u64) {
        if self.held.is_some_and(|(held, _)| held == finger) {
            self.held = None;
        }
    }

    /// Drops any active press (the menu closed under the finger).
    pub fn cancel(&mut self) {
        self.held = None;
    }

    /// KEYINPUT bits of the held control, so the pad can highlight it.
    pub fn pressed(&self) -> u16 {
        self.held.map_or(0, |(_, nav)| nav_key(nav))
    }
}

/// The gamepad control that a navigation step stands for (for highlighting).
pub fn nav_key(nav: Nav) -> u16 {
    match nav {
        Nav::Up => keys::UP,
        Nav::Down => keys::DOWN,
        Nav::Left => keys::LEFT,
        Nav::Right => keys::RIGHT,
        Nav::Confirm => keys::A,
        Nav::Back => keys::B,
        Nav::First => keys::SELECT,
        Nav::Last => keys::START,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::{Duration, Instant};

    /// Centre of open screen: no control drawn there.
    const OPEN: (f32, f32) = (120.0, 40.0);

    #[test]
    fn long_press_fires_only_after_the_threshold() {
        let t0 = Instant::now();
        let mut lp = LongPress::new();
        lp.press(1, OPEN.0, OPEN.1, t0);
        assert!(!lp.poll(t0 + Duration::from_millis(499)), "too early");
        assert!(lp.poll(t0 + Duration::from_millis(500)), "fires at 500ms");
    }

    #[test]
    fn long_press_fires_at_most_once() {
        let t0 = Instant::now();
        let mut lp = LongPress::new();
        lp.press(1, OPEN.0, OPEN.1, t0);
        let t = t0 + Duration::from_millis(600);
        assert!(lp.poll(t));
        assert!(!lp.poll(t), "must not fire again for the same press");
        assert!(!lp.poll(t + Duration::from_secs(1)));
    }

    #[test]
    fn long_press_is_cancelled_by_release() {
        let t0 = Instant::now();
        let mut lp = LongPress::new();
        lp.press(1, OPEN.0, OPEN.1, t0);
        lp.release(1);
        assert!(!lp.poll(t0 + Duration::from_secs(1)));
        assert!(!lp.is_active());
    }

    #[test]
    fn long_press_is_cancelled_by_dragging() {
        let t0 = Instant::now();
        let mut lp = LongPress::new();
        lp.press(1, OPEN.0, OPEN.1, t0);
        lp.moved(1, OPEN.0 + LONG_PRESS_SLOP + 1.0, OPEN.1);
        assert!(
            !lp.poll(t0 + Duration::from_secs(1)),
            "a drag is not a hold"
        );
    }

    #[test]
    fn small_jitter_still_fires() {
        let t0 = Instant::now();
        let mut lp = LongPress::new();
        lp.press(1, OPEN.0, OPEN.1, t0);
        lp.moved(1, OPEN.0 + 3.0, OPEN.1 - 2.0);
        assert!(lp.poll(t0 + Duration::from_millis(500)));
    }

    #[test]
    fn pressing_a_control_never_opens_the_menu() {
        let t0 = Instant::now();
        // Holding the D-pad (or any button) is play input, not a gesture.
        for (x, y) in [
            (DPAD.0, DPAD.1),
            (DPAD.0 + 20.0, DPAD.1),
            (218.0, 108.0), // A
            (95.0, 150.0),  // Select
        ] {
            let mut lp = LongPress::new();
            lp.press(1, x, y, t0);
            assert!(!lp.is_active(), "press at ({x}, {y}) must be ignored");
            assert!(!lp.poll(t0 + Duration::from_secs(2)));
        }
    }

    #[test]
    fn on_control_matches_the_touchable_areas() {
        assert!(on_control(DPAD.0, DPAD.1));
        assert!(on_control(218.0, 108.0));
        assert!(!on_control(OPEN.0, OPEN.1));
        assert!(!on_control(120.0, 80.0), "middle of the screen is free");
    }

    #[test]
    fn a_second_press_is_ignored_while_one_is_tracked() {
        let t0 = Instant::now();
        let mut lp = LongPress::new();
        lp.press(1, OPEN.0, OPEN.1, t0);
        lp.press(2, OPEN.0 + 30.0, OPEN.1, t0 + Duration::from_millis(100));
        // The first press still owns the gesture.
        assert!(lp.poll(t0 + Duration::from_millis(500)));
    }

    #[test]
    fn cancel_drops_the_gesture_without_firing() {
        let t0 = Instant::now();
        let mut lp = LongPress::new();
        lp.press(1, OPEN.0, OPEN.1, t0);
        lp.cancel();
        assert!(!lp.poll(t0 + Duration::from_secs(1)));
    }

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

    #[test]
    fn the_pad_navigates_the_menu() {
        let (cx, cy, _) = DPAD;
        assert_eq!(menu_key(cx, cy - 20.0), Some(Nav::Up));
        assert_eq!(menu_key(cx, cy + 20.0), Some(Nav::Down));
        assert_eq!(menu_key(cx - 20.0, cy), Some(Nav::Left));
        assert_eq!(menu_key(cx + 20.0, cy), Some(Nav::Right));
        assert_eq!(menu_key(218.0, 108.0), Some(Nav::Confirm));
        assert_eq!(menu_key(190.0, 128.0), Some(Nav::Back));
        assert_eq!(menu_key(95.0, 150.0), Some(Nav::First));
        assert_eq!(menu_key(130.0, 150.0), Some(Nav::Last));
        // Shoulders and free screen are not navigation.
        assert_eq!(menu_key(10.0, 5.0), None);
        assert_eq!(menu_key(230.0, 5.0), None);
        assert_eq!(menu_key(120.0, 80.0), None);
        assert_eq!(menu_key(cx, cy), None, "dead centre");
    }

    /// A diagonal on the 8-way D-pad moves the selection, not a slider: the
    /// menu only moves vertically along rows.
    #[test]
    fn a_diagonal_press_still_moves_the_cursor() {
        let (cx, cy, _) = DPAD;
        assert_eq!(menu_key(cx - 15.0, cy + 15.0), Some(Nav::Down));
        assert_eq!(menu_key(cx + 15.0, cy - 15.0), Some(Nav::Up));
    }

    /// One step per touch: holding a direction must not walk the list.
    #[test]
    fn holding_a_control_navigates_once() {
        let (cx, cy, _) = DPAD;
        let mut pad = NavPad::new();
        assert_eq!(pad.press(1, cx, cy + 20.0), Some(Nav::Down));
        assert_eq!(pad.pressed(), keys::DOWN, "drawn as pressed");
        // Still down, still no further navigation: `press` is the only source.
        assert_eq!(pad.pressed(), keys::DOWN);
    }

    #[test]
    fn releasing_allows_the_next_step() {
        let (cx, cy, _) = DPAD;
        let mut pad = NavPad::new();
        pad.press(1, cx, cy + 20.0);
        pad.release(1);
        assert_eq!(pad.pressed(), 0);
        assert_eq!(pad.press(2, cx, cy + 20.0), Some(Nav::Down));
    }

    #[test]
    fn a_press_on_free_screen_is_not_navigation() {
        let mut pad = NavPad::new();
        assert_eq!(pad.press(1, OPEN.0, OPEN.1), None);
        assert_eq!(pad.pressed(), 0);
        // A later real press still works.
        assert_eq!(pad.press(1, 218.0, 108.0), Some(Nav::Confirm));
    }

    #[test]
    fn cancel_drops_the_held_control() {
        let mut pad = NavPad::new();
        pad.press(1, 218.0, 108.0);
        pad.cancel();
        assert_eq!(pad.pressed(), 0);
    }

    #[test]
    fn a_second_finger_does_not_steal_the_press() {
        let (cx, cy, _) = DPAD;
        let mut pad = NavPad::new();
        pad.press(1, cx, cy + 20.0);
        assert_eq!(pad.press(2, 218.0, 108.0), Some(Nav::Confirm));
        // Releasing the first finger leaves the second one's control lit.
        pad.release(1);
        assert_eq!(pad.pressed(), keys::A);
    }
}

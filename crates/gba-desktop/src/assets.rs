//! Branding assets embedded in every frontend binary (SPEC-063 FR-63-03).

/// Splash shown when the game window opens (`assets/logo-alpha.png`).
pub const SPLASH_PNG: &[u8] = include_bytes!("../../../assets/logo-alpha.png");

/// Window / Dock icon of the game and settings windows (`assets/logo-ico.png`).
pub const ICON_PNG: &[u8] = include_bytes!("../../../assets/logo-ico.png");

/// Splash duration requested for the game window.
pub const SPLASH_DURATION: std::time::Duration = std::time::Duration::from_millis(2400);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_logos_decode_as_rgba() {
        for png in [SPLASH_PNG, ICON_PNG] {
            let s = crate::sdl::Surface::from_png(png).expect("PNG decodes");
            let (w, h, rgba) = s.to_rgba().expect("RGBA conversion");
            assert_eq!((w, h), (1254, 1254));
            assert_eq!(rgba.len(), 1254 * 1254 * 4);
        }
        assert_eq!(SPLASH_DURATION.as_millis(), 2400);
    }
}

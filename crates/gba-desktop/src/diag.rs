//! Runtime diagnostics that survive on Android.
//!
//! The frontend reports recoverable problems on `stderr` (a bad settings
//! line, an audio backend that would not open, a missing splash image). That
//! is fine on a desktop, where it lands in the terminal, but an Android
//! package has no console: `stderr` goes nowhere and a failure on a device is
//! invisible unless the user sees a symptom. KNO-009 listed that as a
//! limitation; [`warn`] closes it by also writing to logcat under the
//! `gba-recomp` tag, which is where SDL's own messages appear.
//!
//! The text keeps its `gba-desktop: `-style prefix so the lines read the same
//! in a terminal and in `adb logcat`.

/// Reports a recoverable problem: `stderr` everywhere, plus logcat on Android.
///
/// Takes an already formatted message, so callers keep using `format!`
/// arguments exactly as they did with `eprintln!`.
pub fn warn(message: &str) {
    eprintln!("{message}");
    #[cfg(target_os = "android")]
    crate::sdl::log_to_system(message);
}

/// [`warn`] with a `gba-desktop: ` prefix.
///
/// # Examples
///
/// ```
/// // Does not panic and keeps the prefix the frontend has always used.
/// gba_desktop::diag::warn_prefixed("audio disabled");
/// ```
pub fn warn_prefixed(message: &str) {
    warn(&format!("gba-desktop: {message}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both entry points stay callable off Android, where they are plain
    /// `stderr` writes. This pins the signature so call sites keep compiling
    /// when the Android branch is not part of the build.
    #[test]
    fn warn_is_callable_everywhere() {
        warn("gba-desktop: test line");
        warn_prefixed("test line");
    }
}

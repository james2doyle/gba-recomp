//! Frame pacing sleeps (SPEC-061 pacing, SPEC-068 Web).
//!
//! On desktop targets this is `thread::sleep`. In the browser
//! (`wasm32-unknown-emscripten`) the frame loop is a blocking loop that
//! must hand control back to the page every frame so the canvas is
//! presented and input/audio events are delivered: `emscripten_sleep`
//! unwinds to the event loop and resumes later (requires `-sASYNCIFY` at
//! the final `emcc` link).

use std::time::Duration;

#[cfg(not(target_os = "emscripten"))]
pub fn sleep(d: Duration) {
    std::thread::sleep(d);
}

/// Give the host a chance to run without waiting (no-op on desktop).
#[cfg(not(target_os = "emscripten"))]
pub fn yield_now() {}

#[cfg(target_os = "emscripten")]
#[allow(unsafe_code)]
mod web {
    unsafe extern "C" {
        fn emscripten_sleep(ms: u32);
    }

    pub fn sleep_ms(ms: u32) {
        // SAFETY: plain Emscripten library call; with ASYNCIFY it suspends
        // the wasm stack, returns to the browser and resumes after `ms`.
        unsafe { emscripten_sleep(ms) }
    }
}

#[cfg(target_os = "emscripten")]
pub fn sleep(d: Duration) {
    web::sleep_ms(u32::try_from(d.as_millis()).unwrap_or(u32::MAX));
}

#[cfg(target_os = "emscripten")]
pub fn yield_now() {
    web::sleep_ms(0);
}

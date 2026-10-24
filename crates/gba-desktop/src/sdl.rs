//! Minimal hand-written SDL3 bindings (SPEC-061 FE-61-01).
//!
//! All `unsafe` in the frontend lives in this module. The declarations
//! mirror the SDL 3.4 headers (`SDL_init.h`, `SDL_video.h`, `SDL_render.h`,
//! `SDL_audio.h`, `SDL_events.h`, `SDL_gamepad.h`, `SDL_keyboard.h`); only
//! the functions and constants the frontend uses are declared. Every handle
//! is wrapped in an owning type whose `Drop` releases it, and all owners
//! must be dropped before [`Context`] (which calls `SDL_Quit`).

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

#[repr(C)]
struct SdlWindow {
    _p: [u8; 0],
}
#[repr(C)]
struct SdlRenderer {
    _p: [u8; 0],
}
#[repr(C)]
struct SdlTexture {
    _p: [u8; 0],
}
#[repr(C)]
struct SdlAudioStream {
    _p: [u8; 0],
}
#[repr(C)]
struct SdlGamepad {
    _p: [u8; 0],
}
#[repr(C)]
struct SdlJoystick {
    _p: [u8; 0],
}
#[repr(C)]
struct SdlIoStream {
    _p: [u8; 0],
}
/// `struct SDL_Surface` (public, read-only fields).
#[repr(C)]
struct SdlSurface {
    flags: u32,
    format: u32,
    w: c_int,
    h: c_int,
    pitch: c_int,
    pixels: *mut c_void,
    refcount: c_int,
    reserved: *mut c_void,
}

const SDL_PIXELFORMAT_RGBA32: u32 = 0x1676_2004; // ABGR8888 on little-endian
const SDL_LOGICAL_PRESENTATION_DISABLED: c_int = 0;

#[repr(C)]
struct SdlAudioSpec {
    format: c_int,
    channels: c_int,
    freq: c_int,
}

/// `SDL_Event` is a 128-byte union; we read only the common prefix.
#[repr(C, align(8))]
struct SdlEvent {
    bytes: [u8; 128],
}

const SDL_INIT_AUDIO: u32 = 0x10;
const SDL_INIT_VIDEO: u32 = 0x20;
const SDL_INIT_GAMEPAD: u32 = 0x2000;
const SDL_INIT_JOYSTICK: u32 = 0x200;
const SDL_EVENT_JOYSTICK_AXIS_MOTION: u32 = 0x600;
const SDL_EVENT_JOYSTICK_HAT_MOTION: u32 = 0x602;
const SDL_EVENT_JOYSTICK_BUTTON_DOWN: u32 = 0x603;
const SDL_EVENT_JOYSTICK_ADDED: u32 = 0x605;
const SDL_EVENT_JOYSTICK_REMOVED: u32 = 0x606;
const SDL_INIT_EVENTS: u32 = 0x4000;
const SDL_WINDOW_RESIZABLE: u64 = 0x20;
const SDL_LOGICAL_PRESENTATION_INTEGER_SCALE: c_int = 4;
const SDL_SCALEMODE_NEAREST: c_int = 0;
const SDL_TEXTUREACCESS_STREAMING: c_int = 1;
const SDL_PIXELFORMAT_XRGB8888: u32 = 0x1616_1804;
const SDL_AUDIO_S16LE: c_int = 0x8010;
const SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK: u32 = 0xFFFF_FFFF;
const SDL_EVENT_QUIT: u32 = 0x100;
const SDL_EVENT_WINDOW_FOCUS_LOST: u32 = 0x20F;
const SDL_EVENT_KEY_DOWN: u32 = 0x300;
const SDL_EVENT_MOUSE_BUTTON_DOWN: u32 = 0x401;
const SDL_EVENT_FINGER_DOWN: u32 = 0x700;
const SDL_EVENT_FINGER_UP: u32 = 0x701;
const SDL_EVENT_FINGER_MOTION: u32 = 0x702;
const SDL_EVENT_FINGER_CANCELED: u32 = 0x703;
const SDL_TOUCH_DEVICE_DIRECT: c_int = 1;
const SDL_EVENT_GAMEPAD_BUTTON_DOWN: u32 = 0x651;
const SDL_LOGICAL_PRESENTATION_LETTERBOX: c_int = 2;
const SDL_SCALEMODE_LINEAR: c_int = 1;
const SDL_BLENDMODE_BLEND: u32 = 1;
const SDL_EVENT_GAMEPAD_ADDED: u32 = 0x653;
const SDL_EVENT_GAMEPAD_REMOVED: u32 = 0x654;

unsafe extern "C" {
    fn SDL_Init(flags: u32) -> bool;
    fn SDL_InitSubSystem(flags: u32) -> bool;
    fn SDL_Quit();
    fn SDL_GetError() -> *const c_char;

    fn SDL_CreateWindow(title: *const c_char, w: c_int, h: c_int, flags: u64) -> *mut SdlWindow;
    fn SDL_DestroyWindow(window: *mut SdlWindow);
    fn SDL_CreateRenderer(window: *mut SdlWindow, name: *const c_char) -> *mut SdlRenderer;
    fn SDL_DestroyRenderer(renderer: *mut SdlRenderer);
    fn SDL_SetRenderLogicalPresentation(
        renderer: *mut SdlRenderer,
        w: c_int,
        h: c_int,
        mode: c_int,
    ) -> bool;
    fn SDL_SetRenderDrawColor(renderer: *mut SdlRenderer, r: u8, g: u8, b: u8, a: u8) -> bool;
    fn SDL_RenderClear(renderer: *mut SdlRenderer) -> bool;
    fn SDL_RenderTexture(
        renderer: *mut SdlRenderer,
        texture: *mut SdlTexture,
        src: *const c_void,
        dst: *const c_void,
    ) -> bool;
    fn SDL_RenderPresent(renderer: *mut SdlRenderer) -> bool;
    fn SDL_CreateTexture(
        renderer: *mut SdlRenderer,
        format: u32,
        access: c_int,
        w: c_int,
        h: c_int,
    ) -> *mut SdlTexture;
    fn SDL_DestroyTexture(texture: *mut SdlTexture);
    fn SDL_SetTextureScaleMode(texture: *mut SdlTexture, mode: c_int) -> bool;
    fn SDL_UpdateTexture(
        texture: *mut SdlTexture,
        rect: *const c_void,
        pixels: *const c_void,
        pitch: c_int,
    ) -> bool;

    fn SDL_OpenAudioDeviceStream(
        devid: u32,
        spec: *const SdlAudioSpec,
        callback: *const c_void,
        userdata: *mut c_void,
    ) -> *mut SdlAudioStream;
    fn SDL_ResumeAudioStreamDevice(stream: *mut SdlAudioStream) -> bool;
    fn SDL_PutAudioStreamData(stream: *mut SdlAudioStream, buf: *const c_void, len: c_int) -> bool;
    fn SDL_GetAudioStreamQueued(stream: *mut SdlAudioStream) -> c_int;
    fn SDL_DestroyAudioStream(stream: *mut SdlAudioStream);

    fn SDL_PollEvent(event: *mut SdlEvent) -> bool;
    fn SDL_GetKeyboardState(numkeys: *mut c_int) -> *const bool;

    fn SDL_OpenGamepad(id: u32) -> *mut SdlGamepad;
    fn SDL_CloseGamepad(gamepad: *mut SdlGamepad);
    fn SDL_GetGamepadID(gamepad: *mut SdlGamepad) -> u32;
    fn SDL_GetGamepadButton(gamepad: *mut SdlGamepad, button: c_int) -> bool;
    fn SDL_GetGamepadAxis(gamepad: *mut SdlGamepad, axis: c_int) -> i16;
    fn SDL_GetGamepadName(gamepad: *mut SdlGamepad) -> *const c_char;
    fn SDL_GetGamepadConnectionState(gamepad: *mut SdlGamepad) -> c_int;
    fn SDL_IsGamepad(id: u32) -> bool;
    fn SDL_OpenJoystick(id: u32) -> *mut SdlJoystick;
    fn SDL_CloseJoystick(joystick: *mut SdlJoystick);
    fn SDL_GetJoystickID(joystick: *mut SdlJoystick) -> u32;
    fn SDL_GetJoystickName(joystick: *mut SdlJoystick) -> *const c_char;
    fn SDL_GetJoystickConnectionState(joystick: *mut SdlJoystick) -> c_int;
    fn SDL_GetJoystickButton(joystick: *mut SdlJoystick, button: c_int) -> bool;
    fn SDL_GetJoystickAxis(joystick: *mut SdlJoystick, axis: c_int) -> i16;
    fn SDL_GetJoystickHat(joystick: *mut SdlJoystick, hat: c_int) -> u8;
    fn SDL_GetNumJoystickButtons(joystick: *mut SdlJoystick) -> c_int;
    fn SDL_GetNumJoystickAxes(joystick: *mut SdlJoystick) -> c_int;
    fn SDL_GetNumJoystickHats(joystick: *mut SdlJoystick) -> c_int;
    fn SDL_PumpEvents();
    fn SDL_GetTouchDevices(count: *mut c_int) -> *mut u64;
    fn SDL_GetTouchDeviceType(id: u64) -> c_int;
    fn SDL_free(mem: *mut c_void);
    fn SDL_GetRenderLogicalPresentationRect(renderer: *mut SdlRenderer, rect: *mut FRect) -> bool;
    fn SDL_SetHint(name: *const c_char, value: *const c_char) -> bool;
    fn SDL_IOFromConstMem(mem: *const c_void, size: usize) -> *mut SdlIoStream;
    fn SDL_LoadPNG_IO(src: *mut SdlIoStream, closeio: bool) -> *mut SdlSurface;
    fn SDL_ConvertSurface(surface: *mut SdlSurface, format: u32) -> *mut SdlSurface;
    fn SDL_DestroySurface(surface: *mut SdlSurface);
    fn SDL_SetWindowIcon(window: *mut SdlWindow, icon: *mut SdlSurface) -> bool;
    fn SDL_CreateTextureFromSurface(
        renderer: *mut SdlRenderer,
        surface: *mut SdlSurface,
    ) -> *mut SdlTexture;
    fn SDL_SetTextureAlphaModFloat(texture: *mut SdlTexture, alpha: f32) -> bool;
    fn SDL_GetRenderOutputSize(renderer: *mut SdlRenderer, w: *mut c_int, h: *mut c_int) -> bool;
    fn SDL_GetGamepadStringForButton(button: c_int) -> *const c_char;
    fn SDL_GetGamepadButtonFromString(name: *const c_char) -> c_int;
    fn SDL_GetScancodeName(scancode: c_int) -> *const c_char;
    fn SDL_GetScancodeFromName(name: *const c_char) -> c_int;

    fn SDL_SetWindowFullscreen(window: *mut SdlWindow, fullscreen: bool) -> bool;
    fn SDL_SetWindowSize(window: *mut SdlWindow, w: c_int, h: c_int) -> bool;
    fn SDL_SetWindowTitle(window: *mut SdlWindow, title: *const c_char) -> bool;
    fn SDL_SetRenderVSync(renderer: *mut SdlRenderer, vsync: c_int) -> bool;
    fn SDL_SetRenderDrawBlendMode(renderer: *mut SdlRenderer, mode: u32) -> bool;
    fn SDL_RenderFillRect(renderer: *mut SdlRenderer, rect: *const FRect) -> bool;
    fn SDL_RenderDebugText(renderer: *mut SdlRenderer, x: f32, y: f32, text: *const c_char)
    -> bool;

    fn SDL_SetAudioStreamGain(stream: *mut SdlAudioStream, gain: f32) -> bool;
    fn SDL_ClearAudioStream(stream: *mut SdlAudioStream) -> bool;
    fn SDL_PauseAudioStreamDevice(stream: *mut SdlAudioStream) -> bool;
}

#[repr(C)]
pub struct FRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

fn c_str_to_string(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: SDL returns NUL-terminated strings it owns.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// A decoded image owned by SDL (PNG assets: splash and icon).
pub struct Surface {
    ptr: NonNull<SdlSurface>,
}

impl Surface {
    /// Decode an in-memory PNG (SDL 3.4 `SDL_LoadPNG_IO`).
    pub fn from_png(bytes: &'static [u8]) -> Result<Self, String> {
        // SAFETY: `bytes` is static and outlives the stream; SDL closes the
        // stream (closeio = true) and returns an owned surface or null.
        let raw = unsafe {
            let io = SDL_IOFromConstMem(bytes.as_ptr().cast(), bytes.len());
            if io.is_null() {
                return Err(last_error("SDL_IOFromConstMem"));
            }
            SDL_LoadPNG_IO(io, true)
        };
        NonNull::new(raw)
            .map(|ptr| Self { ptr })
            .ok_or_else(|| last_error("SDL_LoadPNG_IO"))
    }

    /// (width, height, RGBA8 pixels) for toolkits that take raw icons.
    pub fn to_rgba(&self) -> Result<(u32, u32, Vec<u8>), String> {
        // SAFETY: converts our live surface into a new owned RGBA surface,
        // copies its rows (pitch may exceed 4*w) and destroys it.
        unsafe {
            let conv = SDL_ConvertSurface(self.ptr.as_ptr(), SDL_PIXELFORMAT_RGBA32);
            if conv.is_null() {
                return Err(last_error("SDL_ConvertSurface"));
            }
            let s = &*conv;
            let (w, h, pitch) = (s.w as usize, s.h as usize, s.pitch as usize);
            let mut out = Vec::with_capacity(w * h * 4);
            for y in 0..h {
                let row = std::slice::from_raw_parts((s.pixels as *const u8).add(y * pitch), w * 4);
                out.extend_from_slice(row);
            }
            SDL_DestroySurface(conv);
            Ok((w as u32, h as u32, out))
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: destroying the owned surface.
        unsafe { SDL_DestroySurface(self.ptr.as_ptr()) };
    }
}

/// Set an SDL hint before initialisation.
pub fn set_hint(name: &str, value: &str) {
    if let (Ok(n), Ok(v)) = (CString::new(name), CString::new(value)) {
        // SAFETY: valid C strings for the duration of the call.
        unsafe { SDL_SetHint(n.as_ptr(), v.as_ptr()) };
    }
}

#[cfg(target_os = "android")]
unsafe extern "C" {
    fn SDL_GetPrefPath(org: *const c_char, app: *const c_char) -> *mut c_char;
}

/// Per-app writable directory (SPEC-068): on Android `SDL_GetPrefPath`
/// returns the app's internal storage (`/data/data/<package>/files/`).
/// SDL documents it as callable before `SDL_Init`.
#[cfg(target_os = "android")]
pub fn pref_path() -> Option<std::path::PathBuf> {
    // SAFETY: valid C strings; the returned buffer is owned by us and freed
    // with SDL_free after copying.
    unsafe {
        let p = SDL_GetPrefPath(c"gba-recomp".as_ptr(), c"gba-recomp".as_ptr());
        if p.is_null() {
            return None;
        }
        let path = CStr::from_ptr(p).to_string_lossy().into_owned();
        SDL_free(p.cast());
        Some(std::path::PathBuf::from(path))
    }
}

/// Android has no console: `stderr` from the runtime goes nowhere, so a
/// package that fails on device is silently broken (KNO-009). Forward a
/// diagnostic to logcat under the `gba-recomp` tag, which is where
/// `SDL_Log` itself writes.
///
/// `ANDROID_LOG_INFO` is 4 and the priority is a C `int`; the message is
/// passed as `%s` so a `%` inside a diagnostic can never be read as a format
/// directive. `__android_log_print` truncates long messages to just under
/// 4 KiB per call, which is far more than the frontend emits.
#[cfg(target_os = "android")]
pub fn log_to_system(message: &str) {
    const ANDROID_LOG_INFO: c_int = 4;
    unsafe extern "C" {
        fn __android_log_print(prio: c_int, tag: *const c_char, fmt: *const c_char, ...) -> c_int;
    }
    // SAFETY: static C strings plus one NUL-terminated copy of `message`.
    if let Ok(text) = CString::new(message) {
        unsafe {
            __android_log_print(
                ANDROID_LOG_INFO,
                c"gba-recomp".as_ptr(),
                c"%s".as_ptr(),
                text.as_ptr(),
            );
        }
    }
}

/// SDL name of a scancode (e.g. "Z", "Return"); empty if unknown.
pub fn scancode_name(scancode: usize) -> String {
    // SAFETY: plain FFI call; unknown scancodes yield "".
    c_str_to_string(unsafe { SDL_GetScancodeName(scancode as c_int) })
}

pub fn scancode_from_name(name: &str) -> Option<usize> {
    let c = CString::new(name).ok()?;
    // SAFETY: valid C string for the duration of the call.
    let sc = unsafe { SDL_GetScancodeFromName(c.as_ptr()) };
    (sc > 0).then_some(sc as usize)
}

pub fn pad_button_name(button: i32) -> String {
    // SAFETY: plain FFI call; invalid buttons yield null.
    c_str_to_string(unsafe { SDL_GetGamepadStringForButton(button) })
}

pub fn pad_button_from_name(name: &str) -> Option<i32> {
    let c = CString::new(name).ok()?;
    // SAFETY: valid C string for the duration of the call.
    let b = unsafe { SDL_GetGamepadButtonFromString(c.as_ptr()) };
    (b >= 0).then_some(b)
}

fn last_error(what: &str) -> String {
    // SAFETY: SDL_GetError always returns a valid NUL-terminated string
    // owned by SDL (possibly empty).
    let msg = unsafe { CStr::from_ptr(SDL_GetError()) };
    format!("{what}: {}", msg.to_string_lossy())
}

/// Scancodes used by the frontend (`SDL_scancode.h`).
pub mod scancode {
    pub const A: usize = 4;
    pub const F1: usize = 58;
    pub const F11: usize = 68;
    pub const SPACE: usize = 44;
    pub const S: usize = 22;
    pub const X: usize = 27;
    pub const Z: usize = 29;
    pub const RETURN: usize = 40;
    pub const ESCAPE: usize = 41;
    pub const BACKSPACE: usize = 42;
    pub const TAB: usize = 43;
    pub const RIGHT: usize = 79;
    pub const LEFT: usize = 80;
    pub const DOWN: usize = 81;
    pub const UP: usize = 82;
    pub const RSHIFT: usize = 229;
}

/// Gamepad buttons (`SDL_GamepadButton`).
pub mod button {
    pub const SOUTH: i32 = 0;
    pub const EAST: i32 = 1;
    pub const BACK: i32 = 4;
    pub const GUIDE: i32 = 5;
    pub const START: i32 = 6;
    pub const LEFT_SHOULDER: i32 = 9;
    pub const RIGHT_SHOULDER: i32 = 10;
    pub const DPAD_UP: i32 = 11;
    pub const DPAD_DOWN: i32 = 12;
    pub const DPAD_LEFT: i32 = 13;
    pub const DPAD_RIGHT: i32 = 14;
}

/// SDL library lifetime. Not `Send`: SDL video must stay on the main thread.
pub struct Context {
    _not_send: PhantomData<*const ()>,
}

pub enum Event {
    Quit,
    FocusLost,
    KeyDown {
        scancode: usize,
        repeat: bool,
    },
    MouseDown {
        button: u8,
    },
    /// Finger down/move at normalized window coordinates (0..1).
    FingerDown {
        finger: u64,
        x: f32,
        y: f32,
    },
    FingerUp {
        finger: u64,
    },
    PadDown {
        button: i32,
    },
    GamepadAdded(u32),
    GamepadRemoved(u32),
    JoystickAdded(u32),
    JoystickRemoved(u32),
    JoyButtonDown {
        which: u32,
        button: i32,
    },
    JoyHat {
        which: u32,
        hat: i32,
        value: u8,
    },
    JoyAxis {
        which: u32,
        axis: i32,
        value: i16,
    },
    Other,
}

impl Context {
    /// Initialises video, events and gamepads (required).
    pub fn init() -> Result<Self, String> {
        // SAFETY: plain FFI call with constant flags.
        if unsafe {
            SDL_Init(SDL_INIT_VIDEO | SDL_INIT_EVENTS | SDL_INIT_GAMEPAD | SDL_INIT_JOYSTICK)
        } {
            Ok(Self {
                _not_send: PhantomData,
            })
        } else {
            Err(last_error("SDL_Init"))
        }
    }

    /// Input-only initialisation for the settings app (no window).
    pub fn init_input() -> Result<Self, String> {
        // SAFETY: plain FFI call with constant flags.
        if unsafe { SDL_Init(SDL_INIT_EVENTS | SDL_INIT_GAMEPAD | SDL_INIT_JOYSTICK) } {
            Ok(Self {
                _not_send: PhantomData,
            })
        } else {
            Err(last_error("SDL_Init"))
        }
    }

    /// Whether a direct touch screen is present (not a trackpad).
    pub fn has_touchscreen(&self) -> bool {
        let mut n: c_int = 0;
        // SAFETY: SDL returns an owned array of `n` ids (freed below) or null.
        unsafe {
            let ids = SDL_GetTouchDevices(&mut n);
            if ids.is_null() {
                return false;
            }
            let found = (0..n.max(0) as usize)
                .any(|i| SDL_GetTouchDeviceType(*ids.add(i)) == SDL_TOUCH_DEVICE_DIRECT);
            SDL_free(ids.cast());
            found
        }
    }

    pub fn pump(&self) {
        // SAFETY: plain FFI call on the thread that initialised SDL.
        unsafe { SDL_PumpEvents() };
    }

    /// Whether the joystick `id` has a standard gamepad mapping.
    pub fn is_gamepad(&self, id: u32) -> bool {
        // SAFETY: plain FFI call; unknown ids return false.
        unsafe { SDL_IsGamepad(id) }
    }

    pub fn open_joystick(&self, id: u32) -> Result<Joystick<'_>, String> {
        // SAFETY: plain FFI call; an invalid id yields null.
        let raw = unsafe { SDL_OpenJoystick(id) };
        let joy = NonNull::new(raw).ok_or_else(|| last_error("SDL_OpenJoystick"))?;
        Ok(Joystick {
            joy,
            _ctx: PhantomData,
        })
    }

    /// Opens the default playback device as a 32768 Hz stereo S16 stream.
    /// Failure is reported but never fatal for the caller (AUD-03).
    pub fn open_audio(&self, freq: i32) -> Result<Audio<'_>, String> {
        // SAFETY: plain FFI call with constant flags.
        if !unsafe { SDL_InitSubSystem(SDL_INIT_AUDIO) } {
            return Err(last_error("SDL_InitSubSystem(AUDIO)"));
        }
        let spec = SdlAudioSpec {
            format: SDL_AUDIO_S16LE,
            channels: 2,
            freq,
        };
        // SAFETY: `spec` outlives the call; a null callback selects the
        // push (put-data) model.
        let raw = unsafe {
            SDL_OpenAudioDeviceStream(
                SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK,
                &spec,
                ptr::null(),
                ptr::null_mut(),
            )
        };
        let stream = NonNull::new(raw).ok_or_else(|| last_error("SDL_OpenAudioDeviceStream"))?;
        // SAFETY: `stream` is a live stream created above.
        if !unsafe { SDL_ResumeAudioStreamDevice(stream.as_ptr()) } {
            // SAFETY: destroying the stream we own.
            unsafe { SDL_DestroyAudioStream(stream.as_ptr()) };
            return Err(last_error("SDL_ResumeAudioStreamDevice"));
        }
        Ok(Audio {
            stream,
            _ctx: PhantomData,
        })
    }

    pub fn create_video(
        &self,
        title: &str,
        w: i32,
        h: i32,
        scale: i32,
    ) -> Result<Video<'_>, String> {
        Video::new(title, w, h, scale)
    }

    pub fn poll_event(&self) -> Option<Event> {
        let mut ev = SdlEvent { bytes: [0; 128] };
        // SAFETY: `ev` is a properly sized and aligned SDL_Event buffer.
        if !unsafe { SDL_PollEvent(&mut ev) } {
            return None;
        }
        let word = |at: usize| u32::from_ne_bytes(ev.bytes[at..at + 4].try_into().unwrap());
        // SDL_GamepadDeviceEvent: type u32, reserved u32, timestamp u64, which u32.
        Some(match word(0) {
            SDL_EVENT_QUIT => Event::Quit,
            SDL_EVENT_WINDOW_FOCUS_LOST => Event::FocusLost,
            // SDL_KeyboardEvent: scancode at 24, repeat at 37.
            SDL_EVENT_KEY_DOWN => Event::KeyDown {
                scancode: word(24) as usize,
                repeat: ev.bytes[37] != 0,
            },
            // SDL_MouseButtonEvent: button at 24.
            SDL_EVENT_MOUSE_BUTTON_DOWN => Event::MouseDown {
                button: ev.bytes[24],
            },
            // SDL_TouchFingerEvent: fingerID at 24, x/y floats at 32/36.
            SDL_EVENT_FINGER_DOWN | SDL_EVENT_FINGER_MOTION => {
                let f = |at: usize| f32::from_ne_bytes(ev.bytes[at..at + 4].try_into().unwrap());
                Event::FingerDown {
                    finger: u64::from_ne_bytes(ev.bytes[24..32].try_into().unwrap()),
                    x: f(32),
                    y: f(36),
                }
            }
            SDL_EVENT_FINGER_UP | SDL_EVENT_FINGER_CANCELED => Event::FingerUp {
                finger: u64::from_ne_bytes(ev.bytes[24..32].try_into().unwrap()),
            },
            // SDL_GamepadButtonEvent: button at 20.
            SDL_EVENT_GAMEPAD_BUTTON_DOWN => Event::PadDown {
                button: i32::from(ev.bytes[20]),
            },
            SDL_EVENT_GAMEPAD_ADDED => Event::GamepadAdded(word(16)),
            SDL_EVENT_JOYSTICK_ADDED => Event::JoystickAdded(word(16)),
            SDL_EVENT_JOYSTICK_REMOVED => Event::JoystickRemoved(word(16)),
            // SDL_JoyButtonEvent / SDL_JoyHatEvent: index at 20, value at 21.
            SDL_EVENT_JOYSTICK_BUTTON_DOWN => Event::JoyButtonDown {
                which: word(16),
                button: i32::from(ev.bytes[20]),
            },
            SDL_EVENT_JOYSTICK_HAT_MOTION => Event::JoyHat {
                which: word(16),
                hat: i32::from(ev.bytes[20]),
                value: ev.bytes[21],
            },
            // SDL_JoyAxisEvent: axis at 20, value (i16) at 24.
            SDL_EVENT_JOYSTICK_AXIS_MOTION => Event::JoyAxis {
                which: word(16),
                axis: i32::from(ev.bytes[20]),
                value: i16::from_ne_bytes([ev.bytes[24], ev.bytes[25]]),
            },
            SDL_EVENT_GAMEPAD_REMOVED => Event::GamepadRemoved(word(16)),
            _ => Event::Other,
        })
    }

    /// Current keyboard state for one scancode (updated by event polling).
    pub fn key_down(&self, scancode: usize) -> bool {
        let mut n: c_int = 0;
        // SAFETY: SDL returns a pointer to an internal array of `n` bools
        // valid for the application lifetime; we read one element in range
        // without forming a long-lived reference.
        unsafe {
            let state = SDL_GetKeyboardState(&mut n);
            !state.is_null() && scancode < n as usize && state.add(scancode).read()
        }
    }

    pub fn open_gamepad(&self, id: u32) -> Result<Gamepad<'_>, String> {
        // SAFETY: plain FFI call; an invalid id yields null.
        let raw = unsafe { SDL_OpenGamepad(id) };
        let pad = NonNull::new(raw).ok_or_else(|| last_error("SDL_OpenGamepad"))?;
        Ok(Gamepad {
            pad,
            _ctx: PhantomData,
        })
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: every owner borrows the context, so all handles are gone.
        unsafe { SDL_Quit() };
    }
}

pub struct Video<'a> {
    window: NonNull<SdlWindow>,
    renderer: NonNull<SdlRenderer>,
    texture: NonNull<SdlTexture>,
    width: i32,
    height: i32,
    _ctx: PhantomData<&'a Context>,
}

impl Video<'_> {
    fn new(title: &str, w: i32, h: i32, scale: i32) -> Result<Self, String> {
        let title = CString::new(title).map_err(|_| "window title contains NUL".to_owned())?;
        // SAFETY: `title` is a valid C string for the duration of the call.
        let raw =
            unsafe { SDL_CreateWindow(title.as_ptr(), w * scale, h * scale, SDL_WINDOW_RESIZABLE) };
        let window = NonNull::new(raw).ok_or_else(|| last_error("SDL_CreateWindow"))?;
        // SAFETY: `window` is live; null name selects the default driver.
        let raw = unsafe { SDL_CreateRenderer(window.as_ptr(), ptr::null()) };
        let Some(renderer) = NonNull::new(raw) else {
            let e = last_error("SDL_CreateRenderer");
            // SAFETY: destroying the window we own.
            unsafe { SDL_DestroyWindow(window.as_ptr()) };
            return Err(e);
        };
        // SAFETY: `renderer` is live.
        let raw = unsafe {
            SDL_CreateTexture(
                renderer.as_ptr(),
                SDL_PIXELFORMAT_XRGB8888,
                SDL_TEXTUREACCESS_STREAMING,
                w,
                h,
            )
        };
        let Some(texture) = NonNull::new(raw) else {
            let e = last_error("SDL_CreateTexture");
            // SAFETY: destroying handles we own, child first.
            unsafe {
                SDL_DestroyRenderer(renderer.as_ptr());
                SDL_DestroyWindow(window.as_ptr());
            }
            return Err(e);
        };
        let video = Self {
            window,
            renderer,
            texture,
            width: w,
            height: h,
            _ctx: PhantomData,
        };
        // SAFETY: handles are live (owned by `video`).
        unsafe {
            if !SDL_SetTextureScaleMode(texture.as_ptr(), SDL_SCALEMODE_NEAREST) {
                return Err(last_error("SDL_SetTextureScaleMode"));
            }
            // Integer scaling, letterboxed to the guest 3:2 aspect.
            if !SDL_SetRenderLogicalPresentation(
                renderer.as_ptr(),
                w,
                h,
                SDL_LOGICAL_PRESENTATION_INTEGER_SCALE,
            ) {
                return Err(last_error("SDL_SetRenderLogicalPresentation"));
            }
        }
        Ok(video)
    }

    pub fn set_fullscreen(&mut self, on: bool) {
        // SAFETY: window is live.
        unsafe { SDL_SetWindowFullscreen(self.window.as_ptr(), on) };
    }

    pub fn set_scale(&mut self, scale: i32) {
        // SAFETY: window is live.
        unsafe {
            SDL_SetWindowSize(
                self.window.as_ptr(),
                self.width * scale,
                self.height * scale,
            )
        };
    }

    /// Map normalized window coordinates to guest logical coordinates
    /// (240×160) through the current letterbox; `None` outside the game.
    pub fn window_to_logical(&self, nx: f32, ny: f32) -> Option<(f32, f32)> {
        let mut rect = FRect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
        let (mut ow, mut oh) = (0, 0);
        // SAFETY: renderer is live; out-parameters are valid.
        unsafe {
            SDL_GetRenderOutputSize(self.renderer.as_ptr(), &mut ow, &mut oh);
            SDL_GetRenderLogicalPresentationRect(self.renderer.as_ptr(), &mut rect);
        }
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return None;
        }
        let (px, py) = (nx * ow as f32, ny * oh as f32);
        let lx = (px - rect.x) * self.width as f32 / rect.w;
        let ly = (py - rect.y) * self.height as f32 / rect.h;
        ((0.0..self.width as f32).contains(&lx) && (0.0..self.height as f32).contains(&ly))
            .then_some((lx, ly))
    }

    pub fn set_icon(&mut self, icon: &Surface) {
        // SAFETY: window and surface are live; SDL copies the pixels.
        unsafe { SDL_SetWindowIcon(self.window.as_ptr(), icon.ptr.as_ptr()) };
    }

    /// Show `image` centred over a dark background for `duration`, with
    /// fade in/out, at the output resolution. `pump` handles events and
    /// returns false to abort (window closed). Returns whether to continue.
    pub fn splash(
        &mut self,
        image: &Surface,
        duration: std::time::Duration,
        mut pump: impl FnMut() -> bool,
    ) -> bool {
        // SAFETY: renderer/window are live; the texture is created from our
        // surface and destroyed before returning; FRects live for each call.
        unsafe {
            let tex = SDL_CreateTextureFromSurface(self.renderer.as_ptr(), image.ptr.as_ptr());
            if tex.is_null() {
                return true;
            }
            SDL_SetTextureScaleMode(tex, SDL_SCALEMODE_LINEAR);
            SDL_SetRenderLogicalPresentation(
                self.renderer.as_ptr(),
                0,
                0,
                SDL_LOGICAL_PRESENTATION_DISABLED,
            );
            let (iw, ih) = (
                (*image.ptr.as_ptr()).w as f32,
                (*image.ptr.as_ptr()).h as f32,
            );
            let start = std::time::Instant::now();
            let fade = 0.35f32;
            let total = duration.as_secs_f32();
            let mut keep = true;
            while start.elapsed() < duration {
                if !pump() {
                    keep = false;
                    break;
                }
                let t = start.elapsed().as_secs_f32();
                let alpha = (t / fade).min((total - t) / fade).clamp(0.0, 1.0);
                let (mut ow, mut oh) = (0, 0);
                SDL_GetRenderOutputSize(self.renderer.as_ptr(), &mut ow, &mut oh);
                let side = (ow.min(oh) as f32) * 0.86;
                let scale = (side / iw).min(side / ih);
                let (dw, dh) = (iw * scale, ih * scale);
                let dst = FRect {
                    x: (ow as f32 - dw) / 2.0,
                    y: (oh as f32 - dh) / 2.0,
                    w: dw,
                    h: dh,
                };
                SDL_SetRenderDrawColor(self.renderer.as_ptr(), 13, 8, 32, 255);
                SDL_RenderClear(self.renderer.as_ptr());
                SDL_SetTextureAlphaModFloat(tex, alpha);
                SDL_RenderTexture(
                    self.renderer.as_ptr(),
                    tex,
                    ptr::null(),
                    (&dst as *const FRect).cast(),
                );
                SDL_RenderPresent(self.renderer.as_ptr());
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
            SDL_DestroyTexture(tex);
            SDL_SetRenderLogicalPresentation(
                self.renderer.as_ptr(),
                self.width,
                self.height,
                SDL_LOGICAL_PRESENTATION_INTEGER_SCALE,
            );
            keep
        }
    }

    pub fn set_title(&mut self, title: &str) {
        if let Ok(c) = CString::new(title) {
            // SAFETY: window is live; C string valid for the call.
            unsafe { SDL_SetWindowTitle(self.window.as_ptr(), c.as_ptr()) };
        }
    }

    pub fn set_presentation(&mut self, linear: bool, integer: bool, vsync: bool) {
        // SAFETY: handles are live.
        unsafe {
            SDL_SetTextureScaleMode(
                self.texture.as_ptr(),
                if linear {
                    SDL_SCALEMODE_LINEAR
                } else {
                    SDL_SCALEMODE_NEAREST
                },
            );
            SDL_SetRenderLogicalPresentation(
                self.renderer.as_ptr(),
                self.width,
                self.height,
                if integer {
                    SDL_LOGICAL_PRESENTATION_INTEGER_SCALE
                } else {
                    SDL_LOGICAL_PRESENTATION_LETTERBOX
                },
            );
            SDL_SetRenderVSync(self.renderer.as_ptr(), c_int::from(vsync));
        }
    }

    /// Uploads a full XRGB8888 frame, lets `overlay` draw on top (in guest
    /// logical coordinates) and presents.
    pub fn present_with(
        &mut self,
        pixels: &[u32],
        overlay: impl FnOnce(&mut Painter<'_>),
    ) -> Result<(), String> {
        self.present_inner(pixels, overlay)
    }

    /// Uploads a full XRGB8888 frame and presents it.
    pub fn present(&mut self, pixels: &[u32]) -> Result<(), String> {
        self.present_inner(pixels, |_| {})
    }

    fn present_inner(
        &mut self,
        pixels: &[u32],
        overlay: impl FnOnce(&mut Painter<'_>),
    ) -> Result<(), String> {
        let pitch = self.width * 4;
        assert_eq!(
            pixels.len(),
            (self.width * self.height) as usize,
            "frame size mismatch"
        );
        // SAFETY: handles are live; `pixels` holds exactly w*h u32 values and is only read during the call.
        unsafe {
            if !SDL_UpdateTexture(
                self.texture.as_ptr(),
                ptr::null(),
                pixels.as_ptr().cast(),
                pitch,
            ) {
                return Err(last_error("SDL_UpdateTexture"));
            }
            SDL_SetRenderDrawColor(self.renderer.as_ptr(), 0, 0, 0, 255);
            SDL_RenderClear(self.renderer.as_ptr());
            if !SDL_RenderTexture(
                self.renderer.as_ptr(),
                self.texture.as_ptr(),
                ptr::null(),
                ptr::null(),
            ) {
                return Err(last_error("SDL_RenderTexture"));
            }
        }
        let mut painter = Painter {
            renderer: self.renderer,
            _video: PhantomData,
        };
        overlay(&mut painter);
        // SAFETY: renderer is live.
        unsafe {
            SDL_RenderPresent(self.renderer.as_ptr());
        }
        Ok(())
    }
}

/// Draws overlays in guest logical coordinates (240x160).
pub struct Painter<'v> {
    renderer: NonNull<SdlRenderer>,
    _video: PhantomData<&'v mut ()>,
}

impl Painter<'_> {
    pub fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, rgba: [u8; 4]) {
        let rect = FRect { x, y, w, h };
        // SAFETY: renderer is live; `rect` outlives the call.
        unsafe {
            SDL_SetRenderDrawBlendMode(self.renderer.as_ptr(), SDL_BLENDMODE_BLEND);
            SDL_SetRenderDrawColor(self.renderer.as_ptr(), rgba[0], rgba[1], rgba[2], rgba[3]);
            SDL_RenderFillRect(self.renderer.as_ptr(), &rect);
        }
    }

    /// 8x8 SDL debug font; ASCII only.
    pub fn text(&mut self, x: f32, y: f32, text: &str, rgb: [u8; 3]) {
        let ascii: String = text
            .chars()
            .map(|c| if c.is_ascii() { c } else { '?' })
            .collect();
        if let Ok(c) = CString::new(ascii) {
            // SAFETY: renderer is live; C string valid for the call.
            unsafe {
                SDL_SetRenderDrawColor(self.renderer.as_ptr(), rgb[0], rgb[1], rgb[2], 255);
                SDL_RenderDebugText(self.renderer.as_ptr(), x, y, c.as_ptr());
            }
        }
    }
}

impl Drop for Video<'_> {
    fn drop(&mut self) {
        // SAFETY: destroying owned handles, children first.
        unsafe {
            SDL_DestroyTexture(self.texture.as_ptr());
            SDL_DestroyRenderer(self.renderer.as_ptr());
            SDL_DestroyWindow(self.window.as_ptr());
        }
    }
}

pub struct Audio<'a> {
    stream: NonNull<SdlAudioStream>,
    _ctx: PhantomData<&'a Context>,
}

impl Audio<'_> {
    /// Bytes queued and not yet consumed by the device.
    pub fn queued_bytes(&self) -> usize {
        // SAFETY: `stream` is live.
        let n = unsafe { SDL_GetAudioStreamQueued(self.stream.as_ptr()) };
        n.max(0) as usize
    }

    pub fn set_gain(&mut self, gain: f32) {
        // SAFETY: `stream` is live.
        unsafe { SDL_SetAudioStreamGain(self.stream.as_ptr(), gain) };
    }

    /// Pause playback and drop queued audio (menu open, UI-06).
    pub fn pause(&mut self) {
        // SAFETY: `stream` is live.
        unsafe {
            SDL_PauseAudioStreamDevice(self.stream.as_ptr());
            SDL_ClearAudioStream(self.stream.as_ptr());
        }
    }

    pub fn resume(&mut self) {
        // SAFETY: `stream` is live.
        unsafe { SDL_ResumeAudioStreamDevice(self.stream.as_ptr()) };
    }

    pub fn put(&mut self, samples: &[i16]) -> Result<(), String> {
        if samples.is_empty() {
            return Ok(());
        }
        let len = c_int::try_from(std::mem::size_of_val(samples))
            .map_err(|_| "audio chunk too large".to_owned())?;
        // SAFETY: `stream` is live; SDL copies `len` bytes from `samples`.
        if unsafe { SDL_PutAudioStreamData(self.stream.as_ptr(), samples.as_ptr().cast(), len) } {
            Ok(())
        } else {
            Err(last_error("SDL_PutAudioStreamData"))
        }
    }
}

impl Drop for Audio<'_> {
    fn drop(&mut self) {
        // SAFETY: destroying the owned stream (also closes its device).
        unsafe { SDL_DestroyAudioStream(self.stream.as_ptr()) };
    }
}

pub struct Gamepad<'a> {
    pad: NonNull<SdlGamepad>,
    _ctx: PhantomData<&'a Context>,
}

impl Gamepad<'_> {
    pub fn id(&self) -> u32 {
        // SAFETY: `pad` is live.
        unsafe { SDL_GetGamepadID(self.pad.as_ptr()) }
    }

    pub fn name(&self) -> String {
        // SAFETY: `pad` is live.
        c_str_to_string(unsafe { SDL_GetGamepadName(self.pad.as_ptr()) })
    }

    pub fn connection(&self) -> Connection {
        // SAFETY: `pad` is live.
        Connection::from_raw(unsafe { SDL_GetGamepadConnectionState(self.pad.as_ptr()) })
    }

    /// Left stick (x, y) in SDL range.
    pub fn left_stick(&self) -> (i16, i16) {
        // SAFETY: `pad` is live; axes 0/1 are LEFTX/LEFTY.
        unsafe {
            (
                SDL_GetGamepadAxis(self.pad.as_ptr(), 0),
                SDL_GetGamepadAxis(self.pad.as_ptr(), 1),
            )
        }
    }

    pub fn button(&self, button: i32) -> bool {
        // SAFETY: `pad` is live; unknown buttons return false.
        unsafe { SDL_GetGamepadButton(self.pad.as_ptr(), button) }
    }
}

impl Drop for Gamepad<'_> {
    fn drop(&mut self) {
        // SAFETY: closing the owned gamepad.
        unsafe { SDL_CloseGamepad(self.pad.as_ptr()) };
    }
}

/// Physical connection reported by SDL (DEV-64-01).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    Unknown,
    Wired,
    Wireless,
}

impl Connection {
    fn from_raw(v: c_int) -> Self {
        match v {
            1 => Connection::Wired,
            2 => Connection::Wireless,
            _ => Connection::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Connection::Unknown => "desconhecida",
            Connection::Wired => "com fio (USB)",
            Connection::Wireless => "sem fio (Bluetooth)",
        }
    }
}

/// A joystick without a standard gamepad mapping (DEV-64-02).
pub struct Joystick<'a> {
    joy: NonNull<SdlJoystick>,
    _ctx: PhantomData<&'a Context>,
}

impl Joystick<'_> {
    pub fn id(&self) -> u32 {
        // SAFETY: `joy` is live.
        unsafe { SDL_GetJoystickID(self.joy.as_ptr()) }
    }
    pub fn name(&self) -> String {
        // SAFETY: `joy` is live.
        c_str_to_string(unsafe { SDL_GetJoystickName(self.joy.as_ptr()) })
    }
    pub fn connection(&self) -> Connection {
        // SAFETY: `joy` is live.
        Connection::from_raw(unsafe { SDL_GetJoystickConnectionState(self.joy.as_ptr()) })
    }
    pub fn button(&self, b: i32) -> bool {
        // SAFETY: `joy` is live; out-of-range indices return false.
        unsafe { SDL_GetJoystickButton(self.joy.as_ptr(), b) }
    }
    pub fn axis(&self, a: i32) -> i16 {
        // SAFETY: `joy` is live; out-of-range indices return 0.
        unsafe { SDL_GetJoystickAxis(self.joy.as_ptr(), a) }
    }
    pub fn hat(&self, h: i32) -> u8 {
        // SAFETY: `joy` is live; out-of-range indices return 0.
        unsafe { SDL_GetJoystickHat(self.joy.as_ptr(), h) }
    }
    /// (buttons, axes, hats).
    pub fn counts(&self) -> (i32, i32, i32) {
        // SAFETY: `joy` is live.
        unsafe {
            (
                SDL_GetNumJoystickButtons(self.joy.as_ptr()),
                SDL_GetNumJoystickAxes(self.joy.as_ptr()),
                SDL_GetNumJoystickHats(self.joy.as_ptr()),
            )
        }
    }
}

impl Drop for Joystick<'_> {
    fn drop(&mut self) {
        // SAFETY: closing the owned joystick.
        unsafe { SDL_CloseJoystick(self.joy.as_ptr()) };
    }
}

//! Desktop frontend library (SPEC-061 FE-61-01, SPEC-063): SDL3 window,
//! audio and input, settings, in-window menu and native context menu.
//! Used by the interpreted `gba-desktop` binary and by recompiled packages
//! through `gba-native-rt`. Host preferences never change the simulation.

#![deny(unsafe_code)]

pub mod app;
pub mod assets;
pub mod ctxmenu;
pub mod gpio;
pub mod input;
pub mod menu;
pub mod pace;
pub mod sdl;
pub mod settings;
pub mod touch;

pub use app::{AppOptions, AppReport, run_app};

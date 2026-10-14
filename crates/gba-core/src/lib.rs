//! `gba-core`: interpreted GBA core (ADR-041). CPU (SPEC-056), bus and
//! timing (SPEC-057), PPU (SPEC-058), APU (SPEC-059), HLE BIOS and backup
//! (SPEC-060). It has no external dependencies and no host APIs.

#![forbid(unsafe_code)]

pub mod apu;
pub mod backup;
pub mod bios;
pub mod bus;
pub mod cpu;
pub mod gba;
pub mod ppu;

pub use backup::BackupKind;
pub use bus::SystemFault;
pub use gba::{Gba, GbaConfig, keys};
pub use ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};

/// Version of the interpreted core contract (recorded in manifests, IAC-09).
pub const CORE_VERSION: u32 = 1;

#![forbid(unsafe_code)]

use std::mem::{align_of, offset_of, size_of};

pub const ABI_HEADER_V1: &str = include_str!("../include/gba_abi_v1.h");

pub const GBA_ABI_VERSION: u32 = 1;

pub const EXIT_CONTINUE: u32 = 0;
pub const EXIT_EVENT: u32 = 1;
pub const EXIT_HALT: u32 = 2;
pub const EXIT_MISSING_CODE: u32 = 3;
pub const EXIT_FAULT: u32 = 4;
pub const EXIT_INVALIDATED: u32 = 5;
pub const EXIT_STOP: u32 = 6;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GbaRuntimeV1 {
    pub r: [u32; 16],
    pub cpsr: u32,
    pub abi_version: u32,
    pub guest_cycle: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GbaExitV1 {
    pub reason: u32,
    pub next_pc: u32,
}

const _: () = assert!(size_of::<GbaRuntimeV1>() == 80);
const _: () = assert!(align_of::<GbaRuntimeV1>() == 8);
const _: () = assert!(offset_of!(GbaRuntimeV1, r) == 0);
const _: () = assert!(offset_of!(GbaRuntimeV1, cpsr) == 64);
const _: () = assert!(offset_of!(GbaRuntimeV1, abi_version) == 68);
const _: () = assert!(offset_of!(GbaRuntimeV1, guest_cycle) == 72);
const _: () = assert!(size_of::<GbaExitV1>() == 8);
const _: () = assert!(align_of::<GbaExitV1>() == 4);
const _: () = assert!(offset_of!(GbaExitV1, reason) == 0);
const _: () = assert!(offset_of!(GbaExitV1, next_pc) == 4);

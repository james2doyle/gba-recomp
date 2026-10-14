#![forbid(unsafe_code)]

use std::mem::{align_of, offset_of, size_of};

pub const ABI_HEADER_V1: &str = include_str!("../include/gba_abi_v1.h");
pub const ABI_HEADER_V2: &str = include_str!("../include/gba_abi_v2.h");
pub const ABI_RUNTIME_V2_SOURCE: &str = include_str!("../src/runtime_v2.c");
pub const ABI_HEADER_V3: &str = include_str!("../include/gba_abi_v3.h");
pub const ABI_RUNTIME_V3_SOURCE: &str = include_str!("../src/runtime_v3.c");
pub const ABI_HEADER_V4: &str = include_str!("../include/gba_abi_v4.h");
pub const ABI_RUNTIME_HLE_V1_SOURCE: &str = include_str!("../src/runtime_hle_v1.c");

pub const GBA_ABI_VERSION: u32 = 1;

pub const EXIT_CONTINUE: u32 = 0;
pub const EXIT_EVENT: u32 = 1;
pub const EXIT_HALT: u32 = 2;
pub const EXIT_MISSING_CODE: u32 = 3;
pub const EXIT_FAULT: u32 = 4;
pub const EXIT_INVALIDATED: u32 = 5;
pub const EXIT_STOP: u32 = 6;

pub const GBA_MEMORY_FAULT_ROM_SPECIALIZATION_V3: u32 = 7;

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

pub const GBA_ABI_VERSION_V2: u32 = 2;
pub const GBA_ABI_VERSION_V3: u32 = 3;
pub const GBA_ABI_VERSION_V4: u32 = 4;
pub const GBA_MACHINE_STATE_VERSION_V1: u32 = 1;

pub const EWRAM_SIZE: usize = 256 * 1024;
pub const IWRAM_SIZE: usize = 32 * 1024;
pub const PALETTE_RAM_SIZE: usize = 0x400;
pub const VRAM_SIZE: usize = 0x18000;
pub const OAM_SIZE: usize = 0x400;
pub const IO_LATCH_SIZE: usize = 0x400;
pub const WAVE_RAM_SIZE: usize = 32;

#[repr(C)]
pub struct GbaMemoryV1 {
    pub ewram: [u8; EWRAM_SIZE],
    pub iwram: [u8; IWRAM_SIZE],
}

#[repr(C)]
pub struct GbaRuntimeV3 {
    pub cpu: GbaRuntimeV2,
    pub memory: *mut GbaMemoryV1,
    pub abi_version: u32,
}

#[repr(C)]
pub struct GbaMachineStateV1 {
    pub memory: GbaMemoryV1,
    pub abi_version: u32,
    pub palette_ram: [u8; PALETTE_RAM_SIZE],
    pub vram: [u8; VRAM_SIZE],
    pub oam: [u8; OAM_SIZE],
    pub io: [u8; IO_LATCH_SIZE],
    pub wave_ram: [u8; WAVE_RAM_SIZE],
}

#[repr(C)]
pub struct GbaRuntimeV4 {
    pub base: GbaRuntimeV3,
    pub machine: *mut GbaMachineStateV1,
    pub abi_version: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GbaExitV3 {
    pub reason: u32,
    pub next_pc: u32,
    pub fault_address: u32,
    pub fault_code: u32,
}

/// Register banks in ABI v2. User and System share the same R13/R14 bank.
pub const BANK_USER_SYSTEM: usize = 0;
pub const BANK_FIQ: usize = 1;
pub const BANK_IRQ: usize = 2;
pub const BANK_SUPERVISOR: usize = 3;
pub const BANK_ABORT: usize = 4;
pub const BANK_UNDEFINED: usize = 5;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GbaRuntimeV2 {
    pub r: [u32; 16],
    pub cpsr: u32,
    pub abi_version: u32,
    pub guest_cycle: u64,
    pub common_r8_r12: [u32; 5],
    pub fiq_r8_r12: [u32; 5],
    pub banked_r13_r14: [[u32; 2]; 6],
    /// FIQ, IRQ, Supervisor, Abort, Undefined; User/System have no SPSR.
    pub spsr: [u32; 5],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GbaExitV2 {
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
const _: () = assert!(size_of::<GbaRuntimeV2>() == 192);
const _: () = assert!(align_of::<GbaRuntimeV2>() == 8);
const _: () = assert!(offset_of!(GbaRuntimeV2, r) == 0);
const _: () = assert!(offset_of!(GbaRuntimeV2, cpsr) == 64);
const _: () = assert!(offset_of!(GbaRuntimeV2, abi_version) == 68);
const _: () = assert!(offset_of!(GbaRuntimeV2, guest_cycle) == 72);
const _: () = assert!(offset_of!(GbaRuntimeV2, common_r8_r12) == 80);
const _: () = assert!(offset_of!(GbaRuntimeV2, fiq_r8_r12) == 100);
const _: () = assert!(offset_of!(GbaRuntimeV2, banked_r13_r14) == 120);
const _: () = assert!(offset_of!(GbaRuntimeV2, spsr) == 168);
const _: () = assert!(size_of::<GbaExitV2>() == 8);
const _: () = assert!(align_of::<GbaExitV2>() == 4);
const _: () = assert!(offset_of!(GbaExitV2, reason) == 0);
const _: () = assert!(offset_of!(GbaExitV2, next_pc) == 4);
const _: () = assert!(size_of::<GbaMemoryV1>() == EWRAM_SIZE + IWRAM_SIZE);
const _: () = assert!(align_of::<GbaMemoryV1>() == 1);
const _: () = assert!(size_of::<GbaRuntimeV3>() == 208);
const _: () = assert!(align_of::<GbaRuntimeV3>() == 8);
const _: () = assert!(offset_of!(GbaRuntimeV3, cpu) == 0);
const _: () = assert!(offset_of!(GbaRuntimeV3, memory) == 192);
const _: () = assert!(offset_of!(GbaRuntimeV3, abi_version) == 200);
const _: () = assert!(size_of::<GbaMachineStateV1>() == 0x60C24);
const _: () = assert!(align_of::<GbaMachineStateV1>() == 4);
const _: () = assert!(offset_of!(GbaMachineStateV1, memory) == 0);
const _: () = assert!(offset_of!(GbaMachineStateV1, abi_version) == 0x48000);
const _: () = assert!(offset_of!(GbaMachineStateV1, palette_ram) == 0x48004);
const _: () = assert!(offset_of!(GbaMachineStateV1, vram) == 0x48404);
const _: () = assert!(offset_of!(GbaMachineStateV1, oam) == 0x60404);
const _: () = assert!(offset_of!(GbaMachineStateV1, io) == 0x60804);
const _: () = assert!(offset_of!(GbaMachineStateV1, wave_ram) == 0x60C04);
const _: () = assert!(size_of::<GbaRuntimeV4>() == 224);
const _: () = assert!(align_of::<GbaRuntimeV4>() == 8);
const _: () = assert!(offset_of!(GbaRuntimeV4, base) == 0);
const _: () = assert!(offset_of!(GbaRuntimeV4, machine) == 208);
const _: () = assert!(offset_of!(GbaRuntimeV4, abi_version) == 216);
const _: () = assert!(size_of::<GbaExitV3>() == 16);
const _: () = assert!(align_of::<GbaExitV3>() == 4);
const _: () = assert!(offset_of!(GbaExitV3, reason) == 0);
const _: () = assert!(offset_of!(GbaExitV3, next_pc) == 4);
const _: () = assert!(offset_of!(GbaExitV3, fault_address) == 8);
const _: () = assert!(offset_of!(GbaExitV3, fault_code) == 12);

//! Runtime for `aot-compatible` packages (SPEC-062): native-code context
//! ABI, memory helpers called by generated code, the dispatcher that offers
//! instruction boundaries to native runs, and the package entry point.
//!
//! Generated code only receives guest addresses as integers; every guest
//! memory access goes through the `gba_rt_*` helpers (AGENTS rule 7).

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void};

use gba_core::bus::Bus;
use gba_core::cpu::{Cpu, CpuBus};
use gba_core::{NativeExec, NativeOutcome};

mod cli;

pub const NATIVE_ABI_VERSION: u32 = 1;

pub const EXIT_SEQ: u32 = 0;
pub const EXIT_BRANCH: u32 = 1;
pub const EXIT_INTERPRET: u32 = 2;

/// Context shared with generated code (`GbaNativeCtxV1`, NB-62-05).
#[repr(C)]
#[derive(Debug)]
pub struct NativeCtx {
    pub r: [u32; 16],
    pub cpsr: u32,
    pub next_seq: u32,
    pub cycles: u64,
    pub next_event: u64,
    pub retired: u64,
    pub last_opcode: u32,
    pub reserved: u32,
    pub wait: *const u8,
    pub bus: *mut c_void,
}

/// One table row exported by the generated object.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AotEntry {
    /// Guest PC with bit 0 set for Thumb entries.
    pub key: u32,
    pub index: u32,
    pub func: NativeFn,
}

pub type NativeFn = unsafe extern "C" fn(*mut NativeCtx, u32) -> u32;

const _: () = {
    assert!(std::mem::offset_of!(NativeCtx, cpsr) == 64);
    assert!(std::mem::offset_of!(NativeCtx, next_seq) == 68);
    assert!(std::mem::offset_of!(NativeCtx, cycles) == 72);
    assert!(std::mem::offset_of!(NativeCtx, next_event) == 80);
    assert!(std::mem::offset_of!(NativeCtx, retired) == 88);
    assert!(std::mem::offset_of!(NativeCtx, last_opcode) == 96);
    assert!(std::mem::offset_of!(NativeCtx, wait) == 104);
    assert!(std::mem::offset_of!(NativeCtx, bus) == 112);
    assert!(std::mem::size_of::<NativeCtx>() == 120);
};

// ------------------------------------------------------------------ helpers

/// # Safety
/// `ctx` must be the live context passed to a native run by [`NativeProgram`],
/// whose `bus` points at the `Bus` borrowed for that call.
#[inline]
unsafe fn with_bus<R>(ctx: *mut NativeCtx, f: impl FnOnce(&mut Bus) -> R) -> R {
    // SAFETY: guaranteed by the caller contract above; the dispatcher does
    // not touch the bus while native code runs.
    let ctx = unsafe { &mut *ctx };
    let bus = unsafe { &mut *(ctx.bus as *mut Bus) };
    bus.cycles = ctx.cycles;
    let r = f(bus);
    ctx.cycles = bus.cycles;
    ctx.next_event = bus.next_event();
    r
}

macro_rules! load_helper {
    ($name:ident, |$bus:ident, $addr:ident| $body:expr) => {
        /// Data load with the interpreter's exact semantics and cycle cost.
        ///
        /// # Safety
        /// Called only by generated code with the context of the current run.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(ctx: *mut NativeCtx, $addr: u32, open_bus: u32) -> u32 {
            unsafe {
                with_bus(ctx, |$bus| {
                    $bus.open_bus = open_bus;
                    $body
                })
            }
        }
    };
}

load_helper!(gba_rt_ldr, |bus, addr| Cpu::load32_rotated(bus, addr));
load_helper!(gba_rt_ldrh, |bus, addr| Cpu::load16_rotated(bus, addr));
load_helper!(gba_rt_ldrsh, |bus, addr| Cpu::load_signed16(bus, addr));
load_helper!(gba_rt_ldrb, |bus, addr| u32::from(bus.read8(addr, false)));
load_helper!(
    gba_rt_ldrsb,
    |bus, addr| bus.read8(addr, false) as i8 as i32 as u32
);

/// Word read for LDM/POP (aligned by the caller).
///
/// # Safety
/// Called only by generated code with the context of the current run.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gba_rt_read32(
    ctx: *mut NativeCtx,
    addr: u32,
    seq: u32,
    open_bus: u32,
) -> u32 {
    unsafe {
        with_bus(ctx, |bus| {
            bus.open_bus = open_bus;
            bus.read32(addr, seq != 0)
        })
    }
}

#[inline]
fn store_exit(addr: u32) -> u32 {
    // Any I/O write may change IRQ, DMA, HALT, timer or waitstate state:
    // finish the instruction and return to the dispatcher (NB-62-02).
    u32::from(addr >> 24 == 0x04)
}

/// # Safety
/// Called only by generated code with the context of the current run.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gba_rt_write32(
    ctx: *mut NativeCtx,
    addr: u32,
    value: u32,
    seq: u32,
) -> u32 {
    unsafe { with_bus(ctx, |bus| bus.write32(addr, value, seq != 0)) };
    store_exit(addr)
}

/// # Safety
/// Called only by generated code with the context of the current run.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gba_rt_write16(
    ctx: *mut NativeCtx,
    addr: u32,
    value: u32,
    seq: u32,
) -> u32 {
    unsafe { with_bus(ctx, |bus| bus.write16(addr, value as u16, seq != 0)) };
    store_exit(addr)
}

/// # Safety
/// Called only by generated code with the context of the current run.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gba_rt_write8(
    ctx: *mut NativeCtx,
    addr: u32,
    value: u32,
    seq: u32,
) -> u32 {
    unsafe { with_bus(ctx, |bus| bus.write8(addr, value as u8, seq != 0)) };
    store_exit(addr)
}

// ------------------------------------------------------------------ dispatcher

#[derive(Clone, Debug, Default)]
pub struct NativeCounters {
    pub retired: u64,
    pub exits_seq: u64,
    pub exits_branch: u64,
    pub exits_interpret: u64,
}

pub struct NativeProgram {
    map: HashMap<u32, (NativeFn, u32)>,
    wait: Box<[u8; 65]>,
    last_waitcnt: Option<u16>,
    pub counters: NativeCounters,
}

impl NativeProgram {
    /// Build the lookup table from the generated entry array.
    pub fn new(entries: &[AotEntry]) -> Self {
        let mut map = HashMap::with_capacity(entries.len());
        for e in entries {
            map.insert(e.key, (e.func, e.index));
        }
        Self {
            map,
            wait: Box::new([0; 65]),
            last_waitcnt: None,
            counters: NativeCounters::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl NativeExec for NativeProgram {
    fn run(&mut self, cpu: &mut Cpu, bus: &mut Bus) -> NativeOutcome {
        let thumb = cpu.thumb();
        let key = cpu.pc() | u32::from(thumb);
        let Some(&(func, index)) = self.map.get(&key) else {
            return NativeOutcome::Miss;
        };
        if self.last_waitcnt != Some(bus.waitcnt) {
            bus.wait_snapshot(&mut self.wait);
            self.last_waitcnt = Some(bus.waitcnt);
        }
        let mut ctx = NativeCtx {
            r: cpu.r,
            cpsr: cpu.cpsr,
            next_seq: u32::from(cpu.next_seq()),
            cycles: bus.cycles,
            next_event: bus.next_event(),
            retired: 0,
            last_opcode: bus.open_bus,
            reserved: 0,
            wait: self.wait.as_ptr(),
            bus: bus as *mut Bus as *mut c_void,
        };
        // SAFETY: `func` comes from the generated table for this ROM; it only
        // touches `ctx` and calls the helpers above, which use `ctx.bus`
        // while `bus` is not otherwise accessed.
        let reason = unsafe { func(&mut ctx, index) };
        cpu.r = ctx.r;
        cpu.cpsr = ctx.cpsr;
        bus.cycles = ctx.cycles;
        bus.open_bus = ctx.last_opcode;
        self.counters.retired += ctx.retired;
        match reason {
            EXIT_BRANCH => {
                self.counters.exits_branch += 1;
                cpu.refill(bus);
                NativeOutcome::Ran
            }
            EXIT_INTERPRET => {
                self.counters.exits_interpret += 1;
                cpu.set_next_seq(ctx.next_seq != 0);
                if ctx.retired == 0 {
                    NativeOutcome::Interpret
                } else {
                    // Progress was made; the shell runs on the next step,
                    // after the boundary checks (it is the first instruction
                    // of that call, so the run will request it again).
                    NativeOutcome::Ran
                }
            }
            _ => {
                self.counters.exits_seq += 1;
                cpu.set_next_seq(ctx.next_seq != 0);
                NativeOutcome::Ran
            }
        }
    }
}

/// Package entry point called by the generated `main` (NB-62-07).
///
/// # Safety
/// `argv` must hold `argc` valid C strings; `entries` must point at `count`
/// rows and `sha256` at 32 bytes, all with static lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gba_native_main(
    argc: c_int,
    argv: *const *const c_char,
    entries: *const AotEntry,
    count: u32,
    sha256: *const u8,
) -> c_int {
    let args: Vec<String> = (0..argc.max(0) as usize)
        .map(|i| {
            // SAFETY: caller contract.
            let p = unsafe { *argv.add(i) };
            unsafe { std::ffi::CStr::from_ptr(p) }
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    // SAFETY: caller contract.
    let entries = unsafe { std::slice::from_raw_parts(entries, count as usize) };
    let sha = unsafe { std::slice::from_raw_parts(sha256, 32) };
    let mut expected = [0u8; 32];
    expected.copy_from_slice(sha);
    match cli::run(&args, entries, expected) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gba-native: {e}");
            1
        }
    }
}

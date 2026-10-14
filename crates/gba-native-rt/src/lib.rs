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

pub const NATIVE_ABI_VERSION: u32 = 2;

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
    /// Index into the guard table, or `u32::MAX` for ROM code (SPEC-065).
    pub guard: u32,
    pub reserved: u32,
}

/// Expected bytes of a RAM run (SPEC-065 RAM-65-02).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AotGuard {
    pub addr: u32,
    pub len: u32,
    pub bytes: *const u8,
}

struct GuardState {
    addr: u32,
    bytes: &'static [u8],
    pages: std::ops::RangeInclusive<usize>,
    /// Generation sum of the pages at the last comparison and its result.
    checked: Option<(u64, bool)>,
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
fn store_exit(bus: &Bus, addr: u32) -> u32 {
    // Any I/O write may change IRQ, DMA, HALT, timer or waitstate state
    // (NB-62-02); a write to a page with compiled RAM code may invalidate
    // it (RAM-65-04). Finish the instruction and return to the dispatcher.
    u32::from(addr >> 24 == 0x04 || bus.is_code_page(addr))
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
    unsafe {
        with_bus(ctx, |bus| {
            bus.write32(addr, value, seq != 0);
            store_exit(bus, addr)
        })
    }
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
    unsafe {
        with_bus(ctx, |bus| {
            bus.write16(addr, value as u16, seq != 0);
            store_exit(bus, addr)
        })
    }
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
    unsafe {
        with_bus(ctx, |bus| {
            bus.write8(addr, value as u8, seq != 0);
            store_exit(bus, addr)
        })
    }
}

// ------------------------------------------------------------------ dispatcher

#[derive(Clone, Debug, Default)]
pub struct NativeCounters {
    pub guard_failures: u64,
    pub guard_compares: u64,
    pub retired: u64,
    pub exits_seq: u64,
    pub exits_branch: u64,
    pub exits_interpret: u64,
}

pub struct NativeProgram {
    /// Per key: (function, resume index, guard or `u32::MAX`).
    map: HashMap<u32, Vec<(NativeFn, u32, u32)>>,
    guards: Vec<GuardState>,
    wait: Box<[u8; 65]>,
    last_waitcnt: Option<u16>,
    pub counters: NativeCounters,
}

impl NativeProgram {
    /// Build the lookup table from the generated entry array.
    pub fn new(entries: &[AotEntry], guards: &[AotGuard]) -> Self {
        let mut map: HashMap<u32, Vec<(NativeFn, u32, u32)>> =
            HashMap::with_capacity(entries.len());
        for e in entries {
            map.entry(e.key)
                .or_default()
                .push((e.func, e.index, e.guard));
        }
        let guards = guards
            .iter()
            .map(|g| {
                // SAFETY: the generated object keeps the guard bytes alive
                // for the whole program.
                let bytes: &'static [u8] =
                    unsafe { std::slice::from_raw_parts(g.bytes, g.len as usize) };
                let first = gba_core::bus::ram_page(g.addr).unwrap_or(0);
                let last = gba_core::bus::ram_page(g.addr + g.len.max(1) - 1).unwrap_or(first);
                GuardState {
                    addr: g.addr,
                    bytes,
                    pages: first..=last.max(first),
                    checked: None,
                }
            })
            .collect();
        Self {
            map,
            guards,
            wait: Box::new([0; 65]),
            last_waitcnt: None,
            counters: NativeCounters::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn guard_count(&self) -> usize {
        self.guards.len()
    }

    fn mark_code_pages(&self, bus: &mut Bus) {
        bus.code_pages.fill(false);
        for g in &self.guards {
            for p in g.pages.clone() {
                bus.code_pages[p] = true;
            }
        }
        bus.code_pages_marked = true;
    }

    /// RAM-65-03: bytes must match; the page generation sum skips the
    /// comparison when nothing was written since the last check.
    fn guard_ok(&mut self, i: usize, bus: &Bus) -> bool {
        let g = &mut self.guards[i];
        let sum: u64 = g.pages.clone().map(|p| u64::from(bus.ram_gen[p])).sum();
        if let Some((s, ok)) = g.checked
            && s == sum
        {
            return ok;
        }
        self.counters.guard_compares += 1;
        let ok = bus.ram_bytes(g.addr, g.bytes.len()) == Some(g.bytes);
        g.checked = Some((sum, ok));
        ok
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl NativeExec for NativeProgram {
    fn run(&mut self, cpu: &mut Cpu, bus: &mut Bus) -> NativeOutcome {
        let thumb = cpu.thumb();
        let key = cpu.pc() | u32::from(thumb);
        let Some(candidates) = self.map.get(&key) else {
            return NativeOutcome::Miss;
        };
        let mut chosen = None;
        let candidates = candidates.clone();
        for (func, index, guard) in candidates {
            if guard == u32::MAX {
                chosen = Some((func, index));
                break;
            }
            if !bus.code_pages_marked {
                self.mark_code_pages(bus);
            }
            if self.guard_ok(guard as usize, bus) {
                chosen = Some((func, index));
                break;
            }
        }
        let Some((func, index)) = chosen else {
            self.counters.guard_failures += 1;
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
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn gba_native_main(
    argc: c_int,
    argv: *const *const c_char,
    entries: *const AotEntry,
    count: u32,
    sha256: *const u8,
    guards: *const AotGuard,
    guard_count: u32,
    rom: *const u8,
    rom_len: u64,
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
    // SAFETY: caller contract (generated table).
    let guards = if guard_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(guards, guard_count as usize) }
    };
    // SAFETY: an embedded ROM (SPEC-066) is a static constant in the image.
    let embedded: Option<&'static [u8]> = (!rom.is_null() && rom_len > 0)
        .then(|| unsafe { std::slice::from_raw_parts(rom, rom_len as usize) });
    match cli::run(&args, entries, guards, embedded, expected) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gba-native: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gba_core::{Gba, GbaConfig};

    unsafe extern "C" fn fake_run(ctx: *mut NativeCtx, _entry: u32) -> u32 {
        // Pretend one instruction ran and stay at the same PC.
        unsafe { (*ctx).retired = 1 };
        EXIT_SEQ
    }

    #[test]
    fn ram_guard_rejects_changed_bytes_and_accepts_restored_ones() {
        static EXPECTED: [u8; 4] = [0x01, 0x20, 0x70, 0x47]; // movs r0,#1; bx lr
        let rom = vec![0u8; 0x200];
        let mut gba = Gba::new(GbaConfig {
            rom: &rom,
            bios: None,
            backup: None,
            save: None,
        });
        let entries = [AotEntry {
            key: 0x0300_0000 | 1,
            index: 0,
            func: fake_run,
            guard: 0,
            reserved: 0,
        }];
        let guards = [AotGuard {
            addr: 0x0300_0000,
            len: 4,
            bytes: EXPECTED.as_ptr(),
        }];
        let mut program = NativeProgram::new(&entries, &guards);
        gba.cpu.write_cpsr(gba.cpu.cpsr | gba_core::cpu::FLAG_T);
        gba.cpu.set_pc(0x0300_0000);

        // RAM still zero: guard fails → miss.
        assert_eq!(program.run(&mut gba.cpu, &mut gba.bus), NativeOutcome::Miss);
        assert_eq!(program.counters.guard_failures, 1);
        assert!(gba.bus.is_code_page(0x0300_0010), "code pages are marked");

        for (i, b) in EXPECTED.iter().enumerate() {
            gba.bus.poke8(0x0300_0000 + i as u32, *b);
        }
        assert_eq!(program.run(&mut gba.cpu, &mut gba.bus), NativeOutcome::Ran);
        let compares = program.counters.guard_compares;
        // No write since the last check: the generation sum skips memcmp.
        assert_eq!(program.run(&mut gba.cpu, &mut gba.bus), NativeOutcome::Ran);
        assert_eq!(program.counters.guard_compares, compares);

        // Overwrite the code (e.g. a DMA or CPU store): miss again.
        gba.bus.poke8(0x0300_0002, 0xFF);
        assert_eq!(program.run(&mut gba.cpu, &mut gba.bus), NativeOutcome::Miss);
    }
}

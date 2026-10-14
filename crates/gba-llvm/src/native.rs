//! Native runs for `aot-compatible` packages (SPEC-062).
//!
//! Each run is a linear sequence of ARM or Thumb instructions lowered to one
//! LLVM function `i32 run(ptr ctx, i32 entry)` with a resume entry per
//! instruction. The lowering mirrors `gba-core`'s interpreter instruction by
//! instruction, including cycle accounting (NB-62-01), so native and
//! interpreted execution are cycle-identical. Instructions whose effects are
//! outside the native subset exit with `EXIT_INTERPRET` (a one-step shell).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::CString;
use std::path::Path;
use std::ptr;

use llvm_sys::LLVMIntPredicate::*;
use llvm_sys::core::*;
use llvm_sys::prelude::*;
use llvm_sys::target::*;
use llvm_sys::target_machine::*;
use llvm_sys::{LLVMIntPredicate, LLVMLinkage};

use crate::{
    EmitInfo, Optimization, emit_object, initialize_backend, native_target, run_optimization,
    take_llvm_message, verify_module,
};

pub const EXIT_SEQ: u64 = 0;
pub const EXIT_BRANCH: u64 = 1;
pub const EXIT_INTERPRET: u64 = 2;

const ROM_START: u32 = 0x0800_0000;
const MAX_RUN: usize = 1024;

/// LLVM code generation target for native runs (SPEC-068 TGT-68-01).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetSpec {
    pub triple: String,
    pub cpu: String,
    pub features: String,
    /// LLVM backend to initialise: x86, aarch64, arm or webassembly.
    pub backend: &'static str,
}

impl TargetSpec {
    /// The build host.
    pub fn host() -> Self {
        let t = native_target();
        Self::for_triple(t.triple).expect("host triple is supported")
    }

    /// Supported package targets (SPEC-068 TGT-68-02).
    pub fn for_triple(triple: &str) -> Result<Self, String> {
        let (cpu, features, backend) = match triple {
            "aarch64-apple-darwin"
            | "aarch64-unknown-linux-gnu"
            | "aarch64-apple-ios"
            | "aarch64-apple-ios-sim"
            | "aarch64-linux-android" => ("generic", "", "aarch64"),
            "x86_64-apple-darwin" | "x86_64-unknown-linux-gnu" | "x86_64-pc-windows-msvc" => {
                ("x86-64", "", "x86")
            }
            "armv7-unknown-linux-gnueabihf" => ("cortex-a7", "+neon,+vfp4", "arm"),
            "wasm32-unknown-emscripten" => ("generic", "", "webassembly"),
            other => {
                return Err(format!(
                    "unsupported target `{other}`; supported: aarch64-apple-darwin, x86_64-apple-darwin, \
                     x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, armv7-unknown-linux-gnueabihf, \
                     x86_64-pc-windows-msvc, aarch64-apple-ios, aarch64-apple-ios-sim, \
                     aarch64-linux-android, wasm32-unknown-emscripten"
                ));
            }
        };
        Ok(Self {
            triple: triple.to_owned(),
            cpu: cpu.to_owned(),
            features: features.to_owned(),
            backend,
        })
    }

    /// Symbol of the generated entry point: SDL owns `main` on iOS and
    /// Android and calls `SDL_main` (SPEC-068).
    pub fn entry_symbol(&self) -> &'static str {
        if self.triple.contains("apple-ios") || self.triple.contains("android") {
            "SDL_main"
        } else {
            "main"
        }
    }

    fn llvm_triple(&self) -> String {
        match self.triple.as_str() {
            // LLVM spells the simulator/Android variants differently.
            "aarch64-apple-ios-sim" => "arm64-apple-ios14.0-simulator".to_owned(),
            "aarch64-apple-ios" => "arm64-apple-ios14.0".to_owned(),
            "aarch64-linux-android" => "aarch64-linux-android24".to_owned(),
            t => t.to_owned(),
        }
    }
}

/// A guest entry point: address and ISA state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Entry {
    pub pc: u32,
    pub thumb: bool,
}

#[derive(Clone, Debug, Default)]
pub struct NativeStats {
    pub ram_runs: usize,
    pub ram_instructions: usize,
    pub runs: usize,
    pub instructions: usize,
    pub shells: usize,
    pub entries_in: usize,
    pub entries_static: usize,
}

pub struct NativeEmitResult {
    pub info: EmitInfo,
    pub stats: NativeStats,
}

// ------------------------------------------------------------------ decoding

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    /// Execution may continue with the next instruction.
    Continue,
    /// Always leaves the run (unconditional branch or shell).
    Terminate,
}

fn bit(v: u32, n: u32) -> bool {
    (v >> n) & 1 != 0
}

/// ARM shells (NB-62-03). Must agree with `lower_arm`.
fn arm_is_shell(op: u32) -> bool {
    let cond = op >> 28;
    if cond == 0xF {
        return false; // never executes: lowered as skipped
    }
    match (op >> 25) & 7 {
        0 => {
            if op & 0x0FFF_FFF0 == 0x012F_FF10 {
                false
            } else if op & 0x0FC0_00F0 == 0x0000_0090 || op & 0x0F80_00F0 == 0x0080_0090 {
                false
            } else if op & 0x0FB0_0FF0 == 0x0100_0090 {
                true // SWP
            } else if op & 0x90 == 0x90 && (op >> 5) & 3 != 0 {
                let load = bit(op, 20);
                let sh = (op >> 5) & 3;
                let rn = (op >> 16) & 0xF;
                let writeback = bit(op, 21) || !bit(op, 24);
                (!load && sh != 1) || (rn == 15 && writeback)
            } else if op & 0x0FBF_0FFF == 0x010F_0000 || op & 0x0FB0_FFF0 == 0x0120_F000 {
                true // MRS / MSR
            } else if op & 0x0190_0000 == 0x0100_0000 {
                true // undefined space
            } else {
                // Data processing: Rd = PC with S restores CPSR.
                (op >> 12) & 0xF == 15 && bit(op, 20)
            }
        }
        1 => {
            op & 0x0FB0_F000 == 0x0320_F000
                || op & 0x0190_0000 == 0x0100_0000
                || ((op >> 12) & 0xF == 15 && bit(op, 20))
        }
        2 | 3 => {
            if (op >> 25) & 7 == 3 && op & 0x10 != 0 {
                return true;
            }
            let rn = (op >> 16) & 0xF;
            let writeback = bit(op, 21) || !bit(op, 24);
            rn == 15 && writeback
        }
        4 => {
            let rn = (op >> 16) & 0xF;
            bit(op, 22) || rn == 15 || op & 0xFFFF == 0
        }
        5 => false,
        _ => true, // coprocessor, SWI
    }
}

fn arm_writes_pc(op: u32) -> bool {
    match (op >> 25) & 7 {
        0 => {
            if op & 0x0FFF_FFF0 == 0x012F_FF10 {
                return true;
            }
            if op & 0x0FC0_00F0 == 0x0000_0090 || op & 0x0F80_00F0 == 0x0080_0090 {
                return false;
            }
            if op & 0x90 == 0x90 && (op >> 5) & 3 != 0 {
                return bit(op, 20) && (op >> 12) & 0xF == 15;
            }
            let opcode = (op >> 21) & 0xF;
            (op >> 12) & 0xF == 15 && !(0x8..=0xB).contains(&opcode)
        }
        1 => {
            let opcode = (op >> 21) & 0xF;
            (op >> 12) & 0xF == 15 && !(0x8..=0xB).contains(&opcode)
        }
        2 | 3 => bit(op, 20) && (op >> 12) & 0xF == 15,
        4 => bit(op, 20) && bit(op, 15),
        5 => true,
        _ => false,
    }
}

fn arm_flow(op: u32) -> Flow {
    let always = op >> 28 == 0xE;
    if arm_is_shell(op) {
        return if always {
            Flow::Terminate
        } else {
            Flow::Continue
        };
    }
    if always && arm_writes_pc(op) {
        Flow::Terminate
    } else {
        Flow::Continue
    }
}

fn arm_direct_target(pc: u32, op: u32) -> Option<u32> {
    if (op >> 25) & 7 == 5 && op >> 28 != 0xF {
        let offset = (((op & 0x00FF_FFFF) << 8) as i32 >> 6) as u32;
        Some(pc.wrapping_add(8).wrapping_add(offset))
    } else {
        None
    }
}

/// Thumb shells (NB-62-03). Must agree with `lower_thumb`.
fn thumb_is_shell(op: u32) -> bool {
    match op >> 13 {
        5 => {
            op & (1 << 12) != 0
                && (op >> 8) & 0xF != 0
                // Undefined encodings, or PUSH/POP with an empty list.
                && ((op >> 9) & 3 != 2 || op & 0x1FF == 0)
        }
        6 => {
            if op & (1 << 12) == 0 {
                op & 0xFF == 0 // empty LDMIA/STMIA list
            } else {
                (op >> 8) & 0xF >= 0xE // undefined / SWI
            }
        }
        7 => (op >> 11) & 3 == 1, // BLX suffix (ARMv5)
        _ => false,
    }
}

fn thumb_flow(op: u32) -> Flow {
    if thumb_is_shell(op) {
        return Flow::Terminate;
    }
    let terminates = match op >> 13 {
        2 if op >> 10 == 0b010001 => {
            let rd = (op & 7) | ((op >> 4) & 8);
            let kind = (op >> 8) & 3;
            kind == 3 || (rd == 15 && kind != 1)
        }
        5 => op & (1 << 12) != 0 && (op >> 9) & 3 == 2 && op & (1 << 11) != 0 && op & 0x100 != 0,
        7 => matches!((op >> 11) & 3, 0 | 3),
        _ => false,
    };
    if terminates {
        Flow::Terminate
    } else {
        Flow::Continue
    }
}

fn thumb_direct_target(pc: u32, op: u32, prev: Option<u32>) -> Option<u32> {
    match op >> 13 {
        6 if op & (1 << 12) != 0 && (op >> 8) & 0xF < 0xE => {
            let offset = ((op & 0xFF) as i8 as i32 as u32) << 1;
            Some(pc.wrapping_add(4).wrapping_add(offset))
        }
        7 => match (op >> 11) & 3 {
            0 => {
                let offset = (((op & 0x7FF) << 21) as i32 >> 20) as u32;
                Some(pc.wrapping_add(4).wrapping_add(offset))
            }
            3 => {
                let prefix = prev?;
                if prefix >> 11 != 0b11110 {
                    return None;
                }
                let hi = (((prefix & 0x7FF) << 21) as i32 >> 9) as u32;
                Some(
                    pc.wrapping_sub(2)
                        .wrapping_add(4)
                        .wrapping_add(hi)
                        .wrapping_add((op & 0x7FF) << 1),
                )
            }
            _ => None,
        },
        _ => None,
    }
}

struct Run {
    entry: Entry,
    /// Opcodes in order; instruction i is at `entry.pc + i * size`.
    ops: Vec<u32>,
    /// Address to continue at if the run ends without a terminator.
    tail: Option<u32>,
    /// Content guard for code in RAM (SPEC-065): index into the guards.
    guard: Option<usize>,
}

/// Code observed in EWRAM/IWRAM during profiling (SPEC-065 RAM-65-01):
/// entry point plus the bytes present when it executed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RamEntry {
    pub pc: u32,
    pub thumb: bool,
    pub bytes: Vec<u8>,
}

/// Decode one RAM run from its snapshot: opcodes, tail and bytes covered.
fn ram_run(e: &RamEntry) -> Option<(Vec<u32>, Option<u32>, usize)> {
    let size = if e.thumb { 2 } else { 4 };
    let mut ops = Vec::new();
    let mut off = 0usize;
    let mut tail = None;
    while off + size <= e.bytes.len() {
        let b = &e.bytes[off..off + size];
        let op = if e.thumb {
            u32::from(u16::from_le_bytes([b[0], b[1]]))
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        };
        ops.push(op);
        off += size;
        let flow = if e.thumb {
            thumb_flow(op)
        } else {
            arm_flow(op)
        };
        if flow == Flow::Terminate {
            break;
        }
        if ops.len() >= MAX_RUN || off + size > e.bytes.len() {
            tail = Some(e.pc.wrapping_add(off as u32));
            break;
        }
    }
    (!ops.is_empty()).then_some((ops, tail, off))
}

pub struct Guard {
    pub addr: u32,
    pub bytes: Vec<u8>,
}

/// RAM runs and their guards; entries already covered by an identical
/// earlier run (same bytes at the same address) reuse its resume index.
fn build_ram_runs(entries: &[RamEntry], guards: &mut Vec<Guard>) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for e in entries {
        let covered = runs.iter().any(|r| {
            let g = &guards[r.guard.expect("RAM runs have guards")];
            let size = if r.entry.thumb { 2 } else { 4 };
            r.entry.thumb == e.thumb
                && e.pc >= g.addr
                && e.pc < g.addr + g.bytes.len() as u32
                && (e.pc - g.addr) % size == 0
                && {
                    let off = (e.pc - g.addr) as usize;
                    let n = (g.bytes.len() - off).min(e.bytes.len());
                    g.bytes[off..off + n] == e.bytes[..n]
                }
        });
        if covered {
            continue;
        }
        if let Some((ops, tail, len)) = ram_run(e) {
            guards.push(Guard {
                addr: e.pc,
                bytes: e.bytes[..len].to_vec(),
            });
            runs.push(Run {
                entry: Entry {
                    pc: e.pc,
                    thumb: e.thumb,
                },
                ops,
                tail,
                guard: Some(guards.len() - 1),
            });
        }
    }
    runs
}

fn rom_word(rom: &[u8], addr: u32, thumb: bool) -> Option<u32> {
    if !(ROM_START..ROM_START + 0x0200_0000).contains(&addr) {
        return None;
    }
    let o = (addr - ROM_START) as usize;
    if thumb {
        rom.get(o..o + 2)
            .map(|b| u32::from(u16::from_le_bytes([b[0], b[1]])))
    } else {
        rom.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Build runs covering every entry; follows direct branches when requested.
fn build_runs(rom: &[u8], entries: &[Entry], follow: bool, stats: &mut NativeStats) -> Vec<Run> {
    let mut pending: BTreeSet<Entry> = entries.iter().copied().collect();
    let mut covered: HashMap<Entry, ()> = HashMap::new();
    let mut runs = Vec::new();
    let mut seen_static = BTreeSet::new();
    while let Some(entry) = pending.pop_first() {
        if covered.contains_key(&entry) {
            continue;
        }
        let size = if entry.thumb { 2 } else { 4 };
        let mut ops = Vec::new();
        let mut pc = entry.pc;
        let mut tail = None;
        loop {
            let here = Entry {
                pc,
                thumb: entry.thumb,
            };
            if !ops.is_empty() && covered.contains_key(&here) {
                tail = Some(pc);
                break;
            }
            let Some(op) = rom_word(rom, pc, entry.thumb) else {
                tail = if ops.is_empty() { None } else { Some(pc) };
                break;
            };
            ops.push(op);
            covered.insert(here, ());
            if follow {
                let target = if entry.thumb {
                    let prev = if ops.len() >= 2 {
                        Some(ops[ops.len() - 2])
                    } else {
                        None
                    };
                    thumb_direct_target(pc, op, prev)
                } else {
                    arm_direct_target(pc, op)
                };
                if let Some(t) = target {
                    let e = Entry {
                        pc: t,
                        thumb: entry.thumb,
                    };
                    if rom_word(rom, t, entry.thumb).is_some() && seen_static.insert(e) {
                        pending.insert(e);
                    }
                }
            }
            let flow = if entry.thumb {
                thumb_flow(op)
            } else {
                arm_flow(op)
            };
            pc = pc.wrapping_add(size);
            if flow == Flow::Terminate {
                break;
            }
            if ops.len() >= MAX_RUN {
                tail = Some(pc);
                break;
            }
        }
        if ops.is_empty() {
            continue;
        }
        runs.push(Run {
            entry,
            ops,
            tail,
            guard: None,
        });
    }
    stats.entries_static = seen_static.len();
    runs
}

// ------------------------------------------------------------------ emitter

struct Types {
    i1: LLVMTypeRef,
    i32: LLVMTypeRef,
    i64: LLVMTypeRef,
    ptr: LLVMTypeRef,
    ctx: LLVMTypeRef,
    run_fn: LLVMTypeRef,
    load_fn: LLVMTypeRef,
    read_fn: LLVMTypeRef,
    write_fn: LLVMTypeRef,
    fshr_fn: LLVMTypeRef,
}

struct Helpers {
    ldr: LLVMValueRef,
    ldrh: LLVMValueRef,
    ldrsh: LLVMValueRef,
    ldrb: LLVMValueRef,
    ldrsb: LLVMValueRef,
    read32: LLVMValueRef,
    write32: LLVMValueRef,
    write16: LLVMValueRef,
    write8: LLVMValueRef,
    fshr: LLVMValueRef,
}

/// Per-function state: allocas for guest registers and bookkeeping.
struct Fn {
    func: LLVMValueRef,
    ctx: LLVMValueRef,
    regs: [LLVMValueRef; 15],
    n: LLVMValueRef,
    z: LLVMValueRef,
    c: LLVMValueRef,
    v: LLVMValueRef,
    seq: LLVMValueRef,
    cycles: LLVMValueRef,
    next_event: LLVMValueRef,
    retired: LLVMValueRef,
    lastop: LLVMValueRef,
    cpsr_rest: LLVMValueRef,
    wait: LLVMValueRef,
    exit_pc: LLVMValueRef,
    exit_reason: LLVMValueRef,
    exit_bb: LLVMBasicBlockRef,
    /// Set when a store helper requested an exit after this instruction.
    store_exit: LLVMValueRef,
}

struct Emitter {
    context: LLVMContextRef,
    module: LLVMModuleRef,
    b: LLVMBuilderRef,
    t: Types,
    h: Helpers,
}

const EMPTY: *const std::ffi::c_char = c"".as_ptr();

impl Emitter {
    // All methods wrap LLVM C API calls on handles owned by this emitter;
    // values passed in were produced by the same context and builder.

    fn c32(&self, v: u32) -> LLVMValueRef {
        unsafe { LLVMConstInt(self.t.i32, u64::from(v), 0) }
    }
    fn c64(&self, v: u64) -> LLVMValueRef {
        unsafe { LLVMConstInt(self.t.i64, v, 0) }
    }
    fn c1(&self, v: bool) -> LLVMValueRef {
        unsafe { LLVMConstInt(self.t.i1, u64::from(v), 0) }
    }
    fn load(&self, ty: LLVMTypeRef, p: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildLoad2(self.b, ty, p, EMPTY) }
    }
    fn store(&self, v: LLVMValueRef, p: LLVMValueRef) {
        unsafe { LLVMBuildStore(self.b, v, p) };
    }
    fn add(&self, a: LLVMValueRef, b: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildAdd(self.b, a, b, EMPTY) }
    }
    fn sub(&self, a: LLVMValueRef, b: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildSub(self.b, a, b, EMPTY) }
    }
    fn mul(&self, a: LLVMValueRef, b: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildMul(self.b, a, b, EMPTY) }
    }
    fn and(&self, a: LLVMValueRef, b: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildAnd(self.b, a, b, EMPTY) }
    }
    fn or(&self, a: LLVMValueRef, b: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildOr(self.b, a, b, EMPTY) }
    }
    fn xor(&self, a: LLVMValueRef, b: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildXor(self.b, a, b, EMPTY) }
    }
    fn not(&self, a: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildNot(self.b, a, EMPTY) }
    }
    fn shl(&self, a: LLVMValueRef, b: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildShl(self.b, a, b, EMPTY) }
    }
    fn lshr(&self, a: LLVMValueRef, b: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildLShr(self.b, a, b, EMPTY) }
    }
    fn ashr(&self, a: LLVMValueRef, b: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildAShr(self.b, a, b, EMPTY) }
    }
    fn icmp(&self, p: LLVMIntPredicate, a: LLVMValueRef, b: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildICmp(self.b, p, a, b, EMPTY) }
    }
    fn select(&self, c: LLVMValueRef, a: LLVMValueRef, b: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildSelect(self.b, c, a, b, EMPTY) }
    }
    fn zext(&self, v: LLVMValueRef, ty: LLVMTypeRef) -> LLVMValueRef {
        unsafe { LLVMBuildZExt(self.b, v, ty, EMPTY) }
    }
    fn sext(&self, v: LLVMValueRef, ty: LLVMTypeRef) -> LLVMValueRef {
        unsafe { LLVMBuildSExt(self.b, v, ty, EMPTY) }
    }
    fn trunc(&self, v: LLVMValueRef, ty: LLVMTypeRef) -> LLVMValueRef {
        unsafe { LLVMBuildTrunc(self.b, v, ty, EMPTY) }
    }
    fn br(&self, bb: LLVMBasicBlockRef) {
        unsafe { LLVMBuildBr(self.b, bb) };
    }
    fn cond_br(&self, c: LLVMValueRef, t: LLVMBasicBlockRef, f: LLVMBasicBlockRef) {
        unsafe { LLVMBuildCondBr(self.b, c, t, f) };
    }
    fn block(&self, f: &Fn) -> LLVMBasicBlockRef {
        unsafe { LLVMAppendBasicBlockInContext(self.context, f.func, EMPTY) }
    }
    fn at(&self, bb: LLVMBasicBlockRef) {
        unsafe { LLVMPositionBuilderAtEnd(self.b, bb) };
    }
    fn field(&self, f: &Fn, index: u32) -> LLVMValueRef {
        unsafe { LLVMBuildStructGEP2(self.b, self.t.ctx, f.ctx, index, EMPTY) }
    }
    fn reg_ptr_in_ctx(&self, f: &Fn, r: u32) -> LLVMValueRef {
        let regs = self.field(f, 0);
        let arr = unsafe { LLVMArrayType2(self.t.i32, 16) };
        let mut idx = [self.c32(0), self.c32(r)];
        unsafe { LLVMBuildInBoundsGEP2(self.b, arr, regs, idx.as_mut_ptr(), 2, EMPTY) }
    }
    fn call(&self, ty: LLVMTypeRef, func: LLVMValueRef, args: &mut [LLVMValueRef]) -> LLVMValueRef {
        unsafe {
            LLVMBuildCall2(
                self.b,
                ty,
                func,
                args.as_mut_ptr(),
                args.len() as u32,
                EMPTY,
            )
        }
    }

    // ---- guest state

    fn reg(&self, f: &Fn, r: u32) -> LLVMValueRef {
        self.load(self.t.i32, f.regs[r as usize])
    }
    fn set_reg(&self, f: &Fn, r: u32, v: LLVMValueRef) {
        self.store(v, f.regs[r as usize]);
    }
    /// Read a register; r15 yields the pipeline constant `pc_value`.
    fn read(&self, f: &Fn, r: u32, pc_value: u32) -> LLVMValueRef {
        if r == 15 {
            self.c32(pc_value)
        } else {
            self.reg(f, r)
        }
    }
    fn flag(&self, p: LLVMValueRef) -> LLVMValueRef {
        self.load(self.t.i1, p)
    }
    fn set_nz(&self, f: &Fn, v: LLVMValueRef) {
        let n = self.icmp(LLVMIntSLT, v, self.c32(0));
        let z = self.icmp(LLVMIntEQ, v, self.c32(0));
        self.store(n, f.n);
        self.store(z, f.z);
    }
    fn add_cycles(&self, f: &Fn, v: LLVMValueRef) {
        let c = self.load(self.t.i64, f.cycles);
        self.store(self.add(c, v), f.cycles);
    }
    fn idle(&self, f: &Fn, n: u64) {
        self.add_cycles(f, self.c64(n));
    }

    /// add_flags of the interpreter: a + b + carry, optionally setting NZCV.
    fn add_flags(
        &self,
        f: &Fn,
        a: LLVMValueRef,
        b: LLVMValueRef,
        carry: LLVMValueRef,
        set: bool,
    ) -> LLVMValueRef {
        let wa = self.zext(a, self.t.i64);
        let wb = self.zext(b, self.t.i64);
        let wc = self.zext(carry, self.t.i64);
        let wide = self.add(self.add(wa, wb), wc);
        let res = self.trunc(wide, self.t.i32);
        if set {
            self.set_nz(f, res);
            let c = self.icmp(LLVMIntUGT, wide, self.c64(0xFFFF_FFFF));
            self.store(c, f.c);
            let ov = self.and(self.not(self.xor(a, b)), self.xor(a, res));
            let v = self.icmp(LLVMIntSLT, ov, self.c32(0));
            self.store(v, f.v);
        }
        res
    }
    fn sub_flags(
        &self,
        f: &Fn,
        a: LLVMValueRef,
        b: LLVMValueRef,
        carry: LLVMValueRef,
        set: bool,
    ) -> LLVMValueRef {
        let nb = self.not(b);
        self.add_flags(f, a, nb, carry, set)
    }

    fn bit_of(&self, v: LLVMValueRef, n: LLVMValueRef) -> LLVMValueRef {
        let s = self.lshr(v, n);
        self.trunc(s, self.t.i1)
    }

    /// Immediate-amount shift (encoding semantics, mirrors `Cpu::shift_imm`).
    fn shift_imm(
        &self,
        f: &Fn,
        ty: u32,
        v: LLVMValueRef,
        amount: u32,
    ) -> (LLVMValueRef, LLVMValueRef) {
        let c = self.flag(f.c);
        match ty {
            0 => {
                if amount == 0 {
                    (v, c)
                } else {
                    (
                        self.shl(v, self.c32(amount)),
                        self.bit_of(v, self.c32(32 - amount)),
                    )
                }
            }
            1 => {
                if amount == 0 {
                    (self.c32(0), self.bit_of(v, self.c32(31)))
                } else {
                    (
                        self.lshr(v, self.c32(amount)),
                        self.bit_of(v, self.c32(amount - 1)),
                    )
                }
            }
            2 => {
                if amount == 0 {
                    (self.ashr(v, self.c32(31)), self.bit_of(v, self.c32(31)))
                } else {
                    (
                        self.ashr(v, self.c32(amount)),
                        self.bit_of(v, self.c32(amount - 1)),
                    )
                }
            }
            _ => {
                if amount == 0 {
                    let cin = self.shl(self.zext(c, self.t.i32), self.c32(31));
                    let res = self.or(cin, self.lshr(v, self.c32(1)));
                    (res, self.trunc(v, self.t.i1))
                } else {
                    let res = self.call(self.t.fshr_fn, self.h.fshr, &mut [v, v, self.c32(amount)]);
                    (res, self.bit_of(v, self.c32(amount - 1)))
                }
            }
        }
    }

    /// Register-amount shift (low byte), mirrors `Cpu::shift_reg`.
    fn shift_reg(
        &self,
        f: &Fn,
        ty: u32,
        v: LLVMValueRef,
        amount: LLVMValueRef,
    ) -> (LLVMValueRef, LLVMValueRef) {
        let c = self.flag(f.c);
        let zero = self.icmp(LLVMIntEQ, amount, self.c32(0));
        let lt32 = self.icmp(LLVMIntULT, amount, self.c32(32));
        let eq32 = self.icmp(LLVMIntEQ, amount, self.c32(32));
        let amt31 = self.and(amount, self.c32(31));
        let amt_m1 = self.and(self.sub(amount, self.c32(1)), self.c32(31));
        let f0 = self.c1(false);
        let (res, carry) = match ty {
            0 => {
                let shifted = self.shl(v, amt31);
                let res = self.select(lt32, shifted, self.c32(0));
                // bit (32 - amount) for 1..=32
                let idx = self.and(self.sub(self.c32(32), amount), self.c32(31));
                let cb = self.bit_of(v, idx);
                let le32 = self.icmp(LLVMIntULE, amount, self.c32(32));
                (res, self.select(le32, cb, f0))
            }
            1 => {
                let shifted = self.lshr(v, amt31);
                let res = self.select(lt32, shifted, self.c32(0));
                let cb = self.bit_of(v, amt_m1);
                let le32 = self.icmp(LLVMIntULE, amount, self.c32(32));
                let _ = eq32;
                (res, self.select(le32, cb, f0))
            }
            2 => {
                let shifted = self.ashr(v, amt31);
                let sign = self.ashr(v, self.c32(31));
                let res = self.select(lt32, shifted, sign);
                let cb = self.bit_of(v, amt_m1);
                let top = self.bit_of(v, self.c32(31));
                (res, self.select(lt32, cb, top))
            }
            _ => {
                let rot0 = self.icmp(LLVMIntEQ, amt31, self.c32(0));
                let rotated = self.call(self.t.fshr_fn, self.h.fshr, &mut [v, v, amt31]);
                let res = self.select(rot0, v, rotated);
                let cb = self.bit_of(v, self.and(self.sub(amt31, self.c32(1)), self.c32(31)));
                let top = self.bit_of(v, self.c32(31));
                (res, self.select(rot0, top, cb))
            }
        };
        (self.select(zero, v, res), self.select(zero, c, carry))
    }

    // ---- helpers with cycle sync

    fn sync_out(&self, f: &Fn) {
        let c = self.load(self.t.i64, f.cycles);
        self.store(c, self.field(f, 3));
    }
    fn sync_in(&self, f: &Fn) {
        let c = self.load(self.t.i64, self.field(f, 3));
        self.store(c, f.cycles);
        let ne = self.load(self.t.i64, self.field(f, 4));
        self.store(ne, f.next_event);
    }
    fn helper_load(
        &self,
        f: &Fn,
        helper: LLVMValueRef,
        addr: LLVMValueRef,
        openbus: u32,
    ) -> LLVMValueRef {
        self.sync_out(f);
        let v = self.call(
            self.t.load_fn,
            helper,
            &mut [f.ctx, addr, self.c32(openbus)],
        );
        self.sync_in(f);
        v
    }
    fn helper_read32(&self, f: &Fn, addr: LLVMValueRef, seq: bool, openbus: u32) -> LLVMValueRef {
        self.sync_out(f);
        let v = self.call(
            self.t.read_fn,
            self.h.read32,
            &mut [f.ctx, addr, self.c32(u32::from(seq)), self.c32(openbus)],
        );
        self.sync_in(f);
        v
    }
    fn helper_write(
        &self,
        f: &Fn,
        helper: LLVMValueRef,
        addr: LLVMValueRef,
        value: LLVMValueRef,
        seq: bool,
    ) {
        self.sync_out(f);
        let r = self.call(
            self.t.write_fn,
            helper,
            &mut [f.ctx, addr, value, self.c32(u32::from(seq))],
        );
        self.sync_in(f);
        let flag = self.icmp(LLVMIntNE, r, self.c32(0));
        let prev = self.flag(f.store_exit);
        self.store(self.or(prev, flag), f.store_exit);
    }

    // ---- exits

    fn exit(&self, f: &Fn, reason: u64, pc: LLVMValueRef) {
        self.store(pc, f.exit_pc);
        self.store(self.c32(reason as u32), f.exit_reason);
        self.br(f.exit_bb);
    }
    fn branch_exit(&self, f: &Fn, target: LLVMValueRef, thumb: bool) {
        let mask = if thumb { !1u32 } else { !3u32 };
        self.exit(f, EXIT_BRANCH, self.and(target, self.c32(mask)));
    }
    /// BX: set T from bit 0 and align accordingly.
    fn bx_exit(&self, f: &Fn, target: LLVMValueRef) {
        let t = self.and(target, self.c32(1));
        let rest = self.load(self.t.i32, f.cpsr_rest);
        let rest = self.or(self.and(rest, self.c32(!0x20)), self.shl(t, self.c32(5)));
        self.store(rest, f.cpsr_rest);
        let is_t = self.icmp(LLVMIntNE, t, self.c32(0));
        let aligned = self.select(
            is_t,
            self.and(target, self.c32(!1)),
            self.and(target, self.c32(!3)),
        );
        self.exit(f, EXIT_BRANCH, aligned);
    }
    /// After an instruction with stores: exit to the dispatcher if any store
    /// hit I/O (NB-62-02), else continue at `next`.
    fn after_stores(&self, f: &Fn, next_pc: u32, next: LLVMBasicBlockRef) {
        let flag = self.flag(f.store_exit);
        let out = self.block(f);
        self.cond_br(flag, out, next);
        self.at(out);
        self.exit(f, EXIT_SEQ, self.c32(next_pc));
    }

    fn condition(&self, f: &Fn, cond: u32) -> Option<LLVMValueRef> {
        let n = || self.flag(f.n);
        let z = || self.flag(f.z);
        let c = || self.flag(f.c);
        let v = || self.flag(f.v);
        let not = |x| self.xor(x, self.c1(true));
        Some(match cond {
            0x0 => z(),
            0x1 => not(z()),
            0x2 => c(),
            0x3 => not(c()),
            0x4 => n(),
            0x5 => not(n()),
            0x6 => v(),
            0x7 => not(v()),
            0x8 => self.and(c(), not(z())),
            0x9 => self.or(not(c()), z()),
            0xA => self.icmp(LLVMIntEQ, n(), v()),
            0xB => self.icmp(LLVMIntNE, n(), v()),
            0xC => self.and(not(z()), self.icmp(LLVMIntEQ, n(), v())),
            0xD => self.or(z(), self.icmp(LLVMIntNE, n(), v())),
            0xE => return None,
            _ => self.c1(false),
        })
    }

    fn multiply_cycles(&self, rs: LLVMValueRef, signed: bool) -> LLVMValueRef {
        let check = |mask: u32| {
            let v = self.and(rs, self.c32(mask));
            let zero = self.icmp(LLVMIntEQ, v, self.c32(0));
            if signed {
                let ones = self.icmp(LLVMIntEQ, v, self.c32(mask));
                self.or(zero, ones)
            } else {
                zero
            }
        };
        let c1 = check(0xFFFF_FF00);
        let c2 = check(0xFFFF_0000);
        let c3 = check(0xFF00_0000);
        let m = self.select(c3, self.c64(3), self.c64(4));
        let m = self.select(c2, self.c64(2), m);
        self.select(c1, self.c64(1), m)
    }
}

// ------------------------------------------------------------------ lowering

struct InstrCtx {
    pc: u32,
    op: u32,
    openbus: u32,
    next: LLVMBasicBlockRef,
    next_pc: u32,
}

impl Emitter {
    /// Lower one ARM instruction. The builder is positioned after fetch
    /// accounting; must end every path with a branch to `ic.next` or an exit.
    fn lower_arm(&self, f: &Fn, ic: &InstrCtx) {
        let op = ic.op;
        let cond = op >> 28;
        if let Some(c) = self.condition(f, cond) {
            let body = self.block(f);
            self.cond_br(c, body, ic.next);
            self.at(body);
        }
        let pc8 = ic.pc.wrapping_add(8);
        match (op >> 25) & 7 {
            0 => {
                if op & 0x0FFF_FFF0 == 0x012F_FF10 {
                    let t = self.read(f, op & 0xF, pc8);
                    self.bx_exit(f, t);
                } else if op & 0x0FC0_00F0 == 0x0000_0090 {
                    self.arm_multiply(f, ic);
                } else if op & 0x0F80_00F0 == 0x0080_0090 {
                    self.arm_multiply_long(f, ic);
                } else if op & 0x90 == 0x90 && (op >> 5) & 3 != 0 {
                    self.arm_halfword(f, ic);
                } else {
                    self.arm_data_processing(f, ic);
                }
            }
            1 => self.arm_data_processing(f, ic),
            2 | 3 => self.arm_single_transfer(f, ic),
            4 => self.arm_block_transfer(f, ic),
            _ => {
                // Branch (shells were filtered before lowering).
                let offset = (((op & 0x00FF_FFFF) << 8) as i32 >> 6) as u32;
                if op & (1 << 24) != 0 {
                    self.set_reg(f, 14, self.c32(ic.pc.wrapping_add(4)));
                }
                self.branch_exit(f, self.c32(pc8.wrapping_add(offset)), false);
            }
        }
    }

    fn arm_data_processing(&self, f: &Fn, ic: &InstrCtx) {
        let op = ic.op;
        let opcode = (op >> 21) & 0xF;
        let set = bit(op, 20);
        let rn = (op >> 16) & 0xF;
        let rd = (op >> 12) & 0xF;
        let pc8 = ic.pc.wrapping_add(8);
        let mut pc_extra = 0;
        let (operand, shifter_carry) = if bit(op, 25) {
            let rot = ((op >> 8) & 0xF) * 2;
            let value = (op & 0xFF).rotate_right(rot);
            let carry = if rot == 0 {
                self.flag(f.c)
            } else {
                self.c1(bit(value, 31))
            };
            (self.c32(value), carry)
        } else {
            let rm = op & 0xF;
            let ty = (op >> 5) & 3;
            if bit(op, 4) {
                self.idle(f, 1);
                pc_extra = 4;
                let rs = (op >> 8) & 0xF;
                let amount = self.and(self.read(f, rs, pc8), self.c32(0xFF));
                let value = self.read(f, rm, pc8.wrapping_add(4));
                self.shift_reg(f, ty, value, amount)
            } else {
                let amount = (op >> 7) & 0x1F;
                let value = self.read(f, rm, pc8);
                self.shift_imm(f, ty, value, amount)
            }
        };
        let a = self.read(f, rn, pc8.wrapping_add(pc_extra));
        let carry = self.flag(f.c);
        let flags = set && rd != 15;
        let logical = |r: LLVMValueRef| {
            if flags {
                self.set_nz(f, r);
                self.store(shifter_carry, f.c);
            }
            r
        };
        let t = self.c1(true);
        let fz = self.c1(false);
        let (result, write) = match opcode {
            0x0 => (logical(self.and(a, operand)), true),
            0x1 => (logical(self.xor(a, operand)), true),
            0x2 => (self.sub_flags(f, a, operand, t, flags), true),
            0x3 => (self.sub_flags(f, operand, a, t, flags), true),
            0x4 => (self.add_flags(f, a, operand, fz, flags), true),
            0x5 => (self.add_flags(f, a, operand, carry, flags), true),
            0x6 => (self.sub_flags(f, a, operand, carry, flags), true),
            0x7 => (self.sub_flags(f, operand, a, carry, flags), true),
            0x8 => (logical(self.and(a, operand)), false),
            0x9 => (logical(self.xor(a, operand)), false),
            0xA => (self.sub_flags(f, a, operand, t, flags), false),
            0xB => (self.add_flags(f, a, operand, fz, flags), false),
            0xC => (logical(self.or(a, operand)), true),
            0xD => (logical(operand), true),
            0xE => (logical(self.and(a, self.not(operand))), true),
            _ => (logical(self.not(operand)), true),
        };
        if rd == 15 {
            if write {
                self.branch_exit(f, result, false);
            } else {
                self.br(ic.next);
            }
        } else {
            if write {
                self.set_reg(f, rd, result);
            }
            self.br(ic.next);
        }
    }

    fn arm_multiply(&self, f: &Fn, ic: &InstrCtx) {
        let op = ic.op;
        let rd = (op >> 16) & 0xF;
        let rn = (op >> 12) & 0xF;
        let rs = (op >> 8) & 0xF;
        let rm = op & 0xF;
        let pc8 = ic.pc.wrapping_add(8);
        let vs = self.read(f, rs, pc8);
        let mut result = self.mul(self.read(f, rm, pc8), vs);
        let mut cycles = self.multiply_cycles(vs, true);
        if bit(op, 21) {
            result = self.add(result, self.read(f, rn, pc8));
            cycles = self.add(cycles, self.c64(1));
        }
        self.add_cycles(f, cycles);
        if rd != 15 {
            self.set_reg(f, rd, result);
        }
        if bit(op, 20) {
            self.set_nz(f, result);
        }
        self.br(ic.next);
    }

    fn arm_multiply_long(&self, f: &Fn, ic: &InstrCtx) {
        let op = ic.op;
        let rd_hi = (op >> 16) & 0xF;
        let rd_lo = (op >> 12) & 0xF;
        let rs = (op >> 8) & 0xF;
        let rm = op & 0xF;
        let pc8 = ic.pc.wrapping_add(8);
        let signed = bit(op, 22);
        let vs = self.read(f, rs, pc8);
        let vm = self.read(f, rm, pc8);
        let (a, b) = if signed {
            (self.sext(vm, self.t.i64), self.sext(vs, self.t.i64))
        } else {
            (self.zext(vm, self.t.i64), self.zext(vs, self.t.i64))
        };
        let mut result = self.mul(a, b);
        let mut cycles = self.add(self.multiply_cycles(vs, signed), self.c64(1));
        if bit(op, 21) {
            let hi = self.zext(self.read(f, rd_hi, pc8), self.t.i64);
            let lo = self.zext(self.read(f, rd_lo, pc8), self.t.i64);
            let acc = self.or(self.shl(hi, self.c64(32)), lo);
            result = self.add(result, acc);
            cycles = self.add(cycles, self.c64(1));
        }
        self.add_cycles(f, cycles);
        let lo = self.trunc(result, self.t.i32);
        let hi = self.trunc(self.lshr(result, self.c64(32)), self.t.i32);
        // Interpreter order: lo then hi (hi wins if equal registers).
        let lo_target = rd_lo;
        let hi_target = rd_hi;
        if lo_target != 15 {
            self.set_reg(f, lo_target, lo);
        }
        if hi_target != 15 {
            self.set_reg(f, hi_target, hi);
        }
        if bit(op, 20) {
            let n = self.icmp(LLVMIntSLT, hi, self.c32(0));
            let z = self.icmp(LLVMIntEQ, result, self.c64(0));
            self.store(n, f.n);
            self.store(z, f.z);
        }
        self.br(ic.next);
    }

    fn arm_single_transfer(&self, f: &Fn, ic: &InstrCtx) {
        let op = ic.op;
        let pre = bit(op, 24);
        let up = bit(op, 23);
        let byte = bit(op, 22);
        let writeback = bit(op, 21) || !pre;
        let load = bit(op, 20);
        let rn = (op >> 16) & 0xF;
        let rd = (op >> 12) & 0xF;
        let pc8 = ic.pc.wrapping_add(8);
        let offset = if bit(op, 25) {
            let rm = op & 0xF;
            let ty = (op >> 5) & 3;
            let amount = (op >> 7) & 0x1F;
            let v = self.read(f, rm, pc8);
            self.shift_imm(f, ty, v, amount).0
        } else {
            self.c32(op & 0xFFF)
        };
        let base = self.read(f, rn, pc8);
        let offset_base = if up {
            self.add(base, offset)
        } else {
            self.sub(base, offset)
        };
        let addr = if pre { offset_base } else { base };
        if load {
            let helper = if byte { self.h.ldrb } else { self.h.ldr };
            let value = self.helper_load(f, helper, addr, ic.openbus);
            self.idle(f, 1);
            if writeback && rn != rd {
                self.set_reg(f, rn, offset_base);
            }
            self.store(self.c1(false), f.seq);
            if rd == 15 {
                self.branch_exit(f, value, false);
            } else {
                self.set_reg(f, rd, value);
                self.br(ic.next);
            }
        } else {
            let value = if rd == 15 {
                self.c32(pc8.wrapping_add(4))
            } else {
                self.reg(f, rd)
            };
            if byte {
                self.helper_write(f, self.h.write8, addr, value, false);
            } else {
                let aligned = self.and(addr, self.c32(!3));
                self.helper_write(f, self.h.write32, aligned, value, false);
            }
            if writeback {
                self.set_reg(f, rn, offset_base);
            }
            self.store(self.c1(false), f.seq);
            self.after_stores(f, ic.next_pc, ic.next);
        }
    }

    fn arm_halfword(&self, f: &Fn, ic: &InstrCtx) {
        let op = ic.op;
        let pre = bit(op, 24);
        let up = bit(op, 23);
        let writeback = bit(op, 21) || !pre;
        let load = bit(op, 20);
        let rn = (op >> 16) & 0xF;
        let rd = (op >> 12) & 0xF;
        let sh = (op >> 5) & 3;
        let pc8 = ic.pc.wrapping_add(8);
        let offset = if bit(op, 22) {
            self.c32(((op >> 4) & 0xF0) | (op & 0xF))
        } else {
            self.read(f, op & 0xF, pc8)
        };
        let base = self.read(f, rn, pc8);
        let offset_base = if up {
            self.add(base, offset)
        } else {
            self.sub(base, offset)
        };
        let addr = if pre { offset_base } else { base };
        if load {
            let helper = match sh {
                1 => self.h.ldrh,
                2 => self.h.ldrsb,
                _ => self.h.ldrsh,
            };
            let value = self.helper_load(f, helper, addr, ic.openbus);
            self.idle(f, 1);
            if writeback && rn != rd {
                self.set_reg(f, rn, offset_base);
            }
            self.store(self.c1(false), f.seq);
            if rd == 15 {
                self.branch_exit(f, value, false);
            } else {
                self.set_reg(f, rd, value);
                self.br(ic.next);
            }
        } else {
            let value = if rd == 15 {
                self.c32(pc8.wrapping_add(4))
            } else {
                self.reg(f, rd)
            };
            let aligned = self.and(addr, self.c32(!1));
            self.helper_write(f, self.h.write16, aligned, value, false);
            if writeback {
                self.set_reg(f, rn, offset_base);
            }
            self.store(self.c1(false), f.seq);
            self.after_stores(f, ic.next_pc, ic.next);
        }
    }

    fn arm_block_transfer(&self, f: &Fn, ic: &InstrCtx) {
        let op = ic.op;
        let rn = (op >> 16) & 0xF;
        let base = self.reg(f, rn);
        self.block_transfer(
            f,
            ic,
            rn,
            op & 0xFFFF,
            base,
            bit(op, 24),
            bit(op, 23),
            bit(op, 21),
            bit(op, 20),
            false,
        );
    }

    /// Mirrors `Cpu::block_transfer` for a non-empty list without `^`.
    #[allow(clippy::too_many_arguments)]
    fn block_transfer(
        &self,
        f: &Fn,
        ic: &InstrCtx,
        rn: u32,
        list: u32,
        base: LLVMValueRef,
        pre: bool,
        up: bool,
        writeback: bool,
        load: bool,
        thumb: bool,
    ) {
        let count = list.count_ones();
        let size = self.c32(count * 4);
        let (start, final_base) = if up {
            (
                if pre {
                    self.add(base, self.c32(4))
                } else {
                    base
                },
                self.add(base, size),
            )
        } else {
            let low = self.sub(base, size);
            (if pre { low } else { self.add(low, self.c32(4)) }, low)
        };
        let first = list.trailing_zeros();
        let mut addr = start;
        let mut seq = false;
        let mut pc_value = None;
        if load {
            if writeback {
                self.set_reg(f, rn, final_base);
            }
            for reg in 0..16 {
                if list & (1 << reg) == 0 {
                    continue;
                }
                let aligned = self.and(addr, self.c32(!3));
                let v = self.helper_read32(f, aligned, seq, ic.openbus);
                seq = true;
                addr = self.add(addr, self.c32(4));
                if reg == 15 {
                    pc_value = Some(v);
                } else {
                    self.set_reg(f, reg, v);
                }
            }
            self.idle(f, 1);
            self.store(self.c1(false), f.seq);
            match pc_value {
                Some(v) => self.branch_exit(f, v, thumb),
                None => self.br(ic.next),
            }
        } else {
            for reg in 0..16 {
                if list & (1 << reg) == 0 {
                    continue;
                }
                let mut value = if reg == 15 {
                    let pipeline = if thumb {
                        ic.pc.wrapping_add(4)
                    } else {
                        ic.pc.wrapping_add(8)
                    };
                    self.c32(pipeline.wrapping_add(if thumb { 2 } else { 4 }))
                } else {
                    self.reg(f, reg)
                };
                if reg == rn && reg != first && writeback {
                    value = final_base;
                }
                let aligned = self.and(addr, self.c32(!3));
                self.helper_write(f, self.h.write32, aligned, value, seq);
                seq = true;
                addr = self.add(addr, self.c32(4));
            }
            if writeback {
                self.set_reg(f, rn, final_base);
            }
            self.store(self.c1(false), f.seq);
            self.after_stores(f, ic.next_pc, ic.next);
        }
    }

    /// Lower one Thumb instruction (mirrors `Cpu::exec_thumb`).
    fn lower_thumb(&self, f: &Fn, ic: &InstrCtx) {
        let op = ic.op;
        let pc4 = ic.pc.wrapping_add(4);
        match op >> 13 {
            0 => {
                if (op >> 11) & 3 == 3 {
                    let rn_or_imm = (op >> 6) & 7;
                    let rs = (op >> 3) & 7;
                    let rd = op & 7;
                    let operand = if op & (1 << 10) != 0 {
                        self.c32(rn_or_imm)
                    } else {
                        self.reg(f, rn_or_imm)
                    };
                    let a = self.reg(f, rs);
                    let r = if op & (1 << 9) != 0 {
                        self.sub_flags(f, a, operand, self.c1(true), true)
                    } else {
                        self.add_flags(f, a, operand, self.c1(false), true)
                    };
                    self.set_reg(f, rd, r);
                } else {
                    let ty = (op >> 11) & 3;
                    let amount = (op >> 6) & 0x1F;
                    let rs = (op >> 3) & 7;
                    let rd = op & 7;
                    let v = self.reg(f, rs);
                    let (r, c) = self.shift_imm(f, ty, v, amount);
                    self.set_reg(f, rd, r);
                    self.set_nz(f, r);
                    self.store(c, f.c);
                }
                self.br(ic.next);
            }
            1 => {
                let rd = (op >> 8) & 7;
                let imm = self.c32(op & 0xFF);
                match (op >> 11) & 3 {
                    0 => {
                        self.set_reg(f, rd, imm);
                        self.set_nz(f, imm);
                    }
                    1 => {
                        let a = self.reg(f, rd);
                        self.sub_flags(f, a, imm, self.c1(true), true);
                    }
                    2 => {
                        let a = self.reg(f, rd);
                        let r = self.add_flags(f, a, imm, self.c1(false), true);
                        self.set_reg(f, rd, r);
                    }
                    _ => {
                        let a = self.reg(f, rd);
                        let r = self.sub_flags(f, a, imm, self.c1(true), true);
                        self.set_reg(f, rd, r);
                    }
                }
                self.br(ic.next);
            }
            2 => {
                if op >> 10 == 0b010000 {
                    self.thumb_alu(f, ic);
                } else if op >> 10 == 0b010001 {
                    self.thumb_hi_reg(f, ic);
                } else if op >> 11 == 0b01001 {
                    let rd = (op >> 8) & 7;
                    let addr = (pc4 & !2).wrapping_add((op & 0xFF) << 2);
                    let v = self.helper_load(f, self.h.ldr, self.c32(addr), ic.openbus);
                    self.set_reg(f, rd, v);
                    self.idle(f, 1);
                    self.store(self.c1(false), f.seq);
                    self.br(ic.next);
                } else {
                    self.thumb_load_store_reg(f, ic);
                }
            }
            3 => {
                let byte = op & (1 << 12) != 0;
                let load = op & (1 << 11) != 0;
                let offset = (op >> 6) & 0x1F;
                let rb = (op >> 3) & 7;
                let rd = op & 7;
                let addr = self.add(
                    self.reg(f, rb),
                    self.c32(if byte { offset } else { offset << 2 }),
                );
                self.mem_rd(f, ic, rd, addr, load, if byte { 8 } else { 32 });
            }
            4 => {
                if op & (1 << 12) == 0 {
                    let offset = ((op >> 6) & 0x1F) << 1;
                    let rb = (op >> 3) & 7;
                    let rd = op & 7;
                    let addr = self.add(self.reg(f, rb), self.c32(offset));
                    self.mem_rd(f, ic, rd, addr, op & (1 << 11) != 0, 16);
                } else {
                    let rd = (op >> 8) & 7;
                    let addr = self.add(self.reg(f, 13), self.c32((op & 0xFF) << 2));
                    self.mem_rd(f, ic, rd, addr, op & (1 << 11) != 0, 32);
                }
            }
            5 => {
                if op & (1 << 12) == 0 {
                    let rd = (op >> 8) & 7;
                    let imm = (op & 0xFF) << 2;
                    let v = if op & (1 << 11) != 0 {
                        self.add(self.reg(f, 13), self.c32(imm))
                    } else {
                        self.c32((pc4 & !2).wrapping_add(imm))
                    };
                    self.set_reg(f, rd, v);
                    self.br(ic.next);
                } else if (op >> 8) & 0xF == 0 {
                    let imm = self.c32((op & 0x7F) << 2);
                    let sp = self.reg(f, 13);
                    let v = if op & 0x80 != 0 {
                        self.sub(sp, imm)
                    } else {
                        self.add(sp, imm)
                    };
                    self.set_reg(f, 13, v);
                    self.br(ic.next);
                } else {
                    // PUSH / POP.
                    let load = op & (1 << 11) != 0;
                    let mut list = op & 0xFF;
                    if op & (1 << 8) != 0 {
                        list |= if load { 1 << 15 } else { 1 << 14 };
                    }
                    let sp = self.reg(f, 13);
                    if load {
                        self.block_transfer(f, ic, 13, list, sp, false, true, true, true, true);
                    } else {
                        self.block_transfer(f, ic, 13, list, sp, true, false, true, false, true);
                    }
                }
            }
            6 => {
                if op & (1 << 12) == 0 {
                    let rb = (op >> 8) & 7;
                    let list = op & 0xFF;
                    let load = op & (1 << 11) != 0;
                    let base = self.reg(f, rb);
                    let writeback = !(load && list & (1 << rb) != 0);
                    self.block_transfer(f, ic, rb, list, base, false, true, writeback, load, true);
                } else {
                    let cond = (op >> 8) & 0xF;
                    let c = self.condition(f, cond).expect("cond < 0xE");
                    let taken = self.block(f);
                    self.cond_br(c, taken, ic.next);
                    self.at(taken);
                    let offset = ((op & 0xFF) as i8 as i32 as u32) << 1;
                    self.branch_exit(f, self.c32(pc4.wrapping_add(offset)), true);
                }
            }
            _ => match (op >> 11) & 3 {
                0 => {
                    let offset = (((op & 0x7FF) << 21) as i32 >> 20) as u32;
                    self.branch_exit(f, self.c32(pc4.wrapping_add(offset)), true);
                }
                2 => {
                    let offset = (((op & 0x7FF) << 21) as i32 >> 9) as u32;
                    self.set_reg(f, 14, self.c32(pc4.wrapping_add(offset)));
                    self.br(ic.next);
                }
                _ => {
                    let lr = self.reg(f, 14);
                    let target = self.add(lr, self.c32((op & 0x7FF) << 1));
                    self.set_reg(f, 14, self.c32(ic.pc.wrapping_add(2) | 1));
                    self.branch_exit(f, target, true);
                }
            },
        }
    }

    /// Thumb single load/store of `width` bits with Rd/address already known.
    fn mem_rd(&self, f: &Fn, ic: &InstrCtx, rd: u32, addr: LLVMValueRef, load: bool, width: u32) {
        if load {
            let helper = match width {
                8 => self.h.ldrb,
                16 => self.h.ldrh,
                _ => self.h.ldr,
            };
            let v = self.helper_load(f, helper, addr, ic.openbus);
            self.set_reg(f, rd, v);
            self.idle(f, 1);
            self.store(self.c1(false), f.seq);
            self.br(ic.next);
        } else {
            let v = self.reg(f, rd);
            match width {
                8 => self.helper_write(f, self.h.write8, addr, v, false),
                16 => {
                    let a = self.and(addr, self.c32(!1));
                    self.helper_write(f, self.h.write16, a, v, false);
                }
                _ => {
                    let a = self.and(addr, self.c32(!3));
                    self.helper_write(f, self.h.write32, a, v, false);
                }
            }
            self.store(self.c1(false), f.seq);
            self.after_stores(f, ic.next_pc, ic.next);
        }
    }

    fn thumb_load_store_reg(&self, f: &Fn, ic: &InstrCtx) {
        let op = ic.op;
        let ro = (op >> 6) & 7;
        let rb = (op >> 3) & 7;
        let rd = op & 7;
        let addr = self.add(self.reg(f, rb), self.reg(f, ro));
        match (op >> 9) & 7 {
            0 => self.mem_rd(f, ic, rd, addr, false, 32),
            1 => self.mem_rd(f, ic, rd, addr, false, 16),
            2 => self.mem_rd(f, ic, rd, addr, false, 8),
            3 => self.thumb_signed_load(f, ic, rd, addr, self.h.ldrsb),
            4 => self.mem_rd(f, ic, rd, addr, true, 32),
            5 => self.mem_rd(f, ic, rd, addr, true, 16),
            6 => self.mem_rd(f, ic, rd, addr, true, 8),
            _ => self.thumb_signed_load(f, ic, rd, addr, self.h.ldrsh),
        }
    }

    fn thumb_signed_load(
        &self,
        f: &Fn,
        ic: &InstrCtx,
        rd: u32,
        addr: LLVMValueRef,
        helper: LLVMValueRef,
    ) {
        let v = self.helper_load(f, helper, addr, ic.openbus);
        self.set_reg(f, rd, v);
        self.idle(f, 1);
        self.store(self.c1(false), f.seq);
        self.br(ic.next);
    }

    fn thumb_alu(&self, f: &Fn, ic: &InstrCtx) {
        let op = ic.op;
        let rs = (op >> 3) & 7;
        let rd = op & 7;
        let a = self.reg(f, rd);
        let b = self.reg(f, rs);
        let c = self.flag(f.c);
        let logical = |v: LLVMValueRef| {
            self.set_nz(f, v);
            v
        };
        let shift = |ty: u32| {
            self.idle(f, 1);
            let amount = self.and(b, self.c32(0xFF));
            let (v, carry) = self.shift_reg(f, ty, a, amount);
            self.set_nz(f, v);
            self.store(carry, f.c);
            v
        };
        let t = self.c1(true);
        let (result, write) = match (op >> 6) & 0xF {
            0x0 => (logical(self.and(a, b)), true),
            0x1 => (logical(self.xor(a, b)), true),
            0x2 => (shift(0), true),
            0x3 => (shift(1), true),
            0x4 => (shift(2), true),
            0x5 => (self.add_flags(f, a, b, c, true), true),
            0x6 => (self.sub_flags(f, a, b, c, true), true),
            0x7 => (shift(3), true),
            0x8 => (logical(self.and(a, b)), false),
            0x9 => (self.sub_flags(f, self.c32(0), b, t, true), true),
            0xA => (self.sub_flags(f, a, b, t, true), false),
            0xB => (self.add_flags(f, a, b, self.c1(false), true), false),
            0xC => (logical(self.or(a, b)), true),
            0xD => {
                let cycles = self.multiply_cycles(a, true);
                self.add_cycles(f, cycles);
                (logical(self.mul(a, b)), true)
            }
            0xE => (logical(self.and(a, self.not(b))), true),
            _ => (logical(self.not(b)), true),
        };
        if write {
            self.set_reg(f, rd, result);
        }
        self.br(ic.next);
    }

    fn thumb_hi_reg(&self, f: &Fn, ic: &InstrCtx) {
        let op = ic.op;
        let rs = ((op >> 3) & 7) | ((op >> 3) & 8);
        let rd = (op & 7) | ((op >> 4) & 8);
        let pc4 = ic.pc.wrapping_add(4);
        let value = self.read(f, rs, pc4);
        match (op >> 8) & 3 {
            0 => {
                let r = self.add(self.read(f, rd, pc4), value);
                if rd == 15 {
                    self.branch_exit(f, r, true);
                } else {
                    self.set_reg(f, rd, r);
                    self.br(ic.next);
                }
            }
            1 => {
                let a = self.read(f, rd, pc4);
                self.sub_flags(f, a, value, self.c1(true), true);
                self.br(ic.next);
            }
            2 => {
                if rd == 15 {
                    self.branch_exit(f, value, true);
                } else {
                    self.set_reg(f, rd, value);
                    self.br(ic.next);
                }
            }
            _ => self.bx_exit(f, value),
        }
    }
}

// ------------------------------------------------------------------ module

fn emit_run(e: &Emitter, run: &Run, name: &str) -> LLVMValueRef {
    let cname = CString::new(name).expect("run name has no NUL");
    let func = unsafe { LLVMAddFunction(e.module, cname.as_ptr(), e.t.run_fn) };
    // External: the table object references runs across shard objects.
    let ctx = unsafe { LLVMGetParam(func, 0) };
    let entry_param = unsafe { LLVMGetParam(func, 1) };
    let entry_bb = unsafe { LLVMAppendBasicBlockInContext(e.context, func, EMPTY) };
    e.at(entry_bb);
    let alloca = |ty| unsafe { LLVMBuildAlloca(e.b, ty, EMPTY) };
    let mut regs = [ptr::null_mut(); 15];
    for r in regs.iter_mut() {
        *r = alloca(e.t.i32);
    }
    let mut f = Fn {
        func,
        ctx,
        regs,
        n: alloca(e.t.i1),
        z: alloca(e.t.i1),
        c: alloca(e.t.i1),
        v: alloca(e.t.i1),
        seq: alloca(e.t.i1),
        cycles: alloca(e.t.i64),
        next_event: alloca(e.t.i64),
        retired: alloca(e.t.i64),
        lastop: alloca(e.t.i32),
        cpsr_rest: alloca(e.t.i32),
        wait: ptr::null_mut(),
        exit_pc: alloca(e.t.i32),
        exit_reason: alloca(e.t.i32),
        exit_bb: ptr::null_mut(),
        store_exit: alloca(e.t.i1),
    };
    // Load guest state.
    for r in 0..15 {
        let p = e.reg_ptr_in_ctx(&f, r);
        let v = e.load(e.t.i32, p);
        e.store(v, f.regs[r as usize]);
    }
    let cpsr = e.load(e.t.i32, e.field(&f, 1));
    let flag = |shift: u32| e.trunc(e.lshr(cpsr, e.c32(shift)), e.t.i1);
    e.store(flag(31), f.n);
    e.store(flag(30), f.z);
    e.store(flag(29), f.c);
    e.store(flag(28), f.v);
    e.store(e.and(cpsr, e.c32(0x0FFF_FFFF)), f.cpsr_rest);
    let seq = e.load(e.t.i32, e.field(&f, 2));
    e.store(e.icmp(LLVMIntNE, seq, e.c32(0)), f.seq);
    let cyc = e.load(e.t.i64, e.field(&f, 3));
    e.store(cyc, f.cycles);
    let ne = e.load(e.t.i64, e.field(&f, 4));
    e.store(ne, f.next_event);
    e.store(e.c64(0), f.retired);
    let lastop = e.load(e.t.i32, e.field(&f, 6));
    e.store(lastop, f.lastop);
    f.wait = e.load(e.t.ptr, e.field(&f, 8));

    let thumb = run.entry.thumb;
    let size: u32 = if thumb { 2 } else { 4 };
    let n = run.ops.len();
    // Labels: chk_k (event check) and body_k for each instruction, plus end.
    let checks: Vec<LLVMBasicBlockRef> = (0..=n).map(|_| e.block(&f)).collect();
    let bodies: Vec<LLVMBasicBlockRef> = (0..n).map(|_| e.block(&f)).collect();
    let exit_bb = e.block(&f);
    f.exit_bb = exit_bb;
    let default_bb = e.block(&f);

    // Dispatch on the resume index.
    e.at(entry_bb);
    let switch = unsafe { LLVMBuildSwitch(e.b, entry_param, default_bb, n as u32) };
    for (k, body) in bodies.iter().enumerate() {
        unsafe { LLVMAddCase(switch, e.c32(k as u32), *body) };
    }
    e.at(default_bb);
    e.exit(&f, EXIT_INTERPRET, e.c32(run.entry.pc));

    let region = ((run.entry.pc >> 24) & 0xF) as u64;
    let rom = (0x08..=0x0D).contains(&region);
    for k in 0..n {
        let pc = run.entry.pc.wrapping_add(k as u32 * size);
        let op = run.ops[k];
        // Event check (not for the first instruction of a call: the
        // dispatcher just serviced events).
        e.at(checks[k]);
        let cyc = e.load(e.t.i64, f.cycles);
        let ne = e.load(e.t.i64, f.next_event);
        let due = e.icmp(LLVMIntUGE, cyc, ne);
        let out = e.block(&f);
        e.cond_br(due, out, bodies[k]);
        e.at(out);
        e.exit(&f, EXIT_SEQ, e.c32(pc));

        e.at(bodies[k]);
        let shell = if thumb {
            thumb_is_shell(op)
        } else {
            arm_is_shell(op)
        };
        if shell {
            e.exit(&f, EXIT_INTERPRET, e.c32(pc));
            continue;
        }
        let openbus = if thumb { op * 0x0001_0001 } else { op };
        e.store(e.c32(openbus), f.lastop);
        e.store(e.c1(false), f.store_exit);
        // Fetch cost (mirrors Bus::fetch16/32 with the current seq flag).
        let seq = e.flag(f.seq);
        let table = |base: u64| {
            let mut idx = [e.c64(base + region)];
            let p = unsafe {
                LLVMBuildInBoundsGEP2(
                    e.b,
                    LLVMInt8TypeInContext(e.context),
                    f.wait,
                    idx.as_mut_ptr(),
                    1,
                    EMPTY,
                )
            };
            let v = e.load(unsafe { LLVMInt8TypeInContext(e.context) }, p);
            e.zext(v, e.t.i64)
        };
        let (nb, sb) = if thumb { (0, 16) } else { (32, 48) };
        let n_cost = table(nb);
        let s_cost = table(sb);
        let s_eff = if rom {
            let mut idx = [e.c64(64)];
            let p = unsafe {
                LLVMBuildInBoundsGEP2(
                    e.b,
                    LLVMInt8TypeInContext(e.context),
                    f.wait,
                    idx.as_mut_ptr(),
                    1,
                    EMPTY,
                )
            };
            let pf = e.load(unsafe { LLVMInt8TypeInContext(e.context) }, p);
            let on = e.icmp(LLVMIntNE, pf, unsafe {
                LLVMConstInt(LLVMInt8TypeInContext(e.context), 0, 0)
            });
            e.select(on, e.c64(1), s_cost)
        } else {
            s_cost
        };
        let cost = e.select(seq, s_eff, n_cost);
        e.add_cycles(&f, cost);
        e.store(e.c1(true), f.seq);
        let r = e.load(e.t.i64, f.retired);
        e.store(e.add(r, e.c64(1)), f.retired);
        let ic = InstrCtx {
            pc,
            op,
            openbus,
            next: checks[k + 1],
            next_pc: pc.wrapping_add(size),
        };
        if thumb {
            // Thumb conditional branches with cond 0xE/0xF are shells above.
            e.lower_thumb(&f, &ic);
        } else {
            e.lower_arm(&f, &ic);
        }
    }
    // Falling off the run: continue at the tail address.
    e.at(checks[n]);
    let tail = run
        .tail
        .unwrap_or(run.entry.pc.wrapping_add(n as u32 * size));
    e.exit(&f, EXIT_SEQ, e.c32(tail));

    // Common exit: write state back.
    e.at(exit_bb);
    for r in 0..15 {
        let v = e.load(e.t.i32, f.regs[r as usize]);
        let p = e.reg_ptr_in_ctx(&f, r);
        e.store(v, p);
    }
    let pc = e.load(e.t.i32, f.exit_pc);
    e.store(pc, e.reg_ptr_in_ctx(&f, 15));
    let rest = e.load(e.t.i32, f.cpsr_rest);
    let put = |p: LLVMValueRef, shift: u32| e.shl(e.zext(e.flag(p), e.t.i32), e.c32(shift));
    let cpsr = e.or(
        e.or(rest, put(f.n, 31)),
        e.or(put(f.z, 30), e.or(put(f.c, 29), put(f.v, 28))),
    );
    e.store(cpsr, e.field(&f, 1));
    let seq = e.zext(e.flag(f.seq), e.t.i32);
    e.store(seq, e.field(&f, 2));
    let cyc = e.load(e.t.i64, f.cycles);
    e.store(cyc, e.field(&f, 3));
    let ret = e.load(e.t.i64, f.retired);
    e.store(ret, e.field(&f, 5));
    let lop = e.load(e.t.i32, f.lastop);
    e.store(lop, e.field(&f, 6));
    let reason = e.load(e.t.i32, f.exit_reason);
    unsafe { LLVMBuildRet(e.b, reason) };
    func
}

/// Emit native runs for `entries` as several objects compiled in parallel
/// (one LLVM context per shard) plus one object with the lookup table and
/// `main`, which hands control to `gba_native_main` in `gba-native-rt`.
#[allow(clippy::too_many_arguments)]
pub fn emit_native_objects(
    rom: &[u8],
    rom_sha256: [u8; 32],
    entries: &[Entry],
    ram_entries: &[RamEntry],
    follow_direct_branches: bool,
    embed_rom: bool,
    out_dir: &Path,
    optimization: Optimization,
    threads: usize,
    target: &TargetSpec,
) -> Result<(Vec<std::path::PathBuf>, NativeEmitResult), String> {
    let mut stats = NativeStats {
        entries_in: entries.len(),
        ..NativeStats::default()
    };
    let mut runs = build_runs(rom, entries, follow_direct_branches, &mut stats);
    let mut guards = Vec::new();
    let ram_runs = build_ram_runs(ram_entries, &mut guards);
    stats.ram_runs = ram_runs.len();
    stats.ram_instructions = ram_runs.iter().map(|r| r.ops.len()).sum();
    runs.extend(ram_runs);
    stats.runs = runs.len();
    stats.instructions = runs.iter().map(|r| r.ops.len()).sum();
    stats.shells = runs
        .iter()
        .map(|r| {
            r.ops
                .iter()
                .filter(|&&op| {
                    if r.entry.thumb {
                        thumb_is_shell(op)
                    } else {
                        arm_is_shell(op)
                    }
                })
                .count()
        })
        .sum();
    if runs.is_empty() {
        return Err("no ROM entry points to compile".to_owned());
    }
    // SAFETY: backend initialisation happens once, before worker threads.
    unsafe { initialize_backend(target.backend) }?;

    // Balance shards by instruction count.
    let threads = threads.clamp(1, 64).min(runs.len());
    let mut shards: Vec<Vec<&Run>> = vec![Vec::new(); threads];
    let mut load = vec![0usize; threads];
    let mut order: Vec<&Run> = runs.iter().collect();
    order.sort_by_key(|r| std::cmp::Reverse(r.ops.len()));
    for run in order {
        let i = (0..threads).min_by_key(|&i| load[i]).expect("threads > 0");
        load[i] += run.ops.len();
        shards[i].push(run);
    }
    let mut outputs: Vec<std::path::PathBuf> = (0..threads)
        .map(|i| out_dir.join(format!("runs{i}.o")))
        .collect();
    let table_out = out_dir.join("table.o");
    let results: Vec<Result<EmitInfo, String>> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for (shard, out) in shards.iter().zip(&outputs) {
            handles.push(scope.spawn(move || {
                // SAFETY: each thread owns its LLVM context and module.
                unsafe {
                    with_module(out, optimization, target, |e| {
                        for run in shard {
                            emit_run(e, run, &run_name(run));
                        }
                        Ok(())
                    })
                }
            }));
        }
        let table = scope.spawn(|| {
            // SAFETY: as above.
            unsafe {
                with_module(&table_out, optimization, target, |e| {
                    build_table(
                        e,
                        rom_sha256,
                        &runs,
                        &guards,
                        embed_rom.then_some(rom),
                        target.entry_symbol(),
                    )
                })
            }
        });
        let mut r: Vec<_> = handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("LLVM worker panicked".into()))
            })
            .collect();
        r.push(
            table
                .join()
                .unwrap_or_else(|_| Err("LLVM worker panicked".into())),
        );
        r
    });
    let mut info = None;
    for r in results {
        info = Some(r?);
    }
    outputs.push(table_out);
    Ok((
        outputs,
        NativeEmitResult {
            info: info.expect("at least one module"),
            stats,
        },
    ))
}

fn run_name(run: &Run) -> String {
    let mode = if run.entry.thumb { 't' } else { 'a' };
    match run.guard {
        None => format!("gba_run_{mode}_{:08x}", run.entry.pc),
        Some(g) => format!("gba_run_{mode}_{:08x}_g{g}", run.entry.pc),
    }
}

/// Create a context/module/target for one object, let `build` fill it, then
/// verify, optimise and emit.
unsafe fn with_module(
    output: &Path,
    optimization: Optimization,
    spec: &TargetSpec,
    build: impl FnOnce(&Emitter) -> Result<(), String>,
) -> Result<EmitInfo, String> {
    let triple = CString::new(spec.llvm_triple()).expect("triple has no NUL");
    let cpu = CString::new(spec.cpu.as_str()).expect("cpu has no NUL");
    let features = CString::new(spec.features.as_str()).expect("features have no NUL");
    let reloc = if spec.backend == "webassembly" {
        LLVMRelocMode::LLVMRelocDefault
    } else {
        LLVMRelocMode::LLVMRelocPIC
    };
    let level = match optimization {
        Optimization::O0 => LLVMCodeGenOptLevel::LLVMCodeGenLevelNone,
        Optimization::O2 => LLVMCodeGenOptLevel::LLVMCodeGenLevelDefault,
    };
    let mut llvm_target = ptr::null_mut();
    let mut message = ptr::null_mut();
    if unsafe { LLVMGetTargetFromTriple(triple.as_ptr(), &mut llvm_target, &mut message) } != 0 {
        return Err(unsafe { take_llvm_message(message) });
    }
    let machine = unsafe {
        LLVMCreateTargetMachine(
            llvm_target,
            triple.as_ptr(),
            cpu.as_ptr(),
            features.as_ptr(),
            level,
            reloc,
            LLVMCodeModel::LLVMCodeModelSmall,
        )
    };
    if machine.is_null() {
        return Err("LLVM could not create a TargetMachine".to_owned());
    }
    let context = unsafe { LLVMContextCreate() };
    let module = unsafe { LLVMModuleCreateWithNameInContext(c"gba_native".as_ptr(), context) };
    let builder = unsafe { LLVMCreateBuilderInContext(context) };
    let target_data = unsafe { LLVMCreateTargetDataLayout(machine) };
    let layout_raw = unsafe { LLVMCopyStringRepOfTargetData(target_data) };
    let data_layout = unsafe { std::ffi::CStr::from_ptr(layout_raw) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        LLVMSetTarget(module, triple.as_ptr());
        LLVMSetDataLayout(module, layout_raw);
        LLVMDisposeMessage(layout_raw);
    }
    let emitter = unsafe { make_emitter(context, module, builder) };
    let result = build(&emitter)
        .and_then(|()| unsafe { verify_module(module, "before optimization") })
        .and_then(|()| unsafe { run_optimization(module, machine, optimization) })
        .and_then(|()| unsafe { verify_module(module, "after optimization") })
        .and_then(|()| unsafe { emit_object(machine, module, output) });
    unsafe {
        LLVMDisposeTargetData(target_data);
        LLVMDisposeBuilder(builder);
        LLVMDisposeModule(module);
        LLVMContextDispose(context);
        LLVMDisposeTargetMachine(machine);
    }
    result.map(|()| EmitInfo {
        llvm_version: env!("GBA_LLVM_VERSION"),
        triple: Box::leak(spec.triple.clone().into_boxed_str()),
        cpu: Box::leak(spec.cpu.clone().into_boxed_str()),
        object_format: match spec.backend {
            "webassembly" => "WebAssembly",
            _ if spec.triple.contains("apple") => "Mach-O",
            _ if spec.triple.contains("windows") => "COFF",
            _ => "ELF",
        },
        data_layout,
        optimization,
    })
}

unsafe fn make_emitter(
    context: LLVMContextRef,
    module: LLVMModuleRef,
    builder: LLVMBuilderRef,
) -> Emitter {
    let i1 = unsafe { LLVMInt1TypeInContext(context) };
    let i32 = unsafe { LLVMInt32TypeInContext(context) };
    let i64 = unsafe { LLVMInt64TypeInContext(context) };
    let ptr_t = unsafe { LLVMPointerTypeInContext(context, 0) };
    let regs = unsafe { LLVMArrayType2(i32, 16) };
    let mut ctx_fields = [regs, i32, i32, i64, i64, i64, i32, i32, ptr_t, ptr_t];
    let ctx = unsafe {
        LLVMStructTypeInContext(context, ctx_fields.as_mut_ptr(), ctx_fields.len() as u32, 0)
    };
    let fnty = |ret: LLVMTypeRef, params: &mut [LLVMTypeRef]| unsafe {
        LLVMFunctionType(ret, params.as_mut_ptr(), params.len() as u32, 0)
    };
    let run_fn = fnty(i32, &mut [ptr_t, i32]);
    let load_fn = fnty(i32, &mut [ptr_t, i32, i32]);
    let read_fn = fnty(i32, &mut [ptr_t, i32, i32, i32]);
    let write_fn = fnty(i32, &mut [ptr_t, i32, i32, i32]);
    let fshr_fn = fnty(i32, &mut [i32, i32, i32]);
    let declare = |name: &std::ffi::CStr, ty: LLVMTypeRef| unsafe {
        LLVMAddFunction(module, name.as_ptr(), ty)
    };
    let h = Helpers {
        ldr: declare(c"gba_rt_ldr", load_fn),
        ldrh: declare(c"gba_rt_ldrh", load_fn),
        ldrsh: declare(c"gba_rt_ldrsh", load_fn),
        ldrb: declare(c"gba_rt_ldrb", load_fn),
        ldrsb: declare(c"gba_rt_ldrsb", load_fn),
        read32: declare(c"gba_rt_read32", read_fn),
        write32: declare(c"gba_rt_write32", write_fn),
        write16: declare(c"gba_rt_write16", write_fn),
        write8: declare(c"gba_rt_write8", write_fn),
        fshr: declare(c"llvm.fshr.i32", fshr_fn),
    };
    Emitter {
        context,
        module,
        b: builder,
        t: Types {
            i1,
            i32,
            i64,
            ptr: ptr_t,
            ctx,
            run_fn,
            load_fn,
            read_fn,
            write_fn,
            fshr_fn,
        },
        h,
    }
}

/// Table of every resume entry, ROM hash and `main`.
fn build_table(
    e: &Emitter,
    rom_sha256: [u8; 32],
    runs: &[Run],
    guards: &[Guard],
    rom: Option<&[u8]>,
    entry: &str,
) -> Result<(), String> {
    let context = e.context;
    let module = e.module;
    // ROM rows are unique per key; RAM rows may have several guarded
    // variants for the same key (RAM-65-02).
    let mut table: BTreeMap<(u32, u32), (LLVMValueRef, u32)> = BTreeMap::new();
    for run in runs {
        let name = CString::new(run_name(run)).expect("no NUL");
        // SAFETY: declares an external function defined in a shard object.
        let func = unsafe { LLVMAddFunction(module, name.as_ptr(), e.t.run_fn) };
        let size = if run.entry.thumb { 2 } else { 4 };
        let guard = run.guard.map_or(u32::MAX, |g| g as u32);
        for k in 0..run.ops.len() as u32 {
            let key = run.entry.pc.wrapping_add(k * size) | u32::from(run.entry.thumb);
            table.entry((key, guard)).or_insert((func, k));
        }
    }
    // SAFETY: constants and globals created in this module's context.
    unsafe {
        let i8 = LLVMInt8TypeInContext(context);
        let mut row_fields = [e.t.i32, e.t.i32, e.t.ptr, e.t.i32, e.t.i32];
        let row_t = LLVMStructTypeInContext(context, row_fields.as_mut_ptr(), 5, 0);
        let mut rows: Vec<LLVMValueRef> = table
            .iter()
            .map(|(&(key, guard), &(func, index))| {
                let mut vals = [e.c32(key), e.c32(index), func, e.c32(guard), e.c32(0)];
                LLVMConstNamedStruct(row_t, vals.as_mut_ptr(), 5)
            })
            .collect();
        let rows_t = LLVMArrayType2(row_t, rows.len() as u64);
        let rows_init = LLVMConstArray2(row_t, rows.as_mut_ptr(), rows.len() as u64);
        let rows_g = LLVMAddGlobal(module, rows_t, c"gba_aot_entries".as_ptr());
        LLVMSetInitializer(rows_g, rows_init);
        LLVMSetGlobalConstant(rows_g, 1);
        LLVMSetLinkage(rows_g, LLVMLinkage::LLVMInternalLinkage);
        let mut sha_vals: Vec<LLVMValueRef> = rom_sha256
            .iter()
            .map(|&b| LLVMConstInt(i8, u64::from(b), 0))
            .collect();
        let sha_t = LLVMArrayType2(i8, 32);
        let sha_init = LLVMConstArray2(i8, sha_vals.as_mut_ptr(), 32);
        let sha_g = LLVMAddGlobal(module, sha_t, c"gba_aot_rom_sha256".as_ptr());
        LLVMSetInitializer(sha_g, sha_init);
        LLVMSetGlobalConstant(sha_g, 1);
        LLVMSetLinkage(sha_g, LLVMLinkage::LLVMInternalLinkage);

        // Guards: { i32 addr, i32 len, ptr bytes }.
        let mut guard_fields = [e.t.i32, e.t.i32, e.t.ptr];
        let guard_t = LLVMStructTypeInContext(context, guard_fields.as_mut_ptr(), 3, 0);
        let mut guard_rows: Vec<LLVMValueRef> = Vec::with_capacity(guards.len());
        for (i, g) in guards.iter().enumerate() {
            let name = CString::new(format!("gba_aot_guard_bytes_{i}")).expect("no NUL");
            let data =
                LLVMConstStringInContext2(context, g.bytes.as_ptr().cast(), g.bytes.len(), 1);
            let gv = LLVMAddGlobal(
                module,
                LLVMArrayType2(i8, g.bytes.len() as u64),
                name.as_ptr(),
            );
            LLVMSetInitializer(gv, data);
            LLVMSetGlobalConstant(gv, 1);
            LLVMSetLinkage(gv, LLVMLinkage::LLVMInternalLinkage);
            let mut vals = [e.c32(g.addr), e.c32(g.bytes.len() as u32), gv];
            guard_rows.push(LLVMConstNamedStruct(guard_t, vals.as_mut_ptr(), 3));
        }
        let guards_g = LLVMAddGlobal(
            module,
            LLVMArrayType2(guard_t, guard_rows.len() as u64),
            c"gba_aot_guards".as_ptr(),
        );
        LLVMSetInitializer(
            guards_g,
            LLVMConstArray2(guard_t, guard_rows.as_mut_ptr(), guard_rows.len() as u64),
        );
        LLVMSetGlobalConstant(guards_g, 1);
        LLVMSetLinkage(guards_g, LLVMLinkage::LLVMInternalLinkage);

        // Embedded ROM for `bundle` packages (SPEC-066), else null/0.
        let (rom_ptr, rom_len) = match rom {
            Some(bytes) => {
                let data =
                    LLVMConstStringInContext2(context, bytes.as_ptr().cast(), bytes.len(), 1);
                let gv = LLVMAddGlobal(
                    module,
                    LLVMArrayType2(i8, bytes.len() as u64),
                    c"gba_aot_rom".as_ptr(),
                );
                LLVMSetInitializer(gv, data);
                LLVMSetGlobalConstant(gv, 1);
                LLVMSetLinkage(gv, LLVMLinkage::LLVMInternalLinkage);
                (gv, bytes.len() as u64)
            }
            None => (LLVMConstNull(e.t.ptr), 0),
        };

        let mut nm = [
            e.t.i32, e.t.ptr, e.t.ptr, e.t.i32, e.t.ptr, e.t.ptr, e.t.i32, e.t.ptr, e.t.i64,
        ];
        let native_main_t = LLVMFunctionType(e.t.i32, nm.as_mut_ptr(), nm.len() as u32, 0);
        let native_main = LLVMAddFunction(module, c"gba_native_main".as_ptr(), native_main_t);
        let mut mp = [e.t.i32, e.t.ptr];
        let main_t = LLVMFunctionType(e.t.i32, mp.as_mut_ptr(), 2, 0);
        let main = LLVMAddFunction(module, CString::new(entry).expect("no NUL").as_ptr(), main_t);
        let bb = LLVMAppendBasicBlockInContext(context, main, EMPTY);
        e.at(bb);
        let mut args = [
            LLVMGetParam(main, 0),
            LLVMGetParam(main, 1),
            rows_g,
            e.c32(rows.len() as u32),
            sha_g,
            guards_g,
            e.c32(guard_rows.len() as u32),
            rom_ptr,
            e.c64(rom_len),
        ];
        let r = e.call(native_main_t, native_main, &mut args);
        LLVMBuildRet(e.b, r);
    }
    Ok(())
}

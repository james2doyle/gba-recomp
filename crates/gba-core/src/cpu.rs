//! ARM7TDMI interpreter (SPEC-056).
//!
//! The CPU owns architectural state only. Memory, timing and BIOS hooks go
//! through [`CpuBus`]; guest addresses are plain `u32` values.

pub const MODE_USR: u32 = 0x10;
pub const MODE_FIQ: u32 = 0x11;
pub const MODE_IRQ: u32 = 0x12;
pub const MODE_SVC: u32 = 0x13;
pub const MODE_ABT: u32 = 0x17;
pub const MODE_UND: u32 = 0x1B;
pub const MODE_SYS: u32 = 0x1F;

pub const FLAG_N: u32 = 1 << 31;
pub const FLAG_Z: u32 = 1 << 30;
pub const FLAG_C: u32 = 1 << 29;
pub const FLAG_V: u32 = 1 << 28;
pub const FLAG_I: u32 = 1 << 7;
pub const FLAG_F: u32 = 1 << 6;
pub const FLAG_T: u32 = 1 << 5;

/// Memory and system interface used by the CPU. Every access charges its own
/// cycles to the implementor's clock.
pub trait CpuBus {
    fn read8(&mut self, addr: u32, seq: bool) -> u8;
    fn read16(&mut self, addr: u32, seq: bool) -> u16;
    fn read32(&mut self, addr: u32, seq: bool) -> u32;
    fn write8(&mut self, addr: u32, value: u8, seq: bool);
    fn write16(&mut self, addr: u32, value: u16, seq: bool);
    fn write32(&mut self, addr: u32, value: u32, seq: bool);
    fn fetch16(&mut self, addr: u32, seq: bool) -> u16;
    fn fetch32(&mut self, addr: u32, seq: bool) -> u32;
    /// Internal (I) cycles.
    fn idle(&mut self, cycles: u32);
    /// High-level BIOS hook for SWI. Returns true when the call was handled
    /// and the CPU must not enter the SVC vector.
    fn swi_hle(&mut self, _cpu: &mut Cpu, _number: u8) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Architecturally unpredictable encoding/state combination.
    Unpredictable { pc: u32, opcode: u32 },
    /// CPSR mode bits outside the seven valid modes.
    InvalidMode { pc: u32, cpsr: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepResult {
    Retired,
    /// Condition failed; the instruction was fetched and skipped.
    Skipped,
    Exception,
    Fault(Fault),
}

/// Instruction family counters (IAC-10 granularity for interpreted steps).
#[derive(Clone, Debug, Default)]
pub struct CpuStats {
    pub arm: u64,
    pub thumb: u64,
    pub irq: u64,
    pub swi: u64,
    pub undefined: u64,
}

#[derive(Clone, Debug)]
pub struct Cpu {
    /// Visible registers for the current mode. During execution r[15] holds
    /// the pipeline value (instruction address + 8 ARM / + 4 Thumb).
    pub r: [u32; 16],
    pub cpsr: u32,
    /// r8–r12 for non-FIQ (index 0) and FIQ (index 1).
    bank_r8_12: [[u32; 5]; 2],
    /// r13/r14 per bank: usr/sys, fiq, irq, svc, abt, und.
    bank_r13_14: [[u32; 2]; 6],
    /// SPSR per bank (index 0 unused).
    spsr: [u32; 6],
    pub halted: bool,
    branched: bool,
    next_seq: bool,
    pub stats: CpuStats,
    /// Address of the instruction currently executing.
    pub current_pc: u32,
    /// ISA state of the instruction currently executing.
    pub current_thumb: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

fn bank_index(mode: u32) -> Option<usize> {
    match mode {
        MODE_USR | MODE_SYS => Some(0),
        MODE_FIQ => Some(1),
        MODE_IRQ => Some(2),
        MODE_SVC => Some(3),
        MODE_ABT => Some(4),
        MODE_UND => Some(5),
        _ => None,
    }
}

#[inline]
fn bit(value: u32, n: u32) -> bool {
    (value >> n) & 1 != 0
}

impl Cpu {
    /// Reset state: SVC mode, ARM, IRQ/FIQ masked, PC at the reset vector.
    pub fn new() -> Self {
        Self {
            r: [0; 16],
            cpsr: MODE_SVC | FLAG_I | FLAG_F,
            bank_r8_12: [[0; 5]; 2],
            bank_r13_14: [[0; 2]; 6],
            spsr: [0; 6],
            halted: false,
            branched: false,
            next_seq: false,
            stats: CpuStats::default(),
            current_pc: 0,
            current_thumb: false,
        }
    }

    /// State after the GBA BIOS boot sequence, used with HLE BIOS
    /// (SPEC-060): System mode, stacks at their BIOS defaults, PC at ROM.
    pub fn post_boot(entry: u32) -> Self {
        let mut cpu = Self::new();
        cpu.bank_r13_14[3][0] = 0x0300_7FE0; // SVC
        cpu.bank_r13_14[2][0] = 0x0300_7FA0; // IRQ
        cpu.cpsr = MODE_SYS;
        cpu.r[13] = 0x0300_7F00;
        cpu.r[15] = entry;
        cpu
    }

    #[inline]
    pub fn mode(&self) -> u32 {
        self.cpsr & 0x1F
    }

    #[inline]
    pub fn thumb(&self) -> bool {
        self.cpsr & FLAG_T != 0
    }

    /// Address of the next instruction to execute (outside of `step`).
    pub fn pc(&self) -> u32 {
        self.r[15]
    }

    /// Whether the next code fetch is sequential (native exits, SPEC-062).
    pub fn next_seq(&self) -> bool {
        self.next_seq
    }

    pub fn set_next_seq(&mut self, seq: bool) {
        self.next_seq = seq;
    }

    pub fn set_pc(&mut self, pc: u32) {
        self.r[15] = pc;
        self.next_seq = false;
    }

    #[inline]
    fn flag(&self, f: u32) -> bool {
        self.cpsr & f != 0
    }

    #[inline]
    fn set_flag(&mut self, f: u32, on: bool) {
        if on {
            self.cpsr |= f;
        } else {
            self.cpsr &= !f;
        }
    }

    #[inline]
    fn set_nz(&mut self, value: u32) {
        self.cpsr = (self.cpsr & !(FLAG_N | FLAG_Z))
            | (value & FLAG_N)
            | if value == 0 { FLAG_Z } else { 0 };
    }

    pub fn spsr(&self) -> Option<u32> {
        match bank_index(self.mode()) {
            Some(0) | None => None,
            Some(i) => Some(self.spsr[i]),
        }
    }

    fn set_spsr(&mut self, value: u32) {
        if let Some(i) = bank_index(self.mode())
            && i != 0
        {
            self.spsr[i] = value;
        }
    }

    /// Read a register as seen from User mode (for STM/LDM with `^`).
    fn user_reg(&self, n: usize) -> u32 {
        let mode = self.mode();
        match n {
            8..=12 if mode == MODE_FIQ => self.bank_r8_12[0][n - 8],
            13 | 14 if !matches!(mode, MODE_USR | MODE_SYS) => self.bank_r13_14[0][n - 13],
            _ => self.r[n],
        }
    }

    fn set_user_reg(&mut self, n: usize, value: u32) {
        let mode = self.mode();
        match n {
            8..=12 if mode == MODE_FIQ => self.bank_r8_12[0][n - 8] = value,
            13 | 14 if !matches!(mode, MODE_USR | MODE_SYS) => self.bank_r13_14[0][n - 13] = value,
            _ => self.r[n] = value,
        }
    }

    /// Replace CPSR, swapping banked registers when the mode changes.
    pub fn write_cpsr(&mut self, value: u32) {
        let old = self.mode();
        let new = value & 0x1F;
        if old != new {
            self.swap_banks(old, new);
        }
        self.cpsr = value;
    }

    fn swap_banks(&mut self, old: u32, new: u32) {
        let (Some(oi), Some(ni)) = (bank_index(old), bank_index(new)) else {
            return;
        };
        let old_fiq = usize::from(old == MODE_FIQ);
        let new_fiq = usize::from(new == MODE_FIQ);
        if old_fiq != new_fiq {
            self.bank_r8_12[old_fiq].copy_from_slice(&self.r[8..13]);
            let regs = self.bank_r8_12[new_fiq];
            self.r[8..13].copy_from_slice(&regs);
        }
        if oi != ni {
            self.bank_r13_14[oi] = [self.r[13], self.r[14]];
            let [sp, lr] = self.bank_r13_14[ni];
            self.r[13] = sp;
            self.r[14] = lr;
        }
    }

    pub fn banked_sp(&self, mode: u32) -> u32 {
        if bank_index(mode) == bank_index(self.mode()) {
            self.r[13]
        } else {
            bank_index(mode).map_or(0, |i| self.bank_r13_14[i][0])
        }
    }

    fn condition(&self, cond: u32) -> bool {
        let n = self.flag(FLAG_N);
        let z = self.flag(FLAG_Z);
        let c = self.flag(FLAG_C);
        let v = self.flag(FLAG_V);
        match cond {
            0x0 => z,
            0x1 => !z,
            0x2 => c,
            0x3 => !c,
            0x4 => n,
            0x5 => !n,
            0x6 => v,
            0x7 => !v,
            0x8 => c && !z,
            0x9 => !c || z,
            0xA => n == v,
            0xB => n != v,
            0xC => !z && n == v,
            0xD => z || n != v,
            0xE => true,
            _ => false, // 0xF is unpredictable on ARMv4 (NV): never executes.
        }
    }

    /// Enter an exception (CPU-56-07). `return_addr` is stored in LR.
    fn enter_exception(&mut self, mode: u32, vector: u32, return_addr: u32) {
        let old_cpsr = self.cpsr;
        let mut new_cpsr = (old_cpsr & !(0x1F | FLAG_T)) | mode | FLAG_I;
        if mode == MODE_FIQ {
            new_cpsr |= FLAG_F;
        }
        self.write_cpsr(new_cpsr);
        self.set_spsr(old_cpsr);
        self.r[14] = return_addr;
        self.r[15] = vector;
        self.branched = true;
    }

    /// Take a pending IRQ at an instruction boundary (CPU-56-08). Returns true
    /// if the exception was entered.
    pub fn irq<B: CpuBus>(&mut self, bus: &mut B) -> bool {
        self.halted = false;
        if self.flag(FLAG_I) {
            return false;
        }
        // LR = address of next instruction + 4 in both states.
        let next = self.r[15];
        self.enter_exception(MODE_IRQ, 0x18, next.wrapping_add(4));
        self.stats.irq += 1;
        self.refill(bus);
        true
    }

    /// Pipeline refill after a branch (also used after native branch exits,
    /// SPEC-062 NB-62-04).
    pub fn refill<B: CpuBus>(&mut self, bus: &mut B) {
        // Pipeline refill after a branch: N + S fetch (CPU-56-09).
        let pc = self.r[15];
        if self.thumb() {
            let pc = pc & !1;
            self.r[15] = pc;
            bus.fetch16(pc, false);
            bus.fetch16(pc.wrapping_add(2), true);
        } else {
            let pc = pc & !3;
            self.r[15] = pc;
            bus.fetch32(pc, false);
            bus.fetch32(pc.wrapping_add(4), true);
        }
        self.next_seq = true;
        self.branched = false;
    }

    /// Execute one instruction.
    pub fn step<B: CpuBus>(&mut self, bus: &mut B) -> StepResult {
        if bank_index(self.mode()).is_none() {
            return StepResult::Fault(Fault::InvalidMode {
                pc: self.r[15],
                cpsr: self.cpsr,
            });
        }
        self.branched = false;
        let seq = self.next_seq;
        self.next_seq = true;
        let result = if self.thumb() {
            let pc = self.r[15] & !1;
            self.current_pc = pc;
            self.current_thumb = true;
            let op = bus.fetch16(pc, seq);
            self.r[15] = pc.wrapping_add(4);
            self.stats.thumb += 1;
            let result = self.exec_thumb(bus, op);
            if !self.branched {
                self.r[15] = pc.wrapping_add(2);
            }
            result
        } else {
            let pc = self.r[15] & !3;
            self.current_pc = pc;
            self.current_thumb = false;
            let op = bus.fetch32(pc, seq);
            self.r[15] = pc.wrapping_add(8);
            self.stats.arm += 1;
            let result = self.exec_arm(bus, op);
            if !self.branched {
                self.r[15] = pc.wrapping_add(4);
            }
            result
        };
        if let StepResult::Fault(_) = result {
            self.r[15] = self.current_pc;
            return result;
        }
        if self.branched {
            self.refill(bus);
        }
        result
    }

    fn undefined(&mut self) -> StepResult {
        let size = if self.thumb() { 2 } else { 4 };
        let ret = self.current_pc.wrapping_add(size);
        self.enter_exception(MODE_UND, 0x04, ret);
        self.stats.undefined += 1;
        StepResult::Exception
    }

    fn swi<B: CpuBus>(&mut self, bus: &mut B, number: u8) -> StepResult {
        self.stats.swi += 1;
        let size = if self.thumb() { 2 } else { 4 };
        let ret = self.current_pc.wrapping_add(size);
        // Expose the architectural next PC to the HLE hook.
        self.r[15] = ret;
        if bus.swi_hle(self, number) {
            // HLE may redirect execution by changing r[15].
            if self.r[15] != ret {
                self.branched = true;
            } else if self.thumb() {
                self.r[15] = self.current_pc.wrapping_add(4);
            } else {
                self.r[15] = self.current_pc.wrapping_add(8);
            }
            return StepResult::Retired;
        }
        self.enter_exception(MODE_SVC, 0x08, ret);
        StepResult::Exception
    }

    /// Write PC as a branch: ARM-aligned, mode unchanged.
    fn branch_to(&mut self, addr: u32) {
        self.r[15] = if self.thumb() { addr & !1 } else { addr & !3 };
        self.branched = true;
    }

    fn branch_exchange(&mut self, addr: u32) {
        self.set_flag(FLAG_T, addr & 1 != 0);
        self.branch_to(addr);
    }

    // ---------------------------------------------------------------- ALU

    #[inline]
    fn add_flags(&mut self, a: u32, b: u32, carry: bool, set: bool) -> u32 {
        let wide = u64::from(a) + u64::from(b) + u64::from(carry);
        let result = wide as u32;
        if set {
            self.set_nz(result);
            self.set_flag(FLAG_C, wide > 0xFFFF_FFFF);
            self.set_flag(FLAG_V, (!(a ^ b) & (a ^ result)) >> 31 != 0);
        }
        result
    }

    #[inline]
    fn sub_flags(&mut self, a: u32, b: u32, carry: bool, set: bool) -> u32 {
        self.add_flags(a, !b, carry, set)
    }

    /// Shift by immediate amount (encoding semantics, CPU-56-02).
    fn shift_imm(&self, ty: u32, value: u32, amount: u32) -> (u32, bool) {
        let c = self.flag(FLAG_C);
        match ty {
            0 => {
                if amount == 0 {
                    (value, c)
                } else {
                    (value << amount, bit(value, 32 - amount))
                }
            }
            1 => {
                if amount == 0 {
                    (0, bit(value, 31))
                } else {
                    (value >> amount, bit(value, amount - 1))
                }
            }
            2 => {
                if amount == 0 {
                    (((value as i32) >> 31) as u32, bit(value, 31))
                } else {
                    (((value as i32) >> amount) as u32, bit(value, amount - 1))
                }
            }
            _ => {
                if amount == 0 {
                    ((u32::from(c) << 31) | (value >> 1), value & 1 != 0)
                } else {
                    (value.rotate_right(amount), bit(value, amount - 1))
                }
            }
        }
    }

    /// Shift by register amount (low byte), CPU-56-02.
    fn shift_reg(&self, ty: u32, value: u32, amount: u32) -> (u32, bool) {
        let c = self.flag(FLAG_C);
        if amount == 0 {
            return (value, c);
        }
        match ty {
            0 => match amount {
                1..=31 => (value << amount, bit(value, 32 - amount)),
                32 => (0, value & 1 != 0),
                _ => (0, false),
            },
            1 => match amount {
                1..=31 => (value >> amount, bit(value, amount - 1)),
                32 => (0, bit(value, 31)),
                _ => (0, false),
            },
            2 => {
                if amount < 32 {
                    (((value as i32) >> amount) as u32, bit(value, amount - 1))
                } else {
                    (((value as i32) >> 31) as u32, bit(value, 31))
                }
            }
            _ => {
                let rot = amount & 31;
                if rot == 0 {
                    (value, bit(value, 31))
                } else {
                    (value.rotate_right(rot), bit(value, rot - 1))
                }
            }
        }
    }

    // ---------------------------------------------------------------- ARM

    fn exec_arm<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        if !self.condition(op >> 28) {
            return StepResult::Skipped;
        }
        match (op >> 25) & 7 {
            0 => {
                if op & 0x0FFF_FFF0 == 0x012F_FF10 {
                    let rn = (op & 0xF) as usize;
                    let target = self.r[rn];
                    self.branch_exchange(target);
                    StepResult::Retired
                } else if op & 0x0FC0_00F0 == 0x0000_0090 {
                    self.arm_multiply(bus, op)
                } else if op & 0x0F80_00F0 == 0x0080_0090 {
                    self.arm_multiply_long(bus, op)
                } else if op & 0x0FB0_0FF0 == 0x0100_0090 {
                    self.arm_swap(bus, op)
                } else if op & 0x90 == 0x90 && (op >> 5) & 3 != 0 {
                    self.arm_halfword(bus, op)
                } else if op & 0x0FBF_0FFF == 0x010F_0000 {
                    self.arm_mrs(op)
                } else if op & 0x0FB0_FFF0 == 0x0120_F000 {
                    let value = self.r[(op & 0xF) as usize];
                    self.arm_msr(op, value)
                } else if op & 0x0190_0000 == 0x0100_0000 {
                    // TST/TEQ/CMP/CMN without S: remaining PSR/undefined space.
                    self.undefined()
                } else {
                    self.arm_data_processing(bus, op)
                }
            }
            1 => {
                if op & 0x0FB0_F000 == 0x0320_F000 {
                    let rot = ((op >> 8) & 0xF) * 2;
                    let value = (op & 0xFF).rotate_right(rot);
                    self.arm_msr(op, value)
                } else if op & 0x0190_0000 == 0x0100_0000 {
                    self.undefined()
                } else {
                    self.arm_data_processing(bus, op)
                }
            }
            2 => self.arm_single_transfer(bus, op),
            3 => {
                if op & 0x10 != 0 {
                    self.undefined()
                } else {
                    self.arm_single_transfer(bus, op)
                }
            }
            4 => self.arm_block_transfer(bus, op),
            5 => {
                let offset = (((op & 0x00FF_FFFF) << 8) as i32 >> 6) as u32;
                if op & (1 << 24) != 0 {
                    self.r[14] = self.current_pc.wrapping_add(4);
                }
                let target = self.r[15].wrapping_add(offset);
                self.branch_to(target);
                StepResult::Retired
            }
            6 => self.undefined(),
            _ => {
                if op & (1 << 24) != 0 {
                    self.swi(bus, ((op >> 16) & 0xFF) as u8)
                } else {
                    self.undefined()
                }
            }
        }
    }

    fn arm_data_processing<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let opcode = (op >> 21) & 0xF;
        let set = bit(op, 20);
        let rn = ((op >> 16) & 0xF) as usize;
        let rd = ((op >> 12) & 0xF) as usize;
        let mut pc_extra = 0;
        let (operand, shifter_carry) = if bit(op, 25) {
            let rot = ((op >> 8) & 0xF) * 2;
            let value = (op & 0xFF).rotate_right(rot);
            let carry = if rot == 0 {
                self.flag(FLAG_C)
            } else {
                bit(value, 31)
            };
            (value, carry)
        } else {
            let rm = (op & 0xF) as usize;
            let ty = (op >> 5) & 3;
            if bit(op, 4) {
                // Register-specified shift: one extra internal cycle and PC
                // reads as +12 (CPU-56-04).
                bus.idle(1);
                pc_extra = 4;
                let rs = ((op >> 8) & 0xF) as usize;
                let amount = self.r[rs] & 0xFF;
                let mut value = self.r[rm];
                if rm == 15 {
                    value = value.wrapping_add(4);
                }
                self.shift_reg(ty, value, amount)
            } else {
                let amount = (op >> 7) & 0x1F;
                self.shift_imm(ty, self.r[rm], amount)
            }
        };
        let mut a = self.r[rn];
        if rn == 15 {
            a = a.wrapping_add(pc_extra);
        }
        let carry = self.flag(FLAG_C);
        // Flags are only written by S variants with Rd != PC; Rd == PC with
        // S restores CPSR from SPSR (CPU-56-03).
        let flags = set && rd != 15;
        let logical = |cpu: &mut Self, result: u32| {
            if flags {
                cpu.set_nz(result);
                cpu.set_flag(FLAG_C, shifter_carry);
            }
            result
        };
        let (result, write) = match opcode {
            0x0 => (logical(self, a & operand), true),
            0x1 => (logical(self, a ^ operand), true),
            0x2 => (self.sub_flags(a, operand, true, flags), true),
            0x3 => (self.sub_flags(operand, a, true, flags), true),
            0x4 => (self.add_flags(a, operand, false, flags), true),
            0x5 => (self.add_flags(a, operand, carry, flags), true),
            0x6 => (self.sub_flags(a, operand, carry, flags), true),
            0x7 => (self.sub_flags(operand, a, carry, flags), true),
            0x8 => (logical(self, a & operand), false),
            0x9 => (logical(self, a ^ operand), false),
            0xA => (self.sub_flags(a, operand, true, flags), false),
            0xB => (self.add_flags(a, operand, false, flags), false),
            0xC => (logical(self, a | operand), true),
            0xD => (logical(self, operand), true),
            0xE => (logical(self, a & !operand), true),
            _ => (logical(self, !operand), true),
        };
        if rd == 15 {
            if set {
                let Some(spsr) = self.spsr() else {
                    return StepResult::Fault(Fault::Unpredictable {
                        pc: self.current_pc,
                        opcode: op,
                    });
                };
                self.write_cpsr(spsr);
            }
            if write || set {
                if write {
                    self.branch_to(result);
                } else {
                    // TSTP-style: CPSR restored, PC unchanged.
                    self.branch_to(self.current_pc.wrapping_add(4));
                }
            }
        } else if write {
            self.r[rd] = result;
        }
        StepResult::Retired
    }

    fn arm_mrs(&mut self, op: u32) -> StepResult {
        let rd = ((op >> 12) & 0xF) as usize;
        let value = if bit(op, 22) {
            match self.spsr() {
                Some(v) => v,
                None => self.cpsr,
            }
        } else {
            self.cpsr
        };
        self.r[rd] = value;
        StepResult::Retired
    }

    fn arm_msr(&mut self, op: u32, value: u32) -> StepResult {
        let mut mask = 0u32;
        if bit(op, 19) {
            mask |= 0xFF00_0000;
        }
        if bit(op, 18) {
            mask |= 0x00FF_0000;
        }
        if bit(op, 17) {
            mask |= 0x0000_FF00;
        }
        if bit(op, 16) {
            mask |= 0x0000_00FF;
        }
        if bit(op, 22) {
            if let Some(spsr) = self.spsr() {
                self.set_spsr((spsr & !mask) | (value & mask));
            }
        } else {
            if self.mode() == MODE_USR {
                mask &= 0xFF00_0000;
            }
            // The T bit is not writable through MSR.
            mask &= !FLAG_T;
            let new = (self.cpsr & !mask) | (value & mask);
            if bank_index(new & 0x1F).is_none() {
                return StepResult::Fault(Fault::InvalidMode {
                    pc: self.current_pc,
                    cpsr: new,
                });
            }
            self.write_cpsr(new);
        }
        StepResult::Retired
    }

    fn multiply_cycles(rs: u32, signed: bool) -> u32 {
        let check = |mask: u32| {
            let v = rs & mask;
            v == 0 || (signed && v == mask)
        };
        if check(0xFFFF_FF00) {
            1
        } else if check(0xFFFF_0000) {
            2
        } else if check(0xFF00_0000) {
            3
        } else {
            4
        }
    }

    fn arm_multiply<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let rd = ((op >> 16) & 0xF) as usize;
        let rn = ((op >> 12) & 0xF) as usize;
        let rs = ((op >> 8) & 0xF) as usize;
        let rm = (op & 0xF) as usize;
        let accumulate = bit(op, 21);
        let mut result = self.r[rm].wrapping_mul(self.r[rs]);
        let mut cycles = Self::multiply_cycles(self.r[rs], true);
        if accumulate {
            result = result.wrapping_add(self.r[rn]);
            cycles += 1;
        }
        bus.idle(cycles);
        if rd != 15 {
            self.r[rd] = result;
        }
        if bit(op, 20) {
            self.set_nz(result);
        }
        StepResult::Retired
    }

    fn arm_multiply_long<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let rd_hi = ((op >> 16) & 0xF) as usize;
        let rd_lo = ((op >> 12) & 0xF) as usize;
        let rs = ((op >> 8) & 0xF) as usize;
        let rm = (op & 0xF) as usize;
        let signed = bit(op, 22);
        let accumulate = bit(op, 21);
        let mut result = if signed {
            (i64::from(self.r[rm] as i32) * i64::from(self.r[rs] as i32)) as u64
        } else {
            u64::from(self.r[rm]) * u64::from(self.r[rs])
        };
        let mut cycles = Self::multiply_cycles(self.r[rs], signed) + 1;
        if accumulate {
            let acc = (u64::from(self.r[rd_hi]) << 32) | u64::from(self.r[rd_lo]);
            result = result.wrapping_add(acc);
            cycles += 1;
        }
        bus.idle(cycles);
        self.r[rd_lo] = result as u32;
        self.r[rd_hi] = (result >> 32) as u32;
        if bit(op, 20) {
            self.set_flag(FLAG_N, bit((result >> 32) as u32, 31));
            self.set_flag(FLAG_Z, result == 0);
        }
        StepResult::Retired
    }

    fn arm_swap<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let rn = ((op >> 16) & 0xF) as usize;
        let rd = ((op >> 12) & 0xF) as usize;
        let rm = (op & 0xF) as usize;
        let addr = self.r[rn];
        let source = self.r[rm];
        if bit(op, 22) {
            let value = bus.read8(addr, false);
            bus.write8(addr, source as u8, false);
            self.r[rd] = u32::from(value);
        } else {
            let value = bus.read32(addr & !3, false).rotate_right((addr & 3) * 8);
            bus.write32(addr & !3, source, false);
            self.r[rd] = value;
        }
        bus.idle(1);
        self.next_seq = false;
        StepResult::Retired
    }

    /// Load a word with ARM7TDMI misalignment rotation (CPU-56-05).
    pub fn load32_rotated<B: CpuBus>(bus: &mut B, addr: u32) -> u32 {
        bus.read32(addr & !3, false).rotate_right((addr & 3) * 8)
    }

    pub fn load16_rotated<B: CpuBus>(bus: &mut B, addr: u32) -> u32 {
        u32::from(bus.read16(addr & !1, false)).rotate_right((addr & 1) * 8)
    }

    pub fn load_signed16<B: CpuBus>(bus: &mut B, addr: u32) -> u32 {
        if addr & 1 != 0 {
            bus.read8(addr, false) as i8 as i32 as u32
        } else {
            bus.read16(addr, false) as i16 as i32 as u32
        }
    }

    fn arm_single_transfer<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let pre = bit(op, 24);
        let up = bit(op, 23);
        let byte = bit(op, 22);
        let writeback = bit(op, 21) || !pre;
        let load = bit(op, 20);
        let rn = ((op >> 16) & 0xF) as usize;
        let rd = ((op >> 12) & 0xF) as usize;
        let offset = if bit(op, 25) {
            let rm = (op & 0xF) as usize;
            let ty = (op >> 5) & 3;
            let amount = (op >> 7) & 0x1F;
            self.shift_imm(ty, self.r[rm], amount).0
        } else {
            op & 0xFFF
        };
        let base = self.r[rn];
        let offset_base = if up {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let addr = if pre { offset_base } else { base };
        if load {
            let value = if byte {
                u32::from(bus.read8(addr, false))
            } else {
                Self::load32_rotated(bus, addr)
            };
            bus.idle(1);
            if writeback && rn != rd {
                self.r[rn] = offset_base;
            }
            if rd == 15 {
                self.branch_to(value);
            } else {
                self.r[rd] = value;
            }
        } else {
            let mut value = self.r[rd];
            if rd == 15 {
                value = value.wrapping_add(4);
            }
            if byte {
                bus.write8(addr, value as u8, false);
            } else {
                bus.write32(addr & !3, value, false);
            }
            if writeback {
                self.r[rn] = offset_base;
            }
        }
        self.next_seq = false;
        StepResult::Retired
    }

    fn arm_halfword<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let pre = bit(op, 24);
        let up = bit(op, 23);
        let immediate = bit(op, 22);
        let writeback = bit(op, 21) || !pre;
        let load = bit(op, 20);
        let rn = ((op >> 16) & 0xF) as usize;
        let rd = ((op >> 12) & 0xF) as usize;
        let sh = (op >> 5) & 3;
        let offset = if immediate {
            ((op >> 4) & 0xF0) | (op & 0xF)
        } else {
            self.r[(op & 0xF) as usize]
        };
        let base = self.r[rn];
        let offset_base = if up {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let addr = if pre { offset_base } else { base };
        if load {
            let value = match sh {
                1 => Self::load16_rotated(bus, addr),
                2 => bus.read8(addr, false) as i8 as i32 as u32,
                _ => Self::load_signed16(bus, addr),
            };
            bus.idle(1);
            if writeback && rn != rd {
                self.r[rn] = offset_base;
            }
            if rd == 15 {
                self.branch_to(value);
            } else {
                self.r[rd] = value;
            }
        } else {
            if sh != 1 {
                // LDRD/STRD do not exist on ARMv4T.
                return self.undefined();
            }
            let mut value = self.r[rd];
            if rd == 15 {
                value = value.wrapping_add(4);
            }
            bus.write16(addr & !1, value as u16, false);
            if writeback {
                self.r[rn] = offset_base;
            }
        }
        self.next_seq = false;
        StepResult::Retired
    }

    fn arm_block_transfer<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let pre = bit(op, 24);
        let up = bit(op, 23);
        let psr = bit(op, 22);
        let writeback = bit(op, 21);
        let load = bit(op, 20);
        let rn = ((op >> 16) & 0xF) as usize;
        let list = op & 0xFFFF;
        let base = self.r[rn];
        self.block_transfer(bus, rn, list, base, pre, up, psr, writeback, load);
        StepResult::Retired
    }

    /// Shared LDM/STM implementation (CPU-56-05), also used by Thumb.
    #[allow(clippy::too_many_arguments)]
    fn block_transfer<B: CpuBus>(
        &mut self,
        bus: &mut B,
        rn: usize,
        list: u32,
        base: u32,
        pre: bool,
        up: bool,
        psr: bool,
        writeback: bool,
        load: bool,
    ) {
        let (list, count) = if list == 0 {
            (1 << 15, 16u32)
        } else {
            (list, list.count_ones())
        };
        let size = count * 4;
        // Transfers always go from the lowest address upward.
        let (start, final_base) = if up {
            (
                if pre { base.wrapping_add(4) } else { base },
                base.wrapping_add(size),
            )
        } else {
            let low = base.wrapping_sub(size);
            (if pre { low } else { low.wrapping_add(4) }, low)
        };
        let user_bank = psr && !(load && list & (1 << 15) != 0);
        let mut addr = start;
        let mut seq = false;
        let first = list.trailing_zeros() as usize;
        if load {
            if writeback {
                self.r[rn] = final_base;
            }
            for reg in 0..16 {
                if list & (1 << reg) == 0 {
                    continue;
                }
                let value = bus.read32(addr & !3, seq);
                seq = true;
                addr = addr.wrapping_add(4);
                if reg == 15 {
                    if psr && let Some(spsr) = self.spsr() {
                        self.write_cpsr(spsr);
                    }
                    self.branch_to(value);
                } else if user_bank {
                    self.set_user_reg(reg, value);
                } else {
                    self.r[reg] = value;
                }
            }
            bus.idle(1);
        } else {
            for reg in 0..16 {
                if list & (1 << reg) == 0 {
                    continue;
                }
                let mut value = if user_bank {
                    self.user_reg(reg)
                } else {
                    self.r[reg]
                };
                if reg == 15 {
                    value = value.wrapping_add(if self.thumb() { 2 } else { 4 });
                }
                if reg == rn && reg != first && writeback {
                    value = final_base;
                }
                bus.write32(addr & !3, value, seq);
                seq = true;
                addr = addr.wrapping_add(4);
            }
            if writeback {
                self.r[rn] = final_base;
            }
        }
        self.next_seq = false;
    }

    // ---------------------------------------------------------------- Thumb

    fn exec_thumb<B: CpuBus>(&mut self, bus: &mut B, op: u16) -> StepResult {
        let op = u32::from(op);
        match op >> 13 {
            0 => {
                if (op >> 11) & 3 == 3 {
                    self.thumb_add_sub(op)
                } else {
                    self.thumb_shift_imm(op)
                }
            }
            1 => self.thumb_imm(op),
            2 => {
                if op >> 10 == 0b010000 {
                    self.thumb_alu(bus, op)
                } else if op >> 10 == 0b010001 {
                    self.thumb_hi_reg(op)
                } else if op >> 11 == 0b01001 {
                    let rd = ((op >> 8) & 7) as usize;
                    let addr = (self.r[15] & !2).wrapping_add((op & 0xFF) << 2);
                    self.r[rd] = Self::load32_rotated(bus, addr);
                    bus.idle(1);
                    self.next_seq = false;
                    StepResult::Retired
                } else {
                    self.thumb_load_store_reg(bus, op)
                }
            }
            3 => self.thumb_load_store_imm(bus, op),
            4 => {
                if op & (1 << 12) == 0 {
                    self.thumb_halfword_imm(bus, op)
                } else {
                    self.thumb_sp_relative(bus, op)
                }
            }
            5 => {
                if op & (1 << 12) == 0 {
                    let rd = ((op >> 8) & 7) as usize;
                    let imm = (op & 0xFF) << 2;
                    self.r[rd] = if op & (1 << 11) != 0 {
                        self.r[13].wrapping_add(imm)
                    } else {
                        (self.r[15] & !2).wrapping_add(imm)
                    };
                    StepResult::Retired
                } else if (op >> 8) & 0xF == 0 {
                    let imm = (op & 0x7F) << 2;
                    self.r[13] = if op & 0x80 != 0 {
                        self.r[13].wrapping_sub(imm)
                    } else {
                        self.r[13].wrapping_add(imm)
                    };
                    StepResult::Retired
                } else if (op >> 9) & 3 == 2 {
                    self.thumb_push_pop(bus, op)
                } else {
                    self.undefined()
                }
            }
            6 => {
                if op & (1 << 12) == 0 {
                    let rb = ((op >> 8) & 7) as usize;
                    let list = op & 0xFF;
                    let load = op & (1 << 11) != 0;
                    let base = self.r[rb];
                    // Writeback is suppressed for LDMIA when Rb is in the list.
                    let writeback = !(load && list & (1 << rb) != 0);
                    self.block_transfer(bus, rb, list, base, false, true, false, writeback, load);
                    if list == 0 && !load {
                        // Empty list stores PC; handled in block_transfer.
                    }
                    StepResult::Retired
                } else {
                    let cond = (op >> 8) & 0xF;
                    match cond {
                        0xF => self.swi(bus, op as u8),
                        0xE => self.undefined(),
                        _ => {
                            if self.condition(cond) {
                                let offset = ((op & 0xFF) as i8 as i32 as u32) << 1;
                                let target = self.r[15].wrapping_add(offset);
                                self.branch_to(target);
                                StepResult::Retired
                            } else {
                                StepResult::Skipped
                            }
                        }
                    }
                }
            }
            _ => {
                match (op >> 11) & 3 {
                    0 => {
                        let offset = (((op & 0x7FF) << 21) as i32 >> 20) as u32;
                        let target = self.r[15].wrapping_add(offset);
                        self.branch_to(target);
                        StepResult::Retired
                    }
                    1 => self.undefined(), // BLX suffix is ARMv5.
                    2 => {
                        let offset = (((op & 0x7FF) << 21) as i32 >> 9) as u32;
                        self.r[14] = self.r[15].wrapping_add(offset);
                        StepResult::Retired
                    }
                    _ => {
                        let target = self.r[14].wrapping_add((op & 0x7FF) << 1);
                        self.r[14] = self.current_pc.wrapping_add(2) | 1;
                        self.branch_to(target);
                        StepResult::Retired
                    }
                }
            }
        }
    }

    fn thumb_shift_imm(&mut self, op: u32) -> StepResult {
        let ty = (op >> 11) & 3;
        let amount = (op >> 6) & 0x1F;
        let rs = ((op >> 3) & 7) as usize;
        let rd = (op & 7) as usize;
        let (value, carry) = self.shift_imm(ty, self.r[rs], amount);
        self.r[rd] = value;
        self.set_nz(value);
        self.set_flag(FLAG_C, carry);
        StepResult::Retired
    }

    fn thumb_add_sub(&mut self, op: u32) -> StepResult {
        let rn_or_imm = (op >> 6) & 7;
        let rs = ((op >> 3) & 7) as usize;
        let rd = (op & 7) as usize;
        let operand = if op & (1 << 10) != 0 {
            rn_or_imm
        } else {
            self.r[rn_or_imm as usize]
        };
        let a = self.r[rs];
        self.r[rd] = if op & (1 << 9) != 0 {
            self.sub_flags(a, operand, true, true)
        } else {
            self.add_flags(a, operand, false, true)
        };
        StepResult::Retired
    }

    fn thumb_imm(&mut self, op: u32) -> StepResult {
        let rd = ((op >> 8) & 7) as usize;
        let imm = op & 0xFF;
        match (op >> 11) & 3 {
            0 => {
                self.r[rd] = imm;
                self.set_nz(imm);
            }
            1 => {
                self.sub_flags(self.r[rd], imm, true, true);
            }
            2 => self.r[rd] = self.add_flags(self.r[rd], imm, false, true),
            _ => self.r[rd] = self.sub_flags(self.r[rd], imm, true, true),
        }
        StepResult::Retired
    }

    fn thumb_alu<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let rs = ((op >> 3) & 7) as usize;
        let rd = (op & 7) as usize;
        let a = self.r[rd];
        let b = self.r[rs];
        let c = self.flag(FLAG_C);
        let logical = |cpu: &mut Self, v: u32| {
            cpu.set_nz(v);
            v
        };
        let shift = |cpu: &mut Self, bus: &mut B, ty: u32| {
            bus.idle(1);
            let (v, carry) = cpu.shift_reg(ty, a, b & 0xFF);
            cpu.set_nz(v);
            cpu.set_flag(FLAG_C, carry);
            v
        };
        let (result, write) = match (op >> 6) & 0xF {
            0x0 => (logical(self, a & b), true),
            0x1 => (logical(self, a ^ b), true),
            0x2 => (shift(self, bus, 0), true),
            0x3 => (shift(self, bus, 1), true),
            0x4 => (shift(self, bus, 2), true),
            0x5 => (self.add_flags(a, b, c, true), true),
            0x6 => (self.sub_flags(a, b, c, true), true),
            0x7 => (shift(self, bus, 3), true),
            0x8 => (logical(self, a & b), false),
            0x9 => (self.sub_flags(0, b, true, true), true),
            0xA => (self.sub_flags(a, b, true, true), false),
            0xB => (self.add_flags(a, b, false, true), false),
            0xC => (logical(self, a | b), true),
            0xD => {
                bus.idle(Self::multiply_cycles(a, true));
                (logical(self, a.wrapping_mul(b)), true)
            }
            0xE => (logical(self, a & !b), true),
            _ => (logical(self, !b), true),
        };
        if write {
            self.r[rd] = result;
        }
        StepResult::Retired
    }

    fn thumb_hi_reg(&mut self, op: u32) -> StepResult {
        let rs = (((op >> 3) & 7) | ((op >> 3) & 8)) as usize;
        let rd = ((op & 7) | ((op >> 4) & 8)) as usize;
        let value = self.r[rs];
        match (op >> 8) & 3 {
            0 => {
                let result = self.r[rd].wrapping_add(value);
                if rd == 15 {
                    self.branch_to(result);
                } else {
                    self.r[rd] = result;
                }
            }
            1 => {
                self.sub_flags(self.r[rd], value, true, true);
            }
            2 => {
                if rd == 15 {
                    self.branch_to(value);
                } else {
                    self.r[rd] = value;
                }
            }
            _ => self.branch_exchange(value),
        }
        StepResult::Retired
    }

    fn thumb_load_store_reg<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let ro = ((op >> 6) & 7) as usize;
        let rb = ((op >> 3) & 7) as usize;
        let rd = (op & 7) as usize;
        let addr = self.r[rb].wrapping_add(self.r[ro]);
        match (op >> 9) & 7 {
            0 => bus.write32(addr & !3, self.r[rd], false),
            1 => bus.write16(addr & !1, self.r[rd] as u16, false),
            2 => bus.write8(addr, self.r[rd] as u8, false),
            3 => {
                self.r[rd] = bus.read8(addr, false) as i8 as i32 as u32;
                bus.idle(1);
            }
            4 => {
                self.r[rd] = Self::load32_rotated(bus, addr);
                bus.idle(1);
            }
            5 => {
                self.r[rd] = Self::load16_rotated(bus, addr);
                bus.idle(1);
            }
            6 => {
                self.r[rd] = u32::from(bus.read8(addr, false));
                bus.idle(1);
            }
            _ => {
                self.r[rd] = Self::load_signed16(bus, addr);
                bus.idle(1);
            }
        }
        self.next_seq = false;
        StepResult::Retired
    }

    fn thumb_load_store_imm<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let byte = op & (1 << 12) != 0;
        let load = op & (1 << 11) != 0;
        let offset = (op >> 6) & 0x1F;
        let rb = ((op >> 3) & 7) as usize;
        let rd = (op & 7) as usize;
        let addr = self.r[rb].wrapping_add(if byte { offset } else { offset << 2 });
        match (byte, load) {
            (false, false) => bus.write32(addr & !3, self.r[rd], false),
            (true, false) => bus.write8(addr, self.r[rd] as u8, false),
            (false, true) => {
                self.r[rd] = Self::load32_rotated(bus, addr);
                bus.idle(1);
            }
            (true, true) => {
                self.r[rd] = u32::from(bus.read8(addr, false));
                bus.idle(1);
            }
        }
        self.next_seq = false;
        StepResult::Retired
    }

    fn thumb_halfword_imm<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let offset = ((op >> 6) & 0x1F) << 1;
        let rb = ((op >> 3) & 7) as usize;
        let rd = (op & 7) as usize;
        let addr = self.r[rb].wrapping_add(offset);
        if op & (1 << 11) != 0 {
            self.r[rd] = Self::load16_rotated(bus, addr);
            bus.idle(1);
        } else {
            bus.write16(addr & !1, self.r[rd] as u16, false);
        }
        self.next_seq = false;
        StepResult::Retired
    }

    fn thumb_sp_relative<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let rd = ((op >> 8) & 7) as usize;
        let addr = self.r[13].wrapping_add((op & 0xFF) << 2);
        if op & (1 << 11) != 0 {
            self.r[rd] = Self::load32_rotated(bus, addr);
            bus.idle(1);
        } else {
            bus.write32(addr & !3, self.r[rd], false);
        }
        self.next_seq = false;
        StepResult::Retired
    }

    fn thumb_push_pop<B: CpuBus>(&mut self, bus: &mut B, op: u32) -> StepResult {
        let load = op & (1 << 11) != 0;
        let extra = op & (1 << 8) != 0;
        let mut list = op & 0xFF;
        if extra {
            list |= if load { 1 << 15 } else { 1 << 14 };
        }
        let sp = self.r[13];
        if load {
            self.block_transfer(bus, 13, list, sp, false, true, false, true, true);
        } else {
            self.block_transfer(bus, 13, list, sp, true, false, false, true, false);
        }
        StepResult::Retired
    }
}

#[cfg(test)]
mod tests;

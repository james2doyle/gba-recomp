use super::*;

/// Flat 64 KiB test memory mirrored over the address space; 1 cycle/access.
struct TestBus {
    mem: Vec<u8>,
    cycles: u64,
    swis: Vec<u8>,
    hle: bool,
}

impl TestBus {
    fn new() -> Self {
        Self {
            mem: vec![0; 0x10000],
            cycles: 0,
            swis: Vec::new(),
            hle: false,
        }
    }
    fn a(addr: u32) -> usize {
        (addr & 0xFFFF) as usize
    }
    fn put32(&mut self, addr: u32, v: u32) {
        let a = Self::a(addr);
        self.mem[a..a + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put16(&mut self, addr: u32, v: u16) {
        let a = Self::a(addr);
        self.mem[a..a + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn get32(&self, addr: u32) -> u32 {
        let a = Self::a(addr);
        u32::from_le_bytes(self.mem[a..a + 4].try_into().unwrap())
    }
}

impl CpuBus for TestBus {
    fn read8(&mut self, addr: u32, _: bool) -> u8 {
        self.cycles += 1;
        self.mem[Self::a(addr)]
    }
    fn read16(&mut self, addr: u32, _: bool) -> u16 {
        self.cycles += 1;
        let a = Self::a(addr & !1);
        u16::from_le_bytes([self.mem[a], self.mem[a + 1]])
    }
    fn read32(&mut self, addr: u32, _: bool) -> u32 {
        self.cycles += 1;
        self.get32(addr & !3)
    }
    fn write8(&mut self, addr: u32, v: u8, _: bool) {
        self.cycles += 1;
        self.mem[Self::a(addr)] = v;
    }
    fn write16(&mut self, addr: u32, v: u16, _: bool) {
        self.cycles += 1;
        self.put16(addr & !1, v);
    }
    fn write32(&mut self, addr: u32, v: u32, _: bool) {
        self.cycles += 1;
        self.put32(addr & !3, v);
    }
    fn fetch16(&mut self, addr: u32, s: bool) -> u16 {
        self.read16(addr, s)
    }
    fn fetch32(&mut self, addr: u32, s: bool) -> u32 {
        self.read32(addr, s)
    }
    fn idle(&mut self, c: u32) {
        self.cycles += u64::from(c);
    }
    fn swi_hle(&mut self, _cpu: &mut Cpu, n: u8) -> bool {
        self.swis.push(n);
        self.hle
    }
}

fn arm(program: &[u32]) -> (Cpu, TestBus) {
    let mut bus = TestBus::new();
    for (i, w) in program.iter().enumerate() {
        bus.put32(0x100 + i as u32 * 4, *w);
    }
    let mut cpu = Cpu::new();
    cpu.write_cpsr(MODE_SYS);
    cpu.set_pc(0x100);
    (cpu, bus)
}

fn thumb(program: &[u16]) -> (Cpu, TestBus) {
    let mut bus = TestBus::new();
    for (i, h) in program.iter().enumerate() {
        bus.put16(0x100 + i as u32 * 2, *h);
    }
    let mut cpu = Cpu::new();
    cpu.write_cpsr(MODE_SYS | FLAG_T);
    cpu.set_pc(0x100);
    (cpu, bus)
}

fn run(cpu: &mut Cpu, bus: &mut TestBus, steps: usize) {
    for _ in 0..steps {
        let r = cpu.step(bus);
        assert!(!matches!(r, StepResult::Fault(_)), "{r:?}");
    }
}

#[test]
fn all_conditions_against_all_flag_combinations() {
    let cpu_flags = |n: bool, z: bool, c: bool, v: bool| {
        let mut cpu = Cpu::new();
        cpu.cpsr = MODE_SYS
            | if n { FLAG_N } else { 0 }
            | if z { FLAG_Z } else { 0 }
            | if c { FLAG_C } else { 0 }
            | if v { FLAG_V } else { 0 };
        cpu
    };
    for bits in 0..16u32 {
        let (n, z, c, v) = (bits & 8 != 0, bits & 4 != 0, bits & 2 != 0, bits & 1 != 0);
        let cpu = cpu_flags(n, z, c, v);
        let expected = [
            z,
            !z,
            c,
            !c,
            n,
            !n,
            v,
            !v,
            c && !z,
            !c || z,
            n == v,
            n != v,
            !z && n == v,
            z || n != v,
            true,
            false,
        ];
        for (cond, want) in expected.iter().enumerate() {
            assert_eq!(
                cpu.condition(cond as u32),
                *want,
                "cond {cond} flags {bits:04b}"
            );
        }
    }
}

#[test]
fn shifter_immediate_and_register_edge_cases() {
    let mut cpu = Cpu::new();
    cpu.cpsr |= FLAG_C;
    // LSL #0 keeps value and carry; LSR #0 means 32; ASR #0 means 32; ROR #0 is RRX.
    assert_eq!(cpu.shift_imm(0, 0x8000_0001, 0), (0x8000_0001, true));
    assert_eq!(cpu.shift_imm(1, 0x8000_0001, 0), (0, true));
    assert_eq!(cpu.shift_imm(2, 0x8000_0000, 0), (0xFFFF_FFFF, true));
    assert_eq!(cpu.shift_imm(3, 0x0000_0003, 0), (0x8000_0001, true));
    assert_eq!(cpu.shift_imm(0, 0x8000_0001, 1), (0x0000_0002, true));
    assert_eq!(cpu.shift_imm(1, 0x0000_0003, 1), (0x0000_0001, true));
    assert_eq!(cpu.shift_imm(0, 1, 31), (0x8000_0000, false));
    // Register shifts.
    assert_eq!(cpu.shift_reg(0, 5, 0), (5, true));
    assert_eq!(cpu.shift_reg(0, 1, 32), (0, true));
    assert_eq!(cpu.shift_reg(0, 1, 33), (0, false));
    assert_eq!(cpu.shift_reg(1, 0x8000_0000, 32), (0, true));
    assert_eq!(cpu.shift_reg(1, 0x8000_0000, 33), (0, false));
    assert_eq!(cpu.shift_reg(2, 0x8000_0000, 255), (0xFFFF_FFFF, true));
    assert_eq!(cpu.shift_reg(3, 0x8000_0001, 32), (0x8000_0001, true));
    assert_eq!(cpu.shift_reg(3, 0x0000_0002, 33), (0x0000_0001, false));
}

#[test]
fn arithmetic_flags_signed_and_unsigned_limits() {
    let mut cpu = Cpu::new();
    assert_eq!(cpu.add_flags(0x7FFF_FFFF, 1, false, true), 0x8000_0000);
    assert!(cpu.flag(FLAG_V) && cpu.flag(FLAG_N) && !cpu.flag(FLAG_C));
    assert_eq!(cpu.add_flags(0xFFFF_FFFF, 1, false, true), 0);
    assert!(cpu.flag(FLAG_Z) && cpu.flag(FLAG_C) && !cpu.flag(FLAG_V));
    // SUB carry = no borrow.
    assert_eq!(cpu.sub_flags(5, 5, true, true), 0);
    assert!(cpu.flag(FLAG_C) && cpu.flag(FLAG_Z));
    assert_eq!(cpu.sub_flags(0, 1, true, true), 0xFFFF_FFFF);
    assert!(!cpu.flag(FLAG_C) && cpu.flag(FLAG_N));
    assert_eq!(cpu.sub_flags(0x8000_0000, 1, true, true), 0x7FFF_FFFF);
    assert!(cpu.flag(FLAG_V) && cpu.flag(FLAG_C));
}

#[test]
fn arm_data_processing_mov_add_and_pc_reads() {
    let (mut cpu, mut bus) = arm(&[
        0xE3A0_0005, // mov r0, #5
        0xE280_1003, // add r1, r0, #3
        0xE1A0_200F, // mov r2, pc         (pc+8)
        0xE3A0_3001, // mov r3, #1
        0xE1A0_4313, // mov r4, r3, lsl r3 (register shift)
        0xE08F_5313, // add r5, pc, r3, lsl r3 → pc+12 + 2
    ]);
    run(&mut cpu, &mut bus, 6);
    assert_eq!(cpu.r[0], 5);
    assert_eq!(cpu.r[1], 8);
    assert_eq!(cpu.r[2], 0x108 + 8);
    assert_eq!(cpu.r[4], 2);
    assert_eq!(cpu.r[5], 0x114 + 12 + 2);
}

#[test]
fn arm_ldr_misaligned_rotates_and_halfword_signed_cases() {
    let (mut cpu, mut bus) = arm(&[
        0xE591_0000, // ldr r0, [r1]
        0xE1D1_20B0, // ldrh r2, [r1]
        0xE1D1_30F0, // ldrsh r3, [r1]
        0xE1D1_40D0, // ldrsb r4, [r1]
    ]);
    bus.put32(0x1000, 0x8899_AABB);
    cpu.r[1] = 0x1001;
    run(&mut cpu, &mut bus, 4);
    assert_eq!(cpu.r[0], 0xBB88_99AA);
    // LDRH misaligned rotates by 8.
    assert_eq!(cpu.r[2], 0xBB00_00AA);
    // LDRSH misaligned behaves as LDRSB.
    assert_eq!(cpu.r[3], 0xFFFF_FFAA);
    assert_eq!(cpu.r[4], 0xFFFF_FFAA);
}

#[test]
fn arm_ldm_stm_writeback_base_in_list_and_empty_list() {
    let (mut cpu, mut bus) = arm(&[
        0xE8A0_000E, // stmia r0!, {r1-r3}
        0xE8B0_0001, // ldmia r0!, {r0}   base in list: no writeback
        0xE92D_0003, // stmdb sp!, {r0, r1}
        0xE8A4_0000, // stmia r4!, {}     empty list: stores pc, +0x40
    ]);
    cpu.r[0] = 0x2000;
    cpu.r[1] = 11;
    cpu.r[2] = 22;
    cpu.r[3] = 33;
    bus.put32(0x200C, 0x1234);
    cpu.r[13] = 0x3000;
    cpu.r[4] = 0x4000;
    run(&mut cpu, &mut bus, 4);
    assert_eq!(bus.get32(0x2000), 11);
    assert_eq!(bus.get32(0x2008), 33);
    assert_eq!(cpu.r[0], 0x1234);
    assert_eq!(cpu.r[13], 0x2FF8);
    assert_eq!(bus.get32(0x2FF8), 0x1234);
    assert_eq!(bus.get32(0x4000), 0x10C + 12);
    assert_eq!(cpu.r[4], 0x4040);
}

#[test]
fn arm_stm_with_base_in_list_stores_original_when_first() {
    let (mut cpu, mut bus) = arm(&[0xE8A0_0003, 0xE8A1_0003]); // stmia r0!,{r0,r1}; stmia r1!,{r0,r1}
    cpu.r[0] = 0x2000;
    cpu.r[1] = 0x3000;
    run(&mut cpu, &mut bus, 1);
    assert_eq!(bus.get32(0x2000), 0x2000);
    assert_eq!(cpu.r[0], 0x2008);
    run(&mut cpu, &mut bus, 1);
    // r1 is not the first register: stores the written-back value.
    assert_eq!(bus.get32(0x3004), 0x3008);
}

#[test]
fn arm_multiply_and_long_multiply() {
    let (mut cpu, mut bus) = arm(&[
        0xE002_0190, // mul r2, r0, r1
        0xE0E5_4190, // smlal r4, r5, r0, r1
        0xE087_6190, // umull r6, r7, r0, r1
    ]);
    cpu.r[0] = 0xFFFF_FFFE; // -2
    cpu.r[1] = 3;
    run(&mut cpu, &mut bus, 3);
    assert_eq!(cpu.r[2], (-6i32) as u32);
    assert_eq!(cpu.r[4], (-6i32) as u32);
    assert_eq!(cpu.r[5], 0xFFFF_FFFF);
    assert_eq!(cpu.r[6], 0xFFFF_FFFA);
    assert_eq!(cpu.r[7], 2);
}

#[test]
fn arm_msr_mode_switch_banks_registers_and_spsr() {
    let (mut cpu, mut bus) = arm(&[
        0xE321_F012, // msr cpsr_c, #0x12 (IRQ)
        0xE3A0_D0AA, // mov sp, #0xAA
        0xE321_F01F, // msr cpsr_c, #0x1F (SYS)
        0xE161_F000, // msr spsr_c, r0 (no SPSR in SYS: ignored)
    ]);
    cpu.r[13] = 0x55;
    run(&mut cpu, &mut bus, 4);
    assert_eq!(cpu.mode(), MODE_SYS);
    assert_eq!(cpu.r[13], 0x55);
    assert_eq!(cpu.banked_sp(MODE_IRQ), 0xAA);
}

#[test]
fn swi_enters_svc_and_movs_pc_lr_returns() {
    let (mut cpu, mut bus) = arm(&[0xEF05_0000]); // swi 0x05 (ARM comment >> 16)
    // SVC vector at 0x08: movs pc, lr
    bus.put32(0x08, 0xE1B0_F00E);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.mode(), MODE_SVC);
    assert_eq!(cpu.r[14], 0x104);
    assert_eq!(cpu.pc(), 0x08);
    assert_eq!(bus.swis, vec![5]);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.mode(), MODE_SYS);
    assert_eq!(cpu.pc(), 0x104);
}

#[test]
fn irq_entry_from_thumb_and_subs_return() {
    let (mut cpu, mut bus) = thumb(&[0x2001, 0x2102]); // movs r0,#1; movs r1,#2
    bus.put32(0x18, 0xE25E_F004); // subs pc, lr, #4
    run(&mut cpu, &mut bus, 1);
    assert!(cpu.irq(&mut bus));
    assert_eq!(cpu.mode(), MODE_IRQ);
    assert!(!cpu.thumb());
    run(&mut cpu, &mut bus, 1);
    assert!(cpu.thumb());
    assert_eq!(cpu.pc(), 0x102);
    run(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.r[1], 2);
}

#[test]
fn undefined_instruction_takes_vector() {
    let (mut cpu, mut bus) = arm(&[0xE600_0010]);
    let r = cpu.step(&mut bus);
    assert_eq!(r, StepResult::Exception);
    assert_eq!(cpu.mode(), MODE_UND);
    assert_eq!(cpu.pc(), 0x04);
    assert_eq!(cpu.r[14], 0x104);
}

#[test]
fn bx_interworking_and_thumb_bl_pair() {
    let (mut cpu, mut bus) = arm(&[0xE12F_FF10]); // bx r0
    cpu.r[0] = 0x201;
    bus.put16(0x200, 0xF000); // bl +0x10 (prefix)
    bus.put16(0x202, 0xF808); // suffix offset 8 halfwords
    run(&mut cpu, &mut bus, 1);
    assert!(cpu.thumb());
    assert_eq!(cpu.pc(), 0x200);
    run(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.pc(), 0x204 + 0x10);
    assert_eq!(cpu.r[14], 0x205);
}

#[test]
fn thumb_formats_alu_hi_and_memory() {
    let (mut cpu, mut bus) = thumb(&[
        0x2005, // movs r0, #5
        0x0041, // lsls r1, r0, #1
        0x1A42, // subs r2, r0, r1
        0x4240, // negs r0, r0
        0x4683, // mov r11, r0
        0x4B02, // ldr r3, [pc, #8]
        0x6019, // str r1, [r3]
        0x681C, // ldr r4, [r3]
        0xB403, // push {r0, r1}
        0xBC30, // pop {r4, r5}
    ]);
    bus.put32(0x114, 0x3000); // literal at ((0x10A+4) & !2) + 8
    cpu.r[13] = 0x4000;
    run(&mut cpu, &mut bus, 10);
    assert_eq!(cpu.r[1], 10);
    assert_eq!(cpu.r[2], (-5i32) as u32);
    assert_eq!(cpu.r[0], (-5i32) as u32);
    assert_eq!(cpu.r[11], (-5i32) as u32);
    assert_eq!(cpu.r[3], 0x3000);
    assert_eq!(bus.get32(0x3000), 10);
    assert_eq!(cpu.r[4], (-5i32) as u32);
    assert_eq!(cpu.r[5], 10);
    assert_eq!(cpu.r[13], 0x4000);
}

#[test]
fn thumb_conditional_branch_and_long_branch() {
    let (mut cpu, mut bus) = thumb(&[
        0x2800, // cmp r0, #0
        0xD001, // beq +2 → 0x108
        0x2101, // movs r1, #1 (skipped)
        0x2102, // movs r1, #2 (skipped)
        0x2203, // movs r2, #3
    ]);
    run(&mut cpu, &mut bus, 3);
    assert_eq!(cpu.r[1], 0);
    assert_eq!(cpu.r[2], 3);
}

#[test]
fn hle_swi_hook_skips_vector() {
    let (mut cpu, mut bus) = thumb(&[0xDF06, 0x2001]); // swi 6; movs r0,#1
    bus.hle = true;
    run(&mut cpu, &mut bus, 2);
    assert_eq!(bus.swis, vec![6]);
    assert_eq!(cpu.mode(), MODE_SYS);
    assert_eq!(cpu.r[0], 1);
}

#[test]
fn data_processing_to_pc_with_s_in_user_mode_faults() {
    let (mut cpu, mut bus) = arm(&[0xE1B0_F00E]); // movs pc, lr in SYS
    let r = cpu.step(&mut bus);
    assert!(matches!(r, StepResult::Fault(Fault::Unpredictable { .. })));
    assert_eq!(cpu.pc(), 0x100);
}

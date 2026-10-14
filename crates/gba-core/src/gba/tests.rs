//! System tests: tiny hand-assembled ARM ROMs driven through `Gba`, covering
//! the BIOS IRQ stub path (HLE-60-02), IntrWait/VBlankIntrWait (HLE-60-03)
//! and HALT with IME=0 (IRQ-57-02).

use super::*;
use crate::cpu::{MODE_SYS, StepResult};

// ------------------------------------------------------------ ARM encoder

fn imm(v: u32) -> u32 {
    (0..16)
        .find_map(|rot| {
            let r = v.rotate_left(rot * 2);
            (r < 256).then_some((rot << 8) | r)
        })
        .expect("encodable immediate")
}
fn mov(rd: u32, v: u32) -> u32 {
    0xE3A0_0000 | rd << 12 | imm(v)
}
fn add(rd: u32, rn: u32, v: u32) -> u32 {
    0xE280_0000 | rn << 16 | rd << 12 | imm(v)
}
fn orr(rd: u32, rn: u32, v: u32) -> u32 {
    0xE380_0000 | rn << 16 | rd << 12 | imm(v)
}
fn ldr(rd: u32, rn: u32, off: u32) -> u32 {
    0xE590_0000 | rn << 16 | rd << 12 | off
}
fn str_(rd: u32, rn: u32, off: u32) -> u32 {
    0xE580_0000 | rn << 16 | rd << 12 | off
}
/// `str rd, [rn, #-off]`
fn str_neg(rd: u32, rn: u32, off: u32) -> u32 {
    0xE500_0000 | rn << 16 | rd << 12 | off
}
fn strb(rd: u32, rn: u32, off: u32) -> u32 {
    0xE5C0_0000 | rn << 16 | rd << 12 | off
}
fn strh(rd: u32, rn: u32, off: u32) -> u32 {
    0xE1C0_00B0 | rn << 16 | rd << 12 | (off >> 4) << 8 | (off & 0xF)
}
fn ldrh(rd: u32, rn: u32, off: u32) -> u32 {
    0xE1D0_00B0 | rn << 16 | rd << 12 | (off >> 4) << 8 | (off & 0xF)
}
fn bx(rm: u32) -> u32 {
    0xE12F_FF10 | rm
}
/// `b target` for an instruction at ROM offset `at`.
fn b(at: u32, target: u32) -> u32 {
    0xEA00_0000 | ((target.wrapping_sub(at + 8) >> 2) & 0x00FF_FFFF)
}
fn swi(n: u32) -> u32 {
    0xEF00_0000 | n << 16
}

const HANDLER: u32 = 0x100;
const COUNTER: u32 = 0x0300_0100;

/// Assemble `main` at ROM offset 0 and `handler` at 0x100.
fn rom(main: &[u32], handler: &[u32]) -> Vec<u8> {
    let mut rom = vec![0u8; 0x400];
    for (base, code) in [(0usize, main), (HANDLER as usize, handler)] {
        for (i, w) in code.iter().enumerate() {
            rom[base + 4 * i..base + 4 * i + 4].copy_from_slice(&w.to_le_bytes());
        }
    }
    rom
}

fn boot(rom: &[u8]) -> Gba {
    Gba::new(GbaConfig {
        rom,
        bios: None,
        backup: Some(BackupKind::None),
        save: None,
    })
}

/// Common setup: install the handler at 0x03007FFC (via the 0x03FFFFFC
/// mirror), enable VBlank IRQ in DISPSTAT and IE, and IME.
fn irq_setup() -> Vec<u32> {
    vec![
        mov(0, 0x0400_0000),
        mov(1, 0x0800_0000),
        add(1, 1, HANDLER),
        str_neg(1, 0, 4),
        mov(1, 8),
        strh(1, 0, 4),
        add(2, 0, 0x200),
        mov(1, 1),
        strh(1, 2, 0),
        strh(1, 2, 8),
    ]
}

/// Handler prologue: acknowledge VBlank in IF.
fn ack_vblank() -> Vec<u32> {
    vec![
        mov(0, 0x0400_0000),
        add(0, 0, 0x200),
        mov(1, 1),
        strh(1, 0, 2),
    ]
}

#[test]
fn vblank_irq_through_bios_stub_counts_frames() {
    let mut main = irq_setup();
    let at = 4 * main.len() as u32;
    main.push(b(at, at)); // idle loop
    let mut handler = ack_vblank();
    handler.extend([
        mov(2, 0x0300_0000),
        ldr(3, 2, 0x100),
        add(3, 3, 1),
        str_(3, 2, 0x100),
        bx(14),
    ]);
    let mut gba = boot(&rom(&main, &handler));
    for _ in 0..10 {
        gba.run_frame().unwrap();
    }
    // The 10th VBlank's IRQ is taken on the next step.
    for _ in 0..40 {
        gba.step().unwrap();
    }
    assert_eq!(gba.bus.peek32(COUNTER), 10);
    assert_eq!(gba.cpu.stats.irq, 10);
    assert_eq!(gba.bus.if_ & 1, 0, "handler acknowledged IF");
    assert_eq!(gba.cpu.mode(), MODE_SYS, "returned from IRQ mode");
    assert!(gba.cpu.pc() >= 0x0800_0000 && gba.cpu.pc() < 0x0800_0000 + HANDLER);
    assert_eq!(gba.cpu.r[13], 0x0300_7F00, "user stack balanced");
    assert_eq!(
        gba.cpu.banked_sp(crate::cpu::MODE_IRQ),
        0x0300_7FA0,
        "IRQ stack balanced"
    );
}

#[test]
fn vblank_intr_wait_halts_between_frames() {
    let mut main = irq_setup();
    let lp = 4 * main.len() as u32;
    main.extend([
        swi(0x05),
        mov(2, 0x0300_0000),
        ldr(3, 2, 0x100),
        add(3, 3, 1),
        str_(3, 2, 0x100),
    ]);
    let at = 4 * main.len() as u32;
    main.push(b(at, lp));
    let mut handler = ack_vblank();
    handler.extend([
        mov(2, 0x0300_0000),
        add(2, 2, 0x7F00),
        ldrh(3, 2, 0xF8),
        orr(3, 3, 1),
        strh(3, 2, 0xF8),
        bx(14),
    ]);
    let mut gba = boot(&rom(&main, &handler));
    for _ in 0..10 {
        gba.run_frame().unwrap();
    }
    for _ in 0..60 {
        gba.step().unwrap();
    }
    assert_eq!(gba.bus.peek32(COUNTER), 10, "one loop iteration per VBlank");
    // First wait + (re-execution that returns, next wait) per VBlank.
    assert_eq!(gba.bus.hle.calls[&0x05], 21);
    // Almost all time is spent halted: a busy loop would run ~10^5 steps.
    assert!(gba.instructions < 1000, "{} instructions", gba.instructions);
    assert_eq!(
        gba.bus.peek16(0x0300_7FF8) & 1,
        0,
        "flag consumed by IntrWait"
    );
}

#[test]
fn halt_with_ime_zero_wakes_without_taking_the_irq() {
    let main = [
        mov(0, 0x0400_0000),
        mov(1, 8),
        strh(1, 0, 4),
        add(2, 0, 0x200),
        mov(1, 1),
        strh(1, 2, 0), // IE = VBlank, IME stays 0
        mov(1, 0),
        strb(1, 2, 0x101), // HALTCNT
        mov(3, 0x0300_0000),
        mov(1, 0x55),
        str_(1, 3, 0),
        b(0x2C, 0x2C),
    ];
    let mut gba = boot(&rom(&main, &[]));
    for _ in 0..8 {
        gba.step().unwrap();
    }
    assert!(gba.cpu.halted);
    let mut guard = 0;
    while gba.bus.peek32(0x0300_0000) != 0x55 {
        gba.step().unwrap();
        guard += 1;
        assert!(guard < 10_000, "never woke up");
    }
    assert!(gba.bus.cycles >= 160 * crate::bus::CYCLES_PER_LINE);
    assert_eq!(gba.cpu.stats.irq, 0, "IME=0: no IRQ exception");
    assert_eq!(gba.bus.if_ & 1, 1, "VBlank still pending");
}

#[test]
fn unsupported_swi_stops_the_system() {
    let main = [mov(0, 0), swi(0x2A)];
    let mut gba = boot(&rom(&main, &[]));
    gba.step().unwrap();
    assert_eq!(
        gba.step(),
        Err(SystemFault::UnsupportedSwi {
            number: 0x2A,
            pc: 0x0800_0004
        })
    );
}

#[test]
fn hle_post_boot_state() {
    let gba = boot(&rom(&[], &[]));
    assert_eq!(gba.cpu.pc(), 0x0800_0000);
    assert_eq!(gba.cpu.mode(), MODE_SYS);
    assert_eq!(gba.cpu.r[13], 0x0300_7F00);
    let mut gba = gba;
    assert_eq!(gba.bus.peek16(0x0400_0088), 0x0200);
    assert_eq!(gba.bus.peek8(0x0400_0300), 1);
    // The framebuffer hash is stable for an idle ROM.
    let main = [b(0, 0)];
    let mut a = boot(&rom(&main, &[]));
    let mut c = boot(&rom(&main, &[]));
    a.run_frame().unwrap();
    c.run_frame().unwrap();
    assert_eq!(a.frame_hash(), c.frame_hash());
    assert!(matches!(
        a.cpu.step(&mut a.bus),
        StepResult::Retired | StepResult::Skipped
    ));
}

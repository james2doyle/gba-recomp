//! SPEC-060 acceptance for the HLE BIOS: arithmetic, memory copy,
//! decompression (with write widths), affine helpers, RAM reset, IntrWait
//! and the unsupported-SWI fault.

use super::*;
use crate::bus::tests::test_bus;

fn setup() -> (Bus, Cpu) {
    let bus = test_bus();
    let mut cpu = Cpu::post_boot(0x0800_0000);
    cpu.current_pc = 0x0800_0100;
    (bus, cpu)
}

fn call(bus: &mut Bus, cpu: &mut Cpu, number: u8, regs: [u32; 4]) {
    cpu.r[..4].copy_from_slice(&regs);
    swi(bus, cpu, number);
}

fn put_bytes(bus: &mut Bus, addr: u32, bytes: &[u8]) {
    for (i, b) in bytes.iter().enumerate() {
        bus.poke8(addr + i as u32, *b);
    }
}

fn get_bytes(bus: &mut Bus, addr: u32, len: u32) -> Vec<u8> {
    (0..len).map(|i| bus.peek8(addr + i)).collect()
}

const EW: u32 = 0x0200_0000;
const IW: u32 = 0x0300_0000;

// ------------------------------------------------------------ arithmetic

#[test]
fn div_and_divarm_signs() {
    let (mut bus, mut cpu) = setup();
    for (num, den, q, r) in [
        (7i32, 2i32, 3i32, 1i32),
        (-7, 2, -3, -1),
        (7, -2, -3, 1),
        (-7, -2, 3, -1),
    ] {
        call(&mut bus, &mut cpu, 0x06, [num as u32, den as u32, 0, 0]);
        assert_eq!(
            (cpu.r[0] as i32, cpu.r[1] as i32, cpu.r[3]),
            (q, r, q.unsigned_abs()),
            "Div {num}/{den}"
        );
        call(&mut bus, &mut cpu, 0x07, [den as u32, num as u32, 0, 0]);
        assert_eq!(
            (cpu.r[0] as i32, cpu.r[1] as i32),
            (q, r),
            "DivArm {num}/{den}"
        );
    }
    call(
        &mut bus,
        &mut cpu,
        0x06,
        [i32::MIN as u32, -1i32 as u32, 0, 0],
    );
    assert_eq!((cpu.r[0], cpu.r[1]), (i32::MIN as u32, 0), "overflow wraps");
}

#[test]
fn sqrt() {
    let (mut bus, mut cpu) = setup();
    for (v, root) in [
        (0u32, 0u32),
        (1, 1),
        (15, 3),
        (16, 4),
        (1 << 30, 1 << 15),
        (u32::MAX, 0xFFFF),
    ] {
        call(&mut bus, &mut cpu, 0x08, [v, 0, 0, 0]);
        assert_eq!(cpu.r[0], root, "sqrt({v})");
    }
}

#[test]
fn arctan_matches_gbatek_polynomial() {
    let (mut bus, mut cpu) = setup();
    // Values from an independent evaluation of the GBATEK formula.
    for (t, expected) in [
        (0i32, 0i32),
        (0x1000, 2555),
        (0x2000, 4836),
        (0x4000, 0x2000),
        (-0x4000, -0x2000),
        (-0x2000, -4836),
    ] {
        call(&mut bus, &mut cpu, 0x09, [t as u32, 0, 0, 0]);
        assert_eq!(cpu.r[0] as i32, expected, "ArcTan({t:#x})");
    }
}

#[test]
fn arctan2_quadrants() {
    let (mut bus, mut cpu) = setup();
    for (x, y, expected) in [
        (1i32, 0i32, 0u32),
        (-1, 0, 0x8000),
        (0, 1, 0x4000),
        (0, -1, 0xC000),
        (0, 0, 0),
        (256, 256, 0x2000),
        (-256, 256, 0x6000),
        (-256, -256, 0xA000),
        (256, -256, 0xE000),
        (256, 128, 0x12E4),
        (100, 300, 0x32E5),
        (-300, -100, 0x8D1B),
    ] {
        call(&mut bus, &mut cpu, 0x0A, [x as u32, y as u32, 0, 0]);
        assert_eq!(cpu.r[0], expected, "ArcTan2({x}, {y})");
    }
}

#[test]
fn misc_simple_swis() {
    let (mut bus, mut cpu) = setup();
    call(&mut bus, &mut cpu, 0x0D, [0; 4]);
    assert_eq!(cpu.r[0], 0xBAAE_187F);
    call(&mut bus, &mut cpu, 0x02, [0; 4]);
    assert!(bus.halt_requested);
    call(&mut bus, &mut cpu, 0x27, [0, 0, 0x80, 0]); // CustomHalt → STOP
    assert!(bus.stop_requested);
    call(&mut bus, &mut cpu, 0x19, [0, 0, 0, 0]); // SoundBias down
    assert_eq!(bus.peek16(0x0400_0088) & 0x3FE, 0);
    call(&mut bus, &mut cpu, 0x19, [1, 0, 0, 0]);
    assert_eq!(bus.peek16(0x0400_0088) & 0x3FE, 0x200);
    call(&mut bus, &mut cpu, 0x25, [0; 4]); // MultiBoot: documented failure
    assert_eq!(cpu.r[0], 1);
    assert_eq!(bus.hle.calls[&0x25], 1);
}

#[test]
fn unsupported_swi_faults_with_number_and_pc() {
    let (mut bus, mut cpu) = setup();
    for number in [0x1A, 0x1E, 0x20, 0x26, 0x2A, 0xFF] {
        bus.fault = None;
        call(&mut bus, &mut cpu, number, [0; 4]);
        assert_eq!(
            bus.fault,
            Some(SystemFault::UnsupportedSwi {
                number,
                pc: 0x0800_0100
            })
        );
    }
    for &number in IMPLEMENTED_SWIS {
        if number == 0x00 {
            continue; // SoftReset redirects the CPU; covered elsewhere.
        }
        let (mut bus, mut cpu) = setup();
        call(&mut bus, &mut cpu, number, [EW, EW + 0x1000, 0, 0]);
        assert_eq!(bus.fault, None, "SWI {number:#x}");
    }
}

// ------------------------------------------------------------ CpuSet

#[test]
fn cpu_set_copy_and_fill_16_and_32() {
    let (mut bus, mut cpu) = setup();
    for i in 0..8u32 {
        bus.poke32(EW + 4 * i, 0x1111_1111 * (i + 1));
    }
    call(&mut bus, &mut cpu, 0x0B, [EW, IW, 3, 0]);
    assert_eq!(
        get_bytes(&mut bus, IW, 8),
        [0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0, 0]
    );
    call(&mut bus, &mut cpu, 0x0B, [EW, IW + 0x100, 3 | 1 << 26, 0]);
    let words: Vec<u32> = (0..4).map(|i| bus.peek32(IW + 0x100 + 4 * i)).collect();
    assert_eq!(words, [0x1111_1111, 0x2222_2222, 0x3333_3333, 0]);
    bus.poke16(EW + 0x80, 0xABCD);
    call(
        &mut bus,
        &mut cpu,
        0x0B,
        [EW + 0x80, IW + 0x200, 3 | 1 << 24, 0],
    );
    let halves: Vec<u16> = (0..4).map(|i| bus.peek16(IW + 0x200 + 2 * i)).collect();
    assert_eq!(halves, [0xABCD, 0xABCD, 0xABCD, 0]);
    call(
        &mut bus,
        &mut cpu,
        0x0B,
        [EW + 4, IW + 0x300, 2 | 1 << 24 | 1 << 26, 0],
    );
    let words: Vec<u32> = (0..3).map(|i| bus.peek32(IW + 0x300 + 4 * i)).collect();
    assert_eq!(words, [0x2222_2222, 0x2222_2222, 0]);
}

#[test]
fn cpu_fast_set_rounds_up_to_eight_words() {
    let (mut bus, mut cpu) = setup();
    for i in 0..20u32 {
        bus.poke32(EW + 4 * i, i + 1);
    }
    call(&mut bus, &mut cpu, 0x0C, [EW, IW, 3, 0]);
    let words: Vec<u32> = (0..9).map(|i| bus.peek32(IW + 4 * i)).collect();
    assert_eq!(words, [1, 2, 3, 4, 5, 6, 7, 8, 0]);
    call(
        &mut bus,
        &mut cpu,
        0x0C,
        [EW + 8, IW + 0x100, 9 | 1 << 24, 0],
    );
    let words: Vec<u32> = (0..17).map(|i| bus.peek32(IW + 0x100 + 4 * i)).collect();
    assert!(words[..16].iter().all(|&w| w == 3));
    assert_eq!(words[16], 0);
}

// ------------------------------------------------------------ decompression

const LZ77: [u8; 11] = [0x10, 10, 0, 0, 0x10, b'A', b'B', b'C', 0x30, 0x02, b'X'];

#[test]
fn lz77_wram_writes_bytes() {
    let (mut bus, mut cpu) = setup();
    put_bytes(&mut bus, EW, &LZ77);
    bus.poke8(EW + 0x1000 + 11, 0xEE);
    // Odd destination: byte writes must not touch neighbours.
    call(&mut bus, &mut cpu, 0x11, [EW, EW + 0x1001, 0, 0]);
    assert_eq!(get_bytes(&mut bus, EW + 0x1001, 10), b"ABCABCABCX");
    assert_eq!(bus.peek8(EW + 0x1000), 0);
    assert_eq!(bus.peek8(EW + 0x1000 + 11), 0xEE);
}

#[test]
fn lz77_vram_variant_writes_halfwords() {
    let (mut bus, mut cpu) = setup();
    put_bytes(&mut bus, EW, &LZ77);
    call(&mut bus, &mut cpu, 0x12, [EW, 0x0600_0000, 0, 0]);
    assert_eq!(get_bytes(&mut bus, 0x0600_0000, 10), b"ABCABCABCX");
    // The WRAM variant on VRAM shows why the width matters: each byte
    // write is duplicated into the halfword (BUS-57-02).
    call(&mut bus, &mut cpu, 0x11, [EW, 0x0600_0100, 0, 0]);
    assert_eq!(bus.peek16(0x0600_0100), u16::from_le_bytes(*b"BB"));
}

#[test]
fn run_length_wram_and_vram() {
    let (mut bus, mut cpu) = setup();
    put_bytes(&mut bus, EW, &[0x30, 7, 0, 0, 0x82, b'Z', 0x01, b'a', b'b']);
    call(&mut bus, &mut cpu, 0x14, [EW, EW + 0x101, 0, 0]);
    assert_eq!(get_bytes(&mut bus, EW + 0x101, 7), b"ZZZZZab");
    call(&mut bus, &mut cpu, 0x15, [EW, 0x0600_0000, 0, 0]);
    assert_eq!(get_bytes(&mut bus, 0x0600_0000, 7), b"ZZZZZab");
}

#[test]
fn huffman_8bit_and_4bit() {
    let (mut bus, mut cpu) = setup();
    // Tree: root (both children leaves) → 'a' (bit 0), 'b' (bit 1).
    put_bytes(&mut bus, EW, &[0x28, 4, 0, 0, 1, 0xC0, b'a', b'b']);
    bus.poke32(EW + 8, 0x6000_0000); // bits 0,1,1,0
    call(&mut bus, &mut cpu, 0x13, [EW, IW, 0, 0]);
    assert_eq!(get_bytes(&mut bus, IW, 4), b"abba");

    // 4-bit symbols, lower nibble first.
    put_bytes(&mut bus, EW + 0x100, &[0x24, 2, 0, 0, 1, 0xC0, 1, 2]);
    bus.poke32(EW + 0x108, 0x6000_0000); // nibbles 1,2,2,1
    call(&mut bus, &mut cpu, 0x13, [EW + 0x100, IW + 0x100, 0, 0]);
    assert_eq!(bus.peek32(IW + 0x100), 0x0000_1221);
}

#[test]
fn diff_filters() {
    let (mut bus, mut cpu) = setup();
    put_bytes(&mut bus, EW, &[0x81, 4, 0, 0, 1, 1, 1, 253]);
    call(&mut bus, &mut cpu, 0x16, [EW, EW + 0x101, 0, 0]);
    assert_eq!(get_bytes(&mut bus, EW + 0x101, 4), [1, 2, 3, 0]);
    call(&mut bus, &mut cpu, 0x17, [EW, 0x0600_0000, 0, 0]);
    assert_eq!(get_bytes(&mut bus, 0x0600_0000, 4), [1, 2, 3, 0]);
    put_bytes(
        &mut bus,
        EW + 0x200,
        &[0x82, 4, 0, 0, 0x00, 0x01, 0x01, 0x00],
    );
    call(&mut bus, &mut cpu, 0x18, [EW + 0x200, IW, 0, 0]);
    assert_eq!((bus.peek16(IW), bus.peek16(IW + 2)), (0x0100, 0x0101));
}

#[test]
fn bit_unpack_offsets_and_zero_flag() {
    let (mut bus, mut cpu) = setup();
    bus.poke8(EW, 0b11_10_01_00);
    let info = EW + 0x100;
    bus.poke16(info, 1);
    bus.poke8(info + 2, 2);
    bus.poke8(info + 3, 8);
    bus.poke32(info + 4, 1);
    call(&mut bus, &mut cpu, 0x10, [EW, IW, info, 0]);
    assert_eq!(bus.peek32(IW), 0x0403_0200, "zero stays zero");
    bus.poke32(info + 4, 0x8000_0001);
    call(&mut bus, &mut cpu, 0x10, [EW, IW, info, 0]);
    assert_eq!(bus.peek32(IW), 0x0403_0201);
    // 8 → 32 bits.
    put_bytes(&mut bus, EW, &[5, 6]);
    bus.poke16(info, 2);
    bus.poke8(info + 2, 8);
    bus.poke8(info + 3, 32);
    bus.poke32(info + 4, 0);
    call(&mut bus, &mut cpu, 0x10, [EW, IW + 0x10, info, 0]);
    assert_eq!((bus.peek32(IW + 0x10), bus.peek32(IW + 0x14)), (5, 6));
}

// ------------------------------------------------------------ affine

#[test]
fn bg_affine_set_identity_and_quarter_turn() {
    let (mut bus, mut cpu) = setup();
    for (n, theta) in [(0u32, 0u16), (1, 0x4000)] {
        let s = EW + 20 * n;
        bus.poke32(s, 0x1000);
        bus.poke32(s + 4, 0x2000);
        bus.poke16(s + 8, 10);
        bus.poke16(s + 10, 20);
        bus.poke16(s + 12, 0x100);
        bus.poke16(s + 14, 0x100);
        bus.poke16(s + 16, theta);
    }
    call(&mut bus, &mut cpu, 0x0E, [EW, IW, 2, 0]);
    let m: Vec<u16> = (0..4).map(|i| bus.peek16(IW + 2 * i)).collect();
    assert_eq!(m, [0x100, 0, 0, 0x100]);
    assert_eq!(bus.peek32(IW + 8), 0x1000 - 10 * 0x100);
    assert_eq!(bus.peek32(IW + 12), 0x2000 - 20 * 0x100);
    let m: Vec<u16> = (0..4).map(|i| bus.peek16(IW + 16 + 2 * i)).collect();
    assert_eq!(m, [0, 0xFF00, 0x100, 0]);
    // x = ox - (pa*cx + pb*cy) = 0x1000 + 20*0x100.
    assert_eq!(bus.peek32(IW + 24), 0x1000 + 20 * 0x100);
    assert_eq!(bus.peek32(IW + 28), 0x2000 - 10 * 0x100);
}

#[test]
fn obj_affine_set_strides() {
    let (mut bus, mut cpu) = setup();
    bus.poke16(EW, 0x100);
    bus.poke16(EW + 2, 0x200);
    bus.poke16(EW + 4, 0);
    bus.poke16(EW + 8, 0x100);
    bus.poke16(EW + 10, 0x100);
    bus.poke16(EW + 12, 0x8000); // 180°
    call(&mut bus, &mut cpu, 0x0F, [EW, IW, 2, 2]);
    let m: Vec<u16> = (0..8).map(|i| bus.peek16(IW + 2 * i)).collect();
    assert_eq!(m, [0x100, 0, 0, 0x200, 0xFF00, 0, 0, 0xFF00]);
    // Stride 8 straight into OAM.
    call(&mut bus, &mut cpu, 0x0F, [EW, 0x0700_0006, 1, 8]);
    let m: Vec<u16> = [6, 14, 22, 30]
        .iter()
        .map(|o| bus.peek16(0x0700_0000 + o))
        .collect();
    assert_eq!(m, [0x100, 0, 0, 0x200]);
}

// ------------------------------------------------------------ RAM reset

fn dirty_everything(bus: &mut Bus) {
    bus.ewram.fill(0x55);
    bus.iwram.fill(0x55);
    bus.ppu.palette.fill(0x55);
    bus.ppu.vram.fill(0x55);
    bus.ppu.oam.fill(0x55);
    bus.poke16(0x0400_0200, 0x3FFF);
    bus.poke16(0x0400_0208, 1);
    bus.poke16(0x0400_0008, 0x1234);
}

#[test]
fn register_ram_reset_flags() {
    let (mut bus, mut cpu) = setup();
    dirty_everything(&mut bus);
    call(&mut bus, &mut cpu, 0x01, [0x01, 0, 0, 0]);
    assert!(bus.ewram.iter().all(|&b| b == 0));
    assert!(bus.iwram.iter().all(|&b| b == 0x55));
    assert_eq!(bus.peek16(0x0400_0000), 0x0080, "always forced blank");

    dirty_everything(&mut bus);
    call(&mut bus, &mut cpu, 0x01, [0x02, 0, 0, 0]);
    assert!(bus.iwram[..0x7E00].iter().all(|&b| b == 0));
    assert!(
        bus.iwram[0x7E00..].iter().all(|&b| b == 0x55),
        "top 0x200 kept"
    );
    assert!(bus.ewram.iter().all(|&b| b == 0x55));

    dirty_everything(&mut bus);
    call(&mut bus, &mut cpu, 0x01, [0x1C, 0, 0, 0]);
    assert!(bus.ppu.palette.iter().all(|&b| b == 0));
    assert!(bus.ppu.vram.iter().all(|&b| b == 0));
    assert!(bus.ppu.oam.iter().all(|&b| b == 0));
    assert_eq!(bus.ie, 0x3FFF, "registers untouched without bit 7");

    dirty_everything(&mut bus);
    bus.request_irq(0x0001);
    call(&mut bus, &mut cpu, 0x01, [0x80, 0, 0, 0]);
    assert_eq!((bus.ie, bus.if_, bus.ime), (0, 0, 0));
    assert_eq!(bus.peek16(0x0400_0008), 0);
    assert!(bus.ewram.iter().all(|&b| b == 0x55));
}

#[test]
fn soft_reset_returns_to_rom_with_default_stacks() {
    let (mut bus, mut cpu) = setup();
    bus.poke32(0x0300_7F00, 0xFFFF_FFFF);
    cpu.r[5] = 99;
    call(&mut bus, &mut cpu, 0x00, [0; 4]);
    assert_eq!(cpu.r[15], 0x0800_0000);
    assert_eq!(cpu.mode(), MODE_SYS);
    assert_eq!(cpu.r[13], 0x0300_7F00);
    assert_eq!(cpu.r[5], 0);
    assert_eq!(cpu.banked_sp(MODE_IRQ), 0x0300_7FA0);
    assert_eq!(cpu.banked_sp(MODE_SVC), 0x0300_7FE0);
    assert_eq!(bus.peek32(0x0300_7F00), 0);
}

// ------------------------------------------------------------ IntrWait

const FLAGS: u32 = 0x0300_7FF8;

#[test]
fn intr_wait_halts_and_reexecutes_until_flag_set() {
    let (mut bus, mut cpu) = setup();
    // Flag already set + discard: the old flag is dropped and we wait.
    bus.poke16(FLAGS, 0x0001);
    call(&mut bus, &mut cpu, 0x04, [1, 0x0001, 0, 0]);
    assert!(bus.halt_requested);
    assert_eq!(bus.ime, 1, "IntrWait enables IME");
    assert_eq!(cpu.r[15], cpu.current_pc, "SWI re-executes after the IRQ");
    assert_eq!(bus.peek16(FLAGS), 0);

    // Woken by an unrelated IRQ: re-executes without discarding, waits again.
    bus.halt_requested = false;
    bus.poke16(FLAGS, 0x0002);
    call(&mut bus, &mut cpu, 0x04, [1, 0x0001, 0, 0]);
    assert!(bus.halt_requested);
    assert_eq!(bus.peek16(FLAGS), 0x0002, "other flags are not discarded");

    // The handler set the flag: return and acknowledge only that bit.
    bus.halt_requested = false;
    bus.poke16(FLAGS, 0x0003);
    cpu.r[15] = 0x0800_0104;
    call(&mut bus, &mut cpu, 0x04, [1, 0x0001, 0, 0]);
    assert!(!bus.halt_requested);
    assert_eq!(cpu.r[15], 0x0800_0104);
    assert_eq!(bus.peek16(FLAGS), 0x0002);
}

#[test]
fn intr_wait_without_discard_returns_immediately() {
    let (mut bus, mut cpu) = setup();
    bus.poke16(FLAGS, 0x0004);
    call(&mut bus, &mut cpu, 0x04, [0, 0x0004, 0, 0]);
    assert!(!bus.halt_requested);
    assert_eq!(bus.peek16(FLAGS), 0);
}

#[test]
fn vblank_intr_wait_sets_arguments() {
    let (mut bus, mut cpu) = setup();
    call(&mut bus, &mut cpu, 0x05, [7, 7, 0, 0]);
    assert_eq!((cpu.r[0], cpu.r[1]), (1, 1));
    assert!(bus.halt_requested);
}

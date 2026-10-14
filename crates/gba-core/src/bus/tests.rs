//! SPEC-057 acceptance matrix: memory map, MMIO, waitstates, IRQ, DMA,
//! timers and keypad, exercised directly on `Bus` (no CPU).

use super::*;
use crate::backup::{Backup, BackupKind};
use crate::cpu::CpuBus;

pub(crate) fn test_bus_with(rom: &[u8], kind: BackupKind) -> Bus {
    Bus::new(rom, None, Backup::new(kind, None))
}

pub(crate) fn test_bus() -> Bus {
    test_bus_with(&[0u8; 0x400], BackupKind::None)
}

/// Advance the scheduler (no CPU) to `target`, running DMAs as they are
/// requested, the same way `Gba::step` services the bus.
pub(crate) fn run_until(bus: &mut Bus, target: u64) {
    while bus.cycles < target {
        let next = bus.next_event().min(target);
        if next > bus.cycles {
            bus.cycles = next;
        }
        bus.process_events();
        while bus.dma_pending() {
            bus.run_dmas();
            bus.process_events();
        }
    }
}

const IO: u32 = 0x0400_0000;

fn cost<T>(bus: &mut Bus, f: impl FnOnce(&mut Bus) -> T) -> u64 {
    let before = bus.cycles;
    f(bus);
    bus.cycles - before
}

// ------------------------------------------------------------ BUS-57-01

#[test]
fn ewram_iwram_mirrors() {
    let mut bus = test_bus();
    bus.write32(0x0200_0010, 0x1122_3344, false);
    assert_eq!(bus.read32(0x0204_0010, false), 0x1122_3344);
    assert_eq!(bus.read32(0x02FC_0010, false), 0x1122_3344);
    bus.write32(0x0300_0020, 0x5566_7788, false);
    assert_eq!(bus.read32(0x0300_8020, false), 0x5566_7788);
    assert_eq!(bus.read32(0x03FF_8020, false), 0x5566_7788);
    // 0x03FFFFxx is the usual alias of the top of IWRAM.
    bus.write32(0x0300_7FFC, 0xCAFE_F00D, false);
    assert_eq!(bus.read32(0x03FF_FFFC, false), 0xCAFE_F00D);
}

#[test]
fn palette_vram_oam_mirrors() {
    let mut bus = test_bus();
    bus.write16(0x0500_0004, 0x1234, false);
    assert_eq!(bus.read16(0x0500_0404, false), 0x1234);
    assert_eq!(bus.read16(0x05FF_FC04, false), 0x1234);
    // VRAM: 0x18000-0x1FFFF mirrors 0x10000-0x17FFF; 128 KiB window repeats.
    bus.write16(0x0601_0002, 0xBEEF, false);
    assert_eq!(bus.read16(0x0601_8002, false), 0xBEEF);
    bus.write16(0x0600_0010, 0x4321, false);
    assert_eq!(bus.read16(0x0602_0010, false), 0x4321);
    bus.write16(0x0700_0008, 0x0F0F, false);
    assert_eq!(bus.read16(0x0700_0408, false), 0x0F0F);
}

#[test]
fn rom_mirrors_and_out_of_range_reads() {
    let mut rom = vec![0u8; 0x400];
    rom[0x10..0x14].copy_from_slice(&0xA1B2_C3D4u32.to_le_bytes());
    let mut bus = test_bus_with(&rom, BackupKind::None);
    for base in [0x0800_0000, 0x0A00_0000, 0x0C00_0000] {
        assert_eq!(bus.read32(base + 0x10, false), 0xA1B2_C3D4);
    }
    // Past the end of the ROM the cartridge bus returns address/2.
    let addr = 0x0800_1000u32;
    assert_eq!(bus.read16(addr, false), (addr >> 1) as u16);
}

#[test]
fn sram_mirror_and_8bit_bus() {
    let mut bus = test_bus_with(&[0u8; 0x400], BackupKind::Sram);
    bus.write8(0x0E00_0010, 0x5A, false);
    assert_eq!(bus.read8(0x0E00_8010, false), 0x5A);
    assert_eq!(bus.read8(0x0F00_0010, false), 0x5A);
    // Wider reads replicate the byte.
    assert_eq!(bus.read16(0x0E00_0010, false), 0x5A5A);
    assert_eq!(bus.read32(0x0E00_0010, false), 0x5A5A_5A5A);
    assert!(bus.backup.dirty);
}

#[test]
fn bios_reads_outside_bios_return_last_fetched_opcode() {
    let mut bus = test_bus();
    bus.pc_in_bios = true;
    let reset_vector = bus.read32(0x0000_0000, false);
    assert_eq!(reset_vector, 0xEA00_001E);
    bus.pc_in_bios = false;
    // Protected: any BIOS read returns the latched opcode.
    assert_eq!(bus.read32(0x0000_0128, false), reset_vector);
    assert_eq!(bus.read16(0x0000_0002, false), (reset_vector >> 16) as u16);
}

#[test]
fn open_bus_reads_return_last_prefetch() {
    let mut rom = vec![0u8; 0x400];
    rom[0..4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    let mut bus = test_bus_with(&rom, BackupKind::None);
    bus.fetch32(0x0800_0000, false);
    assert_eq!(bus.read32(0x0100_0000, false), 0xDEAD_BEEF);
    assert_eq!(bus.read16(0x0100_0002, false), 0xDEAD);
    assert_eq!(bus.read32(0x1000_0000, false), 0xDEAD_BEEF);
    assert_eq!(bus.read32(0x0000_4000, false), 0xDEAD_BEEF);
    // I/O beyond 0x04000400 is unmapped too.
    assert_eq!(bus.read32(0x0400_0400, false), 0xDEAD_BEEF);
    // Thumb fetches replicate the halfword.
    rom[0..4].copy_from_slice(&0x0000_4770u32.to_le_bytes());
    let mut bus = test_bus_with(&rom, BackupKind::None);
    bus.fetch16(0x0800_0000, false);
    assert_eq!(bus.read32(0x0100_0000, false), 0x4770_4770);
}

// ------------------------------------------------------------ BUS-57-02/03

#[test]
fn palette_and_bg_vram_byte_writes_duplicate() {
    let mut bus = test_bus();
    bus.write8(0x0500_0011, 0xAB, false);
    assert_eq!(bus.read16(0x0500_0010, false), 0xABAB);
    bus.write8(0x0600_0001, 0x12, false);
    assert_eq!(bus.read16(0x0600_0000, false), 0x1212);
}

#[test]
fn oam_and_obj_vram_byte_writes_are_ignored() {
    let mut bus = test_bus();
    bus.write16(0x0700_0000, 0x1111, false);
    bus.write8(0x0700_0000, 0x22, false);
    assert_eq!(bus.read16(0x0700_0000, false), 0x1111);
    // Mode 0: OBJ VRAM starts at 0x10000.
    bus.write8(0x0601_0000, 0x34, false);
    assert_eq!(bus.read16(0x0601_0000, false), 0);
    bus.write8(0x0600_FFFE, 0x56, false);
    assert_eq!(bus.read16(0x0600_FFFE, false), 0x5656);
    // Bitmap modes: the BG area extends to 0x14000.
    bus.poke16(IO, 0x0403);
    bus.write8(0x0601_0000, 0x34, false);
    assert_eq!(bus.read16(0x0601_0000, false), 0x3434);
    bus.write8(0x0601_4000, 0x78, false);
    assert_eq!(bus.read16(0x0601_4000, false), 0);
}

#[test]
fn wide_accesses_are_aligned() {
    let mut bus = test_bus();
    bus.write32(0x0200_0000, 0x4433_2211, false);
    assert_eq!(bus.read16(0x0200_0001, false), 0x2211);
    assert_eq!(bus.read32(0x0200_0002, false), 0x4433_2211);
    bus.write32(0x0200_0006, 0x8877_6655, false);
    assert_eq!(bus.read32(0x0200_0004, false), 0x8877_6655);
    bus.write16(0x0200_0009, 0xBBAA, false);
    assert_eq!(bus.read16(0x0200_0008, false), 0xBBAA);
}

// ------------------------------------------------------------ BUS-57-04

#[test]
fn fixed_region_access_costs() {
    let mut bus = test_bus();
    assert_eq!(cost(&mut bus, |b| b.read32(0x0300_0000, false)), 1);
    assert_eq!(cost(&mut bus, |b| b.read16(0x0200_0000, false)), 3);
    assert_eq!(cost(&mut bus, |b| b.read32(0x0200_0000, false)), 6);
    assert_eq!(cost(&mut bus, |b| b.read16(0x0500_0000, false)), 1);
    assert_eq!(cost(&mut bus, |b| b.read32(0x0600_0000, false)), 2);
    assert_eq!(cost(&mut bus, |b| b.read32(0x0700_0000, false)), 1);
    assert_eq!(cost(&mut bus, |b| b.read32(0x0400_0000, false)), 1);
}

#[test]
fn waitcnt_changes_game_pak_costs_at_runtime() {
    let mut bus = test_bus();
    // Reset: WS0 N=4, S=2 (+1 each); WS1 S=4; WS2 S=8; SRAM 4.
    assert_eq!(cost(&mut bus, |b| b.read16(0x0800_0000, false)), 5);
    assert_eq!(cost(&mut bus, |b| b.read16(0x0800_0002, true)), 3);
    assert_eq!(cost(&mut bus, |b| b.read32(0x0800_0000, false)), 8);
    assert_eq!(cost(&mut bus, |b| b.read32(0x0800_0004, true)), 6);
    assert_eq!(cost(&mut bus, |b| b.read16(0x0A00_0002, true)), 5);
    assert_eq!(cost(&mut bus, |b| b.read16(0x0C00_0002, true)), 9);
    assert_eq!(cost(&mut bus, |b| b.read8(0x0E00_0000, false)), 5);
    // WS0 N=3, S=1; SRAM 8 waitstates.
    bus.write16(IO + 0x204, 0x0017, false);
    assert_eq!(bus.read16(IO + 0x204, false), 0x0017);
    assert_eq!(cost(&mut bus, |b| b.read16(0x0800_0000, false)), 4);
    assert_eq!(cost(&mut bus, |b| b.read16(0x0800_0002, true)), 2);
    assert_eq!(cost(&mut bus, |b| b.read32(0x0800_0000, false)), 6);
    assert_eq!(cost(&mut bus, |b| b.read8(0x0E00_0000, false)), 9);
    // Mirrors of WS0 in 0x09xxxxxx share the setting.
    assert_eq!(cost(&mut bus, |b| b.read16(0x0900_0000, false)), 4);
}

#[test]
fn prefetch_makes_sequential_rom_fetches_one_cycle() {
    let mut bus = test_bus();
    assert_eq!(cost(&mut bus, |b| b.fetch16(0x0800_0002, true)), 3);
    bus.write16(IO + 0x204, 0x4000, false);
    assert_eq!(cost(&mut bus, |b| b.fetch16(0x0800_0002, true)), 1);
    assert_eq!(cost(&mut bus, |b| b.fetch32(0x0800_0004, true)), 1);
    // Non-sequential fetches still pay the full N cost.
    assert_eq!(cost(&mut bus, |b| b.fetch16(0x0800_0000, false)), 5);
}

// ------------------------------------------------------------ IRQ-57-01/02

#[test]
fn if_is_write_one_to_clear_including_bytes() {
    let mut bus = test_bus();
    bus.request_irq(0x010F);
    bus.write16(IO + 0x202, 0x0005, false);
    assert_eq!(bus.read16(IO + 0x202, false), 0x010A);
    bus.write8(IO + 0x202, 0x02, false);
    assert_eq!(bus.if_, 0x0108);
    bus.write8(IO + 0x203, 0x01, false);
    assert_eq!(bus.if_, 0x0008);
    // Writing zero bits never clears anything; 32-bit write to IE/IF works.
    bus.write32(IO + 0x200, 0x0008_0001, false);
    assert_eq!(bus.ie, 0x0001);
    assert_eq!(bus.if_, 0);
}

#[test]
fn irq_line_requires_ime_ie_and_if() {
    let mut bus = test_bus();
    bus.request_irq(IRQ_VBLANK);
    assert!(!bus.irq_line());
    bus.write16(IO + 0x200, IRQ_VBLANK, false);
    assert!(!bus.irq_line(), "IME=0 masks the line");
    assert!(bus.wake_condition(), "HALT wakes regardless of IME");
    bus.write32(IO + 0x208, 1, false);
    assert!(bus.irq_line());
    bus.write16(IO + 0x200, IRQ_HBLANK, false);
    assert!(!bus.irq_line());
    assert!(!bus.wake_condition());
}

#[test]
fn ie_ime_and_waitcnt_accept_byte_writes() {
    let mut bus = test_bus();
    bus.write16(IO + 0x200, 0x0001, false);
    bus.write8(IO + 0x201, 0x10, false);
    assert_eq!(bus.ie, 0x1001);
    bus.write8(IO + 0x200, 0x08, false);
    assert_eq!(bus.ie, 0x1008);
    bus.write8(IO + 0x208, 0x01, false);
    assert_eq!(bus.ime, 1);
    bus.write16(IO + 0x204, 0x0014, false);
    bus.write8(IO + 0x205, 0x40, false);
    assert_eq!(bus.waitcnt, 0x4014);
}

#[test]
fn haltcnt_requests_halt_or_stop() {
    let mut bus = test_bus();
    bus.write8(IO + 0x301, 0x00, false);
    assert!(bus.halt_requested && !bus.stop_requested);
    bus.halt_requested = false;
    bus.write8(IO + 0x301, 0x80, false);
    assert!(bus.stop_requested && !bus.halt_requested);
}

// ------------------------------------------------------------ DMA-57-01/02

fn dma_setup(bus: &mut Bus, ch: u32, sad: u32, dad: u32, count: u16, control: u16) {
    let base = IO + 0xB0 + 12 * ch;
    bus.poke32(base, sad);
    bus.poke32(base + 4, dad);
    bus.poke16(base + 8, count);
    bus.poke16(base + 10, control);
}

fn fill_words(bus: &mut Bus, addr: u32, words: &[u32]) {
    for (i, w) in words.iter().enumerate() {
        bus.poke32(addr + 4 * i as u32, *w);
    }
}

#[test]
fn dma_immediate_word_transfer_and_disable() {
    let mut bus = test_bus();
    fill_words(&mut bus, 0x0200_0000, &[1, 2, 3, 4, 5]);
    dma_setup(&mut bus, 3, 0x0200_0000, 0x0300_0000, 4, 0x8400);
    assert!(bus.dma_pending());
    let spent = cost(&mut bus, Bus::run_dmas);
    for i in 0..4 {
        assert_eq!(bus.peek32(0x0300_0000 + 4 * i), i + 1);
    }
    assert_eq!(bus.peek32(0x0300_0010), 0, "count respected");
    assert_eq!(bus.peek16(IO + 0xDE) & 0x8000, 0, "enable cleared");
    // DMA-57-02: 2N + 2(n-1)S + 2I with 1-cycle IWRAM/EWRAM... here both
    // ends are 32-bit: EWRAM read 6, IWRAM write 1, plus 2 idle.
    assert_eq!(spent, 2 + 4 * (6 + 1));
}

#[test]
fn dma_halfword_width_and_address_modes() {
    let mut bus = test_bus();
    for i in 0..8u32 {
        bus.poke16(0x0200_0000 + 2 * i, 0x100 + i as u16);
    }
    // Halfword, increment/increment, count 3.
    dma_setup(&mut bus, 0, 0x0200_0000, 0x0300_0000, 3, 0x8000);
    bus.run_dmas();
    assert_eq!(bus.peek16(0x0300_0000), 0x100);
    assert_eq!(bus.peek16(0x0300_0004), 0x102);
    assert_eq!(bus.peek16(0x0300_0006), 0);

    // Source decrement.
    dma_setup(&mut bus, 1, 0x0200_0006, 0x0300_0100, 4, 0x8080);
    bus.run_dmas();
    let got: Vec<u16> = (0..4).map(|i| bus.peek16(0x0300_0100 + 2 * i)).collect();
    assert_eq!(got, [0x103, 0x102, 0x101, 0x100]);

    // Destination decrement.
    dma_setup(&mut bus, 2, 0x0200_0000, 0x0300_0206, 4, 0x8020);
    bus.run_dmas();
    let got: Vec<u16> = (0..4).map(|i| bus.peek16(0x0300_0200 + 2 * i)).collect();
    assert_eq!(got, [0x103, 0x102, 0x101, 0x100]);

    // Fixed source = fill.
    dma_setup(&mut bus, 3, 0x0200_0002, 0x0300_0300, 3, 0x8100);
    bus.run_dmas();
    let got: Vec<u16> = (0..4).map(|i| bus.peek16(0x0300_0300 + 2 * i)).collect();
    assert_eq!(got, [0x101, 0x101, 0x101, 0]);

    // Fixed destination keeps only the last unit.
    dma_setup(&mut bus, 3, 0x0200_0000, 0x0300_0400, 3, 0x8040);
    bus.run_dmas();
    assert_eq!(bus.peek16(0x0300_0400), 0x102);
    assert_eq!(bus.peek16(0x0300_0402), 0);
}

#[test]
fn dma_count_zero_and_max_counts() {
    let mut bus = test_bus();
    dma_setup(&mut bus, 0, 0x0200_0000, 0x0201_0000, 0, 0x8000);
    bus.run_dmas();
    assert_eq!(bus.dma[0].src, 0x0200_0000 + 0x4000 * 2);
    dma_setup(&mut bus, 3, 0x0200_0000, 0x0202_0000, 0, 0x8000);
    bus.run_dmas();
    assert_eq!(bus.dma[3].src, 0x0200_0000 + 0x10000 * 2);
    // 0x4000 written explicitly on channels 0-2 also means 0x4000 units.
    dma_setup(&mut bus, 1, 0x0200_0000, 0x0201_0000, 0x4000, 0x8000);
    bus.run_dmas();
    assert_eq!(bus.dma[1].src, 0x0200_0000 + 0x4000 * 2);
    // Upper count bits are ignored on channels 0-2 (14-bit counter).
    dma_setup(&mut bus, 2, 0x0200_0000, 0x0201_0000, 0xC002, 0x8000);
    bus.run_dmas();
    assert_eq!(bus.dma[2].src, 0x0200_0004);
}

#[test]
fn dma_irq_on_end() {
    let mut bus = test_bus();
    dma_setup(&mut bus, 2, 0x0200_0000, 0x0300_0000, 1, 0xC000);
    bus.run_dmas();
    assert_eq!(bus.if_, IRQ_DMA0 << 2);
}

#[test]
fn dma_vblank_repeat_with_reload_and_increment() {
    let mut bus = test_bus();
    fill_words(
        &mut bus,
        0x0200_0000,
        &[0xAABB_CCDD, 0x1122_3344, 0x5566_7788],
    );
    // DMA0: VBlank, repeat, 32-bit, dest increment+reload, count 1.
    dma_setup(
        &mut bus,
        0,
        0x0200_0000,
        0x0300_0000,
        1,
        0x8000 | 0x1000 | 0x0200 | 0x0400 | 0x0060,
    );
    // DMA1: VBlank, repeat, 32-bit, dest increment, count 1.
    dma_setup(
        &mut bus,
        1,
        0x0200_0000,
        0x0300_0100,
        1,
        0x8000 | 0x1000 | 0x0200 | 0x0400,
    );
    assert!(!bus.dma_pending(), "VBlank DMA waits for VBlank");
    let vblank = 160 * CYCLES_PER_LINE;
    run_until(&mut bus, vblank - 1);
    assert_eq!(bus.peek32(0x0300_0000), 0);
    run_until(&mut bus, vblank + 1);
    assert_eq!(bus.peek32(0x0300_0000), 0xAABB_CCDD);
    assert_eq!(bus.peek32(0x0300_0100), 0xAABB_CCDD);
    assert_ne!(
        bus.dma[0].control & 0x8000,
        0,
        "repeat keeps the channel enabled"
    );
    run_until(&mut bus, vblank + CYCLES_PER_FRAME + 1);
    // Reload: same destination, next source word.
    assert_eq!(bus.peek32(0x0300_0000), 0x1122_3344);
    assert_eq!(bus.peek32(0x0300_0004), 0);
    // Increment: destination continues.
    assert_eq!(bus.peek32(0x0300_0104), 0x1122_3344);
}

#[test]
fn dma_repeat_reloads_count() {
    let mut bus = test_bus();
    fill_words(&mut bus, 0x0200_0000, &[1, 2, 3, 4, 5]);
    dma_setup(
        &mut bus,
        3,
        0x0200_0000,
        0x0300_0000,
        2,
        0x8000 | 0x1000 | 0x0200 | 0x0400,
    );
    run_until(&mut bus, 160 * CYCLES_PER_LINE + CYCLES_PER_FRAME + 1);
    let got: Vec<u32> = (0..5).map(|i| bus.peek32(0x0300_0000 + 4 * i)).collect();
    assert_eq!(got, [1, 2, 3, 4, 0]);
}

#[test]
fn dma_hblank_only_on_visible_lines() {
    let mut bus = test_bus();
    // DMA2: HBlank, repeat, halfword, inc/inc, count 1.
    dma_setup(
        &mut bus,
        2,
        0x0200_0000,
        0x0201_0000,
        1,
        0x8000 | 0x2000 | 0x0200,
    );
    run_until(&mut bus, HDRAW_CYCLES - 1);
    assert_eq!(bus.dma[2].dst, 0x0201_0000);
    run_until(&mut bus, HDRAW_CYCLES + 1);
    assert_eq!(bus.dma[2].dst, 0x0201_0002);
    run_until(&mut bus, 200 * CYCLES_PER_LINE);
    assert_eq!(
        bus.dma[2].dst,
        0x0201_0000 + 160 * 2,
        "no HBlank DMA in VBlank"
    );
    run_until(&mut bus, CYCLES_PER_FRAME + HDRAW_CYCLES + 1);
    assert_eq!(bus.dma[2].dst, 0x0201_0000 + 161 * 2);
}

#[test]
fn dma_priority_lower_channel_first() {
    let mut bus = test_bus();
    bus.poke32(0x0200_0000, 0x1111_1111);
    bus.poke32(0x0200_0004, 0x2222_2222);
    // Both write the same destination; DMA3 is set up first but DMA0 must
    // run first, so DMA3's value survives.
    dma_setup(&mut bus, 3, 0x0200_0004, 0x0300_0000, 1, 0x8400);
    dma_setup(&mut bus, 0, 0x0200_0000, 0x0300_0000, 1, 0x8400);
    bus.run_dmas();
    assert_eq!(bus.peek32(0x0300_0000), 0x2222_2222);
}

#[test]
fn dma_registers_accept_byte_writes() {
    let mut bus = test_bus();
    bus.poke32(0x0200_0000, 0x1234_5678);
    let base = IO + 0xD4;
    bus.poke32(base, 0x0200_0000);
    bus.poke32(base + 4, 0x0300_0000);
    bus.poke16(base + 8, 1);
    bus.write8(base + 10, 0x00, false); // CNT_H low byte (address control)
    bus.write8(base + 11, 0x84, false); // enable + 32-bit
    bus.run_dmas();
    assert_eq!(bus.peek32(0x0300_0000), 0x1234_5678);
}

// ------------------------------------------------------------ TIM-57-01

fn timer_counter(bus: &mut Bus, i: u32) -> u16 {
    bus.peek16(IO + 0x100 + 4 * i)
}

#[test]
fn timer_prescalers() {
    for (control, shift) in [(0x80u16, 0u32), (0x81, 6), (0x82, 8), (0x83, 10)] {
        let mut bus = test_bus();
        bus.cycles = 1000;
        bus.poke16(IO + 0x100, 0x0100);
        bus.poke16(IO + 0x102, control);
        let start = 1002; // two-cycle start delay
        bus.cycles = start + (10 << shift) - 1;
        assert_eq!(timer_counter(&mut bus, 0), 0x0109, "control {control:#x}");
        bus.cycles = start + (10 << shift);
        assert_eq!(timer_counter(&mut bus, 0), 0x010A, "control {control:#x}");
        assert_eq!(bus.peek16(IO + 0x102), control);
    }
}

#[test]
fn timer_overflow_reload_irq_and_reads_between_overflows() {
    let mut bus = test_bus();
    bus.poke16(IO + 0x100, 0xFF00);
    bus.poke16(IO + 0x102, 0x00C0);
    let start = 2;
    run_until(&mut bus, start + 0xFF);
    assert_eq!(timer_counter(&mut bus, 0), 0xFFFF);
    assert_eq!(bus.if_, 0);
    run_until(&mut bus, start + 0x100);
    assert_eq!(bus.if_, IRQ_TIMER0);
    assert_eq!(timer_counter(&mut bus, 0), 0xFF00);
    run_until(&mut bus, start + 0x100 + 0x37);
    assert_eq!(timer_counter(&mut bus, 0), 0xFF37);
    bus.if_ = 0;
    run_until(&mut bus, start + 0x200 * 3 + 5);
    assert_eq!(timer_counter(&mut bus, 0), 0xFF05);
    assert_eq!(bus.if_, IRQ_TIMER0);
}

#[test]
fn timer_cascade_counts_overflows() {
    let mut bus = test_bus();
    bus.poke16(IO + 0x104, 0xFFFE);
    bus.poke16(IO + 0x106, 0x00C4); // enable, IRQ, cascade
    bus.poke16(IO + 0x100, 0xFFFF);
    bus.poke16(IO + 0x102, 0x0080); // overflows every cycle
    let start = 2;
    assert_eq!(timer_counter(&mut bus, 1), 0xFFFE);
    run_until(&mut bus, start + 1);
    assert_eq!(timer_counter(&mut bus, 1), 0xFFFF);
    assert_eq!(bus.if_, 0);
    run_until(&mut bus, start + 2);
    assert_eq!(bus.if_, IRQ_TIMER0 << 1);
    assert_eq!(timer_counter(&mut bus, 1), 0xFFFE);
    // A cascaded counter does not run on the clock.
    run_until(&mut bus, start + 2);
    let c = timer_counter(&mut bus, 1);
    bus.poke16(IO + 0x102, 0); // stop timer 0
    run_until(&mut bus, start + 1000);
    assert_eq!(timer_counter(&mut bus, 1), c);
}

#[test]
fn timers_overflowing_simultaneously_both_raise_irq() {
    let mut bus = test_bus();
    for i in [0u32, 2] {
        bus.poke16(IO + 0x100 + 4 * i, 0xFFF0);
    }
    bus.poke16(IO + 0x102, 0x00C0);
    bus.poke16(IO + 0x10A, 0x00C0);
    run_until(&mut bus, 2 + 16);
    assert_eq!(bus.if_, IRQ_TIMER0 | (IRQ_TIMER0 << 2));
}

#[test]
fn timer0_ignores_cascade_bit() {
    let mut bus = test_bus();
    bus.poke16(IO + 0x100, 0);
    bus.poke16(IO + 0x102, 0x0084);
    run_until(&mut bus, 2 + 100);
    assert_eq!(timer_counter(&mut bus, 0), 100);
}

#[test]
fn timer_stop_freezes_and_restart_reloads() {
    let mut bus = test_bus();
    bus.poke16(IO + 0x100, 0x1000);
    bus.poke16(IO + 0x102, 0x0080);
    run_until(&mut bus, 2 + 100);
    bus.poke16(IO + 0x102, 0x0000);
    run_until(&mut bus, 5000);
    assert_eq!(timer_counter(&mut bus, 0), 0x1064);
    bus.poke16(IO + 0x102, 0x0080);
    assert_eq!(timer_counter(&mut bus, 0), 0x1000);
}

#[test]
fn timer_registers_accept_byte_writes() {
    let mut bus = test_bus();
    bus.write8(IO + 0x100, 0x34, false);
    bus.write8(IO + 0x101, 0x12, false);
    assert_eq!(bus.timers[0].reload, 0x1234);
    bus.write8(IO + 0x102, 0x80, false);
    bus.write8(IO + 0x103, 0x00, false); // must not stop the timer
    assert_ne!(bus.timers[0].control & 0x80, 0);
}

// ------------------------------------------------------------ PPU timing

#[test]
fn dispstat_flags_and_irqs_follow_line_timing() {
    let mut bus = test_bus();
    // VBlank, HBlank and VCount(=5) IRQ enables.
    bus.poke16(IO + 0x004, 0x0538);
    run_until(&mut bus, HDRAW_CYCLES);
    assert_eq!(bus.peek16(IO + 4) & 2, 2);
    assert_eq!(bus.if_, IRQ_HBLANK);
    run_until(&mut bus, 5 * CYCLES_PER_LINE);
    assert_eq!(bus.peek16(IO + 6), 5);
    assert_ne!(bus.peek16(IO + 4) & 4, 0);
    assert_ne!(bus.if_ & IRQ_VCOUNT, 0);
    run_until(&mut bus, 160 * CYCLES_PER_LINE);
    assert_ne!(bus.peek16(IO + 4) & 1, 0);
    assert_ne!(bus.if_ & IRQ_VBLANK, 0);
    assert_eq!(bus.frame_count, 1);
    run_until(&mut bus, 227 * CYCLES_PER_LINE);
    assert_eq!(bus.peek16(IO + 4) & 1, 0, "VBlank flag clear on line 227");
    run_until(&mut bus, CYCLES_PER_FRAME);
    assert_eq!(bus.peek16(IO + 6), 0);
}

// ------------------------------------------------------------ KEY / IO

#[test]
fn keyinput_and_keycnt_irq() {
    let mut bus = test_bus();
    bus.set_keys(crate::keys::A | crate::keys::START);
    assert_eq!(bus.peek16(IO + 0x130), 0x03F6);
    // OR mode on A.
    bus.poke16(IO + 0x132, 0x4001);
    bus.set_keys(crate::keys::A);
    assert_eq!(bus.if_, IRQ_KEYPAD);
    bus.if_ = 0;
    // AND mode on A+B.
    bus.poke16(IO + 0x132, 0xC003);
    bus.set_keys(crate::keys::A);
    assert_eq!(bus.if_, 0);
    bus.set_keys(crate::keys::A | crate::keys::B);
    assert_eq!(bus.if_, IRQ_KEYPAD);
}

#[test]
fn unmodelled_registers_are_counted() {
    let mut bus = test_bus();
    assert_eq!(bus.peek16(IO + 0x110), 0);
    bus.poke16(IO + 0x0E0, 0x1234);
    assert!(bus.unsupported_io.contains_key(&0x110));
    assert!(bus.unsupported_io.contains_key(&0x0E0));
    // SIO reports idle/disconnected.
    assert_eq!(bus.peek16(IO + 0x134), 0x8000);
}

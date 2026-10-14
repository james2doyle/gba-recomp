//! SPEC-060 acceptance for cartridge backup: detection, SRAM, Flash
//! command protocol (64/128 KiB) and the EEPROM serial protocol, including
//! size inference from the DMA length.

use super::*;
use crate::bus::Bus;
use crate::bus::tests::test_bus_with;
use crate::cpu::CpuBus;

fn rom_with(signature: &[u8]) -> Vec<u8> {
    let mut rom = vec![0u8; 0x1000];
    rom[0x800..0x800 + signature.len()].copy_from_slice(signature);
    rom
}

#[test]
fn detection_by_signature_strings() {
    assert_eq!(detect(&rom_with(b"EEPROM_V124")), BackupKind::Eeprom);
    assert_eq!(detect(&rom_with(b"FLASH1M_V103")), BackupKind::Flash128);
    assert_eq!(detect(&rom_with(b"FLASH512_V131")), BackupKind::Flash64);
    assert_eq!(detect(&rom_with(b"FLASH_V126")), BackupKind::Flash64);
    assert_eq!(detect(&rom_with(b"SRAM_V113")), BackupKind::Sram);
    assert_eq!(detect(&rom_with(b"SRAM_F_V100")), BackupKind::Sram);
    assert_eq!(detect(&rom_with(b"NOTHING")), BackupKind::None);
    for kind in [
        BackupKind::None,
        BackupKind::Sram,
        BackupKind::Flash64,
        BackupKind::Flash128,
        BackupKind::Eeprom,
        BackupKind::Eeprom512,
        BackupKind::Eeprom8k,
    ] {
        assert_eq!(BackupKind::parse(kind.as_str()), Some(kind));
    }
}

#[test]
fn sram_read_write_mirror_and_existing_save() {
    let mut b = Backup::new(BackupKind::Sram, Some(&[1, 2, 3]));
    assert_eq!(b.data().len(), 0x8000);
    assert_eq!(
        (
            b.read8(0x0E00_0000),
            b.read8(0x0E00_0002),
            b.read8(0x0E00_0003)
        ),
        (1, 3, 0xFF)
    );
    assert!(!b.dirty);
    b.write8(0x0E00_7FFF, 0x42);
    assert_eq!(b.read8(0x0E00_FFFF), 0x42);
    assert!(b.dirty);
}

// ------------------------------------------------------------ Flash

fn flash_cmd(b: &mut Backup, cmd: u8) {
    b.write8(0x0E00_5555, 0xAA);
    b.write8(0x0E00_2AAA, 0x55);
    b.write8(0x0E00_5555, cmd);
}

#[test]
fn flash_id_mode() {
    for (kind, id) in [
        (BackupKind::Flash128, [0xC2, 0x09]),
        (BackupKind::Flash64, [0x32, 0x1B]),
    ] {
        let mut b = Backup::new(kind, None);
        flash_cmd(&mut b, 0x90);
        assert_eq!([b.read8(0x0E00_0000), b.read8(0x0E00_0001)], id);
        flash_cmd(&mut b, 0xF0);
        assert_eq!(b.read8(0x0E00_0000), 0xFF);
        // A bare F0 also leaves ID mode.
        flash_cmd(&mut b, 0x90);
        b.write8(0x0E00_0000, 0xF0);
        assert_eq!(b.read8(0x0E00_0000), 0xFF);
    }
}

#[test]
fn flash_program_is_and_and_needs_the_command() {
    let mut b = Backup::new(BackupKind::Flash64, None);
    b.write8(0x0E00_0100, 0x12); // no command: ignored
    assert_eq!(b.read8(0x0E00_0100), 0xFF);
    flash_cmd(&mut b, 0xA0);
    b.write8(0x0E00_0100, 0x12);
    assert_eq!(b.read8(0x0E00_0100), 0x12);
    assert!(b.dirty);
    // Programming can only clear bits.
    flash_cmd(&mut b, 0xA0);
    b.write8(0x0E00_0100, 0xF0);
    assert_eq!(b.read8(0x0E00_0100), 0x10);
}

#[test]
fn flash_sector_and_chip_erase() {
    let mut b = Backup::new(BackupKind::Flash64, None);
    for addr in [0x0E00_0100u32, 0x0E00_1000, 0x0E00_1FFF, 0x0E00_2000] {
        flash_cmd(&mut b, 0xA0);
        b.write8(addr, 0x00);
    }
    flash_cmd(&mut b, 0x80);
    b.write8(0x0E00_5555, 0xAA);
    b.write8(0x0E00_2AAA, 0x55);
    b.write8(0x0E00_1234, 0x30); // erase the 4 KiB sector 0x1000-0x1FFF
    assert_eq!(b.read8(0x0E00_1000), 0xFF);
    assert_eq!(b.read8(0x0E00_1FFF), 0xFF);
    assert_eq!(b.read8(0x0E00_0100), 0x00);
    assert_eq!(b.read8(0x0E00_2000), 0x00);
    flash_cmd(&mut b, 0x80);
    flash_cmd(&mut b, 0x10); // chip erase
    assert!(b.data().iter().all(|&v| v == 0xFF));
}

#[test]
fn flash128_bank_switch() {
    let mut b = Backup::new(BackupKind::Flash128, None);
    flash_cmd(&mut b, 0xB0);
    b.write8(0x0E00_0000, 1);
    flash_cmd(&mut b, 0xA0);
    b.write8(0x0E00_0010, 0x5A);
    assert_eq!(b.read8(0x0E00_0010), 0x5A);
    assert_eq!(b.data()[0x1_0010], 0x5A);
    // Sector erase applies to the selected bank.
    flash_cmd(&mut b, 0xB0);
    b.write8(0x0E00_0000, 0);
    assert_eq!(b.read8(0x0E00_0010), 0xFF);
    flash_cmd(&mut b, 0xA0);
    b.write8(0x0E00_0010, 0x11);
    flash_cmd(&mut b, 0xB0);
    b.write8(0x0E00_0000, 1);
    flash_cmd(&mut b, 0x80);
    b.write8(0x0E00_5555, 0xAA);
    b.write8(0x0E00_2AAA, 0x55);
    b.write8(0x0E00_0000, 0x30);
    assert_eq!(b.data()[0x1_0010], 0xFF);
    assert_eq!(b.data()[0x0_0010], 0x11);
    // 64 KiB parts ignore the bank command.
    let mut b = Backup::new(BackupKind::Flash64, None);
    flash_cmd(&mut b, 0xB0);
    b.write8(0x0E00_0000, 1);
    flash_cmd(&mut b, 0xA0);
    b.write8(0x0E00_0010, 0x5A);
    assert_eq!(b.data()[0x10], 0x5A);
}

#[test]
fn flash_through_the_bus() {
    let mut bus = test_bus_with(&[0u8; 0x400], BackupKind::Flash128);
    for (addr, v) in [
        (0x0E00_5555u32, 0xAAu8),
        (0x0E00_2AAA, 0x55),
        (0x0E00_5555, 0x90),
    ] {
        bus.write8(addr, v, false);
    }
    assert_eq!(bus.read8(0x0E00_0000, false), 0xC2);
    assert_eq!(bus.read8(0x0E00_0001, false), 0x09);
}

// ------------------------------------------------------------ EEPROM

const EEPROM: u32 = 0x0D00_0000;

fn bits_of(value: u64, n: usize) -> Vec<u16> {
    (0..n).rev().map(|i| ((value >> i) & 1) as u16).collect()
}

fn write_request(addr_bits: usize, block: u64, data: u64) -> Vec<u16> {
    let mut bits = vec![1, 0];
    bits.extend(bits_of(block, addr_bits));
    bits.extend(bits_of(data, 64));
    bits.push(0);
    bits
}

fn read_request(addr_bits: usize, block: u64) -> Vec<u16> {
    let mut bits = vec![1, 1];
    bits.extend(bits_of(block, addr_bits));
    bits.push(0);
    bits
}

fn send(bus: &mut Bus, bits: &[u16]) {
    for &b in bits {
        bus.write16(EEPROM, b, false);
    }
}

fn receive(bus: &mut Bus) -> u64 {
    let stream: Vec<u16> = (0..68).map(|_| bus.read16(EEPROM, false) & 1).collect();
    assert_eq!(stream[..4], [0, 0, 0, 0], "4 dummy bits");
    stream[4..]
        .iter()
        .fold(0, |acc, &b| (acc << 1) | u64::from(b))
}

#[test]
fn eeprom_8k_cpu_protocol() {
    let mut bus = test_bus_with(&[0u8; 0x400], BackupKind::Eeprom8k);
    assert_eq!(bus.read16(EEPROM, false) & 1, 1, "idle reads ready");
    send(&mut bus, &write_request(14, 3, 0x0123_4567_89AB_CDEF));
    assert_eq!(bus.read16(EEPROM, false) & 1, 1, "ready after write");
    assert_eq!(
        bus.backup.data()[24..32],
        [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]
    );
    assert!(bus.backup.dirty);
    send(&mut bus, &read_request(14, 3));
    assert_eq!(receive(&mut bus), 0x0123_4567_89AB_CDEF);
    send(&mut bus, &read_request(14, 4));
    assert_eq!(receive(&mut bus), u64::MAX, "erased block reads 1s");
}

#[test]
fn eeprom_512_uses_6_bit_addresses() {
    let mut bus = test_bus_with(&[0u8; 0x400], BackupKind::Eeprom512);
    send(&mut bus, &write_request(6, 63, 0xFEDC_BA98_7654_3210));
    assert_eq!(bus.backup.data().len(), 0x200);
    assert_eq!(bus.backup.data()[0x1F8], 0xFE);
    send(&mut bus, &read_request(6, 63));
    assert_eq!(receive(&mut bus), 0xFEDC_BA98_7654_3210);
}

fn dma3(bus: &mut Bus, src: u32, dst: u32, count: u16) {
    bus.poke32(0x0400_00D4, src);
    bus.poke32(0x0400_00D8, dst);
    bus.poke16(0x0400_00DC, count);
    bus.poke16(0x0400_00DE, 0x8000);
    bus.run_dmas();
}

fn stage(bus: &mut Bus, addr: u32, bits: &[u16]) {
    for (i, &b) in bits.iter().enumerate() {
        bus.poke16(addr + 2 * i as u32, b);
    }
}

#[test]
fn eeprom_size_inferred_from_dma_length() {
    for (addr_bits, kind) in [(6usize, BackupKind::Eeprom512), (14, BackupKind::Eeprom8k)] {
        let mut bus = test_bus_with(&[0u8; 0x400], BackupKind::Eeprom);
        let value = 0x1122_3344_5566_7788u64;
        // Write request through DMA (73 or 81 units).
        let req = write_request(addr_bits, 2, value);
        stage(&mut bus, 0x0200_0000, &req);
        dma3(&mut bus, 0x0200_0000, EEPROM, req.len() as u16);
        assert_eq!(bus.backup.kind(), kind, "{addr_bits}-bit address");
        // Read request (9 or 17 units), then the 68-unit reply.
        let req = read_request(addr_bits, 2);
        stage(&mut bus, 0x0200_0000, &req);
        dma3(&mut bus, 0x0200_0000, EEPROM, req.len() as u16);
        dma3(&mut bus, EEPROM, 0x0200_1000, 68);
        let got = (0..64).fold(0u64, |acc, i| {
            (acc << 1) | u64::from(bus.peek16(0x0200_1000 + 2 * (4 + i)) & 1)
        });
        assert_eq!(got, value);
        assert_eq!(bus.peek16(0x0200_1000), 0);
    }
}

#[test]
fn eeprom_address_window() {
    let b = Backup::new(BackupKind::Eeprom, None);
    assert!(b.is_eeprom_address(0x0D00_0000, 0x80_0000));
    assert!(!b.is_eeprom_address(0x0D00_0000, 0x200_0000), "32 MiB ROM");
    assert!(b.is_eeprom_address(0x0DFF_FF00, 0x200_0000));
    assert!(!b.is_eeprom_address(0x0C00_0000, 0x80_0000));
    let flash = Backup::new(BackupKind::Flash64, None);
    assert!(!flash.is_eeprom_address(0x0D00_0000, 0x80_0000));
}

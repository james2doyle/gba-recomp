//! System bus, MMIO, scheduler, IRQ, DMA, timers and keypad (SPEC-057).

use std::collections::BTreeMap;

use crate::apu::Apu;
use crate::backup::Backup;
use crate::cpu::{Cpu, CpuBus};
use crate::ppu::{self, Ppu};

pub const IRQ_VBLANK: u16 = 1 << 0;
pub const IRQ_HBLANK: u16 = 1 << 1;
pub const IRQ_VCOUNT: u16 = 1 << 2;
pub const IRQ_TIMER0: u16 = 1 << 3;
pub const IRQ_SERIAL: u16 = 1 << 7;
pub const IRQ_DMA0: u16 = 1 << 8;
pub const IRQ_KEYPAD: u16 = 1 << 12;

pub const CYCLES_PER_LINE: u64 = 1232;
pub const HDRAW_CYCLES: u64 = 1008;
pub const LINES_PER_FRAME: u32 = 228;
pub const CYCLES_PER_FRAME: u64 = CYCLES_PER_LINE * LINES_PER_FRAME as u64;

const TIMER_PRESCALE: [u32; 4] = [0, 6, 8, 10];

/// Interpretation of an SWI the HLE BIOS cannot honour (BIOS-01).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemFault {
    UnsupportedSwi { number: u8, pc: u32 },
    Cpu(crate::cpu::Fault),
}

#[derive(Clone, Copy, Debug)]
pub struct Timer {
    pub reload: u16,
    pub control: u16,
    /// Counter value at `start`.
    counter: u16,
    start: u64,
    /// Cycle at which the counter overflows (only for running, non-cascade timers).
    overflow_at: u64,
}

impl Default for Timer {
    fn default() -> Self {
        Self {
            reload: 0,
            control: 0,
            counter: 0,
            start: 0,
            // Never scheduled: a zero here made every untouched timer fire a
            // spurious overflow (FIFO pop, DMA request) at the first event.
            overflow_at: u64::MAX,
        }
    }
}

impl Timer {
    fn running(&self) -> bool {
        self.control & 0x80 != 0
    }
    fn cascade(&self) -> bool {
        self.control & 0x04 != 0
    }
    fn shift(&self) -> u32 {
        TIMER_PRESCALE[(self.control & 3) as usize]
    }
    fn current(&self, now: u64) -> u16 {
        if !self.running() || self.cascade() {
            return self.counter;
        }
        let ticks = (now.saturating_sub(self.start)) >> self.shift();
        (u64::from(self.counter) + ticks) as u16
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Dma {
    pub sad: u32,
    pub dad: u32,
    pub count: u16,
    pub control: u16,
    pub(crate) src: u32,
    pub(crate) dst: u32,
    remaining: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DmaTiming {
    Immediate,
    VBlank,
    HBlank,
    Special,
}

impl Dma {
    fn enabled(&self) -> bool {
        self.control & 0x8000 != 0
    }
    fn timing(&self) -> DmaTiming {
        match (self.control >> 12) & 3 {
            0 => DmaTiming::Immediate,
            1 => DmaTiming::VBlank,
            2 => DmaTiming::HBlank,
            _ => DmaTiming::Special,
        }
    }
}

pub struct Bus {
    bios: Box<[u8]>,
    pub hle_bios: bool,
    bios_latch: u32,
    pub pc_in_bios: bool,
    pub ewram: Box<[u8]>,
    pub iwram: Box<[u8]>,
    pub io: Box<[u8]>,
    pub ppu: Ppu,
    pub apu: Apu,
    pub rom: Box<[u8]>,
    rom_mask: usize,
    pub backup: Backup,
    pub timers: [Timer; 4],
    pub dma: [Dma; 4],
    dma_pending: u8,
    dma_running: bool,
    pub ie: u16,
    pub if_: u16,
    pub ime: u16,
    pub waitcnt: u16,
    pub halt_requested: bool,
    pub stop_requested: bool,
    pub keyinput: u16,
    pub cycles: u64,
    next_event: u64,
    ppu_event: u64,
    pub open_bus: u32,
    wait_n16: [u8; 16],
    wait_s16: [u8; 16],
    wait_n32: [u8; 16],
    wait_s32: [u8; 16],
    prefetch: bool,
    /// MMIO registers accessed without a model (IO-57-01): offset → count.
    pub unsupported_io: BTreeMap<u32, u64>,
    pub fault: Option<SystemFault>,
    pub frame_ready: bool,
    pub frame_count: u64,
    pub hle: crate::bios::HleState,
}

impl Bus {
    pub fn new(rom: &[u8], bios: Option<&[u8]>, backup: Backup) -> Self {
        let mut rom_buf = rom.to_vec();
        let size = rom_buf.len().max(1).next_power_of_two();
        rom_buf.resize(size.max(0x100), 0);
        let mut bios_buf = vec![0u8; 0x4000];
        let hle_bios = bios.is_none();
        match bios {
            Some(b) => {
                let n = b.len().min(0x4000);
                bios_buf[..n].copy_from_slice(&b[..n]);
            }
            None => crate::bios::install_stub(&mut bios_buf),
        }
        let rom_mask = rom_buf.len() - 1;
        let mut bus = Self {
            bios: bios_buf.into_boxed_slice(),
            hle_bios,
            bios_latch: 0xE129_F000,
            pc_in_bios: false,
            ewram: vec![0; 0x40000].into_boxed_slice(),
            iwram: vec![0; 0x8000].into_boxed_slice(),
            io: vec![0; 0x400].into_boxed_slice(),
            ppu: Ppu::new(),
            apu: Apu::new(),
            rom: rom_buf.into_boxed_slice(),
            rom_mask,
            backup,
            timers: [Timer::default(); 4],
            dma: [Dma::default(); 4],
            dma_pending: 0,
            dma_running: false,
            ie: 0,
            if_: 0,
            ime: 0,
            waitcnt: 0,
            halt_requested: false,
            stop_requested: false,
            keyinput: 0x03FF,
            cycles: 0,
            next_event: 0,
            ppu_event: HDRAW_CYCLES,
            open_bus: 0,
            wait_n16: [1; 16],
            wait_s16: [1; 16],
            wait_n32: [1; 16],
            wait_s32: [1; 16],
            prefetch: false,
            unsupported_io: BTreeMap::new(),
            fault: None,
            frame_ready: false,
            frame_count: 0,
            hle: crate::bios::HleState::default(),
        };
        bus.update_waitstates();
        // Hardware reset values of the affine BG matrices: PA = PD = 1.0
        // (identity), so bitmap modes display unscaled without setup.
        for offset in [0x20, 0x26, 0x30, 0x36] {
            bus.io_store16(offset, 0x0100);
        }
        bus.next_event = bus.ppu_event;
        bus.apu.reset_clock(0);
        bus
    }

    pub fn rom_size(&self) -> usize {
        self.rom.len()
    }

    fn update_waitstates(&mut self) {
        const N: [u8; 4] = [4, 3, 2, 8];
        let w = self.waitcnt;
        let sram = N[(w & 3) as usize] + 1;
        let ws = [
            (N[((w >> 2) & 3) as usize], if w & (1 << 4) != 0 { 1 } else { 2 }),
            (N[((w >> 5) & 3) as usize], if w & (1 << 7) != 0 { 1 } else { 4 }),
            (N[((w >> 8) & 3) as usize], if w & (1 << 10) != 0 { 1 } else { 8 }),
        ];
        // Fixed regions (BUS-57-04).
        for (region, n16, n32) in [
            (0x0usize, 1u8, 1u8),
            (0x1, 1, 1),
            (0x2, 3, 6),
            (0x3, 1, 1),
            (0x4, 1, 1),
            (0x5, 1, 2),
            (0x6, 1, 2),
            (0x7, 1, 1),
        ] {
            self.wait_n16[region] = n16;
            self.wait_s16[region] = n16;
            self.wait_n32[region] = n32;
            self.wait_s32[region] = n32;
        }
        for (i, (n, s)) in ws.iter().enumerate() {
            for region in [0x8 + i * 2, 0x9 + i * 2] {
                self.wait_n16[region] = n + 1;
                self.wait_s16[region] = s + 1;
                self.wait_n32[region] = n + 1 + s + 1;
                self.wait_s32[region] = 2 * (s + 1);
            }
        }
        for region in [0xE, 0xF] {
            self.wait_n16[region] = sram;
            self.wait_s16[region] = sram;
            self.wait_n32[region] = sram;
            self.wait_s32[region] = sram;
        }
        self.prefetch = w & (1 << 14) != 0;
    }

    #[inline]
    fn charge(&mut self, addr: u32, wide: bool, seq: bool) {
        let region = ((addr >> 24) & 0xF) as usize;
        let cost = match (wide, seq) {
            (false, false) => self.wait_n16[region],
            (false, true) => self.wait_s16[region],
            (true, false) => self.wait_n32[region],
            (true, true) => self.wait_s32[region],
        };
        self.cycles += u64::from(cost);
    }

    // ------------------------------------------------------------ raw access

    fn rom_read16(&self, addr: u32) -> u16 {
        let offset = (addr & 0x01FF_FFFE) as usize;
        if offset >= self.rom.len() {
            // Unmapped cartridge space returns the address bus (BUS-57-01).
            return (addr >> 1) as u16;
        }
        let o = offset & self.rom_mask;
        u16::from_le_bytes([self.rom[o], self.rom[o + 1]])
    }

    fn read_raw16(&mut self, addr: u32) -> u16 {
        match addr >> 24 {
            0x00 => {
                if addr < 0x4000 {
                    if self.pc_in_bios {
                        let a = (addr & 0x3FFC) as usize;
                        self.bios_latch = u32::from_le_bytes([
                            self.bios[a],
                            self.bios[a + 1],
                            self.bios[a + 2],
                            self.bios[a + 3],
                        ]);
                    }
                    (self.bios_latch >> ((addr & 2) * 8)) as u16
                } else {
                    (self.open_bus >> ((addr & 2) * 8)) as u16
                }
            }
            0x02 => {
                let a = (addr & 0x3FFFE) as usize;
                u16::from_le_bytes([self.ewram[a], self.ewram[a + 1]])
            }
            0x03 => {
                let a = (addr & 0x7FFE) as usize;
                u16::from_le_bytes([self.iwram[a], self.iwram[a + 1]])
            }
            0x04 => self.io_read16(addr & 0x00FF_FFFE),
            0x05 => self.ppu.read_palette16(addr),
            0x06 => self.ppu.read_vram16(addr),
            0x07 => self.ppu.read_oam16(addr),
            0x08..=0x0C => self.rom_read16(addr),
            0x0D => {
                if self.backup.is_eeprom_address(addr, self.rom.len()) {
                    self.backup.eeprom_read()
                } else {
                    self.rom_read16(addr)
                }
            }
            0x0E | 0x0F => {
                let b = u16::from(self.backup.read8(addr));
                b * 0x0101
            }
            _ => (self.open_bus >> ((addr & 2) * 8)) as u16,
        }
    }

    fn read_raw8(&mut self, addr: u32) -> u8 {
        match addr >> 24 {
            0x0E | 0x0F => self.backup.read8(addr),
            0x04 => self.io_read8(addr & 0x00FF_FFFF),
            _ => (self.read_raw16(addr & !1) >> ((addr & 1) * 8)) as u8,
        }
    }

    fn read_raw32(&mut self, addr: u32) -> u32 {
        match addr >> 24 {
            0x0E | 0x0F => u32::from(self.backup.read8(addr)) * 0x0101_0101,
            _ => {
                // BUS-57-03: word accesses are word-aligned on the bus.
                let addr = addr & !3;
                let lo = u32::from(self.read_raw16(addr));
                let hi = u32::from(self.read_raw16(addr | 2));
                lo | (hi << 16)
            }
        }
    }

    fn write_raw16(&mut self, addr: u32, value: u16) {
        match addr >> 24 {
            0x02 => {
                let a = (addr & 0x3FFFE) as usize;
                self.ewram[a..a + 2].copy_from_slice(&value.to_le_bytes());
            }
            0x03 => {
                let a = (addr & 0x7FFE) as usize;
                self.iwram[a..a + 2].copy_from_slice(&value.to_le_bytes());
            }
            0x04 => self.io_write16(addr & 0x00FF_FFFE, value),
            0x05 => self.ppu.write_palette16(addr, value),
            0x06 => self.ppu.write_vram16(addr, value),
            0x07 => self.ppu.write_oam16(addr, value),
            0x08..=0x0D => {
                if self.backup.is_eeprom_address(addr, self.rom.len()) {
                    self.backup.eeprom_write(value);
                } else {
                    self.backup.gpio_write16(addr, value);
                }
            }
            0x0E | 0x0F => self.backup.write8(addr, (value >> ((addr & 1) * 8)) as u8),
            _ => {}
        }
    }

    fn write_raw8(&mut self, addr: u32, value: u8) {
        match addr >> 24 {
            0x02 => self.ewram[(addr & 0x3FFFF) as usize] = value,
            0x03 => self.iwram[(addr & 0x7FFF) as usize] = value,
            0x04 => self.io_write8(addr & 0x00FF_FFFF, value),
            0x05 => self.ppu.write_palette8(addr, value),
            0x06 => self.ppu.write_vram8(addr, value),
            0x07 => {} // OAM ignores byte writes (BUS-57-02).
            0x0E | 0x0F => self.backup.write8(addr, value),
            _ => {}
        }
    }

    fn write_raw32(&mut self, addr: u32, value: u32) {
        match addr >> 24 {
            0x0E | 0x0F => self.backup.write8(addr, (value >> ((addr & 3) * 8)) as u8),
            0x04 if (addr & 0xFFFFFF) == 0xA0 || (addr & 0xFFFFFF) == 0xA4 => {
                let channel = usize::from((addr & 0xFFFFFF) == 0xA4);
                for byte in value.to_le_bytes() {
                    self.apu.fifo_push(channel, byte as i8);
                }
            }
            _ => {
                // BUS-57-03: word accesses are word-aligned on the bus.
                let addr = addr & !3;
                self.write_raw16(addr, value as u16);
                self.write_raw16(addr | 2, (value >> 16) as u16);
            }
        }
    }

    // ------------------------------------------------------------ public peek

    /// Side-effect-free word read used by HLE code and debuggers.
    pub fn peek32(&mut self, addr: u32) -> u32 {
        self.read_raw32(addr & !3)
    }

    pub fn peek16(&mut self, addr: u32) -> u16 {
        self.read_raw16(addr & !1)
    }

    pub fn peek8(&mut self, addr: u32) -> u8 {
        self.read_raw8(addr)
    }

    pub fn poke32(&mut self, addr: u32, value: u32) {
        self.write_raw32(addr & !3, value);
    }

    pub fn poke16(&mut self, addr: u32, value: u16) {
        self.write_raw16(addr & !1, value);
    }

    pub fn poke8(&mut self, addr: u32, value: u8) {
        self.write_raw8(addr, value);
    }

    pub fn add_cycles(&mut self, cycles: u64) {
        self.cycles += cycles;
    }

    // ------------------------------------------------------------ MMIO

    fn io_stored16(&self, offset: u32) -> u16 {
        let o = offset as usize;
        if o + 1 < self.io.len() {
            u16::from_le_bytes([self.io[o], self.io[o + 1]])
        } else {
            0
        }
    }

    fn io_store16(&mut self, offset: u32, value: u16) {
        let o = offset as usize;
        if o + 1 < self.io.len() {
            self.io[o..o + 2].copy_from_slice(&value.to_le_bytes());
        }
    }

    /// Last value written to a halfword register, including registers whose
    /// state lives outside `io` (IE/WAITCNT/IME, DMA, timers). Byte writes
    /// merge with this value so that writing one byte never clobbers the
    /// other half of the register.
    fn io_latched16(&self, offset: u32) -> u16 {
        match offset {
            0x0B0..=0x0DE => {
                let d = &self.dma[((offset - 0xB0) / 12) as usize];
                match (offset - 0xB0) % 12 {
                    0 => d.sad as u16,
                    2 => (d.sad >> 16) as u16,
                    4 => d.dad as u16,
                    6 => (d.dad >> 16) as u16,
                    8 => d.count,
                    _ => d.control,
                }
            }
            0x100..=0x10E => {
                let t = &self.timers[((offset - 0x100) / 4) as usize];
                if offset & 2 == 0 { t.reload } else { t.control }
            }
            0x200 => self.ie,
            0x204 => self.waitcnt,
            0x208 => self.ime,
            _ => self.io_stored16(offset),
        }
    }

    fn note_unsupported(&mut self, offset: u32) {
        *self.unsupported_io.entry(offset).or_insert(0) += 1;
    }

    fn io_read8(&mut self, offset: u32) -> u8 {
        (self.io_read16(offset & !1) >> ((offset & 1) * 8)) as u8
    }

    fn io_read16(&mut self, offset: u32) -> u16 {
        if offset >= 0x400 {
            if offset & 0xFFFF == 0x800 {
                return self.io_stored16(0x3FE);
            }
            return (self.open_bus >> ((offset & 2) * 8)) as u16;
        }
        match offset {
            0x000 => self.io_stored16(0),
            0x002 => self.io_stored16(2),
            0x004 => self.ppu.dispstat,
            0x006 => self.ppu.vcount,
            0x008..=0x00E => self.io_stored16(offset),
            0x048 | 0x04A => self.io_stored16(offset) & 0x3F3F,
            0x050 => self.io_stored16(offset) & 0x3FFF,
            0x052 => self.io_stored16(offset) & 0x1F1F,
            // Write-only video registers.
            0x010..=0x046 | 0x04C..=0x04E | 0x054..=0x05E => 0,
            0x060..=0x0A8 => self.apu.read16(offset),
            0x0B0..=0x0DE => {
                let channel = ((offset - 0xB0) / 12) as usize;
                match (offset - 0xB0) % 12 {
                    10 => self.dma[channel].control,
                    _ => 0,
                }
            }
            0x100..=0x10E => {
                let channel = ((offset - 0x100) / 4) as usize;
                if offset & 2 == 0 {
                    self.timer_counter(channel)
                } else {
                    self.timers[channel].control
                }
            }
            0x120..=0x12A | 0x134 | 0x140..=0x15A => {
                // Serial/link: disconnected (SIO responds idle).
                self.note_unsupported(offset);
                match offset {
                    0x128 => self.io_stored16(offset) & !0x0080,
                    0x134 => 0x8000,
                    _ => self.io_stored16(offset),
                }
            }
            0x130 => self.keyinput,
            0x132 => self.io_stored16(offset),
            0x200 => self.ie,
            0x202 => self.if_,
            0x204 => self.waitcnt,
            0x208 => self.ime,
            0x300 => self.io_stored16(0x300) & 0x00FF,
            _ => {
                self.note_unsupported(offset);
                0
            }
        }
    }

    fn io_write8(&mut self, offset: u32, value: u8) {
        match offset {
            0x202 | 0x203 => {
                // IF is write-one-to-clear per byte.
                let shift = (offset & 1) * 8;
                self.if_ &= !(u16::from(value) << shift);
            }
            0x300 => self.io[0x300] = value,
            0x301 => {
                if value & 0x80 != 0 {
                    self.stop_requested = true;
                } else {
                    self.halt_requested = true;
                }
            }
            0x0A0..=0x0A7 => {
                let channel = usize::from(offset >= 0xA4);
                self.apu.fifo_push(channel, value as i8);
            }
            0x060..=0x09F => self.apu.write8(offset, value),
            _ if offset < 0x400 => {
                let aligned = offset & !1;
                let current = self.io_latched16(aligned);
                let shift = (offset & 1) * 8;
                let merged = (current & !(0xFF << shift)) | (u16::from(value) << shift);
                self.io_write16(aligned, merged);
            }
            _ => {}
        }
    }

    fn io_write16(&mut self, offset: u32, value: u16) {
        if offset >= 0x400 {
            return;
        }
        match offset {
            0x000 => {
                self.ppu.dispcnt = value;
                self.io_store16(offset, value);
            }
            0x004 => {
                self.ppu.dispstat = (self.ppu.dispstat & 0x0007) | (value & 0xFFB8);
                self.io_store16(offset, value);
            }
            0x006 => {}
            0x028 | 0x02C | 0x038 | 0x03C => {
                self.io_store16(offset, value);
                self.ppu.latch_reference(&self.io, offset);
            }
            0x02A | 0x02E | 0x03A | 0x03E => {
                self.io_store16(offset, value);
                self.ppu.latch_reference(&self.io, offset - 2);
            }
            0x060..=0x0A8 => {
                self.apu.write8(offset, value as u8);
                self.apu.write8(offset + 1, (value >> 8) as u8);
                if (0xA0..0xA8).contains(&offset) {
                    // FIFO pushes were already performed byte-wise.
                }
            }
            0x0B0..=0x0DE => self.dma_write(offset, value),
            0x100..=0x10E => self.timer_write(offset, value),
            0x130 => {}
            0x132 => {
                self.io_store16(offset, value);
                self.check_keypad_irq();
            }
            0x200 => self.ie = value & 0x3FFF,
            0x202 => self.if_ &= !value,
            0x204 => {
                self.waitcnt = value & 0x5FFF;
                self.update_waitstates();
            }
            0x208 => self.ime = value & 1,
            0x300 => {
                self.io_write8(0x300, value as u8);
                self.io_write8(0x301, (value >> 8) as u8);
            }
            _ => {
                if !matches!(offset, 0x000..=0x05E | 0x120..=0x15A) {
                    self.note_unsupported(offset);
                }
                self.io_store16(offset, value);
            }
        }
    }

    // ------------------------------------------------------------ IRQ

    pub fn request_irq(&mut self, mask: u16) {
        self.if_ |= mask;
    }

    pub fn irq_line(&self) -> bool {
        self.ime & 1 != 0 && self.ie & self.if_ & 0x3FFF != 0
    }

    pub fn wake_condition(&self) -> bool {
        self.ie & self.if_ & 0x3FFF != 0
    }

    pub fn set_keys(&mut self, pressed_mask: u16) {
        self.keyinput = !pressed_mask & 0x03FF;
        self.check_keypad_irq();
    }

    fn check_keypad_irq(&mut self) {
        let keycnt = self.io_stored16(0x132);
        if keycnt & 0x4000 == 0 {
            return;
        }
        let selected = keycnt & 0x03FF;
        let pressed = !self.keyinput & 0x03FF;
        let hit = if keycnt & 0x8000 != 0 {
            selected != 0 && pressed & selected == selected
        } else {
            pressed & selected != 0
        };
        if hit {
            self.request_irq(IRQ_KEYPAD);
        }
    }

    // ------------------------------------------------------------ timers

    fn timer_counter(&self, i: usize) -> u16 {
        self.timers[i].current(self.cycles)
    }

    fn timer_write(&mut self, offset: u32, value: u16) {
        let i = ((offset - 0x100) / 4) as usize;
        if offset & 2 == 0 {
            self.timers[i].reload = value;
            return;
        }
        let now = self.cycles;
        let t = &mut self.timers[i];
        let was_running = t.running();
        // Freeze the counter at its current value before changing control.
        t.counter = t.current(now);
        t.start = now;
        // TM0 has no count-up (cascade) mode: bit 2 is unused there.
        t.control = value & if i == 0 { 0x00C3 } else { 0x00C7 };
        if !was_running && t.running() {
            t.counter = t.reload;
            // The timer starts counting after a short delay.
            t.start = now + 2;
        }
        self.schedule_timer(i);
    }

    fn schedule_timer(&mut self, i: usize) {
        let t = &mut self.timers[i];
        if t.running() && !(t.cascade() && i > 0) {
            let remaining = 0x1_0000 - u64::from(t.counter);
            t.overflow_at = t.start + (remaining << t.shift());
            self.next_event = self.next_event.min(t.overflow_at);
        } else {
            t.overflow_at = u64::MAX;
        }
    }

    fn timer_overflow(&mut self, i: usize, at: u64) {
        let reload = self.timers[i].reload;
        let control = self.timers[i].control;
        if control & 0x40 != 0 {
            self.request_irq(IRQ_TIMER0 << i);
        }
        if i < 2 {
            let requests = self.apu.timer_overflow(i, at);
            for channel in 0..2 {
                if requests & (1 << channel) != 0 {
                    self.trigger_fifo_dma(channel);
                }
            }
        }
        if i < 3 {
            let next = &mut self.timers[i + 1];
            if next.running() && next.cascade() {
                next.counter = next.counter.wrapping_add(1);
                if next.counter == 0 {
                    next.counter = next.reload;
                    self.timer_overflow(i + 1, at);
                }
            }
        }
        let t = &mut self.timers[i];
        if !(t.cascade() && i > 0) {
            t.counter = reload;
            t.start = at;
            self.schedule_timer(i);
        }
    }

    // ------------------------------------------------------------ DMA

    fn dma_write(&mut self, offset: u32, value: u16) {
        let i = ((offset - 0xB0) / 12) as usize;
        let reg = (offset - 0xB0) % 12;
        let d = &mut self.dma[i];
        match reg {
            0 => d.sad = (d.sad & 0xFFFF_0000) | u32::from(value),
            2 => d.sad = (d.sad & 0xFFFF) | (u32::from(value) << 16),
            4 => d.dad = (d.dad & 0xFFFF_0000) | u32::from(value),
            6 => d.dad = (d.dad & 0xFFFF) | (u32::from(value) << 16),
            8 => d.count = value,
            _ => {
                let was_enabled = d.enabled();
                d.control = value & if i == 3 { 0xFFE0 } else { 0xF7E0 };
                if !was_enabled && d.enabled() {
                    let src_mask = if i == 0 { 0x07FF_FFFF } else { 0x0FFF_FFFF };
                    let dst_mask = if i == 3 { 0x0FFF_FFFF } else { 0x07FF_FFFF };
                    d.src = d.sad & src_mask;
                    d.dst = d.dad & dst_mask;
                    d.remaining = dma_count(i, d.count);
                    if d.timing() == DmaTiming::Immediate {
                        self.dma_pending |= 1 << i;
                    }
                }
            }
        }
    }

    pub fn trigger_dma(&mut self, timing_vblank: bool) {
        for i in 0..4 {
            let d = &self.dma[i];
            if d.enabled()
                && d.timing()
                    == if timing_vblank {
                        DmaTiming::VBlank
                    } else {
                        DmaTiming::HBlank
                    }
            {
                self.dma_pending |= 1 << i;
            }
        }
    }

    fn trigger_fifo_dma(&mut self, fifo: usize) {
        let channel = fifo + 1;
        let d = &self.dma[channel];
        let fifo_addr = 0x0400_00A0 + 4 * fifo as u32;
        if d.enabled() && d.timing() == DmaTiming::Special && d.dad == fifo_addr {
            self.dma_pending |= 1 << channel;
        }
    }

    pub fn dma_pending(&self) -> bool {
        self.dma_pending != 0
    }

    /// Run pending DMA transfers in priority order (DMA-57-01/02).
    pub fn run_dmas(&mut self) {
        if self.dma_running {
            return;
        }
        self.dma_running = true;
        while self.dma_pending != 0 {
            let i = self.dma_pending.trailing_zeros() as usize;
            self.dma_pending &= !(1 << i);
            self.run_dma(i);
        }
        self.dma_running = false;
    }

    fn run_dma(&mut self, i: usize) {
        let d = self.dma[i];
        if !d.enabled() {
            return;
        }
        let fifo = d.timing() == DmaTiming::Special && (i == 1 || i == 2);
        if d.timing() == DmaTiming::Special && i == 3 {
            // Video capture DMA is a missing capability.
            self.note_unsupported(0xDE);
            return;
        }
        let word = fifo || d.control & 0x0400 != 0;
        let count = if fifo { 4 } else { d.remaining };
        let step: u32 = if word { 4 } else { 2 };
        let src_step = match (d.control >> 7) & 3 {
            0 => step as i32,
            1 => -(step as i32),
            2 => 0,
            _ => step as i32,
        };
        let dst_step = if fifo {
            0
        } else {
            match (d.control >> 5) & 3 {
                0 | 3 => step as i32,
                1 => -(step as i32),
                _ => 0,
            }
        };
        // CART-60-01: the EEPROM size is inferred from the length of the
        // first DMA that addresses it (9/73 units → 512 B, 17/81 → 8 KiB).
        let rom_len = self.rom.len();
        if !fifo
            && (self.backup.is_eeprom_address(d.src, rom_len)
                || self.backup.is_eeprom_address(d.dst, rom_len))
        {
            self.backup.eeprom_dma_hint(count);
        }
        let mut src = d.src;
        let mut dst = d.dst;
        self.cycles += 2;
        for n in 0..count {
            let seq = n != 0;
            if word {
                self.charge(src, true, seq);
                let v = if src >= 0x0200_0000 {
                    self.read_raw32(src & !3)
                } else {
                    self.open_bus
                };
                self.charge(dst, true, seq);
                self.write_raw32(dst & !3, v);
                self.open_bus = v;
            } else {
                self.charge(src, false, seq);
                let v = if src >= 0x0200_0000 {
                    self.read_raw16(src & !1)
                } else {
                    self.open_bus as u16
                };
                self.charge(dst, false, seq);
                self.write_raw16(dst & !1, v);
                self.open_bus = u32::from(v) * 0x0001_0001;
            }
            src = src.wrapping_add(src_step as u32);
            dst = dst.wrapping_add(dst_step as u32);
        }
        let dm = &mut self.dma[i];
        dm.src = src;
        dm.dst = dst;
        if dm.control & 0x4000 != 0 {
            self.request_irq(IRQ_DMA0 << i);
        }
        let dm = &mut self.dma[i];
        if dm.control & 0x0200 != 0 && dm.timing() != DmaTiming::Immediate {
            dm.remaining = dma_count(i, dm.count);
            if (dm.control >> 5) & 3 == 3 {
                let dst_mask = if i == 3 { 0x0FFF_FFFF } else { 0x07FF_FFFF };
                dm.dst = dm.dad & dst_mask;
            }
        } else {
            dm.control &= !0x8000;
        }
    }

    // ------------------------------------------------------------ scheduler

    pub fn next_event(&self) -> u64 {
        self.next_event
    }

    /// Process all events due at or before the current cycle (SCH-57-01).
    pub fn process_events(&mut self) {
        while self.cycles >= self.next_event {
            let mut next = u64::MAX;
            if self.ppu_event <= self.cycles {
                let at = self.ppu_event;
                self.ppu_step(at);
            }
            next = next.min(self.ppu_event);
            for i in 0..4 {
                let at = self.timers[i].overflow_at;
                if at <= self.cycles {
                    self.timer_overflow(i, at);
                }
                next = next.min(self.timers[i].overflow_at);
            }
            let sample_at = self.apu.next_sample_at();
            if sample_at <= self.cycles {
                self.apu.run_until(self.cycles);
            }
            next = next.min(self.apu.next_sample_at());
            self.next_event = next;
        }
    }

    fn ppu_step(&mut self, at: u64) {
        if self.ppu.dispstat & 2 == 0 {
            // HDraw → HBlank.
            self.ppu.dispstat |= 2;
            if self.ppu.vcount < 160 {
                self.ppu.render_line(&self.io);
                self.trigger_dma(false);
            }
            if self.ppu.dispstat & 0x10 != 0 {
                self.request_irq(IRQ_HBLANK);
            }
            self.ppu_event = at + (CYCLES_PER_LINE - HDRAW_CYCLES);
        } else {
            // End of line.
            self.ppu.dispstat &= !2;
            self.ppu.vcount += 1;
            if self.ppu.vcount as u32 == LINES_PER_FRAME {
                self.ppu.vcount = 0;
            }
            let line = self.ppu.vcount;
            if line == 160 {
                self.ppu.dispstat |= 1;
                if self.ppu.dispstat & 0x08 != 0 {
                    self.request_irq(IRQ_VBLANK);
                }
                self.trigger_dma(true);
                self.ppu.vblank_latch(&self.io);
                self.frame_ready = true;
                self.frame_count += 1;
            } else if line == 227 {
                self.ppu.dispstat &= !1;
            }
            if line > 0 && line < 160 {
                self.ppu.advance_affine();
            }
            let target = self.ppu.dispstat >> 8;
            if line == target {
                self.ppu.dispstat |= 4;
                if self.ppu.dispstat & 0x20 != 0 {
                    self.request_irq(IRQ_VCOUNT);
                }
            } else {
                self.ppu.dispstat &= !4;
            }
            self.ppu_event = at + HDRAW_CYCLES;
        }
    }

    /// Skip to the next event while halted (IRQ-57-02).
    pub fn skip_to_next_event(&mut self) {
        if self.next_event > self.cycles && self.next_event != u64::MAX {
            self.cycles = self.next_event;
        }
    }
}

fn dma_count(channel: usize, count: u16) -> u32 {
    if count == 0 {
        if channel == 3 { 0x10000 } else { 0x4000 }
    } else if channel == 3 {
        u32::from(count)
    } else {
        // Channels 0-2 have a 14-bit counter: 0x4000 (masked to 0) also
        // means 0x4000 units.
        match u32::from(count & 0x3FFF) {
            0 => 0x4000,
            n => n,
        }
    }
}

impl CpuBus for Bus {
    #[inline]
    fn read8(&mut self, addr: u32, seq: bool) -> u8 {
        self.charge(addr, false, seq);
        self.read_raw8(addr)
    }
    #[inline]
    fn read16(&mut self, addr: u32, seq: bool) -> u16 {
        self.charge(addr, false, seq);
        self.read_raw16(addr)
    }
    #[inline]
    fn read32(&mut self, addr: u32, seq: bool) -> u32 {
        self.charge(addr, true, seq);
        self.read_raw32(addr)
    }
    #[inline]
    fn write8(&mut self, addr: u32, value: u8, seq: bool) {
        self.charge(addr, false, seq);
        self.write_raw8(addr, value);
    }
    #[inline]
    fn write16(&mut self, addr: u32, value: u16, seq: bool) {
        self.charge(addr, false, seq);
        self.write_raw16(addr, value);
    }
    #[inline]
    fn write32(&mut self, addr: u32, value: u32, seq: bool) {
        self.charge(addr, true, seq);
        self.write_raw32(addr, value);
    }
    #[inline]
    fn fetch16(&mut self, addr: u32, seq: bool) -> u16 {
        if self.prefetch && seq && (0x08..=0x0D).contains(&(addr >> 24)) {
            self.cycles += 1;
        } else {
            self.charge(addr, false, seq);
        }
        let v = self.read_raw16(addr);
        self.open_bus = u32::from(v) * 0x0001_0001;
        v
    }
    #[inline]
    fn fetch32(&mut self, addr: u32, seq: bool) -> u32 {
        if self.prefetch && seq && (0x08..=0x0D).contains(&(addr >> 24)) {
            self.cycles += 1;
        } else {
            self.charge(addr, true, seq);
        }
        let v = self.read_raw32(addr);
        self.open_bus = v;
        v
    }
    #[inline]
    fn idle(&mut self, cycles: u32) {
        self.cycles += u64::from(cycles);
    }
    fn swi_hle(&mut self, cpu: &mut Cpu, number: u8) -> bool {
        if !self.hle_bios {
            return false;
        }
        crate::bios::swi(self, cpu, number);
        true
    }
}

pub use ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};

#[cfg(test)]
pub(crate) mod tests;

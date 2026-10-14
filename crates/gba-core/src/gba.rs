//! System driver: CPU + bus + scheduler loop (SPEC-057).

use crate::backup::{Backup, BackupKind};
use crate::bus::{Bus, SystemFault};
use crate::cpu::{Cpu, StepResult};

/// Key bits for [`Gba::set_keys`] (KEYINPUT order).
pub mod keys {
    pub const A: u16 = 1 << 0;
    pub const B: u16 = 1 << 1;
    pub const SELECT: u16 = 1 << 2;
    pub const START: u16 = 1 << 3;
    pub const RIGHT: u16 = 1 << 4;
    pub const LEFT: u16 = 1 << 5;
    pub const UP: u16 = 1 << 6;
    pub const DOWN: u16 = 1 << 7;
    pub const R: u16 = 1 << 8;
    pub const L: u16 = 1 << 9;
}

pub struct GbaConfig<'a> {
    pub rom: &'a [u8],
    /// `None` selects the versioned HLE BIOS (BIOS-01).
    pub bios: Option<&'a [u8]>,
    pub backup: Option<BackupKind>,
    pub save: Option<&'a [u8]>,
}

pub struct Gba {
    pub cpu: Cpu,
    pub bus: Bus,
    pub instructions: u64,
}

impl Gba {
    pub fn new(config: GbaConfig<'_>) -> Self {
        let kind = config
            .backup
            .unwrap_or_else(|| crate::backup::detect(config.rom));
        let backup = Backup::new(kind, config.save);
        let mut bus = Bus::new(config.rom, config.bios, backup);
        let cpu = if bus.hle_bios {
            // Registers the BIOS leaves behind after its boot sequence.
            bus.poke16(0x0400_0088, 0x0200);
            bus.poke8(0x0400_0300, 1);
            Cpu::post_boot(0x0800_0000)
        } else {
            Cpu::new()
        };
        Self {
            cpu,
            bus,
            instructions: 0,
        }
    }

    pub fn set_keys(&mut self, pressed: u16) {
        self.bus.set_keys(pressed);
    }

    /// Execute one CPU step (or one halted wait) and service events.
    pub fn step(&mut self) -> Result<(), SystemFault> {
        if self.bus.dma_pending() {
            self.bus.run_dmas();
        }
        if self.bus.cycles >= self.bus.next_event() {
            self.bus.process_events();
            if self.bus.dma_pending() {
                self.bus.run_dmas();
            }
        }
        if self.cpu.halted {
            if self.bus.wake_condition() {
                self.cpu.halted = false;
            } else {
                self.bus.skip_to_next_event();
                self.bus.process_events();
                return Ok(());
            }
        }
        if self.bus.irq_line() {
            self.bus.pc_in_bios = true;
            self.cpu.irq(&mut self.bus);
        }
        self.bus.pc_in_bios = self.cpu.pc() < 0x4000;
        match self.cpu.step(&mut self.bus) {
            StepResult::Fault(fault) => return Err(SystemFault::Cpu(fault)),
            StepResult::Retired | StepResult::Skipped | StepResult::Exception => {}
        }
        self.instructions += 1;
        if self.bus.halt_requested || self.bus.stop_requested {
            self.bus.halt_requested = false;
            self.bus.stop_requested = false;
            self.cpu.halted = true;
        }
        if let Some(fault) = self.bus.fault.take() {
            return Err(fault);
        }
        Ok(())
    }

    /// Run until the PPU enters VBlank (one guest frame).
    pub fn run_frame(&mut self) -> Result<(), SystemFault> {
        self.bus.frame_ready = false;
        while !self.bus.frame_ready {
            self.step()?;
        }
        Ok(())
    }

    pub fn framebuffer(&self) -> &[u16] {
        &self.bus.ppu.framebuffer
    }

    pub fn take_audio(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.bus.apu.samples)
    }

    /// FNV-1a over the raw framebuffer (AV-01 canonical hash).
    pub fn frame_hash(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for &p in self.framebuffer() {
            for b in p.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01B3);
            }
        }
        h
    }
}

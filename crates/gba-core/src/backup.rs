//! Cartridge backup media (SPEC-060, CART-01): SRAM, Flash 64/128 KiB and
//! EEPROM 512 B/8 KiB.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupKind {
    None,
    Sram,
    Flash64,
    Flash128,
    /// EEPROM with size still unknown (resolved from the first DMA length).
    Eeprom,
    Eeprom512,
    Eeprom8k,
}

impl BackupKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BackupKind::None => "none",
            BackupKind::Sram => "sram",
            BackupKind::Flash64 => "flash64",
            BackupKind::Flash128 => "flash128",
            BackupKind::Eeprom => "eeprom",
            BackupKind::Eeprom512 => "eeprom512",
            BackupKind::Eeprom8k => "eeprom8k",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "none" => BackupKind::None,
            "sram" => BackupKind::Sram,
            "flash64" => BackupKind::Flash64,
            "flash128" => BackupKind::Flash128,
            "eeprom" => BackupKind::Eeprom,
            "eeprom512" => BackupKind::Eeprom512,
            "eeprom8k" => BackupKind::Eeprom8k,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            BackupKind::None => 0,
            BackupKind::Sram => 0x8000,
            BackupKind::Flash64 => 0x10000,
            BackupKind::Flash128 => 0x20000,
            BackupKind::Eeprom | BackupKind::Eeprom8k => 0x2000,
            BackupKind::Eeprom512 => 0x200,
        }
    }
}

/// Detect the backup type from library signature strings in the ROM. The
/// result is a hint; profiles may override it (CART-01).
pub fn detect(rom: &[u8]) -> BackupKind {
    let find = |needle: &[u8]| rom.windows(needle.len()).any(|w| w == needle);
    if find(b"EEPROM_V") {
        BackupKind::Eeprom
    } else if find(b"FLASH1M_V") {
        BackupKind::Flash128
    } else if find(b"FLASH512_V") || find(b"FLASH_V") {
        BackupKind::Flash64
    } else if find(b"SRAM_V") || find(b"SRAM_F_V") {
        BackupKind::Sram
    } else {
        BackupKind::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlashState {
    Ready,
    Cmd1,
    Cmd2,
    EraseReady,
    Erase1,
    Erase2,
    Write,
    Bank,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum EepromState {
    Idle,
    Reading {
        bits_left: u32,
        value: u64,
        dummy: u32,
    },
}

pub struct Backup {
    kind: BackupKind,
    data: Vec<u8>,
    pub dirty: bool,
    flash_state: FlashState,
    flash_id_mode: bool,
    flash_bank: usize,
    eeprom_bits: Vec<u8>,
    eeprom_state: EepromState,
}

impl Backup {
    pub fn new(kind: BackupKind, existing: Option<&[u8]>) -> Self {
        let mut data = vec![0xFF; kind.size()];
        if let Some(saved) = existing {
            let n = saved.len().min(data.len());
            data[..n].copy_from_slice(&saved[..n]);
        }
        Self {
            kind,
            data,
            dirty: false,
            flash_state: FlashState::Ready,
            flash_id_mode: false,
            flash_bank: 0,
            eeprom_bits: Vec::new(),
            eeprom_state: EepromState::Idle,
        }
    }

    pub fn kind(&self) -> BackupKind {
        self.kind
    }

    pub fn data(&self) -> &[u8] {
        match self.kind {
            BackupKind::Eeprom512 => &self.data[..0x200],
            _ => &self.data,
        }
    }

    fn is_eeprom(&self) -> bool {
        matches!(
            self.kind,
            BackupKind::Eeprom | BackupKind::Eeprom512 | BackupKind::Eeprom8k
        )
    }

    pub fn is_eeprom_address(&self, addr: u32, rom_len: usize) -> bool {
        if !self.is_eeprom() || addr >> 24 != 0x0D {
            return false;
        }
        rom_len <= 0x0100_0000 || addr >= 0x0DFF_FF00
    }

    /// Size inference from the DMA length used for an EEPROM request.
    pub fn eeprom_dma_hint(&mut self, count: u32) {
        if self.kind == BackupKind::Eeprom {
            match count {
                9 | 73 => self.kind = BackupKind::Eeprom512,
                17 | 81 => self.kind = BackupKind::Eeprom8k,
                _ => {}
            }
        }
    }

    pub fn gpio_write16(&mut self, _addr: u32, _value: u16) {}

    // ---------------------------------------------------------------- SRAM/Flash

    pub fn read8(&mut self, addr: u32) -> u8 {
        let offset = (addr & 0xFFFF) as usize;
        match self.kind {
            BackupKind::Sram => self.data[offset & 0x7FFF],
            BackupKind::Flash64 | BackupKind::Flash128 => {
                if self.flash_id_mode && offset < 2 {
                    let (maker, device) = if self.kind == BackupKind::Flash128 {
                        (0xC2, 0x09)
                    } else {
                        (0x32, 0x1B)
                    };
                    return if offset == 0 { maker } else { device };
                }
                self.data[self.flash_bank * 0x10000 + offset]
            }
            _ => 0xFF,
        }
    }

    pub fn write8(&mut self, addr: u32, value: u8) {
        let offset = (addr & 0xFFFF) as usize;
        match self.kind {
            BackupKind::Sram => {
                self.data[offset & 0x7FFF] = value;
                self.dirty = true;
            }
            BackupKind::Flash64 | BackupKind::Flash128 => self.flash_write(offset, value),
            _ => {}
        }
    }

    fn flash_write(&mut self, offset: usize, value: u8) {
        use FlashState::*;
        self.flash_state = match self.flash_state {
            Write => {
                self.data[self.flash_bank * 0x10000 + offset] &= value;
                self.dirty = true;
                Ready
            }
            Bank => {
                if offset == 0 && self.kind == BackupKind::Flash128 {
                    self.flash_bank = usize::from(value & 1);
                }
                Ready
            }
            Ready if offset == 0x5555 && value == 0xAA => Cmd1,
            EraseReady if offset == 0x5555 && value == 0xAA => Erase1,
            Cmd1 if offset == 0x2AAA && value == 0x55 => Cmd2,
            Erase1 if offset == 0x2AAA && value == 0x55 => Erase2,
            Cmd2 if offset == 0x5555 => match value {
                0x90 => {
                    self.flash_id_mode = true;
                    Ready
                }
                0xF0 => {
                    self.flash_id_mode = false;
                    Ready
                }
                0x80 => EraseReady,
                0xA0 => Write,
                0xB0 => Bank,
                _ => Ready,
            },
            Erase2 => {
                if offset == 0x5555 && value == 0x10 {
                    self.data.fill(0xFF);
                    self.dirty = true;
                } else if value == 0x30 {
                    let start = self.flash_bank * 0x10000 + (offset & 0xF000);
                    self.data[start..start + 0x1000].fill(0xFF);
                    self.dirty = true;
                }
                Ready
            }
            _ => {
                if value == 0xF0 {
                    self.flash_id_mode = false;
                }
                Ready
            }
        };
    }

    // ---------------------------------------------------------------- EEPROM

    fn eeprom_addr_bits(&self) -> usize {
        if self.kind == BackupKind::Eeprom512 {
            6
        } else {
            14
        }
    }

    pub fn eeprom_write(&mut self, value: u16) {
        self.eeprom_bits.push((value & 1) as u8);
        let addr_bits = self.eeprom_addr_bits();
        let bits = &self.eeprom_bits;
        if bits.len() < 2 {
            return;
        }
        let read = bits[0] == 1 && bits[1] == 1;
        let write = bits[0] == 1 && bits[1] == 0;
        let to_num = |slice: &[u8]| slice.iter().fold(0u64, |acc, b| (acc << 1) | u64::from(*b));
        if read && bits.len() == 2 + addr_bits + 1 {
            let block = (to_num(&bits[2..2 + addr_bits]) as usize) & 0x3FF;
            let base = (block * 8) % self.data.len();
            let mut v = 0u64;
            for i in 0..8 {
                v = (v << 8) | u64::from(self.data[base + i]);
            }
            self.eeprom_state = EepromState::Reading {
                bits_left: 64,
                value: v,
                dummy: 4,
            };
            self.eeprom_bits.clear();
        } else if write && bits.len() == 2 + addr_bits + 64 + 1 {
            let block = (to_num(&bits[2..2 + addr_bits]) as usize) & 0x3FF;
            let base = (block * 8) % self.data.len();
            let data = to_num(&bits[2 + addr_bits..2 + addr_bits + 64]);
            for i in 0..8 {
                self.data[base + i] = (data >> (56 - i * 8)) as u8;
            }
            self.dirty = true;
            self.eeprom_state = EepromState::Idle;
            self.eeprom_bits.clear();
        } else if !read && !write {
            self.eeprom_bits.clear();
        }
    }

    pub fn eeprom_read(&mut self) -> u16 {
        match &mut self.eeprom_state {
            EepromState::Idle => 1,
            EepromState::Reading {
                bits_left,
                value,
                dummy,
            } => {
                if *dummy > 0 {
                    *dummy -= 1;
                    return 0;
                }
                *bits_left -= 1;
                let bit = (*value >> *bits_left) & 1;
                if *bits_left == 0 {
                    self.eeprom_state = EepromState::Idle;
                }
                bit as u16
            }
        }
    }
}

#[cfg(test)]
mod tests;

use std::path::Path;

use sha2::{Digest, Sha256};

pub const ROM_BASE: u32 = 0x0800_0000;
pub const ROM_HEADER_SIZE: usize = 0xC0;
pub const MAX_ROM_SIZE: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RomHeader {
    pub title: String,
    pub game_code: String,
    pub maker_code: String,
    pub version: u8,
    pub fixed_value: u8,
    pub stored_checksum: u8,
    pub calculated_checksum: u8,
}

#[derive(Clone, Debug)]
pub struct RomImage {
    bytes: Vec<u8>,
    pub sha256: String,
    pub header: RomHeader,
}

impl RomImage {
    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("cannot read ROM {}: {error}", path.display()))?;
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() < ROM_HEADER_SIZE {
            return Err(format!(
                "image_too_small: {} bytes; GBA header requires at least {ROM_HEADER_SIZE:#X}",
                bytes.len()
            ));
        }
        if bytes.len() > MAX_ROM_SIZE {
            return Err(format!(
                "image_too_large: {} bytes; baseline maximum is {MAX_ROM_SIZE}",
                bytes.len()
            ));
        }
        if !bytes.len().is_power_of_two() {
            return Err(format!(
                "image_size_unmapped: {} bytes is not a power of two",
                bytes.len()
            ));
        }
        let fixed_value = bytes[0xB2];
        if fixed_value != 0x96 {
            return Err(format!(
                "header_invalid: fixed value at 0xB2 is 0x{fixed_value:02X}, expected 0x96"
            ));
        }
        let calculated_checksum = header_checksum(&bytes);
        let stored_checksum = bytes[0xBD];
        if stored_checksum != calculated_checksum {
            return Err(format!(
                "header_invalid: checksum at 0xBD is 0x{stored_checksum:02X}, calculated 0x{calculated_checksum:02X}"
            ));
        }
        let header = RomHeader {
            title: header_text(&bytes[0xA0..0xAC]),
            game_code: header_text(&bytes[0xAC..0xB0]),
            maker_code: header_text(&bytes[0xB0..0xB2]),
            version: bytes[0xBC],
            fixed_value,
            stored_checksum,
            calculated_checksum,
        };
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        Ok(Self {
            bytes,
            sha256,
            header,
        })
    }

    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    #[cfg(test)]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn offset_for_guest(&self, address: u32, length: usize) -> Result<usize, String> {
        let offset = address
            .checked_sub(ROM_BASE)
            .ok_or_else(|| format!("guest address 0x{address:08X} is below ROM base"))?
            as usize;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| format!("guest range at 0x{address:08X} overflows"))?;
        if end > self.bytes.len() {
            return Err(format!(
                "guest range 0x{address:08X}..0x{:08X} is outside ROM image ({} bytes)",
                address.saturating_add(length as u32),
                self.bytes.len()
            ));
        }
        Ok(offset)
    }

    pub fn read_arm_word(&self, address: u32) -> Result<u32, String> {
        if address & 3 != 0 {
            return Err(format!("ARM address 0x{address:08X} is not word aligned"));
        }
        let offset = self.offset_for_guest(address, 4)?;
        Ok(u32::from_le_bytes(
            self.bytes[offset..offset + 4]
                .try_into()
                .expect("four bytes were range checked"),
        ))
    }

    pub fn read_thumb_halfword(&self, address: u32) -> Result<u16, String> {
        if address & 1 != 0 {
            return Err(format!(
                "Thumb address 0x{address:08X} is not halfword aligned"
            ));
        }
        let offset = self.offset_for_guest(address, 2)?;
        Ok(u16::from_le_bytes(
            self.bytes[offset..offset + 2]
                .try_into()
                .expect("two bytes were range checked"),
        ))
    }
}

pub fn header_checksum(bytes: &[u8]) -> u8 {
    let sum = bytes[0xA0..0xBD]
        .iter()
        .fold(0u32, |sum, byte| sum + u32::from(*byte));
    (0u32.wrapping_sub(sum).wrapping_sub(0x19) & 0xFF) as u8
}

fn header_text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .map(char::from)
        .collect::<String>()
        .trim_end_matches(' ')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_image() -> Vec<u8> {
        let mut bytes = vec![0; 256];
        bytes[0xA0..0xAC].copy_from_slice(b"TINY COUNTER");
        bytes[0xAC..0xB0].copy_from_slice(b"TCGE");
        bytes[0xB0..0xB2].copy_from_slice(b"00");
        bytes[0xB2] = 0x96;
        bytes[0xBC] = 1;
        bytes[0xBD] = header_checksum(&bytes);
        bytes
    }

    #[test]
    fn image_identity_covers_all_bytes_and_header_is_parsed() {
        let first = RomImage::from_bytes(valid_image()).unwrap();
        assert_eq!(first.header.title, "TINY COUNTER");
        assert_eq!(first.header.game_code, "TCGE");
        let mut changed = first.bytes().to_vec();
        changed[0xC0] = 1;
        changed[0xBD] = header_checksum(&changed);
        let second = RomImage::from_bytes(changed).unwrap();
        assert_ne!(first.sha256, second.sha256);
    }

    #[test]
    fn rejects_short_non_power_of_two_and_bad_checksum_images() {
        assert!(
            RomImage::from_bytes(vec![0; 0xBF])
                .unwrap_err()
                .contains("image_too_small")
        );
        assert!(
            RomImage::from_bytes(vec![0; 0xC1])
                .unwrap_err()
                .contains("image_size_unmapped")
        );
        let mut invalid = valid_image();
        invalid[0xBD] ^= 1;
        assert!(
            RomImage::from_bytes(invalid)
                .unwrap_err()
                .contains("header_invalid: checksum")
        );
    }

    #[test]
    fn guest_address_mapping_checks_bounds_and_alignment() {
        let image = RomImage::from_bytes(valid_image()).unwrap();
        assert_eq!(image.read_arm_word(ROM_BASE).unwrap(), 0);
        assert!(image.read_arm_word(ROM_BASE + 1).is_err());
        assert!(image.read_arm_word(ROM_BASE + 256).is_err());
    }

    #[test]
    fn reads_little_endian_thumb_halfwords_with_alignment_and_bounds_checks() {
        let mut bytes = valid_image();
        bytes[0xC0..0xC2].copy_from_slice(&0x4708_u16.to_le_bytes());
        let image = RomImage::from_bytes(bytes).unwrap();
        assert_eq!(image.read_thumb_halfword(ROM_BASE + 0xC0).unwrap(), 0x4708);
        assert!(image.read_thumb_halfword(ROM_BASE + 0xC1).is_err());
        assert!(image.read_thumb_halfword(ROM_BASE + 256).is_err());
    }
}

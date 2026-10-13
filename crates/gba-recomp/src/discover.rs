use std::collections::{BTreeMap, VecDeque};

use gba_ir::{ArmInstruction, ArmOperation, ArmProgram, GBA_IR_VERSION};

use crate::rom::{ROM_BASE, RomImage};

const MAX_DISCOVERED_INSTRUCTIONS: usize = 1_000_000;

pub fn discover_arm_program(image: &RomImage) -> Result<ArmProgram, String> {
    let mut pending = VecDeque::from([ROM_BASE]);
    let mut decoded = BTreeMap::<u32, ArmInstruction>::new();
    while let Some(mut pc) = pending.pop_front() {
        loop {
            if decoded.contains_key(&pc) {
                break;
            }
            if decoded.len() >= MAX_DISCOVERED_INSTRUCTIONS {
                return Err(format!(
                    "discovery_limit: more than {MAX_DISCOVERED_INSTRUCTIONS} reachable instructions"
                ));
            }
            let encoding = image
                .read_arm_word(pc)
                .map_err(|error| format!("discovery failed at 0x{pc:08X}: {error}"))?;
            let mut instruction = crate::decoder::decode_arm(pc, encoding)?;
            if let ArmOperation::LoadPcRelativeWord { rd, guest_address } = instruction.operation {
                let value = image.read_arm_word(guest_address).map_err(|error| {
                    format!(
                        "rom_literal_out_of_rom at 0x{pc:08X}: address 0x{guest_address:08X}: {error}"
                    )
                })?;
                instruction.operation = ArmOperation::LoadRomLiteral {
                    rd,
                    guest_address,
                    value,
                };
            }
            let next_pc = pc
                .checked_add(4)
                .ok_or_else(|| format!("sequential PC overflow after 0x{pc:08X}"))?;
            match instruction.operation {
                ArmOperation::Branch { target, link } => {
                    ensure_target_in_image(image, pc, target)?;
                    pending.push_back(target);
                    if instruction.condition == 14 && !link {
                        decoded.insert(pc, instruction);
                        break;
                    }
                    // A conditional branch may fall through. BL also has a
                    // return continuation; any unsupported return encoding
                    // will fail discovery before code is published.
                    decoded.insert(pc, instruction);
                    pc = next_pc;
                }
                _ => {
                    decoded.insert(pc, instruction);
                    pc = next_pc;
                }
            }
        }
    }
    let program = ArmProgram {
        version: GBA_IR_VERSION,
        instructions: decoded.into_values().collect(),
    };
    program
        .verify()
        .map_err(|error| format!("GBA IR rejected: {error}"))?;
    Ok(program)
}

fn ensure_target_in_image(image: &RomImage, pc: u32, target: u32) -> Result<(), String> {
    image.offset_for_guest(target, 4).map_err(|error| {
        format!("branch_target_out_of_rom at 0x{pc:08X}: target 0x{target:08X}: {error}")
    })?;
    if target & 3 != 0 {
        return Err(format!(
            "branch_target_misaligned at 0x{pc:08X}: target 0x{target:08X}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::header_checksum;

    fn counter_rom() -> RomImage {
        let mut bytes = vec![0; 256];
        bytes[0..4].copy_from_slice(&0xEA00_002E_u32.to_le_bytes());
        bytes[0xA0..0xAC].copy_from_slice(b"TINY COUNTER");
        bytes[0xAC..0xB0].copy_from_slice(b"TCGE");
        bytes[0xB0..0xB2].copy_from_slice(b"00");
        bytes[0xB2] = 0x96;
        bytes[0xBD] = header_checksum(&bytes);
        let words = [
            0xE3A0_0000_u32,
            0xE280_0001,
            0xE350_000A,
            0x1AFF_FFFC,
            0xEAFF_FFFE,
        ];
        for (index, word) in words.into_iter().enumerate() {
            let offset = 0xC0 + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        RomImage::from_bytes(bytes).unwrap()
    }

    fn literal_rom() -> RomImage {
        let mut bytes = counter_rom().bytes().to_vec();
        // `LDR r0, [pc, #4]` at 0x080000C0 reaches 0x080000CC.
        bytes[0xC0..0xC4].copy_from_slice(&0xE59F_0004_u32.to_le_bytes());
        bytes[0xC4..0xC8].copy_from_slice(&0xEAFF_FFFE_u32.to_le_bytes());
        bytes[0xCC..0xD0].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
        bytes[0xBD] = header_checksum(&bytes);
        RomImage::from_bytes(bytes).unwrap()
    }

    #[test]
    fn discovers_entry_branch_loop_and_fallthrough() {
        let program = discover_arm_program(&counter_rom()).unwrap();
        assert_eq!(program.instructions.len(), 6);
        assert_eq!(program.instructions[0].guest_pc, ROM_BASE);
        assert!(
            program
                .instructions
                .iter()
                .any(|instruction| instruction.guest_pc == ROM_BASE + 0xD0)
        );
    }

    #[test]
    fn out_of_rom_branch_is_rejected_during_discovery() {
        let mut bytes = counter_rom().bytes().to_vec();
        bytes[0xC0..0xC4].copy_from_slice(&0xEA00_003E_u32.to_le_bytes());
        bytes[0xBD] = header_checksum(&bytes);
        let image = RomImage::from_bytes(bytes).unwrap();
        assert!(
            discover_arm_program(&image)
                .unwrap_err()
                .contains("branch_target_out_of_rom")
        );
    }

    #[test]
    fn resolves_pc_relative_word_from_rom_into_a_constant() {
        let program = discover_arm_program(&literal_rom()).unwrap();
        let literal = program
            .instructions
            .iter()
            .find(|instruction| instruction.guest_pc == ROM_BASE + 0xC0)
            .unwrap();
        assert_eq!(
            literal.operation,
            ArmOperation::LoadRomLiteral {
                rd: 0,
                guest_address: ROM_BASE + 0xCC,
                value: 0x1234_5678,
            }
        );
        assert_eq!(program.instructions.len(), 3);
        assert!(program.verify().is_ok());
    }

    #[test]
    fn out_of_rom_literal_is_rejected_during_discovery() {
        let mut bytes = counter_rom().bytes().to_vec();
        // PC+8 plus 0x138 is beyond this 256-byte image.
        bytes[0xC0..0xC4].copy_from_slice(&0xE59F_0138_u32.to_le_bytes());
        bytes[0xBD] = header_checksum(&bytes);
        let image = RomImage::from_bytes(bytes).unwrap();
        let error = discover_arm_program(&image).unwrap_err();
        assert!(error.contains("rom_literal_out_of_rom at 0x080000C0"));
        assert!(error.contains("address 0x08000200"));
    }
}

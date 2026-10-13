use gba_ir::{ArmInstruction, ArmOperation};

pub fn decode_arm(guest_pc: u32, encoding: u32) -> Result<ArmInstruction, String> {
    if guest_pc & 3 != 0 {
        return Err(format!("ARM address 0x{guest_pc:08X} is not word aligned"));
    }
    let condition = (encoding >> 28) as u8;
    if condition == 15 {
        return unsupported(guest_pc, encoding, "condition 0xF is outside ARMv4T subset");
    }

    let operation = if encoding & 0x0E00_0000 == 0x0A00_0000 {
        let immediate = (encoding & 0x00FF_FFFF) as i32;
        let signed_immediate = (immediate << 8) >> 8;
        let displacement = signed_immediate << 2;
        let target = guest_pc.wrapping_add(8).wrapping_add(displacement as u32);
        ArmOperation::Branch {
            target,
            link: encoding & (1 << 24) != 0,
        }
    } else if encoding & 0x0E00_0000 == 0x0200_0000 {
        let opcode = (encoding >> 21) & 0xF;
        let set_flags = encoding & (1 << 20) != 0;
        let rn = ((encoding >> 16) & 0xF) as u8;
        let rd = ((encoding >> 12) & 0xF) as u8;
        let rotation = ((encoding >> 8) & 0xF) * 2;
        let immediate = (encoding & 0xFF).rotate_right(rotation) as u32;
        match (opcode, set_flags) {
            (13, false) if rn == 0 && rd < 15 => ArmOperation::MovImmediate {
                rd,
                value: immediate,
            },
            (4, false) if rn < 15 && rd < 15 => ArmOperation::AddImmediate {
                rd,
                rn,
                value: immediate,
            },
            (10, true) if rd == 0 && rn < 15 => ArmOperation::CompareImmediate {
                rn,
                value: immediate,
            },
            _ => {
                return unsupported(
                    guest_pc,
                    encoding,
                    "data-processing opcode or flags mode is not implemented",
                );
            }
        }
    } else {
        return unsupported(guest_pc, encoding, "instruction family is not implemented");
    };

    Ok(ArmInstruction {
        guest_pc,
        encoding,
        condition,
        operation,
    })
}

fn unsupported<T>(guest_pc: u32, encoding: u32, reason: &str) -> Result<T, String> {
    Err(format!(
        "unsupported_instruction at 0x{guest_pc:08X}: 0x{encoding:08X} ({reason})"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_homebrew_subset_and_sign_extends_branch() {
        assert_eq!(
            decode_arm(0x0800_00C0, 0xE3A0_0000).unwrap().operation,
            ArmOperation::MovImmediate { rd: 0, value: 0 }
        );
        assert_eq!(
            decode_arm(0x0800_00C4, 0xE280_0001).unwrap().operation,
            ArmOperation::AddImmediate {
                rd: 0,
                rn: 0,
                value: 1
            }
        );
        assert_eq!(
            decode_arm(0x0800_00CC, 0x1AFF_FFFC).unwrap().operation,
            ArmOperation::Branch {
                target: 0x0800_00C4,
                link: false
            }
        );
    }

    #[test]
    fn rejects_unimplemented_and_undefined_encodings_with_source_pc() {
        assert!(
            decode_arm(0x0800_0120, 0xE591_0000)
                .unwrap_err()
                .contains("unsupported_instruction at 0x08000120")
        );
        assert!(
            decode_arm(0x0800_0124, 0xF000_0000)
                .unwrap_err()
                .contains("condition 0xF")
        );
    }
}

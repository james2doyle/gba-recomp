use gba_ir::{ArmInstruction, ArmOperation, ThumbInstruction, ThumbOperation};

pub fn decode_arm(guest_pc: u32, encoding: u32) -> Result<ArmInstruction, String> {
    if guest_pc & 3 != 0 {
        return Err(format!("ARM address 0x{guest_pc:08X} is not word aligned"));
    }
    let condition = (encoding >> 28) as u8;
    if condition == 15 {
        return unsupported(guest_pc, encoding, "condition 0xF is outside ARMv4T subset");
    }

    let operation = if encoding & 0x0FFF_FFF0 == 0x012F_FF10 {
        ArmOperation::BranchExchange {
            rm: (encoding & 0xF) as u8,
        }
    } else if encoding & 0x0C00_0000 == 0x0400_0000 {
        let immediate_offset = encoding & (1 << 25) == 0;
        let pre_indexed = encoding & (1 << 24) != 0;
        let byte_transfer = encoding & (1 << 22) != 0;
        let writeback = encoding & (1 << 21) != 0;
        let load = encoding & (1 << 20) != 0;
        let rn = ((encoding >> 16) & 0xF) as u8;
        let rd = ((encoding >> 12) & 0xF) as u8;
        if !load {
            if !immediate_offset
                || !pre_indexed
                || byte_transfer
                || writeback
                || rn == 15
                || rd == 15
            {
                return unsupported(
                    guest_pc,
                    encoding,
                    "only immediate pre-indexed STR word without writeback to a general register base is implemented",
                );
            }
            return Ok(ArmInstruction {
                guest_pc,
                encoding,
                condition,
                operation: ArmOperation::StoreWord {
                    rd,
                    rn,
                    offset: (encoding & 0xFFF) as u16,
                    up: encoding & (1 << 23) != 0,
                },
            });
        }
        if !immediate_offset || !pre_indexed || byte_transfer || writeback || rn != 15 || rd == 15 {
            return unsupported(
                guest_pc,
                encoding,
                "only immediate pre-indexed LDR word from PC without writeback is implemented",
            );
        }
        let pc_operand = guest_pc
            .checked_add(8)
            .ok_or_else(|| format!("PC-relative address overflow at 0x{guest_pc:08X}"))?;
        let offset = encoding & 0xFFF;
        let guest_address = if encoding & (1 << 23) != 0 {
            pc_operand.checked_add(offset)
        } else {
            pc_operand.checked_sub(offset)
        }
        .ok_or_else(|| format!("PC-relative address overflow at 0x{guest_pc:08X}"))?;
        if guest_address & 3 != 0 {
            return unsupported(
                guest_pc,
                encoding,
                "PC-relative LDR word address is not 4-byte aligned",
            );
        }
        ArmOperation::LoadPcRelativeWord { rd, guest_address }
    } else if encoding & 0x0FB0_FFF0 == 0x0120_F000 {
        if encoding & (1 << 22) != 0 {
            return unsupported(guest_pc, encoding, "MSR SPSR is not implemented");
        }
        let field_mask = ((encoding >> 16) & 0xF) as u8;
        if field_mask & !0x9 != 0 {
            return unsupported(guest_pc, encoding, "MSR fields x/s are not implemented");
        }
        let rm = (encoding & 0xF) as u8;
        if rm == 15 {
            return unsupported(guest_pc, encoding, "MSR source r15 is not implemented");
        }
        ArmOperation::MsrCpsrRegister { rm, field_mask }
    } else if encoding & 0x0E00_0000 == 0 && (encoding >> 21) & 0xF == 13 {
        let set_flags = encoding & (1 << 20) != 0;
        let rn = ((encoding >> 16) & 0xF) as u8;
        let rd = ((encoding >> 12) & 0xF) as u8;
        let rm = (encoding & 0xF) as u8;
        if set_flags || rn != 0 || rd == 15 || encoding & 0xFF0 != 0 {
            return unsupported(
                guest_pc,
                encoding,
                "only MOV Rd,Rm with LSL #0, no flags, and a non-PC destination is implemented",
            );
        }
        ArmOperation::MovRegister { rd, rm }
    } else if encoding & 0x0E00_0000 == 0x0A00_0000 {
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
            (4, false) if rn == 15 && rd < 15 => ArmOperation::AddPcRelativeImmediate {
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

pub fn decode_thumb(guest_pc: u32, encoding: u16) -> Result<ThumbInstruction, String> {
    if guest_pc & 1 != 0 {
        return Err(format!(
            "Thumb address 0x{guest_pc:08X} is not halfword aligned"
        ));
    }
    if encoding & 0xFF87 == 0x4700 {
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::BranchExchange {
                rm: ((encoding >> 3) & 0xF) as u8,
            },
        });
    }
    if encoding & 0xFE00 == 0xB400 {
        let register_list = encoding & 0x01FF;
        if register_list == 0 {
            return Err(format!(
                "unsupported_thumb_instruction at 0x{guest_pc:08X}: 0x{encoding:04X} (empty PUSH register list)"
            ));
        }
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::Push { register_list },
        });
    }
    if encoding & 0xF800 == 0x2000 {
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::MovImmediate {
                rd: ((encoding >> 8) & 0x7) as u8,
                value: (encoding & 0xFF) as u8,
            },
        });
    }
    if encoding & 0xFC00 == 0x4400 {
        let opcode = (encoding >> 8) & 0x3;
        if opcode != 0x2 {
            return Err(format!(
                "unsupported_thumb_instruction at 0x{guest_pc:08X}: 0x{encoding:04X} (only high-register MOV is implemented)"
            ));
        }
        let rd = ((encoding & 0x7) | ((encoding >> 4) & 0x8)) as u8;
        if rd == 15 {
            return Err(format!(
                "unsupported_thumb_instruction at 0x{guest_pc:08X}: 0x{encoding:04X} (MOV destination PC is not implemented)"
            ));
        }
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::MovRegister {
                rd,
                rm: ((encoding >> 3) & 0xF) as u8,
            },
        });
    }
    Err(format!(
        "unsupported_thumb_instruction at 0x{guest_pc:08X}: 0x{encoding:04X}"
    ))
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
        assert_eq!(
            decode_arm(0x0800_0208, 0xE129_F000).unwrap().operation,
            ArmOperation::MsrCpsrRegister {
                rm: 0,
                field_mask: 0x9
            }
        );
        assert_eq!(
            decode_arm(0x0800_00C0, 0xE59F_0004).unwrap().operation,
            ArmOperation::LoadPcRelativeWord {
                rd: 0,
                guest_address: 0x0800_00CC
            }
        );
        assert_eq!(
            decode_arm(0x0800_00C0, 0xE51F_1004).unwrap().operation,
            ArmOperation::LoadPcRelativeWord {
                rd: 1,
                guest_address: 0x0800_00C4
            }
        );
        assert_eq!(
            decode_arm(0x0800_0220, 0xE28F_0020).unwrap().operation,
            ArmOperation::AddPcRelativeImmediate { rd: 0, value: 0x20 }
        );
        assert_eq!(
            decode_arm(0x0800_0220, 0xE28F_0F01).unwrap().operation,
            ArmOperation::AddPcRelativeImmediate { rd: 0, value: 4 }
        );
        assert_eq!(
            decode_arm(0x0800_022C, 0xE1A0_E00F).unwrap().operation,
            ArmOperation::MovRegister { rd: 14, rm: 15 }
        );
        assert_eq!(
            decode_arm(0x0800_0230, 0xE12F_FF11).unwrap().operation,
            ArmOperation::BranchExchange { rm: 1 }
        );
        assert_eq!(
            decode_arm(0x0800_0230, 0xE1A0_0001).unwrap().operation,
            ArmOperation::MovRegister { rd: 0, rm: 1 }
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
        assert!(
            decode_arm(0x0800_0128, 0xE169_F000)
                .unwrap_err()
                .contains("MSR SPSR")
        );
        assert!(
            decode_arm(0x0800_012C, 0xE122_F000)
                .unwrap_err()
                .contains("fields x/s")
        );
        assert!(
            decode_arm(0x0800_0130, 0xE591_0000)
                .unwrap_err()
                .contains("only immediate pre-indexed LDR word from PC")
        );
        assert!(
            decode_arm(0x0800_0134, 0xE59F_0002)
                .unwrap_err()
                .contains("not 4-byte aligned")
        );
        assert!(decode_arm(0x0800_0138, 0xE29F_0020).is_err());
        assert!(decode_arm(0x0800_013C, 0xE28F_F020).is_err());
        assert!(decode_arm(0x0800_0140, 0xE1B0_0001).is_err()); // MOVS
        assert!(decode_arm(0x0800_0144, 0xE1A0_0081).is_err()); // LSL #1
        assert!(decode_arm(0x0800_0148, 0xE1A0_F001).is_err()); // destination PC
        assert!(decode_arm(0x0800_014C, 0xE1A0_0031).is_err()); // register-specified shift
        assert!(decode_arm(0x0800_0150, 0xE1A1_0001).is_err()); // nonzero ignored Rn
    }

    #[test]
    fn decodes_thumb_branch_exchange_and_movs_immediate() {
        assert_eq!(
            decode_thumb(0x0800_0084, 0x4708).unwrap().operation,
            ThumbOperation::BranchExchange { rm: 1 }
        );
        assert_eq!(
            decode_thumb(0x0800_0084, 0x2007).unwrap().operation,
            ThumbOperation::MovImmediate { rd: 0, value: 7 }
        );
        assert!(decode_thumb(0x0800_0085, 0x4708).is_err());
    }

    #[test]
    fn decodes_thumb_push_register_list_and_rejects_empty_list() {
        assert_eq!(
            decode_thumb(0x0800_03A4, 0xB5F0).unwrap().operation,
            ThumbOperation::Push {
                register_list: 0x01F0
            }
        );
        assert!(
            decode_thumb(0x0800_03A4, 0xB400)
                .unwrap_err()
                .contains("empty PUSH register list")
        );
        assert!(decode_thumb(0x0800_03A4, 0xB600).is_err());
    }

    #[test]
    fn decodes_only_thumb_high_register_mov_without_pc_destination() {
        assert_eq!(
            decode_thumb(0x0800_03A6, 0x4647).unwrap().operation,
            ThumbOperation::MovRegister { rd: 7, rm: 8 }
        );
        assert_eq!(
            decode_thumb(0x0800_03A6, 0x4678).unwrap().operation,
            ThumbOperation::MovRegister { rd: 0, rm: 15 }
        );
        assert!(
            decode_thumb(0x0800_03A6, 0x468F)
                .unwrap_err()
                .contains("destination PC")
        );
        assert!(decode_thumb(0x0800_03A6, 0x44C7).is_err()); // ADD high-register.
        assert!(decode_thumb(0x0800_03A6, 0x45C7).is_err()); // CMP high-register.
    }

    #[test]
    fn decodes_thumb_movs_immediate_and_rejects_other_format_three_ops() {
        assert_eq!(
            decode_thumb(0x0800_03AA, 0x2000).unwrap().operation,
            ThumbOperation::MovImmediate { rd: 0, value: 0 }
        );
        assert_eq!(
            decode_thumb(0x0800_03AA, 0x27FF).unwrap().operation,
            ThumbOperation::MovImmediate { rd: 7, value: 0xFF }
        );
        for encoding in [0x28FF, 0x30FF, 0x38FF] {
            assert!(decode_thumb(0x0800_03AA, encoding).is_err());
        }
    }

    #[test]
    fn decodes_only_preindexed_immediate_word_stores_without_writeback() {
        assert_eq!(
            decode_arm(0x0800_0224, 0xE581_0000).unwrap().operation,
            ArmOperation::StoreWord {
                rd: 0,
                rn: 1,
                offset: 0,
                up: true,
            }
        );
        assert_eq!(
            decode_arm(0x0800_0224, 0xE501_0004).unwrap().operation,
            ArmOperation::StoreWord {
                rd: 0,
                rn: 1,
                offset: 4,
                up: false,
            }
        );
        assert!(decode_arm(0x0800_0224, 0xE781_0000).is_err());
        assert!(decode_arm(0x0800_0224, 0xE481_0000).is_err());
        assert!(decode_arm(0x0800_0224, 0xE5A1_0000).is_err());
        assert!(decode_arm(0x0800_0224, 0xE4A1_0000).is_err());
        assert!(decode_arm(0x0800_0224, 0xE5C1_0000).is_err());
        assert!(decode_arm(0x0800_0224, 0xE581_F000).is_err());
    }
}

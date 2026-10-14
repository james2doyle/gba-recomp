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
    if encoding & 0xE000 == 0x2000 {
        let opcode = (encoding >> 11) & 0x3;
        let rd = ((encoding >> 8) & 0x7) as u8;
        let immediate = encoding as u8;
        let operation = match opcode {
            0 => ThumbOperation::MovImmediate {
                rd,
                value: immediate,
            },
            1 => ThumbOperation::CompareImmediate { rd, immediate },
            2 => ThumbOperation::AddSubtractImmediate {
                rd,
                immediate,
                subtract: false,
            },
            _ => ThumbOperation::AddSubtractImmediate {
                rd,
                immediate,
                subtract: true,
            },
        };
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation,
        });
    }
    if encoding & 0xF800 == 0x1800 {
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::AddSubtract {
                rd: (encoding & 0x7) as u8,
                rn: ((encoding >> 3) & 0x7) as u8,
                operand: ((encoding >> 6) & 0x7) as u8,
                immediate: encoding & (1 << 10) != 0,
                subtract: encoding & (1 << 9) != 0,
            },
        });
    }
    if encoding & 0xFFC0 == 0x4300 {
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::OrrRegister {
                rd: (encoding & 0x7) as u8,
                rs: ((encoding >> 3) & 0x7) as u8,
            },
        });
    }
    if encoding & 0xF800 == 0x7000 {
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::StoreByteImmediate {
                rd: (encoding & 0x7) as u8,
                rb: ((encoding >> 3) & 0x7) as u8,
                imm5: ((encoding >> 6) & 0x1F) as u8,
            },
        });
    }
    if encoding & 0xF800 == 0x7800 {
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::LoadByteImmediate {
                rd: (encoding & 0x7) as u8,
                rb: ((encoding >> 3) & 0x7) as u8,
                imm5: ((encoding >> 6) & 0x1F) as u8,
            },
        });
    }
    if encoding & 0xF800 == 0x8000 {
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::StoreHalfwordImmediate {
                rd: (encoding & 0x7) as u8,
                rb: ((encoding >> 3) & 0x7) as u8,
                imm5: ((encoding >> 6) & 0x1F) as u8,
            },
        });
    }
    if encoding & 0xF800 == 0 {
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::LslImmediate {
                rd: (encoding & 0x7) as u8,
                rs: ((encoding >> 3) & 0x7) as u8,
                amount: ((encoding >> 6) & 0x1F) as u8,
            },
        });
    }
    if encoding & 0xF800 == 0x4800 {
        let pipeline_pc = guest_pc
            .checked_add(4)
            .ok_or_else(|| format!("Thumb literal PC overflow at 0x{guest_pc:08X}"))?;
        let aligned_pc = pipeline_pc & !3;
        let offset = u32::from(encoding & 0x00FF) * 4;
        let guest_address = aligned_pc
            .checked_add(offset)
            .ok_or_else(|| format!("Thumb literal address overflow at 0x{guest_pc:08X}"))?;
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::LoadPcRelativeWord {
                rd: ((encoding >> 8) & 0x7) as u8,
                guest_address,
            },
        });
    }
    if encoding & 0xF000 == 0xD000 {
        let condition = ((encoding >> 8) & 0xF) as u8;
        if condition <= 0xD {
            let signed_immediate = i32::from((encoding as u8) as i8);
            let displacement = signed_immediate.wrapping_shl(1) as u32;
            return Ok(ThumbInstruction {
                guest_pc,
                encoding,
                operation: ThumbOperation::ConditionalBranch {
                    condition,
                    target: guest_pc.wrapping_add(4).wrapping_add(displacement),
                },
            });
        }
    }
    if encoding & 0xFF00 == 0xDF00 {
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::Swi {
                immediate: encoding as u8,
            },
        });
    }
    if encoding & 0xFF00 == 0xDE00 {
        return Err(format!(
            "undefined_thumb_instruction at 0x{guest_pc:08X}: 0x{encoding:04X}"
        ));
    }
    if encoding & 0xF800 == 0xF000 {
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::BlPrefix {
                lr_partial: thumb_bl_lr_partial(guest_pc, encoding),
            },
        });
    }
    if encoding & 0xF800 == 0xF800 {
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::BlSuffix {
                immediate: encoding & 0x07FF,
            },
        });
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
    if encoding & 0xFE00 == 0xBC00 {
        if encoding & 0x0100 != 0 {
            return Err(format!(
                "unsupported_thumb_instruction at 0x{guest_pc:08X}: 0x{encoding:04X} (POP with PC is not implemented)"
            ));
        }
        let register_list = encoding & 0x00FF;
        if register_list == 0 {
            return Err(format!(
                "unsupported_thumb_instruction at 0x{guest_pc:08X}: 0x{encoding:04X} (empty POP register list)"
            ));
        }
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::Pop { register_list },
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
    if encoding & 0xF800 == 0xC800 {
        let register_list = encoding & 0x00FF;
        if register_list == 0 {
            return Err(format!(
                "unsupported_thumb_instruction at 0x{guest_pc:08X}: 0x{encoding:04X} (empty LDMIA register list is unpredictable)"
            ));
        }
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::LoadMultipleIncrementAfter {
                base_register: ((encoding >> 8) & 0x7) as u8,
                register_list,
            },
        });
    }
    if encoding & 0xF800 == 0xC000 {
        let base_register = ((encoding >> 8) & 0x7) as u8;
        let register_list = encoding & 0x00FF;
        if register_list == 0 {
            return Err(format!(
                "unsupported_thumb_instruction at 0x{guest_pc:08X}: 0x{encoding:04X} (empty STMIA register list is unpredictable)"
            ));
        }
        let base_in_list = register_list & (1 << base_register) != 0;
        let lower_registers = if base_register == 0 {
            0
        } else {
            register_list & ((1 << base_register) - 1)
        };
        if base_in_list && lower_registers != 0 {
            return Err(format!(
                "unsupported_thumb_instruction at 0x{guest_pc:08X}: 0x{encoding:04X} (STMIA base register is included after a lower register; result is unpredictable)"
            ));
        }
        return Ok(ThumbInstruction {
            guest_pc,
            encoding,
            operation: ThumbOperation::StoreMultipleIncrementAfter {
                base_register,
                register_list,
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

fn thumb_bl_lr_partial(guest_pc: u32, prefix_encoding: u16) -> u32 {
    let immediate = i32::from(prefix_encoding & 0x07FF);
    let signed = (immediate << 21) >> 21;
    guest_pc.wrapping_add(4).wrapping_add((signed << 12) as u32)
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
    fn decodes_thumb_conditional_branch_formats_conditions_offsets_and_wrapping_pc() {
        let guest_pc = 0x0800_0100;
        for condition in 0..=0xD_u16 {
            for immediate in [0x00_u16, 0x7F, 0x80, 0xFF] {
                let encoding = 0xD000 | (condition << 8) | immediate;
                let instruction = decode_thumb(guest_pc, encoding).unwrap();
                let displacement = i32::from(immediate as u8 as i8) * 2;
                assert_eq!(instruction.guest_pc, guest_pc);
                assert_eq!(instruction.encoding, encoding);
                assert_eq!(
                    instruction.operation,
                    ThumbOperation::ConditionalBranch {
                        condition: condition as u8,
                        target: guest_pc.wrapping_add(4).wrapping_add(displacement as u32),
                    },
                    "condition 0x{condition:X}, immediate 0x{immediate:02X}"
                );
            }
        }

        assert_eq!(
            decode_thumb(0x0800_09A6, 0xDDF6).unwrap().operation,
            ThumbOperation::ConditionalBranch {
                condition: 0xD,
                target: 0x0800_0996,
            }
        );
        assert_eq!(
            decode_thumb(0xFFFF_FFFE, 0xD001).unwrap().operation,
            ThumbOperation::ConditionalBranch {
                condition: 0,
                target: 4,
            }
        );
        assert_eq!(
            decode_thumb(0, 0xD080).unwrap().operation,
            ThumbOperation::ConditionalBranch {
                condition: 0,
                target: 0xFFFF_FF04,
            }
        );
    }

    #[test]
    fn preserves_thumb_conditional_branch_undefined_and_swi_classifications() {
        assert!(
            decode_thumb(0x0800_0084, 0xDE01)
                .unwrap_err()
                .contains("undefined_thumb_instruction at 0x08000084")
        );
        assert_eq!(
            decode_thumb(0x0800_0084, 0xDFFF).unwrap().operation,
            ThumbOperation::Swi { immediate: 0xFF }
        );
    }

    #[test]
    fn decodes_thumb_lsl_immediate_and_rejects_other_shift_formats() {
        assert_eq!(
            decode_thumb(0x0800_03B2, 0x04C9).unwrap().operation,
            ThumbOperation::LslImmediate {
                rd: 1,
                rs: 1,
                amount: 19,
            }
        );
        assert_eq!(
            decode_thumb(0x0800_0084, 0x0000).unwrap().operation,
            ThumbOperation::LslImmediate {
                rd: 0,
                rs: 0,
                amount: 0,
            }
        );
        for encoding in [0x0849, 0x1049, 0x4089] {
            assert!(decode_thumb(0x0800_0084, encoding).is_err());
        }
    }

    #[test]
    fn decodes_only_thumb_orr_register_format_4() {
        assert_eq!(
            decode_thumb(0x0800_099E, 0x4318).unwrap().operation,
            ThumbOperation::OrrRegister { rd: 0, rs: 3 }
        );
        assert_eq!(
            decode_thumb(0x0800_0084, 0x433F).unwrap().operation,
            ThumbOperation::OrrRegister { rd: 7, rs: 7 }
        );

        for rs in 0..8_u16 {
            for rd in 0..8_u16 {
                let encoding = 0x4300 | (rs << 3) | rd;
                assert_eq!(
                    decode_thumb(0x0800_0084, encoding).unwrap().operation,
                    ThumbOperation::OrrRegister {
                        rd: rd as u8,
                        rs: rs as u8,
                    }
                );
            }
        }

        for encoding in (0x4000..0x4400_u16).step_by(0x40) {
            if encoding != 0x4300 {
                assert!(decode_thumb(0x0800_0084, encoding).is_err());
            }
        }
        assert!(decode_thumb(0x0800_0085, 0x4318).is_err());
    }

    #[test]
    fn decodes_all_thumb_add_sub_format_2_variants() {
        let cases = [
            (
                0x1808,
                ThumbOperation::AddSubtract {
                    rd: 0,
                    rn: 1,
                    operand: 0,
                    immediate: false,
                    subtract: false,
                },
            ),
            (
                0x1A08,
                ThumbOperation::AddSubtract {
                    rd: 0,
                    rn: 1,
                    operand: 0,
                    immediate: false,
                    subtract: true,
                },
            ),
            (
                0x1C10,
                ThumbOperation::AddSubtract {
                    rd: 0,
                    rn: 2,
                    operand: 0,
                    immediate: true,
                    subtract: false,
                },
            ),
            (
                0x1FC8,
                ThumbOperation::AddSubtract {
                    rd: 0,
                    rn: 1,
                    operand: 7,
                    immediate: true,
                    subtract: true,
                },
            ),
        ];
        for (encoding, expected) in cases {
            assert_eq!(
                decode_thumb(0x0800_0084, encoding).unwrap().operation,
                expected
            );
        }
        assert!(decode_thumb(0x0800_0084, 0x4008).is_err());
    }

    #[test]
    fn decodes_thumb_strh_immediate_format_10_and_rejects_other_halfword_forms() {
        assert_eq!(
            decode_thumb(0x0800_03B8, 0x8008).unwrap().operation,
            ThumbOperation::StoreHalfwordImmediate {
                rd: 0,
                rb: 1,
                imm5: 0,
            }
        );
        assert_eq!(
            decode_thumb(0x0800_03B8, 0x87C8).unwrap().operation,
            ThumbOperation::StoreHalfwordImmediate {
                rd: 0,
                rb: 1,
                imm5: 31,
            }
        );
        for encoding in [0x8808, 0x5A08, 0x5208] {
            assert!(
                decode_thumb(0x0800_03B8, encoding).is_err(),
                "encoding 0x{encoding:04X} must remain unsupported"
            );
        }
    }

    #[test]
    fn decodes_thumb_strb_immediate_format_9_and_rejects_other_byte_forms() {
        assert_eq!(
            decode_thumb(0x0800_0998, 0x7005).unwrap().operation,
            ThumbOperation::StoreByteImmediate {
                rd: 5,
                rb: 0,
                imm5: 0,
            }
        );
        assert_eq!(
            decode_thumb(0x0800_0998, 0x77C5).unwrap().operation,
            ThumbOperation::StoreByteImmediate {
                rd: 5,
                rb: 0,
                imm5: 31,
            }
        );
        assert_eq!(
            decode_thumb(0x0800_0998, 0x77FD).unwrap().operation,
            ThumbOperation::StoreByteImmediate {
                rd: 5,
                rb: 7,
                imm5: 31,
            }
        );
        for encoding in [0x5408, 0x5C08] {
            assert!(
                decode_thumb(0x0800_0998, encoding).is_err(),
                "encoding 0x{encoding:04X} must remain unsupported"
            );
        }
    }

    #[test]
    fn decodes_thumb_ldrb_immediate_format_9_and_rejects_other_byte_forms() {
        assert_eq!(
            decode_thumb(0x0800_099C, 0x7808).unwrap().operation,
            ThumbOperation::LoadByteImmediate {
                rd: 0,
                rb: 1,
                imm5: 0,
            }
        );
        assert_eq!(
            decode_thumb(0x0800_099C, 0x7FC5).unwrap().operation,
            ThumbOperation::LoadByteImmediate {
                rd: 5,
                rb: 0,
                imm5: 31,
            }
        );
        assert_eq!(
            decode_thumb(0x0800_099C, 0x7FFF).unwrap().operation,
            ThumbOperation::LoadByteImmediate {
                rd: 7,
                rb: 7,
                imm5: 31,
            }
        );
        for encoding in [0x5C08, 0x5808, 0x8808] {
            assert!(
                decode_thumb(0x0800_099C, encoding).is_err(),
                "encoding 0x{encoding:04X} must remain unsupported"
            );
        }
    }

    #[test]
    fn decodes_thumb_ldr_literal_with_aligned_pipeline_pc_and_checked_address() {
        assert_eq!(
            decode_thumb(0x0800_03B4, 0x4A2C).unwrap().operation,
            ThumbOperation::LoadPcRelativeWord {
                rd: 2,
                guest_address: 0x0800_0468,
            }
        );
        assert_eq!(
            decode_thumb(0x0800_0086, 0x4901).unwrap().operation,
            ThumbOperation::LoadPcRelativeWord {
                rd: 1,
                guest_address: 0x0800_008C,
            }
        );
        assert_eq!(
            decode_thumb(0x0800_0086, 0x4800).unwrap().operation,
            ThumbOperation::LoadPcRelativeWord {
                rd: 0,
                guest_address: 0x0800_0088,
            }
        );
        assert!(
            decode_thumb(0xFFFF_FFFE, 0x4800)
                .unwrap_err()
                .contains("Thumb literal PC overflow at 0xFFFFFFFE")
        );
        assert!(decode_thumb(0x0800_0086, 0x6800).is_err());
    }

    #[test]
    fn decodes_thumb_swi_immediate_and_keeps_the_undefined_range_undefined() {
        for immediate in [0x00_u8, 0x01, 0xFF] {
            assert_eq!(
                decode_thumb(0x0800_0084, 0xDF00 | u16::from(immediate))
                    .unwrap()
                    .operation,
                ThumbOperation::Swi { immediate }
            );
        }
        assert!(
            decode_thumb(0x0800_0084, 0xDE01)
                .unwrap_err()
                .contains("undefined_thumb_instruction at 0x08000084")
        );
    }

    #[test]
    fn decodes_thumb_bl_halves_and_sign_extends_the_prefix_offset() {
        assert_eq!(
            decode_thumb(0x0800_0084, 0xF000).unwrap().operation,
            ThumbOperation::BlPrefix {
                lr_partial: 0x0800_0088,
            }
        );
        assert_eq!(
            decode_thumb(0x0800_0086, 0xF804).unwrap().operation,
            ThumbOperation::BlSuffix { immediate: 4 }
        );
        assert_eq!(
            decode_thumb(0x0800_0084, 0xF7FF).unwrap().operation,
            ThumbOperation::BlPrefix {
                lr_partial: 0x07FF_F088,
            }
        );
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
    fn decodes_thumb_pop_low_register_list_and_rejects_pc_or_empty_list() {
        let instruction = decode_thumb(0x0800_03A6, 0xBC08).unwrap();
        assert_eq!(instruction.guest_pc, 0x0800_03A6);
        assert_eq!(instruction.encoding, 0xBC08);
        assert_eq!(
            instruction.operation,
            ThumbOperation::Pop {
                register_list: 0x08
            }
        );
        assert_eq!(
            decode_thumb(0x0800_03A6, 0xBC01).unwrap().operation,
            ThumbOperation::Pop { register_list: 1 }
        );
        assert_eq!(
            decode_thumb(0x0800_03A6, 0xBCFF).unwrap().operation,
            ThumbOperation::Pop {
                register_list: 0xFF
            }
        );
        assert!(
            decode_thumb(0x0800_03A6, 0xBD08)
                .unwrap_err()
                .contains("POP with PC")
        );
        assert!(
            decode_thumb(0x0800_03A6, 0xBC00)
                .unwrap_err()
                .contains("empty POP register list")
        );
        assert!(decode_thumb(0x0800_03A6, 0xBE08).is_err());
        assert!(decode_thumb(0x0800_03A7, 0xBC08).is_err());
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
    fn decodes_all_thumb_format3_immediate_operations() {
        assert_eq!(
            decode_thumb(0x0800_09A2, 0x3201).unwrap().operation,
            ThumbOperation::AddSubtractImmediate {
                rd: 2,
                immediate: 1,
                subtract: false,
            }
        );

        for (opcode, operation) in [(0, 0), (1, 1), (2, 2), (3, 3)] {
            for rd in 0..8_u16 {
                for immediate in [0, 0xFF] {
                    let encoding = 0x2000 | (opcode << 11) | (rd << 8) | immediate;
                    let expected = match operation {
                        0 => ThumbOperation::MovImmediate {
                            rd: rd as u8,
                            value: immediate as u8,
                        },
                        1 => ThumbOperation::CompareImmediate {
                            rd: rd as u8,
                            immediate: immediate as u8,
                        },
                        2 => ThumbOperation::AddSubtractImmediate {
                            rd: rd as u8,
                            immediate: immediate as u8,
                            subtract: false,
                        },
                        _ => ThumbOperation::AddSubtractImmediate {
                            rd: rd as u8,
                            immediate: immediate as u8,
                            subtract: true,
                        },
                    };
                    assert_eq!(
                        decode_thumb(0x0800_03AA, encoding).unwrap().operation,
                        expected
                    );
                }
            }
        }
        assert!(decode_thumb(0x0800_03AA, 0x4008).is_err());
        assert!(decode_thumb(0x0800_03AB, 0x3201).is_err());
    }

    #[test]
    fn decodes_thumb_ldmia_and_stmia_and_rejects_unpredictable_forms() {
        let instruction = decode_thumb(0x0800_03A8, 0xCB01).unwrap();
        assert_eq!(instruction.guest_pc, 0x0800_03A8);
        assert_eq!(instruction.encoding, 0xCB01);
        assert_eq!(
            instruction.operation,
            ThumbOperation::LoadMultipleIncrementAfter {
                base_register: 3,
                register_list: 1
            }
        );
        assert!(
            decode_thumb(0x0800_03A8, 0xC800)
                .unwrap_err()
                .contains("empty LDMIA register list is unpredictable")
        );
        assert_eq!(
            decode_thumb(0x0800_03A8, 0xC201).unwrap().operation,
            ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 1,
            }
        );
        assert_eq!(
            decode_thumb(0x0800_03A8, 0xC001).unwrap().operation,
            ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 0,
                register_list: 1,
            }
        );
        assert!(
            decode_thumb(0x0800_03A8, 0xC000)
                .unwrap_err()
                .contains("empty STMIA register list is unpredictable")
        );
        assert!(
            decode_thumb(0x0800_03A8, 0xC205)
                .unwrap_err()
                .contains("STMIA base register is included after a lower register")
        );
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

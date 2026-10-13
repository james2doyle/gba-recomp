#![forbid(unsafe_code)]

use std::fmt;

pub const GBA_IR_VERSION: u16 = 1;
pub const REG_PC: u8 = 15;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IsaMode {
    Arm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddRegister {
    pub rd: u8,
    pub rn: u8,
    pub rm: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GbaIrBlock {
    pub version: u16,
    pub guest_pc: u32,
    pub isa: IsaMode,
    pub instructions: Vec<AddRegister>,
}

/// The ARM subset used by the first complete ROM-to-native-code path.
/// Guest instructions retain their original address and encoding for reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArmOperation {
    MovImmediate {
        rd: u8,
        value: u32,
    },
    MovRegister {
        rd: u8,
        rm: u8,
    },
    AddImmediate {
        rd: u8,
        rn: u8,
        value: u32,
    },
    CompareImmediate {
        rn: u8,
        value: u32,
    },
    AddPcRelativeImmediate {
        rd: u8,
        value: u32,
    },
    MsrCpsrRegister {
        rm: u8,
        field_mask: u8,
    },
    /// Decoder result awaiting resolution against the immutable source ROM.
    LoadPcRelativeWord {
        rd: u8,
        guest_address: u32,
    },
    /// A word read from the source ROM and fixed into the AOT program.
    LoadRomLiteral {
        rd: u8,
        guest_address: u32,
        value: u32,
    },
    StoreWord {
        rd: u8,
        rn: u8,
        offset: u16,
        up: bool,
    },
    Branch {
        target: u32,
        link: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArmInstruction {
    pub guest_pc: u32,
    pub encoding: u32,
    /// ARM condition field. Values 0..=14 are defined for this subset.
    pub condition: u8,
    pub operation: ArmOperation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArmProgram {
    pub version: u16,
    pub instructions: Vec<ArmInstruction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuState {
    pub r: [u32; 16],
    pub cpsr: u32,
    pub abi_version: u32,
    pub guest_cycle: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepFault {
    AbiMismatch,
    CycleOverflow,
    ProgramCounterMismatch { expected: u32, actual: u32 },
    BankedRuntimeRequired,
    RomLiteralUnresolved,
    MemoryRuntimeRequired,
}

impl fmt::Display for StepFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AbiMismatch => f.write_str("runtime ABI version mismatch"),
            Self::CycleOverflow => f.write_str("guest cycle counter overflow"),
            Self::ProgramCounterMismatch { expected, actual } => write!(
                f,
                "instruction at 0x{expected:08X} was dispatched with PC 0x{actual:08X}"
            ),
            Self::BankedRuntimeRequired => {
                f.write_str("MSR CPSR requires the banked runtime ABI v2")
            }
            Self::RomLiteralUnresolved => {
                f.write_str("PC-relative ROM literal was not resolved during discovery")
            }
            Self::MemoryRuntimeRequired => {
                f.write_str("guest memory access requires the GBA runtime ABI v3")
            }
        }
    }
}

impl std::error::Error for StepFault {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerifyError {
    UnsupportedVersion(u16),
    MisalignedArmPc(u32),
    EmptyBlock,
    SliceOnlyOneInstruction(usize),
    ProgramCounterRegisterUnsupported,
    InvalidRegister(u8),
    InvalidCondition(u8),
    MisalignedBranchTarget(u32),
    MisalignedRomLiteral(u32),
    UnresolvedRomLiteral(u32),
    DuplicateInstructionPc(u32),
    UnsupportedInstructionEncoding(u32),
    MissingSuccessor { pc: u32, target: u32 },
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported GBA IR version {version}")
            }
            Self::MisalignedArmPc(pc) => write!(f, "ARM guest PC 0x{pc:08X} is not 4-byte aligned"),
            Self::EmptyBlock => f.write_str("GBA IR block contains no instructions"),
            Self::SliceOnlyOneInstruction(count) => {
                write!(f, "GBA-010 ALU slice accepts one instruction, got {count}")
            }
            Self::ProgramCounterRegisterUnsupported => {
                f.write_str("GBA-010 ALU slice does not implement ADD writes to r15")
            }
            Self::InvalidRegister(register) => {
                write!(
                    f,
                    "register r{register} is outside the implemented ALU subset"
                )
            }
            Self::InvalidCondition(condition) => {
                write!(f, "ARM condition field {condition} is undefined")
            }
            Self::MisalignedBranchTarget(target) => {
                write!(f, "ARM branch target 0x{target:08X} is not word aligned")
            }
            Self::MisalignedRomLiteral(address) => {
                write!(
                    f,
                    "ARM ROM literal address 0x{address:08X} is not word aligned"
                )
            }
            Self::UnresolvedRomLiteral(pc) => write!(
                f,
                "PC-relative ROM literal at 0x{pc:08X} was not resolved during discovery"
            ),
            Self::DuplicateInstructionPc(pc) => {
                write!(f, "multiple guest instructions have PC 0x{pc:08X}")
            }
            Self::UnsupportedInstructionEncoding(encoding) => {
                write!(f, "unsupported ARM encoding 0x{encoding:08X}")
            }
            Self::MissingSuccessor { pc, target } => write!(
                f,
                "instruction at 0x{pc:08X} has no discovered successor at 0x{target:08X}"
            ),
        }
    }
}

impl ArmProgram {
    pub fn verify(&self) -> Result<(), VerifyError> {
        if self.version != GBA_IR_VERSION {
            return Err(VerifyError::UnsupportedVersion(self.version));
        }
        if self.instructions.is_empty() {
            return Err(VerifyError::EmptyBlock);
        }
        let mut pcs = std::collections::HashSet::with_capacity(self.instructions.len());
        for instruction in &self.instructions {
            if instruction.guest_pc & 3 != 0 {
                return Err(VerifyError::MisalignedArmPc(instruction.guest_pc));
            }
            if !pcs.insert(instruction.guest_pc) {
                return Err(VerifyError::DuplicateInstructionPc(instruction.guest_pc));
            }
            if instruction.condition > 14 {
                return Err(VerifyError::InvalidCondition(instruction.condition));
            }
            match instruction.operation {
                ArmOperation::MovImmediate { rd, .. } => verify_general_register(rd)?,
                ArmOperation::MovRegister { rd, rm } => {
                    verify_general_register(rd)?;
                    verify_operand_register(rm)?;
                }
                ArmOperation::AddImmediate { rd, rn, .. } => {
                    verify_general_register(rd)?;
                    verify_general_register(rn)?;
                }
                ArmOperation::CompareImmediate { rn, .. } => verify_general_register(rn)?,
                ArmOperation::AddPcRelativeImmediate { rd, .. } => {
                    verify_general_register(rd)?;
                }
                ArmOperation::MsrCpsrRegister { rm, field_mask } => {
                    verify_general_register(rm)?;
                    if field_mask & !0x9 != 0 {
                        return Err(VerifyError::UnsupportedInstructionEncoding(
                            instruction.encoding,
                        ));
                    }
                }
                ArmOperation::LoadPcRelativeWord { .. } => {
                    return Err(VerifyError::UnresolvedRomLiteral(instruction.guest_pc));
                }
                ArmOperation::LoadRomLiteral {
                    rd, guest_address, ..
                } => {
                    verify_general_register(rd)?;
                    if guest_address & 3 != 0 {
                        return Err(VerifyError::MisalignedRomLiteral(guest_address));
                    }
                }
                ArmOperation::StoreWord { rd, rn, .. } => {
                    verify_general_register(rd)?;
                    verify_general_register(rn)?;
                }
                ArmOperation::Branch { target, .. } => {
                    if target & 3 != 0 {
                        return Err(VerifyError::MisalignedBranchTarget(target));
                    }
                }
            }
        }
        for instruction in &self.instructions {
            let fallthrough = instruction.guest_pc.wrapping_add(4);
            match instruction.operation {
                ArmOperation::Branch { target, link } => {
                    if !pcs.contains(&target) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target,
                        });
                    }
                    if (instruction.condition != 14 || link) && !pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                _ if !pcs.contains(&fallthrough) => {
                    return Err(VerifyError::MissingSuccessor {
                        pc: instruction.guest_pc,
                        target: fallthrough,
                    });
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn verify_general_register(register: u8) -> Result<(), VerifyError> {
    if register >= REG_PC {
        return Err(VerifyError::InvalidRegister(register));
    }
    Ok(())
}

fn verify_operand_register(register: u8) -> Result<(), VerifyError> {
    if register > REG_PC {
        return Err(VerifyError::InvalidRegister(register));
    }
    Ok(())
}

pub fn condition_passes(condition: u8, cpsr: u32) -> bool {
    let n = cpsr & (1 << 31) != 0;
    let z = cpsr & (1 << 30) != 0;
    let c = cpsr & (1 << 29) != 0;
    let v = cpsr & (1 << 28) != 0;
    match condition {
        0 => z,
        1 => !z,
        2 => c,
        3 => !c,
        4 => n,
        5 => !n,
        6 => v,
        7 => !v,
        8 => c && !z,
        9 => !c || z,
        10 => n == v,
        11 => n != v,
        12 => !z && n == v,
        13 => z || n != v,
        14 => true,
        _ => false,
    }
}

pub fn execute_instruction(
    state: &mut CpuState,
    instruction: ArmInstruction,
) -> Result<(), StepFault> {
    if state.abi_version != 1 {
        return Err(StepFault::AbiMismatch);
    }
    if state.guest_cycle == u64::MAX {
        return Err(StepFault::CycleOverflow);
    }
    if state.r[REG_PC as usize] != instruction.guest_pc {
        return Err(StepFault::ProgramCounterMismatch {
            expected: instruction.guest_pc,
            actual: state.r[REG_PC as usize],
        });
    }

    let executes = condition_passes(instruction.condition, state.cpsr);
    let pc_after = instruction.guest_pc.wrapping_add(4);
    if executes {
        match instruction.operation {
            ArmOperation::MovImmediate { rd, value } => state.r[rd as usize] = value,
            ArmOperation::MovRegister { rd, rm } => {
                let value = if rm == REG_PC {
                    instruction.guest_pc.wrapping_add(8)
                } else {
                    state.r[rm as usize]
                };
                state.r[rd as usize] = value;
            }
            ArmOperation::AddImmediate { rd, rn, value } => {
                state.r[rd as usize] = state.r[rn as usize].wrapping_add(value);
            }
            ArmOperation::CompareImmediate { rn, value } => {
                let lhs = state.r[rn as usize];
                let result = lhs.wrapping_sub(value);
                let carry = lhs >= value;
                let overflow = ((lhs ^ value) & (lhs ^ result) & 0x8000_0000) != 0;
                state.cpsr = (state.cpsr & 0x0fff_ffff)
                    | (result & 0x8000_0000)
                    | (u32::from(result == 0) << 30)
                    | (u32::from(carry) << 29)
                    | (u32::from(overflow) << 28);
            }
            ArmOperation::AddPcRelativeImmediate { rd, value } => {
                let visible_pc = instruction.guest_pc.wrapping_add(8);
                state.r[rd as usize] = visible_pc.wrapping_add(value);
            }
            ArmOperation::MsrCpsrRegister { .. } => {
                return Err(StepFault::BankedRuntimeRequired);
            }
            ArmOperation::LoadPcRelativeWord { .. } => {
                return Err(StepFault::RomLiteralUnresolved);
            }
            ArmOperation::LoadRomLiteral { rd, value, .. } => {
                state.r[rd as usize] = value;
            }
            ArmOperation::StoreWord { .. } => {
                return Err(StepFault::MemoryRuntimeRequired);
            }
            ArmOperation::Branch { target, link } => {
                if link {
                    state.r[14] = pc_after;
                }
                state.r[REG_PC as usize] = target;
                state.guest_cycle += 1;
                return Ok(());
            }
        }
    }
    state.r[REG_PC as usize] = pc_after;
    state.guest_cycle += 1;
    Ok(())
}

impl std::error::Error for VerifyError {}

impl GbaIrBlock {
    pub fn verify(&self) -> Result<(), VerifyError> {
        if self.version != GBA_IR_VERSION {
            return Err(VerifyError::UnsupportedVersion(self.version));
        }
        if self.isa == IsaMode::Arm && self.guest_pc & 3 != 0 {
            return Err(VerifyError::MisalignedArmPc(self.guest_pc));
        }
        if self.instructions.is_empty() {
            return Err(VerifyError::EmptyBlock);
        }
        if self.instructions.len() != 1 {
            return Err(VerifyError::SliceOnlyOneInstruction(
                self.instructions.len(),
            ));
        }
        let add = self.instructions[0];
        if add.rd == REG_PC {
            return Err(VerifyError::ProgramCounterRegisterUnsupported);
        }
        for register in [add.rd, add.rn, add.rm] {
            if register >= REG_PC {
                return Err(VerifyError::InvalidRegister(register));
            }
        }
        Ok(())
    }
}

pub fn alu_fixture() -> GbaIrBlock {
    GbaIrBlock {
        version: GBA_IR_VERSION,
        guest_pc: 0x0800_0000,
        isa: IsaMode::Arm,
        instructions: vec![AddRegister {
            rd: 0,
            rn: 1,
            rm: 2,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(pc: u32) -> CpuState {
        let mut cpu = CpuState {
            r: [0; 16],
            cpsr: 0xA000_001F,
            abi_version: 1,
            guest_cycle: 5,
        };
        cpu.r[15] = pc;
        cpu
    }

    #[test]
    fn cmp_sets_zero_and_no_borrow_carry_without_changing_control_bits() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0xE350_000A,
            condition: 14,
            operation: ArmOperation::CompareImmediate { rn: 0, value: 10 },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.r[0] = 10;
        execute_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.cpsr, 0x6000_001F);
        assert_eq!(cpu.r[15], instruction.guest_pc + 4);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn failed_predicate_advances_pc_without_writing_link_or_flags() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0x1B00_0000,
            condition: 1,
            operation: ArmOperation::Branch {
                target: 0x0800_0100,
                link: true,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 1 << 30;
        let before_flags = cpu.cpsr;
        execute_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[14], 0);
        assert_eq!(cpu.r[15], instruction.guest_pc + 4);
        assert_eq!(cpu.cpsr, before_flags);
    }

    #[test]
    fn resolved_rom_literal_load_commits_only_when_condition_passes() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0x059F_0004,
            condition: 0,
            operation: ArmOperation::LoadRomLiteral {
                rd: 0,
                guest_address: 0x0800_000C,
                value: 0x1234_5678,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.r[0] = 0xA5;
        execute_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[0], 0xA5);
        assert_eq!(cpu.r[15], instruction.guest_pc + 4);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn pc_relative_add_uses_pipeline_pc_without_changing_flags() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0xE28F_0020,
            condition: 14,
            operation: ArmOperation::AddPcRelativeImmediate { rd: 0, value: 0x20 },
        };
        let mut cpu = state(instruction.guest_pc);
        let flags = cpu.cpsr;
        execute_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[0], 0x0800_0028);
        assert_eq!(cpu.r[15], instruction.guest_pc + 4);
        assert_eq!(cpu.cpsr, flags);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn mov_register_reads_pipeline_pc_and_preserves_flags() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0xE1A0_E00F,
            condition: 14,
            operation: ArmOperation::MovRegister { rd: 14, rm: REG_PC },
        };
        let mut cpu = state(instruction.guest_pc);
        let flags = cpu.cpsr;
        execute_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[14], instruction.guest_pc + 8);
        assert_eq!(cpu.r[15], instruction.guest_pc + 4);
        assert_eq!(cpu.cpsr, flags);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn false_predicate_preserves_mov_register_destination_and_flags() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0x01A0_0001,
            condition: 0,
            operation: ArmOperation::MovRegister { rd: 0, rm: 1 },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.r[0] = 0xA5;
        cpu.r[1] = 0x1234_5678;
        let flags = cpu.cpsr;
        execute_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[0], 0xA5);
        assert_eq!(cpu.r[15], instruction.guest_pc + 4);
        assert_eq!(cpu.cpsr, flags);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn failed_pc_relative_add_predicate_preserves_destination_and_flags() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0x028F_0020,
            condition: 0,
            operation: ArmOperation::AddPcRelativeImmediate { rd: 0, value: 0x20 },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.r[0] = 0xA5;
        let flags = cpu.cpsr;
        execute_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[0], 0xA5);
        assert_eq!(cpu.r[15], instruction.guest_pc + 4);
        assert_eq!(cpu.cpsr, flags);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn false_predicate_skips_guest_store_without_memory_runtime() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0x0581_0000,
            condition: 0,
            operation: ArmOperation::StoreWord {
                rd: 0,
                rn: 1,
                offset: 0,
                up: true,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.r[0] = 0x1234_5678;
        cpu.r[1] = 0x0400_0000;
        let flags = cpu.cpsr;
        execute_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[0], 0x1234_5678);
        assert_eq!(cpu.r[1], 0x0400_0000);
        assert_eq!(cpu.r[15], instruction.guest_pc + 4);
        assert_eq!(cpu.cpsr, flags);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn executed_guest_store_requires_runtime_and_fails_atomically() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0xE581_0000,
            condition: 14,
            operation: ArmOperation::StoreWord {
                rd: 0,
                rn: 1,
                offset: 0,
                up: true,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.r[0] = 0x1234_5678;
        cpu.r[1] = 0x0200_0000;
        let before = cpu;
        assert_eq!(
            execute_instruction(&mut cpu, instruction),
            Err(StepFault::MemoryRuntimeRequired)
        );
        assert_eq!(cpu, before);
    }

    #[test]
    fn program_verifier_rejects_unresolved_pc_relative_literals() {
        let program = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![ArmInstruction {
                guest_pc: 0x0800_0000,
                encoding: 0xE59F_0004,
                condition: 14,
                operation: ArmOperation::LoadPcRelativeWord {
                    rd: 0,
                    guest_address: 0x0800_000C,
                },
            }],
        };
        assert_eq!(
            program.verify(),
            Err(VerifyError::UnresolvedRomLiteral(0x0800_0000))
        );
    }

    #[test]
    fn cycle_overflow_does_not_mutate_guest_state() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0xE280_0001,
            condition: 14,
            operation: ArmOperation::AddImmediate {
                rd: 0,
                rn: 0,
                value: 1,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.guest_cycle = u64::MAX;
        let before = cpu;
        assert_eq!(
            execute_instruction(&mut cpu, instruction),
            Err(StepFault::CycleOverflow)
        );
        assert_eq!(cpu, before);
    }

    #[test]
    fn program_verifier_rejects_missing_conditional_fallthrough() {
        let program = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![ArmInstruction {
                guest_pc: 0x0800_0000,
                encoding: 0x1A00_0000,
                condition: 1,
                operation: ArmOperation::Branch {
                    target: 0x0800_0000,
                    link: false,
                },
            }],
        };
        assert!(matches!(
            program.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: 0x0800_0000,
                target: 0x0800_0004
            })
        ));
    }
}

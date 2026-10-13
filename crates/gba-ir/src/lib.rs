#![forbid(unsafe_code)]

use std::fmt;

pub const GBA_IR_VERSION: u16 = 1;
pub const REG_PC: u8 = 15;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum IsaMode {
    Arm,
    Thumb,
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
    BranchExchange {
        rm: u8,
    },
    Branch {
        target: u32,
        link: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThumbOperation {
    BranchExchange {
        rm: u8,
    },
    Push {
        register_list: u16,
    },
    MovImmediate {
        rd: u8,
        value: u8,
    },
    MovRegister {
        rd: u8,
        rm: u8,
    },
    BlPrefix {
        lr_partial: u32,
    },
    /// Decoder-only form; discovery resolves it to a statically closed BL call.
    BlSuffix {
        immediate: u16,
    },
    BlCall {
        target: u32,
        expected_lr: u32,
        return_address: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThumbInstruction {
    pub guest_pc: u32,
    pub encoding: u16,
    pub operation: ThumbOperation,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct StaticBranchTarget {
    pub source_mode: IsaMode,
    pub source_pc: u32,
    pub target_mode: IsaMode,
    pub target_pc: u32,
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
    pub thumb_instructions: Vec<ThumbInstruction>,
    pub branch_targets: Vec<StaticBranchTarget>,
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
    IsaModeMismatch { expected: IsaMode, actual: IsaMode },
    BankedRuntimeRequired,
    RomLiteralUnresolved,
    MemoryRuntimeRequired,
    UnresolvedThumbBlSuffix,
    ThumbBlStateMismatch { expected: u32, actual: u32 },
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
            Self::IsaModeMismatch { expected, actual } => {
                write!(
                    f,
                    "instruction mode {expected:?} was dispatched in {actual:?} state"
                )
            }
            Self::BankedRuntimeRequired => {
                f.write_str("MSR CPSR requires the banked runtime ABI v2")
            }
            Self::RomLiteralUnresolved => {
                f.write_str("PC-relative ROM literal was not resolved during discovery")
            }
            Self::MemoryRuntimeRequired => {
                f.write_str("guest memory access requires the GBA runtime ABI v3")
            }
            Self::UnresolvedThumbBlSuffix => {
                f.write_str("Thumb BL suffix was not resolved during discovery")
            }
            Self::ThumbBlStateMismatch { expected, actual } => write!(
                f,
                "Thumb BL suffix expected LR 0x{expected:08X}, got 0x{actual:08X}"
            ),
        }
    }
}

impl std::error::Error for StepFault {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerifyError {
    UnsupportedVersion(u16),
    MisalignedArmPc(u32),
    MisalignedThumbPc(u32),
    InvalidThumbPushRegisterList(u16),
    InvalidThumbImmediateRegister(u8),
    EmptyBlock,
    SliceOnlyOneInstruction(usize),
    ProgramCounterRegisterUnsupported,
    InvalidRegister(u8),
    InvalidCondition(u8),
    MisalignedBranchTarget(u32),
    MisalignedRomLiteral(u32),
    UnresolvedRomLiteral(u32),
    DuplicateInstructionPc(u32),
    BranchExchangeWithoutTargets { mode: IsaMode, pc: u32 },
    BranchTargetSourceInvalid { mode: IsaMode, pc: u32 },
    UnresolvedThumbBlSuffix(u32),
    InvalidThumbBlPair(u32),
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
            Self::MisalignedThumbPc(pc) => {
                write!(f, "Thumb guest PC 0x{pc:08X} is not halfword aligned")
            }
            Self::InvalidThumbPushRegisterList(list) => {
                write!(f, "invalid Thumb PUSH register list 0x{list:04X}")
            }
            Self::InvalidThumbImmediateRegister(register) => write!(
                f,
                "Thumb immediate MOV destination r{register} is not a low register"
            ),
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
            Self::BranchExchangeWithoutTargets { mode, pc } => write!(
                f,
                "{mode:?} BX at 0x{pc:08X} has no statically discovered target"
            ),
            Self::BranchTargetSourceInvalid { mode, pc } => write!(
                f,
                "static branch target at 0x{pc:08X} does not originate at a {mode:?} BX"
            ),
            Self::UnresolvedThumbBlSuffix(pc) => write!(
                f,
                "Thumb BL suffix at 0x{pc:08X} was not resolved during discovery"
            ),
            Self::InvalidThumbBlPair(pc) => {
                write!(f, "invalid Thumb BL pair at 0x{pc:08X}")
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
        if self.instructions.is_empty() && self.thumb_instructions.is_empty() {
            return Err(VerifyError::EmptyBlock);
        }
        let mut arm_pcs = std::collections::HashSet::with_capacity(self.instructions.len());
        for instruction in &self.instructions {
            if instruction.guest_pc & 3 != 0 {
                return Err(VerifyError::MisalignedArmPc(instruction.guest_pc));
            }
            if !arm_pcs.insert(instruction.guest_pc) {
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
                ArmOperation::BranchExchange { rm } => verify_operand_register(rm)?,
                ArmOperation::Branch { target, .. } => {
                    if target & 3 != 0 {
                        return Err(VerifyError::MisalignedBranchTarget(target));
                    }
                }
            }
        }
        let mut thumb_pcs = std::collections::HashSet::with_capacity(self.thumb_instructions.len());
        let thumb_by_pc = self
            .thumb_instructions
            .iter()
            .map(|instruction| (instruction.guest_pc, instruction))
            .collect::<std::collections::HashMap<_, _>>();
        for instruction in &self.thumb_instructions {
            if instruction.guest_pc & 1 != 0 {
                return Err(VerifyError::MisalignedThumbPc(instruction.guest_pc));
            }
            if !thumb_pcs.insert(instruction.guest_pc) {
                return Err(VerifyError::DuplicateInstructionPc(instruction.guest_pc));
            }
            match instruction.operation {
                ThumbOperation::BranchExchange { rm } => verify_operand_register(rm)?,
                ThumbOperation::MovImmediate { rd, .. } if rd > 7 => {
                    return Err(VerifyError::InvalidThumbImmediateRegister(rd));
                }
                ThumbOperation::MovImmediate { .. } => {}
                ThumbOperation::MovRegister { rd, rm } => {
                    verify_general_register(rd)?;
                    verify_operand_register(rm)?;
                }
                ThumbOperation::Push { register_list }
                    if register_list == 0 || register_list & !0x01FF != 0 =>
                {
                    return Err(VerifyError::InvalidThumbPushRegisterList(register_list));
                }
                ThumbOperation::Push { .. } => {}
                ThumbOperation::BlPrefix { lr_partial }
                    if instruction.encoding & 0xF800 != 0xF000
                        || lr_partial
                            != thumb_bl_lr_partial(instruction.guest_pc, instruction.encoding) =>
                {
                    return Err(VerifyError::InvalidThumbBlPair(instruction.guest_pc));
                }
                ThumbOperation::BlPrefix { .. } => {}
                ThumbOperation::BlSuffix { .. } => {
                    return Err(VerifyError::UnresolvedThumbBlSuffix(instruction.guest_pc));
                }
                ThumbOperation::BlCall {
                    target,
                    expected_lr,
                    return_address,
                } => {
                    if instruction.encoding & 0xF800 != 0xF800
                        || target & 1 != 0
                        || return_address != (instruction.guest_pc.wrapping_add(2) | 1)
                    {
                        return Err(VerifyError::InvalidThumbBlPair(instruction.guest_pc));
                    }
                    let prefix_pc = instruction.guest_pc.wrapping_sub(2);
                    let Some(prefix) = thumb_by_pc.get(&prefix_pc) else {
                        return Err(VerifyError::InvalidThumbBlPair(instruction.guest_pc));
                    };
                    let ThumbOperation::BlPrefix { lr_partial } = prefix.operation else {
                        return Err(VerifyError::InvalidThumbBlPair(instruction.guest_pc));
                    };
                    let immediate = u32::from(instruction.encoding & 0x07FF) << 1;
                    if expected_lr != lr_partial || target != lr_partial.wrapping_add(immediate) {
                        return Err(VerifyError::InvalidThumbBlPair(instruction.guest_pc));
                    }
                }
            }
        }
        for target in &self.branch_targets {
            let source_exists = match target.source_mode {
                IsaMode::Arm => arm_pcs.contains(&target.source_pc),
                IsaMode::Thumb => thumb_pcs.contains(&target.source_pc),
            };
            let target_exists = match target.target_mode {
                IsaMode::Arm => arm_pcs.contains(&target.target_pc),
                IsaMode::Thumb => thumb_pcs.contains(&target.target_pc),
            };
            if !source_exists || !target_exists {
                return Err(VerifyError::MissingSuccessor {
                    pc: target.source_pc,
                    target: target.target_pc,
                });
            }
            let source_is_bx = match target.source_mode {
                IsaMode::Arm => self.instructions.iter().any(|instruction| {
                    instruction.guest_pc == target.source_pc
                        && matches!(instruction.operation, ArmOperation::BranchExchange { .. })
                }),
                IsaMode::Thumb => self.thumb_instructions.iter().any(|instruction| {
                    instruction.guest_pc == target.source_pc
                        && matches!(instruction.operation, ThumbOperation::BranchExchange { .. })
                }),
            };
            if !source_is_bx {
                return Err(VerifyError::BranchTargetSourceInvalid {
                    mode: target.source_mode,
                    pc: target.source_pc,
                });
            }
            match target.target_mode {
                IsaMode::Arm if target.target_pc & 3 != 0 => {
                    return Err(VerifyError::MisalignedBranchTarget(target.target_pc));
                }
                IsaMode::Thumb if target.target_pc & 1 != 0 => {
                    return Err(VerifyError::MisalignedThumbPc(target.target_pc));
                }
                _ => {}
            }
        }
        for instruction in &self.instructions {
            let fallthrough = instruction.guest_pc.wrapping_add(4);
            match instruction.operation {
                ArmOperation::Branch { target, link } => {
                    if !arm_pcs.contains(&target) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target,
                        });
                    }
                    if (instruction.condition != 14 || link) && !arm_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ArmOperation::BranchExchange { .. } => {
                    if !self.branch_targets.iter().any(|target| {
                        target.source_mode == IsaMode::Arm
                            && target.source_pc == instruction.guest_pc
                    }) {
                        return Err(VerifyError::BranchExchangeWithoutTargets {
                            mode: IsaMode::Arm,
                            pc: instruction.guest_pc,
                        });
                    }
                    if instruction.condition != 14 && !arm_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                _ if !arm_pcs.contains(&fallthrough) => {
                    return Err(VerifyError::MissingSuccessor {
                        pc: instruction.guest_pc,
                        target: fallthrough,
                    });
                }
                _ => {}
            }
        }
        for instruction in &self.thumb_instructions {
            match instruction.operation {
                ThumbOperation::BranchExchange { .. }
                    if !self.branch_targets.iter().any(|target| {
                        target.source_mode == IsaMode::Thumb
                            && target.source_pc == instruction.guest_pc
                    }) =>
                {
                    return Err(VerifyError::BranchExchangeWithoutTargets {
                        mode: IsaMode::Thumb,
                        pc: instruction.guest_pc,
                    });
                }
                ThumbOperation::BranchExchange { .. } => {}
                ThumbOperation::Push { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::MovImmediate { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::MovRegister { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::BlPrefix { .. } => {
                    let suffix = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&suffix) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: suffix,
                        });
                    }
                    let is_call_suffix = thumb_by_pc.get(&suffix).is_some_and(|candidate| {
                        matches!(candidate.operation, ThumbOperation::BlCall { .. })
                    });
                    if !is_call_suffix {
                        return Err(VerifyError::InvalidThumbBlPair(instruction.guest_pc));
                    }
                }
                ThumbOperation::BlCall { target, .. } => {
                    if !thumb_pcs.contains(&target) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target,
                        });
                    }
                }
                ThumbOperation::BlSuffix { .. } => {
                    return Err(VerifyError::UnresolvedThumbBlSuffix(instruction.guest_pc));
                }
            }
        }
        Ok(())
    }
}

fn thumb_bl_lr_partial(guest_pc: u32, prefix_encoding: u16) -> u32 {
    let immediate = i32::from(prefix_encoding & 0x07FF);
    let signed = (immediate << 21) >> 21;
    guest_pc.wrapping_add(4).wrapping_add((signed << 12) as u32)
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
    if state.cpsr & 0x20 != 0 {
        return Err(StepFault::IsaModeMismatch {
            expected: IsaMode::Arm,
            actual: IsaMode::Thumb,
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
            ArmOperation::BranchExchange { rm } => {
                let target = if rm == REG_PC {
                    instruction.guest_pc.wrapping_add(8)
                } else {
                    state.r[rm as usize]
                };
                if target & 1 != 0 {
                    state.cpsr |= 0x20;
                    state.r[REG_PC as usize] = target & !1;
                } else {
                    state.cpsr &= !0x20;
                    state.r[REG_PC as usize] = target & !3;
                }
                state.guest_cycle += 1;
                return Ok(());
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

pub fn execute_thumb_instruction(
    state: &mut CpuState,
    instruction: ThumbInstruction,
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
    if state.cpsr & 0x20 == 0 {
        return Err(StepFault::IsaModeMismatch {
            expected: IsaMode::Thumb,
            actual: IsaMode::Arm,
        });
    }
    match instruction.operation {
        ThumbOperation::BranchExchange { rm } => {
            let target = if rm == REG_PC {
                instruction.guest_pc.wrapping_add(4)
            } else {
                state.r[rm as usize]
            };
            if target & 1 != 0 {
                state.cpsr |= 0x20;
                state.r[REG_PC as usize] = target & !1;
            } else {
                state.cpsr &= !0x20;
                state.r[REG_PC as usize] = target & !3;
            }
        }
        ThumbOperation::Push { .. } => return Err(StepFault::MemoryRuntimeRequired),
        ThumbOperation::MovImmediate { rd, value } => {
            state.r[rd as usize] = u32::from(value);
            state.cpsr = (state.cpsr & !0xC000_0000) | (u32::from(value == 0) << 30);
            state.r[REG_PC as usize] = instruction.guest_pc.wrapping_add(2);
        }
        ThumbOperation::MovRegister { rd, rm } => {
            let value = if rm == REG_PC {
                instruction.guest_pc.wrapping_add(4)
            } else {
                state.r[rm as usize]
            };
            state.r[rd as usize] = value;
            state.r[REG_PC as usize] = instruction.guest_pc.wrapping_add(2);
        }
        ThumbOperation::BlPrefix { lr_partial } => {
            state.r[14] = lr_partial;
            state.r[REG_PC as usize] = instruction.guest_pc.wrapping_add(2);
        }
        ThumbOperation::BlSuffix { .. } => return Err(StepFault::UnresolvedThumbBlSuffix),
        ThumbOperation::BlCall {
            target,
            expected_lr,
            return_address,
        } => {
            let actual = state.r[14];
            if actual != expected_lr {
                return Err(StepFault::ThumbBlStateMismatch {
                    expected: expected_lr,
                    actual,
                });
            }
            state.r[14] = return_address;
            state.r[REG_PC as usize] = target;
        }
    }
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
    fn arm_bx_switches_to_thumb_and_masks_only_bit_zero() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0xE12F_FF10,
            condition: 14,
            operation: ArmOperation::BranchExchange { rm: 0 },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.r[0] = 0x0800_0101;
        let flags_without_t = cpu.cpsr & !0x20;
        execute_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[15], 0x0800_0100);
        assert_eq!(cpu.cpsr & 0x20, 0x20);
        assert_eq!(cpu.cpsr & !0x20, flags_without_t);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn arm_bx_to_arm_clears_t_and_word_aligns_target() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0xE12F_FF10,
            condition: 14,
            operation: ArmOperation::BranchExchange { rm: 0 },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.r[0] = 0x0800_0102;
        execute_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[15], 0x0800_0100);
        assert_eq!(cpu.cpsr & 0x20, 0);
    }

    #[test]
    fn false_arm_bx_preserves_mode_and_advances_pc() {
        let instruction = ArmInstruction {
            guest_pc: 0x0800_0000,
            encoding: 0x012F_FF10,
            condition: 0,
            operation: ArmOperation::BranchExchange { rm: 0 },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.r[0] = 0x0800_0101;
        let cpsr = cpu.cpsr;
        execute_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[15], instruction.guest_pc + 4);
        assert_eq!(cpu.cpsr, cpsr);
    }

    #[test]
    fn thumb_bx_switches_back_to_arm_and_word_aligns_target() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0x4708,
            operation: ThumbOperation::BranchExchange { rm: 1 },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[1] = 0x0800_0202;
        execute_thumb_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[15], 0x0800_0200);
        assert_eq!(cpu.cpsr & 0x20, 0);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn thumb_push_requires_memory_runtime_without_mutating_cpu_state() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0xB5F0,
            operation: ThumbOperation::Push {
                register_list: 0x01F0,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[13] = 0x0300_0020;
        let before = cpu;
        assert_eq!(
            execute_thumb_instruction(&mut cpu, instruction),
            Err(StepFault::MemoryRuntimeRequired)
        );
        assert_eq!(cpu, before);
    }

    #[test]
    fn thumb_high_mov_copies_registers_and_preserves_cpsr() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0x4647,
            operation: ThumbOperation::MovRegister { rd: 7, rm: 8 },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[8] = 0x1234_5678;
        let cpsr = cpu.cpsr;
        execute_thumb_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[7], 0x1234_5678);
        assert_eq!(cpu.r[15], instruction.guest_pc + 2);
        assert_eq!(cpu.cpsr, cpsr);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn thumb_high_mov_reads_pc_as_instruction_address_plus_four() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0x4678,
            operation: ThumbOperation::MovRegister { rd: 0, rm: REG_PC },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        execute_thumb_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[0], instruction.guest_pc + 4);
        assert_eq!(cpu.r[15], instruction.guest_pc + 2);
    }

    #[test]
    fn thumb_mov_immediate_updates_nz_and_preserves_cv_and_mode() {
        for (value, expected_cpsr) in [(0xFF, 0x3000_003F), (0, 0x7000_003F)] {
            let instruction = ThumbInstruction {
                guest_pc: 0x0800_0100,
                encoding: 0x2000 | value as u16,
                operation: ThumbOperation::MovImmediate { rd: 2, value },
            };
            let mut cpu = state(instruction.guest_pc);
            cpu.cpsr = 0xF000_003F;
            execute_thumb_instruction(&mut cpu, instruction).unwrap();
            assert_eq!(cpu.r[2], u32::from(value));
            assert_eq!(cpu.r[15], instruction.guest_pc + 2);
            assert_eq!(cpu.cpsr, expected_cpsr);
            assert_eq!(cpu.guest_cycle, 6);
        }
    }

    #[test]
    fn thumb_bl_halves_set_and_validate_link_register_before_static_dispatch() {
        let prefix = ThumbInstruction {
            guest_pc: 0x0800_0084,
            encoding: 0xF000,
            operation: ThumbOperation::BlPrefix {
                lr_partial: 0x0800_0088,
            },
        };
        let call = ThumbInstruction {
            guest_pc: 0x0800_0086,
            encoding: 0xF804,
            operation: ThumbOperation::BlCall {
                target: 0x0800_0090,
                expected_lr: 0x0800_0088,
                return_address: 0x0800_0089,
            },
        };
        let mut cpu = state(prefix.guest_pc);
        cpu.cpsr |= 0x20;
        let flags = cpu.cpsr;
        execute_thumb_instruction(&mut cpu, prefix).unwrap();
        assert_eq!(cpu.r[14], 0x0800_0088);
        assert_eq!(cpu.r[15], 0x0800_0086);
        assert_eq!(cpu.cpsr, flags);
        execute_thumb_instruction(&mut cpu, call).unwrap();
        assert_eq!(cpu.r[14], 0x0800_0089);
        assert_eq!(cpu.r[15], 0x0800_0090);
        assert_eq!(cpu.cpsr, flags);
        assert_eq!(cpu.guest_cycle, 7);
    }

    #[test]
    fn thumb_bl_suffix_rejects_a_different_incoming_lr() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0086,
            encoding: 0xF804,
            operation: ThumbOperation::BlCall {
                target: 0x0800_0090,
                expected_lr: 0x0800_0088,
                return_address: 0x0800_0089,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[14] = 0x0800_1234;
        let before = cpu;
        assert_eq!(
            execute_thumb_instruction(&mut cpu, instruction),
            Err(StepFault::ThumbBlStateMismatch {
                expected: 0x0800_0088,
                actual: 0x0800_1234,
            })
        );
        assert_eq!(cpu, before);
    }

    #[test]
    fn arm_program_verifier_rejects_high_destination_for_thumb_immediate_mov() {
        let program = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![ThumbInstruction {
                guest_pc: 0x0800_0100,
                encoding: 0x2000,
                operation: ThumbOperation::MovImmediate { rd: 8, value: 0 },
            }],
            branch_targets: vec![],
        };
        assert_eq!(
            program.verify(),
            Err(VerifyError::InvalidThumbImmediateRegister(8))
        );
    }

    #[test]
    fn arm_program_verifier_rejects_unresolved_thumb_bl_suffix() {
        let program = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![ThumbInstruction {
                guest_pc: 0x0800_0100,
                encoding: 0xF800,
                operation: ThumbOperation::BlSuffix { immediate: 0 },
            }],
            branch_targets: vec![],
        };
        assert_eq!(
            program.verify(),
            Err(VerifyError::UnresolvedThumbBlSuffix(0x0800_0100))
        );
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
            thumb_instructions: vec![],
            branch_targets: vec![],
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
            thumb_instructions: vec![],
            branch_targets: vec![],
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

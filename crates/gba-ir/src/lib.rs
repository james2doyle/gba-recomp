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
    LslImmediate {
        rd: u8,
        rs: u8,
        amount: u8,
    },
    AddSubtract {
        rd: u8,
        rn: u8,
        operand: u8,
        immediate: bool,
        subtract: bool,
    },
    StoreHalfwordImmediate {
        rd: u8,
        rb: u8,
        imm5: u8,
    },
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
    Swi {
        immediate: u8,
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
    SwiRuntimeRequired,
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
            Self::SwiRuntimeRequired => {
                f.write_str("Thumb SWI requires the versioned GBA HLE runtime")
            }
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
    InvalidThumbRomLiteralRegister(u8),
    InvalidThumbAddSubtractRegister(u8),
    InvalidThumbHalfwordStoreRegister(u8),
    EmptyBlock,
    SliceOnlyOneInstruction(usize),
    ProgramCounterRegisterUnsupported,
    InvalidRegister(u8),
    InvalidCondition(u8),
    MisalignedBranchTarget(u32),
    MisalignedRomLiteral(u32),
    UnresolvedRomLiteral(u32),
    MisalignedThumbRomLiteral(u32),
    UnresolvedThumbRomLiteral(u32),
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
            Self::InvalidThumbRomLiteralRegister(register) => write!(
                f,
                "Thumb ROM literal destination r{register} is not a low register"
            ),
            Self::InvalidThumbAddSubtractRegister(register) => write!(
                f,
                "Thumb ADD/SUB Format 2 register r{register} is not a low register"
            ),
            Self::InvalidThumbHalfwordStoreRegister(register) => write!(
                f,
                "Thumb STRH immediate register r{register} is not a low register"
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
            Self::MisalignedThumbRomLiteral(address) => write!(
                f,
                "Thumb ROM literal address 0x{address:08X} is not word aligned"
            ),
            Self::UnresolvedThumbRomLiteral(pc) => write!(
                f,
                "Thumb PC-relative ROM literal at 0x{pc:08X} was not resolved during discovery"
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
                ThumbOperation::LoadPcRelativeWord { .. } => {
                    return Err(VerifyError::UnresolvedThumbRomLiteral(instruction.guest_pc));
                }
                ThumbOperation::LoadRomLiteral {
                    rd, guest_address, ..
                } => {
                    if rd > 7 {
                        return Err(VerifyError::InvalidThumbRomLiteralRegister(rd));
                    }
                    if guest_address & 3 != 0 {
                        return Err(VerifyError::MisalignedThumbRomLiteral(guest_address));
                    }
                    let encoded_rd = ((instruction.encoding >> 8) & 0x7) as u8;
                    let expected_address =
                        thumb_rom_literal_address(instruction.guest_pc, instruction.encoding);
                    if instruction.encoding & 0xF800 != 0x4800
                        || rd != encoded_rd
                        || expected_address != Some(guest_address)
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            instruction.encoding,
                        )));
                    }
                }
                ThumbOperation::LslImmediate { rd, rs, amount } => {
                    let encoded_rd = (instruction.encoding & 0x7) as u8;
                    let encoded_rs = ((instruction.encoding >> 3) & 0x7) as u8;
                    let encoded_amount = ((instruction.encoding >> 6) & 0x1F) as u8;
                    if instruction.encoding & 0xF800 != 0
                        || rd != encoded_rd
                        || rs != encoded_rs
                        || amount != encoded_amount
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            instruction.encoding,
                        )));
                    }
                }
                ThumbOperation::AddSubtract {
                    rd,
                    rn,
                    operand,
                    immediate,
                    subtract,
                } => {
                    for register in [rd, rn, operand] {
                        if register > 7 {
                            return Err(VerifyError::InvalidThumbAddSubtractRegister(register));
                        }
                    }
                    let encoding = instruction.encoding;
                    let encoded_immediate = encoding & (1 << 10) != 0;
                    let encoded_subtract = encoding & (1 << 9) != 0;
                    let encoded_operand = ((encoding >> 6) & 0x7) as u8;
                    let encoded_rn = ((encoding >> 3) & 0x7) as u8;
                    let encoded_rd = (encoding & 0x7) as u8;
                    if encoding & 0xF800 != 0x1800
                        || immediate != encoded_immediate
                        || subtract != encoded_subtract
                        || operand != encoded_operand
                        || rn != encoded_rn
                        || rd != encoded_rd
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                }
                ThumbOperation::StoreHalfwordImmediate { rd, rb, imm5 } => {
                    for register in [rd, rb] {
                        if register > 7 {
                            return Err(VerifyError::InvalidThumbHalfwordStoreRegister(register));
                        }
                    }
                    let encoding = instruction.encoding;
                    let encoded_imm5 = ((encoding >> 6) & 0x1F) as u8;
                    let encoded_rb = ((encoding >> 3) & 0x7) as u8;
                    let encoded_rd = (encoding & 0x7) as u8;
                    if encoding & 0xF800 != 0x8000
                        || rd != encoded_rd
                        || rb != encoded_rb
                        || imm5 != encoded_imm5
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                }
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
                ThumbOperation::Swi { immediate }
                    if instruction.encoding & 0xFF00 != 0xDF00
                        || instruction.encoding as u8 != immediate
                        || immediate != 1 =>
                {
                    return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                        instruction.encoding,
                    )));
                }
                ThumbOperation::Swi { .. } => {}
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
                ThumbOperation::LoadRomLiteral { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::LoadPcRelativeWord { .. } => {
                    return Err(VerifyError::UnresolvedThumbRomLiteral(instruction.guest_pc));
                }
                ThumbOperation::LslImmediate { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::AddSubtract { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::StoreHalfwordImmediate { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
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
                ThumbOperation::Swi { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
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

fn thumb_rom_literal_address(guest_pc: u32, encoding: u16) -> Option<u32> {
    let pipeline_pc = guest_pc.checked_add(4)?;
    let aligned_pc = pipeline_pc & !3;
    let offset = u32::from(encoding & 0x00FF).checked_mul(4)?;
    aligned_pc.checked_add(offset)
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
        ThumbOperation::LoadPcRelativeWord { .. } => {
            return Err(StepFault::RomLiteralUnresolved);
        }
        ThumbOperation::LoadRomLiteral { rd, value, .. } => {
            state.r[rd as usize] = value;
            state.r[REG_PC as usize] = instruction.guest_pc.wrapping_add(2);
        }
        ThumbOperation::LslImmediate { rd, rs, amount } => {
            let old_source = state.r[rs as usize];
            let result = old_source.wrapping_shl(u32::from(amount));
            let carry = if amount == 0 {
                state.cpsr & 0x2000_0000
            } else {
                u32::from(old_source & (1_u32 << (32 - u32::from(amount))) != 0) << 29
            };
            state.r[rd as usize] = result;
            state.cpsr = (state.cpsr & !0xE000_0000)
                | (result & 0x8000_0000)
                | (u32::from(result == 0) << 30)
                | carry;
            state.r[REG_PC as usize] = instruction.guest_pc.wrapping_add(2);
        }
        ThumbOperation::AddSubtract {
            rd,
            rn,
            operand,
            immediate,
            subtract,
        } => {
            let lhs = state.r[rn as usize];
            let rhs = if immediate {
                u32::from(operand)
            } else {
                state.r[operand as usize]
            };
            let (result, carry, overflow) = if subtract {
                let result = lhs.wrapping_sub(rhs);
                (
                    result,
                    lhs >= rhs,
                    ((lhs ^ rhs) & (lhs ^ result) & 0x8000_0000) != 0,
                )
            } else {
                let wide = u64::from(lhs) + u64::from(rhs);
                let result = wide as u32;
                (
                    result,
                    wide > u64::from(u32::MAX),
                    (!(lhs ^ rhs) & (lhs ^ result) & 0x8000_0000) != 0,
                )
            };
            let nzcv = (result & 0x8000_0000)
                | (u32::from(result == 0) << 30)
                | (u32::from(carry) << 29)
                | (u32::from(overflow) << 28);
            state.r[rd as usize] = result;
            state.cpsr = (state.cpsr & !0xF000_0000) | nzcv;
            state.r[REG_PC as usize] = instruction.guest_pc.wrapping_add(2);
        }
        ThumbOperation::StoreHalfwordImmediate { .. } => {
            return Err(StepFault::MemoryRuntimeRequired);
        }
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
        ThumbOperation::Swi { .. } => return Err(StepFault::SwiRuntimeRequired),
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
    fn thumb_halfword_store_requires_memory_runtime_without_mutating_cpu_state() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0x87C8,
            operation: ThumbOperation::StoreHalfwordImmediate {
                rd: 0,
                rb: 1,
                imm5: 31,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[0] = 0xAABB_CCDD;
        cpu.r[1] = 0x0200_0001;
        let before = cpu;
        assert_eq!(
            execute_thumb_instruction(&mut cpu, instruction),
            Err(StepFault::MemoryRuntimeRequired)
        );
        assert_eq!(cpu, before);
    }

    #[test]
    fn thumb_swi_requires_the_explicit_hle_runtime_without_mutating_cpu_state() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0xDF01,
            operation: ThumbOperation::Swi { immediate: 1 },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[0] = 0xFF;
        let before = cpu;
        assert_eq!(
            execute_thumb_instruction(&mut cpu, instruction),
            Err(StepFault::SwiRuntimeRequired)
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
    fn thumb_lsl_immediate_updates_flags_with_armv4t_zero_shift_carry_rules() {
        let cases = [
            (0x8000_0000, 0, 0x0000_003F, 1, 1, 0x8000_003F),
            (0, 0, 0x3000_003F, 1, 1, 0x7000_003F),
            (0x8000_0001, 1, 0x0000_003F, 1, 1, 0x2000_003F),
            (0x4000_0000, 1, 0x3000_003F, 0, 1, 0x9000_003F),
            (0x0000_1000, 19, 0x3000_003F, 0, 1, 0x9000_003F),
            (0x0000_2000, 19, 0x0000_003F, 1, 1, 0x6000_003F),
            (2, 31, 0x0000_003F, 1, 1, 0x6000_003F),
            (1, 31, 0x3000_003F, 0, 1, 0x9000_003F),
        ];
        for (value, amount, cpsr, rd, rs, expected_cpsr) in cases {
            let guest_pc = 0x0800_0100;
            let encoding = (u16::from(amount) << 6) | (u16::from(rs) << 3) | u16::from(rd);
            let instruction = ThumbInstruction {
                guest_pc,
                encoding,
                operation: ThumbOperation::LslImmediate { rd, rs, amount },
            };
            let mut cpu = state(guest_pc);
            cpu.cpsr = cpsr;
            cpu.r[rs as usize] = value;
            let expected_value = value.wrapping_shl(u32::from(amount));
            execute_thumb_instruction(&mut cpu, instruction).unwrap();
            assert_eq!(cpu.r[rd as usize], expected_value);
            if rd != rs {
                assert_eq!(cpu.r[rs as usize], value);
            }
            assert_eq!(cpu.r[15], guest_pc + 2);
            assert_eq!(cpu.cpsr, expected_cpsr);
            assert_eq!(cpu.guest_cycle, 6);
        }
    }

    #[test]
    fn thumb_addsub_format2_sets_all_nzcv_flags_and_preserves_control_bits() {
        let cases = [
            (0, 1, 1, true, false, 0xFFFF_FFFF, 1, 0, 0x6000_00FF),
            (
                0,
                1,
                1,
                true,
                false,
                0x7FFF_FFFF,
                1,
                0x8000_0000,
                0x9000_00FF,
            ),
            (0, 1, 1, true, true, 0, 1, 0xFFFF_FFFF, 0x8000_00FF),
            (
                0,
                1,
                1,
                true,
                true,
                0x8000_0000,
                1,
                0x7FFF_FFFF,
                0x3000_00FF,
            ),
            (
                0,
                1,
                0,
                true,
                false,
                0x8000_0000,
                0,
                0x8000_0000,
                0x8000_00FF,
            ),
            (
                0,
                1,
                0,
                true,
                true,
                0x8000_0000,
                0,
                0x8000_0000,
                0xA000_00FF,
            ),
            (0, 1, 0, false, false, 2, 1, 3, 0x0000_00FF),
            (1, 1, 0, false, true, 3, 1, 2, 0x2000_00FF),
            (1, 1, 7, true, false, 5, 7, 12, 0x0000_00FF),
        ];
        for (rd, rn, operand, immediate, subtract, lhs, rhs, expected, expected_cpsr) in cases {
            let guest_pc = 0x0800_0100;
            let instruction = ThumbInstruction {
                guest_pc,
                encoding: 0x1800
                    | (u16::from(immediate) << 10)
                    | (u16::from(subtract) << 9)
                    | (u16::from(operand) << 6)
                    | (u16::from(rn) << 3)
                    | u16::from(rd),
                operation: ThumbOperation::AddSubtract {
                    rd,
                    rn,
                    operand,
                    immediate,
                    subtract,
                },
            };
            let mut cpu = state(guest_pc);
            cpu.cpsr = 0xF000_00FF;
            cpu.r[rn as usize] = lhs;
            if !immediate {
                cpu.r[operand as usize] = rhs;
            }
            let before = cpu.r;
            execute_thumb_instruction(&mut cpu, instruction).unwrap();
            assert_eq!(cpu.r[rd as usize], expected);
            assert_eq!(cpu.cpsr, expected_cpsr);
            assert_eq!(cpu.cpsr & 0x0FFF_FFFF, 0x0000_00FF);
            for register in 0..16 {
                if register != usize::from(rd) && register != usize::from(REG_PC) {
                    assert_eq!(cpu.r[register], before[register]);
                }
            }
            assert_eq!(cpu.r[REG_PC as usize], guest_pc + 2);
            assert_eq!(cpu.guest_cycle, 6);
        }

        let guest_pc = 0x0800_0100;
        let subtract_zero = ThumbInstruction {
            guest_pc,
            encoding: 0x1E09,
            operation: ThumbOperation::AddSubtract {
                rd: 1,
                rn: 1,
                operand: 0,
                immediate: true,
                subtract: true,
            },
        };
        let mut cpu = state(guest_pc);
        cpu.cpsr = 0x0000_00FF;
        cpu.r[1] = 9;
        execute_thumb_instruction(&mut cpu, subtract_zero).unwrap();
        assert_eq!(cpu.r[1], 9);
        assert_eq!(cpu.cpsr, 0x2000_00FF);
    }

    #[test]
    fn thumb_rom_literal_loads_constant_without_changing_flags() {
        let guest_pc = 0x0800_0086;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x4916,
            operation: ThumbOperation::LoadRomLiteral {
                rd: 1,
                guest_address: 0x0800_00E0,
                value: 0x0800_00D4,
            },
        };
        let mut cpu = state(guest_pc);
        cpu.cpsr = 0xB000_003F;
        cpu.r[1] = 0xDEAD_BEEF;
        cpu.r[2] = 0x1234_5678;
        let flags_and_control = cpu.cpsr;
        execute_thumb_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[1], 0x0800_00D4);
        assert_eq!(cpu.r[2], 0x1234_5678);
        assert_eq!(cpu.cpsr, flags_and_control);
        assert_eq!(cpu.r[15], guest_pc + 2);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn unresolved_thumb_rom_literal_fails_without_mutating_cpu_state() {
        let guest_pc = 0x0800_0086;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x4916,
            operation: ThumbOperation::LoadPcRelativeWord {
                rd: 1,
                guest_address: 0x0800_00E0,
            },
        };
        let mut cpu = state(guest_pc);
        cpu.cpsr |= 0x20;
        let before = cpu;
        assert_eq!(
            execute_thumb_instruction(&mut cpu, instruction),
            Err(StepFault::RomLiteralUnresolved)
        );
        assert_eq!(cpu, before);
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
    fn arm_program_verifier_checks_thumb_lsl_encoding_fields_and_alignment() {
        let make_program = |guest_pc, rd, rs, amount| ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![ThumbInstruction {
                guest_pc,
                encoding: 0x04C9,
                operation: ThumbOperation::LslImmediate { rd, rs, amount },
            }],
            branch_targets: vec![],
        };
        assert_eq!(
            make_program(0x0800_0100, 0, 1, 19).verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x04C9))
        );
        assert_eq!(
            make_program(0x0800_0100, 1, 1, 18).verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x04C9))
        );
        assert_eq!(
            make_program(0x0800_0101, 1, 1, 19).verify(),
            Err(VerifyError::MisalignedThumbPc(0x0800_0101))
        );
        assert_eq!(
            make_program(0x0800_0100, 1, 1, 19).verify(),
            Err(VerifyError::MissingSuccessor {
                pc: 0x0800_0100,
                target: 0x0800_0102,
            })
        );
    }

    #[test]
    fn arm_program_verifier_checks_thumb_addsub_fields_alignment_and_fallthrough() {
        let guest_pc = 0x0800_0100;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x1C10,
            operation: ThumbOperation::AddSubtract {
                rd: 0,
                rn: 2,
                operand: 0,
                immediate: true,
                subtract: false,
            },
        };
        let arm_loop = ArmInstruction {
            guest_pc: 0x0800_0080,
            encoding: 0xEAFF_FFFE,
            condition: 14,
            operation: ArmOperation::Branch {
                target: 0x0800_0080,
                link: false,
            },
        };
        let bx = ThumbInstruction {
            guest_pc: guest_pc + 2,
            encoding: 0x4700,
            operation: ThumbOperation::BranchExchange { rm: 0 },
        };
        let valid_program = || ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![arm_loop],
            thumb_instructions: vec![instruction, bx],
            branch_targets: vec![StaticBranchTarget {
                source_mode: IsaMode::Thumb,
                source_pc: guest_pc + 2,
                target_mode: IsaMode::Arm,
                target_pc: arm_loop.guest_pc,
            }],
        };
        assert!(valid_program().verify().is_ok());

        let mut mismatched = valid_program();
        mismatched.thumb_instructions[0].operation = ThumbOperation::AddSubtract {
            rd: 0,
            rn: 1,
            operand: 0,
            immediate: true,
            subtract: false,
        };
        assert_eq!(
            mismatched.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x1C10))
        );

        for operation in [
            ThumbOperation::AddSubtract {
                rd: 1,
                rn: 2,
                operand: 0,
                immediate: true,
                subtract: false,
            },
            ThumbOperation::AddSubtract {
                rd: 0,
                rn: 2,
                operand: 1,
                immediate: true,
                subtract: false,
            },
            ThumbOperation::AddSubtract {
                rd: 0,
                rn: 2,
                operand: 0,
                immediate: false,
                subtract: false,
            },
            ThumbOperation::AddSubtract {
                rd: 0,
                rn: 2,
                operand: 0,
                immediate: true,
                subtract: true,
            },
        ] {
            let mut mismatched_field = valid_program();
            mismatched_field.thumb_instructions[0].operation = operation;
            assert_eq!(
                mismatched_field.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(0x1C10))
            );
        }

        let mut out_of_family = valid_program();
        out_of_family.thumb_instructions[0].encoding = 0x4008;
        assert_eq!(
            out_of_family.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x4008))
        );

        let mut invalid_register = valid_program();
        invalid_register.thumb_instructions[0].operation = ThumbOperation::AddSubtract {
            rd: 8,
            rn: 2,
            operand: 0,
            immediate: true,
            subtract: false,
        };
        assert_eq!(
            invalid_register.verify(),
            Err(VerifyError::InvalidThumbAddSubtractRegister(8))
        );

        let mut misaligned = valid_program();
        misaligned.thumb_instructions[0].guest_pc += 1;
        assert_eq!(
            misaligned.verify(),
            Err(VerifyError::MisalignedThumbPc(guest_pc + 1))
        );

        let mut missing_fallthrough = valid_program();
        missing_fallthrough.thumb_instructions.pop();
        missing_fallthrough.branch_targets.clear();
        assert_eq!(
            missing_fallthrough.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: guest_pc,
                target: guest_pc + 2,
            })
        );
    }

    #[test]
    fn arm_program_verifier_checks_thumb_strh_fields_and_fallthrough() {
        let guest_pc = 0x0800_0100;
        let store = ThumbInstruction {
            guest_pc,
            encoding: 0x8008,
            operation: ThumbOperation::StoreHalfwordImmediate {
                rd: 0,
                rb: 1,
                imm5: 0,
            },
        };
        let make_program = |store| ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![
                store,
                ThumbInstruction {
                    guest_pc: guest_pc + 2,
                    encoding: 0x4700,
                    operation: ThumbOperation::BranchExchange { rm: 0 },
                },
            ],
            branch_targets: vec![StaticBranchTarget {
                source_mode: IsaMode::Thumb,
                source_pc: guest_pc + 2,
                target_mode: IsaMode::Thumb,
                target_pc: guest_pc,
            }],
        };
        assert!(make_program(store).verify().is_ok());

        let max_offset_store = ThumbInstruction {
            guest_pc,
            encoding: 0x87C8,
            operation: ThumbOperation::StoreHalfwordImmediate {
                rd: 0,
                rb: 1,
                imm5: 31,
            },
        };
        assert!(make_program(max_offset_store).verify().is_ok());

        let aliased_store = ThumbInstruction {
            guest_pc,
            encoding: 0x8009,
            operation: ThumbOperation::StoreHalfwordImmediate {
                rd: 1,
                rb: 1,
                imm5: 0,
            },
        };
        assert!(make_program(aliased_store).verify().is_ok());

        for operation in [
            ThumbOperation::StoreHalfwordImmediate {
                rd: 1,
                rb: 1,
                imm5: 0,
            },
            ThumbOperation::StoreHalfwordImmediate {
                rd: 0,
                rb: 0,
                imm5: 0,
            },
            ThumbOperation::StoreHalfwordImmediate {
                rd: 0,
                rb: 1,
                imm5: 1,
            },
        ] {
            let mut mismatched = make_program(store);
            mismatched.thumb_instructions[0].operation = operation;
            assert_eq!(
                mismatched.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(0x8008))
            );
        }

        for encoding in [0x8808, 0x5A08, 0x5208] {
            let mut out_of_family = make_program(store);
            out_of_family.thumb_instructions[0].encoding = encoding;
            assert_eq!(
                out_of_family.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                    encoding,
                )))
            );
        }

        let mut invalid_register = make_program(store);
        invalid_register.thumb_instructions[0].operation = ThumbOperation::StoreHalfwordImmediate {
            rd: 8,
            rb: 1,
            imm5: 0,
        };
        assert_eq!(
            invalid_register.verify(),
            Err(VerifyError::InvalidThumbHalfwordStoreRegister(8))
        );

        let missing_fallthrough = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![store],
            branch_targets: vec![],
        };
        assert_eq!(
            missing_fallthrough.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: guest_pc,
                target: guest_pc + 2,
            })
        );
    }

    #[test]
    fn arm_program_verifier_checks_thumb_rom_literal_resolution_and_fields() {
        let make_program = |operation: ThumbOperation, encoding: u16, thumb_pc: u32| {
            let bx_pc = thumb_pc.wrapping_add(2);
            ArmProgram {
                version: GBA_IR_VERSION,
                instructions: vec![ArmInstruction {
                    guest_pc: 0x0800_0090,
                    encoding: 0xEAFF_FFFE,
                    condition: 14,
                    operation: ArmOperation::Branch {
                        target: 0x0800_0090,
                        link: false,
                    },
                }],
                thumb_instructions: vec![
                    ThumbInstruction {
                        guest_pc: thumb_pc,
                        encoding,
                        operation,
                    },
                    ThumbInstruction {
                        guest_pc: bx_pc,
                        encoding: 0x4708,
                        operation: ThumbOperation::BranchExchange { rm: 1 },
                    },
                ],
                branch_targets: vec![StaticBranchTarget {
                    source_mode: IsaMode::Thumb,
                    source_pc: bx_pc,
                    target_mode: IsaMode::Arm,
                    target_pc: 0x0800_0090,
                }],
            }
        };
        let resolved = make_program(
            ThumbOperation::LoadRomLiteral {
                rd: 1,
                guest_address: 0x0800_008C,
                value: 0x0800_0090,
            },
            0x4901,
            0x0800_0086,
        );
        assert!(resolved.verify().is_ok());
        assert_eq!(
            make_program(
                ThumbOperation::LoadPcRelativeWord {
                    rd: 1,
                    guest_address: 0x0800_008C,
                },
                0x4901,
                0x0800_0086,
            )
            .verify(),
            Err(VerifyError::UnresolvedThumbRomLiteral(0x0800_0086))
        );
        assert_eq!(
            make_program(
                ThumbOperation::LoadRomLiteral {
                    rd: 2,
                    guest_address: 0x0800_008C,
                    value: 0,
                },
                0x4901,
                0x0800_0086,
            )
            .verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x4901))
        );
        assert_eq!(
            make_program(
                ThumbOperation::LoadRomLiteral {
                    rd: 1,
                    guest_address: 0x0800_008D,
                    value: 0,
                },
                0x4901,
                0x0800_0086,
            )
            .verify(),
            Err(VerifyError::MisalignedThumbRomLiteral(0x0800_008D))
        );
        assert_eq!(
            make_program(
                ThumbOperation::LoadRomLiteral {
                    rd: 1,
                    guest_address: 0x0800_008C,
                    value: 0,
                },
                0x6801,
                0x0800_0086,
            )
            .verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x6801))
        );
        assert_eq!(
            make_program(
                ThumbOperation::LoadRomLiteral {
                    rd: 8,
                    guest_address: 0x0800_008C,
                    value: 0,
                },
                0x4901,
                0x0800_0086,
            )
            .verify(),
            Err(VerifyError::InvalidThumbRomLiteralRegister(8))
        );
        assert_eq!(
            make_program(
                ThumbOperation::LoadRomLiteral {
                    rd: 1,
                    guest_address: 0x0800_0090,
                    value: 0,
                },
                0x4901,
                0x0800_0086,
            )
            .verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x4901))
        );
        assert_eq!(
            make_program(
                ThumbOperation::LoadRomLiteral {
                    rd: 1,
                    guest_address: 0x0800_008C,
                    value: 0,
                },
                0x4901,
                0x0800_0087,
            )
            .verify(),
            Err(VerifyError::MisalignedThumbPc(0x0800_0087))
        );
        let mut missing_successor = resolved;
        missing_successor.thumb_instructions.pop();
        missing_successor.branch_targets.clear();
        assert_eq!(
            missing_successor.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: 0x0800_0086,
                target: 0x0800_0088,
            })
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

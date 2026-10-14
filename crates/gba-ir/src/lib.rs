#![forbid(unsafe_code)]

use std::fmt;

pub const GBA_IR_VERSION: u16 = 2;
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
        /// Discovery certificate: bit 0 is taken; bit 1 is conditional fallthrough.
        reachable_successors: u8,
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
    LsrImmediate {
        rd: u8,
        rs: u8,
        amount: u8,
    },
    OrrRegister {
        rd: u8,
        rs: u8,
    },
    AddSubtract {
        rd: u8,
        rn: u8,
        operand: u8,
        immediate: bool,
        subtract: bool,
    },
    StoreWordImmediate {
        rd: u8,
        rb: u8,
        imm5: u8,
    },
    StoreHalfwordImmediate {
        rd: u8,
        rb: u8,
        imm5: u8,
    },
    StoreByteImmediate {
        rd: u8,
        rb: u8,
        imm5: u8,
    },
    LoadByteImmediate {
        rd: u8,
        rb: u8,
        imm5: u8,
    },
    LoadWordImmediate {
        rd: u8,
        rb: u8,
        imm5: u8,
    },
    LoadHalfwordImmediate {
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
    Pop {
        register_list: u16,
    },
    LoadMultipleIncrementAfter {
        base_register: u8,
        register_list: u16,
    },
    StoreMultipleIncrementAfter {
        base_register: u8,
        register_list: u16,
    },
    /// LDMIA whose singleton base was resolved against the immutable ROM image.
    /// Values are indexed by register number; entries outside register_list are ignored.
    LoadRomMultipleIncrementAfter {
        base_register: u8,
        register_list: u16,
        base_address: u32,
        values_by_register: [u32; 8],
    },
    /// LDMIA resolved with a finite runtime base-to-ROM-word table.
    LoadRomMultipleIncrementAfterCases {
        base_register: u8,
        register_list: u16,
        table_id: u32,
    },
    MovImmediate {
        rd: u8,
        value: u8,
    },
    CompareImmediate {
        rd: u8,
        immediate: u8,
    },
    AddSubtractImmediate {
        rd: u8,
        immediate: u8,
        subtract: bool,
    },
    ConditionalBranch {
        condition: u8,
        target: u32,
        /// Discovery certificate: bit 0 is taken; bit 1 is fallthrough.
        reachable_successors: u8,
    },
    UnconditionalBranch {
        target: u32,
    },
    MovRegister {
        rd: u8,
        rm: u8,
    },
    AndRegister {
        rd: u8,
        rs: u8,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbLdmiaRomTable {
    pub cases: Vec<ThumbLdmiaRomCase>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThumbLdmiaRomCase {
    pub base_address: u32,
    /// Values are indexed by register number. Unselected registers must be zero.
    pub values_by_register: [u32; 8],
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
    pub thumb_ldmia_rom_tables: Vec<ThumbLdmiaRomTable>,
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
    InvalidThumbPopRegisterList(u16),
    InvalidThumbLdmiaRegisterList(u16),
    InvalidThumbStmiaRegisterList(u16),
    InvalidThumbImmediateRegister(u8),
    InvalidThumbFormat3Register(u8),
    InvalidThumbRomLiteralRegister(u8),
    InvalidThumbAddSubtractRegister(u8),
    InvalidThumbWordStoreRegister(u8),
    InvalidThumbHalfwordStoreRegister(u8),
    InvalidThumbByteStoreRegister(u8),
    InvalidThumbByteLoadRegister(u8),
    InvalidThumbWordLoadRegister(u8),
    InvalidThumbHalfwordLoadRegister(u8),
    InvalidThumbLsrRegister(u8),
    InvalidThumbOrrRegister(u8),
    InvalidThumbAndRegister(u8),
    InvalidThumbBranchCondition(u8),
    InvalidThumbBranchSuccessorMask(u8),
    InvalidArmBranchSuccessorMask(u8),
    InvalidThumbLdmiaRomTableCaseCount(usize),
    UnorderedThumbLdmiaRomTable {
        table_id: usize,
        case_index: usize,
    },
    InvalidThumbLdmiaRomTableBase {
        table_id: usize,
        case_index: usize,
        base_address: u32,
    },
    UnreferencedThumbLdmiaRomTable(usize),
    NonzeroUnselectedThumbLdmiaRomValue {
        table_id: usize,
        case_index: usize,
        register: u8,
    },
    InvalidThumbLdmiaRomTableId(u32),
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
    BranchExchangeWithoutTargets {
        mode: IsaMode,
        pc: u32,
    },
    BranchTargetSourceInvalid {
        mode: IsaMode,
        pc: u32,
    },
    UnresolvedThumbBlSuffix(u32),
    InvalidThumbBlPair(u32),
    UnsupportedInstructionEncoding(u32),
    MissingSuccessor {
        pc: u32,
        target: u32,
    },
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
            Self::InvalidThumbPopRegisterList(list) => {
                write!(f, "invalid Thumb POP register list 0x{list:04X}")
            }
            Self::InvalidThumbLdmiaRegisterList(list) => {
                write!(f, "invalid Thumb LDMIA register list 0x{list:04X}")
            }
            Self::InvalidThumbStmiaRegisterList(list) => {
                write!(
                    f,
                    "invalid or unpredictable Thumb STMIA register list 0x{list:04X}"
                )
            }
            Self::InvalidThumbImmediateRegister(register) => write!(
                f,
                "Thumb immediate MOV destination r{register} is not a low register"
            ),
            Self::InvalidThumbFormat3Register(register) => write!(
                f,
                "Thumb Format 3 register r{register} is not a low register"
            ),
            Self::InvalidThumbRomLiteralRegister(register) => write!(
                f,
                "Thumb ROM literal destination r{register} is not a low register"
            ),
            Self::InvalidThumbAddSubtractRegister(register) => write!(
                f,
                "Thumb ADD/SUB Format 2 register r{register} is not a low register"
            ),
            Self::InvalidThumbWordStoreRegister(register) => write!(
                f,
                "Thumb STR immediate register r{register} is not a low register"
            ),
            Self::InvalidThumbHalfwordStoreRegister(register) => write!(
                f,
                "Thumb STRH immediate register r{register} is not a low register"
            ),
            Self::InvalidThumbByteStoreRegister(register) => write!(
                f,
                "Thumb STRB immediate register r{register} is not a low register"
            ),
            Self::InvalidThumbByteLoadRegister(register) => write!(
                f,
                "Thumb LDRB immediate register r{register} is not a low register"
            ),
            Self::InvalidThumbWordLoadRegister(register) => write!(
                f,
                "Thumb LDR immediate register r{register} is not a low register"
            ),
            Self::InvalidThumbHalfwordLoadRegister(register) => write!(
                f,
                "Thumb LDRH immediate register r{register} is not a low register"
            ),
            Self::InvalidThumbLsrRegister(register) => {
                write!(
                    f,
                    "Thumb LSR immediate register r{register} is not a low register"
                )
            }
            Self::InvalidThumbOrrRegister(register) => {
                write!(f, "Thumb ORR register r{register} is not a low register")
            }
            Self::InvalidThumbAndRegister(register) => {
                write!(f, "Thumb AND register r{register} is not a low register")
            }
            Self::InvalidThumbBranchCondition(condition) => write!(
                f,
                "Thumb conditional branch condition {condition:#X} is outside 0x0..=0xD"
            ),
            Self::InvalidThumbBranchSuccessorMask(mask) => write!(
                f,
                "Thumb conditional branch successor mask 0b{mask:08b} is empty or has reserved bits"
            ),
            Self::InvalidArmBranchSuccessorMask(mask) => write!(
                f,
                "ARM branch successor mask 0b{mask:08b} is empty or has reserved bits"
            ),
            Self::InvalidThumbLdmiaRomTableCaseCount(count) => write!(
                f,
                "Thumb ROM LDMIA table must contain 2..=16 cases, got {count}"
            ),
            Self::UnorderedThumbLdmiaRomTable {
                table_id,
                case_index,
            } => write!(
                f,
                "Thumb ROM LDMIA table {table_id} case {case_index} is not strictly ordered by base address"
            ),
            Self::InvalidThumbLdmiaRomTableBase {
                table_id,
                case_index,
                base_address,
            } => write!(
                f,
                "Thumb ROM LDMIA table {table_id} case {case_index} base 0x{base_address:08X} is outside the GBA ROM window"
            ),
            Self::UnreferencedThumbLdmiaRomTable(table_id) => {
                write!(f, "Thumb ROM LDMIA table {table_id} is not referenced")
            }
            Self::NonzeroUnselectedThumbLdmiaRomValue {
                table_id,
                case_index,
                register,
            } => write!(
                f,
                "Thumb ROM LDMIA table {table_id} case {case_index} has a value for unselected r{register}"
            ),
            Self::InvalidThumbLdmiaRomTableId(table_id) => {
                write!(f, "Thumb ROM LDMIA references missing table {table_id}")
            }
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

fn thumb_format3_encoding_matches(encoding: u16, opcode: u8, rd: u8, immediate: u8) -> bool {
    encoding & 0xE000 == 0x2000
        && ((encoding >> 11) & 0x3) as u8 == opcode
        && ((encoding >> 8) & 0x7) as u8 == rd
        && encoding as u8 == immediate
}

impl ArmProgram {
    pub fn verify(&self) -> Result<(), VerifyError> {
        if self.version != GBA_IR_VERSION {
            return Err(VerifyError::UnsupportedVersion(self.version));
        }
        if self.instructions.is_empty() && self.thumb_instructions.is_empty() {
            return Err(VerifyError::EmptyBlock);
        }
        for (table_id, table) in self.thumb_ldmia_rom_tables.iter().enumerate() {
            if !(2..=16).contains(&table.cases.len()) {
                return Err(VerifyError::InvalidThumbLdmiaRomTableCaseCount(
                    table.cases.len(),
                ));
            }
            for (case_index, case) in table.cases.iter().enumerate() {
                if !(0x0800_0000..0x0A00_0000).contains(&case.base_address) {
                    return Err(VerifyError::InvalidThumbLdmiaRomTableBase {
                        table_id,
                        case_index,
                        base_address: case.base_address,
                    });
                }
                if case_index == 0 {
                    continue;
                }
                if table.cases[case_index - 1].base_address >= table.cases[case_index].base_address
                {
                    return Err(VerifyError::UnorderedThumbLdmiaRomTable {
                        table_id,
                        case_index,
                    });
                }
            }
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
                ArmOperation::Branch {
                    target,
                    reachable_successors,
                    ..
                } => {
                    if target & 3 != 0 {
                        return Err(VerifyError::MisalignedBranchTarget(target));
                    }
                    if reachable_successors == 0 || reachable_successors & !0b11 != 0 {
                        return Err(VerifyError::InvalidArmBranchSuccessorMask(
                            reachable_successors,
                        ));
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
        let mut referenced_thumb_ldmia_tables = vec![false; self.thumb_ldmia_rom_tables.len()];
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
                ThumbOperation::LsrImmediate { rd, rs, amount } => {
                    for register in [rd, rs] {
                        if register > 7 {
                            return Err(VerifyError::InvalidThumbLsrRegister(register));
                        }
                    }
                    let encoding = instruction.encoding;
                    let encoded_rd = (encoding & 0x7) as u8;
                    let encoded_rs = ((encoding >> 3) & 0x7) as u8;
                    let encoded_amount = ((encoding >> 6) & 0x1F) as u8;
                    if encoding & 0xF800 != 0x0800
                        || rd != encoded_rd
                        || rs != encoded_rs
                        || amount != encoded_amount
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                }
                ThumbOperation::OrrRegister { rd, rs } => {
                    for register in [rd, rs] {
                        if register > 7 {
                            return Err(VerifyError::InvalidThumbOrrRegister(register));
                        }
                    }
                    let encoding = instruction.encoding;
                    let encoded_rd = (encoding & 0x7) as u8;
                    let encoded_rs = ((encoding >> 3) & 0x7) as u8;
                    if encoding & 0xFFC0 != 0x4300 || rd != encoded_rd || rs != encoded_rs {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                }
                ThumbOperation::AndRegister { rd, rs } => {
                    for register in [rd, rs] {
                        if register > 7 {
                            return Err(VerifyError::InvalidThumbAndRegister(register));
                        }
                    }
                    let encoding = instruction.encoding;
                    let encoded_rd = (encoding & 0x7) as u8;
                    let encoded_rs = ((encoding >> 3) & 0x7) as u8;
                    if encoding & 0xFFC0 != 0x4000 || rd != encoded_rd || rs != encoded_rs {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
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
                ThumbOperation::StoreWordImmediate { rd, rb, imm5 } => {
                    for register in [rd, rb] {
                        if register > 7 {
                            return Err(VerifyError::InvalidThumbWordStoreRegister(register));
                        }
                    }
                    let encoding = instruction.encoding;
                    let encoded_imm5 = ((encoding >> 6) & 0x1F) as u8;
                    let encoded_rb = ((encoding >> 3) & 0x7) as u8;
                    let encoded_rd = (encoding & 0x7) as u8;
                    if encoding & 0xF800 != 0x6000
                        || rd != encoded_rd
                        || rb != encoded_rb
                        || imm5 != encoded_imm5
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                }
                ThumbOperation::StoreByteImmediate { rd, rb, imm5 } => {
                    for register in [rd, rb] {
                        if register > 7 {
                            return Err(VerifyError::InvalidThumbByteStoreRegister(register));
                        }
                    }
                    let encoding = instruction.encoding;
                    let encoded_imm5 = ((encoding >> 6) & 0x1F) as u8;
                    let encoded_rb = ((encoding >> 3) & 0x7) as u8;
                    let encoded_rd = (encoding & 0x7) as u8;
                    if encoding & 0xF800 != 0x7000
                        || rd != encoded_rd
                        || rb != encoded_rb
                        || imm5 != encoded_imm5
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                }
                ThumbOperation::LoadByteImmediate { rd, rb, imm5 } => {
                    for register in [rd, rb] {
                        if register > 7 {
                            return Err(VerifyError::InvalidThumbByteLoadRegister(register));
                        }
                    }
                    let encoding = instruction.encoding;
                    let encoded_imm5 = ((encoding >> 6) & 0x1F) as u8;
                    let encoded_rb = ((encoding >> 3) & 0x7) as u8;
                    let encoded_rd = (encoding & 0x7) as u8;
                    if encoding & 0xF800 != 0x7800
                        || rd != encoded_rd
                        || rb != encoded_rb
                        || imm5 != encoded_imm5
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                }
                ThumbOperation::LoadWordImmediate { rd, rb, imm5 } => {
                    for register in [rd, rb] {
                        if register > 7 {
                            return Err(VerifyError::InvalidThumbWordLoadRegister(register));
                        }
                    }
                    let encoding = instruction.encoding;
                    let encoded_imm5 = ((encoding >> 6) & 0x1F) as u8;
                    let encoded_rb = ((encoding >> 3) & 0x7) as u8;
                    let encoded_rd = (encoding & 0x7) as u8;
                    if encoding & 0xF800 != 0x6800
                        || rd != encoded_rd
                        || rb != encoded_rb
                        || imm5 != encoded_imm5
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                }
                ThumbOperation::LoadHalfwordImmediate { rd, rb, imm5 } => {
                    for register in [rd, rb] {
                        if register > 7 {
                            return Err(VerifyError::InvalidThumbHalfwordLoadRegister(register));
                        }
                    }
                    let encoding = instruction.encoding;
                    let encoded_imm5 = ((encoding >> 6) & 0x1F) as u8;
                    let encoded_rb = ((encoding >> 3) & 0x7) as u8;
                    let encoded_rd = (encoding & 0x7) as u8;
                    if encoding & 0xF800 != 0x8800
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
                ThumbOperation::MovImmediate { rd, value } => {
                    if rd > 7 {
                        return Err(VerifyError::InvalidThumbImmediateRegister(rd));
                    }
                    if !thumb_format3_encoding_matches(instruction.encoding, 0, rd, value) {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            instruction.encoding,
                        )));
                    }
                }
                ThumbOperation::CompareImmediate { rd, immediate } => {
                    if rd > 7 {
                        return Err(VerifyError::InvalidThumbFormat3Register(rd));
                    }
                    if !thumb_format3_encoding_matches(instruction.encoding, 1, rd, immediate) {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            instruction.encoding,
                        )));
                    }
                }
                ThumbOperation::AddSubtractImmediate {
                    rd,
                    immediate,
                    subtract,
                } => {
                    if rd > 7 {
                        return Err(VerifyError::InvalidThumbFormat3Register(rd));
                    }
                    let opcode = if subtract { 3 } else { 2 };
                    if !thumb_format3_encoding_matches(instruction.encoding, opcode, rd, immediate)
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            instruction.encoding,
                        )));
                    }
                }
                ThumbOperation::ConditionalBranch {
                    condition,
                    target,
                    reachable_successors,
                } => {
                    if condition > 0xD {
                        return Err(VerifyError::InvalidThumbBranchCondition(condition));
                    }
                    if reachable_successors == 0 || reachable_successors & !0b11 != 0 {
                        return Err(VerifyError::InvalidThumbBranchSuccessorMask(
                            reachable_successors,
                        ));
                    }
                    let encoded_condition = ((instruction.encoding >> 8) & 0xF) as u8;
                    let immediate = i32::from(instruction.encoding as u8 as i8) << 1;
                    let encoded_target = instruction
                        .guest_pc
                        .wrapping_add(4)
                        .wrapping_add(immediate as u32);
                    if instruction.encoding & 0xF000 != 0xD000
                        || condition != encoded_condition
                        || target != encoded_target
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            instruction.encoding,
                        )));
                    }
                    if target & 1 != 0 {
                        return Err(VerifyError::MisalignedThumbPc(target));
                    }
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if fallthrough & 1 != 0 {
                        return Err(VerifyError::MisalignedThumbPc(fallthrough));
                    }
                }
                ThumbOperation::UnconditionalBranch { target } => {
                    let immediate = i32::from(((instruction.encoding & 0x07FF) << 5) as i16 >> 5);
                    let encoded_target = instruction
                        .guest_pc
                        .wrapping_add(4)
                        .wrapping_add((immediate << 1) as u32);
                    if instruction.encoding & 0xF800 != 0xE000 || target != encoded_target {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            instruction.encoding,
                        )));
                    }
                    if target & 1 != 0 {
                        return Err(VerifyError::MisalignedThumbPc(target));
                    }
                }
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
                ThumbOperation::Pop { register_list } => {
                    let encoding = instruction.encoding;
                    if encoding & 0xFE00 != 0xBC00 || encoding & 0x0100 != 0 {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                    if register_list == 0 || register_list & !0x00FF != 0 {
                        return Err(VerifyError::InvalidThumbPopRegisterList(register_list));
                    }
                    if register_list != encoding & 0x00FF {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                }
                ThumbOperation::LoadMultipleIncrementAfter {
                    base_register,
                    register_list,
                }
                | ThumbOperation::LoadRomMultipleIncrementAfter {
                    base_register,
                    register_list,
                    ..
                }
                | ThumbOperation::LoadRomMultipleIncrementAfterCases {
                    base_register,
                    register_list,
                    ..
                } => {
                    let encoding = instruction.encoding;
                    if base_register > 7 {
                        return Err(VerifyError::InvalidThumbLdmiaRegisterList(register_list));
                    }
                    if register_list == 0 || register_list & !0x00FF != 0 {
                        return Err(VerifyError::InvalidThumbLdmiaRegisterList(register_list));
                    }
                    let encoded_base = ((encoding >> 8) & 0x7) as u8;
                    if encoding & 0xF800 != 0xC800
                        || base_register != encoded_base
                        || register_list != encoding & 0x00FF
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                    if let ThumbOperation::LoadRomMultipleIncrementAfter {
                        values_by_register,
                        ..
                    } = instruction.operation
                    {
                        for register in 0..8 {
                            if register_list & (1 << register) == 0
                                && values_by_register[register] != 0
                            {
                                return Err(VerifyError::UnsupportedInstructionEncoding(
                                    u32::from(encoding),
                                ));
                            }
                        }
                    }
                    if let ThumbOperation::LoadRomMultipleIncrementAfterCases { table_id, .. } =
                        instruction.operation
                    {
                        let table_index = usize::try_from(table_id)
                            .ok()
                            .filter(|index| *index < self.thumb_ldmia_rom_tables.len())
                            .ok_or(VerifyError::InvalidThumbLdmiaRomTableId(table_id))?;
                        referenced_thumb_ldmia_tables[table_index] = true;
                        for (case_index, case) in self.thumb_ldmia_rom_tables[table_index]
                            .cases
                            .iter()
                            .enumerate()
                        {
                            for register in 0..8 {
                                if register_list & (1 << register) == 0
                                    && case.values_by_register[register] != 0
                                {
                                    return Err(VerifyError::NonzeroUnselectedThumbLdmiaRomValue {
                                        table_id: table_index,
                                        case_index,
                                        register: register as u8,
                                    });
                                }
                            }
                        }
                    }
                }
                ThumbOperation::StoreMultipleIncrementAfter {
                    base_register,
                    register_list,
                } => {
                    let encoding = instruction.encoding;
                    if base_register > 7 || register_list == 0 || register_list & !0x00FF != 0 {
                        return Err(VerifyError::InvalidThumbStmiaRegisterList(register_list));
                    }
                    let base_in_list = register_list & (1 << base_register) != 0;
                    let lower_registers = if base_register == 0 {
                        0
                    } else {
                        register_list & ((1 << base_register) - 1)
                    };
                    if base_in_list && lower_registers != 0 {
                        return Err(VerifyError::InvalidThumbStmiaRegisterList(register_list));
                    }
                    let encoded_base = ((encoding >> 8) & 0x7) as u8;
                    if encoding & 0xF800 != 0xC000
                        || base_register != encoded_base
                        || register_list != encoding & 0x00FF
                    {
                        return Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                            encoding,
                        )));
                    }
                }
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
        if let Some(table_id) = referenced_thumb_ldmia_tables
            .iter()
            .position(|referenced| !referenced)
        {
            return Err(VerifyError::UnreferencedThumbLdmiaRomTable(table_id));
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
                ArmOperation::Branch {
                    target,
                    link,
                    reachable_successors,
                } => {
                    if reachable_successors & 0b01 != 0 && !arm_pcs.contains(&target) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target,
                        });
                    }
                    if (reachable_successors & 0b10 != 0
                        || (link && reachable_successors & 0b01 != 0))
                        && !arm_pcs.contains(&fallthrough)
                    {
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
                ThumbOperation::LsrImmediate { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::OrrRegister { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::AndRegister { .. } => {
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
                ThumbOperation::StoreWordImmediate { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::StoreByteImmediate { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::LoadByteImmediate { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::LoadWordImmediate { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::LoadHalfwordImmediate { .. } => {
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
                ThumbOperation::Pop { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if fallthrough & 1 != 0 {
                        return Err(VerifyError::MisalignedThumbPc(fallthrough));
                    }
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::LoadMultipleIncrementAfter { .. }
                | ThumbOperation::LoadRomMultipleIncrementAfter { .. }
                | ThumbOperation::LoadRomMultipleIncrementAfterCases { .. }
                | ThumbOperation::StoreMultipleIncrementAfter { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if fallthrough & 1 != 0 {
                        return Err(VerifyError::MisalignedThumbPc(fallthrough));
                    }
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::MovImmediate { .. }
                | ThumbOperation::CompareImmediate { .. }
                | ThumbOperation::AddSubtractImmediate { .. } => {
                    let fallthrough = instruction.guest_pc.wrapping_add(2);
                    if !thumb_pcs.contains(&fallthrough) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target: fallthrough,
                        });
                    }
                }
                ThumbOperation::ConditionalBranch {
                    target,
                    reachable_successors,
                    ..
                } => {
                    for (edge_bit, successor) in
                        [(0b01, target), (0b10, instruction.guest_pc.wrapping_add(2))]
                    {
                        if reachable_successors & edge_bit != 0 && !thumb_pcs.contains(&successor) {
                            return Err(VerifyError::MissingSuccessor {
                                pc: instruction.guest_pc,
                                target: successor,
                            });
                        }
                    }
                }
                ThumbOperation::UnconditionalBranch { target } => {
                    if !thumb_pcs.contains(&target) {
                        return Err(VerifyError::MissingSuccessor {
                            pc: instruction.guest_pc,
                            target,
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
            ArmOperation::Branch { target, link, .. } => {
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
        ThumbOperation::LoadRomMultipleIncrementAfter {
            base_register,
            register_list,
            base_address,
            values_by_register,
        } => {
            if register_list & (1 << base_register) == 0 {
                state.r[base_register as usize] =
                    base_address.wrapping_add(register_list.count_ones().wrapping_mul(4));
            }
            for register in 0..8 {
                if register_list & (1 << register) != 0 {
                    state.r[register] = values_by_register[register];
                }
            }
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
        ThumbOperation::LsrImmediate { rd, rs, amount } => {
            let old_source = state.r[rs as usize];
            let (result, carry) = if amount == 0 {
                (0, old_source & 0x8000_0000 != 0)
            } else {
                (
                    old_source >> amount,
                    old_source & (1_u32 << (u32::from(amount) - 1)) != 0,
                )
            };
            state.r[rd as usize] = result;
            state.cpsr = (state.cpsr & !0xE000_0000)
                | (result & 0x8000_0000)
                | (u32::from(result == 0) << 30)
                | (u32::from(carry) << 29);
            state.r[REG_PC as usize] = instruction.guest_pc.wrapping_add(2);
        }
        ThumbOperation::OrrRegister { rd, rs } => {
            let lhs = state.r[rd as usize];
            let rhs = state.r[rs as usize];
            let result = lhs | rhs;
            state.r[rd as usize] = result;
            state.cpsr = (state.cpsr & !0xC000_0000)
                | (result & 0x8000_0000)
                | (u32::from(result == 0) << 30);
            state.r[REG_PC as usize] = instruction.guest_pc.wrapping_add(2);
        }
        ThumbOperation::AndRegister { rd, rs } => {
            let lhs = state.r[rd as usize];
            let rhs = state.r[rs as usize];
            let result = lhs & rhs;
            state.r[rd as usize] = result;
            state.cpsr = (state.cpsr & !0xC000_0000)
                | (result & 0x8000_0000)
                | (u32::from(result == 0) << 30);
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
        ThumbOperation::StoreWordImmediate { .. } => {
            return Err(StepFault::MemoryRuntimeRequired);
        }
        ThumbOperation::StoreByteImmediate { .. } => {
            return Err(StepFault::MemoryRuntimeRequired);
        }
        ThumbOperation::LoadByteImmediate { .. } => {
            return Err(StepFault::MemoryRuntimeRequired);
        }
        ThumbOperation::LoadWordImmediate { .. } => {
            return Err(StepFault::MemoryRuntimeRequired);
        }
        ThumbOperation::LoadHalfwordImmediate { .. } => {
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
        ThumbOperation::Push { .. }
        | ThumbOperation::Pop { .. }
        | ThumbOperation::LoadMultipleIncrementAfter { .. }
        | ThumbOperation::LoadRomMultipleIncrementAfterCases { .. }
        | ThumbOperation::StoreMultipleIncrementAfter { .. } => {
            return Err(StepFault::MemoryRuntimeRequired);
        }
        ThumbOperation::MovImmediate { rd, value } => {
            state.r[rd as usize] = u32::from(value);
            state.cpsr = (state.cpsr & !0xC000_0000) | (u32::from(value == 0) << 30);
            state.r[REG_PC as usize] = instruction.guest_pc.wrapping_add(2);
        }
        ThumbOperation::CompareImmediate { rd, immediate } => {
            let lhs = state.r[rd as usize];
            let rhs = u32::from(immediate);
            let result = lhs.wrapping_sub(rhs);
            let carry = lhs >= rhs;
            let overflow = ((lhs ^ rhs) & (lhs ^ result) & 0x8000_0000) != 0;
            state.cpsr = (state.cpsr & !0xF000_0000)
                | (result & 0x8000_0000)
                | (u32::from(result == 0) << 30)
                | (u32::from(carry) << 29)
                | (u32::from(overflow) << 28);
            state.r[REG_PC as usize] = instruction.guest_pc.wrapping_add(2);
        }
        ThumbOperation::ConditionalBranch {
            condition, target, ..
        } => {
            let fallthrough = instruction.guest_pc.wrapping_add(2);
            state.r[REG_PC as usize] = if condition_passes(condition, state.cpsr) {
                target
            } else {
                fallthrough
            };
        }
        ThumbOperation::UnconditionalBranch { target } => {
            state.r[REG_PC as usize] = target;
        }
        ThumbOperation::AddSubtractImmediate {
            rd,
            immediate,
            subtract,
        } => {
            let lhs = state.r[rd as usize];
            let rhs = u32::from(immediate);
            let (result, carry, overflow) = if subtract {
                let result = lhs.wrapping_sub(rhs);
                (
                    result,
                    lhs >= rhs,
                    ((lhs ^ rhs) & (lhs ^ result) & 0x8000_0000) != 0,
                )
            } else {
                let (result, carry) = lhs.overflowing_add(rhs);
                (
                    result,
                    carry,
                    (!(lhs ^ rhs) & (lhs ^ result) & 0x8000_0000) != 0,
                )
            };
            state.r[rd as usize] = result;
            state.cpsr = (state.cpsr & !0xF000_0000)
                | (result & 0x8000_0000)
                | (u32::from(result == 0) << 30)
                | (u32::from(carry) << 29)
                | (u32::from(overflow) << 28);
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

    fn conditional_branch_program(condition: u8) -> ArmProgram {
        let guest_pc = 0x0800_0100;
        ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![
                ThumbInstruction {
                    guest_pc,
                    encoding: 0xD000 | (u16::from(condition) << 8),
                    operation: ThumbOperation::ConditionalBranch {
                        condition,
                        target: guest_pc.wrapping_add(4),
                        reachable_successors: 0b11,
                    },
                },
                ThumbInstruction {
                    guest_pc: guest_pc + 2,
                    encoding: 0x4700,
                    operation: ThumbOperation::BranchExchange { rm: 0 },
                },
                ThumbInstruction {
                    guest_pc: guest_pc + 4,
                    encoding: 0x4700,
                    operation: ThumbOperation::BranchExchange { rm: 0 },
                },
            ],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![
                StaticBranchTarget {
                    source_mode: IsaMode::Thumb,
                    source_pc: guest_pc + 2,
                    target_mode: IsaMode::Thumb,
                    target_pc: guest_pc + 4,
                },
                StaticBranchTarget {
                    source_mode: IsaMode::Thumb,
                    source_pc: guest_pc + 4,
                    target_mode: IsaMode::Thumb,
                    target_pc: guest_pc + 2,
                },
            ],
        }
    }

    fn unconditional_branch_program(encoding: u16, target: u32) -> ArmProgram {
        ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![ThumbInstruction {
                guest_pc: 0x0800_0100,
                encoding,
                operation: ThumbOperation::UnconditionalBranch { target },
            }],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![],
        }
    }

    fn thumb_pop_program(guest_pc: u32, encoding: u16, register_list: u16) -> ArmProgram {
        let fallthrough = guest_pc.wrapping_add(2);
        ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![
                ThumbInstruction {
                    guest_pc,
                    encoding,
                    operation: ThumbOperation::Pop { register_list },
                },
                ThumbInstruction {
                    guest_pc: fallthrough,
                    encoding: 0x4700,
                    operation: ThumbOperation::BranchExchange { rm: 0 },
                },
            ],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![StaticBranchTarget {
                source_mode: IsaMode::Thumb,
                source_pc: fallthrough,
                target_mode: IsaMode::Thumb,
                target_pc: fallthrough,
            }],
        }
    }

    fn thumb_ldmia_program(guest_pc: u32, encoding: u16, operation: ThumbOperation) -> ArmProgram {
        let fallthrough = guest_pc.wrapping_add(2);
        ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![
                ThumbInstruction {
                    guest_pc,
                    encoding,
                    operation,
                },
                ThumbInstruction {
                    guest_pc: fallthrough,
                    encoding: 0x4700,
                    operation: ThumbOperation::BranchExchange { rm: 0 },
                },
            ],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![StaticBranchTarget {
                source_mode: IsaMode::Thumb,
                source_pc: fallthrough,
                target_mode: IsaMode::Thumb,
                target_pc: fallthrough,
            }],
        }
    }

    fn expected_thumb_condition(condition: u8, cpsr: u32) -> bool {
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
            _ => false,
        }
    }

    #[test]
    fn thumb_conditional_branch_matches_all_conditions_and_nzcv_combinations() {
        let guest_pc = 0x0800_0100;
        let target = guest_pc + 4;
        for condition in 0..=0xD {
            let instruction = ThumbInstruction {
                guest_pc,
                encoding: 0xD000 | (u16::from(condition) << 8),
                operation: ThumbOperation::ConditionalBranch {
                    condition,
                    target,
                    reachable_successors: 0b11,
                },
            };
            for nzcv in 0..16 {
                let mut cpu = state(guest_pc);
                cpu.cpsr = (cpu.cpsr & !0xF000_0000) | (nzcv << 28) | 0x20;
                cpu.r[..15]
                    .iter_mut()
                    .enumerate()
                    .for_each(|(index, register)| *register = 0x1234_0000 + index as u32);
                cpu.r[15] = guest_pc;
                let registers_before = cpu.r;
                let cpsr_before = cpu.cpsr;
                let expected_target = if expected_thumb_condition(condition, cpu.cpsr) {
                    target
                } else {
                    guest_pc + 2
                };

                execute_thumb_instruction(&mut cpu, instruction).unwrap();

                assert_eq!(
                    cpu.r[15], expected_target,
                    "condition={condition}, NZCV={nzcv:04b}"
                );
                assert_eq!(&cpu.r[..15], &registers_before[..15]);
                assert_eq!(cpu.cpsr, cpsr_before);
                assert_eq!(cpu.guest_cycle, 6);
            }
        }
    }

    #[test]
    fn thumb_unconditional_branch_updates_only_pc_and_cycle() {
        let guest_pc = 0x0800_0100;
        let target = guest_pc + 6;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xE001,
            operation: ThumbOperation::UnconditionalBranch { target },
        };
        let mut cpu = state(guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[..15]
            .iter_mut()
            .enumerate()
            .for_each(|(index, register)| *register = 0x1234_0000 + index as u32);
        cpu.r[15] = guest_pc;
        let registers_before = cpu.r;
        let cpsr_before = cpu.cpsr;

        execute_thumb_instruction(&mut cpu, instruction).unwrap();

        assert_eq!(cpu.r[15], target);
        assert_eq!(&cpu.r[..15], &registers_before[..15]);
        assert_eq!(cpu.cpsr, cpsr_before);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn arm_program_verifier_closes_only_thumb_unconditional_branch_target() {
        let guest_pc = 0x0800_0100;
        let self_loop = unconditional_branch_program(0xE7FE, guest_pc);
        assert!(self_loop.verify().is_ok());

        let mismatched_target = unconditional_branch_program(0xE7FE, guest_pc + 2);
        assert_eq!(
            mismatched_target.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0xE7FE))
        );

        let invalid_encoding = unconditional_branch_program(0xD000, guest_pc + 4);
        assert_eq!(
            invalid_encoding.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0xD000))
        );

        let missing_target = unconditional_branch_program(0xE000, guest_pc + 4);
        assert_eq!(
            missing_target.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: guest_pc,
                target: guest_pc + 4,
            })
        );
    }

    #[test]
    fn arm_program_verifier_closes_thumb_conditional_branch_successors() {
        for condition in 0..=0xD {
            assert!(conditional_branch_program(condition).verify().is_ok());
        }

        let mut mismatched_condition = conditional_branch_program(0);
        mismatched_condition.thumb_instructions[0].encoding |= 1 << 8;
        assert_eq!(
            mismatched_condition.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0xD100))
        );

        let mut mismatched_target = conditional_branch_program(0);
        mismatched_target.thumb_instructions[0].operation = ThumbOperation::ConditionalBranch {
            condition: 0,
            target: 0x0800_0106,
            reachable_successors: 0b11,
        };
        assert_eq!(
            mismatched_target.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0xD000))
        );

        let mut invalid_condition = conditional_branch_program(0);
        invalid_condition.thumb_instructions[0].operation = ThumbOperation::ConditionalBranch {
            condition: 0xE,
            target: 0x0800_0104,
            reachable_successors: 0b11,
        };
        assert_eq!(
            invalid_condition.verify(),
            Err(VerifyError::InvalidThumbBranchCondition(0xE))
        );

        let mut missing_target = conditional_branch_program(0);
        missing_target
            .thumb_instructions
            .retain(|instruction| instruction.guest_pc != 0x0800_0104);
        missing_target.branch_targets.clear();
        assert_eq!(
            missing_target.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: 0x0800_0100,
                target: 0x0800_0104,
            })
        );

        let mut missing_fallthrough = conditional_branch_program(0);
        missing_fallthrough
            .thumb_instructions
            .retain(|instruction| instruction.guest_pc != 0x0800_0102);
        missing_fallthrough.branch_targets.clear();
        assert_eq!(
            missing_fallthrough.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: 0x0800_0100,
                target: 0x0800_0102,
            })
        );

        let mut invalid_empty_mask = conditional_branch_program(0);
        let ThumbOperation::ConditionalBranch {
            condition, target, ..
        } = invalid_empty_mask.thumb_instructions[0].operation
        else {
            unreachable!();
        };
        invalid_empty_mask.thumb_instructions[0].operation = ThumbOperation::ConditionalBranch {
            condition,
            target,
            reachable_successors: 0,
        };
        assert_eq!(
            invalid_empty_mask.verify(),
            Err(VerifyError::InvalidThumbBranchSuccessorMask(0))
        );

        let mut invalid_reserved_mask = conditional_branch_program(0);
        let ThumbOperation::ConditionalBranch {
            condition, target, ..
        } = invalid_reserved_mask.thumb_instructions[0].operation
        else {
            unreachable!();
        };
        invalid_reserved_mask.thumb_instructions[0].operation = ThumbOperation::ConditionalBranch {
            condition,
            target,
            reachable_successors: 0b100,
        };
        assert_eq!(
            invalid_reserved_mask.verify(),
            Err(VerifyError::InvalidThumbBranchSuccessorMask(0b100))
        );

        let mut omitted_unmarked_fallthrough = conditional_branch_program(0);
        let ThumbOperation::ConditionalBranch {
            condition, target, ..
        } = omitted_unmarked_fallthrough.thumb_instructions[0].operation
        else {
            unreachable!();
        };
        omitted_unmarked_fallthrough.thumb_instructions[0].operation =
            ThumbOperation::ConditionalBranch {
                condition,
                target,
                reachable_successors: 0b01,
            };
        omitted_unmarked_fallthrough
            .thumb_instructions
            .retain(|instruction| instruction.guest_pc != 0x0800_0102);
        omitted_unmarked_fallthrough.branch_targets = vec![StaticBranchTarget {
            source_mode: IsaMode::Thumb,
            source_pc: 0x0800_0104,
            target_mode: IsaMode::Thumb,
            target_pc: 0x0800_0104,
        }];
        assert!(omitted_unmarked_fallthrough.verify().is_ok());
    }

    fn thumb_rom_ldmia_cases_program() -> ArmProgram {
        let mut program = thumb_ldmia_program(
            0x0800_0200,
            0xCB03,
            ThumbOperation::LoadRomMultipleIncrementAfterCases {
                base_register: 3,
                register_list: 0x03,
                table_id: 0,
            },
        );
        program.thumb_ldmia_rom_tables = vec![ThumbLdmiaRomTable {
            cases: vec![
                ThumbLdmiaRomCase {
                    base_address: 0x0800_0100,
                    values_by_register: [0x1234_5678, 0x2345_6789, 0, 0, 0, 0, 0, 0],
                },
                ThumbLdmiaRomCase {
                    base_address: 0x0800_0120,
                    values_by_register: [0x3456_789A, 0x4567_89AB, 0, 0, 0, 0, 0, 0],
                },
            ],
        }];
        program
    }

    #[test]
    fn arm_program_verifier_checks_thumb_rom_ldmia_tables() {
        assert!(thumb_rom_ldmia_cases_program().verify().is_ok());

        let mut too_few_cases = thumb_rom_ldmia_cases_program();
        too_few_cases.thumb_ldmia_rom_tables[0].cases.pop();
        assert_eq!(
            too_few_cases.verify(),
            Err(VerifyError::InvalidThumbLdmiaRomTableCaseCount(1))
        );

        let mut empty_table = thumb_rom_ldmia_cases_program();
        empty_table.thumb_ldmia_rom_tables[0].cases.clear();
        assert_eq!(
            empty_table.verify(),
            Err(VerifyError::InvalidThumbLdmiaRomTableCaseCount(0))
        );

        let mut too_many_cases = thumb_rom_ldmia_cases_program();
        let repeated_case = too_many_cases.thumb_ldmia_rom_tables[0].cases[0];
        for _ in 0..15 {
            too_many_cases.thumb_ldmia_rom_tables[0]
                .cases
                .push(repeated_case);
        }
        assert_eq!(
            too_many_cases.verify(),
            Err(VerifyError::InvalidThumbLdmiaRomTableCaseCount(17))
        );

        let mut unordered_bases = thumb_rom_ldmia_cases_program();
        unordered_bases.thumb_ldmia_rom_tables[0].cases[1].base_address = 0x0800_0100;
        assert_eq!(
            unordered_bases.verify(),
            Err(VerifyError::UnorderedThumbLdmiaRomTable {
                table_id: 0,
                case_index: 1,
            })
        );

        let mut non_rom_base = thumb_rom_ldmia_cases_program();
        non_rom_base.thumb_ldmia_rom_tables[0].cases[0].base_address = 0x0200_0000;
        assert_eq!(
            non_rom_base.verify(),
            Err(VerifyError::InvalidThumbLdmiaRomTableBase {
                table_id: 0,
                case_index: 0,
                base_address: 0x0200_0000,
            })
        );

        let mut nonzero_unselected_word = thumb_rom_ldmia_cases_program();
        nonzero_unselected_word.thumb_ldmia_rom_tables[0].cases[0].values_by_register[2] =
            0xDEAD_BEEF;
        assert_eq!(
            nonzero_unselected_word.verify(),
            Err(VerifyError::NonzeroUnselectedThumbLdmiaRomValue {
                table_id: 0,
                case_index: 0,
                register: 2,
            })
        );

        let mut invalid_table_id = thumb_rom_ldmia_cases_program();
        invalid_table_id.thumb_instructions[0].operation =
            ThumbOperation::LoadRomMultipleIncrementAfterCases {
                base_register: 3,
                register_list: 0x03,
                table_id: 1,
            };
        assert_eq!(
            invalid_table_id.verify(),
            Err(VerifyError::InvalidThumbLdmiaRomTableId(1))
        );

        let mut unreferenced_table = thumb_rom_ldmia_cases_program();
        unreferenced_table.thumb_instructions[0].operation =
            ThumbOperation::LoadMultipleIncrementAfter {
                base_register: 3,
                register_list: 0x03,
            };
        assert_eq!(
            unreferenced_table.verify(),
            Err(VerifyError::UnreferencedThumbLdmiaRomTable(0))
        );
    }

    #[test]
    fn arm_program_verifier_checks_thumb_pop_encoding_list_and_successor() {
        assert!(
            thumb_pop_program(0x0800_0100, 0xBC08, 0x08)
                .verify()
                .is_ok()
        );
        assert!(
            thumb_pop_program(0x0800_0100, 0xBC01, 0x01)
                .verify()
                .is_ok()
        );
        assert!(
            thumb_pop_program(0x0800_0100, 0xBCFF, 0xFF)
                .verify()
                .is_ok()
        );
        assert!(
            thumb_pop_program(u32::MAX - 1, 0xBC08, 0x08)
                .verify()
                .is_ok()
        );

        let empty_list = thumb_pop_program(0x0800_0100, 0xBC08, 0);
        assert_eq!(
            empty_list.verify(),
            Err(VerifyError::InvalidThumbPopRegisterList(0))
        );

        let high_list_bit = thumb_pop_program(0x0800_0100, 0xBC08, 0x0108);
        assert_eq!(
            high_list_bit.verify(),
            Err(VerifyError::InvalidThumbPopRegisterList(0x0108))
        );

        let mismatched_list = thumb_pop_program(0x0800_0100, 0xBC08, 0x10);
        assert_eq!(
            mismatched_list.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0xBC08))
        );

        let pc_pop = thumb_pop_program(0x0800_0100, 0xBD08, 0x08);
        assert_eq!(
            pc_pop.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0xBD08))
        );

        let wrong_format = thumb_pop_program(0x0800_0100, 0xBE08, 0x08);
        assert_eq!(
            wrong_format.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0xBE08))
        );

        let misaligned_pc = thumb_pop_program(0x0800_0101, 0xBC08, 0x08);
        assert_eq!(
            misaligned_pc.verify(),
            Err(VerifyError::MisalignedThumbPc(0x0800_0101))
        );

        let mut missing_successor = thumb_pop_program(0x0800_0100, 0xBC08, 0x08);
        missing_successor.thumb_instructions.pop();
        missing_successor.branch_targets.clear();
        assert_eq!(
            missing_successor.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: 0x0800_0100,
                target: 0x0800_0102,
            })
        );
    }

    #[test]
    fn arm_program_verifier_checks_thumb_ldmia_fields_and_successor() {
        let guest_pc = 0x0800_0100;
        let mut program = thumb_ldmia_program(
            guest_pc,
            0xCB01,
            ThumbOperation::LoadMultipleIncrementAfter {
                base_register: 3,
                register_list: 1,
            },
        );
        assert!(program.verify().is_ok());

        program.thumb_instructions[0].operation = ThumbOperation::LoadMultipleIncrementAfter {
            base_register: 2,
            register_list: 1,
        };
        assert_eq!(
            program.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0xCB01))
        );

        let empty = thumb_ldmia_program(
            guest_pc,
            0xCB00,
            ThumbOperation::LoadMultipleIncrementAfter {
                base_register: 3,
                register_list: 0,
            },
        );
        assert_eq!(
            empty.verify(),
            Err(VerifyError::InvalidThumbLdmiaRegisterList(0))
        );

        let bad_register = thumb_ldmia_program(
            guest_pc,
            0xCB01,
            ThumbOperation::LoadMultipleIncrementAfter {
                base_register: 8,
                register_list: 1,
            },
        );
        assert!(matches!(
            bad_register.verify(),
            Err(VerifyError::InvalidThumbLdmiaRegisterList(_))
        ));

        let mut missing_successor = thumb_ldmia_program(
            guest_pc,
            0xCB01,
            ThumbOperation::LoadMultipleIncrementAfter {
                base_register: 3,
                register_list: 1,
            },
        );
        missing_successor.thumb_instructions.pop();
        missing_successor.branch_targets.clear();
        assert_eq!(
            missing_successor.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: guest_pc,
                target: guest_pc + 2,
            })
        );
    }

    #[test]
    fn arm_program_verifier_checks_thumb_stmia_fields_and_successor() {
        let guest_pc = 0x0800_0100;
        let mut program = thumb_ldmia_program(
            guest_pc,
            0xC201,
            ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 1,
            },
        );
        assert!(program.verify().is_ok());

        let base_is_first = thumb_ldmia_program(
            guest_pc,
            0xC224,
            ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 0x24,
            },
        );
        assert!(base_is_first.verify().is_ok());

        program.thumb_instructions[0].operation = ThumbOperation::StoreMultipleIncrementAfter {
            base_register: 3,
            register_list: 1,
        };
        assert_eq!(
            program.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0xC201))
        );

        let empty = thumb_ldmia_program(
            guest_pc,
            0xC200,
            ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 0,
            },
        );
        assert_eq!(
            empty.verify(),
            Err(VerifyError::InvalidThumbStmiaRegisterList(0))
        );

        let unpredictable = thumb_ldmia_program(
            guest_pc,
            0xC205,
            ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 5,
            },
        );
        assert_eq!(
            unpredictable.verify(),
            Err(VerifyError::InvalidThumbStmiaRegisterList(5))
        );

        let mut missing_successor = thumb_ldmia_program(
            guest_pc,
            0xC201,
            ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 1,
            },
        );
        missing_successor.thumb_instructions.pop();
        missing_successor.branch_targets.clear();
        assert_eq!(
            missing_successor.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: guest_pc,
                target: guest_pc + 2,
            })
        );
    }

    #[test]
    fn resolved_thumb_rom_ldmia_loads_values_and_obeys_base_overlap() {
        let guest_pc = 0x0800_0100;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xCB09, // LDMIA r3!, {r0,r3}.
            operation: ThumbOperation::LoadRomMultipleIncrementAfter {
                base_register: 3,
                register_list: 0x09,
                base_address: 0x0800_00F0,
                values_by_register: [0x0800_0089, 0, 0, 0xAABB_CCDD, 0, 0, 0, 0],
            },
        };
        let mut cpu = state(guest_pc);
        cpu.cpsr = 0xA000_003F;
        cpu.r[3] = 0x0800_00F0;
        let original_cpsr = cpu.cpsr;
        let original_cycle = cpu.guest_cycle;
        execute_thumb_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[0], 0x0800_0089);
        assert_eq!(cpu.r[3], 0xAABB_CCDD);
        assert_eq!(cpu.r[15], guest_pc + 2);
        assert_eq!(cpu.cpsr, original_cpsr);
        assert_eq!(cpu.guest_cycle, original_cycle + 1);

        let mut no_base_in_list = instruction;
        no_base_in_list.encoding = 0xCB01;
        no_base_in_list.operation = ThumbOperation::LoadRomMultipleIncrementAfter {
            base_register: 3,
            register_list: 1,
            base_address: 0x0800_00F1,
            values_by_register: [0x1234_5678, 0, 0, 0, 0, 0, 0, 0],
        };
        let mut cpu = state(guest_pc);
        cpu.cpsr |= 0x20;
        execute_thumb_instruction(&mut cpu, no_base_in_list).unwrap();
        assert_eq!(cpu.r[0], 0x1234_5678);
        assert_eq!(cpu.r[3], 0x0800_00F5);
        assert_eq!(cpu.r[15], guest_pc + 2);
        assert_eq!(cpu.guest_cycle, 6);
    }

    #[test]
    fn arm_program_verifier_accepts_leafgreen_thumb_le_branch_encoding() {
        let guest_pc = 0x0800_09A6;
        let program = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![
                ThumbInstruction {
                    guest_pc,
                    encoding: 0xDDF6,
                    operation: ThumbOperation::ConditionalBranch {
                        condition: 0xD,
                        target: 0x0800_0996,
                        reachable_successors: 0b11,
                    },
                },
                ThumbInstruction {
                    guest_pc: 0x0800_0996,
                    encoding: 0x4700,
                    operation: ThumbOperation::BranchExchange { rm: 0 },
                },
                ThumbInstruction {
                    guest_pc: 0x0800_09A8,
                    encoding: 0x4700,
                    operation: ThumbOperation::BranchExchange { rm: 0 },
                },
            ],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![
                StaticBranchTarget {
                    source_mode: IsaMode::Thumb,
                    source_pc: 0x0800_0996,
                    target_mode: IsaMode::Thumb,
                    target_pc: 0x0800_09A8,
                },
                StaticBranchTarget {
                    source_mode: IsaMode::Thumb,
                    source_pc: 0x0800_09A8,
                    target_mode: IsaMode::Thumb,
                    target_pc: 0x0800_0996,
                },
            ],
        };
        assert!(program.verify().is_ok());
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
                reachable_successors: 0b11,
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
    fn thumb_stmia_requires_memory_runtime_without_mutating_cpu_state() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0xC201,
            operation: ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 1,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[0] = 0xAABB_CCDD;
        cpu.r[2] = 0x0300_0020;
        let before = cpu;
        assert_eq!(
            execute_thumb_instruction(&mut cpu, instruction),
            Err(StepFault::MemoryRuntimeRequired)
        );
        assert_eq!(cpu, before);
    }

    #[test]
    fn thumb_pop_requires_memory_runtime_without_mutating_cpu_state() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0xBC08,
            operation: ThumbOperation::Pop {
                register_list: 0x08,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[3] = 0x1234_5678;
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
    fn thumb_word_store_requires_memory_runtime_without_mutating_cpu_state() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0x67C9,
            operation: ThumbOperation::StoreWordImmediate {
                rd: 1,
                rb: 1,
                imm5: 31,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[1] = 0x0200_0000;
        let before = cpu;
        assert_eq!(
            execute_thumb_instruction(&mut cpu, instruction),
            Err(StepFault::MemoryRuntimeRequired)
        );
        assert_eq!(cpu, before);
    }

    #[test]
    fn thumb_byte_store_requires_memory_runtime_without_mutating_cpu_state() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0x77C5,
            operation: ThumbOperation::StoreByteImmediate {
                rd: 5,
                rb: 0,
                imm5: 31,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[5] = 0xAABB_CCDD;
        cpu.r[0] = 0x0200_0001;
        let before = cpu;
        assert_eq!(
            execute_thumb_instruction(&mut cpu, instruction),
            Err(StepFault::MemoryRuntimeRequired)
        );
        assert_eq!(cpu, before);
    }

    #[test]
    fn thumb_byte_load_requires_memory_runtime_without_mutating_cpu_state() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0x7809,
            operation: ThumbOperation::LoadByteImmediate {
                rd: 1,
                rb: 1,
                imm5: 0,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[1] = 0x0200_0001;
        let before = cpu;
        assert_eq!(
            execute_thumb_instruction(&mut cpu, instruction),
            Err(StepFault::MemoryRuntimeRequired)
        );
        assert_eq!(cpu, before);
    }

    #[test]
    fn thumb_word_load_requires_memory_runtime_without_mutating_cpu_state() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0x6880,
            operation: ThumbOperation::LoadWordImmediate {
                rd: 0,
                rb: 0,
                imm5: 2,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[0] = 0x0400_00D4;
        let before = cpu;
        assert_eq!(
            execute_thumb_instruction(&mut cpu, instruction),
            Err(StepFault::MemoryRuntimeRequired)
        );
        assert_eq!(cpu, before);
    }

    #[test]
    fn thumb_halfword_load_requires_memory_runtime_without_mutating_cpu_state() {
        let instruction = ThumbInstruction {
            guest_pc: 0x0800_0100,
            encoding: 0x8821,
            operation: ThumbOperation::LoadHalfwordImmediate {
                rd: 1,
                rb: 4,
                imm5: 0,
            },
        };
        let mut cpu = state(instruction.guest_pc);
        cpu.cpsr |= 0x20;
        cpu.r[1] = 0xAABB_CCDD;
        cpu.r[4] = 0x0200_0001;
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
    fn thumb_cmp_immediate_sets_nzcv_without_writing_registers() {
        let guest_pc = 0x0800_0100;
        let rd = 2;
        let cases = [
            (5, 5, 0x6000_0000),
            (0, 1, 0x8000_0000),
            (0x80, 0xFF, 0x8000_0000),
            (0x8000_0000, 1, 0x3000_0000),
        ];
        for (lhs, immediate, nzcv) in cases {
            let instruction = ThumbInstruction {
                guest_pc,
                encoding: 0x2800 | (u16::from(rd) << 8) | immediate as u16,
                operation: ThumbOperation::CompareImmediate {
                    rd,
                    immediate: immediate as u8,
                },
            };
            let mut cpu = state(guest_pc);
            cpu.cpsr = 0xA5A5_5A7F;
            cpu.r[rd as usize] = lhs;
            cpu.r[3] = 0x1234_5678;
            let old_registers = cpu.r;
            let old_non_flag_cpsr = cpu.cpsr & !0xF000_0000;

            execute_thumb_instruction(&mut cpu, instruction).unwrap();

            let mut expected_registers = old_registers;
            expected_registers[REG_PC as usize] = guest_pc + 2;
            assert_eq!(cpu.r, expected_registers);
            assert_eq!(cpu.cpsr, old_non_flag_cpsr | nzcv);
            assert_eq!(cpu.guest_cycle, 6);
        }
    }

    #[test]
    fn thumb_adds_subs_immediate_set_nzcv_and_write_wrapping_result() {
        let guest_pc = 0x0800_0100;
        let rd = 2;
        let cases = [
            (false, u32::MAX, 1, 0, 0x6000_0000),
            (false, 0x7FFF_FFFF, 1, 0x8000_0000, 0x9000_0000),
            (false, 0x7FFF_FFF0, 0xFF, 0x8000_00EF, 0x9000_0000),
            (true, 0, 1, u32::MAX, 0x8000_0000),
            (true, 0x8000_0000, 1, 0x7FFF_FFFF, 0x3000_0000),
        ];
        for (subtract, lhs, immediate, expected_result, nzcv) in cases {
            let opcode = if subtract { 0x3800 } else { 0x3000 };
            let instruction = ThumbInstruction {
                guest_pc,
                encoding: opcode | (u16::from(rd) << 8) | immediate as u16,
                operation: ThumbOperation::AddSubtractImmediate {
                    rd,
                    immediate: immediate as u8,
                    subtract,
                },
            };
            let mut cpu = state(guest_pc);
            cpu.cpsr = 0xA5A5_5A7F;
            cpu.r[rd as usize] = lhs;
            cpu.r[3] = 0x1234_5678;
            let mut expected_registers = cpu.r;
            expected_registers[rd as usize] = expected_result;
            expected_registers[REG_PC as usize] = guest_pc + 2;
            let old_non_flag_cpsr = cpu.cpsr & !0xF000_0000;

            execute_thumb_instruction(&mut cpu, instruction).unwrap();

            assert_eq!(cpu.r, expected_registers);
            assert_eq!(cpu.cpsr, old_non_flag_cpsr | nzcv);
            assert_eq!(cpu.guest_cycle, 6);
        }
    }

    #[test]
    fn thumb_format3_operations_reject_arm_mode_atomically() {
        let guest_pc = 0x0800_0100;
        let cases = [
            ThumbInstruction {
                guest_pc,
                encoding: 0x20FF,
                operation: ThumbOperation::MovImmediate { rd: 0, value: 0xFF },
            },
            ThumbInstruction {
                guest_pc,
                encoding: 0x28FF,
                operation: ThumbOperation::CompareImmediate {
                    rd: 0,
                    immediate: 0xFF,
                },
            },
            ThumbInstruction {
                guest_pc,
                encoding: 0x30FF,
                operation: ThumbOperation::AddSubtractImmediate {
                    rd: 0,
                    immediate: 0xFF,
                    subtract: false,
                },
            },
            ThumbInstruction {
                guest_pc,
                encoding: 0x38FF,
                operation: ThumbOperation::AddSubtractImmediate {
                    rd: 0,
                    immediate: 0xFF,
                    subtract: true,
                },
            },
        ];
        for instruction in cases {
            let mut cpu = state(guest_pc);
            cpu.cpsr = 0xA000_001F;
            cpu.r[0] = 0x8000_0000;
            let before = cpu;
            assert_eq!(
                execute_thumb_instruction(&mut cpu, instruction),
                Err(StepFault::IsaModeMismatch {
                    expected: IsaMode::Thumb,
                    actual: IsaMode::Arm,
                })
            );
            assert_eq!(cpu, before);
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
    fn thumb_lsr_immediate_updates_result_flags_and_preserves_v_control_and_alias() {
        let guest_pc = 0x0800_0100;
        let cases = [
            // Immediate zero means shift by 32; carry comes from bit 31.
            (0x8000_0001, 0, 0x1ABC_10BF, 0, 1, 0x7ABC_10BF, 0, 1),
            // Shift by one; carry comes from bit 0 and V remains clear.
            (
                0x8000_0003,
                1,
                0x0ABC_20BF,
                0x4000_0001,
                1,
                0x2ABC_20BF,
                2,
                1,
            ),
            // Shift by sixteen; bit 15 becomes carry and V remains set.
            (0x0001_8000, 16, 0x1ABC_30BF, 1, 1, 0x3ABC_30BF, 2, 1),
            // Shift by thirty-one; bit 30 is clear, so carry is cleared.
            (0x8000_0000, 31, 0x0ABC_40BF, 1, 0, 0x0ABC_40BF, 2, 1),
            // Rd == Rs must read the old value before overwriting the register.
            (
                0x8001_8000,
                16,
                0x1ABC_50BF,
                0x0000_8001,
                1,
                0x3ABC_50BF,
                2,
                2,
            ),
        ];

        for (source, amount, cpsr, expected_result, carry, expected_cpsr, rd, rs) in cases {
            let encoding = 0x0800 | (u16::from(amount) << 6) | (u16::from(rs) << 3) | u16::from(rd);
            let instruction = ThumbInstruction {
                guest_pc,
                encoding,
                operation: ThumbOperation::LsrImmediate { rd, rs, amount },
            };
            let mut cpu = state(guest_pc);
            cpu.cpsr = cpsr;
            cpu.r[rs as usize] = source;
            cpu.r[3] = 0xAABB_CCDD;
            let mut expected_registers = cpu.r;
            expected_registers[rd as usize] = expected_result;
            expected_registers[15] = guest_pc + 2;

            execute_thumb_instruction(&mut cpu, instruction).unwrap();

            assert_eq!(cpu.r, expected_registers);
            assert_eq!(cpu.r[rd as usize], expected_result);
            assert_eq!(cpu.cpsr, expected_cpsr);
            assert_eq!(cpu.cpsr & 0x2000_0000 != 0, carry != 0);
            assert_eq!(cpu.cpsr & 0x1000_0000, cpsr & 0x1000_0000);
            assert_eq!(cpu.cpsr & !0xF000_0000, cpsr & !0xF000_0000);
            assert_eq!(cpu.r[15], guest_pc + 2);
            assert_eq!(cpu.guest_cycle, 6);
        }
    }

    #[test]
    fn thumb_orr_register_sets_nz_and_preserves_cv_and_other_cpsr_bits() {
        let guest_pc = 0x0800_0100;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x4318,
            operation: ThumbOperation::OrrRegister { rd: 0, rs: 3 },
        };
        let cases = [
            (0, 0, 0xF123_003F, 0, 0x7123_003F),
            (
                0x0100_0000,
                0x0000_0010,
                0xDACE_00BF,
                0x0100_0010,
                0x1ACE_00BF,
            ),
            (0x8000_0000, 1, 0x5123_0125, 0x8000_0001, 0x9123_0125),
        ];
        for (lhs, rhs, cpsr, expected_result, expected_cpsr) in cases {
            let mut cpu = state(guest_pc);
            cpu.cpsr = cpsr;
            cpu.r[0] = lhs;
            cpu.r[3] = rhs;
            cpu.r[1] = 0xAABB_CCDD;
            let before = cpu.r;

            execute_thumb_instruction(&mut cpu, instruction).unwrap();

            assert_eq!(cpu.r[0], expected_result);
            assert_eq!(cpu.r[3], rhs);
            assert_eq!(cpu.r[1], before[1]);
            assert_eq!(cpu.r[15], guest_pc + 2);
            assert_eq!(cpu.cpsr, expected_cpsr);
            assert_eq!(cpu.guest_cycle, 6);
        }
    }

    #[test]
    fn thumb_orr_register_alias_uses_the_old_value_and_requires_thumb_mode() {
        let guest_pc = 0x0800_0100;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x4312,
            operation: ThumbOperation::OrrRegister { rd: 2, rs: 2 },
        };
        let mut cpu = state(guest_pc);
        cpu.cpsr = 0xB000_003F;
        cpu.r[2] = 0x8000_1234;
        execute_thumb_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[2], 0x8000_1234);
        assert_eq!(cpu.cpsr, 0xB000_003F);
        assert_eq!(cpu.r[15], guest_pc + 2);
        assert_eq!(cpu.guest_cycle, 6);

        let mut arm_state = state(guest_pc);
        arm_state.r[2] = 0xA5A5_5A5A;
        let before = arm_state;
        assert_eq!(
            execute_thumb_instruction(&mut arm_state, instruction),
            Err(StepFault::IsaModeMismatch {
                expected: IsaMode::Thumb,
                actual: IsaMode::Arm,
            })
        );
        assert_eq!(arm_state, before);
    }

    #[test]
    fn thumb_and_register_sets_nz_and_preserves_cv_and_other_cpsr_bits() {
        let guest_pc = 0x0800_0100;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x4018,
            operation: ThumbOperation::AndRegister { rd: 0, rs: 3 },
        };
        let cases = [
            (0, 0xFFFF_FFFF, 0xF123_003F, 0, 0x7123_003F),
            (
                0x0100_0010,
                0x0100_0000,
                0xDACE_00BF,
                0x0100_0000,
                0x1ACE_00BF,
            ),
            (
                0x8000_0001,
                0x8000_0000,
                0x5123_0125,
                0x8000_0000,
                0x9123_0125,
            ),
        ];
        for (lhs, rhs, cpsr, expected_result, expected_cpsr) in cases {
            let mut cpu = state(guest_pc);
            cpu.cpsr = cpsr;
            cpu.r[0] = lhs;
            cpu.r[3] = rhs;
            cpu.r[1] = 0xAABB_CCDD;
            let before = cpu.r;

            execute_thumb_instruction(&mut cpu, instruction).unwrap();

            assert_eq!(cpu.r[0], expected_result);
            assert_eq!(cpu.r[3], rhs);
            assert_eq!(cpu.r[1], before[1]);
            assert_eq!(cpu.r[15], guest_pc + 2);
            assert_eq!(cpu.cpsr, expected_cpsr);
            assert_eq!(cpu.guest_cycle, 6);
        }
    }

    #[test]
    fn thumb_and_register_alias_uses_the_old_value_and_requires_thumb_mode() {
        let guest_pc = 0x0800_0100;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x4012,
            operation: ThumbOperation::AndRegister { rd: 2, rs: 2 },
        };
        let mut cpu = state(guest_pc);
        cpu.cpsr = 0xB000_003F;
        cpu.r[2] = 0x8000_1234;
        execute_thumb_instruction(&mut cpu, instruction).unwrap();
        assert_eq!(cpu.r[2], 0x8000_1234);
        assert_eq!(cpu.cpsr, 0xB000_003F);
        assert_eq!(cpu.r[15], guest_pc + 2);
        assert_eq!(cpu.guest_cycle, 6);

        let mut arm_state = state(guest_pc);
        arm_state.r[2] = 0xA5A5_5A5A;
        let before = arm_state;
        assert_eq!(
            execute_thumb_instruction(&mut arm_state, instruction),
            Err(StepFault::IsaModeMismatch {
                expected: IsaMode::Thumb,
                actual: IsaMode::Arm,
            })
        );
        assert_eq!(arm_state, before);
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
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![],
        };
        assert_eq!(
            program.verify(),
            Err(VerifyError::InvalidThumbImmediateRegister(8))
        );
    }

    #[test]
    fn arm_program_verifier_checks_thumb_format3_variants_and_fields() {
        let guest_pc = 0x0800_0100;
        let cases = [
            ThumbInstruction {
                guest_pc,
                encoding: 0x20FF,
                operation: ThumbOperation::MovImmediate { rd: 0, value: 0xFF },
            },
            ThumbInstruction {
                guest_pc,
                encoding: 0x2D7F,
                operation: ThumbOperation::CompareImmediate {
                    rd: 5,
                    immediate: 0x7F,
                },
            },
            ThumbInstruction {
                guest_pc,
                encoding: 0x3201,
                operation: ThumbOperation::AddSubtractImmediate {
                    rd: 2,
                    immediate: 1,
                    subtract: false,
                },
            },
            ThumbInstruction {
                guest_pc,
                encoding: 0x3F80,
                operation: ThumbOperation::AddSubtractImmediate {
                    rd: 7,
                    immediate: 0x80,
                    subtract: true,
                },
            },
        ];
        let arm_pc = 0x0800_0080;
        let make_program = |instruction: ThumbInstruction| {
            let bx_pc = instruction.guest_pc.wrapping_add(2);
            ArmProgram {
                version: GBA_IR_VERSION,
                instructions: vec![ArmInstruction {
                    guest_pc: arm_pc,
                    encoding: 0xEAFF_FFFE,
                    condition: 14,
                    operation: ArmOperation::Branch {
                        target: arm_pc,
                        link: false,
                        reachable_successors: 0b01,
                    },
                }],
                thumb_instructions: vec![
                    instruction,
                    ThumbInstruction {
                        guest_pc: bx_pc,
                        encoding: 0x4700,
                        operation: ThumbOperation::BranchExchange { rm: 0 },
                    },
                ],
                thumb_ldmia_rom_tables: vec![],
                branch_targets: vec![StaticBranchTarget {
                    source_mode: IsaMode::Thumb,
                    source_pc: bx_pc,
                    target_mode: IsaMode::Arm,
                    target_pc: arm_pc,
                }],
            }
        };

        for instruction in cases {
            assert!(make_program(instruction).verify().is_ok());
        }

        let mismatches = [
            (
                cases[0],
                ThumbOperation::MovImmediate { rd: 0, value: 0xFE },
            ),
            (
                cases[0],
                ThumbOperation::MovImmediate { rd: 1, value: 0xFF },
            ),
            (
                cases[0],
                ThumbOperation::CompareImmediate {
                    rd: 0,
                    immediate: 0xFF,
                },
            ),
            (
                cases[1],
                ThumbOperation::CompareImmediate {
                    rd: 4,
                    immediate: 0x7F,
                },
            ),
            (
                cases[1],
                ThumbOperation::CompareImmediate {
                    rd: 5,
                    immediate: 0x7E,
                },
            ),
            (
                cases[2],
                ThumbOperation::AddSubtractImmediate {
                    rd: 3,
                    immediate: 1,
                    subtract: false,
                },
            ),
            (
                cases[2],
                ThumbOperation::AddSubtractImmediate {
                    rd: 2,
                    immediate: 2,
                    subtract: false,
                },
            ),
            (
                cases[2],
                ThumbOperation::AddSubtractImmediate {
                    rd: 2,
                    immediate: 1,
                    subtract: true,
                },
            ),
            (
                cases[3],
                ThumbOperation::AddSubtractImmediate {
                    rd: 7,
                    immediate: 0x80,
                    subtract: false,
                },
            ),
        ];
        for (mut instruction, operation) in mismatches {
            instruction.operation = operation;
            assert_eq!(
                make_program(instruction).verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                    instruction.encoding
                )))
            );
        }

        for operation in [
            ThumbOperation::CompareImmediate {
                rd: 8,
                immediate: 0,
            },
            ThumbOperation::AddSubtractImmediate {
                rd: 8,
                immediate: 0,
                subtract: false,
            },
            ThumbOperation::AddSubtractImmediate {
                rd: 8,
                immediate: 0,
                subtract: true,
            },
        ] {
            let mut instruction = cases[2];
            instruction.operation = operation;
            assert_eq!(
                make_program(instruction).verify(),
                Err(VerifyError::InvalidThumbFormat3Register(8))
            );
        }

        let mut out_of_family = cases[2];
        out_of_family.encoding = 0x4058;
        assert_eq!(
            make_program(out_of_family).verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x4058))
        );

        let mut misaligned = make_program(cases[2]);
        misaligned.thumb_instructions[0].guest_pc += 1;
        assert_eq!(
            misaligned.verify(),
            Err(VerifyError::MisalignedThumbPc(guest_pc + 1))
        );

        let missing_successor = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![cases[2]],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![],
        };
        assert_eq!(
            missing_successor.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: guest_pc,
                target: guest_pc + 2,
            })
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
            thumb_ldmia_rom_tables: vec![],
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
    fn arm_program_verifier_checks_thumb_lsr_immediate_fields_registers_and_successor() {
        let guest_pc = 0x0800_0100;
        let make_program = |instruction: ThumbInstruction| ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![
                instruction,
                ThumbInstruction {
                    guest_pc: guest_pc + 2,
                    encoding: 0x4700,
                    operation: ThumbOperation::BranchExchange { rm: 0 },
                },
            ],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![StaticBranchTarget {
                source_mode: IsaMode::Thumb,
                source_pc: guest_pc + 2,
                target_mode: IsaMode::Thumb,
                target_pc: guest_pc + 2,
            }],
        };

        for (rd, rs, amount) in [(0, 0, 0), (1, 2, 16), (7, 7, 31)] {
            let encoding = 0x0800 | (u16::from(amount) << 6) | (u16::from(rs) << 3) | u16::from(rd);
            let instruction = ThumbInstruction {
                guest_pc,
                encoding,
                operation: ThumbOperation::LsrImmediate { rd, rs, amount },
            };
            assert!(make_program(instruction).verify().is_ok());
        }

        let valid = ThumbInstruction {
            guest_pc,
            encoding: 0x0C00,
            operation: ThumbOperation::LsrImmediate {
                rd: 0,
                rs: 0,
                amount: 16,
            },
        };
        for operation in [
            ThumbOperation::LsrImmediate {
                rd: 1,
                rs: 0,
                amount: 16,
            },
            ThumbOperation::LsrImmediate {
                rd: 0,
                rs: 1,
                amount: 16,
            },
            ThumbOperation::LsrImmediate {
                rd: 0,
                rs: 0,
                amount: 15,
            },
        ] {
            let mut mismatched = make_program(valid);
            mismatched.thumb_instructions[0].operation = operation;
            assert_eq!(
                mismatched.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(0x0C00))
            );
        }

        for operation in [
            ThumbOperation::LsrImmediate {
                rd: 8,
                rs: 0,
                amount: 16,
            },
            ThumbOperation::LsrImmediate {
                rd: 0,
                rs: 8,
                amount: 16,
            },
        ] {
            let invalid_register = match operation {
                ThumbOperation::LsrImmediate { rd: 8, .. }
                | ThumbOperation::LsrImmediate { rs: 8, .. } => 8,
                _ => unreachable!(),
            };
            let mut invalid = make_program(valid);
            invalid.thumb_instructions[0].operation = operation;
            assert_eq!(
                invalid.verify(),
                Err(VerifyError::InvalidThumbLsrRegister(invalid_register))
            );
        }

        for encoding in [0x0000, 0x1049, 0x4089] {
            let mut out_of_family = make_program(valid);
            out_of_family.thumb_instructions[0].encoding = encoding;
            assert_eq!(
                out_of_family.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                    encoding
                )))
            );
        }

        let mut missing_successor = make_program(valid);
        missing_successor.thumb_instructions.pop();
        missing_successor.branch_targets.clear();
        assert_eq!(
            missing_successor.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: guest_pc,
                target: guest_pc + 2,
            })
        );
    }

    #[test]
    fn arm_program_verifier_checks_thumb_orr_fields_registers_alignment_and_successor() {
        let guest_pc = 0x0800_0100;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x4318,
            operation: ThumbOperation::OrrRegister { rd: 0, rs: 3 },
        };
        let bx = ThumbInstruction {
            guest_pc: guest_pc + 2,
            encoding: 0x4700,
            operation: ThumbOperation::BranchExchange { rm: 0 },
        };
        let make_program = |instruction| ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![instruction, bx],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![StaticBranchTarget {
                source_mode: IsaMode::Thumb,
                source_pc: guest_pc + 2,
                target_mode: IsaMode::Thumb,
                target_pc: guest_pc,
            }],
        };
        assert!(make_program(instruction).verify().is_ok());

        let mut mismatched = make_program(instruction);
        mismatched.thumb_instructions[0].operation = ThumbOperation::OrrRegister { rd: 0, rs: 2 };
        assert_eq!(
            mismatched.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x4318))
        );

        let mut mismatched = make_program(instruction);
        mismatched.thumb_instructions[0].operation = ThumbOperation::OrrRegister { rd: 1, rs: 3 };
        assert_eq!(
            mismatched.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x4318))
        );

        let mut out_of_family = make_program(instruction);
        out_of_family.thumb_instructions[0].encoding = 0x4358;
        assert_eq!(
            out_of_family.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x4358))
        );

        for operation in [
            ThumbOperation::OrrRegister { rd: 8, rs: 3 },
            ThumbOperation::OrrRegister { rd: 0, rs: 8 },
        ] {
            let mut invalid_register = make_program(instruction);
            invalid_register.thumb_instructions[0].operation = operation;
            let invalid = match operation {
                ThumbOperation::OrrRegister { rd: 8, .. } => 8,
                ThumbOperation::OrrRegister { rs: 8, .. } => 8,
                _ => unreachable!(),
            };
            assert_eq!(
                invalid_register.verify(),
                Err(VerifyError::InvalidThumbOrrRegister(invalid))
            );
        }

        let mut misaligned = make_program(instruction);
        misaligned.thumb_instructions[0].guest_pc += 1;
        assert_eq!(
            misaligned.verify(),
            Err(VerifyError::MisalignedThumbPc(guest_pc + 1))
        );

        let missing_successor = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![instruction],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![],
        };
        assert_eq!(
            missing_successor.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: guest_pc,
                target: guest_pc + 2,
            })
        );
    }

    #[test]
    fn arm_program_verifier_checks_thumb_and_fields_registers_alignment_and_successor() {
        let guest_pc = 0x0800_0100;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x4018,
            operation: ThumbOperation::AndRegister { rd: 0, rs: 3 },
        };
        let bx = ThumbInstruction {
            guest_pc: guest_pc + 2,
            encoding: 0x4700,
            operation: ThumbOperation::BranchExchange { rm: 0 },
        };
        let make_program = |instruction| ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![instruction, bx],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![StaticBranchTarget {
                source_mode: IsaMode::Thumb,
                source_pc: guest_pc + 2,
                target_mode: IsaMode::Thumb,
                target_pc: guest_pc,
            }],
        };
        assert!(make_program(instruction).verify().is_ok());

        for operation in [
            ThumbOperation::AndRegister { rd: 0, rs: 2 },
            ThumbOperation::AndRegister { rd: 1, rs: 3 },
        ] {
            let mut mismatched = make_program(instruction);
            mismatched.thumb_instructions[0].operation = operation;
            assert_eq!(
                mismatched.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(0x4018))
            );
        }

        let mut out_of_family = make_program(instruction);
        out_of_family.thumb_instructions[0].encoding = 0x4058;
        assert_eq!(
            out_of_family.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x4058))
        );

        for operation in [
            ThumbOperation::AndRegister { rd: 8, rs: 3 },
            ThumbOperation::AndRegister { rd: 0, rs: 8 },
        ] {
            let mut invalid_register = make_program(instruction);
            invalid_register.thumb_instructions[0].operation = operation;
            assert_eq!(
                invalid_register.verify(),
                Err(VerifyError::InvalidThumbAndRegister(8))
            );
        }

        let mut misaligned = make_program(instruction);
        misaligned.thumb_instructions[0].guest_pc += 1;
        assert_eq!(
            misaligned.verify(),
            Err(VerifyError::MisalignedThumbPc(guest_pc + 1))
        );

        let mut missing_successor = make_program(instruction);
        missing_successor.thumb_instructions.pop();
        missing_successor.branch_targets.clear();
        assert_eq!(
            missing_successor.verify(),
            Err(VerifyError::MissingSuccessor {
                pc: guest_pc,
                target: guest_pc + 2,
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
                reachable_successors: 0b01,
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
            thumb_ldmia_rom_tables: vec![],
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
        out_of_family.thumb_instructions[0].encoding = 0x4058;
        assert_eq!(
            out_of_family.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x4058))
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
    fn arm_program_verifier_checks_thumb_ldr_word_fields_and_fallthrough() {
        let guest_pc = 0x0800_0100;
        let load = ThumbInstruction {
            guest_pc,
            encoding: 0x6880,
            operation: ThumbOperation::LoadWordImmediate {
                rd: 0,
                rb: 0,
                imm5: 2,
            },
        };
        let make_program = |load| ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![
                load,
                ThumbInstruction {
                    guest_pc: guest_pc + 2,
                    encoding: 0x4700,
                    operation: ThumbOperation::BranchExchange { rm: 0 },
                },
            ],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![StaticBranchTarget {
                source_mode: IsaMode::Thumb,
                source_pc: guest_pc + 2,
                target_mode: IsaMode::Thumb,
                target_pc: guest_pc,
            }],
        };
        assert!(make_program(load).verify().is_ok());

        let zero_offset = ThumbInstruction {
            guest_pc,
            encoding: 0x6800,
            operation: ThumbOperation::LoadWordImmediate {
                rd: 0,
                rb: 0,
                imm5: 0,
            },
        };
        assert!(make_program(zero_offset).verify().is_ok());

        let max_offset = ThumbInstruction {
            guest_pc,
            encoding: 0x6FC7,
            operation: ThumbOperation::LoadWordImmediate {
                rd: 7,
                rb: 0,
                imm5: 31,
            },
        };
        assert!(make_program(max_offset).verify().is_ok());

        let mut invalid_register = make_program(load);
        invalid_register.thumb_instructions[0].operation = ThumbOperation::LoadWordImmediate {
            rd: 8,
            rb: 0,
            imm5: 2,
        };
        assert_eq!(
            invalid_register.verify(),
            Err(VerifyError::InvalidThumbWordLoadRegister(8))
        );

        let mut mismatched = make_program(load);
        mismatched.thumb_instructions[0].operation = ThumbOperation::LoadWordImmediate {
            rd: 1,
            rb: 0,
            imm5: 2,
        };
        assert_eq!(
            mismatched.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x6880))
        );

        for encoding in [0x5808, 0x5C08, 0x9800] {
            let mut out_of_family = make_program(load);
            out_of_family.thumb_instructions[0].encoding = encoding;
            assert_eq!(
                out_of_family.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                    encoding
                )))
            );
        }

        let mut missing_fallthrough = make_program(load);
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
    fn arm_program_verifier_checks_thumb_ldrh_fields_and_fallthrough() {
        let guest_pc = 0x0800_0100;
        let load = ThumbInstruction {
            guest_pc,
            encoding: 0x8821,
            operation: ThumbOperation::LoadHalfwordImmediate {
                rd: 1,
                rb: 4,
                imm5: 0,
            },
        };
        let make_program = |load| ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![
                load,
                ThumbInstruction {
                    guest_pc: guest_pc + 2,
                    encoding: 0x4700,
                    operation: ThumbOperation::BranchExchange { rm: 0 },
                },
            ],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![StaticBranchTarget {
                source_mode: IsaMode::Thumb,
                source_pc: guest_pc + 2,
                target_mode: IsaMode::Thumb,
                target_pc: guest_pc,
            }],
        };
        assert!(make_program(load).verify().is_ok());

        let zero_offset = ThumbInstruction {
            guest_pc,
            encoding: 0x8808,
            operation: ThumbOperation::LoadHalfwordImmediate {
                rd: 0,
                rb: 1,
                imm5: 0,
            },
        };
        assert!(make_program(zero_offset).verify().is_ok());

        let max_offset = ThumbInstruction {
            guest_pc,
            encoding: 0x8FC8,
            operation: ThumbOperation::LoadHalfwordImmediate {
                rd: 0,
                rb: 1,
                imm5: 31,
            },
        };
        assert!(make_program(max_offset).verify().is_ok());

        let alias = ThumbInstruction {
            guest_pc,
            encoding: 0x8809,
            operation: ThumbOperation::LoadHalfwordImmediate {
                rd: 1,
                rb: 1,
                imm5: 0,
            },
        };
        assert!(make_program(alias).verify().is_ok());

        let mut invalid_register = make_program(load);
        invalid_register.thumb_instructions[0].operation = ThumbOperation::LoadHalfwordImmediate {
            rd: 8,
            rb: 4,
            imm5: 0,
        };
        assert_eq!(
            invalid_register.verify(),
            Err(VerifyError::InvalidThumbHalfwordLoadRegister(8))
        );

        let mut mismatched = make_program(load);
        mismatched.thumb_instructions[0].operation = ThumbOperation::LoadHalfwordImmediate {
            rd: 0,
            rb: 4,
            imm5: 0,
        };
        assert_eq!(
            mismatched.verify(),
            Err(VerifyError::UnsupportedInstructionEncoding(0x8821))
        );

        for encoding in [0x5A49, 0x5A08, 0x8008, 0x9800] {
            let mut out_of_family = make_program(load);
            out_of_family.thumb_instructions[0].encoding = encoding;
            assert_eq!(
                out_of_family.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                    encoding
                )))
            );
        }

        let mut missing_fallthrough = make_program(load);
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
            thumb_ldmia_rom_tables: vec![],
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
            thumb_ldmia_rom_tables: vec![],
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
    fn arm_program_verifier_checks_thumb_strb_fields_and_fallthrough() {
        let guest_pc = 0x0800_0100;
        let store = ThumbInstruction {
            guest_pc,
            encoding: 0x7005,
            operation: ThumbOperation::StoreByteImmediate {
                rd: 5,
                rb: 0,
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
            thumb_ldmia_rom_tables: vec![],
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
            encoding: 0x77FD,
            operation: ThumbOperation::StoreByteImmediate {
                rd: 5,
                rb: 7,
                imm5: 31,
            },
        };
        assert!(make_program(max_offset_store).verify().is_ok());

        let aliased_store = ThumbInstruction {
            guest_pc,
            encoding: 0x7000,
            operation: ThumbOperation::StoreByteImmediate {
                rd: 0,
                rb: 0,
                imm5: 0,
            },
        };
        assert!(make_program(aliased_store).verify().is_ok());

        for operation in [
            ThumbOperation::StoreByteImmediate {
                rd: 0,
                rb: 0,
                imm5: 1,
            },
            ThumbOperation::StoreByteImmediate {
                rd: 1,
                rb: 0,
                imm5: 0,
            },
            ThumbOperation::StoreByteImmediate {
                rd: 5,
                rb: 1,
                imm5: 0,
            },
        ] {
            let mut mismatched = make_program(store);
            mismatched.thumb_instructions[0].operation = operation;
            assert_eq!(
                mismatched.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(0x7005))
            );
        }

        for encoding in [0x7808, 0x5408, 0x8008] {
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
        invalid_register.thumb_instructions[0].operation = ThumbOperation::StoreByteImmediate {
            rd: 8,
            rb: 0,
            imm5: 0,
        };
        assert_eq!(
            invalid_register.verify(),
            Err(VerifyError::InvalidThumbByteStoreRegister(8))
        );

        let missing_fallthrough = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![store],
            thumb_ldmia_rom_tables: vec![],
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
    fn arm_program_verifier_checks_thumb_str_fields_and_fallthrough() {
        let guest_pc = 0x0800_0100;
        let store = ThumbInstruction {
            guest_pc,
            encoding: 0x6005,
            operation: ThumbOperation::StoreWordImmediate {
                rd: 5,
                rb: 0,
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
            thumb_ldmia_rom_tables: vec![],
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
            encoding: 0x67C9,
            operation: ThumbOperation::StoreWordImmediate {
                rd: 1,
                rb: 1,
                imm5: 31,
            },
        };
        assert!(make_program(max_offset_store).verify().is_ok());

        for operation in [
            ThumbOperation::StoreWordImmediate {
                rd: 0,
                rb: 0,
                imm5: 1,
            },
            ThumbOperation::StoreWordImmediate {
                rd: 1,
                rb: 0,
                imm5: 0,
            },
            ThumbOperation::StoreWordImmediate {
                rd: 5,
                rb: 1,
                imm5: 0,
            },
        ] {
            let mut mismatched = make_program(store);
            mismatched.thumb_instructions[0].operation = operation;
            assert_eq!(
                mismatched.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(0x6005))
            );
        }

        let mut invalid_register = make_program(store);
        invalid_register.thumb_instructions[0].operation = ThumbOperation::StoreWordImmediate {
            rd: 8,
            rb: 0,
            imm5: 0,
        };
        assert_eq!(
            invalid_register.verify(),
            Err(VerifyError::InvalidThumbWordStoreRegister(8))
        );

        for encoding in [0x6808, 0x5008, 0x5208] {
            let mut out_of_family = make_program(store);
            out_of_family.thumb_instructions[0].encoding = encoding;
            assert_eq!(
                out_of_family.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                    encoding,
                )))
            );
        }

        let missing_fallthrough = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![store],
            thumb_ldmia_rom_tables: vec![],
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
    fn arm_program_verifier_checks_thumb_ldrb_fields_and_fallthrough() {
        let guest_pc = 0x0800_0100;
        let load = ThumbInstruction {
            guest_pc,
            encoding: 0x7808,
            operation: ThumbOperation::LoadByteImmediate {
                rd: 0,
                rb: 1,
                imm5: 0,
            },
        };
        let make_program = |load| ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![
                load,
                ThumbInstruction {
                    guest_pc: guest_pc + 2,
                    encoding: 0x4700,
                    operation: ThumbOperation::BranchExchange { rm: 0 },
                },
            ],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![StaticBranchTarget {
                source_mode: IsaMode::Thumb,
                source_pc: guest_pc + 2,
                target_mode: IsaMode::Thumb,
                target_pc: guest_pc,
            }],
        };
        assert!(make_program(load).verify().is_ok());

        let max_offset_load = ThumbInstruction {
            guest_pc,
            encoding: 0x7FFF,
            operation: ThumbOperation::LoadByteImmediate {
                rd: 7,
                rb: 7,
                imm5: 31,
            },
        };
        assert!(make_program(max_offset_load).verify().is_ok());

        let aliased_load = ThumbInstruction {
            guest_pc,
            encoding: 0x7809,
            operation: ThumbOperation::LoadByteImmediate {
                rd: 1,
                rb: 1,
                imm5: 0,
            },
        };
        assert!(make_program(aliased_load).verify().is_ok());

        for operation in [
            ThumbOperation::LoadByteImmediate {
                rd: 0,
                rb: 1,
                imm5: 1,
            },
            ThumbOperation::LoadByteImmediate {
                rd: 1,
                rb: 1,
                imm5: 0,
            },
            ThumbOperation::LoadByteImmediate {
                rd: 0,
                rb: 0,
                imm5: 0,
            },
        ] {
            let mut mismatched = make_program(load);
            mismatched.thumb_instructions[0].operation = operation;
            assert_eq!(
                mismatched.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(0x7808))
            );
        }

        for encoding in [0x7008, 0x5C08, 0x5808, 0x8008] {
            let mut out_of_family = make_program(load);
            out_of_family.thumb_instructions[0].encoding = encoding;
            assert_eq!(
                out_of_family.verify(),
                Err(VerifyError::UnsupportedInstructionEncoding(u32::from(
                    encoding,
                )))
            );
        }

        let mut invalid_register = make_program(load);
        invalid_register.thumb_instructions[0].operation = ThumbOperation::LoadByteImmediate {
            rd: 8,
            rb: 1,
            imm5: 0,
        };
        assert_eq!(
            invalid_register.verify(),
            Err(VerifyError::InvalidThumbByteLoadRegister(8))
        );

        let missing_fallthrough = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![],
            thumb_instructions: vec![load],
            thumb_ldmia_rom_tables: vec![],
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
                        reachable_successors: 0b01,
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
                thumb_ldmia_rom_tables: vec![],
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
            thumb_ldmia_rom_tables: vec![],
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
            thumb_ldmia_rom_tables: vec![],
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
                    reachable_successors: 0b11,
                },
            }],
            thumb_instructions: vec![],
            thumb_ldmia_rom_tables: vec![],
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

    #[test]
    fn program_verifier_rejects_invalid_arm_branch_successor_mask() {
        let program = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![ArmInstruction {
                guest_pc: 0x0800_0000,
                encoding: 0xEAFF_FFFE,
                condition: 14,
                operation: ArmOperation::Branch {
                    target: 0x0800_0000,
                    link: false,
                    reachable_successors: 0b100,
                },
            }],
            thumb_instructions: vec![],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![],
        };
        assert_eq!(
            program.verify(),
            Err(VerifyError::InvalidArmBranchSuccessorMask(0b100))
        );
    }
}

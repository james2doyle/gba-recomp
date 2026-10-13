use std::collections::{BTreeMap, BTreeSet, VecDeque};

use gba_ir::{
    ArmInstruction, ArmOperation, ArmProgram, GBA_IR_VERSION, IsaMode, REG_PC, StaticBranchTarget,
    ThumbInstruction, ThumbOperation,
};

use crate::rom::{ROM_BASE, RomImage};

const MAX_DISCOVERED_INSTRUCTIONS: usize = 1_000_000;
const MAX_CONSTANT_TARGETS: usize = 16;
const MAX_ANALYSIS_UPDATES: usize = MAX_DISCOVERED_INSTRUCTIONS * 32;

pub fn discover_arm_program(image: &RomImage) -> Result<ArmProgram, String> {
    let entry = (IsaMode::Arm, ROM_BASE);
    let mut pending = VecDeque::from([entry]);
    let mut incoming = BTreeMap::from([(entry, AbstractState::unknown())]);
    let mut decoded_arm = BTreeMap::<u32, ArmInstruction>::new();
    let mut decoded_thumb = BTreeMap::<u32, ThumbInstruction>::new();
    let mut branch_targets = BTreeSet::<StaticBranchTarget>::new();
    let mut analysis_updates = 0usize;

    while let Some((mode, pc)) = pending.pop_front() {
        analysis_updates += 1;
        if analysis_updates > MAX_ANALYSIS_UPDATES {
            return Err(format!(
                "discovery_limit: abstract analysis exceeded {MAX_ANALYSIS_UPDATES} state updates"
            ));
        }
        if decoded_arm.len() + decoded_thumb.len() > MAX_DISCOVERED_INSTRUCTIONS {
            return Err(format!(
                "discovery_limit: more than {MAX_DISCOVERED_INSTRUCTIONS} reachable instructions"
            ));
        }
        let state = incoming
            .get(&(mode, pc))
            .cloned()
            .expect("queued abstract state must be present");
        match mode {
            IsaMode::Arm => {
                let instruction = if let Some(instruction) = decoded_arm.get(&pc) {
                    *instruction
                } else {
                    let encoding = image
                        .read_arm_word(pc)
                        .map_err(|error| format!("discovery failed at 0x{pc:08X}: {error}"))?;
                    let mut instruction = crate::decoder::decode_arm(pc, encoding)?;
                    if let ArmOperation::LoadPcRelativeWord { rd, guest_address } =
                        instruction.operation
                    {
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
                    decoded_arm.insert(pc, instruction);
                    instruction
                };
                process_arm_instruction(
                    image,
                    instruction,
                    &state,
                    &mut pending,
                    &mut incoming,
                    &mut branch_targets,
                )?;
            }
            IsaMode::Thumb => {
                let instruction = if let Some(instruction) = decoded_thumb.get(&pc) {
                    *instruction
                } else {
                    let encoding = image
                        .read_thumb_halfword(pc)
                        .map_err(|error| format!("discovery failed at 0x{pc:08X}: {error}"))?;
                    let mut instruction = crate::decoder::decode_thumb(pc, encoding)?;
                    if let ThumbOperation::LoadPcRelativeWord { rd, guest_address } =
                        instruction.operation
                    {
                        let value = image.read_arm_word(guest_address).map_err(|error| {
                            format!(
                                "thumb_rom_literal_out_of_rom at 0x{pc:08X}: address 0x{guest_address:08X}: {error}"
                            )
                        })?;
                        instruction.operation = ThumbOperation::LoadRomLiteral {
                            rd,
                            guest_address,
                            value,
                        };
                    }
                    match instruction.operation {
                        ThumbOperation::BlPrefix { .. } => {
                            let suffix_pc = pc.wrapping_add(2);
                            let suffix_encoding = image.read_thumb_halfword(suffix_pc).map_err(|error| {
                                format!(
                                    "thumb_bl_pair_broken at 0x{pc:08X}: expected suffix at 0x{suffix_pc:08X}: {error}"
                                )
                            })?;
                            if !matches!(
                                crate::decoder::decode_thumb(suffix_pc, suffix_encoding)?.operation,
                                ThumbOperation::BlSuffix { .. }
                            ) {
                                return Err(format!(
                                    "thumb_bl_pair_broken at 0x{pc:08X}: 0x{suffix_encoding:04X} is not an adjacent BL suffix"
                                ));
                            }
                        }
                        ThumbOperation::BlSuffix { immediate } => {
                            let prefix_pc = pc.checked_sub(2).ok_or_else(|| {
                                format!("thumb_bl_suffix_without_prefix at 0x{pc:08X}")
                            })?;
                            let prefix_encoding =
                                image.read_thumb_halfword(prefix_pc).map_err(|_| {
                                    format!("thumb_bl_suffix_without_prefix at 0x{pc:08X}")
                                })?;
                            let prefix = crate::decoder::decode_thumb(prefix_pc, prefix_encoding)
                                .map_err(|_| {
                                format!("thumb_bl_suffix_without_prefix at 0x{pc:08X}")
                            })?;
                            let ThumbOperation::BlPrefix { lr_partial } = prefix.operation else {
                                return Err(format!(
                                    "thumb_bl_suffix_without_prefix at 0x{pc:08X}: previous halfword is 0x{prefix_encoding:04X}"
                                ));
                            };
                            instruction.operation = ThumbOperation::BlCall {
                                target: lr_partial.wrapping_add(u32::from(immediate) << 1),
                                expected_lr: lr_partial,
                                return_address: pc.wrapping_add(2) | 1,
                            };
                        }
                        _ => {}
                    }
                    decoded_thumb.insert(pc, instruction);
                    instruction
                };
                process_thumb_instruction(
                    image,
                    instruction,
                    &state,
                    &mut pending,
                    &mut incoming,
                    &mut branch_targets,
                )?;
            }
        }
    }
    let program = ArmProgram {
        version: GBA_IR_VERSION,
        instructions: decoded_arm.into_values().collect(),
        thumb_instructions: decoded_thumb.into_values().collect(),
        branch_targets: branch_targets.into_iter().collect(),
    };
    program
        .verify()
        .map_err(|error| format!("GBA IR rejected: {error}"))?;
    Ok(program)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AbstractValue {
    Unknown,
    Constants(BTreeSet<u32>),
}

impl AbstractValue {
    fn constant(value: u32) -> Self {
        Self::Constants(BTreeSet::from([value]))
    }

    fn merged(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Constants(left), Self::Constants(right)) => {
                let values = left.union(right).copied().collect::<BTreeSet<_>>();
                if values.len() > MAX_CONSTANT_TARGETS {
                    Self::Unknown
                } else {
                    Self::Constants(values)
                }
            }
        }
    }

    fn wrapping_add(&self, immediate: u32) -> Self {
        match self {
            Self::Unknown => Self::Unknown,
            Self::Constants(values) => Self::Constants(
                values
                    .iter()
                    .map(|value| value.wrapping_add(immediate))
                    .collect(),
            ),
        }
    }

    fn wrapping_add_subtract(&self, other: &Self, subtract: bool) -> Self {
        let (Self::Constants(left), Self::Constants(right)) = (self, other) else {
            return Self::Unknown;
        };
        let mut values = BTreeSet::new();
        for lhs in left {
            for rhs in right {
                values.insert(if subtract {
                    lhs.wrapping_sub(*rhs)
                } else {
                    lhs.wrapping_add(*rhs)
                });
                if values.len() > MAX_CONSTANT_TARGETS {
                    return Self::Unknown;
                }
            }
        }
        Self::Constants(values)
    }

    fn values(&self) -> Option<&BTreeSet<u32>> {
        match self {
            Self::Unknown => None,
            Self::Constants(values) => Some(values),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AbstractState {
    registers: [AbstractValue; REG_PC as usize],
}

impl AbstractState {
    fn unknown() -> Self {
        Self {
            registers: std::array::from_fn(|_| AbstractValue::Unknown),
        }
    }

    fn merge_from(&mut self, other: &Self) -> bool {
        let mut changed = false;
        for (current, incoming) in self.registers.iter_mut().zip(&other.registers) {
            let merged = current.merged(incoming);
            if *current != merged {
                *current = merged;
                changed = true;
            }
        }
        changed
    }

    fn set_constant(&mut self, register: u8, value: u32) {
        if register < REG_PC {
            self.registers[register as usize] = AbstractValue::constant(value);
        }
    }

    fn invalidate_all(&mut self) {
        self.registers.fill(AbstractValue::Unknown);
    }
}

fn enqueue_state(
    mode: IsaMode,
    pc: u32,
    state: AbstractState,
    pending: &mut VecDeque<(IsaMode, u32)>,
    incoming: &mut BTreeMap<(IsaMode, u32), AbstractState>,
) {
    let key = (mode, pc);
    match incoming.get_mut(&key) {
        Some(current) => {
            if current.merge_from(&state) {
                pending.push_back(key);
            }
        }
        None => {
            incoming.insert(key, state);
            pending.push_back(key);
        }
    }
}

fn read_operand(state: &AbstractState, register: u8, pc_value: u32) -> AbstractValue {
    if register == REG_PC {
        AbstractValue::constant(pc_value)
    } else {
        state.registers[register as usize].clone()
    }
}

fn transfer_arm(instruction: ArmInstruction, incoming: &AbstractState) -> AbstractState {
    let mut result = incoming.clone();
    match instruction.operation {
        ArmOperation::MovImmediate { rd, value } => result.set_constant(rd, value),
        ArmOperation::MovRegister { rd, rm } => {
            result.registers[rd as usize] =
                read_operand(incoming, rm, instruction.guest_pc.wrapping_add(8));
        }
        ArmOperation::AddImmediate { rd, rn, value } => {
            result.registers[rd as usize] = incoming.registers[rn as usize].wrapping_add(value);
        }
        ArmOperation::AddPcRelativeImmediate { rd, value } => {
            result.set_constant(rd, instruction.guest_pc.wrapping_add(8).wrapping_add(value));
        }
        ArmOperation::LoadRomLiteral { rd, value, .. } => result.set_constant(rd, value),
        ArmOperation::MsrCpsrRegister { field_mask, .. } if field_mask & 1 != 0 => {
            // CPSR mode writes can swap the FIQ and SP/LR banks. The analyzer
            // does not assume that banked values are identical.
            result.registers[8..15].fill(AbstractValue::Unknown);
        }
        ArmOperation::Branch { link: true, .. } => {
            result.set_constant(14, instruction.guest_pc.wrapping_add(4));
        }
        _ => {}
    }
    result
}

fn conditional_state(
    condition: u8,
    before: &AbstractState,
    executed: &AbstractState,
) -> AbstractState {
    if condition == 14 {
        executed.clone()
    } else {
        let mut merged = before.clone();
        merged.merge_from(executed);
        merged
    }
}

fn process_arm_instruction(
    image: &RomImage,
    instruction: ArmInstruction,
    state: &AbstractState,
    pending: &mut VecDeque<(IsaMode, u32)>,
    incoming: &mut BTreeMap<(IsaMode, u32), AbstractState>,
    branch_targets: &mut BTreeSet<StaticBranchTarget>,
) -> Result<(), String> {
    let next_pc = instruction.guest_pc.wrapping_add(4);
    match instruction.operation {
        ArmOperation::Branch { target, link } => {
            ensure_target_in_image(image, instruction.guest_pc, IsaMode::Arm, target)?;
            let mut taken_state = transfer_arm(instruction, state);
            enqueue_state(IsaMode::Arm, target, taken_state.clone(), pending, incoming);
            if instruction.condition != 14 || link {
                if link {
                    // The called function may change caller-visible registers.
                    taken_state.invalidate_all();
                    enqueue_state(IsaMode::Arm, next_pc, taken_state, pending, incoming);
                } else {
                    enqueue_state(
                        IsaMode::Arm,
                        next_pc,
                        conditional_state(instruction.condition, state, &taken_state),
                        pending,
                        incoming,
                    );
                }
            }
        }
        ArmOperation::BranchExchange { rm } => {
            let targets = read_operand(state, rm, instruction.guest_pc.wrapping_add(8));
            let Some(values) = targets.values() else {
                return Err(format!(
                    "bx_target_unresolved at 0x{:08X}: r{rm} is not statically bounded",
                    instruction.guest_pc
                ));
            };
            for raw_target in values {
                let target_mode = if raw_target & 1 != 0 {
                    IsaMode::Thumb
                } else {
                    IsaMode::Arm
                };
                let target_pc = match target_mode {
                    IsaMode::Arm => raw_target & !3,
                    IsaMode::Thumb => raw_target & !1,
                };
                ensure_target_in_image(image, instruction.guest_pc, target_mode, target_pc)?;
                branch_targets.insert(StaticBranchTarget {
                    source_mode: IsaMode::Arm,
                    source_pc: instruction.guest_pc,
                    target_mode,
                    target_pc,
                });
                enqueue_state(target_mode, target_pc, state.clone(), pending, incoming);
            }
            if instruction.condition != 14 {
                enqueue_state(IsaMode::Arm, next_pc, state.clone(), pending, incoming);
            }
        }
        _ => {
            let executed = transfer_arm(instruction, state);
            let after = conditional_state(instruction.condition, state, &executed);
            enqueue_state(IsaMode::Arm, next_pc, after, pending, incoming);
        }
    }
    Ok(())
}

fn process_thumb_instruction(
    image: &RomImage,
    instruction: ThumbInstruction,
    state: &AbstractState,
    pending: &mut VecDeque<(IsaMode, u32)>,
    incoming: &mut BTreeMap<(IsaMode, u32), AbstractState>,
    branch_targets: &mut BTreeSet<StaticBranchTarget>,
) -> Result<(), String> {
    match instruction.operation {
        ThumbOperation::LoadPcRelativeWord { rd, guest_address } => {
            return Err(format!(
                "thumb_rom_literal_unresolved at 0x{:08X}: r{rd} address 0x{guest_address:08X}",
                instruction.guest_pc
            ));
        }
        ThumbOperation::LoadRomLiteral { rd, value, .. } => {
            let mut next_state = state.clone();
            next_state.set_constant(rd, value);
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
        ThumbOperation::AddSubtract {
            rd,
            rn,
            operand,
            immediate,
            subtract,
        } => {
            let rhs = if immediate {
                AbstractValue::constant(u32::from(operand))
            } else {
                state.registers[operand as usize].clone()
            };
            let result = state.registers[rn as usize].wrapping_add_subtract(&rhs, subtract);
            let mut next_state = state.clone();
            next_state.registers[rd as usize] = result;
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
        ThumbOperation::LslImmediate { rd, rs, amount } => {
            let shifted_source = match &state.registers[rs as usize] {
                AbstractValue::Unknown => AbstractValue::Unknown,
                AbstractValue::Constants(values) => AbstractValue::Constants(
                    values
                        .iter()
                        .map(|value| value.wrapping_shl(u32::from(amount)))
                        .collect(),
                ),
            };
            let mut next_state = state.clone();
            next_state.registers[rd as usize] = shifted_source;
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
        ThumbOperation::BranchExchange { rm } => {
            let targets = read_operand(state, rm, instruction.guest_pc.wrapping_add(4));
            let Some(values) = targets.values() else {
                return Err(format!(
                    "bx_target_unresolved at 0x{:08X}: r{rm} is not statically bounded",
                    instruction.guest_pc
                ));
            };
            for raw_target in values {
                let target_mode = if raw_target & 1 != 0 {
                    IsaMode::Thumb
                } else {
                    IsaMode::Arm
                };
                let target_pc = match target_mode {
                    IsaMode::Arm => raw_target & !3,
                    IsaMode::Thumb => raw_target & !1,
                };
                ensure_target_in_image(image, instruction.guest_pc, target_mode, target_pc)?;
                branch_targets.insert(StaticBranchTarget {
                    source_mode: IsaMode::Thumb,
                    source_pc: instruction.guest_pc,
                    target_mode,
                    target_pc,
                });
                enqueue_state(target_mode, target_pc, state.clone(), pending, incoming);
            }
        }
        ThumbOperation::BlPrefix { lr_partial } => {
            let suffix_pc = instruction.guest_pc.wrapping_add(2);
            let suffix_encoding = image.read_thumb_halfword(suffix_pc).map_err(|error| {
                format!(
                    "thumb_bl_pair_broken at 0x{:08X}: expected suffix at 0x{suffix_pc:08X}: {error}",
                    instruction.guest_pc
                )
            })?;
            if !matches!(
                crate::decoder::decode_thumb(suffix_pc, suffix_encoding)?.operation,
                ThumbOperation::BlSuffix { .. }
            ) {
                return Err(format!(
                    "thumb_bl_pair_broken at 0x{:08X}: 0x{suffix_encoding:04X} is not an adjacent BL suffix",
                    instruction.guest_pc
                ));
            }
            let mut next_state = state.clone();
            next_state.registers[14] = AbstractValue::constant(lr_partial);
            enqueue_state(IsaMode::Thumb, suffix_pc, next_state, pending, incoming);
        }
        ThumbOperation::BlSuffix { .. } => {
            return Err(format!(
                "thumb_bl_suffix_unresolved at 0x{:08X}",
                instruction.guest_pc
            ));
        }
        ThumbOperation::BlCall {
            target,
            expected_lr,
            return_address,
        } => {
            if state.registers[14] != AbstractValue::constant(expected_lr) {
                return Err(format!(
                    "thumb_bl_lr_mismatch at 0x{:08X}: expected 0x{expected_lr:08X} from adjacent prefix",
                    instruction.guest_pc
                ));
            }
            ensure_target_in_image(image, instruction.guest_pc, IsaMode::Thumb, target)?;
            let mut target_state = state.clone();
            target_state.registers[14] = AbstractValue::constant(return_address);
            enqueue_state(IsaMode::Thumb, target, target_state, pending, incoming);
        }
        ThumbOperation::Swi { immediate } => {
            if immediate != 1 {
                return Err(format!(
                    "swi_unsupported at 0x{:08X}: Thumb SWI #{immediate}",
                    instruction.guest_pc
                ));
            }
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                state.clone(),
                pending,
                incoming,
            );
        }
        ThumbOperation::Push { register_list } => {
            let bytes = register_list.count_ones().wrapping_mul(4);
            let mut next_state = state.clone();
            next_state.registers[13] = state.registers[13].wrapping_add(0_u32.wrapping_sub(bytes));
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
        ThumbOperation::MovImmediate { rd, value } => {
            let mut next_state = state.clone();
            next_state.set_constant(rd, u32::from(value));
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
        ThumbOperation::MovRegister { rd, rm } => {
            let mut next_state = state.clone();
            next_state.registers[rd as usize] =
                read_operand(state, rm, instruction.guest_pc.wrapping_add(4));
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
    }
    Ok(())
}

fn ensure_target_in_image(
    image: &RomImage,
    pc: u32,
    mode: IsaMode,
    target: u32,
) -> Result<(), String> {
    let (width, mask, mode_name) = match mode {
        IsaMode::Arm => (4, 3, "ARM"),
        IsaMode::Thumb => (2, 1, "Thumb"),
    };
    image.offset_for_guest(target, width).map_err(|error| {
        format!(
            "branch_target_out_of_rom at 0x{pc:08X}: {mode_name} target 0x{target:08X}: {error}"
        )
    })?;
    if target & mask != 0 {
        return Err(format!(
            "branch_target_misaligned at 0x{pc:08X}: {mode_name} target 0x{target:08X}"
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

    fn bx_interwork_rom() -> RomImage {
        let mut bytes = counter_rom().bytes().to_vec();
        bytes[0x84..0x86].copy_from_slice(&0x4708_u16.to_le_bytes()); // Thumb BX r1.
        let words = [
            0xE59F_0008_u32, // LDR r0, [pc, #8] -> Thumb address | 1.
            0xE59F_1008,     // LDR r1, [pc, #8] -> ARM return address.
            0xE12F_FF10,     // BX r0.
            0xEAFF_FFFE,     // ARM B self.
        ];
        for (index, word) in words.into_iter().enumerate() {
            let offset = 0xC0 + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes[0xD0..0xD4].copy_from_slice(&0x0800_0085_u32.to_le_bytes());
        bytes[0xD4..0xD8].copy_from_slice(&0x0800_00CE_u32.to_le_bytes());
        bytes[0xBD] = header_checksum(&bytes);
        RomImage::from_bytes(bytes).unwrap()
    }

    fn thumb_bl_rom() -> RomImage {
        let mut bytes = counter_rom().bytes().to_vec();
        let thumb = [
            0xF000_u16, 0xF804, 0x4610, 0x4708, 0x46C0, 0x46C0, 0x2242, 0x4770,
        ];
        for (index, halfword) in thumb.into_iter().enumerate() {
            let offset = 0x84 + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&halfword.to_le_bytes());
        }
        let arm = [
            0xE59F_0010_u32, // LDR r0, [pc,#0x10] -> Thumb entry | 1.
            0xE59F_1010,     // LDR r1, [pc,#0x10] -> ARM return.
            0xE12F_FF10,     // BX r0.
            0xEAFF_FFFE,     // B self.
        ];
        for (index, word) in arm.into_iter().enumerate() {
            let offset = 0xC0 + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes[0xD8..0xDC].copy_from_slice(&0x0800_0085_u32.to_le_bytes());
        bytes[0xDC..0xE0].copy_from_slice(&0x0800_00D0_u32.to_le_bytes());
        bytes[0xBD] = header_checksum(&bytes);
        RomImage::from_bytes(bytes).unwrap()
    }

    fn thumb_ldr_literal_rom() -> RomImage {
        let mut bytes = counter_rom().bytes().to_vec();
        // Thumb starts at 0x86 (PC+4 aligns to 0x88); +0x58 reaches the pool at 0xE0.
        bytes[0x86..0x8A].copy_from_slice(&0x4708_4916_u32.to_le_bytes());
        bytes[0xC0..0xC4].copy_from_slice(&0xE59F_2008_u32.to_le_bytes());
        bytes[0xC4..0xC8].copy_from_slice(&0xE12F_FF12_u32.to_le_bytes());
        bytes[0xD0..0xD4].copy_from_slice(&0x0800_0087_u32.to_le_bytes());
        bytes[0xD4..0xD8].copy_from_slice(&0xEAFF_FFFE_u32.to_le_bytes());
        bytes[0xE0..0xE4].copy_from_slice(&0x0800_00D4_u32.to_le_bytes());
        bytes[0xBD] = header_checksum(&bytes);
        RomImage::from_bytes(bytes).unwrap()
    }

    fn thumb_addsub_bx_rom() -> RomImage {
        let mut bytes = counter_rom().bytes().to_vec();
        let thumb = [0x4A14_u16, 0x1C10, 0x4700]; // LDR r2,[PC,#0x50]; ADDS r0,r2,#0; BX r0.
        for (index, halfword) in thumb.into_iter().enumerate() {
            let offset = 0x84 + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&halfword.to_le_bytes());
        }
        bytes[0xC0..0xC4].copy_from_slice(&0xE59F_2008_u32.to_le_bytes());
        bytes[0xC4..0xC8].copy_from_slice(&0xE12F_FF12_u32.to_le_bytes());
        bytes[0xC8..0xCC].copy_from_slice(&0xEAFF_FFFE_u32.to_le_bytes());
        bytes[0xD0..0xD4].copy_from_slice(&0x0800_0085_u32.to_le_bytes());
        bytes[0xD8..0xDC].copy_from_slice(&0x0800_00C8_u32.to_le_bytes());
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
    fn resolves_thumb_literal_and_closes_bx_target_from_immutable_rom() {
        let program = discover_arm_program(&thumb_ldr_literal_rom()).unwrap();
        let literal = program
            .thumb_instructions
            .iter()
            .find(|instruction| instruction.guest_pc == ROM_BASE + 0x86)
            .unwrap();
        assert_eq!(
            literal.operation,
            ThumbOperation::LoadRomLiteral {
                rd: 1,
                guest_address: ROM_BASE + 0xE0,
                value: ROM_BASE + 0xD4,
            }
        );
        assert!(program.branch_targets.contains(&StaticBranchTarget {
            source_mode: IsaMode::Thumb,
            source_pc: ROM_BASE + 0x88,
            target_mode: IsaMode::Arm,
            target_pc: ROM_BASE + 0xD4,
        }));
        assert_eq!(program.thumb_instructions.len(), 2);
        assert!(program.verify().is_ok());
    }

    #[test]
    fn thumb_addsub_zero_immediate_propagates_literal_to_valid_bx_target() {
        let program = discover_arm_program(&thumb_addsub_bx_rom()).unwrap();
        assert!(program.thumb_instructions.iter().any(|instruction| {
            instruction.guest_pc == ROM_BASE + 0x86
                && instruction.operation
                    == ThumbOperation::AddSubtract {
                        rd: 0,
                        rn: 2,
                        operand: 0,
                        immediate: true,
                        subtract: false,
                    }
        }));
        assert!(program.branch_targets.contains(&StaticBranchTarget {
            source_mode: IsaMode::Thumb,
            source_pc: ROM_BASE + 0x88,
            target_mode: IsaMode::Arm,
            target_pc: ROM_BASE + 0xC8,
        }));
        assert!(program.verify().is_ok());
    }

    #[test]
    fn thumb_addsub_constant_propagation_invalidates_unknown_inputs() {
        let image = thumb_addsub_bx_rom();
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x1C10,
            operation: ThumbOperation::AddSubtract {
                rd: 0,
                rn: 2,
                operand: 0,
                immediate: true,
                subtract: false,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(2, 0xFFFF_FFFF);
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &image,
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)].registers[0],
            AbstractValue::constant(0xFFFF_FFFF)
        );

        state.registers[2] = AbstractValue::Unknown;
        pending.clear();
        incoming.clear();
        process_thumb_instruction(
            &image,
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)].registers[0],
            AbstractValue::Unknown
        );

        let add_wrap = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x1C50,
            operation: ThumbOperation::AddSubtract {
                rd: 0,
                rn: 2,
                operand: 1,
                immediate: true,
                subtract: false,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(2, u32::MAX);
        pending.clear();
        incoming.clear();
        process_thumb_instruction(
            &image,
            add_wrap,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)].registers[0],
            AbstractValue::constant(0)
        );

        let register_subtract = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x1AD0,
            operation: ThumbOperation::AddSubtract {
                rd: 0,
                rn: 2,
                operand: 3,
                immediate: false,
                subtract: true,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(2, 1);
        state.set_constant(3, 2);
        pending.clear();
        incoming.clear();
        process_thumb_instruction(
            &image,
            register_subtract,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)].registers[0],
            AbstractValue::constant(u32::MAX)
        );
    }

    #[test]
    fn out_of_rom_thumb_literal_is_rejected_during_discovery() {
        let mut bytes = thumb_ldr_literal_rom().bytes().to_vec();
        bytes[0x86..0x88].copy_from_slice(&0x49FF_u16.to_le_bytes());
        bytes[0xBD] = header_checksum(&bytes);
        let image = RomImage::from_bytes(bytes).unwrap();
        let error = discover_arm_program(&image).unwrap_err();
        assert!(error.contains("thumb_rom_literal_out_of_rom at 0x08000086"));
        assert!(error.contains("address 0x08000484"));
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

    #[test]
    fn discovers_and_verifies_arm_thumb_arm_bx_interworking() {
        let program = discover_arm_program(&bx_interwork_rom()).unwrap();
        assert_eq!(program.instructions.len(), 5);
        assert_eq!(program.thumb_instructions.len(), 1);
        assert_eq!(program.branch_targets.len(), 2);
        assert!(program.branch_targets.contains(&StaticBranchTarget {
            source_mode: IsaMode::Arm,
            source_pc: ROM_BASE + 0xC8,
            target_mode: IsaMode::Thumb,
            target_pc: ROM_BASE + 0x84,
        }));
        assert!(program.branch_targets.contains(&StaticBranchTarget {
            source_mode: IsaMode::Thumb,
            source_pc: ROM_BASE + 0x84,
            target_mode: IsaMode::Arm,
            target_pc: ROM_BASE + 0xCC,
        }));
        assert!(program.verify().is_ok());
    }

    #[test]
    fn discovers_thumb_register_ram_reset_as_aot_and_tracks_fallthrough() {
        let mut bytes = bx_interwork_rom().bytes().to_vec();
        bytes[0x84..0x86].copy_from_slice(&0xDF01_u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&0x4708_u16.to_le_bytes()); // BX r1.
        bytes[0xBD] = header_checksum(&bytes);
        let image = RomImage::from_bytes(bytes).unwrap();
        let program = discover_arm_program(&image).unwrap();
        assert_eq!(
            program.thumb_instructions[0].operation,
            ThumbOperation::Swi { immediate: 1 }
        );
        assert_eq!(program.thumb_instructions[0].guest_pc, ROM_BASE + 0x84);
        assert_eq!(program.thumb_instructions[1].guest_pc, ROM_BASE + 0x86);
        assert!(program.verify().is_ok());
    }

    #[test]
    fn rejects_other_thumb_swi_immediates_during_discovery() {
        let mut bytes = bx_interwork_rom().bytes().to_vec();
        bytes[0x84..0x86].copy_from_slice(&0xDF02_u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&0x4708_u16.to_le_bytes()); // BX r1.
        bytes[0xBD] = header_checksum(&bytes);
        let image = RomImage::from_bytes(bytes).unwrap();
        let error = discover_arm_program(&image).unwrap_err();
        assert!(error.contains("swi_unsupported at 0x08000084: Thumb SWI #2"));
    }

    #[test]
    fn discovers_thumb_bl_call_and_bx_lr_return_without_host_abi_clobbers() {
        let program = discover_arm_program(&thumb_bl_rom()).unwrap();
        let prefix = program
            .thumb_instructions
            .iter()
            .find(|instruction| instruction.guest_pc == ROM_BASE + 0x84)
            .unwrap();
        let call = program
            .thumb_instructions
            .iter()
            .find(|instruction| instruction.guest_pc == ROM_BASE + 0x86)
            .unwrap();
        assert_eq!(
            prefix.operation,
            ThumbOperation::BlPrefix {
                lr_partial: ROM_BASE + 0x88,
            }
        );
        assert_eq!(
            call.operation,
            ThumbOperation::BlCall {
                target: ROM_BASE + 0x90,
                expected_lr: ROM_BASE + 0x88,
                return_address: ROM_BASE + 0x89,
            }
        );
        assert_eq!(program.instructions.len(), 5);
        assert_eq!(program.thumb_instructions.len(), 6);
        assert!(program.branch_targets.contains(&StaticBranchTarget {
            source_mode: IsaMode::Thumb,
            source_pc: ROM_BASE + 0x92,
            target_mode: IsaMode::Thumb,
            target_pc: ROM_BASE + 0x88,
        }));
        assert!(program.verify().is_ok());
    }

    #[test]
    fn rejects_broken_unpaired_and_out_of_rom_thumb_bl_pairs() {
        let base = thumb_bl_rom().bytes().to_vec();
        for (encoding, expected) in [
            (0x2001_u16, "thumb_bl_pair_broken at 0x08000084"),
            (0xF87F_u16, "branch_target_out_of_rom at 0x08000086"),
        ] {
            let mut bytes = base.clone();
            bytes[0x84..0x86].copy_from_slice(&0xF000_u16.to_le_bytes());
            bytes[0x86..0x88].copy_from_slice(&encoding.to_le_bytes());
            bytes[0xBD] = header_checksum(&bytes);
            let error = discover_arm_program(&RomImage::from_bytes(bytes).unwrap()).unwrap_err();
            assert!(error.contains(expected), "unexpected error: {error}");
        }

        let mut bytes = base;
        bytes[0x84..0x86].copy_from_slice(&0xF800_u16.to_le_bytes());
        bytes[0xBD] = header_checksum(&bytes);
        let error = discover_arm_program(&RomImage::from_bytes(bytes).unwrap()).unwrap_err();
        assert!(error.contains("thumb_bl_suffix_without_prefix at 0x08000084"));
    }

    #[test]
    fn rejects_thumb_bl_suffix_when_incoming_lr_does_not_match_its_prefix() {
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x86,
            encoding: 0xF804,
            operation: ThumbOperation::BlCall {
                target: ROM_BASE + 0x90,
                expected_lr: ROM_BASE + 0x88,
                return_address: ROM_BASE + 0x89,
            },
        };
        let mut state = AbstractState::unknown();
        state.registers[14] = AbstractValue::constant(ROM_BASE + 0xA0);
        let error = process_thumb_instruction(
            &thumb_bl_rom(),
            instruction,
            &state,
            &mut VecDeque::new(),
            &mut BTreeMap::new(),
            &mut BTreeSet::new(),
        )
        .unwrap_err();
        assert!(error.contains("thumb_bl_lr_mismatch at 0x08000086"));
    }

    #[test]
    fn rejects_branch_exchange_when_target_is_not_statically_bounded() {
        let mut bytes = counter_rom().bytes().to_vec();
        bytes[0xC0..0xC4].copy_from_slice(&0xE12F_FF10_u32.to_le_bytes()); // BX r0.
        bytes[0xBD] = header_checksum(&bytes);
        let image = RomImage::from_bytes(bytes).unwrap();
        let error = discover_arm_program(&image).unwrap_err();
        assert!(error.contains("bx_target_unresolved at 0x080000C0"));
    }

    #[test]
    fn rejects_statically_known_branch_exchange_target_outside_rom() {
        let mut bytes = counter_rom().bytes().to_vec();
        bytes[0xC0..0xC4].copy_from_slice(&0xE59F_0008_u32.to_le_bytes());
        bytes[0xC4..0xC8].copy_from_slice(&0xEAFF_FFFF_u32.to_le_bytes()); // B 0x080000C8.
        bytes[0xC8..0xCC].copy_from_slice(&0xE12F_FF10_u32.to_le_bytes()); // BX r0.
        bytes[0xD0..0xD4].copy_from_slice(&0x0900_0001_u32.to_le_bytes());
        bytes[0xBD] = header_checksum(&bytes);
        let image = RomImage::from_bytes(bytes).unwrap();
        let error = discover_arm_program(&image).unwrap_err();
        assert!(error.contains("branch_target_out_of_rom at 0x080000C8"));
        assert!(error.contains("Thumb target 0x09000000"));
    }

    #[test]
    fn discovers_thumb_push_fallthrough_and_preserves_bx_target_registers() {
        let mut bytes = bx_interwork_rom().bytes().to_vec();
        bytes[0x84..0x86].copy_from_slice(&0xB5F0_u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&0x4708_u16.to_le_bytes()); // BX r1.
        bytes[0xBD] = header_checksum(&bytes);
        let image = RomImage::from_bytes(bytes).unwrap();
        let program = discover_arm_program(&image).unwrap();
        assert_eq!(program.thumb_instructions.len(), 2);
        assert_eq!(
            program.thumb_instructions[0].operation,
            ThumbOperation::Push {
                register_list: 0x01F0
            }
        );
        assert_eq!(program.thumb_instructions[0].guest_pc, ROM_BASE + 0x84);
        assert_eq!(program.thumb_instructions[1].guest_pc, ROM_BASE + 0x86);
        assert!(program.verify().is_ok());
    }

    #[test]
    fn tracks_thumb_high_mov_constants_into_an_indirect_branch() {
        let mut bytes = bx_interwork_rom().bytes().to_vec();
        bytes[0x84..0x86].copy_from_slice(&0x460A_u16.to_le_bytes()); // MOV r2, r1.
        bytes[0x86..0x88].copy_from_slice(&0x4710_u16.to_le_bytes()); // BX r2.
        bytes[0xBD] = header_checksum(&bytes);
        let image = RomImage::from_bytes(bytes).unwrap();
        let program = discover_arm_program(&image).unwrap();
        assert_eq!(program.thumb_instructions.len(), 2);
        assert_eq!(
            program.thumb_instructions[0].operation,
            ThumbOperation::MovRegister { rd: 2, rm: 1 }
        );
        assert!(program.branch_targets.contains(&StaticBranchTarget {
            source_mode: IsaMode::Thumb,
            source_pc: ROM_BASE + 0x86,
            target_mode: IsaMode::Arm,
            target_pc: ROM_BASE + 0xCC,
        }));
        assert!(program.verify().is_ok());
    }

    #[test]
    fn propagates_thumb_movs_immediate_to_the_next_guest_state() {
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x20FF,
            operation: ThumbOperation::MovImmediate { rd: 0, value: 0xFF },
        };
        let image = bx_interwork_rom();
        let state = AbstractState::unknown();
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &image,
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(pending, VecDeque::from([(IsaMode::Thumb, ROM_BASE + 0x86)]));
        assert_eq!(
            incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)].registers[0],
            AbstractValue::constant(0xFF)
        );
    }

    #[test]
    fn propagates_thumb_lsl_constants_and_invalidates_unknown_destinations() {
        let image = bx_interwork_rom();
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x04C8,
            operation: ThumbOperation::LslImmediate {
                rd: 0,
                rs: 1,
                amount: 19,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(0, 0xDEAD_BEEF);
        state.set_constant(1, 0x0000_1000);
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &image,
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        let next_state = &incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)];
        assert_eq!(
            next_state.registers[0],
            AbstractValue::constant(0x8000_0000)
        );
        assert_eq!(
            next_state.registers[1],
            AbstractValue::constant(0x0000_1000)
        );

        state.registers[1] = AbstractValue::Unknown;
        pending.clear();
        incoming.clear();
        process_thumb_instruction(
            &image,
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)].registers[0],
            AbstractValue::Unknown
        );
    }
}

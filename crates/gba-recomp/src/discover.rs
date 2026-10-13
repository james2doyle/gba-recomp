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
                    let instruction = crate::decoder::decode_thumb(pc, encoding)?;
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
                let mut target_state = state.clone();
                if rm == 14 {
                    // Treat BX LR as a return boundary until call summaries exist.
                    target_state.invalidate_all();
                }
                enqueue_state(target_mode, target_pc, target_state, pending, incoming);
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
                let mut target_state = state.clone();
                if rm == 14 {
                    target_state.invalidate_all();
                }
                enqueue_state(target_mode, target_pc, target_state, pending, incoming);
            }
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
}

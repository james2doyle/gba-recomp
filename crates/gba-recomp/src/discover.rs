use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use gba_ir::{
    ArmInstruction, ArmOperation, ArmProgram, GBA_IR_VERSION, IsaMode, REG_PC, StaticBranchTarget,
    ThumbInstruction, ThumbOperation,
};

use crate::rom::{MAX_ROM_SIZE, ROM_BASE, RomImage};

const MAX_DISCOVERED_INSTRUCTIONS: usize = 1_000_000;
const MAX_CONSTANT_TARGETS: usize = 16;
const MAX_ABSTRACT_MEMORY_WORDS: usize = 64;
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
    resolve_thumb_ldmia_rom(&image, &incoming, &mut decoded_thumb)?;
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

    fn bitwise_or(&self, other: &Self) -> Self {
        let (Self::Constants(left), Self::Constants(right)) = (self, other) else {
            return Self::Unknown;
        };
        let mut values = BTreeSet::new();
        for lhs in left {
            for rhs in right {
                values.insert(*lhs | *rhs);
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
    memory_words: Arc<BTreeMap<u32, AbstractValue>>,
}

impl AbstractState {
    fn unknown() -> Self {
        Self {
            registers: std::array::from_fn(|_| AbstractValue::Unknown),
            memory_words: Arc::new(BTreeMap::new()),
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
        let addresses = self.memory_words.keys().copied().collect::<Vec<_>>();
        for address in addresses {
            let Some(incoming_value) = other.memory_words.get(&address) else {
                Arc::make_mut(&mut self.memory_words).remove(&address);
                changed = true;
                continue;
            };
            let current = self
                .memory_words
                .get(&address)
                .expect("address came from this state's memory map");
            let merged = current.merged(incoming_value);
            if merged == AbstractValue::Unknown {
                Arc::make_mut(&mut self.memory_words).remove(&address);
                changed = true;
            } else if *current != merged {
                Arc::make_mut(&mut self.memory_words).insert(address, merged);
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
        self.clear_memory();
    }

    fn clear_memory(&mut self) {
        self.memory_words = Arc::new(BTreeMap::new());
    }

    fn set_memory_word(&mut self, address: u32, value: AbstractValue) {
        match value {
            AbstractValue::Unknown => {
                Arc::make_mut(&mut self.memory_words).remove(&address);
            }
            AbstractValue::Constants(values) if !values.is_empty() => {
                Arc::make_mut(&mut self.memory_words)
                    .insert(address, AbstractValue::Constants(values));
            }
            AbstractValue::Constants(_) => {
                Arc::make_mut(&mut self.memory_words).remove(&address);
            }
        }
        if self.memory_words.len() > MAX_ABSTRACT_MEMORY_WORDS {
            self.clear_memory();
        }
    }

    fn push_words(&mut self, old_sp: &AbstractValue, values: &[AbstractValue]) {
        let Some(stack_pointers) = old_sp.values() else {
            self.clear_memory();
            return;
        };
        if stack_pointers.len() != 1 || values.is_empty() {
            self.clear_memory();
            return;
        }

        let old_sp = *stack_pointers.iter().next().expect("one SP value");
        let Some(byte_count) = u32::try_from(values.len())
            .ok()
            .and_then(|count| count.checked_mul(4))
        else {
            self.clear_memory();
            return;
        };
        let Some(new_sp) = old_sp.checked_sub(byte_count) else {
            self.clear_memory();
            return;
        };
        if old_sp & 3 != 0 || !supported_ram_interval(new_sp, byte_count) {
            self.clear_memory();
            return;
        }

        self.invalidate_word_bytes(new_sp, byte_count);
        let known_values = values
            .iter()
            .filter(|value| !matches!(value, AbstractValue::Unknown))
            .count();
        if self.memory_words.len().saturating_add(known_values) > MAX_ABSTRACT_MEMORY_WORDS {
            self.clear_memory();
            return;
        }
        for (index, value) in values.iter().enumerate() {
            let address = new_sp + (index as u32 * 4);
            self.set_memory_word(address, value.clone());
        }
    }

    fn pop_word(&self, stack_pointers: &AbstractValue, offset: u32) -> AbstractValue {
        let Some(stack_pointers) = stack_pointers.values() else {
            return AbstractValue::Unknown;
        };
        let mut result: Option<AbstractValue> = None;
        for stack_pointer in stack_pointers {
            let Some(address) = stack_pointer.checked_add(offset) else {
                return AbstractValue::Unknown;
            };
            if address & 3 != 0 || !supported_ram_interval(address, 4) {
                return AbstractValue::Unknown;
            }
            let Some(value) = self.memory_words.get(&address) else {
                return AbstractValue::Unknown;
            };
            result = Some(match result {
                Some(current) => current.merged(value),
                None => value.clone(),
            });
        }
        result.unwrap_or(AbstractValue::Unknown)
    }

    fn load_memory_word(&self, address: u32) -> AbstractValue {
        if address & 3 != 0 || !supported_ram_interval(address, 4) {
            return AbstractValue::Unknown;
        }
        self.memory_words
            .get(&address)
            .cloned()
            .unwrap_or(AbstractValue::Unknown)
    }

    fn forget_popped_words(&mut self, stack_pointers: &AbstractValue, byte_count: u32) {
        let Some(stack_pointers) = stack_pointers.values() else {
            return;
        };
        let mut addresses = BTreeSet::new();
        for stack_pointer in stack_pointers {
            for offset in (0..byte_count).step_by(4) {
                if let Some(address) = stack_pointer.checked_add(offset) {
                    addresses.insert(address);
                }
            }
        }
        let memory_words = Arc::make_mut(&mut self.memory_words);
        for address in addresses {
            memory_words.remove(&address);
        }
    }

    fn invalidate_store(&mut self, addresses: &AbstractValue, byte_count: u32, align_mask: u32) {
        let Some(addresses) = addresses.values() else {
            self.clear_memory();
            return;
        };
        if addresses.is_empty() || addresses.len() > MAX_CONSTANT_TARGETS {
            self.clear_memory();
            return;
        }

        let mut overwritten_words = BTreeSet::new();
        for raw_address in addresses {
            let address = raw_address & !align_mask;
            if (byte_count == 4 && address & 3 != 0) || !supported_ram_interval(address, byte_count)
            {
                self.clear_memory();
                return;
            }
            for offset in 0..byte_count {
                overwritten_words.insert(address.wrapping_add(offset) & !3);
            }
        }
        let memory_words = Arc::make_mut(&mut self.memory_words);
        for address in overwritten_words {
            memory_words.remove(&address);
        }
    }

    fn invalidate_word_bytes(&mut self, address: u32, byte_count: u32) {
        let memory_words = Arc::make_mut(&mut self.memory_words);
        for offset in 0..byte_count {
            memory_words.remove(&(address.wrapping_add(offset) & !3));
        }
    }
}

fn supported_ram_interval(address: u32, byte_count: u32) -> bool {
    if byte_count == 0 {
        return false;
    }
    let Some(end) = address.checked_add(byte_count) else {
        return false;
    };
    (address >= 0x0200_0000 && end <= 0x0204_0000) || (address >= 0x0300_0000 && end <= 0x0300_8000)
}

fn is_rom_window_address(address: u32) -> bool {
    address >= ROM_BASE && address < ROM_BASE.wrapping_add(MAX_ROM_SIZE as u32)
}

fn resolve_thumb_ldmia_rom(
    image: &RomImage,
    incoming: &BTreeMap<(IsaMode, u32), AbstractState>,
    decoded_thumb: &mut BTreeMap<u32, ThumbInstruction>,
) -> Result<(), String> {
    for (&pc, instruction) in decoded_thumb.iter_mut() {
        let ThumbOperation::LoadMultipleIncrementAfter {
            base_register,
            register_list,
        } = instruction.operation
        else {
            continue;
        };
        let Some(base_values) = incoming
            .get(&(IsaMode::Thumb, pc))
            .expect("decoded instruction must have incoming state")
            .registers[base_register as usize]
            .values()
        else {
            continue;
        };
        if base_values.len() > 1 {
            if base_values
                .iter()
                .any(|address| is_rom_window_address(*address))
            {
                return Err(format!(
                    "thumb_ldmia_rom_base_not_singleton at 0x{pc:08X} (resolve_thumb_ldmia_rom): r{base_register} has multiple possible ROM bases: {base_values:?}"
                ));
            }
            continue;
        }
        let base_address = *base_values.iter().next().expect("one base value");
        if !is_rom_window_address(base_address) {
            continue;
        }

        let mut values_by_register = [0_u32; 8];
        let mut slot = 0_u32;
        for register in 0..8 {
            if register_list & (1 << register) == 0 {
                continue;
            }
            let guest_ea = base_address.wrapping_add(slot.wrapping_mul(4)) & !3;
            values_by_register[register] = image.read_arm_word(guest_ea).map_err(|error| {
                format!(
                    "thumb_ldmia_rom_load_out_of_image at 0x{pc:08X}: guest address 0x{guest_ea:08X}: {error}"
                )
            })?;
            slot = slot.wrapping_add(1);
        }
        instruction.operation = ThumbOperation::LoadRomMultipleIncrementAfter {
            base_register,
            register_list,
            base_address,
            values_by_register,
        };
    }
    Ok(())
}

fn effective_store_address(base: &AbstractValue, offset: u32, add: bool) -> AbstractValue {
    let Some(bases) = base.values() else {
        return AbstractValue::Unknown;
    };
    let mut addresses = BTreeSet::new();
    for base in bases {
        let address = if add {
            base.checked_add(offset)
        } else {
            base.checked_sub(offset)
        };
        let Some(address) = address else {
            return AbstractValue::Unknown;
        };
        addresses.insert(address);
    }
    if addresses.len() > MAX_CONSTANT_TARGETS {
        AbstractValue::Unknown
    } else {
        AbstractValue::Constants(addresses)
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
        ArmOperation::StoreWord { rn, offset, up, .. } => {
            let address =
                effective_store_address(&incoming.registers[rn as usize], u32::from(offset), up);
            result.invalidate_store(&address, 4, 0);
        }
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
        ThumbOperation::LoadByteImmediate { rd, .. } => {
            let mut next_state = state.clone();
            next_state.registers[rd as usize] = AbstractValue::Unknown;
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
        ThumbOperation::OrrRegister { rd, rs } => {
            let result = if rd == rs {
                state.registers[rd as usize].clone()
            } else {
                state.registers[rd as usize].bitwise_or(&state.registers[rs as usize])
            };
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
        ThumbOperation::StoreHalfwordImmediate { rb, imm5, .. } => {
            let mut next_state = state.clone();
            let address =
                effective_store_address(&state.registers[rb as usize], u32::from(imm5) * 2, true);
            next_state.invalidate_store(&address, 2, 1);
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
        ThumbOperation::StoreByteImmediate { rb, imm5, .. } => {
            let mut next_state = state.clone();
            let address =
                effective_store_address(&state.registers[rb as usize], u32::from(imm5), true);
            next_state.invalidate_store(&address, 1, 0);
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
        ThumbOperation::StoreMultipleIncrementAfter {
            base_register,
            register_list,
        } => {
            let old_base = &state.registers[base_register as usize];
            let transfer_bytes = register_list.count_ones().wrapping_mul(4);
            let mut next_state = state.clone();
            next_state.registers[base_register as usize] = old_base.wrapping_add(transfer_bytes);

            let Some(base_values) = old_base.values() else {
                next_state.clear_memory();
                enqueue_state(
                    IsaMode::Thumb,
                    instruction.guest_pc.wrapping_add(2),
                    next_state,
                    pending,
                    incoming,
                );
                return Ok(());
            };
            if base_values.len() != 1 {
                next_state.clear_memory();
                enqueue_state(
                    IsaMode::Thumb,
                    instruction.guest_pc.wrapping_add(2),
                    next_state,
                    pending,
                    incoming,
                );
                return Ok(());
            }

            let aligned_start = *base_values.iter().next().expect("one base value") & !3;
            if !supported_ram_interval(aligned_start, transfer_bytes) {
                next_state.clear_memory();
            } else {
                next_state.invalidate_word_bytes(aligned_start, transfer_bytes);
                let known_values = (0_u8..8)
                    .filter(|register| register_list & (1_u16 << *register) != 0)
                    .filter(|register| {
                        !matches!(state.registers[*register as usize], AbstractValue::Unknown)
                    })
                    .count();
                if next_state.memory_words.len().saturating_add(known_values)
                    > MAX_ABSTRACT_MEMORY_WORDS
                {
                    next_state.clear_memory();
                } else {
                    let mut slot = 0_u32;
                    for register in 0..8 {
                        if register_list & (1 << register) == 0 {
                            continue;
                        }
                        let address = aligned_start + slot * 4;
                        next_state.set_memory_word(address, state.registers[register].clone());
                        slot += 1;
                    }
                }
            }
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
        ThumbOperation::ConditionalBranch { target, .. } => {
            let fallthrough = instruction.guest_pc.wrapping_add(2);
            ensure_target_in_image(image, instruction.guest_pc, IsaMode::Thumb, target)?;
            ensure_target_in_image(image, instruction.guest_pc, IsaMode::Thumb, fallthrough)?;
            // Static discovery keeps both successors and does not reason about NZCV.
            enqueue_state(IsaMode::Thumb, target, state.clone(), pending, incoming);
            enqueue_state(
                IsaMode::Thumb,
                fallthrough,
                state.clone(),
                pending,
                incoming,
            );
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
            let mut next_state = state.clone();
            next_state.clear_memory();
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
        ThumbOperation::Push { register_list } => {
            let bytes = register_list.count_ones().wrapping_mul(4);
            let mut next_state = state.clone();
            let mut values = Vec::new();
            for register in 0..8 {
                if register_list & (1 << register) != 0 {
                    values.push(state.registers[register].clone());
                }
            }
            if register_list & (1 << 8) != 0 {
                values.push(state.registers[14].clone());
            }
            next_state.push_words(&state.registers[13], &values);
            next_state.registers[13] = state.registers[13].wrapping_add(0_u32.wrapping_sub(bytes));
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
        ThumbOperation::Pop { register_list } => {
            let mut next_state = state.clone();
            let stack_pointer = &state.registers[13];
            let mut offset = 0_u32;
            for register in 0..8 {
                if register_list & (1 << register) != 0 {
                    next_state.registers[register] = state.pop_word(stack_pointer, offset);
                    offset = offset.wrapping_add(4);
                }
            }
            let bytes = register_list.count_ones().wrapping_mul(4);
            next_state.forget_popped_words(stack_pointer, bytes);
            next_state.registers[13] = state.registers[13].wrapping_add(bytes);
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
        ThumbOperation::LoadMultipleIncrementAfter {
            base_register,
            register_list,
        } => {
            let base = &state.registers[base_register as usize];
            if let Some(base_values) = base.values() {
                if base_values.len() > 1
                    && base_values
                        .iter()
                        .any(|value| is_rom_window_address(*value))
                {
                    return Err(format!(
                        "thumb_ldmia_rom_base_not_singleton at 0x{:08X} (process_thumb_instruction): r{base_register} has multiple possible ROM bases: {base_values:?}",
                        instruction.guest_pc
                    ));
                }
            }
            let mut loaded: [AbstractValue; 8] = std::array::from_fn(|_| AbstractValue::Unknown);
            let mut slot = 0_u32;
            for register in 0..8 {
                if register_list & (1 << register) == 0 {
                    continue;
                }
                let offset = slot.wrapping_mul(4);
                loaded[register] = match base.values() {
                    Some(base_values) => {
                        let mut merged: Option<AbstractValue> = None;
                        for base_address in base_values {
                            let guest_ea = base_address.wrapping_add(offset) & !3;
                            let value = if is_rom_window_address(guest_ea) {
                                let rom_word = image.read_arm_word(guest_ea).map_err(|error| {
                                    format!(
                                        "thumb_ldmia_rom_load_out_of_image at 0x{:08X}: guest address 0x{guest_ea:08X}: {error}",
                                        instruction.guest_pc
                                    )
                                })?;
                                AbstractValue::constant(rom_word)
                            } else {
                                state.load_memory_word(guest_ea)
                            };
                            merged = Some(match merged {
                                Some(current) => current.merged(&value),
                                None => value,
                            });
                        }
                        merged.unwrap_or(AbstractValue::Unknown)
                    }
                    None => AbstractValue::Unknown,
                };
                slot = slot.wrapping_add(1);
            }

            let mut next_state = state.clone();
            for register in 0..8 {
                if register_list & (1 << register) != 0 {
                    next_state.registers[register] = loaded[register].clone();
                }
            }
            if register_list & (1 << base_register) == 0 {
                next_state.registers[base_register as usize] =
                    base.wrapping_add(register_list.count_ones().wrapping_mul(4));
            }
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
            );
        }
        ThumbOperation::LoadRomMultipleIncrementAfter {
            base_register,
            register_list,
            base_address,
            values_by_register,
        } => {
            let mut next_state = state.clone();
            for register in 0..8 {
                if register_list & (1 << register) != 0 {
                    next_state.registers[register] =
                        AbstractValue::constant(values_by_register[register]);
                }
            }
            if register_list & (1 << base_register) == 0 {
                next_state.registers[base_register as usize] = AbstractValue::constant(
                    base_address.wrapping_add(register_list.count_ones().wrapping_mul(4)),
                );
            }
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
        ThumbOperation::CompareImmediate { .. } => {
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                state.clone(),
                pending,
                incoming,
            );
        }
        ThumbOperation::AddSubtractImmediate {
            rd,
            immediate,
            subtract,
        } => {
            let result = state.registers[rd as usize]
                .wrapping_add_subtract(&AbstractValue::constant(u32::from(immediate)), subtract);
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

    fn thumb_conditional_branch_rom() -> RomImage {
        let mut bytes = bx_interwork_rom().bytes().to_vec();
        bytes[0x84..0x86].copy_from_slice(&0xD001_u16.to_le_bytes()); // BEQ to 0x0800008A.
        bytes[0x86..0x88].copy_from_slice(&0x4708_u16.to_le_bytes()); // Fallthrough: BX r1.
        bytes[0x8A..0x8C].copy_from_slice(&0x4708_u16.to_le_bytes()); // Target: BX r1.
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

    fn thumb_ldmia_rom_word() -> RomImage {
        let mut bytes = counter_rom().bytes().to_vec();
        bytes[0xF0..0xF4].copy_from_slice(&0x0800_0089_u32.to_le_bytes());
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

    fn thumb_stack_return_rom(known_lr: bool) -> RomImage {
        let mut bytes = counter_rom().bytes().to_vec();
        let arm = [
            0xE59F_001C_u32, // LDR r0,[pc,#0x1C] -> stack pointer at 0x080000E4.
            0xE1A0_D000,     // MOV sp,r0.
            if known_lr { 0xE59F_E010 } else { 0xE1A0_0000 }, // Load a fixed return address, or preserve unknown LR.
            0xE59F_1008, // LDR r1,[pc,#8] -> Thumb entry at 0x08000085.
            0xE59F_2010, // LDR r2,[pc,#0x10] -> return loop at 0x080000A1.
            0xE12F_FF11, // BX r1.
            0xEAFF_FFFE, // B self; unreachable from this entry.
        ];
        for (index, word) in arm.into_iter().enumerate() {
            let offset = 0xC0 + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes[0xDC..0xE0].copy_from_slice(&0x0800_0085_u32.to_le_bytes());
        bytes[0xE0..0xE4].copy_from_slice(&0x0800_0099_u32.to_le_bytes());
        bytes[0xE4..0xE8].copy_from_slice(&0x0200_0100_u32.to_le_bytes());
        bytes[0xE8..0xEC].copy_from_slice(&0x0800_00A1_u32.to_le_bytes());

        let thumb = [
            0xB501_u16, // PUSH {r0,lr}.
            0x20FF,     // MOVS r0,#0xFF.
            0x4669,     // MOV r1,sp.
            0x7008,     // STRB r0,[r1].
            0xBC08,     // POP {r3}; this slot was partially overwritten.
            0xBC01,     // POP {r0}; restores the saved LR.
            0x4700,     // BX r0.
        ];
        for (index, halfword) in thumb.into_iter().enumerate() {
            let offset = 0x84 + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&halfword.to_le_bytes());
        }
        bytes[0x98..0x9A].copy_from_slice(&0x4710_u16.to_le_bytes()); // BX r2 to return loop.
        bytes[0xA0..0xA2].copy_from_slice(&0x4710_u16.to_le_bytes()); // BX r2 to self.
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
    fn discovers_thumb_conditional_branch_target_and_fallthrough() {
        let program = discover_arm_program(&thumb_conditional_branch_rom()).unwrap();
        let branch = program
            .thumb_instructions
            .iter()
            .find(|instruction| instruction.guest_pc == ROM_BASE + 0x84)
            .unwrap();
        assert_eq!(
            branch.operation,
            ThumbOperation::ConditionalBranch {
                condition: 0,
                target: ROM_BASE + 0x8A,
            }
        );
        for pc in [ROM_BASE + 0x86, ROM_BASE + 0x8A] {
            assert!(
                program
                    .thumb_instructions
                    .iter()
                    .any(|instruction| instruction.guest_pc == pc),
                "conditional successor 0x{pc:08X} was not discovered"
            );
        }
        assert!(program.verify().is_ok());
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
    fn thumb_ldmia_reads_known_ram_shadow_and_preserves_memory_facts() {
        let image = counter_rom();
        let guest_pc = 0x0800_0084;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xCB0A, // LDMIA r3!, {r1,r3}.
            operation: ThumbOperation::LoadMultipleIncrementAfter {
                base_register: 3,
                register_list: 0x0A,
            },
        };
        let mut state = AbstractState::unknown();
        state.registers[3] = AbstractValue::constant(0x0300_0001);
        state.set_memory_word(0x0300_0000, AbstractValue::constant(0x1111_1111));
        state.set_memory_word(0x0300_0004, AbstractValue::constant(0x3333_3333));
        let memory_before = state.memory_words.clone();
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), state.clone())]);
        let mut pending = VecDeque::new();
        let mut branch_targets = BTreeSet::new();

        process_thumb_instruction(
            &image,
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut branch_targets,
        )
        .unwrap();

        let after = incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap();
        assert_eq!(after.registers[1], AbstractValue::constant(0x1111_1111));
        assert_eq!(after.registers[3], AbstractValue::constant(0x3333_3333));
        assert_eq!(after.memory_words, memory_before);
    }

    #[test]
    fn thumb_stmia_updates_known_ram_shadow_and_base_writeback() {
        let image = counter_rom();
        let guest_pc = 0x0800_0084;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xC221, // STMIA r2!, {r0,r5}.
            operation: ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 0x21,
            },
        };
        let mut state = AbstractState::unknown();
        state.registers[0] = AbstractValue::constant(0x1111_1111);
        state.registers[2] = AbstractValue::constant(0x0300_0003);
        state.registers[5] = AbstractValue::constant(0x5555_5555);
        state.set_memory_word(0x0300_0000, AbstractValue::constant(0xAAAA_AAAA));
        state.set_memory_word(0x0300_0004, AbstractValue::constant(0xBBBB_BBBB));
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), state.clone())]);
        let mut pending = VecDeque::new();
        let mut branch_targets = BTreeSet::new();

        process_thumb_instruction(
            &image,
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut branch_targets,
        )
        .unwrap();

        let after = incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap();
        assert_eq!(after.registers[2], AbstractValue::constant(0x0300_000B));
        assert_eq!(
            after.load_memory_word(0x0300_0000),
            AbstractValue::constant(0x1111_1111)
        );
        assert_eq!(
            after.load_memory_word(0x0300_0004),
            AbstractValue::constant(0x5555_5555)
        );
    }

    #[test]
    fn thumb_stmia_clears_memory_shadow_for_unknown_or_unsupported_base() {
        let image = counter_rom();
        let guest_pc = 0x0800_0084;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xC201,
            operation: ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 1,
            },
        };
        for base in [AbstractValue::Unknown, AbstractValue::constant(0x0800_0000)] {
            let mut state = AbstractState::unknown();
            state.registers[0] = AbstractValue::constant(0x1234_5678);
            state.registers[2] = base;
            state.set_memory_word(0x0300_0000, AbstractValue::constant(0xAAAA_AAAA));
            let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), state.clone())]);
            let mut pending = VecDeque::new();
            let mut branch_targets = BTreeSet::new();

            process_thumb_instruction(
                &image,
                instruction,
                &state,
                &mut pending,
                &mut incoming,
                &mut branch_targets,
            )
            .unwrap();

            let after = incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap();
            assert!(after.memory_words.is_empty());
        }
    }

    #[test]
    fn thumb_stmia_shadow_stores_old_base_when_base_is_lowest_member() {
        let image = counter_rom();
        let guest_pc = 0x0800_0084;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xC224, // STMIA r2!, {r2,r5}.
            operation: ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 0x24,
            },
        };
        let mut state = AbstractState::unknown();
        state.registers[2] = AbstractValue::constant(0x0300_0003);
        state.registers[5] = AbstractValue::constant(0x5555_5555);
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), state.clone())]);
        let mut pending = VecDeque::new();
        let mut branch_targets = BTreeSet::new();

        process_thumb_instruction(
            &image,
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut branch_targets,
        )
        .unwrap();

        let after = incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap();
        assert_eq!(after.registers[2], AbstractValue::constant(0x0300_000B));
        assert_eq!(
            after.load_memory_word(0x0300_0000),
            AbstractValue::constant(0x0300_0003)
        );
        assert_eq!(
            after.load_memory_word(0x0300_0004),
            AbstractValue::constant(0x5555_5555)
        );
    }

    #[test]
    fn thumb_stmia_unknown_source_invalidates_only_its_shadow_slot() {
        let image = counter_rom();
        let guest_pc = 0x0800_0084;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xC203, // STMIA r2!, {r0,r1}.
            operation: ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 0x03,
            },
        };
        let mut state = AbstractState::unknown();
        state.registers[2] = AbstractValue::constant(0x0300_0000);
        state.registers[1] = AbstractValue::constant(0x1111_1111);
        state.set_memory_word(0x0300_0000, AbstractValue::constant(0xAAAA_AAAA));
        state.set_memory_word(0x0300_0004, AbstractValue::constant(0xBBBB_BBBB));
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), state.clone())]);
        let mut pending = VecDeque::new();
        let mut branch_targets = BTreeSet::new();

        process_thumb_instruction(
            &image,
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut branch_targets,
        )
        .unwrap();

        let after = incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap();
        assert_eq!(after.load_memory_word(0x0300_0000), AbstractValue::Unknown);
        assert_eq!(
            after.load_memory_word(0x0300_0004),
            AbstractValue::constant(0x1111_1111)
        );
    }

    #[test]
    fn thumb_ldmia_resolves_singleton_rom_base_to_aot_values() {
        let image = thumb_ldmia_rom_word();
        let guest_pc = 0x0800_0084;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xCB01,
            operation: ThumbOperation::LoadMultipleIncrementAfter {
                base_register: 3,
                register_list: 1,
            },
        };
        let mut state = AbstractState::unknown();
        state.registers[3] = AbstractValue::constant(ROM_BASE + 0xF1);
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), state.clone())]);
        let mut pending = VecDeque::new();
        let mut branch_targets = BTreeSet::new();

        process_thumb_instruction(
            &image,
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut branch_targets,
        )
        .unwrap();
        assert_eq!(
            incoming[&(IsaMode::Thumb, guest_pc + 2)].registers[0],
            AbstractValue::constant(0x0800_0089)
        );
        assert_eq!(
            incoming[&(IsaMode::Thumb, guest_pc + 2)].registers[3],
            AbstractValue::constant(ROM_BASE + 0xF5)
        );

        let mut decoded = BTreeMap::from([(guest_pc, instruction)]);
        resolve_thumb_ldmia_rom(&image, &incoming, &mut decoded).unwrap();
        assert_eq!(
            decoded[&guest_pc].operation,
            ThumbOperation::LoadRomMultipleIncrementAfter {
                base_register: 3,
                register_list: 1,
                base_address: ROM_BASE + 0xF1,
                values_by_register: [0x0800_0089, 0, 0, 0, 0, 0, 0, 0],
            }
        );
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
    fn tracks_saved_lr_across_partial_stack_store_and_closes_bx() {
        let program = discover_arm_program(&thumb_stack_return_rom(true)).unwrap();
        assert!(program.branch_targets.contains(&StaticBranchTarget {
            source_mode: IsaMode::Thumb,
            source_pc: ROM_BASE + 0x90,
            target_mode: IsaMode::Thumb,
            target_pc: ROM_BASE + 0x98,
        }));
        assert!(program.thumb_instructions.iter().any(|instruction| {
            instruction.guest_pc == ROM_BASE + 0x90
                && instruction.operation == ThumbOperation::BranchExchange { rm: 0 }
        }));
        assert!(program.verify().is_ok());
    }

    #[test]
    fn unknown_saved_lr_still_rejects_indirect_branch() {
        let error = discover_arm_program(&thumb_stack_return_rom(false)).unwrap_err();
        assert!(error.contains("bx_target_unresolved at 0x08000090: r0 is not statically bounded"));
    }

    #[test]
    fn abstract_memory_merge_requires_a_slot_on_every_path() {
        let address = 0x0200_0100;
        let mut left = AbstractState::unknown();
        left.set_memory_word(address, AbstractValue::constant(0x0800_0085));
        let mut right = left.clone();
        right.set_memory_word(address, AbstractValue::constant(0x0800_0099));
        assert!(left.merge_from(&right));
        assert_eq!(
            left.memory_words[&address],
            AbstractValue::Constants(BTreeSet::from([0x0800_0085, 0x0800_0099]))
        );

        let missing = AbstractState::unknown();
        assert!(left.merge_from(&missing));
        assert!(!left.memory_words.contains_key(&address));
    }

    #[test]
    fn thumb_byte_store_invalidates_only_its_overlapping_word() {
        let address = 0x0200_0100;
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x7088,
            operation: ThumbOperation::StoreByteImmediate {
                rd: 0,
                rb: 1,
                imm5: 2,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(1, address);
        state.set_memory_word(address, AbstractValue::constant(0x0800_0085));
        state.set_memory_word(address + 4, AbstractValue::constant(0x0800_0099));
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &counter_rom(),
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        let successor = &incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)];
        assert!(!successor.memory_words.contains_key(&address));
        assert_eq!(
            successor.memory_words[&(address + 4)],
            AbstractValue::constant(0x0800_0099)
        );
    }

    #[test]
    fn unknown_memory_write_clears_all_abstract_slots() {
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x7008,
            operation: ThumbOperation::StoreByteImmediate {
                rd: 0,
                rb: 1,
                imm5: 0,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_memory_word(0x0200_0100, AbstractValue::constant(0x0800_0085));
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &counter_rom(),
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert!(
            incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]
                .memory_words
                .is_empty()
        );
    }

    #[test]
    fn thumb_halfword_store_aligns_and_invalidates_only_overlapping_word() {
        let address = 0x0200_0100;
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x8008,
            operation: ThumbOperation::StoreHalfwordImmediate {
                rd: 0,
                rb: 1,
                imm5: 0,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(1, address + 3);
        state.set_memory_word(address, AbstractValue::constant(0x0800_0085));
        state.set_memory_word(address + 4, AbstractValue::constant(0x0800_0099));
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &counter_rom(),
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        let successor = &incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)];
        assert!(!successor.memory_words.contains_key(&address));
        assert_eq!(
            successor.memory_words[&(address + 4)],
            AbstractValue::constant(0x0800_0099)
        );
    }

    #[test]
    fn conditional_arm_store_forgets_maybe_overwritten_word() {
        let address = 0x0200_0100;
        let instruction = ArmInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x1581_0000,
            condition: 1,
            operation: ArmOperation::StoreWord {
                rd: 0,
                rn: 1,
                offset: 0,
                up: true,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(1, address);
        state.set_memory_word(address, AbstractValue::constant(0x0800_0085));
        state.set_memory_word(address + 4, AbstractValue::constant(0x0800_0099));
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_arm_instruction(
            &counter_rom(),
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        let successor = &incoming[&(IsaMode::Arm, ROM_BASE + 0x88)];
        assert!(!successor.memory_words.contains_key(&address));
        assert_eq!(
            successor.memory_words[&(address + 4)],
            AbstractValue::constant(0x0800_0099)
        );
    }

    #[test]
    fn register_ram_reset_invalidates_abstract_memory() {
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0xDF01,
            operation: ThumbOperation::Swi { immediate: 1 },
        };
        let mut state = AbstractState::unknown();
        state.set_memory_word(0x0200_0100, AbstractValue::constant(0x0800_0085));
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &counter_rom(),
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert!(
            incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]
                .memory_words
                .is_empty()
        );
    }

    #[test]
    fn abstract_memory_growth_is_bounded_by_clearing_the_shadow() {
        let mut state = AbstractState::unknown();
        for index in 0..MAX_ABSTRACT_MEMORY_WORDS {
            state.set_memory_word(
                0x0200_0100 + index as u32 * 4,
                AbstractValue::constant(index as u32),
            );
        }
        state.push_words(
            &AbstractValue::constant(0x0200_1004),
            &[AbstractValue::constant(0x0800_0085)],
        );
        assert!(state.memory_words.is_empty());
    }

    #[test]
    fn thumb_stmia_clears_shadow_instead_of_repopulating_after_capacity_overflow() {
        let image = counter_rom();
        let guest_pc = ROM_BASE + 0x84;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xC0FF, // STMIA r0!, {r0-r7}; Rb is the lowest member.
            operation: ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 0,
                register_list: 0xFF,
            },
        };
        let mut state = AbstractState::unknown();
        state.registers[0] = AbstractValue::constant(0x0300_0100);
        for register in 1..8 {
            state.registers[register] = AbstractValue::constant(register as u32);
        }
        for index in 0..MAX_ABSTRACT_MEMORY_WORDS {
            state.set_memory_word(
                0x0200_0100 + index as u32 * 4,
                AbstractValue::constant(index as u32),
            );
        }
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), state.clone())]);
        let mut pending = VecDeque::new();
        let mut branch_targets = BTreeSet::new();

        process_thumb_instruction(
            &image,
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut branch_targets,
        )
        .unwrap();

        let after = incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap();
        assert!(after.memory_words.is_empty());
        assert_eq!(after.registers[0], AbstractValue::constant(0x0300_0120));
    }

    #[test]
    fn propagates_thumb_pop_register_and_wrapping_stack_effects() {
        let guest_pc = ROM_BASE + 0x84;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xBC49,
            operation: ThumbOperation::Pop {
                register_list: 0x49,
            },
        };
        let mut state = AbstractState::unknown();
        for register in 0..15 {
            state.set_constant(register, 0x1000 + u32::from(register));
        }
        state.set_constant(13, u32::MAX - 3);

        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &counter_rom(),
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();

        let successor = (IsaMode::Thumb, guest_pc.wrapping_add(2));
        assert_eq!(pending, VecDeque::from([successor]));
        let next = &incoming[&successor];
        for register in 0..8 {
            let expected = if 0x49 & (1 << register) != 0 {
                AbstractValue::Unknown
            } else {
                state.registers[register].clone()
            };
            assert_eq!(next.registers[register], expected, "r{register}");
        }
        assert_eq!(next.registers[13], AbstractValue::constant(8));
        for register in 8..13 {
            assert_eq!(
                next.registers[register], state.registers[register],
                "r{register}"
            );
        }
        assert_eq!(next.registers[14], state.registers[14]);
    }

    #[test]
    fn thumb_pop_preserves_unknown_stack_pointer_abstract_value() {
        let guest_pc = ROM_BASE + 0x84;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xBC08,
            operation: ThumbOperation::Pop {
                register_list: 0x08,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(3, 0x1234_5678);
        state.set_constant(4, 0x8765_4321);
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &counter_rom(),
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();

        let next = &incoming[&(IsaMode::Thumb, guest_pc.wrapping_add(2))];
        assert_eq!(next.registers[3], AbstractValue::Unknown);
        assert_eq!(next.registers[4], state.registers[4]);
        assert_eq!(next.registers[13], AbstractValue::Unknown);
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
    fn enqueues_both_thumb_conditional_successors_with_identical_abstract_state() {
        let guest_pc = ROM_BASE + 0x84;
        let image = counter_rom();
        let mut state = AbstractState::unknown();
        state.set_constant(0, 0x1234_5678);
        state.set_constant(13, 0x0300_7F00);
        let target = (IsaMode::Thumb, guest_pc + 6);
        let fallthrough = (IsaMode::Thumb, guest_pc + 2);
        for condition in 0..=0xD_u8 {
            let instruction = ThumbInstruction {
                guest_pc,
                encoding: 0xD001 | (u16::from(condition) << 8),
                operation: ThumbOperation::ConditionalBranch {
                    condition,
                    target: guest_pc + 6,
                },
            };
            let mut incoming = BTreeMap::new();
            let mut pending = VecDeque::new();
            let mut branch_targets = BTreeSet::new();
            process_thumb_instruction(
                &image,
                instruction,
                &state,
                &mut pending,
                &mut incoming,
                &mut branch_targets,
            )
            .unwrap();
            assert_eq!(pending, VecDeque::from([target, fallthrough]));
            assert_eq!(incoming[&target], state, "condition 0x{condition:X}");
            assert_eq!(incoming[&fallthrough], state, "condition 0x{condition:X}");
            assert!(branch_targets.is_empty());
        }
    }

    #[test]
    fn propagates_thumb_format3_constants_and_preserves_registers_for_cmp() {
        let image = bx_interwork_rom();
        let transfer = |instruction: ThumbInstruction, state: &AbstractState| {
            let mut pending = VecDeque::new();
            let mut incoming = BTreeMap::new();
            process_thumb_instruction(
                &image,
                instruction,
                state,
                &mut pending,
                &mut incoming,
                &mut BTreeSet::new(),
            )
            .unwrap();
            incoming[&(IsaMode::Thumb, instruction.guest_pc.wrapping_add(2))].clone()
        };

        let mov = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x27FF,
            operation: ThumbOperation::MovImmediate { rd: 7, value: 0xFF },
        };
        let next = transfer(mov, &AbstractState::unknown());
        assert_eq!(next.registers[7], AbstractValue::constant(0xFF));

        let add = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x3201,
            operation: ThumbOperation::AddSubtractImmediate {
                rd: 2,
                immediate: 1,
                subtract: false,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(2, u32::MAX);
        let next = transfer(add, &state);
        assert_eq!(next.registers[2], AbstractValue::constant(0));

        let sub = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x3A01,
            operation: ThumbOperation::AddSubtractImmediate {
                rd: 2,
                immediate: 1,
                subtract: true,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(2, 0);
        let next = transfer(sub, &state);
        assert_eq!(next.registers[2], AbstractValue::constant(u32::MAX));

        let mut unknown_state = AbstractState::unknown();
        unknown_state.set_constant(3, 0x1234_5678);
        let next = transfer(add, &unknown_state);
        assert_eq!(next.registers[2], AbstractValue::Unknown);
        assert_eq!(next.registers[3], AbstractValue::constant(0x1234_5678));

        let compare = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x2A01,
            operation: ThumbOperation::CompareImmediate {
                rd: 2,
                immediate: 1,
            },
        };
        let mut compare_state = AbstractState::unknown();
        compare_state.set_constant(2, 0x8000_0000);
        compare_state.set_constant(3, 0x0800_1234);
        let next = transfer(compare, &compare_state);
        assert_eq!(next.registers, compare_state.registers);
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

    #[test]
    fn propagates_thumb_orr_constants_with_deduplication_alias_and_limit() {
        let image = counter_rom();
        let guest_pc = ROM_BASE + 0x84;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x4318,
            operation: ThumbOperation::OrrRegister { rd: 0, rs: 3 },
        };
        let mut state = AbstractState::unknown();
        state.registers[0] = AbstractValue::Constants(BTreeSet::from([1, 0x10]));
        state.registers[3] = AbstractValue::Constants(BTreeSet::from([2, 0x20]));
        state.set_constant(1, 0x0200_0000);
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
        let successor = (IsaMode::Thumb, guest_pc + 2);
        assert_eq!(pending, VecDeque::from([successor]));
        assert_eq!(
            incoming[&successor].registers[0],
            AbstractValue::Constants(BTreeSet::from([3, 0x12, 0x21, 0x30]))
        );
        assert_eq!(incoming[&successor].registers[1], state.registers[1]);
        assert_eq!(incoming[&successor].registers[3], state.registers[3]);

        let mut deduplicated_state = AbstractState::unknown();
        let small_constants: BTreeSet<_> = (0..16).collect();
        deduplicated_state.registers[0] = AbstractValue::Constants(small_constants.clone());
        deduplicated_state.registers[3] = AbstractValue::Constants(small_constants);
        pending.clear();
        incoming.clear();
        process_thumb_instruction(
            &image,
            instruction,
            &deduplicated_state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            incoming[&successor].registers[0],
            AbstractValue::Constants((0..16).collect())
        );

        let alias_instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x4312,
            operation: ThumbOperation::OrrRegister { rd: 2, rs: 2 },
        };
        let mut alias_state = AbstractState::unknown();
        let one_hot_constants: BTreeSet<_> = (0..16).map(|bit| 1_u32 << bit).collect();
        alias_state.registers[2] = AbstractValue::Constants(one_hot_constants.clone());
        pending.clear();
        incoming.clear();
        process_thumb_instruction(
            &image,
            alias_instruction,
            &alias_state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            incoming[&successor].registers[2],
            AbstractValue::Constants(one_hot_constants)
        );

        let mut unknown_state = AbstractState::unknown();
        unknown_state.registers[0] = AbstractValue::constant(1);
        pending.clear();
        incoming.clear();
        process_thumb_instruction(
            &image,
            instruction,
            &unknown_state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(incoming[&successor].registers[0], AbstractValue::Unknown);

        let mut overflow_state = AbstractState::unknown();
        overflow_state.registers[0] = AbstractValue::Constants((0..16).collect());
        overflow_state.registers[3] = AbstractValue::Constants(BTreeSet::from([0, 0x10]));
        pending.clear();
        incoming.clear();
        process_thumb_instruction(
            &image,
            instruction,
            &overflow_state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(incoming[&successor].registers[0], AbstractValue::Unknown);
    }

    #[test]
    fn thumb_strh_preserves_abstract_registers_and_enqueues_pc_plus_two() {
        let guest_pc = ROM_BASE + 0x84;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x8008,
            operation: ThumbOperation::StoreHalfwordImmediate {
                rd: 0,
                rb: 1,
                imm5: 0,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(0, 0xAABB_CCDD);
        state.set_constant(1, 0x0200_0001);
        let before = state.clone();
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &counter_rom(),
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();

        let successor = (IsaMode::Thumb, guest_pc + 2);
        assert_eq!(pending, VecDeque::from([successor]));
        assert_eq!(incoming[&successor], before);
    }

    #[test]
    fn thumb_strb_preserves_abstract_registers_and_enqueues_pc_plus_two() {
        let guest_pc = ROM_BASE + 0x84;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x7005,
            operation: ThumbOperation::StoreByteImmediate {
                rd: 5,
                rb: 0,
                imm5: 0,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(5, 0xAABB_CCDD);
        state.set_constant(0, 0x0200_0001);
        let before = state.clone();
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &counter_rom(),
            instruction,
            &state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();

        let successor = (IsaMode::Thumb, guest_pc + 2);
        assert_eq!(pending, VecDeque::from([successor]));
        assert_eq!(incoming[&successor], before);
    }

    #[test]
    fn thumb_ldrb_marks_only_destination_unknown_and_enqueues_pc_plus_two() {
        let guest_pc = ROM_BASE + 0x84;
        for (encoding, rd, rb, imm5) in [(0x7808, 0, 1, 0), (0x7FC9, 1, 1, 31)] {
            let instruction = ThumbInstruction {
                guest_pc,
                encoding,
                operation: ThumbOperation::LoadByteImmediate { rd, rb, imm5 },
            };
            let mut state = AbstractState::unknown();
            state.set_constant(0, 0x1122_3344);
            state.set_constant(1, 0x0200_0001);
            state.set_constant(2, 0xAABB_CCDD);

            let mut expected = state.clone();
            expected.registers[rd as usize] = AbstractValue::Unknown;
            let mut pending = VecDeque::new();
            let mut incoming = BTreeMap::new();
            process_thumb_instruction(
                &counter_rom(),
                instruction,
                &state,
                &mut pending,
                &mut incoming,
                &mut BTreeSet::new(),
            )
            .unwrap();

            let successor = (IsaMode::Thumb, guest_pc + 2);
            assert_eq!(pending, VecDeque::from([successor]));
            assert_eq!(incoming[&successor], expected);
        }
    }
}

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use gba_ir::{
    ArmInstruction, ArmOperation, ArmProgram, GBA_IR_VERSION, IsaMode, REG_PC, StaticBranchTarget,
    ThumbInstruction, ThumbLdmiaRomCase, ThumbLdmiaRomTable, ThumbOperation,
};

use crate::rom::{MAX_ROM_SIZE, ROM_BASE, RomImage};

const MAX_DISCOVERED_INSTRUCTIONS: usize = 1_000_000;
const MAX_CONSTANT_TARGETS: usize = 16;
const MAX_PATH_ALTERNATIVES_PER_PC: usize = 128;
const MAX_ABSTRACT_STATES: usize = 100_000;
const MAX_ROM_LDMIA_CASES: usize = 16;
const MAX_ABSTRACT_MEMORY_WORDS: usize = 64;
const MAX_ANALYSIS_UPDATES: usize = MAX_DISCOVERED_INSTRUCTIONS * 32;
const IO_REGION_START: u32 = 0x0400_0000;
const IO_REGION_END: u32 = 0x0500_0000;
const IO_IE: u32 = 0x0400_0200;
const IO_IF: u32 = 0x0400_0202;
const IO_WAITCNT: u32 = 0x0400_0204;
const IO_IME: u32 = 0x0400_0208;
const IO_DMA0_START: u32 = 0x0400_00B0;
const IO_DMA3_SAD: u32 = 0x0400_00D4;
const IO_DMA3_DAD: u32 = 0x0400_00D8;
const IO_DMA3_CNT: u32 = 0x0400_00DC;
const IO_DMA_END: u32 = 0x0400_00E0;
const DMA3_ADDRESS_MASK: u32 = 0x0FFF_FFFE;
const DMA3_CONTROL_MASK: u32 = 0x0000_FFE0;
const DMA3_ENABLE: u32 = 0x0000_8000;
const DMA3_SAFE_IMMEDIATE_WORD_CONTROL: u32 = 0x0000_8400;

pub fn discover_arm_program(image: &RomImage) -> Result<ArmProgram, String> {
    discover_arm_program_with_states(image).map(|(program, _)| program)
}

fn discover_arm_program_with_states(
    image: &RomImage,
) -> Result<(ArmProgram, BTreeMap<(IsaMode, u32), Vec<AbstractState>>), String> {
    let entry = (IsaMode::Arm, ROM_BASE);
    let mut pending = VecDeque::new();
    let mut incoming = BTreeMap::<(IsaMode, u32), Vec<AbstractState>>::new();
    let mut total_states = 0usize;
    let mut decoded_arm = BTreeMap::<u32, ArmInstruction>::new();
    let mut decoded_thumb = BTreeMap::<u32, ThumbInstruction>::new();
    let mut branch_targets = BTreeSet::<StaticBranchTarget>::new();
    let mut analysis_updates = 0usize;
    enqueue_state(
        entry.0,
        entry.1,
        AbstractState::unknown(),
        &mut pending,
        &mut incoming,
        &mut total_states,
    )?;

    while let Some(((mode, pc), state)) = pending.pop_front() {
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
                    &mut total_states,
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
                    &mut total_states,
                    &mut branch_targets,
                )?;
            }
        }
    }
    let mut thumb_ldmia_rom_tables = Vec::new();
    resolve_thumb_ldmia_rom(
        &image,
        &incoming,
        &mut decoded_thumb,
        &mut thumb_ldmia_rom_tables,
    )?;
    resolve_thumb_branch_masks(&incoming, &mut decoded_thumb)?;
    resolve_arm_branch_masks(&incoming, &mut decoded_arm)?;
    let program = ArmProgram {
        version: GBA_IR_VERSION,
        instructions: decoded_arm.into_values().collect(),
        thumb_instructions: decoded_thumb.into_values().collect(),
        branch_targets: branch_targets.into_iter().collect(),
        thumb_ldmia_rom_tables,
    };
    program
        .verify()
        .map_err(|error| format!("GBA IR rejected: {error}"))?;
    Ok((program, incoming))
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

    fn bitwise_and(&self, other: &Self) -> Self {
        let (Self::Constants(left), Self::Constants(right)) = (self, other) else {
            return Self::Unknown;
        };
        let mut values = BTreeSet::new();
        for lhs in left {
            for rhs in right {
                values.insert(*lhs & *rhs);
                if values.len() > MAX_CONSTANT_TARGETS {
                    return Self::Unknown;
                }
            }
        }
        Self::Constants(values)
    }

    fn rotate_odd_halfword(&self) -> Self {
        match self {
            Self::Unknown => Self::Unknown,
            Self::Constants(values) => Self::Constants(
                values
                    .iter()
                    .map(|value| (value >> 8) | (value << 24))
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

    fn singleton(&self) -> Option<u32> {
        let values = self.values()?;
        (values.len() == 1).then(|| *values.iter().next().expect("one value"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DmaAbstractState {
    other_channels_quiescent: bool,
    dma3_source: AbstractValue,
    dma3_destination: AbstractValue,
    dma3_control: AbstractValue,
}

impl DmaAbstractState {
    fn zeroed() -> Self {
        Self {
            other_channels_quiescent: true,
            dma3_source: AbstractValue::constant(0),
            dma3_destination: AbstractValue::constant(0),
            dma3_control: AbstractValue::constant(0),
        }
    }

    fn unknown() -> Self {
        Self {
            other_channels_quiescent: false,
            dma3_source: AbstractValue::Unknown,
            dma3_destination: AbstractValue::Unknown,
            dma3_control: AbstractValue::Unknown,
        }
    }

    fn dma3_is_proven_disabled(&self) -> bool {
        self.dma3_control.values().is_some_and(|values| {
            !values.is_empty() && values.iter().all(|value| value & DMA3_ENABLE == 0)
        })
    }

    fn all_channels_are_proven_quiescent(&self) -> bool {
        self.other_channels_quiescent && self.dma3_is_proven_disabled()
    }

    fn invalidate(&mut self) {
        *self = Self::unknown();
    }

    fn reset_after_hle(&mut self) {
        *self = Self::zeroed();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AbstractState {
    registers: [AbstractValue; REG_PC as usize],
    memory_words: Arc<BTreeMap<u32, AbstractValue>>,
    io_halfwords: Arc<BTreeMap<u32, AbstractValue>>,
    dma: DmaAbstractState,
    pending_compare: Option<CompareFact>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CompareFact {
    mode: IsaMode,
    guest_pc: u32,
    register: u8,
    immediate: u32,
}

impl AbstractState {
    fn unknown() -> Self {
        Self {
            registers: std::array::from_fn(|_| AbstractValue::Unknown),
            memory_words: Arc::new(BTreeMap::new()),
            io_halfwords: Arc::new(BTreeMap::from([
                (IO_IE, AbstractValue::constant(0)),
                (IO_IF, AbstractValue::constant(0)),
                (IO_IME, AbstractValue::constant(0)),
            ])),
            // The strict-AOT runner starts with its entire I/O array zeroed.
            // This fact is only sound for that explicit entry contract.
            dma: DmaAbstractState::zeroed(),
            pending_compare: None,
        }
    }

    fn set_constant(&mut self, register: u8, value: u32) {
        if register < REG_PC {
            self.registers[register as usize] = AbstractValue::constant(value);
        }
    }

    fn invalidate_all(&mut self) {
        self.registers.fill(AbstractValue::Unknown);
        self.clear_memory();
        self.clear_io();
        self.dma.invalidate();
    }

    fn clear_memory(&mut self) {
        self.memory_words = Arc::new(BTreeMap::new());
    }

    fn clear_io(&mut self) {
        self.io_halfwords = Arc::new(BTreeMap::new());
    }

    fn io_halfword(&self, address: u32) -> AbstractValue {
        self.io_halfwords
            .get(&address)
            .cloned()
            .unwrap_or(AbstractValue::Unknown)
    }

    fn load_io_halfword(&self, addresses: &AbstractValue) -> AbstractValue {
        let Some(addresses) = addresses.values() else {
            return AbstractValue::Unknown;
        };
        let mut result: Option<AbstractValue> = None;
        for raw_address in addresses {
            let address = raw_address & !1;
            let mut value = self.io_halfword(address);
            if matches!(value, AbstractValue::Unknown) {
                return AbstractValue::Unknown;
            }
            if raw_address & 1 != 0 {
                value = value.rotate_odd_halfword();
            }
            result = Some(match result {
                Some(current) => current.merged(&value),
                None => value,
            });
        }
        result.unwrap_or(AbstractValue::Unknown)
    }

    fn set_io_halfword(&mut self, address: u32, value: AbstractValue) {
        let values = Arc::make_mut(&mut self.io_halfwords);
        match value {
            AbstractValue::Unknown => {
                values.remove(&address);
            }
            AbstractValue::Constants(_) => {
                values.insert(address, value);
            }
        }
    }

    fn store_io_halfword(&mut self, address: u32, value: &AbstractValue) {
        match address {
            IO_IE => {
                self.set_io_halfword(IO_IE, value.bitwise_and(&AbstractValue::constant(0xFFFF)))
            }
            IO_IME => self.set_io_halfword(IO_IME, value.bitwise_and(&AbstractValue::constant(1))),
            _ => self.set_io_halfword(address, AbstractValue::Unknown),
        }
    }

    fn reset_io_after_register_ram_reset(&mut self, flags: &AbstractValue) {
        let (may_reset, may_preserve) = match flags.values() {
            Some(values) => (
                values.iter().any(|value| value & 0x80 != 0),
                values.iter().any(|value| value & 0x80 == 0),
            ),
            None => (true, true),
        };
        for address in [IO_IE, IO_IME] {
            if may_reset {
                let reset_value = AbstractValue::constant(0);
                let next_value = if may_preserve {
                    self.io_halfword(address).merged(&reset_value)
                } else {
                    reset_value
                };
                self.set_io_halfword(address, next_value);
            }
        }
        if may_reset {
            let reset_value = AbstractValue::constant(0);
            let next_if = if may_preserve {
                self.io_halfword(IO_IF).merged(&reset_value)
            } else {
                reset_value
            };
            self.set_io_halfword(IO_IF, next_if);
        }
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
        let Some(byte_count) = u32::try_from(values.len())
            .ok()
            .and_then(|count| count.checked_mul(4))
        else {
            self.invalidate_store(&AbstractValue::Unknown, u32::MAX, 0);
            return;
        };
        let Some(stack_pointers) = old_sp.values() else {
            self.invalidate_store(&AbstractValue::Unknown, byte_count, 0);
            return;
        };
        if stack_pointers.len() != 1 || values.is_empty() {
            self.invalidate_store(&AbstractValue::Unknown, byte_count, 0);
            return;
        }

        let old_sp = *stack_pointers.iter().next().expect("one SP value");
        let new_sp = old_sp.wrapping_sub(byte_count);
        if old_sp & 3 != 0 || !supported_ram_interval(new_sp, byte_count) {
            self.invalidate_store(&AbstractValue::constant(new_sp), byte_count, 0);
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
            self.clear_io();
            self.dma.invalidate();
            return;
        };
        if addresses.is_empty() || addresses.len() > MAX_CONSTANT_TARGETS {
            self.clear_memory();
            self.clear_io();
            self.dma.invalidate();
            return;
        }

        let mut overwritten_words = BTreeSet::new();
        let mut clear_memory = false;
        for raw_address in addresses {
            let address = raw_address & !align_mask;
            self.invalidate_dma_state_for_store(address, byte_count);
            if (byte_count == 4 && address & 3 != 0) || !supported_ram_interval(address, byte_count)
            {
                clear_memory = true;
            } else {
                for offset in 0..byte_count {
                    overwritten_words.insert(address + offset & !3);
                }
            }
            self.invalidate_io_facts_for_store(address, byte_count);
        }
        if clear_memory {
            self.clear_memory();
        } else {
            let memory_words = Arc::make_mut(&mut self.memory_words);
            for address in overwritten_words {
                memory_words.remove(&address);
            }
        }
    }

    fn try_dma3_word_store(
        &mut self,
        address: u32,
        value: &AbstractValue,
        image: &RomImage,
    ) -> bool {
        if address & 3 != 0 || !self.dma.all_channels_are_proven_quiescent() {
            return false;
        }

        match address {
            IO_DMA3_SAD => {
                self.dma.dma3_source = match value {
                    AbstractValue::Unknown => AbstractValue::Unknown,
                    AbstractValue::Constants(values) => AbstractValue::Constants(
                        values
                            .iter()
                            .map(|value| value & DMA3_ADDRESS_MASK)
                            .collect(),
                    ),
                };
                self.clear_memory();
                true
            }
            IO_DMA3_DAD => {
                self.dma.dma3_destination = match value {
                    AbstractValue::Unknown => AbstractValue::Unknown,
                    AbstractValue::Constants(values) => AbstractValue::Constants(
                        values
                            .iter()
                            .map(|value| value & DMA3_ADDRESS_MASK)
                            .collect(),
                    ),
                };
                self.clear_memory();
                true
            }
            IO_DMA3_CNT => {
                let Some(word) = value.singleton() else {
                    return false;
                };
                let count = word & 0xFFFF;
                let control = (word >> 16) & DMA3_CONTROL_MASK;
                if control & DMA3_ENABLE == 0 {
                    self.dma.dma3_control = AbstractValue::constant(control);
                    self.clear_memory();
                    return true;
                }

                if control != DMA3_SAFE_IMMEDIATE_WORD_CONTROL || count == 0 {
                    return false;
                }
                let Some(source) = self.dma.dma3_source.singleton() else {
                    return false;
                };
                let Some(destination) = self.dma.dma3_destination.singleton() else {
                    return false;
                };
                if source & 3 != 0 || destination & 3 != 0 {
                    return false;
                }
                let Some(byte_count) = count.checked_mul(4) else {
                    return false;
                };
                let Some(source_end) = source.checked_add(byte_count) else {
                    return false;
                };
                let Some(destination_end) = destination.checked_add(byte_count) else {
                    return false;
                };
                let Ok(source_length) = usize::try_from(byte_count) else {
                    return false;
                };
                if image.offset_for_guest(source, source_length).is_err()
                    || source_end <= source
                    || destination < 0x0300_0000
                    || destination_end > 0x0300_8000
                {
                    return false;
                }

                // Immediate one-shot DMA blocks the guest CPU until completion.
                // The model records only that it is disabled afterward; copied
                // bytes and timing remain outside this discovery abstraction.
                self.dma.dma3_control = AbstractValue::constant(control & !DMA3_ENABLE);
                self.dma.dma3_source = AbstractValue::Unknown;
                self.dma.dma3_destination = AbstractValue::Unknown;
                self.clear_memory();
                true
            }
            _ => false,
        }
    }

    fn invalidate_dma_state_for_store(&mut self, address: u32, byte_count: u32) {
        let Some(end) = address.checked_add(byte_count) else {
            self.dma.invalidate();
            return;
        };
        if address < IO_DMA_END && IO_DMA0_START < end {
            self.dma.invalidate();
        }
    }

    fn reset_dma_after_register_ram_reset(&mut self, flags: &AbstractValue) {
        match flags.singleton() {
            Some(value) if value & 0x80 != 0 => self.dma.reset_after_hle(),
            Some(_) => {}
            None => {
                if flags
                    .values()
                    .is_none_or(|values| values.iter().any(|value| value & 0x80 != 0))
                {
                    self.dma.invalidate();
                }
            }
        }
    }

    fn invalidate_io_facts_for_store(&mut self, address: u32, byte_count: u32) {
        if !matches!(byte_count, 1 | 2 | 4) {
            self.clear_io();
            return;
        }
        let Some(end) = address.checked_add(byte_count) else {
            self.clear_io();
            return;
        };
        let io_start = address.max(IO_REGION_START);
        let io_end = end.min(IO_REGION_END);
        if io_start >= io_end {
            return;
        }

        let mut covered_until = io_start;
        let mut overlapped_latches = Vec::new();
        let modeled_ranges = [
            (IO_IE, IO_IE + 2, Some(IO_IE)),
            (IO_IF, IO_IF + 2, Some(IO_IF)),
            (IO_WAITCNT, IO_WAITCNT + 2, None),
            (IO_IME, IO_IME + 2, Some(IO_IME)),
        ];
        for (range_start, range_end, latch) in modeled_ranges {
            let overlap_start = io_start.max(range_start);
            let overlap_end = io_end.min(range_end);
            if overlap_start < overlap_end {
                if overlap_start > covered_until {
                    self.clear_io();
                    return;
                }
                covered_until = covered_until.max(overlap_end);
                if let Some(latch) = latch {
                    overlapped_latches.push(latch);
                }
            }
        }

        if covered_until < io_end {
            self.clear_io();
            return;
        }
        for latch in overlapped_latches {
            self.set_io_halfword(latch, AbstractValue::Unknown);
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
    incoming: &BTreeMap<(IsaMode, u32), Vec<AbstractState>>,
    decoded_thumb: &mut BTreeMap<u32, ThumbInstruction>,
    tables: &mut Vec<ThumbLdmiaRomTable>,
) -> Result<(), String> {
    for (&pc, instruction) in decoded_thumb.iter_mut() {
        let ThumbOperation::LoadMultipleIncrementAfter {
            base_register,
            register_list,
        } = instruction.operation
        else {
            continue;
        };
        let states = incoming
            .get(&(IsaMode::Thumb, pc))
            .expect("decoded instruction must have incoming states");
        let mut rom_bases = BTreeSet::new();
        let mut saw_unknown = false;
        let mut saw_non_rom = false;
        for state in states {
            match state.registers[base_register as usize].values() {
                None => saw_unknown = true,
                Some(values) => {
                    for address in values {
                        if is_rom_window_address(*address) {
                            rom_bases.insert(*address);
                        } else {
                            saw_non_rom = true;
                        }
                    }
                }
            }
        }
        if rom_bases.is_empty() {
            continue;
        }
        if saw_unknown || saw_non_rom {
            return Err(format!(
                "thumb_ldmia_rom_candidate_not_finite at 0x{pc:08X}: r{base_register} has ROM bases {rom_bases:?} and also Unknown/non-ROM alternatives"
            ));
        }
        if rom_bases.len() > MAX_ROM_LDMIA_CASES {
            return Err(format!(
                "thumb_ldmia_rom_case_limit at 0x{pc:08X}: {} unique ROM bases exceed limit {MAX_ROM_LDMIA_CASES}: {rom_bases:?}",
                rom_bases.len()
            ));
        }

        let mut cases = Vec::with_capacity(rom_bases.len());
        for base_address in rom_bases {
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
            cases.push(ThumbLdmiaRomCase {
                base_address,
                values_by_register,
            });
        }
        if cases.len() == 1 {
            let case = cases[0];
            instruction.operation = ThumbOperation::LoadRomMultipleIncrementAfter {
                base_register,
                register_list,
                base_address: case.base_address,
                values_by_register: case.values_by_register,
            };
        } else {
            let table_id = u32::try_from(tables.len())
                .map_err(|_| format!("thumb_ldmia_rom_table_limit at 0x{pc:08X}"))?;
            tables.push(ThumbLdmiaRomTable { cases });
            instruction.operation = ThumbOperation::LoadRomMultipleIncrementAfterCases {
                base_register,
                register_list,
                table_id,
            };
        }
    }
    Ok(())
}

fn resolve_thumb_branch_masks(
    incoming: &BTreeMap<(IsaMode, u32), Vec<AbstractState>>,
    decoded_thumb: &mut BTreeMap<u32, ThumbInstruction>,
) -> Result<(), String> {
    for (&pc, instruction) in decoded_thumb.iter_mut() {
        let ThumbOperation::ConditionalBranch {
            condition, target, ..
        } = instruction.operation
        else {
            continue;
        };
        let states = incoming
            .get(&(IsaMode::Thumb, pc))
            .expect("decoded branch must have incoming states");
        let reachable_successors = states.iter().fold(0, |mask, state| {
            mask | compare_branch_successors(
                IsaMode::Thumb,
                pc,
                condition,
                state,
                instruction.guest_pc.wrapping_sub(2),
            )
        });
        if reachable_successors == 0 {
            return Err(format!("thumb_branch_no_reachable_successor at 0x{pc:08X}"));
        }
        instruction.operation = ThumbOperation::ConditionalBranch {
            condition,
            target,
            reachable_successors,
        };
    }
    Ok(())
}

fn resolve_arm_branch_masks(
    incoming: &BTreeMap<(IsaMode, u32), Vec<AbstractState>>,
    decoded_arm: &mut BTreeMap<u32, ArmInstruction>,
) -> Result<(), String> {
    for (&pc, instruction) in decoded_arm.iter_mut() {
        let ArmOperation::Branch { target, link, .. } = instruction.operation else {
            continue;
        };
        let states = incoming
            .get(&(IsaMode::Arm, pc))
            .expect("decoded ARM branch must have incoming states");
        let reachable_successors = states.iter().fold(0, |mask, state| {
            mask | arm_branch_successors(*instruction, state)
        });
        if reachable_successors == 0 {
            return Err(format!("arm_branch_no_reachable_successor at 0x{pc:08X}"));
        }
        instruction.operation = ArmOperation::Branch {
            target,
            link,
            reachable_successors,
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
    mut state: AbstractState,
    pending: &mut VecDeque<((IsaMode, u32), AbstractState)>,
    incoming: &mut BTreeMap<(IsaMode, u32), Vec<AbstractState>>,
    total_states: &mut usize,
) -> Result<(), String> {
    if state.pending_compare.is_some_and(|fact| {
        mode != fact.mode
            || pc
                != fact.guest_pc.wrapping_add(match mode {
                    IsaMode::Arm => 4,
                    IsaMode::Thumb => 2,
                })
    }) {
        state.pending_compare = None;
    }
    let alternatives = expand_finite_state(state, mode, pc)?;
    let key = (mode, pc);
    for alternative in alternatives {
        let states = incoming.entry(key).or_default();
        if states.contains(&alternative) {
            continue;
        }
        if states.len() >= MAX_PATH_ALTERNATIVES_PER_PC {
            return Err(format!(
                "discovery_state_alternative_limit at 0x{pc:08X}: more than {MAX_PATH_ALTERNATIVES_PER_PC} complete states at {mode:?} PC"
            ));
        }
        if *total_states >= MAX_ABSTRACT_STATES {
            return Err(format!(
                "discovery_global_state_limit: more than {MAX_ABSTRACT_STATES} complete abstract states"
            ));
        }
        *total_states += 1;
        states.push(alternative.clone());
        pending.push_back((key, alternative));
    }
    Ok(())
}

fn expand_finite_state(
    state: AbstractState,
    mode: IsaMode,
    pc: u32,
) -> Result<Vec<AbstractState>, String> {
    let mut alternatives = vec![state];
    for register in 0..REG_PC as usize {
        let mut expanded = Vec::new();
        for state in alternatives {
            match &state.registers[register] {
                AbstractValue::Constants(values) if values.len() > 1 => {
                    for value in values {
                        let mut alternative = state.clone();
                        alternative.registers[register] = AbstractValue::constant(*value);
                        expanded.push(alternative);
                        if expanded.len() > MAX_PATH_ALTERNATIVES_PER_PC {
                            return Err(format!(
                                "discovery_state_alternative_limit at 0x{pc:08X}: expanding r{register} exceeds {MAX_PATH_ALTERNATIVES_PER_PC} states in {mode:?} mode"
                            ));
                        }
                    }
                }
                _ => expanded.push(state),
            }
        }
        alternatives = expanded;
    }

    let memory_addresses = alternatives
        .first()
        .map(|state| state.memory_words.keys().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    for address in memory_addresses {
        let mut expanded = Vec::new();
        for state in alternatives {
            match state.memory_words.get(&address) {
                Some(AbstractValue::Constants(values)) if values.len() > 1 => {
                    for value in values {
                        let mut alternative = state.clone();
                        alternative.set_memory_word(address, AbstractValue::constant(*value));
                        expanded.push(alternative);
                        if expanded.len() > MAX_PATH_ALTERNATIVES_PER_PC {
                            return Err(format!(
                                "discovery_state_alternative_limit at 0x{pc:08X}: expanding shadow word 0x{address:08X} exceeds {MAX_PATH_ALTERNATIVES_PER_PC} states in {mode:?} mode"
                            ));
                        }
                    }
                }
                _ => expanded.push(state),
            }
        }
        alternatives = expanded;
    }
    if alternatives.len() > MAX_PATH_ALTERNATIVES_PER_PC {
        return Err(format!(
            "discovery_state_alternative_limit at 0x{pc:08X}: {} states exceed {MAX_PATH_ALTERNATIVES_PER_PC} in {mode:?} mode",
            alternatives.len()
        ));
    }
    Ok(alternatives)
}

fn compare_branch_successors(
    mode: IsaMode,
    branch_pc: u32,
    condition: u8,
    state: &AbstractState,
    compare_pc: u32,
) -> u8 {
    let Some(fact) = state
        .pending_compare
        .filter(|fact| fact.mode == mode && fact.guest_pc == compare_pc)
    else {
        return 0b11;
    };
    let Some(values) = state.registers[fact.register as usize].values() else {
        return 0b11;
    };
    if values.is_empty() {
        return 0b11;
    }
    let mut successors = 0;
    for lhs in values {
        if compare_condition_passed(condition, *lhs, fact.immediate) {
            successors |= 0b01;
        } else {
            successors |= 0b10;
        }
    }
    if successors == 0
        || branch_pc
            != fact.guest_pc.wrapping_add(match mode {
                IsaMode::Arm => 4,
                IsaMode::Thumb => 2,
            })
    {
        0b11
    } else {
        successors
    }
}

fn thumb_branch_successors(instruction: ThumbInstruction, state: &AbstractState) -> u8 {
    let ThumbOperation::ConditionalBranch { condition, .. } = instruction.operation else {
        return 0;
    };
    compare_branch_successors(
        IsaMode::Thumb,
        instruction.guest_pc,
        condition,
        state,
        instruction.guest_pc.wrapping_sub(2),
    )
}

fn arm_branch_successors(instruction: ArmInstruction, state: &AbstractState) -> u8 {
    let ArmOperation::Branch { .. } = instruction.operation else {
        return 0;
    };
    if instruction.condition == 14 {
        return 0b01;
    }
    compare_branch_successors(
        IsaMode::Arm,
        instruction.guest_pc,
        instruction.condition,
        state,
        instruction.guest_pc.wrapping_sub(4),
    )
}

fn compare_condition_passed(condition: u8, lhs: u32, rhs: u32) -> bool {
    let result = lhs.wrapping_sub(rhs);
    let negative = result & 0x8000_0000 != 0;
    let zero = result == 0;
    let carry = lhs >= rhs;
    let overflow = ((lhs ^ rhs) & (lhs ^ result) & 0x8000_0000) != 0;
    match condition {
        0 => zero,
        1 => !zero,
        2 => carry,
        3 => !carry,
        4 => negative,
        5 => !negative,
        6 => overflow,
        7 => !overflow,
        8 => carry && !zero,
        9 => !carry || zero,
        10 => negative == overflow,
        11 => negative != overflow,
        12 => !zero && negative == overflow,
        13 => zero || negative != overflow,
        _ => false,
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
        ArmOperation::CompareImmediate { rn, value } => {
            result.pending_compare = Some(CompareFact {
                mode: IsaMode::Arm,
                guest_pc: instruction.guest_pc,
                register: rn,
                immediate: value,
            });
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

fn process_arm_instruction(
    image: &RomImage,
    instruction: ArmInstruction,
    state: &AbstractState,
    pending: &mut VecDeque<((IsaMode, u32), AbstractState)>,
    incoming: &mut BTreeMap<(IsaMode, u32), Vec<AbstractState>>,
    total_states: &mut usize,
    branch_targets: &mut BTreeSet<StaticBranchTarget>,
) -> Result<(), String> {
    let next_pc = instruction.guest_pc.wrapping_add(4);
    match instruction.operation {
        ArmOperation::Branch { target, link, .. } => {
            let successors = arm_branch_successors(instruction, state);
            if successors & 0b01 != 0 {
                ensure_target_in_image(image, instruction.guest_pc, IsaMode::Arm, target)?;
                let taken_state = transfer_arm(instruction, state);
                enqueue_state(
                    IsaMode::Arm,
                    target,
                    taken_state.clone(),
                    pending,
                    incoming,
                    total_states,
                )?;
                if link {
                    // Model the returned call conservatively without merging it
                    // with the skipped path of a conditional call.
                    let mut returned_state = taken_state;
                    returned_state.invalidate_all();
                    enqueue_state(
                        IsaMode::Arm,
                        next_pc,
                        returned_state,
                        pending,
                        incoming,
                        total_states,
                    )?;
                }
            }
            if successors & 0b10 != 0 && instruction.condition != 14 {
                ensure_target_in_image(image, instruction.guest_pc, IsaMode::Arm, next_pc)?;
                enqueue_state(
                    IsaMode::Arm,
                    next_pc,
                    state.clone(),
                    pending,
                    incoming,
                    total_states,
                )?;
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
                enqueue_state(
                    target_mode,
                    target_pc,
                    state.clone(),
                    pending,
                    incoming,
                    total_states,
                )?;
            }
            if instruction.condition != 14 {
                enqueue_state(
                    IsaMode::Arm,
                    next_pc,
                    state.clone(),
                    pending,
                    incoming,
                    total_states,
                )?;
            }
        }
        _ => {
            let executed = transfer_arm(instruction, state);
            enqueue_state(
                IsaMode::Arm,
                next_pc,
                executed,
                pending,
                incoming,
                total_states,
            )?;
            if instruction.condition != 14 {
                enqueue_state(
                    IsaMode::Arm,
                    next_pc,
                    state.clone(),
                    pending,
                    incoming,
                    total_states,
                )?;
            }
        }
    }
    Ok(())
}

fn process_thumb_instruction(
    image: &RomImage,
    instruction: ThumbInstruction,
    state: &AbstractState,
    pending: &mut VecDeque<((IsaMode, u32), AbstractState)>,
    incoming: &mut BTreeMap<(IsaMode, u32), Vec<AbstractState>>,
    total_states: &mut usize,
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
                total_states,
            )?;
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
                total_states,
            )?;
        }
        ThumbOperation::LoadWordImmediate { rd, .. } => {
            let mut next_state = state.clone();
            next_state.registers[rd as usize] = AbstractValue::Unknown;
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
                total_states,
            )?;
        }
        ThumbOperation::LoadHalfwordImmediate { rd, rb, imm5 } => {
            let mut next_state = state.clone();
            let address =
                effective_store_address(&state.registers[rb as usize], u32::from(imm5) * 2, true);
            next_state.registers[rd as usize] = state.load_io_halfword(&address);
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
                total_states,
            )?;
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
                total_states,
            )?;
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
                total_states,
            )?;
        }
        ThumbOperation::StoreHalfwordImmediate { rd, rb, imm5 } => {
            let mut next_state = state.clone();
            let address =
                effective_store_address(&state.registers[rb as usize], u32::from(imm5) * 2, true);
            let exact_address = address.values().and_then(|values| {
                (values.len() == 1).then(|| *values.iter().next().expect("one address"))
            });
            let store_keeps_irq_quiescent = matches!(exact_address, Some(IO_IE | IO_IME))
                && state.io_halfword(IO_IF) == AbstractValue::constant(0);
            if store_keeps_irq_quiescent {
                next_state.store_io_halfword(
                    exact_address.expect("matched exact IE/IME address") & !1,
                    &state.registers[rd as usize],
                );
            } else {
                next_state.invalidate_store(&address, 2, 1);
            }
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
                total_states,
            )?;
        }
        ThumbOperation::StoreWordImmediate { rd, rb, imm5 } => {
            let mut next_state = state.clone();
            let address =
                effective_store_address(&state.registers[rb as usize], u32::from(imm5) * 4, true);
            let recognized_dma_store = address.singleton().is_some_and(|address| {
                next_state.try_dma3_word_store(address, &state.registers[rd as usize], image)
            });
            if !recognized_dma_store {
                next_state.invalidate_store(&address, 4, 0);
            }
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
                total_states,
            )?;
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
                total_states,
            )?;
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
                next_state.clear_io();
                next_state.dma.invalidate();
                enqueue_state(
                    IsaMode::Thumb,
                    instruction.guest_pc.wrapping_add(2),
                    next_state,
                    pending,
                    incoming,
                    total_states,
                )?;
                return Ok(());
            };
            if base_values.len() != 1 {
                next_state.clear_memory();
                next_state.clear_io();
                next_state.dma.invalidate();
                enqueue_state(
                    IsaMode::Thumb,
                    instruction.guest_pc.wrapping_add(2),
                    next_state,
                    pending,
                    incoming,
                    total_states,
                )?;
                return Ok(());
            }

            let aligned_start = *base_values.iter().next().expect("one base value") & !3;
            next_state.invalidate_dma_state_for_store(aligned_start, transfer_bytes);
            if !supported_ram_interval(aligned_start, transfer_bytes) {
                next_state.clear_memory();
                next_state.clear_io();
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
                total_states,
            )?;
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
                total_states,
            )?;
        }
        ThumbOperation::LsrImmediate { rd, rs, amount } => {
            let shifted_source = match &state.registers[rs as usize] {
                AbstractValue::Unknown => AbstractValue::Unknown,
                AbstractValue::Constants(values) => AbstractValue::Constants(
                    values
                        .iter()
                        .map(|value| if amount == 0 { 0 } else { value >> amount })
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
                total_states,
            )?;
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
                enqueue_state(
                    target_mode,
                    target_pc,
                    state.clone(),
                    pending,
                    incoming,
                    total_states,
                )?;
            }
        }
        ThumbOperation::ConditionalBranch { target, .. } => {
            let fallthrough = instruction.guest_pc.wrapping_add(2);
            let successors = thumb_branch_successors(instruction, state);
            if successors & 0b01 != 0 {
                ensure_target_in_image(image, instruction.guest_pc, IsaMode::Thumb, target)?;
                enqueue_state(
                    IsaMode::Thumb,
                    target,
                    state.clone(),
                    pending,
                    incoming,
                    total_states,
                )?;
            }
            if successors & 0b10 != 0 {
                ensure_target_in_image(image, instruction.guest_pc, IsaMode::Thumb, fallthrough)?;
                enqueue_state(
                    IsaMode::Thumb,
                    fallthrough,
                    state.clone(),
                    pending,
                    incoming,
                    total_states,
                )?;
            }
        }
        ThumbOperation::UnconditionalBranch { target } => {
            ensure_target_in_image(image, instruction.guest_pc, IsaMode::Thumb, target)?;
            enqueue_state(
                IsaMode::Thumb,
                target,
                state.clone(),
                pending,
                incoming,
                total_states,
            )?;
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
            enqueue_state(
                IsaMode::Thumb,
                suffix_pc,
                next_state,
                pending,
                incoming,
                total_states,
            )?;
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
            enqueue_state(
                IsaMode::Thumb,
                target,
                target_state,
                pending,
                incoming,
                total_states,
            )?;
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
            next_state.reset_io_after_register_ram_reset(&state.registers[0]);
            next_state.reset_dma_after_register_ram_reset(&state.registers[0]);
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
                total_states,
            )?;
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
                total_states,
            )?;
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
                total_states,
            )?;
        }
        ThumbOperation::LoadMultipleIncrementAfter {
            base_register,
            register_list,
        } => {
            let base = &state.registers[base_register as usize];
            if let Some(base_values) = base.values().filter(|values| values.len() > 1) {
                for base_address in base_values {
                    let mut singleton_state = state.clone();
                    singleton_state.registers[base_register as usize] =
                        AbstractValue::constant(*base_address);
                    process_thumb_instruction(
                        image,
                        instruction,
                        &singleton_state,
                        pending,
                        incoming,
                        total_states,
                        branch_targets,
                    )?;
                }
                return Ok(());
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
                total_states,
            )?;
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
                total_states,
            )?;
        }
        ThumbOperation::LoadRomMultipleIncrementAfterCases { .. } => {
            return Err(format!(
                "thumb_ldmia_rom_table_reentered_discovery at 0x{:08X}",
                instruction.guest_pc
            ));
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
                total_states,
            )?;
        }
        ThumbOperation::CompareImmediate { rd, immediate } => {
            let mut next_state = state.clone();
            next_state.pending_compare = Some(CompareFact {
                mode: IsaMode::Thumb,
                guest_pc: instruction.guest_pc,
                register: rd,
                immediate: u32::from(immediate),
            });
            enqueue_state(
                IsaMode::Thumb,
                instruction.guest_pc.wrapping_add(2),
                next_state,
                pending,
                incoming,
                total_states,
            )?;
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
                total_states,
            )?;
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
                total_states,
            )?;
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

    fn process_arm_instruction(
        image: &RomImage,
        instruction: ArmInstruction,
        state: &AbstractState,
        pending: &mut VecDeque<((IsaMode, u32), AbstractState)>,
        incoming: &mut BTreeMap<(IsaMode, u32), Vec<AbstractState>>,
        branch_targets: &mut BTreeSet<StaticBranchTarget>,
    ) -> Result<(), String> {
        let mut total_states = incoming.values().map(Vec::len).sum();
        for alternative in expand_finite_state(state.clone(), IsaMode::Arm, instruction.guest_pc)? {
            super::process_arm_instruction(
                image,
                instruction,
                &alternative,
                pending,
                incoming,
                &mut total_states,
                branch_targets,
            )?;
        }
        Ok(())
    }

    fn process_thumb_instruction(
        image: &RomImage,
        instruction: ThumbInstruction,
        state: &AbstractState,
        pending: &mut VecDeque<((IsaMode, u32), AbstractState)>,
        incoming: &mut BTreeMap<(IsaMode, u32), Vec<AbstractState>>,
        branch_targets: &mut BTreeSet<StaticBranchTarget>,
    ) -> Result<(), String> {
        // The production worklist calls the transfer function once per finite
        // path alternative. Mirror that boundary for tests that seed a set.
        let mut total_states = incoming.values().map(Vec::len).sum();
        for alternative in expand_finite_state(state.clone(), IsaMode::Thumb, instruction.guest_pc)?
        {
            super::process_thumb_instruction(
                image,
                instruction,
                &alternative,
                pending,
                incoming,
                &mut total_states,
                branch_targets,
            )?;
        }
        Ok(())
    }

    fn only_state(states: &[AbstractState]) -> &AbstractState {
        assert_eq!(states.len(), 1, "expected a single disjunctive state");
        &states[0]
    }

    fn state_constant(state: &AbstractState, register: usize) -> u32 {
        match &state.registers[register] {
            AbstractValue::Constants(values) if values.len() == 1 => *values.first().unwrap(),
            value => panic!("r{register} is not one exact constant: {value:?}"),
        }
    }

    fn state_with_known_io() -> AbstractState {
        let mut state = AbstractState::unknown();
        state.set_io_halfword(IO_IE, AbstractValue::constant(0x1234));
        state.set_io_halfword(IO_IF, AbstractValue::constant(0x0056));
        state.set_io_halfword(IO_IME, AbstractValue::constant(1));
        state
    }

    fn assert_io_facts(
        state: &AbstractState,
        ie: AbstractValue,
        interrupt_flags: AbstractValue,
        ime: AbstractValue,
    ) {
        assert_eq!(state.io_halfword(IO_IE), ie);
        assert_eq!(state.io_halfword(IO_IF), interrupt_flags);
        assert_eq!(state.io_halfword(IO_IME), ime);
    }

    fn run_thumb_successors(
        image: &RomImage,
        instruction: ThumbInstruction,
        state: &AbstractState,
    ) -> Vec<AbstractState> {
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            image,
            instruction,
            state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        incoming
            .remove(&(IsaMode::Thumb, instruction.guest_pc.wrapping_add(2)))
            .expect("Thumb instruction has sequential successor")
    }

    fn thumb_word_store(pc: u32, rd: u8, imm5: u8) -> ThumbInstruction {
        ThumbInstruction {
            guest_pc: pc,
            encoding: 0x6000 | (u16::from(imm5) << 6) | u16::from(rd),
            operation: ThumbOperation::StoreWordImmediate { rd, rb: 0, imm5 },
        }
    }

    fn pending_keys(pending: &VecDeque<((IsaMode, u32), AbstractState)>) -> Vec<(IsaMode, u32)> {
        pending.iter().map(|(key, _)| *key).collect()
    }

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

    fn thumb_unconditional_branch_rom() -> RomImage {
        let mut bytes = bx_interwork_rom().bytes().to_vec();
        bytes[0x84..0x86].copy_from_slice(&0xE001_u16.to_le_bytes()); // B to 0x0800008A.
        bytes[0x86..0x88].copy_from_slice(&0x4008_u16.to_le_bytes()); // Unsupported if fallthrough is reached.
        bytes[0x8A..0x8C].copy_from_slice(&0x4708_u16.to_le_bytes()); // Target: BX r1.
        bytes[0xBD] = header_checksum(&bytes);
        RomImage::from_bytes(bytes).unwrap()
    }

    fn thumb_96_iteration_loop_rom() -> RomImage {
        let mut bytes = bx_interwork_rom().bytes().to_vec();
        let thumb = [
            0x2200_u16, // MOVS r2,#0.
            0x3201,     // loop: ADDS r2,#1.
            0x2A5F,     // CMP r2,#95.
            0xDDFC,     // BLE loop.
            0x4708,     // BX r1 to the ARM self-loop after r2 reaches 96.
        ];
        for (index, halfword) in thumb.into_iter().enumerate() {
            let offset = 0x84 + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&halfword.to_le_bytes());
        }
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
        bytes[0xF4..0xF8].copy_from_slice(&0xAABB_CCDD_u32.to_le_bytes());
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
        assert!(matches!(
            program
                .instructions
                .iter()
                .find(|instruction| instruction.guest_pc == ROM_BASE + 0xCC)
                .unwrap()
                .operation,
            ArmOperation::Branch {
                reachable_successors: 0b11,
                ..
            }
        ));
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
                reachable_successors: 0b11,
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
    fn discovers_only_the_thumb_unconditional_branch_target() {
        let program = discover_arm_program(&thumb_unconditional_branch_rom()).unwrap();
        let branch = program
            .thumb_instructions
            .iter()
            .find(|instruction| instruction.guest_pc == ROM_BASE + 0x84)
            .unwrap();
        assert_eq!(
            branch.operation,
            ThumbOperation::UnconditionalBranch {
                target: ROM_BASE + 0x8A
            }
        );
        assert!(
            program
                .thumb_instructions
                .iter()
                .any(|instruction| instruction.guest_pc == ROM_BASE + 0x8A)
        );
        assert!(
            !program
                .thumb_instructions
                .iter()
                .any(|instruction| instruction.guest_pc == ROM_BASE + 0x86)
        );
        assert!(program.verify().is_ok());
    }

    #[test]
    fn rejects_thumb_unconditional_branch_target_outside_image() {
        let mut bytes = bx_interwork_rom().bytes().to_vec();
        bytes[0x84..0x86].copy_from_slice(&0xE3FF_u16.to_le_bytes());
        bytes[0xBD] = header_checksum(&bytes);
        let image = RomImage::from_bytes(bytes).unwrap();
        let error = discover_arm_program(&image).unwrap_err();
        assert!(error.contains("branch_target_out_of_rom at 0x08000084"));
        assert!(error.contains("0x08000886"));
    }

    #[test]
    fn discovers_96_state_thumb_loop_and_keeps_exact_counter_values() {
        let (program, incoming) =
            discover_arm_program_with_states(&thumb_96_iteration_loop_rom()).unwrap();
        let loop_states = &incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)];
        let counters = loop_states
            .iter()
            .map(|state| state_constant(state, 2))
            .collect::<BTreeSet<_>>();
        assert_eq!(loop_states.len(), 96);
        assert_eq!(counters, (0..96).collect());

        let branch = program
            .thumb_instructions
            .iter()
            .find(|instruction| instruction.guest_pc == ROM_BASE + 0x8A)
            .unwrap();
        assert!(matches!(
            branch.operation,
            ThumbOperation::ConditionalBranch {
                condition: 13,
                target,
                reachable_successors: 0b11,
            } if target == ROM_BASE + 0x86
        ));

        let mut reversed = incoming.clone();
        reversed
            .get_mut(&(IsaMode::Thumb, ROM_BASE + 0x8A))
            .unwrap()
            .reverse();
        let mut decoded = BTreeMap::from([(
            ROM_BASE + 0x8A,
            ThumbInstruction {
                guest_pc: ROM_BASE + 0x8A,
                encoding: 0xDDFE,
                operation: ThumbOperation::ConditionalBranch {
                    condition: 13,
                    target: ROM_BASE + 0x86,
                    reachable_successors: 0b11,
                },
            },
        )]);
        resolve_thumb_branch_masks(&reversed, &mut decoded).unwrap();
        assert_eq!(
            decoded[&(ROM_BASE + 0x8A)].operation,
            branch.operation,
            "branch masks must not depend on incoming state insertion order"
        );
        assert!(
            program
                .thumb_instructions
                .iter()
                .any(|instruction| instruction.guest_pc == ROM_BASE + 0x8C)
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
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), vec![state.clone()])]);
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

        let after = only_state(incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap());
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
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), vec![state.clone()])]);
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

        let after = only_state(incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap());
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
            let unknown_base = matches!(&base, AbstractValue::Unknown);
            let mut state = AbstractState::unknown();
            state.registers[0] = AbstractValue::constant(0x1234_5678);
            state.registers[2] = base;
            state.set_memory_word(0x0300_0000, AbstractValue::constant(0xAAAA_AAAA));
            let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), vec![state.clone()])]);
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

            let after = only_state(incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap());
            assert!(after.memory_words.is_empty());
            if unknown_base {
                assert!(!after.dma.all_channels_are_proven_quiescent());
            }
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
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), vec![state.clone()])]);
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

        let after = only_state(incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap());
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
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), vec![state.clone()])]);
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

        let after = only_state(incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap());
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
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), vec![state.clone()])]);
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
            only_state(&incoming[&(IsaMode::Thumb, guest_pc + 2)]).registers[0],
            AbstractValue::constant(0x0800_0089)
        );
        assert_eq!(
            only_state(&incoming[&(IsaMode::Thumb, guest_pc + 2)]).registers[3],
            AbstractValue::constant(ROM_BASE + 0xF5)
        );

        let mut decoded = BTreeMap::from([(guest_pc, instruction)]);
        let mut tables = Vec::new();
        resolve_thumb_ldmia_rom(&image, &incoming, &mut decoded, &mut tables).unwrap();
        assert!(tables.is_empty());
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
    fn thumb_ldmia_keeps_register_and_rom_base_correlation() {
        let image = thumb_ldmia_rom_word();
        let guest_pc = ROM_BASE + 0x84;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xCB02, // LDMIA r3!, {r1}.
            operation: ThumbOperation::LoadMultipleIncrementAfter {
                base_register: 3,
                register_list: 0x02,
            },
        };
        let mut first = AbstractState::unknown();
        first.registers[0] = AbstractValue::constant(0x1111_1111);
        first.registers[3] = AbstractValue::constant(ROM_BASE + 0xF1);
        let mut second = AbstractState::unknown();
        second.registers[0] = AbstractValue::constant(0x2222_2222);
        second.registers[3] = AbstractValue::constant(ROM_BASE + 0xF5);

        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        let mut branch_targets = BTreeSet::new();
        for state in [&first, &second] {
            process_thumb_instruction(
                &image,
                instruction,
                state,
                &mut pending,
                &mut incoming,
                &mut branch_targets,
            )
            .unwrap();
        }

        let successor = (IsaMode::Thumb, guest_pc + 2);
        let exact_triples: BTreeSet<_> = incoming[&successor]
            .iter()
            .map(|state| {
                (
                    state_constant(state, 0),
                    state_constant(state, 1),
                    state_constant(state, 3),
                )
            })
            .collect();
        assert_eq!(
            exact_triples,
            BTreeSet::from([
                (0x1111_1111, 0x0800_0089, ROM_BASE + 0xF5),
                (0x2222_2222, 0xAABB_CCDD, ROM_BASE + 0xF9),
            ])
        );
        assert_eq!(incoming[&successor].len(), 2);
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn thumb_ldmia_rom_tables_are_finite_bounded_and_reject_mixed_bases() {
        let image = thumb_ldmia_rom_word();
        let guest_pc = ROM_BASE + 0x84;
        let instruction = ThumbInstruction {
            guest_pc,
            encoding: 0xCB01,
            operation: ThumbOperation::LoadMultipleIncrementAfter {
                base_register: 3,
                register_list: 1,
            },
        };

        let mut first = AbstractState::unknown();
        first.registers[3] = AbstractValue::constant(ROM_BASE + 0xF1);
        let mut second = AbstractState::unknown();
        second.registers[3] = AbstractValue::constant(ROM_BASE + 0xF5);
        let incoming = BTreeMap::from([(
            (IsaMode::Thumb, guest_pc),
            vec![first.clone(), second.clone()],
        )]);
        let mut decoded = BTreeMap::from([(guest_pc, instruction)]);
        let mut tables = Vec::new();
        resolve_thumb_ldmia_rom(&image, &incoming, &mut decoded, &mut tables).unwrap();
        assert_eq!(
            decoded[&guest_pc].operation,
            ThumbOperation::LoadRomMultipleIncrementAfterCases {
                base_register: 3,
                register_list: 1,
                table_id: 0,
            }
        );
        assert_eq!(
            tables,
            vec![ThumbLdmiaRomTable {
                cases: vec![
                    ThumbLdmiaRomCase {
                        base_address: ROM_BASE + 0xF1,
                        values_by_register: [0x0800_0089, 0, 0, 0, 0, 0, 0, 0],
                    },
                    ThumbLdmiaRomCase {
                        base_address: ROM_BASE + 0xF5,
                        values_by_register: [0xAABB_CCDD, 0, 0, 0, 0, 0, 0, 0],
                    },
                ],
            }]
        );

        for other_base in [None, Some(0x0300_0000)] {
            let mut other = AbstractState::unknown();
            if let Some(base) = other_base {
                other.registers[3] = AbstractValue::constant(base);
            }
            let incoming =
                BTreeMap::from([((IsaMode::Thumb, guest_pc), vec![first.clone(), other])]);
            let mut decoded = BTreeMap::from([(guest_pc, instruction)]);
            let error = resolve_thumb_ldmia_rom(&image, &incoming, &mut decoded, &mut Vec::new())
                .unwrap_err();
            assert!(error.contains("thumb_ldmia_rom_candidate_not_finite"));
        }

        let too_many_bases = (0..=MAX_ROM_LDMIA_CASES)
            .map(|index| {
                let mut state = AbstractState::unknown();
                state.registers[3] = AbstractValue::constant(ROM_BASE + 0x100 + index as u32 * 4);
                state
            })
            .collect();
        let incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), too_many_bases)]);
        let mut decoded = BTreeMap::from([(guest_pc, instruction)]);
        let error =
            resolve_thumb_ldmia_rom(&image, &incoming, &mut decoded, &mut Vec::new()).unwrap_err();
        assert!(error.contains("thumb_ldmia_rom_case_limit"));
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
            only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]).registers[0],
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
            only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]).registers[0],
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
            only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]).registers[0],
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
            only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]).registers[0],
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
    fn abstract_memory_facts_remain_exact_across_disjunctive_paths() {
        let address = 0x0200_0100;
        let mut left = AbstractState::unknown();
        left.set_memory_word(address, AbstractValue::constant(0x0800_0085));
        let mut right = left.clone();
        right.set_memory_word(address, AbstractValue::constant(0x0800_0099));
        let missing = AbstractState::unknown();
        let key = (IsaMode::Thumb, ROM_BASE + 0x84);
        let mut incoming = BTreeMap::new();
        let mut pending = VecDeque::new();
        let mut total_states = 0;
        for state in [left, right, missing] {
            enqueue_state(
                key.0,
                key.1,
                state,
                &mut pending,
                &mut incoming,
                &mut total_states,
            )
            .unwrap();
        }

        let alternatives = &incoming[&key];
        assert_eq!(alternatives.len(), 3);
        assert_eq!(total_states, 3);
        let observed_words: BTreeSet<_> = alternatives
            .iter()
            .map(|state| {
                state.memory_words.get(&address).map(|value| match value {
                    AbstractValue::Constants(values) => *values.first().unwrap(),
                    AbstractValue::Unknown => panic!("stored test word became unknown"),
                })
            })
            .collect();
        assert_eq!(
            observed_words,
            BTreeSet::from([Some(0x0800_0085), Some(0x0800_0099), None])
        );
        assert_eq!(pending.len(), alternatives.len());
        assert!(pending.iter().all(|(queued_key, _)| *queued_key == key));
    }

    #[test]
    fn abstract_state_limits_reject_the_next_alternative_and_global_state() {
        let key = (IsaMode::Thumb, ROM_BASE + 0x84);
        let mut incoming = BTreeMap::new();
        let mut pending = VecDeque::new();
        let mut total_states = 0;
        for value in 0..MAX_PATH_ALTERNATIVES_PER_PC {
            let mut state = AbstractState::unknown();
            state.registers[0] = AbstractValue::constant(value as u32);
            enqueue_state(
                key.0,
                key.1,
                state,
                &mut pending,
                &mut incoming,
                &mut total_states,
            )
            .unwrap();
        }
        let mut overflow = AbstractState::unknown();
        overflow.registers[0] = AbstractValue::constant(MAX_PATH_ALTERNATIVES_PER_PC as u32);
        let error = enqueue_state(
            key.0,
            key.1,
            overflow,
            &mut pending,
            &mut incoming,
            &mut total_states,
        )
        .unwrap_err();
        assert!(error.contains("discovery_state_alternative_limit"));
        assert!(error.contains("128"));
        assert!(error.contains("0x08000084"));
        assert!(error.contains("Thumb"));
        assert_eq!(incoming[&key].len(), MAX_PATH_ALTERNATIVES_PER_PC);

        let mut global_incoming = BTreeMap::new();
        let mut global_pending = VecDeque::new();
        let mut total_states = MAX_ABSTRACT_STATES;
        let error = enqueue_state(
            key.0,
            key.1,
            AbstractState::unknown(),
            &mut global_pending,
            &mut global_incoming,
            &mut total_states,
        )
        .unwrap_err();
        assert!(error.contains("discovery_global_state_limit"));
        assert_eq!(total_states, MAX_ABSTRACT_STATES);
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
        let successor = only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]);
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
            only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)])
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
        let successor = only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]);
        assert!(!successor.memory_words.contains_key(&address));
        assert_eq!(
            successor.memory_words[&(address + 4)],
            AbstractValue::constant(0x0800_0099)
        );
    }

    #[test]
    fn thumb_halfword_store_to_ime_zero_preserves_saved_return_shadow() {
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x8004,
            operation: ThumbOperation::StoreHalfwordImmediate {
                rd: 4,
                rb: 0,
                imm5: 0,
            },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(0, 0x0400_0208);
        state.set_constant(4, 0);
        state.set_memory_word(0x0300_0018, AbstractValue::constant(0));
        state.set_memory_word(0x0300_001C, AbstractValue::constant(0x0800_00F0));
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
        let successor = only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]);
        assert_eq!(
            successor.memory_words[&0x0300_0018],
            AbstractValue::constant(0)
        );
        assert_eq!(
            successor.memory_words[&0x0300_001C],
            AbstractValue::constant(0x0800_00F0)
        );
    }

    #[test]
    fn thumb_halfword_store_to_ime_preserves_shadow_when_if_is_zero() {
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x8004,
            operation: ThumbOperation::StoreHalfwordImmediate {
                rd: 4,
                rb: 0,
                imm5: 0,
            },
        };
        for value in [None, Some(1)] {
            let mut state = AbstractState::unknown();
            state.set_constant(0, 0x0400_0208);
            if let Some(value) = value {
                state.set_constant(4, value);
            }
            state.set_memory_word(0x0300_0018, AbstractValue::constant(0x0800_0085));
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
            let successor = only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]);
            assert_eq!(
                successor.memory_words[&0x0300_0018],
                AbstractValue::constant(0x0800_0085)
            );
        }
    }

    #[test]
    fn thumb_halfword_store_to_ime_clears_shadow_when_if_is_not_proven_zero() {
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x8004,
            operation: ThumbOperation::StoreHalfwordImmediate {
                rd: 4,
                rb: 0,
                imm5: 0,
            },
        };
        for if_value in [AbstractValue::Unknown, AbstractValue::constant(1)] {
            let mut state = AbstractState::unknown();
            state.set_constant(0, 0x0400_0208);
            state.set_io_halfword(IO_IF, if_value);
            state.set_memory_word(0x0300_0018, AbstractValue::constant(0x0800_0085));
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
                only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)])
                    .memory_words
                    .is_empty()
            );
        }
    }

    #[test]
    fn thumb_halfword_store_to_waitcnt_preserves_latches_and_clears_ram_shadow() {
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x8004,
            operation: ThumbOperation::StoreHalfwordImmediate {
                rd: 4,
                rb: 0,
                imm5: 0,
            },
        };
        let mut state = state_with_known_io();
        state.set_constant(0, IO_WAITCNT);
        state.set_constant(4, 0x4014);
        state.set_memory_word(0x0300_0018, AbstractValue::constant(0x0800_0085));
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

        let successor = only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]);
        assert_io_facts(
            &successor,
            AbstractValue::constant(0x1234),
            AbstractValue::constant(0x0056),
            AbstractValue::constant(1),
        );
        assert!(successor.memory_words.is_empty());
    }

    #[test]
    fn palette_ram_stores_preserve_io_facts_and_clear_ram_shadow() {
        for address in [0x0500_0000, 0x0500_0002] {
            let mut state = state_with_known_io();
            state.set_memory_word(0x0200_0100, AbstractValue::constant(0x0800_0085));
            state.invalidate_store(&AbstractValue::constant(address), 2, 1);

            assert_io_facts(
                &state,
                AbstractValue::constant(0x1234),
                AbstractValue::constant(0x0056),
                AbstractValue::constant(1),
            );
            assert!(state.memory_words.is_empty());
        }
    }

    #[test]
    fn supported_ram_store_keeps_io_facts_and_invalidates_only_overlapped_words() {
        let address = 0x0200_0100;
        let mut state = state_with_known_io();
        state.set_memory_word(address, AbstractValue::constant(0x0800_0085));
        state.set_memory_word(address + 4, AbstractValue::constant(0x0800_0099));
        state.invalidate_store(&AbstractValue::constant(address), 4, 0);

        assert_io_facts(
            &state,
            AbstractValue::constant(0x1234),
            AbstractValue::constant(0x0056),
            AbstractValue::constant(1),
        );
        assert!(!state.memory_words.contains_key(&address));
        assert_eq!(
            state.memory_words[&(address + 4)],
            AbstractValue::constant(0x0800_0099)
        );
    }

    #[test]
    fn byte_stores_invalidate_the_entire_overlapped_io_latch() {
        for (address, expected_latch) in [
            (IO_IE, IO_IE),
            (IO_IE + 1, IO_IE),
            (IO_IF, IO_IF),
            (IO_IF + 1, IO_IF),
            (IO_IME, IO_IME),
            (IO_IME + 1, IO_IME),
        ] {
            let mut state = state_with_known_io();
            state.invalidate_store(&AbstractValue::constant(address), 1, 0);
            assert_io_facts(
                &state,
                if expected_latch == IO_IE {
                    AbstractValue::Unknown
                } else {
                    AbstractValue::constant(0x1234)
                },
                if expected_latch == IO_IF {
                    AbstractValue::Unknown
                } else {
                    AbstractValue::constant(0x0056)
                },
                if expected_latch == IO_IME {
                    AbstractValue::Unknown
                } else {
                    AbstractValue::constant(1)
                },
            );
        }
    }

    #[test]
    fn aligned_word_store_spanning_ie_and_if_preserves_ime_fact() {
        let mut state = state_with_known_io();
        state.invalidate_store(&AbstractValue::constant(IO_IE), 4, 0);

        assert_io_facts(
            &state,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
            AbstractValue::constant(1),
        );
    }

    #[test]
    fn odd_halfword_store_address_is_normalized_before_io_classification() {
        let mut state = state_with_known_io();
        state.invalidate_store(&AbstractValue::constant(IO_IE + 1), 2, 1);

        assert_io_facts(
            &state,
            AbstractValue::Unknown,
            AbstractValue::constant(0x0056),
            AbstractValue::constant(1),
        );
    }

    #[test]
    fn untracked_io_bytes_and_invalid_intervals_clear_all_io_facts() {
        for (address, byte_count, align_mask) in [
            (0x0400_0400, 2, 1),
            (0x0400_0800, 2, 1),
            (0x0400_01FE, 4, 0),
            (0x0400_0207, 2, 1), // Normalizes to 0x04000206.
            (IO_IME, 4, 0),      // IME plus untracked bytes.
            (IO_WAITCNT, 4, 0),  // WAITCNT plus untracked bytes.
            (0x04FF_FFFF, 1, 0),
            (0x04FF_FFFF, 2, 1), // Normalizes to 0x04FFFFFE.
            (0x04FF_FFFE, 4, 0), // Crosses from decode region 0x04 into 0x05.
            (0xFFFF_FFFE, 4, 0), // Interval overflow.
            (IO_IE, 3, 0),       // Unsupported store width.
        ] {
            let mut state = state_with_known_io();
            state.invalidate_store(&AbstractValue::constant(address), byte_count, align_mask);
            assert_io_facts(
                &state,
                AbstractValue::Unknown,
                AbstractValue::Unknown,
                AbstractValue::Unknown,
            );
        }
    }

    #[test]
    fn waitcnt_stores_preserve_io_latches_but_clear_ram_shadow() {
        for (address, byte_count, align_mask) in [
            (IO_WAITCNT, 1, 0),
            (IO_WAITCNT + 1, 1, 0),
            (IO_WAITCNT, 2, 1),
        ] {
            let mut state = state_with_known_io();
            state.set_memory_word(0x0300_0000, AbstractValue::constant(0x0800_0085));
            state.invalidate_store(&AbstractValue::constant(address), byte_count, align_mask);

            assert_io_facts(
                &state,
                AbstractValue::constant(0x1234),
                AbstractValue::constant(0x0056),
                AbstractValue::constant(1),
            );
            assert!(state.memory_words.is_empty());
        }
    }

    #[test]
    fn thumb_dma3_rom_to_iwram_word_transfer_preserves_latches_only_after_proof() {
        let image = counter_rom();
        let source = ROM_BASE + 0xC0;
        let destination = 0x0300_0040;
        let mut state = state_with_known_io();
        state.set_memory_word(0x0300_0000, AbstractValue::constant(0x0800_0085));
        state.set_constant(0, IO_DMA3_SAD);
        state.set_constant(5, source);

        state = only_state(&run_thumb_successors(
            &image,
            thumb_word_store(ROM_BASE + 0x80, 5, 0),
            &state,
        ))
        .clone();
        assert_io_facts(
            &state,
            AbstractValue::constant(0x1234),
            AbstractValue::constant(0x0056),
            AbstractValue::constant(1),
        );
        assert!(state.memory_words.is_empty());
        assert_eq!(state.dma.dma3_source, AbstractValue::constant(source));

        state.set_memory_word(0x0300_0000, AbstractValue::constant(0x0800_0099));
        state.set_constant(4, destination);
        state = only_state(&run_thumb_successors(
            &image,
            thumb_word_store(ROM_BASE + 0x82, 4, 1),
            &state,
        ))
        .clone();
        assert_io_facts(
            &state,
            AbstractValue::constant(0x1234),
            AbstractValue::constant(0x0056),
            AbstractValue::constant(1),
        );
        assert!(state.memory_words.is_empty());
        assert_eq!(
            state.dma.dma3_destination,
            AbstractValue::constant(destination)
        );

        state.set_memory_word(0x0300_0000, AbstractValue::constant(0x0800_00A5));
        state.set_constant(1, (DMA3_SAFE_IMMEDIATE_WORD_CONTROL << 16) | 1);
        state = only_state(&run_thumb_successors(
            &image,
            thumb_word_store(ROM_BASE + 0x84, 1, 2),
            &state,
        ))
        .clone();
        assert_io_facts(
            &state,
            AbstractValue::constant(0x1234),
            AbstractValue::constant(0x0056),
            AbstractValue::constant(1),
        );
        assert!(state.memory_words.is_empty());
        assert!(state.dma.all_channels_are_proven_quiescent());
        assert_eq!(state.dma.dma3_control, AbstractValue::constant(0x0400));
        assert_eq!(state.dma.dma3_source, AbstractValue::Unknown);
        assert_eq!(state.dma.dma3_destination, AbstractValue::Unknown);
    }

    #[test]
    fn dma3_rearm_after_completion_cannot_reuse_preincrement_addresses() {
        let image = counter_rom();
        let mut state = state_with_known_io();
        state.dma.dma3_source = AbstractValue::Unknown;
        state.dma.dma3_destination = AbstractValue::Unknown;
        state.dma.dma3_control = AbstractValue::constant(0x0400);
        state.set_constant(0, IO_DMA3_SAD);
        state.set_constant(1, (DMA3_SAFE_IMMEDIATE_WORD_CONTROL << 16) | 1);

        let successors =
            run_thumb_successors(&image, thumb_word_store(ROM_BASE + 0x84, 1, 2), &state);
        let successor = only_state(&successors);
        assert_io_facts(
            successor,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
        );
        assert!(!successor.dma.all_channels_are_proven_quiescent());
    }

    #[test]
    fn push_to_io_or_unknown_stack_invalidates_latches_and_possible_dma_facts() {
        let image = counter_rom();
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x80,
            encoding: 0xB401,
            operation: ThumbOperation::Push { register_list: 1 },
        };

        let mut mmio_stack = state_with_known_io();
        mmio_stack.set_constant(0, 0xDEAD_BEEF);
        mmio_stack.set_constant(13, IO_IME + 4);
        mmio_stack.set_memory_word(0x0300_0000, AbstractValue::constant(0x0800_0085));
        let successors = run_thumb_successors(&image, instruction, &mmio_stack);
        let successor = only_state(&successors);
        assert_io_facts(
            successor,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
        );
        assert!(successor.memory_words.is_empty());

        let mut unknown_stack = state_with_known_io();
        unknown_stack.set_constant(0, 0xDEAD_BEEF);
        unknown_stack.registers[13] = AbstractValue::Unknown;
        let successors = run_thumb_successors(&image, instruction, &unknown_stack);
        let successor = only_state(&successors);
        assert_io_facts(
            successor,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
        );
        assert!(!successor.dma.all_channels_are_proven_quiescent());
    }

    #[test]
    fn push_to_dma_register_invalidates_quiescence() {
        let image = counter_rom();
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x80,
            encoding: 0xB401,
            operation: ThumbOperation::Push { register_list: 1 },
        };
        let mut state = state_with_known_io();
        state.set_constant(0, 0xDEAD_BEEF);
        state.set_constant(13, IO_DMA3_SAD + 4);

        let successors = run_thumb_successors(&image, instruction, &state);
        let successor = only_state(&successors);
        assert_io_facts(
            successor,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
        );
        assert!(!successor.dma.all_channels_are_proven_quiescent());
    }

    #[test]
    fn stmia_to_dma_invalidates_quiescence_even_when_io_facts_are_cleared() {
        let image = counter_rom();
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x80,
            encoding: 0xC201,
            operation: ThumbOperation::StoreMultipleIncrementAfter {
                base_register: 2,
                register_list: 1,
            },
        };
        let mut state = state_with_known_io();
        state.registers[0] = AbstractValue::constant(0x1234_5678);
        state.set_constant(2, IO_DMA0_START);

        let successors = run_thumb_successors(&image, instruction, &state);
        let successor = only_state(&successors);
        assert_io_facts(
            successor,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
        );
        assert!(!successor.dma.all_channels_are_proven_quiescent());
    }

    #[test]
    fn disabled_dma3_control_store_has_no_latch_effect() {
        let image = counter_rom();
        let mut state = state_with_known_io();
        state.set_memory_word(0x0300_0000, AbstractValue::constant(0x0800_0085));
        state.set_constant(0, IO_DMA3_SAD);
        state.set_constant(1, 0x0002_0001);

        let successors =
            run_thumb_successors(&image, thumb_word_store(ROM_BASE + 0x80, 1, 2), &state);
        let successor = only_state(&successors);
        assert_io_facts(
            successor,
            AbstractValue::constant(0x1234),
            AbstractValue::constant(0x0056),
            AbstractValue::constant(1),
        );
        assert!(successor.memory_words.is_empty());
        assert!(successor.dma.all_channels_are_proven_quiescent());
        assert_eq!(successor.dma.dma3_control, AbstractValue::constant(0));
    }

    #[test]
    fn unsafe_dma3_configurations_clear_latches_and_dma_proof() {
        let image = counter_rom();
        let source = ROM_BASE + 0xC0;
        let destination = 0x0300_0040;
        let cases = [
            (source, destination, 1, 0xC400),          // DMA IRQ enabled.
            (source, destination, 1, 0x8600),          // Repeat.
            (source, destination, 1, 0x8C00),          // DRQ.
            (source, destination, 1, 0x9400),          // VBlank timing.
            (source, destination, 1, 0x8000),          // 16-bit width.
            (source, destination, 0, 0x8400),          // DMA3 count zero means 65536.
            (ROM_BASE + 0xFC, destination, 2, 0x8400), // ROM range overrun.
            (source, 0x0300_7FFC, 2, 0x8400),          // IWRAM range overrun.
            (ROM_BASE + 0xC2, destination, 1, 0x8400), // Misaligned source.
            (source, destination + 2, 1, 0x8400),      // Misaligned destination.
            (source, 0x0200_0000, 1, 0x8400),          // EWRAM destination.
        ];

        for (source, destination, count, control) in cases {
            let mut state = state_with_known_io();
            state.dma.dma3_source = AbstractValue::constant(source & DMA3_ADDRESS_MASK);
            state.dma.dma3_destination = AbstractValue::constant(destination & DMA3_ADDRESS_MASK);
            state.set_constant(0, IO_DMA3_SAD);
            state.set_constant(1, (control << 16) | count);
            let successors =
                run_thumb_successors(&image, thumb_word_store(ROM_BASE + 0x80, 1, 2), &state);
            let successor = only_state(&successors);

            assert_io_facts(
                successor,
                AbstractValue::Unknown,
                AbstractValue::Unknown,
                AbstractValue::Unknown,
            );
            assert!(successor.memory_words.is_empty());
            assert!(!successor.dma.all_channels_are_proven_quiescent());
        }

        let mut unknown_control = state_with_known_io();
        unknown_control.dma.dma3_source = AbstractValue::constant(source);
        unknown_control.dma.dma3_destination = AbstractValue::constant(destination);
        unknown_control.set_constant(0, IO_DMA3_SAD);
        let successors = run_thumb_successors(
            &image,
            thumb_word_store(ROM_BASE + 0x80, 1, 2),
            &unknown_control,
        );
        let successor = only_state(&successors);
        assert_io_facts(
            successor,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
        );
    }

    #[test]
    fn other_dma_channel_or_partial_dma3_store_prevents_quiescence_recovery() {
        let mut other_channel = state_with_known_io();
        other_channel.invalidate_store(&AbstractValue::constant(IO_DMA0_START), 4, 0);
        assert!(!other_channel.dma.all_channels_are_proven_quiescent());
        assert!(!other_channel.try_dma3_word_store(
            IO_DMA3_SAD,
            &AbstractValue::constant(ROM_BASE + 0xC0),
            &counter_rom(),
        ));
        assert_io_facts(
            &other_channel,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
        );

        let mut partial = state_with_known_io();
        partial.invalidate_store(&AbstractValue::constant(IO_DMA3_SAD), 2, 1);
        assert!(!partial.dma.all_channels_are_proven_quiescent());
        assert_io_facts(
            &partial,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
            AbstractValue::Unknown,
        );
    }

    #[test]
    fn register_ram_reset_restores_dma_proof_only_for_singleton_dma_flag() {
        let mut reset = state_with_known_io();
        reset.dma.invalidate();
        reset.reset_dma_after_register_ram_reset(&AbstractValue::constant(0x80));
        assert!(reset.dma.all_channels_are_proven_quiescent());
        assert_eq!(reset.dma.dma3_source, AbstractValue::constant(0));
        assert_eq!(reset.dma.dma3_destination, AbstractValue::constant(0));

        let mut ambiguous = state_with_known_io();
        ambiguous.reset_dma_after_register_ram_reset(&AbstractValue::Unknown);
        assert!(!ambiguous.dma.all_channels_are_proven_quiescent());
        assert_eq!(ambiguous.dma.dma3_source, AbstractValue::Unknown);
    }

    #[test]
    fn finite_store_targets_apply_every_alternative() {
        let cases = [
            (
                BTreeSet::from([0, IO_IE]),
                AbstractValue::Unknown,
                AbstractValue::constant(0x0056),
                AbstractValue::constant(1),
            ),
            (
                BTreeSet::from([IO_IE, IO_IME]),
                AbstractValue::Unknown,
                AbstractValue::constant(0x0056),
                AbstractValue::Unknown,
            ),
            (
                BTreeSet::from([0x0500_0000, IO_IE]),
                AbstractValue::Unknown,
                AbstractValue::constant(0x0056),
                AbstractValue::constant(1),
            ),
            (
                BTreeSet::from([IO_WAITCNT, IO_IE]),
                AbstractValue::Unknown,
                AbstractValue::constant(0x0056),
                AbstractValue::constant(1),
            ),
            (
                BTreeSet::from([IO_IE, 0x0400_0800]),
                AbstractValue::Unknown,
                AbstractValue::Unknown,
                AbstractValue::Unknown,
            ),
            (
                BTreeSet::from([0x0500_0000, 0x0400_0800]),
                AbstractValue::Unknown,
                AbstractValue::Unknown,
                AbstractValue::Unknown,
            ),
        ];

        for (targets, ie, interrupt_flags, ime) in cases {
            let mut state = state_with_known_io();
            state.invalidate_store(&AbstractValue::Constants(targets), 2, 1);
            assert_io_facts(&state, ie, interrupt_flags, ime);
        }
    }

    #[test]
    fn io_fact_invalidation_handles_unknown_empty_and_target_count_boundaries() {
        for addresses in [
            AbstractValue::Unknown,
            AbstractValue::Constants(BTreeSet::new()),
            AbstractValue::Constants((0..=MAX_CONSTANT_TARGETS as u32).collect()),
        ] {
            let mut state = state_with_known_io();
            state.invalidate_store(&addresses, 2, 1);
            assert_io_facts(
                &state,
                AbstractValue::Unknown,
                AbstractValue::Unknown,
                AbstractValue::Unknown,
            );
        }

        let mut exact_limit = state_with_known_io();
        let targets = std::iter::once(IO_IE)
            .chain(std::iter::once(IO_IME))
            .chain((0..MAX_CONSTANT_TARGETS - 2).map(|index| 0x0500_0000 + index as u32 * 2))
            .collect();
        exact_limit.invalidate_store(&AbstractValue::Constants(targets), 2, 1);
        assert_io_facts(
            &exact_limit,
            AbstractValue::Unknown,
            AbstractValue::constant(0x0056),
            AbstractValue::Unknown,
        );
    }

    #[test]
    fn thumb_word_store_invalidates_its_target_word_and_preserves_neighbors() {
        let address = 0x0200_0100;
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x6008,
            operation: ThumbOperation::StoreWordImmediate {
                rd: 0,
                rb: 1,
                imm5: 0,
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
        let successor = only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]);
        assert!(!successor.memory_words.contains_key(&address));
        assert_eq!(
            successor.memory_words[&(address + 4)],
            AbstractValue::constant(0x0800_0099)
        );
    }

    #[test]
    fn thumb_word_store_with_unknown_address_clears_abstract_memory() {
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0x6008,
            operation: ThumbOperation::StoreWordImmediate {
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
            only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)])
                .memory_words
                .is_empty()
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
        let successors = &incoming[&(IsaMode::Arm, ROM_BASE + 0x88)];
        assert_eq!(successors.len(), 2);
        let store_path = successors
            .iter()
            .find(|successor| !successor.memory_words.contains_key(&address))
            .expect("executed conditional store must invalidate its target");
        let skipped_path = successors
            .iter()
            .find(|successor| successor.memory_words.contains_key(&address))
            .expect("skipped conditional store must preserve memory facts");
        assert!(!store_path.memory_words.contains_key(&address));
        assert_eq!(
            store_path.memory_words[&(address + 4)],
            AbstractValue::constant(0x0800_0099)
        );
        assert_eq!(
            skipped_path.memory_words[&address],
            AbstractValue::constant(0x0800_0085)
        );
        assert_eq!(
            skipped_path.memory_words[&(address + 4)],
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
        state.set_constant(0, 0x80);
        state.set_io_halfword(IO_IE, AbstractValue::constant(0x1234));
        state.set_io_halfword(IO_IF, AbstractValue::constant(0x0005));
        state.set_io_halfword(IO_IME, AbstractValue::constant(1));
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
            only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)])
                .memory_words
                .is_empty()
        );
        let reset_state = only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]);
        assert_eq!(reset_state.io_halfword(IO_IE), AbstractValue::constant(0));
        assert_eq!(reset_state.io_halfword(IO_IF), AbstractValue::constant(0));
        assert_eq!(reset_state.io_halfword(IO_IME), AbstractValue::constant(0));
    }

    #[test]
    fn register_ram_reset_without_io_flag_preserves_abstract_latches() {
        let instruction = ThumbInstruction {
            guest_pc: ROM_BASE + 0x84,
            encoding: 0xDF01,
            operation: ThumbOperation::Swi { immediate: 1 },
        };
        let mut state = AbstractState::unknown();
        state.set_constant(0, 0);
        state.set_io_halfword(IO_IE, AbstractValue::constant(0x1234));
        state.set_io_halfword(IO_IF, AbstractValue::constant(0x0005));
        state.set_io_halfword(IO_IME, AbstractValue::constant(1));
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
        let next_state = only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]);
        assert_eq!(
            next_state.io_halfword(IO_IE),
            AbstractValue::constant(0x1234)
        );
        assert_eq!(
            next_state.io_halfword(IO_IF),
            AbstractValue::constant(0x0005)
        );
        assert_eq!(next_state.io_halfword(IO_IME), AbstractValue::constant(1));
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
        let mut incoming = BTreeMap::from([((IsaMode::Thumb, guest_pc), vec![state.clone()])]);
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

        let after = only_state(incoming.get(&(IsaMode::Thumb, guest_pc + 2)).unwrap());
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
        assert_eq!(pending_keys(&pending), vec![successor]);
        let next = only_state(&incoming[&successor]);
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

        let next = only_state(&incoming[&(IsaMode::Thumb, guest_pc.wrapping_add(2))]);
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
        assert_eq!(
            pending_keys(&pending),
            vec![(IsaMode::Thumb, ROM_BASE + 0x86)]
        );
        assert_eq!(
            only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]).registers[0],
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
                    reachable_successors: 0b11,
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
            assert_eq!(pending_keys(&pending), vec![target, fallthrough]);
            assert_eq!(
                only_state(&incoming[&target]),
                &state,
                "condition 0x{condition:X}"
            );
            assert_eq!(
                only_state(&incoming[&fallthrough]),
                &state,
                "condition 0x{condition:X}"
            );
            assert!(branch_targets.is_empty());
        }
    }

    #[test]
    fn cmp_refines_branch_masks_and_unknown_predecessors_keep_both_edges() {
        let compare_pc = ROM_BASE + 0x84;
        let branch_pc = compare_pc + 2;
        let compare = ThumbInstruction {
            guest_pc: compare_pc,
            encoding: 0x280A,
            operation: ThumbOperation::CompareImmediate {
                rd: 0,
                immediate: 10,
            },
        };
        let branch = ThumbInstruction {
            guest_pc: branch_pc,
            encoding: 0xD000,
            operation: ThumbOperation::ConditionalBranch {
                condition: 0,
                target: branch_pc + 4,
                reachable_successors: 0b11,
            },
        };

        let mask_after_cmp = |value: AbstractValue| {
            let mut state = AbstractState::unknown();
            state.registers[0] = value;
            let mut pending = VecDeque::new();
            let mut incoming = BTreeMap::new();
            process_thumb_instruction(
                &counter_rom(),
                compare,
                &state,
                &mut pending,
                &mut incoming,
                &mut BTreeSet::new(),
            )
            .unwrap();
            let mut decoded = BTreeMap::from([(branch_pc, branch)]);
            resolve_thumb_branch_masks(&incoming, &mut decoded).unwrap();
            match decoded[&branch_pc].operation {
                ThumbOperation::ConditionalBranch {
                    reachable_successors,
                    ..
                } => reachable_successors,
                _ => unreachable!(),
            }
        };

        assert_eq!(mask_after_cmp(AbstractValue::constant(10)), 0b01);
        assert_eq!(mask_after_cmp(AbstractValue::constant(9)), 0b10);
        assert_eq!(
            mask_after_cmp(AbstractValue::Constants(BTreeSet::from([9, 10]))),
            0b11
        );

        let mut compared_state = AbstractState::unknown();
        compared_state.registers[0] = AbstractValue::constant(10);
        compared_state.pending_compare = Some(CompareFact {
            mode: IsaMode::Thumb,
            guest_pc: compare_pc,
            register: 0,
            immediate: 10,
        });
        let unrefined_state = AbstractState::unknown();
        let incoming = BTreeMap::from([(
            (IsaMode::Thumb, branch_pc),
            vec![compared_state, unrefined_state],
        )]);
        let mut decoded = BTreeMap::from([(branch_pc, branch)]);
        resolve_thumb_branch_masks(&incoming, &mut decoded).unwrap();
        assert!(matches!(
            decoded[&branch_pc].operation,
            ThumbOperation::ConditionalBranch {
                reachable_successors: 0b11,
                ..
            }
        ));
    }

    #[test]
    fn arm_cmp_refines_arm_branch_edges_and_unknown_states_keep_both() {
        let branch_pc = ROM_BASE + 0x88;
        let branch = ArmInstruction {
            guest_pc: branch_pc,
            encoding: 0x1A00_0000,
            condition: 1,
            operation: ArmOperation::Branch {
                target: ROM_BASE + 0x84,
                link: false,
                reachable_successors: 0b11,
            },
        };
        let mut state = AbstractState::unknown();
        state.pending_compare = Some(CompareFact {
            mode: IsaMode::Arm,
            guest_pc: branch_pc - 4,
            register: 0,
            immediate: 10,
        });
        state.set_constant(0, 9);
        assert_eq!(arm_branch_successors(branch, &state), 0b01);
        state.set_constant(0, 10);
        assert_eq!(arm_branch_successors(branch, &state), 0b10);

        state.registers[0] = AbstractValue::Unknown;
        assert_eq!(arm_branch_successors(branch, &state), 0b11);
        state.pending_compare = None;
        state.set_constant(0, 9);
        assert_eq!(arm_branch_successors(branch, &state), 0b11);
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
            only_state(&incoming[&(IsaMode::Thumb, instruction.guest_pc.wrapping_add(2))]).clone()
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
        let next_state = only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]);
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
            only_state(&incoming[&(IsaMode::Thumb, ROM_BASE + 0x86)]).registers[0],
            AbstractValue::Unknown
        );
    }

    #[test]
    fn propagates_thumb_lsr_immediate_constants_and_preserves_unknowns() {
        let image = bx_interwork_rom();
        let guest_pc = ROM_BASE + 0x84;
        for (source, amount, expected) in [
            (0x8000_0001, 0, 0),
            (0x8000_0001, 1, 0x4000_0000),
            (0x0001_8000, 16, 1),
            (0x8000_0000, 31, 1),
        ] {
            let instruction = ThumbInstruction {
                guest_pc,
                encoding: 0x0800 | (u16::from(amount) << 6) | (1 << 3),
                operation: ThumbOperation::LsrImmediate {
                    rd: 0,
                    rs: 1,
                    amount,
                },
            };
            let mut state = AbstractState::unknown();
            state.set_constant(0, 0xDEAD_BEEF);
            state.set_constant(1, source);
            state.set_constant(3, 0xAABB_CCDD);
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
            assert_eq!(pending_keys(&pending), vec![successor]);
            let next = only_state(&incoming[&successor]);
            assert_eq!(next.registers[0], AbstractValue::constant(expected));
            assert_eq!(next.registers[1], AbstractValue::constant(source));
            assert_eq!(next.registers[3], AbstractValue::constant(0xAABB_CCDD));
        }

        let alias_instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x0C12,
            operation: ThumbOperation::LsrImmediate {
                rd: 2,
                rs: 2,
                amount: 16,
            },
        };
        let mut finite_state = AbstractState::unknown();
        finite_state.registers[2] =
            AbstractValue::Constants(BTreeSet::from([0x8000_0001, 0x0001_8000, 0x8001_8000]));
        finite_state.set_constant(3, 0x1122_3344);
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &image,
            alias_instruction,
            &finite_state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        let successor = (IsaMode::Thumb, guest_pc + 2);
        let values: BTreeSet<_> = incoming[&successor]
            .iter()
            .map(|state| state_constant(state, 2))
            .collect();
        assert_eq!(
            values,
            BTreeSet::from([0x0000_0001, 0x0000_8000, 0x0000_8001])
        );
        assert_eq!(incoming[&successor].len(), 3);
        assert!(
            incoming[&successor]
                .iter()
                .all(|state| state.registers[3] == AbstractValue::constant(0x1122_3344))
        );

        let unknown_instruction = ThumbInstruction {
            guest_pc,
            encoding: 0x0848,
            operation: ThumbOperation::LsrImmediate {
                rd: 0,
                rs: 1,
                amount: 1,
            },
        };
        let mut unknown_state = AbstractState::unknown();
        unknown_state.set_constant(0, 0xDEAD_BEEF);
        unknown_state.set_constant(3, 0x5566_7788);
        let mut pending = VecDeque::new();
        let mut incoming = BTreeMap::new();
        process_thumb_instruction(
            &image,
            unknown_instruction,
            &unknown_state,
            &mut pending,
            &mut incoming,
            &mut BTreeSet::new(),
        )
        .unwrap();
        let next = only_state(&incoming[&successor]);
        assert_eq!(next.registers[0], AbstractValue::Unknown);
        assert_eq!(next.registers[1], AbstractValue::Unknown);
        assert_eq!(next.registers[3], AbstractValue::constant(0x5566_7788));
        assert_eq!(pending_keys(&pending), vec![successor]);
    }

    #[test]
    fn propagates_thumb_orr_as_exact_disjunctive_states_with_limits() {
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
        let exact_pairs: BTreeSet<_> = incoming[&successor]
            .iter()
            .map(|state| (state_constant(state, 0), state_constant(state, 3)))
            .collect();
        assert_eq!(
            exact_pairs,
            BTreeSet::from([(3, 2), (0x12, 2), (0x21, 0x20), (0x30, 0x20)])
        );
        assert_eq!(incoming[&successor].len(), 4);
        assert_eq!(pending_keys(&pending), vec![successor; 4]);
        assert!(
            incoming[&successor]
                .iter()
                .all(|state| state.registers[1] == AbstractValue::constant(0x0200_0000))
        );

        let mut deduplicated_state = AbstractState::unknown();
        let small_constants: BTreeSet<_> = (0..8).collect();
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
        let deduplicated_results: BTreeSet<_> = incoming[&successor]
            .iter()
            .map(|state| state_constant(state, 0))
            .collect();
        let expected_deduplicated_pairs: BTreeSet<_> = (0..8)
            .flat_map(|left| (0..8).map(move |right| (left | right, right)))
            .collect();
        let actual_deduplicated_pairs: BTreeSet<_> = incoming[&successor]
            .iter()
            .map(|state| (state_constant(state, 0), state_constant(state, 3)))
            .collect();
        assert_eq!(actual_deduplicated_pairs, expected_deduplicated_pairs);
        assert_eq!(incoming[&successor].len(), actual_deduplicated_pairs.len());
        assert_eq!(deduplicated_results, (0..8).collect());

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
        let alias_results: BTreeSet<_> = incoming[&successor]
            .iter()
            .map(|state| state_constant(state, 2))
            .collect();
        assert_eq!(incoming[&successor].len(), 16);
        assert_eq!(alias_results, one_hot_constants);

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
        assert_eq!(
            only_state(&incoming[&successor]).registers[0],
            AbstractValue::Unknown
        );

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
        let overflow_pairs: BTreeSet<_> = incoming[&successor]
            .iter()
            .map(|state| (state_constant(state, 0), state_constant(state, 3)))
            .collect();
        let expected_pairs: BTreeSet<_> = (0..16)
            .flat_map(|value| [(value, 0), (value | 0x10, 0x10)])
            .collect();
        assert_eq!(incoming[&successor].len(), 32);
        assert_eq!(overflow_pairs, expected_pairs);
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
        assert_eq!(pending_keys(&pending), vec![successor]);
        assert_eq!(only_state(&incoming[&successor]), &before);
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
        assert_eq!(pending_keys(&pending), vec![successor]);
        assert_eq!(only_state(&incoming[&successor]), &before);
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
            assert_eq!(pending_keys(&pending), vec![successor]);
            assert_eq!(only_state(&incoming[&successor]), &expected);
        }
    }

    #[test]
    fn thumb_ldr_word_marks_only_destination_unknown_and_enqueues_pc_plus_two() {
        let guest_pc = ROM_BASE + 0x84;
        for (encoding, rd, rb, imm5) in [(0x6880, 0, 0, 2), (0x6FC9, 1, 1, 31)] {
            let instruction = ThumbInstruction {
                guest_pc,
                encoding,
                operation: ThumbOperation::LoadWordImmediate { rd, rb, imm5 },
            };
            let mut state = AbstractState::unknown();
            state.set_constant(0, 0x0400_00D4);
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
            assert_eq!(pending_keys(&pending), vec![successor]);
            assert_eq!(only_state(&incoming[&successor]), &expected);
        }
    }

    #[test]
    fn thumb_ldrh_marks_only_destination_unknown_and_enqueues_pc_plus_two() {
        let guest_pc = ROM_BASE + 0x84;
        for (encoding, rd, rb, imm5) in [(0x8821, 1, 4, 0), (0x8FC9, 1, 1, 31)] {
            let instruction = ThumbInstruction {
                guest_pc,
                encoding,
                operation: ThumbOperation::LoadHalfwordImmediate { rd, rb, imm5 },
            };
            let mut state = AbstractState::unknown();
            state.set_constant(0, 0x1122_3344);
            state.set_constant(1, 0x0200_0001);
            state.set_constant(2, 0xAABB_CCDD);
            state.set_constant(4, 0x0300_0000);

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
            assert_eq!(pending_keys(&pending), vec![successor]);
            assert_eq!(only_state(&incoming[&successor]), &expected);
        }
    }
}

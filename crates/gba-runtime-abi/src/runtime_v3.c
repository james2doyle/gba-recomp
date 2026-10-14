#include "gba_abi_v3.h"

static uint32_t fail(GbaRuntimeV3 *rt, GbaExitV3 *out,
                     uint32_t guest_address, GbaMemoryFaultV3 code) {
    if (out != NULL) {
        out->reason = GBA_EXIT_FAULT_V2;
        out->next_pc = rt == NULL ? 0 : rt->cpu.r[15];
        out->fault_address = guest_address;
        out->fault_code = (uint32_t)code;
    }
    return GBA_EXIT_FAULT_V2;
}

static void store_le32(uint8_t *destination, uint32_t value) {
    destination[0] = (uint8_t)value;
    destination[1] = (uint8_t)(value >> 8);
    destination[2] = (uint8_t)(value >> 16);
    destination[3] = (uint8_t)(value >> 24);
}

static uint32_t load_le32(const uint8_t *source) {
    return (uint32_t)source[0] | ((uint32_t)source[1] << 8) |
           ((uint32_t)source[2] << 16) | ((uint32_t)source[3] << 24);
}

static void store_le16(uint8_t *destination, uint16_t value) {
    destination[0] = (uint8_t)value;
    destination[1] = (uint8_t)(value >> 8);
}

uint32_t gba_store32_v3(GbaRuntimeV3 *rt, GbaExitV3 *out,
                        uint32_t guest_address, uint32_t value) {
    if (out == NULL) {
        return GBA_EXIT_FAULT_V2;
    }
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt == NULL ? 0 : rt->cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_MEMORY_FAULT_NONE_V3;

    if (rt == NULL) {
        return fail(rt, out, guest_address, GBA_MEMORY_FAULT_RUNTIME_V3);
    }
    if (rt->abi_version != GBA_ABI_VERSION_V3 ||
        rt->cpu.abi_version != GBA_ABI_VERSION_V2) {
        return fail(rt, out, guest_address, GBA_MEMORY_FAULT_ABI_V3);
    }
    if (rt->memory == NULL) {
        return fail(rt, out, guest_address, GBA_MEMORY_FAULT_NULL_MEMORY_V3);
    }
    if ((guest_address & UINT32_C(3)) != 0) {
        return fail(rt, out, guest_address, GBA_MEMORY_FAULT_ALIGNMENT_V3);
    }

    uint8_t *destination;
    if (guest_address >= UINT32_C(0x02000000) &&
        guest_address <= UINT32_C(0x0203FFFC)) {
        destination = &rt->memory->ewram[guest_address - UINT32_C(0x02000000)];
    } else if (guest_address >= UINT32_C(0x03000000) &&
               guest_address <= UINT32_C(0x03007FFC)) {
        destination = &rt->memory->iwram[guest_address - UINT32_C(0x03000000)];
    } else {
        return fail(rt, out, guest_address, GBA_MEMORY_FAULT_REGION_V3);
    }

    store_le32(destination, value);
    return GBA_EXIT_CONTINUE_V2;
}

uint32_t gba_store16_v3(GbaRuntimeV3 *rt, GbaExitV3 *out,
                        uint32_t guest_ea, uint32_t value) {
    if (out == NULL) {
        return GBA_EXIT_FAULT_V2;
    }
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt == NULL ? 0 : rt->cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_MEMORY_FAULT_NONE_V3;

    if (rt == NULL) {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_RUNTIME_V3);
    }
    if (rt->abi_version != GBA_ABI_VERSION_V3 ||
        rt->cpu.abi_version != GBA_ABI_VERSION_V2) {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_ABI_V3);
    }
    if (rt->memory == NULL) {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_NULL_MEMORY_V3);
    }

    const uint32_t aligned_ea = guest_ea & ~UINT32_C(1);
    uint8_t *destination;
    if (aligned_ea >= UINT32_C(0x02000000) &&
        aligned_ea <= UINT32_C(0x0203FFFE)) {
        destination = &rt->memory->ewram[aligned_ea - UINT32_C(0x02000000)];
    } else if (aligned_ea >= UINT32_C(0x03000000) &&
               aligned_ea <= UINT32_C(0x03007FFE)) {
        destination = &rt->memory->iwram[aligned_ea - UINT32_C(0x03000000)];
    } else {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_REGION_V3);
    }

    store_le16(destination, (uint16_t)value);
    return GBA_EXIT_CONTINUE_V2;
}

uint32_t gba_store8_v3(GbaRuntimeV3 *rt, GbaExitV3 *out,
                       uint32_t guest_ea, uint32_t value) {
    if (out == NULL) {
        return GBA_EXIT_FAULT_V2;
    }
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt == NULL ? 0 : rt->cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_MEMORY_FAULT_NONE_V3;

    if (rt == NULL) {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_RUNTIME_V3);
    }
    if (rt->abi_version != GBA_ABI_VERSION_V3 ||
        rt->cpu.abi_version != GBA_ABI_VERSION_V2) {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_ABI_V3);
    }
    if (rt->memory == NULL) {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_NULL_MEMORY_V3);
    }

    uint8_t *destination;
    if (guest_ea >= UINT32_C(0x02000000) &&
        guest_ea < UINT32_C(0x02040000)) {
        destination = &rt->memory->ewram[guest_ea - UINT32_C(0x02000000)];
    } else if (guest_ea >= UINT32_C(0x03000000) &&
               guest_ea < UINT32_C(0x03008000)) {
        destination = &rt->memory->iwram[guest_ea - UINT32_C(0x03000000)];
    } else {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_REGION_V3);
    }

    *destination = (uint8_t)value;
    return GBA_EXIT_CONTINUE_V2;
}

uint32_t gba_load8_v3(GbaRuntimeV3 *rt, GbaExitV3 *out,
                      uint32_t guest_ea, uint8_t *host_value_out) {
    if (out == NULL || host_value_out == NULL) {
        return GBA_EXIT_FAULT_V2;
    }
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt == NULL ? 0 : rt->cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_MEMORY_FAULT_NONE_V3;

    if (rt == NULL) {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_RUNTIME_V3);
    }
    if (rt->abi_version != GBA_ABI_VERSION_V3 ||
        rt->cpu.abi_version != GBA_ABI_VERSION_V2) {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_ABI_V3);
    }
    if (rt->memory == NULL) {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_NULL_MEMORY_V3);
    }

    uint8_t value;
    if (guest_ea >= UINT32_C(0x02000000) &&
        guest_ea < UINT32_C(0x02040000)) {
        value = rt->memory->ewram[guest_ea - UINT32_C(0x02000000)];
    } else if (guest_ea >= UINT32_C(0x03000000) &&
               guest_ea < UINT32_C(0x03008000)) {
        value = rt->memory->iwram[guest_ea - UINT32_C(0x03000000)];
    } else {
        return fail(rt, out, guest_ea, GBA_MEMORY_FAULT_REGION_V3);
    }

    *host_value_out = value;
    return GBA_EXIT_CONTINUE_V2;
}

uint32_t gba_push_thumb_v3(GbaRuntimeV3 *rt, GbaExitV3 *out,
                           uint32_t register_list) {
    if (out == NULL) {
        return GBA_EXIT_FAULT_V2;
    }
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt == NULL ? 0 : rt->cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_MEMORY_FAULT_NONE_V3;

    if (rt == NULL) {
        return fail(rt, out, 0, GBA_MEMORY_FAULT_RUNTIME_V3);
    }
    if (rt->abi_version != GBA_ABI_VERSION_V3 ||
        rt->cpu.abi_version != GBA_ABI_VERSION_V2) {
        return fail(rt, out, rt->cpu.r[13], GBA_MEMORY_FAULT_ABI_V3);
    }
    if (rt->memory == NULL) {
        return fail(rt, out, rt->cpu.r[13], GBA_MEMORY_FAULT_NULL_MEMORY_V3);
    }
    if (register_list == 0 || (register_list & ~UINT32_C(0x1FF)) != 0) {
        return fail(rt, out, rt->cpu.r[13], GBA_MEMORY_FAULT_REGISTER_LIST_V3);
    }

    uint32_t count = 0;
    for (uint32_t bits = register_list; bits != 0; bits >>= 1) {
        count += bits & UINT32_C(1);
    }
    const uint32_t total_bytes = count * UINT32_C(4);
    const uint32_t old_sp = rt->cpu.r[13];
    const uint32_t new_sp = old_sp - total_bytes;
    if ((new_sp & UINT32_C(3)) != 0) {
        return fail(rt, out, new_sp, GBA_MEMORY_FAULT_ALIGNMENT_V3);
    }

    const uint64_t end = (uint64_t)new_sp + total_bytes;
    uint8_t *destination;
    if (new_sp >= UINT32_C(0x02000000) &&
        end <= UINT64_C(0x02040000)) {
        destination = &rt->memory->ewram[new_sp - UINT32_C(0x02000000)];
    } else if (new_sp >= UINT32_C(0x03000000) &&
               end <= UINT64_C(0x03008000)) {
        destination = &rt->memory->iwram[new_sp - UINT32_C(0x03000000)];
    } else {
        return fail(rt, out, new_sp, GBA_MEMORY_FAULT_REGION_V3);
    }

    uint32_t offset = 0;
    for (uint32_t reg = 0; reg < 8; ++reg) {
        if ((register_list & (UINT32_C(1) << reg)) == 0) {
            continue;
        }
        store_le32(&destination[offset], rt->cpu.r[reg]);
        offset += UINT32_C(4);
    }
    if ((register_list & UINT32_C(0x100)) != 0) {
        store_le32(&destination[offset], rt->cpu.r[14]);
    }

    rt->cpu.r[13] = new_sp;
    return GBA_EXIT_CONTINUE_V2;
}

uint32_t gba_pop_thumb_v3(GbaRuntimeV3 *rt, GbaExitV3 *out,
                          uint32_t register_list) {
    if (out == NULL) {
        return GBA_EXIT_FAULT_V2;
    }
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt == NULL ? 0 : rt->cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_MEMORY_FAULT_NONE_V3;

    if (rt == NULL) {
        return fail(rt, out, 0, GBA_MEMORY_FAULT_RUNTIME_V3);
    }
    const uint32_t old_sp = rt->cpu.r[13];
    if (rt->abi_version != GBA_ABI_VERSION_V3 ||
        rt->cpu.abi_version != GBA_ABI_VERSION_V2) {
        return fail(rt, out, old_sp, GBA_MEMORY_FAULT_ABI_V3);
    }
    if (rt->memory == NULL) {
        return fail(rt, out, old_sp, GBA_MEMORY_FAULT_NULL_MEMORY_V3);
    }
    if (register_list == 0 ||
        (register_list & ~UINT32_C(0xFF)) != 0) {
        return fail(rt, out, old_sp, GBA_MEMORY_FAULT_REGISTER_LIST_V3);
    }
    if ((old_sp & UINT32_C(3)) != 0) {
        return fail(rt, out, old_sp, GBA_MEMORY_FAULT_ALIGNMENT_V3);
    }

    uint32_t count = 0;
    for (uint32_t bits = register_list; bits != 0; bits >>= 1) {
        count += bits & UINT32_C(1);
    }
    const uint32_t total_bytes = count * UINT32_C(4);
    const uint64_t end = (uint64_t)old_sp + total_bytes;
    uint8_t *source;
    uint32_t region_end;
    if (old_sp >= UINT32_C(0x02000000) &&
        old_sp < UINT32_C(0x02040000)) {
        source = &rt->memory->ewram[old_sp - UINT32_C(0x02000000)];
        region_end = UINT32_C(0x02040000);
    } else if (old_sp >= UINT32_C(0x03000000) &&
               old_sp < UINT32_C(0x03008000)) {
        source = &rt->memory->iwram[old_sp - UINT32_C(0x03000000)];
        region_end = UINT32_C(0x03008000);
    } else {
        return fail(rt, out, old_sp, GBA_MEMORY_FAULT_REGION_V3);
    }
    if (end > region_end) {
        return fail(rt, out, region_end, GBA_MEMORY_FAULT_REGION_V3);
    }

    uint32_t values[8] = {0};
    uint32_t offset = 0;
    for (uint32_t reg = 0; reg < 8; ++reg) {
        if ((register_list & (UINT32_C(1) << reg)) == 0) {
            continue;
        }
        values[reg] = load_le32(&source[offset]);
        offset += UINT32_C(4);
    }

    for (uint32_t reg = 0; reg < 8; ++reg) {
        if ((register_list & (UINT32_C(1) << reg)) != 0) {
            rt->cpu.r[reg] = values[reg];
        }
    }
    rt->cpu.r[13] = old_sp + total_bytes;
    return GBA_EXIT_CONTINUE_V2;
}

uint32_t gba_ldmia_thumb_v3(GbaRuntimeV3 *rt, GbaExitV3 *out,
                            uint32_t base_register, uint32_t register_list) {
    if (out == NULL) {
        return GBA_EXIT_FAULT_V2;
    }
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt == NULL ? 0 : rt->cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_MEMORY_FAULT_NONE_V3;

    if (rt == NULL) {
        return fail(rt, out, 0, GBA_MEMORY_FAULT_RUNTIME_V3);
    }
    const uint32_t guest_base = base_register < 8
                                    ? rt->cpu.r[base_register]
                                    : 0;
    if (rt->abi_version != GBA_ABI_VERSION_V3 ||
        rt->cpu.abi_version != GBA_ABI_VERSION_V2) {
        return fail(rt, out, guest_base, GBA_MEMORY_FAULT_ABI_V3);
    }
    if (rt->memory == NULL) {
        return fail(rt, out, guest_base, GBA_MEMORY_FAULT_NULL_MEMORY_V3);
    }
    if (base_register > 7 || register_list == 0 ||
        (register_list & ~UINT32_C(0xFF)) != 0) {
        return fail(rt, out, guest_base, GBA_MEMORY_FAULT_REGISTER_LIST_V3);
    }

    const uint32_t old_base = guest_base;
    uint32_t count = 0;
    for (uint32_t bits = register_list; bits != 0; bits >>= 1) {
        count += bits & UINT32_C(1);
    }
    const uint32_t total_bytes = count * UINT32_C(4);
    uint32_t offsets[8] = {0};
    uint32_t transfer_index = 0;
    int region = 0;
    for (uint32_t reg = 0; reg < 8; ++reg) {
        if ((register_list & (UINT32_C(1) << reg)) == 0) {
            continue;
        }
        const uint32_t guest_ea =
            old_base + (transfer_index * UINT32_C(4));
        const uint32_t aligned_ea = guest_ea & ~UINT32_C(3);
        offsets[transfer_index] = aligned_ea;
        int this_region;
        if (aligned_ea >= UINT32_C(0x02000000) &&
            aligned_ea <= UINT32_C(0x0203FFFC)) {
            this_region = 1;
        } else if (aligned_ea >= UINT32_C(0x03000000) &&
                   aligned_ea <= UINT32_C(0x03007FFC)) {
            this_region = 2;
        } else {
            return fail(rt, out, aligned_ea, GBA_MEMORY_FAULT_REGION_V3);
        }
        if (transfer_index == 0) {
            region = this_region;
        } else if (this_region != region) {
            return fail(rt, out, aligned_ea, GBA_MEMORY_FAULT_REGION_V3);
        }
        ++transfer_index;
    }

    uint8_t *region_bytes = region == 1 ? rt->memory->ewram : rt->memory->iwram;
    const uint32_t region_base = region == 1 ? UINT32_C(0x02000000)
                                             : UINT32_C(0x03000000);
    uint32_t values[8] = {0};
    transfer_index = 0;
    for (uint32_t reg = 0; reg < 8; ++reg) {
        if ((register_list & (UINT32_C(1) << reg)) == 0) {
            continue;
        }
        const uint32_t offset = offsets[transfer_index] - region_base;
        values[reg] = load_le32(&region_bytes[offset]);
        ++transfer_index;
    }

    for (uint32_t reg = 0; reg < 8; ++reg) {
        if ((register_list & (UINT32_C(1) << reg)) != 0) {
            rt->cpu.r[reg] = values[reg];
        }
    }
    if ((register_list & (UINT32_C(1) << base_register)) == 0) {
        rt->cpu.r[base_register] = old_base + total_bytes;
    }
    return GBA_EXIT_CONTINUE_V2;
}

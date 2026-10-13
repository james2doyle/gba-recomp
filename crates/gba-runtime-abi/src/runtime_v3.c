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

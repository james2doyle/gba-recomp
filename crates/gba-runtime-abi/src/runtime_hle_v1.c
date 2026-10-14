#include "gba_abi_v4.h"

#include <string.h>

enum {
    IO_DISPCNT = 0x000,
    IO_DISPSTAT = 0x004,
    IO_VCOUNT = 0x006,
    IO_BG0CNT = 0x008,
    IO_BLDY = 0x054,
    IO_SOUND1CNT_LO = 0x060,
    IO_SOUND1CNT_HI = 0x062,
    IO_SOUND1CNT_X = 0x064,
    IO_SOUND2CNT_LO = 0x068,
    IO_SOUND2CNT_HI = 0x06C,
    IO_SOUND3CNT_LO = 0x070,
    IO_SOUND3CNT_HI = 0x072,
    IO_SOUND3CNT_X = 0x074,
    IO_SOUND4CNT_LO = 0x078,
    IO_SOUND4CNT_HI = 0x07C,
    IO_SOUNDCNT_LO = 0x080,
    IO_SOUNDCNT_HI = 0x082,
    IO_SOUNDCNT_X = 0x084,
    IO_SOUNDBIAS = 0x088,
    IO_DMA0SAD_LO = 0x0B0,
    IO_DMA3CNT_HI = 0x0DE,
    IO_TM0CNT_LO = 0x100,
    IO_TM3CNT_HI = 0x10E,
    IO_SIOCNT = 0x128,
    IO_SIOMLT_SEND = 0x12A,
    IO_RCNT = 0x134,
    IO_JOYCNT = 0x140,
    IO_JOY_RECV_LO = 0x150,
    IO_JOY_TRANS_LO = 0x154,
    IO_IE = 0x200,
    IO_IF = 0x202,
    IO_WAITCNT = 0x204,
    IO_IME = 0x208,
};

static uint16_t load_le16(const uint8_t *bytes) {
    return (uint16_t)bytes[0] | ((uint16_t)bytes[1] << 8);
}

static void store_raw16(uint8_t *bytes, uint32_t offset, uint16_t value) {
    bytes[offset] = (uint8_t)value;
    bytes[offset + 1] = (uint8_t)(value >> 8);
}

static void store_io16(GbaMachineStateV1 *machine, uint32_t offset,
                       uint16_t value) {
    if (offset == IO_VCOUNT) {
        return;
    }
    if (offset == IO_IME) {
        value &= UINT16_C(0x0001);
    } else if (offset == IO_DISPSTAT) {
        const uint16_t old_status = load_le16(&machine->io[offset]) & 0x0003;
        const uint16_t vcount = load_le16(&machine->io[IO_VCOUNT]);
        const uint16_t compare = (value >> 8) & 0x00FF;
        value = (uint16_t)(old_status | (value & 0xFFF8));
        if (vcount == compare) {
            value |= UINT16_C(0x0004);
        }
    } else if (offset == IO_IF) {
        value = (uint16_t)(load_le16(&machine->io[offset]) & (uint16_t)~value);
    } else if (offset == IO_SOUNDCNT_X) {
        value = (uint16_t)((value & UINT16_C(0x0080)) |
                           (load_le16(&machine->io[offset]) & UINT16_C(0x000F)));
    }
    store_raw16(machine->io, offset, value);
}

static void store_io32(GbaMachineStateV1 *machine, uint32_t offset,
                       uint32_t value) {
    store_io16(machine, offset, (uint16_t)value);
    store_io16(machine, offset + 2, (uint16_t)(value >> 16));
}

static uint32_t fail(GbaRuntimeV4 *rt, GbaExitV3 *out, GbaHleFaultV1 code) {
    if (out != NULL) {
        out->reason = GBA_EXIT_FAULT_V2;
        out->next_pc = rt == NULL ? 0 : rt->base.cpu.r[15];
        out->fault_address = 0;
        out->fault_code = (uint32_t)code;
    }
    return GBA_EXIT_FAULT_V2;
}

uint32_t gba_validate_runtime_v4(GbaRuntimeV4 *rt, GbaExitV3 *out) {
    if (rt == NULL || out == NULL) {
        return fail(rt, out, GBA_HLE_FAULT_RUNTIME_V1);
    }
    if (rt->base.cpu.abi_version != GBA_ABI_VERSION_V2) {
        return fail(rt, out, GBA_HLE_FAULT_CPU_ABI_V1);
    }
    if (rt->base.abi_version != GBA_ABI_VERSION_V3 ||
        rt->abi_version != GBA_ABI_VERSION_V4) {
        return fail(rt, out, GBA_HLE_FAULT_RUNTIME_ABI_V1);
    }
    if (rt->machine == NULL) {
        return fail(rt, out, GBA_HLE_FAULT_MACHINE_V1);
    }
    if (rt->machine->abi_version != GBA_MACHINE_STATE_VERSION_V1) {
        return fail(rt, out, GBA_HLE_FAULT_MACHINE_ABI_V1);
    }
    if (rt->base.memory != &rt->machine->memory) {
        return fail(rt, out, GBA_HLE_FAULT_MEMORY_MISMATCH_V1);
    }
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt->base.cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_HLE_FAULT_NONE_V1;
    return GBA_EXIT_CONTINUE_V2;
}

static uint32_t fail_store16_v4(GbaRuntimeV4 *rt, GbaExitV3 *out,
                                uint32_t guest_ea, uint32_t fault_code) {
    if (out != NULL) {
        out->reason = GBA_EXIT_FAULT_V2;
        out->next_pc = rt == NULL ? 0 : rt->base.cpu.r[15];
        out->fault_address = guest_ea;
        out->fault_code = fault_code;
    }
    return GBA_EXIT_FAULT_V2;
}

static uint32_t fail_load16_v4(GbaRuntimeV4 *rt, GbaExitV3 *out,
                               uint32_t guest_ea, uint32_t fault_code) {
    if (out != NULL) {
        out->reason = GBA_EXIT_FAULT_V2;
        out->next_pc = rt == NULL ? 0 : rt->base.cpu.r[15];
        out->fault_address = guest_ea;
        out->fault_code = fault_code;
    }
    return GBA_EXIT_FAULT_V2;
}

uint32_t gba_load16_v4(GbaRuntimeV4 *rt, GbaExitV3 *out,
                       uint32_t guest_ea, uint32_t *host_value_out) {
    if (out == NULL || host_value_out == NULL) {
        return GBA_EXIT_FAULT_V2;
    }
    const uint32_t validation = gba_validate_runtime_v4(rt, out);
    if (validation != GBA_EXIT_CONTINUE_V2) {
        out->fault_address = guest_ea;
        return validation;
    }

    const uint32_t aligned_ea = guest_ea & ~UINT32_C(1);
    if ((aligned_ea >= UINT32_C(0x02000000) &&
         aligned_ea <= UINT32_C(0x0203FFFE)) ||
        (aligned_ea >= UINT32_C(0x03000000) &&
         aligned_ea <= UINT32_C(0x03007FFE))) {
        return gba_load16_v3(&rt->base, out, guest_ea, host_value_out);
    }

    uint32_t io_offset;
    if (aligned_ea == UINT32_C(0x04000200)) {
        io_offset = IO_IE;
    } else if (aligned_ea == UINT32_C(0x04000202)) {
        io_offset = IO_IF;
    } else if (aligned_ea == UINT32_C(0x04000208)) {
        io_offset = IO_IME;
    } else {
        return fail_load16_v4(rt, out, guest_ea, GBA_MEMORY_FAULT_REGION_V3);
    }

    uint32_t value = (uint32_t)load_le16(&rt->machine->io[io_offset]);
    if ((guest_ea & UINT32_C(1)) != 0) {
        value = (value >> 8) | (value << 24);
    }
    *host_value_out = value;
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt->base.cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_MEMORY_FAULT_NONE_V3;
    return GBA_EXIT_CONTINUE_V2;
}

uint32_t gba_store16_v4(GbaRuntimeV4 *rt, GbaExitV3 *out,
                        uint32_t guest_ea, uint32_t value) {
    const uint32_t validation = gba_validate_runtime_v4(rt, out);
    if (validation != GBA_EXIT_CONTINUE_V2) {
        if (out != NULL) {
            out->fault_address = guest_ea;
        }
        return validation;
    }

    const uint32_t aligned_ea = guest_ea & ~UINT32_C(1);
    if ((aligned_ea >= UINT32_C(0x02000000) &&
         aligned_ea <= UINT32_C(0x0203FFFE)) ||
        (aligned_ea >= UINT32_C(0x03000000) &&
         aligned_ea <= UINT32_C(0x03007FFE))) {
        return gba_store16_v3(&rt->base, out, guest_ea, value);
    }

    if (aligned_ea != UINT32_C(0x04000208)) {
        return fail_store16_v4(rt, out, guest_ea,
                               GBA_MEMORY_FAULT_REGION_V3);
    }
    if ((value & UINT32_C(1)) != 0) {
        return fail_store16_v4(rt, out, guest_ea,
                               GBA_MEMORY_FAULT_IO_EFFECT_V4);
    }

    store_io16(rt->machine, IO_IME, (uint16_t)value);
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt->base.cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_MEMORY_FAULT_NONE_V3;
    return GBA_EXIT_CONTINUE_V2;
}

uint32_t gba_store16_v5(GbaRuntimeV4 *rt, GbaExitV3 *out,
                        uint32_t guest_ea, uint32_t value) {
    const uint32_t validation = gba_validate_runtime_v4(rt, out);
    if (validation != GBA_EXIT_CONTINUE_V2) {
        if (out != NULL) {
            out->fault_address = guest_ea;
        }
        return validation;
    }

    const uint32_t aligned_ea = guest_ea & ~UINT32_C(1);
    if ((aligned_ea >= UINT32_C(0x02000000) &&
         aligned_ea <= UINT32_C(0x0203FFFE)) ||
        (aligned_ea >= UINT32_C(0x03000000) &&
         aligned_ea <= UINT32_C(0x03007FFE))) {
        return gba_store16_v3(&rt->base, out, guest_ea, value);
    }

    uint32_t io_offset;
    if (aligned_ea == UINT32_C(0x04000200)) {
        io_offset = IO_IE;
    } else if (aligned_ea == UINT32_C(0x04000208)) {
        io_offset = IO_IME;
    } else if (aligned_ea == UINT32_C(0x04000202)) {
        return fail_store16_v4(rt, out, guest_ea,
                               GBA_MEMORY_FAULT_IO_EFFECT_V4);
    } else {
        return fail_store16_v4(rt, out, guest_ea,
                               GBA_MEMORY_FAULT_REGION_V3);
    }

    if (load_le16(&rt->machine->io[IO_IF]) != 0) {
        return fail_store16_v4(rt, out, guest_ea,
                               GBA_MEMORY_FAULT_IO_EFFECT_V4);
    }

    store_io16(rt->machine, io_offset, (uint16_t)value);
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt->base.cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_MEMORY_FAULT_NONE_V3;
    return GBA_EXIT_CONTINUE_V2;
}

uint32_t gba_register_ram_reset_v1(GbaRuntimeV4 *rt, GbaExitV3 *out,
                                   uint32_t immediate) {
    const uint32_t validation = gba_validate_runtime_v4(rt, out);
    if (validation != GBA_EXIT_CONTINUE_V2) {
        return validation;
    }
    if (immediate != 1) {
        return fail(rt, out, GBA_HLE_FAULT_UNSUPPORTED_SWI_V1);
    }

    const uint32_t flags = rt->base.cpu.r[0];
    GbaMachineStateV1 *machine = rt->machine;
    store_io16(machine, IO_DISPCNT, UINT16_C(0x0080));
    if ((flags & UINT32_C(0x01)) != 0) {
        memset(machine->memory.ewram, 0, sizeof(machine->memory.ewram));
    }
    if ((flags & UINT32_C(0x02)) != 0) {
        memset(machine->memory.iwram, 0, sizeof(machine->memory.iwram) - 0x200);
    }
    if ((flags & UINT32_C(0x04)) != 0) {
        memset(machine->palette_ram, 0, sizeof(machine->palette_ram));
    }
    if ((flags & UINT32_C(0x08)) != 0) {
        memset(machine->vram, 0, sizeof(machine->vram));
    }
    if ((flags & UINT32_C(0x10)) != 0) {
        memset(machine->oam, 0, sizeof(machine->oam));
    }
    if ((flags & UINT32_C(0x20)) != 0) {
        store_io16(machine, IO_SIOCNT, 0);
        store_io16(machine, IO_RCNT, UINT16_C(0x8000));
        store_io16(machine, IO_SIOMLT_SEND, 0);
        store_io16(machine, IO_JOYCNT, 0);
        store_io32(machine, IO_JOY_RECV_LO, 0);
        store_io32(machine, IO_JOY_TRANS_LO, 0);
    }
    if ((flags & UINT32_C(0x40)) != 0) {
        static const uint16_t sound_registers[] = {
            IO_SOUND1CNT_LO, IO_SOUND1CNT_HI, IO_SOUND1CNT_X,
            IO_SOUND2CNT_LO, IO_SOUND2CNT_HI, IO_SOUND3CNT_LO,
            IO_SOUND3CNT_HI, IO_SOUND3CNT_X, IO_SOUND4CNT_LO,
            IO_SOUND4CNT_HI, IO_SOUNDCNT_LO, IO_SOUNDCNT_HI,
            IO_SOUNDCNT_X,
        };
        for (size_t i = 0; i < sizeof(sound_registers) / sizeof(sound_registers[0]); ++i) {
            store_io16(machine, sound_registers[i], 0);
        }
        store_io16(machine, IO_SOUNDBIAS, UINT16_C(0x0200));
        memset(machine->wave_ram, 0, sizeof(machine->wave_ram));
    }
    if ((flags & UINT32_C(0x80)) != 0) {
        store_io16(machine, IO_DISPSTAT, 0);
        store_io16(machine, IO_VCOUNT, 0);
        for (uint32_t offset = IO_BG0CNT; offset <= IO_BLDY; offset += 2) {
            uint16_t value = 0;
            if (offset == 0x020 || offset == 0x026 ||
                offset == 0x030 || offset == 0x036) {
                value = UINT16_C(0x0100);
            }
            store_io16(machine, offset, value);
        }
        for (uint32_t offset = IO_DMA0SAD_LO; offset <= IO_DMA3CNT_HI; offset += 2) {
            store_io16(machine, offset, 0);
        }
        for (uint32_t offset = IO_TM0CNT_LO; offset <= IO_TM3CNT_HI; offset += 2) {
            store_io16(machine, offset, 0);
        }
        store_io16(machine, IO_IE, 0);
        store_io16(machine, IO_IF, UINT16_MAX);
        store_io16(machine, IO_WAITCNT, 0);
        store_io16(machine, IO_IME, 0);
    }
    out->reason = GBA_EXIT_CONTINUE_V2;
    out->next_pc = rt->base.cpu.r[15];
    out->fault_address = 0;
    out->fault_code = GBA_HLE_FAULT_NONE_V1;
    return GBA_EXIT_CONTINUE_V2;
}

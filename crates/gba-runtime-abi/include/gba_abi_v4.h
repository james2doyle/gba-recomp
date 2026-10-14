#ifndef GBA_RECOMP_ABI_V4_H
#define GBA_RECOMP_ABI_V4_H

#include "gba_abi_v3.h"

#define GBA_ABI_VERSION_V4 UINT32_C(4)
#define GBA_MACHINE_STATE_VERSION_V1 UINT32_C(1)
#define GBA_PALETTE_RAM_SIZE_V1 UINT32_C(0x00000400)
#define GBA_VRAM_SIZE_V1 UINT32_C(0x00018000)
#define GBA_OAM_SIZE_V1 UINT32_C(0x00000400)
#define GBA_IO_LATCH_SIZE_V1 UINT32_C(0x00000400)
#define GBA_WAVE_RAM_SIZE_V1 UINT32_C(32)
#define GBA_BIOS_HLE_VERSION_V1 UINT32_C(1)

typedef struct GbaMachineStateV1 {
    GbaMemoryV1 memory;
    uint32_t abi_version;
    uint8_t palette_ram[GBA_PALETTE_RAM_SIZE_V1];
    uint8_t vram[GBA_VRAM_SIZE_V1];
    uint8_t oam[GBA_OAM_SIZE_V1];
    uint8_t io[GBA_IO_LATCH_SIZE_V1];
    uint8_t wave_ram[GBA_WAVE_RAM_SIZE_V1];
} GbaMachineStateV1;

typedef struct GbaRuntimeV4 {
    GbaRuntimeV3 base;
    GbaMachineStateV1 *machine;
    uint32_t abi_version;
} GbaRuntimeV4;

typedef enum GbaHleFaultV1 {
    GBA_HLE_FAULT_NONE_V1 = 0,
    GBA_HLE_FAULT_RUNTIME_V1 = 16,
    GBA_HLE_FAULT_CPU_ABI_V1 = 17,
    GBA_HLE_FAULT_RUNTIME_ABI_V1 = 18,
    GBA_HLE_FAULT_MACHINE_V1 = 19,
    GBA_HLE_FAULT_MACHINE_ABI_V1 = 20,
    GBA_HLE_FAULT_MEMORY_MISMATCH_V1 = 21,
    GBA_HLE_FAULT_UNSUPPORTED_SWI_V1 = 22
} GbaHleFaultV1;

typedef enum GbaMemoryFaultV4 {
    GBA_MEMORY_FAULT_IO_EFFECT_V4 = 8
} GbaMemoryFaultV4;

uint32_t gba_block_v4(GbaRuntimeV4 *rt, GbaExitV3 *out);
uint32_t gba_validate_runtime_v4(GbaRuntimeV4 *rt, GbaExitV3 *out);
uint32_t gba_load16_v4(GbaRuntimeV4 *rt, GbaExitV3 *out,
                       uint32_t guest_ea, uint32_t *host_value_out);
uint32_t gba_store16_v4(GbaRuntimeV4 *rt, GbaExitV3 *out,
                        uint32_t guest_ea, uint32_t value);
uint32_t gba_store16_v5(GbaRuntimeV4 *rt, GbaExitV3 *out,
                        uint32_t guest_ea, uint32_t value);
uint32_t gba_register_ram_reset_v1(GbaRuntimeV4 *rt, GbaExitV3 *out,
                                   uint32_t immediate);

_Static_assert(offsetof(GbaMachineStateV1, memory) == 0,
               "GbaMachineStateV1 memory prefix changed");
_Static_assert(offsetof(GbaMachineStateV1, abi_version) == 0x48000,
               "GbaMachineStateV1 version offset changed");
_Static_assert(offsetof(GbaMachineStateV1, palette_ram) == 0x48004,
               "GbaMachineStateV1 palette offset changed");
_Static_assert(offsetof(GbaMachineStateV1, vram) == 0x48404,
               "GbaMachineStateV1 VRAM offset changed");
_Static_assert(offsetof(GbaMachineStateV1, oam) == 0x60404,
               "GbaMachineStateV1 OAM offset changed");
_Static_assert(offsetof(GbaMachineStateV1, io) == 0x60804,
               "GbaMachineStateV1 IO offset changed");
_Static_assert(offsetof(GbaMachineStateV1, wave_ram) == 0x60C04,
               "GbaMachineStateV1 wave RAM offset changed");
_Static_assert(sizeof(GbaMachineStateV1) == 0x60C24,
               "GbaMachineStateV1 size changed");
_Static_assert(_Alignof(GbaMachineStateV1) == 4,
               "GbaMachineStateV1 alignment changed");
_Static_assert(sizeof(GbaRuntimeV4) == 224, "GbaRuntimeV4 size changed");
_Static_assert(_Alignof(GbaRuntimeV4) == 8, "GbaRuntimeV4 alignment changed");
_Static_assert(offsetof(GbaRuntimeV4, base) == 0, "GbaRuntimeV4 prefix changed");
_Static_assert(offsetof(GbaRuntimeV4, machine) == 208,
               "GbaRuntimeV4 machine offset changed");
_Static_assert(offsetof(GbaRuntimeV4, abi_version) == 216,
               "GbaRuntimeV4 version offset changed");

#endif

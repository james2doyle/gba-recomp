#ifndef GBA_RECOMP_ABI_V3_H
#define GBA_RECOMP_ABI_V3_H

#include "gba_abi_v2.h"

#define GBA_ABI_VERSION_V3 UINT32_C(3)
#define GBA_EWRAM_SIZE_V1 UINT32_C(0x00040000)
#define GBA_IWRAM_SIZE_V1 UINT32_C(0x00008000)

typedef enum GbaMemoryFaultV3 {
    GBA_MEMORY_FAULT_NONE_V3 = 0,
    GBA_MEMORY_FAULT_RUNTIME_V3 = 1,
    GBA_MEMORY_FAULT_ABI_V3 = 2,
    GBA_MEMORY_FAULT_NULL_MEMORY_V3 = 3,
    GBA_MEMORY_FAULT_ALIGNMENT_V3 = 4,
    GBA_MEMORY_FAULT_REGION_V3 = 5
} GbaMemoryFaultV3;

typedef struct GbaMemoryV1 {
    uint8_t ewram[GBA_EWRAM_SIZE_V1];
    uint8_t iwram[GBA_IWRAM_SIZE_V1];
} GbaMemoryV1;

typedef struct GbaRuntimeV3 {
    GbaRuntimeV2 cpu;
    GbaMemoryV1 *memory;
    uint32_t abi_version;
} GbaRuntimeV3;

typedef struct GbaExitV3 {
    uint32_t reason;
    uint32_t next_pc;
    uint32_t fault_address;
    uint32_t fault_code;
} GbaExitV3;

uint32_t gba_block_v3(GbaRuntimeV3 *rt, GbaExitV3 *out);
uint32_t gba_store32_v3(GbaRuntimeV3 *rt, GbaExitV3 *out,
                        uint32_t guest_address, uint32_t value);

_Static_assert(sizeof(GbaMemoryV1) == 0x48000, "GbaMemoryV1 size changed");
_Static_assert(_Alignof(GbaMemoryV1) == 1, "GbaMemoryV1 alignment changed");
_Static_assert(sizeof(GbaRuntimeV3) == 208, "GbaRuntimeV3 size changed");
_Static_assert(_Alignof(GbaRuntimeV3) == 8, "GbaRuntimeV3 alignment changed");
_Static_assert(offsetof(GbaRuntimeV3, cpu) == 0, "GbaRuntimeV3 CPU offset changed");
_Static_assert(offsetof(GbaRuntimeV3, memory) == 192, "GbaRuntimeV3 memory offset changed");
_Static_assert(offsetof(GbaRuntimeV3, abi_version) == 200, "GbaRuntimeV3 version offset changed");
_Static_assert(sizeof(GbaExitV3) == 16, "GbaExitV3 size changed");
_Static_assert(_Alignof(GbaExitV3) == 4, "GbaExitV3 alignment changed");
_Static_assert(offsetof(GbaExitV3, reason) == 0, "GbaExitV3 reason offset changed");
_Static_assert(offsetof(GbaExitV3, next_pc) == 4, "GbaExitV3 PC offset changed");
_Static_assert(offsetof(GbaExitV3, fault_address) == 8, "GbaExitV3 address offset changed");
_Static_assert(offsetof(GbaExitV3, fault_code) == 12, "GbaExitV3 code offset changed");

#endif

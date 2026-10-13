#ifndef GBA_RECOMP_ABI_V1_H
#define GBA_RECOMP_ABI_V1_H

#include <stddef.h>
#include <stdint.h>

#define GBA_ABI_VERSION_V1 UINT32_C(1)

typedef enum GbaExitReasonV1 {
    GBA_EXIT_CONTINUE = 0,
    GBA_EXIT_EVENT = 1,
    GBA_EXIT_HALT = 2,
    GBA_EXIT_MISSING_CODE = 3,
    GBA_EXIT_FAULT = 4,
    GBA_EXIT_INVALIDATED = 5,
    GBA_EXIT_STOP = 6
} GbaExitReasonV1;

typedef struct GbaRuntimeV1 {
    uint32_t r[16];
    uint32_t cpsr;
    uint32_t abi_version;
    uint64_t guest_cycle;
} GbaRuntimeV1;

typedef struct GbaExitV1 {
    uint32_t reason;
    uint32_t next_pc;
} GbaExitV1;

uint32_t gba_block_v1(GbaRuntimeV1 *rt, GbaExitV1 *out);

_Static_assert(sizeof(GbaRuntimeV1) == 80, "GbaRuntimeV1 size changed");
_Static_assert(_Alignof(GbaRuntimeV1) == 8, "GbaRuntimeV1 alignment changed");
_Static_assert(offsetof(GbaRuntimeV1, r) == 0, "GbaRuntimeV1 registers offset changed");
_Static_assert(offsetof(GbaRuntimeV1, cpsr) == 64, "GbaRuntimeV1 cpsr offset changed");
_Static_assert(offsetof(GbaRuntimeV1, abi_version) == 68, "GbaRuntimeV1 abi_version offset changed");
_Static_assert(offsetof(GbaRuntimeV1, guest_cycle) == 72, "GbaRuntimeV1 guest_cycle offset changed");
_Static_assert(sizeof(GbaExitV1) == 8, "GbaExitV1 size changed");
_Static_assert(_Alignof(GbaExitV1) == 4, "GbaExitV1 alignment changed");
_Static_assert(offsetof(GbaExitV1, reason) == 0, "GbaExitV1 reason offset changed");
_Static_assert(offsetof(GbaExitV1, next_pc) == 4, "GbaExitV1 next_pc offset changed");

#endif

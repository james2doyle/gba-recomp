#ifndef GBA_RECOMP_ABI_V2_H
#define GBA_RECOMP_ABI_V2_H

#include <stddef.h>
#include <stdint.h>

#define GBA_ABI_VERSION_V2 UINT32_C(2)
#define GBA_PSR_FIELD_C UINT32_C(0x1)
#define GBA_PSR_FIELD_F UINT32_C(0x8)

typedef enum GbaExitReasonV2 {
    GBA_EXIT_CONTINUE_V2 = 0,
    GBA_EXIT_MISSING_CODE_V2 = 3,
    GBA_EXIT_FAULT_V2 = 4
} GbaExitReasonV2;

typedef enum GbaRegisterBankV2 {
    GBA_BANK_USER_SYSTEM_V2 = 0,
    GBA_BANK_FIQ_V2 = 1,
    GBA_BANK_IRQ_V2 = 2,
    GBA_BANK_SUPERVISOR_V2 = 3,
    GBA_BANK_ABORT_V2 = 4,
    GBA_BANK_UNDEFINED_V2 = 5
} GbaRegisterBankV2;

typedef struct GbaRuntimeV2 {
    uint32_t r[16];
    uint32_t cpsr;
    uint32_t abi_version;
    uint64_t guest_cycle;
    uint32_t common_r8_r12[5];
    uint32_t fiq_r8_r12[5];
    uint32_t banked_r13_r14[6][2];
    uint32_t spsr[5];
} GbaRuntimeV2;

typedef struct GbaExitV2 {
    uint32_t reason;
    uint32_t next_pc;
} GbaExitV2;

uint32_t gba_block_v2(GbaRuntimeV2 *rt, GbaExitV2 *out);
uint32_t gba_msr_cpsr_v2(GbaRuntimeV2 *rt, uint32_t operand, uint32_t field_mask);

_Static_assert(sizeof(GbaRuntimeV2) == 192, "GbaRuntimeV2 size changed");
_Static_assert(_Alignof(GbaRuntimeV2) == 8, "GbaRuntimeV2 alignment changed");
_Static_assert(offsetof(GbaRuntimeV2, r) == 0, "GbaRuntimeV2 registers offset changed");
_Static_assert(offsetof(GbaRuntimeV2, cpsr) == 64, "GbaRuntimeV2 cpsr offset changed");
_Static_assert(offsetof(GbaRuntimeV2, abi_version) == 68, "GbaRuntimeV2 version offset changed");
_Static_assert(offsetof(GbaRuntimeV2, guest_cycle) == 72, "GbaRuntimeV2 cycle offset changed");
_Static_assert(offsetof(GbaRuntimeV2, common_r8_r12) == 80, "GbaRuntimeV2 common bank offset changed");
_Static_assert(offsetof(GbaRuntimeV2, fiq_r8_r12) == 100, "GbaRuntimeV2 FIQ bank offset changed");
_Static_assert(offsetof(GbaRuntimeV2, banked_r13_r14) == 120, "GbaRuntimeV2 SP/LR bank offset changed");
_Static_assert(offsetof(GbaRuntimeV2, spsr) == 168, "GbaRuntimeV2 SPSR offset changed");
_Static_assert(sizeof(GbaExitV2) == 8, "GbaExitV2 size changed");
_Static_assert(_Alignof(GbaExitV2) == 4, "GbaExitV2 alignment changed");
_Static_assert(offsetof(GbaExitV2, reason) == 0, "GbaExitV2 reason offset changed");
_Static_assert(offsetof(GbaExitV2, next_pc) == 4, "GbaExitV2 PC offset changed");

#endif

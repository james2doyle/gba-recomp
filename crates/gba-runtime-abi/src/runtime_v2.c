#include "gba_abi_v2.h"

#include <string.h>

enum {
    MODE_USER = 0x10,
    MODE_FIQ = 0x11,
    MODE_IRQ = 0x12,
    MODE_SUPERVISOR = 0x13,
    MODE_ABORT = 0x17,
    MODE_UNDEFINED = 0x1B,
    MODE_SYSTEM = 0x1F,
    CPSR_T = 0x20,
    CPSR_PRIVILEGED_CONTROL_MASK = 0xCF,
    CPSR_FLAGS_MASK = 0xF0000000
};

static int mode_bank(uint32_t mode) {
    switch (mode) {
    case MODE_USER:
    case MODE_SYSTEM:
        return GBA_BANK_USER_SYSTEM_V2;
    case MODE_FIQ:
        return GBA_BANK_FIQ_V2;
    case MODE_IRQ:
        return GBA_BANK_IRQ_V2;
    case MODE_SUPERVISOR:
        return GBA_BANK_SUPERVISOR_V2;
    case MODE_ABORT:
        return GBA_BANK_ABORT_V2;
    case MODE_UNDEFINED:
        return GBA_BANK_UNDEFINED_V2;
    default:
        return -1;
    }
}

uint32_t gba_msr_cpsr_v2(GbaRuntimeV2 *rt, uint32_t operand, uint32_t field_mask) {
    if (rt == NULL || rt->abi_version != GBA_ABI_VERSION_V2 || (field_mask & ~UINT32_C(0x9)) != 0) {
        return GBA_EXIT_FAULT_V2;
    }
    if (field_mask == 0) {
        return GBA_EXIT_CONTINUE_V2;
    }

    const uint32_t old_cpsr = rt->cpsr;
    const uint32_t old_mode = old_cpsr & UINT32_C(0x1F);
    const int old_bank = mode_bank(old_mode);
    if (old_bank < 0) {
        return GBA_EXIT_FAULT_V2;
    }

    uint32_t next_cpsr = old_cpsr;
    uint32_t next_mode = old_mode;
    if ((field_mask & GBA_PSR_FIELD_F) != 0) {
        next_cpsr = (next_cpsr & ~CPSR_FLAGS_MASK) | (operand & CPSR_FLAGS_MASK);
    }
    if ((field_mask & GBA_PSR_FIELD_C) != 0) {
        if (((old_cpsr ^ operand) & CPSR_T) != 0) {
            return GBA_EXIT_MISSING_CODE_V2;
        }
        if (old_mode != MODE_USER) {
            next_cpsr = (next_cpsr & ~CPSR_PRIVILEGED_CONTROL_MASK) |
                        (operand & CPSR_PRIVILEGED_CONTROL_MASK);
            next_mode = (old_mode & UINT32_C(0x10)) | (operand & UINT32_C(0x0F));
            if (mode_bank(next_mode) < 0) {
                return GBA_EXIT_FAULT_V2;
            }
        }
    }

    const int next_bank = mode_bank(next_mode);
    if (next_bank < 0) {
        return GBA_EXIT_FAULT_V2;
    }

    /* All validation is complete. From here, applying the register-bank change cannot fail. */
    if (old_mode != next_mode) {
        if ((old_mode == MODE_FIQ) != (next_mode == MODE_FIQ)) {
            uint32_t *old_registers = old_mode == MODE_FIQ ? rt->fiq_r8_r12 : rt->common_r8_r12;
            uint32_t *new_registers = next_mode == MODE_FIQ ? rt->fiq_r8_r12 : rt->common_r8_r12;
            memcpy(old_registers, &rt->r[8], sizeof(rt->r[8]) * 5);
            memcpy(&rt->r[8], new_registers, sizeof(rt->r[8]) * 5);
        }
        memcpy(rt->banked_r13_r14[old_bank], &rt->r[13], sizeof(rt->r[13]) * 2);
        memcpy(&rt->r[13], rt->banked_r13_r14[next_bank], sizeof(rt->r[13]) * 2);
    }
    rt->cpsr = next_cpsr;
    return GBA_EXIT_CONTINUE_V2;
}

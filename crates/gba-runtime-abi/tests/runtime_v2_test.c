#include "gba_abi_v2.h"

#include <assert.h>
#include <stdint.h>
#include <string.h>

static void init(GbaRuntimeV2 *rt, uint32_t cpsr) {
    memset(rt, 0, sizeof(*rt));
    rt->abi_version = GBA_ABI_VERSION_V2;
    rt->cpsr = cpsr;
}

static void test_mode_banks(void) {
    GbaRuntimeV2 rt;
    init(&rt, UINT32_C(0x1F));
    for (unsigned i = 0; i < 5; ++i) {
        rt.r[8 + i] = UINT32_C(0x100) + i;
        rt.fiq_r8_r12[i] = UINT32_C(0x200) + i;
    }
    rt.r[13] = UINT32_C(0x111);
    rt.r[14] = UINT32_C(0x112);
    rt.banked_r13_r14[GBA_BANK_SUPERVISOR_V2][0] = UINT32_C(0x313);
    rt.banked_r13_r14[GBA_BANK_SUPERVISOR_V2][1] = UINT32_C(0x314);
    rt.r[0] = UINT32_C(0x13);
    assert(gba_msr_cpsr_v2(&rt, rt.r[0], GBA_PSR_FIELD_C) == GBA_EXIT_CONTINUE_V2);
    assert((rt.cpsr & 0x1F) == 0x13);
    assert(rt.banked_r13_r14[GBA_BANK_USER_SYSTEM_V2][0] == UINT32_C(0x111));
    assert(rt.banked_r13_r14[GBA_BANK_USER_SYSTEM_V2][1] == UINT32_C(0x112));
    assert(rt.r[13] == UINT32_C(0x313) && rt.r[14] == UINT32_C(0x314));

    rt.r[13] = UINT32_C(0x323);
    rt.r[14] = UINT32_C(0x324);
    rt.r[0] = UINT32_C(0x11);
    assert(gba_msr_cpsr_v2(&rt, rt.r[0], GBA_PSR_FIELD_C) == GBA_EXIT_CONTINUE_V2);
    assert((rt.cpsr & 0x1F) == 0x11);
    assert(rt.common_r8_r12[0] == UINT32_C(0x100));
    assert(rt.r[8] == UINT32_C(0x200));
    assert(rt.banked_r13_r14[GBA_BANK_SUPERVISOR_V2][0] == UINT32_C(0x323));
    assert(rt.r[13] == 0);

    rt.r[8] = UINT32_C(0x900);
    rt.r[13] = UINT32_C(0x414);
    rt.r[14] = UINT32_C(0x415);
    rt.r[0] = UINT32_C(0x13);
    assert(gba_msr_cpsr_v2(&rt, rt.r[0], GBA_PSR_FIELD_C) == GBA_EXIT_CONTINUE_V2);
    assert(rt.fiq_r8_r12[0] == UINT32_C(0x900));
    assert(rt.r[8] == UINT32_C(0x100));
    assert(rt.r[13] == UINT32_C(0x323) && rt.r[14] == UINT32_C(0x324));
}

static void test_user_mode_flags_only(void) {
    GbaRuntimeV2 rt;
    GbaRuntimeV2 before;
    init(&rt, UINT32_C(0x80000010));
    rt.r[0] = UINT32_C(0xF00000D3);
    assert(gba_msr_cpsr_v2(&rt, rt.r[0], GBA_PSR_FIELD_C | GBA_PSR_FIELD_F) == GBA_EXIT_CONTINUE_V2);
    assert(rt.cpsr == UINT32_C(0xF0000010));

    rt.r[0] = UINT32_C(0xF0000030);
    before = rt;
    assert(gba_msr_cpsr_v2(&rt, rt.r[0], GBA_PSR_FIELD_C) == GBA_EXIT_MISSING_CODE_V2);
    assert(memcmp(&rt, &before, sizeof(rt)) == 0);
}

static void test_failures_are_atomic(void) {
    GbaRuntimeV2 rt;
    GbaRuntimeV2 before;
    init(&rt, UINT32_C(0x13));
    rt.r[0] = UINT32_C(0x14);
    before = rt;
    assert(gba_msr_cpsr_v2(&rt, rt.r[0], GBA_PSR_FIELD_C) == GBA_EXIT_FAULT_V2);
    assert(memcmp(&rt, &before, sizeof(rt)) == 0);

    rt.r[0] = UINT32_C(0x33);
    before = rt;
    assert(gba_msr_cpsr_v2(&rt, rt.r[0], GBA_PSR_FIELD_C) == GBA_EXIT_MISSING_CODE_V2);
    assert(memcmp(&rt, &before, sizeof(rt)) == 0);

    assert(gba_msr_cpsr_v2(&rt, rt.r[0], UINT32_C(0x2)) == GBA_EXIT_FAULT_V2);
    assert(memcmp(&rt, &before, sizeof(rt)) == 0);
    rt.abi_version = GBA_ABI_VERSION_V2 + 1;
    before = rt;
    assert(gba_msr_cpsr_v2(&rt, 0, GBA_PSR_FIELD_F) == GBA_EXIT_FAULT_V2);
    assert(memcmp(&rt, &before, sizeof(rt)) == 0);
}

int main(void) {
    test_mode_banks();
    test_user_mode_flags_only();
    test_failures_are_atomic();
    return 0;
}

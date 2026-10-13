#include "gba_abi_v1.h"

#include <inttypes.h>
#include <stdio.h>
#include <string.h>

static int check_success_case(void) {
    GbaRuntimeV1 rt = {0};
    GbaExitV1 out = {UINT32_MAX, UINT32_MAX};
    rt.r[1] = UINT32_MAX;
    rt.r[2] = 2;
    rt.r[15] = UINT32_C(0x08000000);
    rt.cpsr = UINT32_C(0xA000001F);
    rt.abi_version = GBA_ABI_VERSION_V1;
    rt.guest_cycle = 41;

    const uint32_t status = gba_block_v1(&rt, &out);
    if (status != GBA_EXIT_CONTINUE || out.reason != GBA_EXIT_CONTINUE) {
        fprintf(stderr, "unexpected success status/reason: %u/%u\n", status, out.reason);
        return 1;
    }
    if (rt.r[0] != 1 || rt.r[15] != UINT32_C(0x08000004) ||
        rt.guest_cycle != 42 || rt.cpsr != UINT32_C(0xA000001F) ||
        out.next_pc != UINT32_C(0x08000004)) {
        fprintf(stderr, "unexpected success state r0=%" PRIu32 " pc=%08" PRIx32
                        " cycle=%" PRIu64 " cpsr=%08" PRIx32 " out_pc=%08" PRIx32 "\n",
                rt.r[0], rt.r[15], rt.guest_cycle, rt.cpsr, out.next_pc);
        return 1;
    }
    return 0;
}

static int check_overflow_case(void) {
    GbaRuntimeV1 rt = {0};
    GbaRuntimeV1 before;
    GbaExitV1 out = {UINT32_MAX, UINT32_MAX};
    rt.r[0] = 17;
    rt.r[1] = UINT32_MAX;
    rt.r[2] = 2;
    rt.r[15] = UINT32_C(0x08000100);
    rt.cpsr = UINT32_C(0x6000001F);
    rt.abi_version = GBA_ABI_VERSION_V1;
    rt.guest_cycle = UINT64_MAX;
    before = rt;

    const uint32_t status = gba_block_v1(&rt, &out);
    if (status != GBA_EXIT_FAULT || out.reason != GBA_EXIT_FAULT ||
        out.next_pc != before.r[15] || memcmp(&rt, &before, sizeof(rt)) != 0) {
        fprintf(stderr, "cycle overflow did not fault atomically (status=%u reason=%u)\n",
                status, out.reason);
        return 1;
    }
    return 0;
}

static int check_abi_mismatch_case(void) {
    GbaRuntimeV1 rt = {0};
    GbaRuntimeV1 before;
    GbaExitV1 out = {UINT32_MAX, UINT32_MAX};
    rt.r[0] = 99;
    rt.r[1] = 4;
    rt.r[2] = 5;
    rt.r[15] = UINT32_C(0x08000200);
    rt.cpsr = UINT32_C(0x8000001F);
    rt.abi_version = 0;
    rt.guest_cycle = 7;
    before = rt;

    const uint32_t status = gba_block_v1(&rt, &out);
    if (status != GBA_EXIT_FAULT || out.reason != GBA_EXIT_FAULT ||
        out.next_pc != before.r[15] || memcmp(&rt, &before, sizeof(rt)) != 0) {
        fprintf(stderr, "ABI mismatch did not fault before commit (status=%u reason=%u)\n",
                status, out.reason);
        return 1;
    }
    return 0;
}

int main(void) {
    if (check_success_case() != 0 || check_overflow_case() != 0 ||
        check_abi_mismatch_case() != 0) {
        return 1;
    }
    puts("GBA-010 ALU fixture passed: O0/O2 caller checks success and overflow states");
    return 0;
}

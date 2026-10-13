#include "gba_abi_v3.h"

#include <assert.h>
#include <stdint.h>
#include <string.h>

static uint32_t read_le32(const uint8_t *source) {
    return (uint32_t)source[0] | ((uint32_t)source[1] << 8) |
           ((uint32_t)source[2] << 16) | ((uint32_t)source[3] << 24);
}

static void init(GbaRuntimeV3 *rt, GbaMemoryV1 *memory) {
    memset(rt, 0, sizeof(*rt));
    memset(memory, 0, sizeof(*memory));
    rt->cpu.abi_version = GBA_ABI_VERSION_V2;
    rt->cpu.r[15] = UINT32_C(0x08000224);
    rt->memory = memory;
    rt->abi_version = GBA_ABI_VERSION_V3;
}

static void test_physical_ram_boundaries_and_little_endian(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaExitV3 exit;
    init(&rt, &memory);

    assert(gba_store32_v3(&rt, &exit, UINT32_C(0x02000000), UINT32_C(0x12345678)) ==
           GBA_EXIT_CONTINUE_V2);
    assert(memory.ewram[0] == 0x78 && memory.ewram[1] == 0x56 &&
           memory.ewram[2] == 0x34 && memory.ewram[3] == 0x12);
    assert(exit.reason == GBA_EXIT_CONTINUE_V2);
    assert(exit.next_pc == UINT32_C(0x08000224));
    assert(exit.fault_address == 0 && exit.fault_code == GBA_MEMORY_FAULT_NONE_V3);

    assert(gba_store32_v3(&rt, &exit, UINT32_C(0x0203FFFC), UINT32_C(0xAABBCCDD)) ==
           GBA_EXIT_CONTINUE_V2);
    assert(memory.ewram[0x3FFFC] == 0xDD && memory.ewram[0x3FFFF] == 0xAA);

    assert(gba_store32_v3(&rt, &exit, UINT32_C(0x03000000), UINT32_C(0x0BADF00D)) ==
           GBA_EXIT_CONTINUE_V2);
    assert(memory.iwram[0] == 0x0D && memory.iwram[1] == 0xF0 &&
           memory.iwram[2] == 0xAD && memory.iwram[3] == 0x0B);

    assert(gba_store32_v3(&rt, &exit, UINT32_C(0x03007FFC), UINT32_C(0xCAFEBABE)) ==
           GBA_EXIT_CONTINUE_V2);
    assert(memory.iwram[0x7FFC] == 0xBE && memory.iwram[0x7FFF] == 0xCA);
}

static void expect_atomic_fault(GbaRuntimeV3 *rt, GbaMemoryV1 *memory,
                                GbaExitV3 *exit, uint32_t address,
                                GbaMemoryFaultV3 expected_code) {
    GbaMemoryV1 before_memory;
    GbaRuntimeV2 before_cpu = rt->cpu;
    memcpy(&before_memory, memory, sizeof(before_memory));
    assert(gba_store32_v3(rt, exit, address, UINT32_C(0xDEADBEEF)) == GBA_EXIT_FAULT_V2);
    assert(exit->reason == GBA_EXIT_FAULT_V2);
    assert(exit->next_pc == UINT32_C(0x08000224));
    assert(exit->fault_address == address);
    assert(exit->fault_code == (uint32_t)expected_code);
    assert(memcmp(memory, &before_memory, sizeof(before_memory)) == 0);
    assert(memcmp(&rt->cpu, &before_cpu, sizeof(before_cpu)) == 0);
}

static void test_rejections_do_not_mutate_cpu_or_memory(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaExitV3 exit;
    init(&rt, &memory);

    expect_atomic_fault(&rt, &memory, &exit, UINT32_C(0x02000002),
                        GBA_MEMORY_FAULT_ALIGNMENT_V3);
    expect_atomic_fault(&rt, &memory, &exit, UINT32_C(0x02040000),
                        GBA_MEMORY_FAULT_REGION_V3);
    expect_atomic_fault(&rt, &memory, &exit, UINT32_C(0x03008000),
                        GBA_MEMORY_FAULT_REGION_V3);
    expect_atomic_fault(&rt, &memory, &exit, UINT32_C(0x08000000),
                        GBA_MEMORY_FAULT_REGION_V3);

    rt.memory = NULL;
    assert(gba_store32_v3(&rt, &exit, UINT32_C(0x02000000), 1) == GBA_EXIT_FAULT_V2);
    assert(exit.fault_code == GBA_MEMORY_FAULT_NULL_MEMORY_V3);
    rt.memory = &memory;

    rt.abi_version = GBA_ABI_VERSION_V3 + 1;
    assert(gba_store32_v3(&rt, &exit, UINT32_C(0x02000000), 1) == GBA_EXIT_FAULT_V2);
    assert(exit.fault_code == GBA_MEMORY_FAULT_ABI_V3);
    rt.abi_version = GBA_ABI_VERSION_V3;

    rt.cpu.abi_version = GBA_ABI_VERSION_V2 + 1;
    assert(gba_store32_v3(&rt, &exit, UINT32_C(0x02000000), 1) == GBA_EXIT_FAULT_V2);
    assert(exit.fault_code == GBA_MEMORY_FAULT_ABI_V3);
}

static void test_thumb_push_order_and_stack_pointer_writeback(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaExitV3 exit;
    init(&rt, &memory);
    rt.cpu.r[13] = UINT32_C(0x03000020);
    rt.cpu.r[4] = UINT32_C(0x11223344);
    rt.cpu.r[5] = UINT32_C(0x55667788);
    rt.cpu.r[6] = UINT32_C(0x90ABCDEF);
    rt.cpu.r[7] = UINT32_C(0x0BADF00D);
    rt.cpu.r[14] = UINT32_C(0xCAFEBABE);
    const uint32_t list = UINT32_C(0x1F0); /* r4-r7, lr */
    GbaRuntimeV2 before_cpu = rt.cpu;

    assert(gba_push_thumb_v3(&rt, &exit, list) == GBA_EXIT_CONTINUE_V2);
    assert(rt.cpu.r[13] == UINT32_C(0x0300000C));
    assert(read_le32(&memory.iwram[0x0C]) == UINT32_C(0x11223344));
    assert(read_le32(&memory.iwram[0x10]) == UINT32_C(0x55667788));
    assert(read_le32(&memory.iwram[0x14]) == UINT32_C(0x90ABCDEF));
    assert(read_le32(&memory.iwram[0x18]) == UINT32_C(0x0BADF00D));
    assert(read_le32(&memory.iwram[0x1C]) == UINT32_C(0xCAFEBABE));
    assert(rt.cpu.r[15] == UINT32_C(0x08000224));
    before_cpu.r[13] = UINT32_C(0x0300000C);
    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(exit.reason == GBA_EXIT_CONTINUE_V2);
    assert(exit.next_pc == UINT32_C(0x08000224));
    assert(exit.fault_address == 0 && exit.fault_code == GBA_MEMORY_FAULT_NONE_V3);
}

static void expect_atomic_push_fault(GbaRuntimeV3 *rt, GbaMemoryV1 *memory,
                                     GbaExitV3 *exit, uint32_t list,
                                     uint32_t address,
                                     GbaMemoryFaultV3 expected_code) {
    GbaMemoryV1 before_memory;
    GbaRuntimeV2 before_cpu = rt->cpu;
    memcpy(&before_memory, memory, sizeof(before_memory));
    assert(gba_push_thumb_v3(rt, exit, list) == GBA_EXIT_FAULT_V2);
    assert(exit->reason == GBA_EXIT_FAULT_V2);
    assert(exit->next_pc == UINT32_C(0x08000224));
    assert(exit->fault_address == address);
    assert(exit->fault_code == (uint32_t)expected_code);
    assert(memcmp(memory, &before_memory, sizeof(before_memory)) == 0);
    assert(memcmp(&rt->cpu, &before_cpu, sizeof(before_cpu)) == 0);
}

static void test_thumb_push_rejections_are_atomic(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaExitV3 exit;
    init(&rt, &memory);
    rt.cpu.r[13] = UINT32_C(0x03008008);
    expect_atomic_push_fault(&rt, &memory, &exit, UINT32_C(0x1F0),
                             UINT32_C(0x03007FF4), GBA_MEMORY_FAULT_REGION_V3);

    rt.cpu.r[13] = UINT32_C(0x03000020);
    expect_atomic_push_fault(&rt, &memory, &exit, 0,
                             UINT32_C(0x03000020),
                             GBA_MEMORY_FAULT_REGISTER_LIST_V3);
    expect_atomic_push_fault(&rt, &memory, &exit, UINT32_C(0x200),
                             UINT32_C(0x03000020),
                             GBA_MEMORY_FAULT_REGISTER_LIST_V3);

    rt.cpu.r[13] = UINT32_C(0x03000015);
    expect_atomic_push_fault(&rt, &memory, &exit, UINT32_C(0x10),
                             UINT32_C(0x03000011), GBA_MEMORY_FAULT_ALIGNMENT_V3);

    rt.cpu.r[13] = UINT32_C(0x02000008);
    rt.cpu.r[0] = UINT32_C(0x12345678);
    assert(gba_push_thumb_v3(&rt, &exit, UINT32_C(0x01)) ==
           GBA_EXIT_CONTINUE_V2);
    assert(rt.cpu.r[13] == UINT32_C(0x02000004));
    assert(read_le32(&memory.ewram[4]) == UINT32_C(0x12345678));
}

int main(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaExitV3 exit;
    test_physical_ram_boundaries_and_little_endian();
    test_rejections_do_not_mutate_cpu_or_memory();
    test_thumb_push_order_and_stack_pointer_writeback();
    test_thumb_push_rejections_are_atomic();
    init(&rt, &memory);
    assert(gba_store32_v3(NULL, &exit, 0, 0) == GBA_EXIT_FAULT_V2);
    assert(exit.fault_code == GBA_MEMORY_FAULT_RUNTIME_V3);
    assert(gba_push_thumb_v3(NULL, &exit, UINT32_C(1)) == GBA_EXIT_FAULT_V2);
    assert(exit.fault_code == GBA_MEMORY_FAULT_RUNTIME_V3);
    assert(gba_store32_v3(&rt, NULL, 0, 0) == GBA_EXIT_FAULT_V2);
    assert(gba_push_thumb_v3(&rt, NULL, UINT32_C(1)) == GBA_EXIT_FAULT_V2);
    return 0;
}

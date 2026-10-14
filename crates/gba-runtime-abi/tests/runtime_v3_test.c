#include "gba_abi_v3.h"

#include <assert.h>
#include <stdint.h>
#include <string.h>

static uint32_t read_le32(const uint8_t *source) {
    return (uint32_t)source[0] | ((uint32_t)source[1] << 8) |
           ((uint32_t)source[2] << 16) | ((uint32_t)source[3] << 24);
}

static void write_le32(uint8_t *destination, uint32_t value) {
    destination[0] = (uint8_t)value;
    destination[1] = (uint8_t)(value >> 8);
    destination[2] = (uint8_t)(value >> 16);
    destination[3] = (uint8_t)(value >> 24);
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

static void expect_atomic_store16_fault(GbaRuntimeV3 *rt, GbaMemoryV1 *memory,
                                        GbaExitV3 *exit, uint32_t guest_ea,
                                        GbaMemoryFaultV3 expected_code) {
    GbaMemoryV1 before_memory;
    GbaRuntimeV2 before_cpu = rt->cpu;
    memcpy(&before_memory, memory, sizeof(before_memory));

    assert(gba_store16_v3(rt, exit, guest_ea, UINT32_C(0xDEADBEEF)) ==
           GBA_EXIT_FAULT_V2);
    assert(exit->reason == GBA_EXIT_FAULT_V2);
    assert(exit->next_pc == before_cpu.r[15]);
    assert(exit->fault_address == guest_ea);
    assert(exit->fault_code == (uint32_t)expected_code);
    assert(memcmp(memory, &before_memory, sizeof(before_memory)) == 0);
    assert(memcmp(&rt->cpu, &before_cpu, sizeof(before_cpu)) == 0);
}

static void test_store16_physical_ram_little_endian_and_boundaries(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaRuntimeV2 before_cpu;
    GbaExitV3 exit;
    init(&rt, &memory);
    before_cpu = rt.cpu;

    memset(&memory.ewram[0], 0xA5, 5);
    assert(gba_store16_v3(&rt, &exit, UINT32_C(0x02000002),
                          UINT32_C(0xAABBCCDD)) == GBA_EXIT_CONTINUE_V2);
    assert(memory.ewram[0] == 0xA5 && memory.ewram[1] == 0xA5);
    assert(memory.ewram[2] == 0xDD && memory.ewram[3] == 0xCC);
    assert(memory.ewram[4] == 0xA5);

    memset(&memory.iwram[0], 0x5A, 5);
    assert(gba_store16_v3(&rt, &exit, UINT32_C(0x03000003),
                          UINT32_C(0x12345678)) == GBA_EXIT_CONTINUE_V2);
    assert(memory.iwram[0] == 0x5A && memory.iwram[1] == 0x5A);
    assert(memory.iwram[2] == 0x78 && memory.iwram[3] == 0x56);
    assert(memory.iwram[4] == 0x5A);

    memset(&memory.ewram[0x3FFFD], 0xA5, 3);
    assert(gba_store16_v3(&rt, &exit, UINT32_C(0x0203FFFF),
                          UINT32_C(0xAABBCCDD)) == GBA_EXIT_CONTINUE_V2);
    assert(memory.ewram[0x3FFFD] == 0xA5);
    assert(memory.ewram[0x3FFFE] == 0xDD && memory.ewram[0x3FFFF] == 0xCC);

    memset(&memory.iwram[0x7FFD], 0x5A, 3);
    assert(gba_store16_v3(&rt, &exit, UINT32_C(0x03007FFF),
                          UINT32_C(0x12345678)) == GBA_EXIT_CONTINUE_V2);
    assert(memory.iwram[0x7FFD] == 0x5A);
    assert(memory.iwram[0x7FFE] == 0x78 && memory.iwram[0x7FFF] == 0x56);

    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(exit.reason == GBA_EXIT_CONTINUE_V2);
    assert(exit.next_pc == UINT32_C(0x08000224));
    assert(exit.fault_address == 0);
    assert(exit.fault_code == GBA_MEMORY_FAULT_NONE_V3);
}

static void test_store16_rejections_are_atomic(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaExitV3 exit;
    init(&rt, &memory);

    const uint32_t outside_addresses[] = {
        UINT32_C(0x01FFFFFE), /* just below EWRAM */
        UINT32_C(0x02040000), /* EWRAM mirror boundary */
        UINT32_C(0x02040001), /* odd address in the mirror window */
        UINT32_C(0x02FFFFFF), /* end of the EWRAM mirror window */
        UINT32_C(0x03008000), /* IWRAM mirror boundary */
        UINT32_C(0x04000000), /* MMIO */
        UINT32_C(0x08000000), /* ROM */
        UINT32_MAX,
    };
    for (size_t index = 0;
         index < sizeof(outside_addresses) / sizeof(outside_addresses[0]);
         ++index) {
        expect_atomic_store16_fault(&rt, &memory, &exit, outside_addresses[index],
                                    GBA_MEMORY_FAULT_REGION_V3);
    }

    rt.abi_version = GBA_ABI_VERSION_V3 + 1;
    expect_atomic_store16_fault(&rt, &memory, &exit, UINT32_C(0x02000001),
                                GBA_MEMORY_FAULT_ABI_V3);
    rt.abi_version = GBA_ABI_VERSION_V3;

    rt.cpu.abi_version = GBA_ABI_VERSION_V2 + 1;
    expect_atomic_store16_fault(&rt, &memory, &exit, UINT32_C(0x03000001),
                                GBA_MEMORY_FAULT_ABI_V3);
    rt.cpu.abi_version = GBA_ABI_VERSION_V2;

    rt.memory = NULL;
    expect_atomic_store16_fault(&rt, &memory, &exit, UINT32_C(0x02000001),
                                GBA_MEMORY_FAULT_NULL_MEMORY_V3);
}

static void test_store16_null_runtime_and_output(void) {
    GbaMemoryV1 memory;
    GbaMemoryV1 before_memory;
    GbaRuntimeV3 rt;
    GbaRuntimeV2 before_cpu;
    GbaExitV3 exit;
    memset(&memory, 0xA5, sizeof(memory));
    memcpy(&before_memory, &memory, sizeof(before_memory));
    memset(&exit, 0xFF, sizeof(exit));

    assert(gba_store16_v3(NULL, &exit, UINT32_C(0x02000001),
                          UINT32_C(0xAABBCCDD)) == GBA_EXIT_FAULT_V2);
    assert(exit.reason == GBA_EXIT_FAULT_V2);
    assert(exit.next_pc == 0);
    assert(exit.fault_address == UINT32_C(0x02000001));
    assert(exit.fault_code == GBA_MEMORY_FAULT_RUNTIME_V3);
    assert(memcmp(&memory, &before_memory, sizeof(memory)) == 0);

    init(&rt, &memory);
    before_cpu = rt.cpu;
    memcpy(&before_memory, &memory, sizeof(before_memory));
    assert(gba_store16_v3(&rt, NULL, UINT32_C(0x02000000),
                          UINT32_C(0xAABBCCDD)) == GBA_EXIT_FAULT_V2);
    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(memory)) == 0);
}

static void expect_atomic_store8_fault(GbaRuntimeV3 *rt, GbaMemoryV1 *memory,
                                       GbaExitV3 *exit, uint32_t guest_ea,
                                       GbaMemoryFaultV3 expected_code) {
    GbaMemoryV1 before_memory;
    GbaRuntimeV2 before_cpu = rt->cpu;
    memcpy(&before_memory, memory, sizeof(before_memory));

    assert(gba_store8_v3(rt, exit, guest_ea, UINT32_C(0xDEADBEEF)) ==
           GBA_EXIT_FAULT_V2);
    assert(exit->reason == GBA_EXIT_FAULT_V2);
    assert(exit->next_pc == before_cpu.r[15]);
    assert(exit->fault_address == guest_ea);
    assert(exit->fault_code == (uint32_t)expected_code);
    assert(memcmp(memory, &before_memory, sizeof(before_memory)) == 0);
    assert(memcmp(&rt->cpu, &before_cpu, sizeof(before_cpu)) == 0);
}

static void test_store8_physical_ram_exact_byte_and_boundaries(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaRuntimeV2 before_cpu;
    GbaExitV3 exit;
    init(&rt, &memory);
    before_cpu = rt.cpu;

    memset(&memory.ewram[0], 0xA5, 3);
    assert(gba_store8_v3(&rt, &exit, UINT32_C(0x02000000),
                         UINT32_C(0xAABBCCDD)) == GBA_EXIT_CONTINUE_V2);
    assert(memory.ewram[0] == 0xDD);
    assert(memory.ewram[1] == 0xA5 && memory.ewram[2] == 0xA5);

    memset(&memory.ewram[0x3FFFD], 0x5A, 3);
    assert(gba_store8_v3(&rt, &exit, UINT32_C(0x0203FFFF),
                         UINT32_C(0x12345678)) == GBA_EXIT_CONTINUE_V2);
    assert(memory.ewram[0x3FFFD] == 0x5A && memory.ewram[0x3FFFE] == 0x5A);
    assert(memory.ewram[0x3FFFF] == 0x78);

    memset(&memory.iwram[0], 0xC3, 3);
    assert(gba_store8_v3(&rt, &exit, UINT32_C(0x03000001),
                         UINT32_C(0xFEDCBA98)) == GBA_EXIT_CONTINUE_V2);
    assert(memory.iwram[0] == 0xC3 && memory.iwram[1] == 0x98);
    assert(memory.iwram[2] == 0xC3);

    memset(&memory.iwram[0x7FFD], 0x69, 3);
    assert(gba_store8_v3(&rt, &exit, UINT32_C(0x03007FFF),
                         UINT32_C(0x87654321)) == GBA_EXIT_CONTINUE_V2);
    assert(memory.iwram[0x7FFD] == 0x69 && memory.iwram[0x7FFE] == 0x69);
    assert(memory.iwram[0x7FFF] == 0x21);

    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(exit.reason == GBA_EXIT_CONTINUE_V2);
    assert(exit.next_pc == UINT32_C(0x08000224));
    assert(exit.fault_address == 0);
    assert(exit.fault_code == GBA_MEMORY_FAULT_NONE_V3);
}

static void test_store8_rejections_are_atomic(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaExitV3 exit;
    init(&rt, &memory);

    const uint32_t outside_addresses[] = {
        UINT32_C(0x01FFFFFF), /* just below EWRAM */
        UINT32_C(0x02040000), /* EWRAM mirror boundary */
        UINT32_C(0x02FFFFFF), /* end of the EWRAM mirror window */
        UINT32_C(0x03008000), /* IWRAM mirror boundary */
        UINT32_C(0x04000000), /* MMIO */
        UINT32_C(0x05000000), /* palette RAM */
        UINT32_C(0x06000000), /* VRAM */
        UINT32_C(0x07000000), /* OAM */
        UINT32_C(0x08000000), /* ROM */
        UINT32_MAX,
    };
    for (size_t index = 0;
         index < sizeof(outside_addresses) / sizeof(outside_addresses[0]);
         ++index) {
        expect_atomic_store8_fault(&rt, &memory, &exit, outside_addresses[index],
                                   GBA_MEMORY_FAULT_REGION_V3);
    }

    rt.abi_version = GBA_ABI_VERSION_V3 + 1;
    expect_atomic_store8_fault(&rt, &memory, &exit, UINT32_C(0x02000001),
                               GBA_MEMORY_FAULT_ABI_V3);
    rt.abi_version = GBA_ABI_VERSION_V3;

    rt.cpu.abi_version = GBA_ABI_VERSION_V2 + 1;
    expect_atomic_store8_fault(&rt, &memory, &exit, UINT32_C(0x03000001),
                               GBA_MEMORY_FAULT_ABI_V3);
    rt.cpu.abi_version = GBA_ABI_VERSION_V2;

    rt.memory = NULL;
    expect_atomic_store8_fault(&rt, &memory, &exit, UINT32_C(0x02000001),
                               GBA_MEMORY_FAULT_NULL_MEMORY_V3);
}

static void test_store8_null_runtime_and_output(void) {
    GbaMemoryV1 memory;
    GbaMemoryV1 before_memory;
    GbaRuntimeV3 rt;
    GbaRuntimeV2 before_cpu;
    GbaExitV3 exit;
    GbaExitV3 before_exit;
    memset(&memory, 0xA5, sizeof(memory));
    memcpy(&before_memory, &memory, sizeof(before_memory));
    memset(&exit, 0xFF, sizeof(exit));

    assert(gba_store8_v3(NULL, &exit, UINT32_C(0x02000001),
                         UINT32_C(0xAABBCCDD)) == GBA_EXIT_FAULT_V2);
    assert(exit.reason == GBA_EXIT_FAULT_V2);
    assert(exit.next_pc == 0);
    assert(exit.fault_address == UINT32_C(0x02000001));
    assert(exit.fault_code == GBA_MEMORY_FAULT_RUNTIME_V3);
    assert(memcmp(&memory, &before_memory, sizeof(memory)) == 0);

    init(&rt, &memory);
    before_cpu = rt.cpu;
    memcpy(&before_memory, &memory, sizeof(before_memory));
    assert(gba_store8_v3(&rt, NULL, UINT32_C(0x02000000),
                         UINT32_C(0xAABBCCDD)) == GBA_EXIT_FAULT_V2);
    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(memory)) == 0);

    memset(&exit, 0x5A, sizeof(exit));
    assert(gba_store8_v3(&rt, &exit, UINT32_C(0x02000001), 0xAA) ==
           GBA_EXIT_CONTINUE_V2);
    before_exit = exit;
    assert(gba_store8_v3(&rt, NULL, UINT32_C(0x02000002), 0xBB) ==
           GBA_EXIT_FAULT_V2);
    assert(memcmp(&exit, &before_exit, sizeof(exit)) == 0);
    assert(memory.ewram[1] == 0xAA && memory.ewram[2] == 0);
    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
}

static void expect_atomic_load8_fault(GbaRuntimeV3 *rt, GbaMemoryV1 *memory,
                                      GbaExitV3 *exit, uint32_t guest_ea,
                                      GbaMemoryFaultV3 expected_code) {
    GbaMemoryV1 before_memory;
    GbaRuntimeV2 before_cpu = rt->cpu;
    uint8_t value = UINT8_C(0xA5);
    memcpy(&before_memory, memory, sizeof(before_memory));

    assert(gba_load8_v3(rt, exit, guest_ea, &value) == GBA_EXIT_FAULT_V2);
    assert(exit->reason == GBA_EXIT_FAULT_V2);
    assert(exit->next_pc == before_cpu.r[15]);
    assert(exit->fault_address == guest_ea);
    assert(exit->fault_code == (uint32_t)expected_code);
    assert(value == UINT8_C(0xA5));
    assert(memcmp(memory, &before_memory, sizeof(before_memory)) == 0);
    assert(memcmp(&rt->cpu, &before_cpu, sizeof(before_cpu)) == 0);
}

static void test_load8_physical_ram_exact_byte_and_boundaries(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaMemoryV1 before_memory;
    GbaRuntimeV2 before_cpu;
    GbaExitV3 exit;
    uint8_t value = UINT8_C(0xA5);
    init(&rt, &memory);
    before_cpu = rt.cpu;

    memory.ewram[0] = UINT8_C(0x00);
    memory.ewram[1] = UINT8_C(0x7F);
    memory.ewram[2] = UINT8_C(0x80);
    memory.ewram[0x3FFFF] = UINT8_C(0xFF);
    memory.iwram[0] = UINT8_C(0x7F);
    memory.iwram[1] = UINT8_C(0x80);
    memory.iwram[2] = UINT8_C(0x00);
    memory.iwram[0x7FFF] = UINT8_C(0xFF);
    memcpy(&before_memory, &memory, sizeof(before_memory));

    assert(gba_load8_v3(&rt, &exit, UINT32_C(0x02000000), &value) ==
           GBA_EXIT_CONTINUE_V2);
    assert(value == UINT8_C(0x00));
    assert(gba_load8_v3(&rt, &exit, UINT32_C(0x02000001), &value) ==
           GBA_EXIT_CONTINUE_V2);
    assert(value == UINT8_C(0x7F));
    assert(gba_load8_v3(&rt, &exit, UINT32_C(0x02000002), &value) ==
           GBA_EXIT_CONTINUE_V2);
    assert(value == UINT8_C(0x80));
    assert(gba_load8_v3(&rt, &exit, UINT32_C(0x0203FFFF), &value) ==
           GBA_EXIT_CONTINUE_V2);
    assert(value == UINT8_C(0xFF));

    assert(gba_load8_v3(&rt, &exit, UINT32_C(0x03000000), &value) ==
           GBA_EXIT_CONTINUE_V2);
    assert(value == UINT8_C(0x7F));
    assert(gba_load8_v3(&rt, &exit, UINT32_C(0x03000001), &value) ==
           GBA_EXIT_CONTINUE_V2);
    assert(value == UINT8_C(0x80));
    assert(gba_load8_v3(&rt, &exit, UINT32_C(0x03000002), &value) ==
           GBA_EXIT_CONTINUE_V2);
    assert(value == UINT8_C(0x00));
    assert(gba_load8_v3(&rt, &exit, UINT32_C(0x03007FFF), &value) ==
           GBA_EXIT_CONTINUE_V2);
    assert(value == UINT8_C(0xFF));

    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(before_memory)) == 0);
    assert(exit.reason == GBA_EXIT_CONTINUE_V2);
    assert(exit.next_pc == UINT32_C(0x08000224));
    assert(exit.fault_address == 0);
    assert(exit.fault_code == GBA_MEMORY_FAULT_NONE_V3);
}

static void test_load8_rejections_are_atomic(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaExitV3 exit;
    init(&rt, &memory);

    const uint32_t outside_addresses[] = {
        UINT32_C(0x01FFFFFF), /* just below EWRAM */
        UINT32_C(0x02040000), /* EWRAM mirror boundary */
        UINT32_C(0x02FFFFFF), /* end of the EWRAM mirror window */
        UINT32_C(0x03008000), /* IWRAM mirror boundary */
        UINT32_C(0x04000000), /* MMIO */
        UINT32_C(0x05000000), /* palette RAM */
        UINT32_C(0x06000000), /* VRAM */
        UINT32_C(0x07000000), /* OAM */
        UINT32_C(0x08000000), /* ROM */
        UINT32_MAX,
    };
    for (size_t index = 0;
         index < sizeof(outside_addresses) / sizeof(outside_addresses[0]);
         ++index) {
        expect_atomic_load8_fault(&rt, &memory, &exit, outside_addresses[index],
                                  GBA_MEMORY_FAULT_REGION_V3);
    }

    rt.abi_version = GBA_ABI_VERSION_V3 + 1;
    expect_atomic_load8_fault(&rt, &memory, &exit, UINT32_C(0x02000001),
                              GBA_MEMORY_FAULT_ABI_V3);
    rt.abi_version = GBA_ABI_VERSION_V3;

    rt.cpu.abi_version = GBA_ABI_VERSION_V2 + 1;
    expect_atomic_load8_fault(&rt, &memory, &exit, UINT32_C(0x03000001),
                              GBA_MEMORY_FAULT_ABI_V3);
    rt.cpu.abi_version = GBA_ABI_VERSION_V2;

    rt.memory = NULL;
    expect_atomic_load8_fault(&rt, &memory, &exit, UINT32_C(0x02000001),
                              GBA_MEMORY_FAULT_NULL_MEMORY_V3);
}

static void test_load8_null_runtime_and_outputs(void) {
    GbaMemoryV1 memory;
    GbaMemoryV1 before_memory;
    GbaRuntimeV3 rt;
    GbaRuntimeV2 before_cpu;
    GbaExitV3 exit;
    GbaExitV3 before_exit;
    uint8_t value = UINT8_C(0xA5);
    memset(&memory, 0x5A, sizeof(memory));
    memcpy(&before_memory, &memory, sizeof(before_memory));
    memset(&exit, 0xC3, sizeof(exit));

    assert(gba_load8_v3(NULL, &exit, UINT32_C(0x02000001), &value) ==
           GBA_EXIT_FAULT_V2);
    assert(exit.reason == GBA_EXIT_FAULT_V2);
    assert(exit.next_pc == 0);
    assert(exit.fault_address == UINT32_C(0x02000001));
    assert(exit.fault_code == GBA_MEMORY_FAULT_RUNTIME_V3);
    assert(value == UINT8_C(0xA5));
    assert(memcmp(&memory, &before_memory, sizeof(memory)) == 0);

    init(&rt, &memory);
    before_cpu = rt.cpu;
    memcpy(&before_memory, &memory, sizeof(before_memory));
    memset(&exit, 0x3C, sizeof(exit));
    before_exit = exit;
    assert(gba_load8_v3(&rt, &exit, UINT32_C(0x02000000), NULL) ==
           GBA_EXIT_FAULT_V2);
    assert(memcmp(&exit, &before_exit, sizeof(exit)) == 0);
    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(memory)) == 0);
    assert(value == UINT8_C(0xA5));

    assert(gba_load8_v3(&rt, NULL, UINT32_C(0x02000000), &value) ==
           GBA_EXIT_FAULT_V2);
    assert(value == UINT8_C(0xA5));
    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(memory)) == 0);

    memset(&exit, 0x96, sizeof(exit));
    before_exit = exit;
    value = UINT8_C(0xA5);
    assert(gba_load8_v3(&rt, &exit, UINT32_C(0x02000000), &value) ==
           GBA_EXIT_CONTINUE_V2);
    before_exit = exit;
    const uint8_t before_value = value;
    assert(gba_load8_v3(&rt, NULL, UINT32_C(0x02000001), &value) ==
           GBA_EXIT_FAULT_V2);
    assert(memcmp(&exit, &before_exit, sizeof(exit)) == 0);
    assert(value == before_value);
    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(before_memory)) == 0);

    memset(&exit, 0x69, sizeof(exit));
    before_exit = exit;
    assert(gba_load8_v3(&rt, &exit, UINT32_C(0x02000001), NULL) ==
           GBA_EXIT_FAULT_V2);
    assert(memcmp(&exit, &before_exit, sizeof(exit)) == 0);
    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(before_memory)) == 0);
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

static void test_thumb_pop_order_writeback_and_boundaries(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaMemoryV1 before_memory;
    GbaExitV3 exit;
    init(&rt, &memory);
    for (uint32_t reg = 0; reg < 8; ++reg) {
        rt.cpu.r[reg] = UINT32_C(0xA0000000) + reg;
    }
    rt.cpu.cpsr = UINT32_C(0x6000001F);
    rt.cpu.guest_cycle = UINT64_C(0x123456789);
    rt.cpu.r[13] = UINT32_C(0x02000020);
    rt.cpu.r[15] = UINT32_C(0x08001234);
    write_le32(&memory.ewram[0x20], UINT32_C(0x11223344));
    write_le32(&memory.ewram[0x24], UINT32_C(0x55667788));
    write_le32(&memory.ewram[0x28], UINT32_C(0x90ABCDEF));
    write_le32(&memory.ewram[0x2C], UINT32_C(0x0BADF00D));
    GbaRuntimeV2 expected = rt.cpu;
    memcpy(&before_memory, &memory, sizeof(before_memory));

    /* Sparse list reads stack words in ascending register order. */
    const uint32_t sparse_list = UINT32_C(0xA5); /* r0, r2, r5, r7 */
    assert(gba_pop_thumb_v3(&rt, &exit, sparse_list) == GBA_EXIT_CONTINUE_V2);
    expected.r[0] = UINT32_C(0x11223344);
    expected.r[2] = UINT32_C(0x55667788);
    expected.r[5] = UINT32_C(0x90ABCDEF);
    expected.r[7] = UINT32_C(0x0BADF00D);
    expected.r[13] = UINT32_C(0x02000030);
    assert(memcmp(&rt.cpu, &expected, sizeof(expected)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(before_memory)) == 0);
    assert(exit.reason == GBA_EXIT_CONTINUE_V2);
    assert(exit.next_pc == UINT32_C(0x08001234));
    assert(exit.fault_address == 0);
    assert(exit.fault_code == GBA_MEMORY_FAULT_NONE_V3);

    /* Eight words ending exactly at the EWRAM limit are all valid. */
    init(&rt, &memory);
    rt.cpu.r[13] = UINT32_C(0x0203FFE0);
    for (uint32_t reg = 0; reg < 8; ++reg) {
        write_le32(&memory.ewram[0x3FFE0 + reg * 4],
                   UINT32_C(0x10000000) + reg);
    }
    expected = rt.cpu;
    memcpy(&before_memory, &memory, sizeof(before_memory));
    assert(gba_pop_thumb_v3(&rt, &exit, UINT32_C(0xFF)) ==
           GBA_EXIT_CONTINUE_V2);
    for (uint32_t reg = 0; reg < 8; ++reg) {
        expected.r[reg] = UINT32_C(0x10000000) + reg;
    }
    expected.r[13] = UINT32_C(0x02040000);
    assert(memcmp(&rt.cpu, &expected, sizeof(expected)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(before_memory)) == 0);
    assert(exit.reason == GBA_EXIT_CONTINUE_V2);
    assert(exit.next_pc == UINT32_C(0x08000224));
    assert(exit.fault_address == 0);
    assert(exit.fault_code == GBA_MEMORY_FAULT_NONE_V3);

    /* The last IWRAM word can be popped as a single selected register. */
    init(&rt, &memory);
    rt.cpu.r[13] = UINT32_C(0x03007FFC);
    write_le32(&memory.iwram[0x7FFC], UINT32_C(0xCAFEBABE));
    expected = rt.cpu;
    memcpy(&before_memory, &memory, sizeof(before_memory));
    assert(gba_pop_thumb_v3(&rt, &exit, UINT32_C(0x80)) ==
           GBA_EXIT_CONTINUE_V2);
    expected.r[7] = UINT32_C(0xCAFEBABE);
    expected.r[13] = UINT32_C(0x03008000);
    assert(memcmp(&rt.cpu, &expected, sizeof(expected)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(before_memory)) == 0);
    assert(exit.reason == GBA_EXIT_CONTINUE_V2);
    assert(exit.next_pc == UINT32_C(0x08000224));
    assert(exit.fault_address == 0);
    assert(exit.fault_code == GBA_MEMORY_FAULT_NONE_V3);
}

static void expect_atomic_pop_fault(GbaRuntimeV3 *rt, GbaMemoryV1 *memory,
                                    GbaExitV3 *exit, uint32_t list,
                                    uint32_t address,
                                    GbaMemoryFaultV3 expected_code) {
    GbaMemoryV1 before_memory;
    GbaRuntimeV2 before_cpu = rt->cpu;
    memcpy(&before_memory, memory, sizeof(before_memory));
    assert(gba_pop_thumb_v3(rt, exit, list) == GBA_EXIT_FAULT_V2);
    assert(exit->reason == GBA_EXIT_FAULT_V2);
    assert(exit->next_pc == before_cpu.r[15]);
    assert(exit->fault_address == address);
    assert(exit->fault_code == (uint32_t)expected_code);
    assert(memcmp(memory, &before_memory, sizeof(before_memory)) == 0);
    assert(memcmp(&rt->cpu, &before_cpu, sizeof(before_cpu)) == 0);
}

static void test_thumb_pop_rejections_are_atomic(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaExitV3 exit;
    init(&rt, &memory);
    rt.cpu.r[13] = UINT32_C(0x0203FFFC);
    rt.cpu.r[0] = UINT32_C(0x11111111);
    rt.cpu.r[1] = UINT32_C(0x22222222);
    write_le32(&memory.ewram[0x3FFFC], UINT32_C(0x33333333));
    /* The second word is outside physical EWRAM; the first read is not committed. */
    expect_atomic_pop_fault(&rt, &memory, &exit, UINT32_C(0x03),
                            UINT32_C(0x02040000), GBA_MEMORY_FAULT_REGION_V3);

    rt.cpu.r[13] = UINT32_C(0x03007FFC);
    expect_atomic_pop_fault(&rt, &memory, &exit, UINT32_C(0x03),
                            UINT32_C(0x03008000), GBA_MEMORY_FAULT_REGION_V3);
    rt.cpu.r[13] = UINT32_C(0x01FFFFFC);
    expect_atomic_pop_fault(&rt, &memory, &exit, UINT32_C(0x01),
                            UINT32_C(0x01FFFFFC), GBA_MEMORY_FAULT_REGION_V3);
    rt.cpu.r[13] = UINT32_C(0x02040000);
    expect_atomic_pop_fault(&rt, &memory, &exit, UINT32_C(0x01),
                            UINT32_C(0x02040000), GBA_MEMORY_FAULT_REGION_V3);
    rt.cpu.r[13] = UINT32_C(0x02000020);
    expect_atomic_pop_fault(&rt, &memory, &exit, 0, UINT32_C(0x02000020),
                            GBA_MEMORY_FAULT_REGISTER_LIST_V3);
    expect_atomic_pop_fault(&rt, &memory, &exit, UINT32_C(0x100),
                            UINT32_C(0x02000020),
                            GBA_MEMORY_FAULT_REGISTER_LIST_V3);
    expect_atomic_pop_fault(&rt, &memory, &exit, UINT32_MAX,
                            UINT32_C(0x02000020),
                            GBA_MEMORY_FAULT_REGISTER_LIST_V3);
    rt.cpu.r[13] = UINT32_C(0x02000021);
    expect_atomic_pop_fault(&rt, &memory, &exit, UINT32_C(0x01),
                            UINT32_C(0x02000021), GBA_MEMORY_FAULT_ALIGNMENT_V3);

    rt.cpu.r[13] = UINT32_C(0x02000020);
    rt.abi_version = GBA_ABI_VERSION_V3 + 1;
    expect_atomic_pop_fault(&rt, &memory, &exit, UINT32_C(0x01),
                            UINT32_C(0x02000020), GBA_MEMORY_FAULT_ABI_V3);
    rt.abi_version = GBA_ABI_VERSION_V3;
    rt.cpu.abi_version = GBA_ABI_VERSION_V2 + 1;
    expect_atomic_pop_fault(&rt, &memory, &exit, UINT32_C(0x01),
                            UINT32_C(0x02000020), GBA_MEMORY_FAULT_ABI_V3);
    rt.cpu.abi_version = GBA_ABI_VERSION_V2;
    rt.memory = NULL;
    expect_atomic_pop_fault(&rt, &memory, &exit, UINT32_C(0x01),
                            UINT32_C(0x02000020),
                            GBA_MEMORY_FAULT_NULL_MEMORY_V3);
}

static void test_thumb_pop_null_runtime_and_output(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaMemoryV1 before_memory;
    GbaRuntimeV2 before_cpu;
    GbaExitV3 exit;
    GbaExitV3 before_exit;
    init(&rt, &memory);
    rt.cpu.r[13] = UINT32_C(0x03000020);
    before_cpu = rt.cpu;
    memcpy(&before_memory, &memory, sizeof(before_memory));

    memset(&exit, 0xA5, sizeof(exit));
    assert(gba_pop_thumb_v3(NULL, &exit, UINT32_C(0x01)) ==
           GBA_EXIT_FAULT_V2);
    assert(exit.reason == GBA_EXIT_FAULT_V2);
    assert(exit.next_pc == 0);
    assert(exit.fault_address == 0);
    assert(exit.fault_code == GBA_MEMORY_FAULT_RUNTIME_V3);
    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(before_memory)) == 0);

    memset(&exit, 0x5A, sizeof(exit));
    before_exit = exit;
    assert(gba_pop_thumb_v3(&rt, NULL, UINT32_C(0x01)) == GBA_EXIT_FAULT_V2);
    assert(memcmp(&exit, &before_exit, sizeof(exit)) == 0);
    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(before_memory)) == 0);
}

static void test_thumb_ldmia_order_writeback_and_alignment(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaMemoryV1 before_memory;
    GbaExitV3 exit;
    init(&rt, &memory);
    for (uint32_t reg = 0; reg < 8; ++reg) {
        rt.cpu.r[reg] = UINT32_C(0xA0000000) + reg;
    }
    rt.cpu.r[3] = UINT32_C(0x02000021);
    rt.cpu.r[15] = UINT32_C(0x08001234);
    rt.cpu.cpsr = UINT32_C(0xA000003F);
    rt.cpu.guest_cycle = UINT64_C(0x123456789);
    write_le32(&memory.ewram[0x20], UINT32_C(0x11223344));
    write_le32(&memory.ewram[0x24], UINT32_C(0x55667788));
    write_le32(&memory.ewram[0x28], UINT32_C(0x90ABCDEF));
    write_le32(&memory.ewram[0x2C], UINT32_C(0x0BADF00D));
    GbaRuntimeV2 expected = rt.cpu;
    memcpy(&before_memory, &memory, sizeof(before_memory));

    /* r0, r2, r5 and r7 receive words from aligned ascending addresses. */
    assert(gba_ldmia_thumb_v3(&rt, &exit, 3, UINT32_C(0xA5)) ==
           GBA_EXIT_CONTINUE_V2);
    expected.r[0] = UINT32_C(0x11223344);
    expected.r[2] = UINT32_C(0x55667788);
    expected.r[5] = UINT32_C(0x90ABCDEF);
    expected.r[7] = UINT32_C(0x0BADF00D);
    expected.r[3] = UINT32_C(0x02000031);
    assert(memcmp(&rt.cpu, &expected, sizeof(expected)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(before_memory)) == 0);
    assert(exit.reason == GBA_EXIT_CONTINUE_V2);
    assert(exit.next_pc == UINT32_C(0x08001234));
    assert(exit.fault_address == 0);
    assert(exit.fault_code == GBA_MEMORY_FAULT_NONE_V3);

    /* Base r3 is in the middle of the mask: its loaded word wins. */
    init(&rt, &memory);
    rt.cpu.r[3] = UINT32_C(0x03000011);
    write_le32(&memory.iwram[0x10], UINT32_C(0x11111111));
    write_le32(&memory.iwram[0x14], UINT32_C(0x33333333));
    write_le32(&memory.iwram[0x18], UINT32_C(0x55555555));
    expected = rt.cpu;
    memcpy(&before_memory, &memory, sizeof(before_memory));
    assert(gba_ldmia_thumb_v3(&rt, &exit, 3, UINT32_C(0x2A)) ==
           GBA_EXIT_CONTINUE_V2);
    expected.r[1] = UINT32_C(0x11111111);
    expected.r[3] = UINT32_C(0x33333333);
    expected.r[5] = UINT32_C(0x55555555);
    assert(memcmp(&rt.cpu, &expected, sizeof(expected)) == 0);
    assert(memcmp(&memory, &before_memory, sizeof(before_memory)) == 0);

    /* Eight aligned words ending at the last EWRAM word are valid. */
    init(&rt, &memory);
    rt.cpu.r[0] = UINT32_C(0x0203FFE1);
    for (uint32_t reg = 0; reg < 8; ++reg) {
        write_le32(&memory.ewram[0x3FFE0 + reg * 4], UINT32_C(0x1000) + reg);
    }
    expected = rt.cpu;
    assert(gba_ldmia_thumb_v3(&rt, &exit, 0, UINT32_C(0xFF)) ==
           GBA_EXIT_CONTINUE_V2);
    for (uint32_t reg = 0; reg < 8; ++reg) {
        expected.r[reg] = UINT32_C(0x1000) + reg;
    }
    assert(memcmp(&rt.cpu, &expected, sizeof(expected)) == 0);
}

static void expect_atomic_ldmia_fault(GbaRuntimeV3 *rt, GbaMemoryV1 *memory,
                                      GbaExitV3 *exit, uint32_t base_register,
                                      uint32_t list, uint32_t fault_address,
                                      GbaMemoryFaultV3 expected_code) {
    GbaMemoryV1 before_memory;
    GbaRuntimeV2 before_cpu = rt->cpu;
    memcpy(&before_memory, memory, sizeof(before_memory));
    assert(gba_ldmia_thumb_v3(rt, exit, base_register, list) == GBA_EXIT_FAULT_V2);
    assert(exit->reason == GBA_EXIT_FAULT_V2);
    assert(exit->next_pc == before_cpu.r[15]);
    assert(exit->fault_address == fault_address);
    assert(exit->fault_code == (uint32_t)expected_code);
    assert(memcmp(memory, &before_memory, sizeof(before_memory)) == 0);
    assert(memcmp(&rt->cpu, &before_cpu, sizeof(before_cpu)) == 0);
}

static void test_thumb_ldmia_faults_are_atomic(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaExitV3 exit;
    init(&rt, &memory);
    rt.cpu.r[3] = UINT32_C(0x0203FFFD);
    rt.cpu.r[0] = UINT32_C(0xAAAAAAAA);
    rt.cpu.r[1] = UINT32_C(0xBBBBBBBB);
    write_le32(&memory.ewram[0x3FFFC], UINT32_C(0xCCCCCCCC));
    /* The second EA is invalid; r0 must not be committed from the first. */
    expect_atomic_ldmia_fault(&rt, &memory, &exit, 3, UINT32_C(0x03),
                              UINT32_C(0x02040000), GBA_MEMORY_FAULT_REGION_V3);

    rt.cpu.r[3] = UINT32_C(0x08000000);
    expect_atomic_ldmia_fault(&rt, &memory, &exit, 3, UINT32_C(0x01),
                              UINT32_C(0x08000000), GBA_MEMORY_FAULT_REGION_V3);
    rt.cpu.r[3] = UINT32_C(0x02000020);
    expect_atomic_ldmia_fault(&rt, &memory, &exit, 8, UINT32_C(0x01), 0,
                              GBA_MEMORY_FAULT_REGISTER_LIST_V3);
    expect_atomic_ldmia_fault(&rt, &memory, &exit, 3, 0,
                              UINT32_C(0x02000020), GBA_MEMORY_FAULT_REGISTER_LIST_V3);
    expect_atomic_ldmia_fault(&rt, &memory, &exit, 3, UINT32_C(0x100),
                              UINT32_C(0x02000020), GBA_MEMORY_FAULT_REGISTER_LIST_V3);

    rt.abi_version = GBA_ABI_VERSION_V3 + 1;
    expect_atomic_ldmia_fault(&rt, &memory, &exit, 3, UINT32_C(0x01),
                              UINT32_C(0x02000020), GBA_MEMORY_FAULT_ABI_V3);
    rt.abi_version = GBA_ABI_VERSION_V3;
    rt.cpu.abi_version = GBA_ABI_VERSION_V2 + 1;
    expect_atomic_ldmia_fault(&rt, &memory, &exit, 3, UINT32_C(0x01),
                              UINT32_C(0x02000020), GBA_MEMORY_FAULT_ABI_V3);
    rt.cpu.abi_version = GBA_ABI_VERSION_V2;
    rt.memory = NULL;
    GbaRuntimeV2 before_cpu = rt.cpu;
    assert(gba_ldmia_thumb_v3(&rt, &exit, 3, UINT32_C(0x01)) ==
           GBA_EXIT_FAULT_V2);
    assert(exit.fault_code == GBA_MEMORY_FAULT_NULL_MEMORY_V3);
    assert(exit.fault_address == UINT32_C(0x02000020));
    assert(memcmp(&rt.cpu, &before_cpu, sizeof(before_cpu)) == 0);
}

int main(void) {
    GbaRuntimeV3 rt;
    GbaMemoryV1 memory;
    GbaExitV3 exit;
    test_physical_ram_boundaries_and_little_endian();
    test_rejections_do_not_mutate_cpu_or_memory();
    test_store16_physical_ram_little_endian_and_boundaries();
    test_store16_rejections_are_atomic();
    test_store16_null_runtime_and_output();
    test_store8_physical_ram_exact_byte_and_boundaries();
    test_store8_rejections_are_atomic();
    test_store8_null_runtime_and_output();
    test_load8_physical_ram_exact_byte_and_boundaries();
    test_load8_rejections_are_atomic();
    test_load8_null_runtime_and_outputs();
    test_thumb_push_order_and_stack_pointer_writeback();
    test_thumb_push_rejections_are_atomic();
    test_thumb_pop_order_writeback_and_boundaries();
    test_thumb_pop_rejections_are_atomic();
    test_thumb_pop_null_runtime_and_output();
    test_thumb_ldmia_order_writeback_and_alignment();
    test_thumb_ldmia_faults_are_atomic();
    init(&rt, &memory);
    assert(gba_store32_v3(NULL, &exit, 0, 0) == GBA_EXIT_FAULT_V2);
    assert(exit.fault_code == GBA_MEMORY_FAULT_RUNTIME_V3);
    assert(gba_push_thumb_v3(NULL, &exit, UINT32_C(1)) == GBA_EXIT_FAULT_V2);
    assert(exit.fault_code == GBA_MEMORY_FAULT_RUNTIME_V3);
    assert(gba_ldmia_thumb_v3(NULL, &exit, 3, UINT32_C(1)) == GBA_EXIT_FAULT_V2);
    assert(exit.fault_code == GBA_MEMORY_FAULT_RUNTIME_V3);
    assert(gba_store32_v3(&rt, NULL, 0, 0) == GBA_EXIT_FAULT_V2);
    assert(gba_store16_v3(&rt, NULL, 0, 0) == GBA_EXIT_FAULT_V2);
    assert(gba_store8_v3(&rt, NULL, 0, 0) == GBA_EXIT_FAULT_V2);
    assert(gba_load8_v3(&rt, NULL, 0, NULL) == GBA_EXIT_FAULT_V2);
    assert(gba_push_thumb_v3(&rt, NULL, UINT32_C(1)) == GBA_EXIT_FAULT_V2);
    assert(gba_pop_thumb_v3(&rt, NULL, UINT32_C(1)) == GBA_EXIT_FAULT_V2);
    assert(gba_ldmia_thumb_v3(&rt, NULL, 3, UINT32_C(1)) == GBA_EXIT_FAULT_V2);
    return 0;
}

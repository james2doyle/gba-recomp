#include "gba_abi_v4.h"

#include <assert.h>
#include <string.h>

static GbaRuntimeV4 runtime;
static GbaMachineStateV1 machine;
static GbaExitV3 exit_state;

static uint16_t load_io16(uint32_t offset) {
    return (uint16_t)machine.io[offset] |
           ((uint16_t)machine.io[offset + 1] << 8);
}

static void store_io16(uint32_t offset, uint16_t value) {
    machine.io[offset] = (uint8_t)value;
    machine.io[offset + 1] = (uint8_t)(value >> 8);
}

static void reset_fixture(uint32_t flags) {
    memset(&runtime, 0, sizeof(runtime));
    memset(&machine, 0xA5, sizeof(machine));
    memset(&exit_state, 0, sizeof(exit_state));
    runtime.base.cpu.abi_version = GBA_ABI_VERSION_V2;
    runtime.base.cpu.cpsr = UINT32_C(0xA000003F);
    runtime.base.cpu.guest_cycle = UINT64_C(0x123456789ABCDEF0);
    for (uint32_t i = 0; i < 16; ++i) {
        runtime.base.cpu.r[i] = UINT32_C(0x10000000) + i;
    }
    runtime.base.cpu.r[0] = flags;
    runtime.base.cpu.r[15] = UINT32_C(0x08001234);
    for (uint32_t i = 0; i < 5; ++i) {
        runtime.base.cpu.common_r8_r12[i] = UINT32_C(0x20000000) + i;
        runtime.base.cpu.fiq_r8_r12[i] = UINT32_C(0x30000000) + i;
        runtime.base.cpu.spsr[i] = UINT32_C(0x40000000) + i;
        runtime.base.cpu.banked_r13_r14[i][0] = UINT32_C(0x50000000) + i;
        runtime.base.cpu.banked_r13_r14[i][1] = UINT32_C(0x60000000) + i;
    }
    runtime.base.memory = &machine.memory;
    runtime.base.abi_version = GBA_ABI_VERSION_V3;
    runtime.machine = &machine;
    runtime.abi_version = GBA_ABI_VERSION_V4;
    machine.abi_version = GBA_MACHINE_STATE_VERSION_V1;
}

static void assert_cpu_unchanged(GbaRuntimeV2 before) {
    assert(memcmp(&before, &runtime.base.cpu, sizeof(before)) == 0);
}

static void invoke_reset(void) {
    GbaRuntimeV2 before = runtime.base.cpu;
    assert(gba_register_ram_reset_v1(&runtime, &exit_state, 1) ==
           GBA_EXIT_CONTINUE_V2);
    assert(exit_state.reason == GBA_EXIT_CONTINUE_V2);
    assert(exit_state.next_pc == UINT32_C(0x08001234));
    assert(exit_state.fault_address == 0);
    assert(exit_state.fault_code == GBA_HLE_FAULT_NONE_V1);
    assert_cpu_unchanged(before);
    assert(load_io16(0x000) == UINT16_C(0x0080));
}

static void test_ewram_flag(void) {
    reset_fixture(0x01);
    invoke_reset();
    for (size_t i = 0; i < sizeof(machine.memory.ewram); ++i) {
        assert(machine.memory.ewram[i] == 0);
    }
    assert(machine.memory.iwram[0] == 0xA5);
    assert(machine.palette_ram[0] == 0xA5);
}

static void test_iwram_flag_preserves_last_0x200_bytes(void) {
    reset_fixture(0x02);
    invoke_reset();
    for (size_t i = 0; i < sizeof(machine.memory.iwram) - 0x200; ++i) {
        assert(machine.memory.iwram[i] == 0);
    }
    for (size_t i = sizeof(machine.memory.iwram) - 0x200;
         i < sizeof(machine.memory.iwram); ++i) {
        assert(machine.memory.iwram[i] == 0xA5);
    }
    assert(machine.memory.ewram[0] == 0xA5);
}

static void test_palette_vram_and_oam_flags(void) {
    reset_fixture(0x04);
    invoke_reset();
    for (size_t i = 0; i < sizeof(machine.palette_ram); ++i) {
        assert(machine.palette_ram[i] == 0);
    }
    assert(machine.vram[0] == 0xA5);

    reset_fixture(0x08);
    invoke_reset();
    for (size_t i = 0; i < sizeof(machine.vram); ++i) {
        assert(machine.vram[i] == 0);
    }
    assert(machine.oam[0] == 0xA5);

    reset_fixture(0x10);
    invoke_reset();
    for (size_t i = 0; i < sizeof(machine.oam); ++i) {
        assert(machine.oam[i] == 0);
    }
    assert(machine.palette_ram[0] == 0xA5);
}

static void test_serial_flag_resets_all_documented_latches(void) {
    reset_fixture(0x20);
    invoke_reset();
    assert(load_io16(0x128) == 0);
    assert(load_io16(0x12A) == 0);
    assert(load_io16(0x134) == UINT16_C(0x8000));
    assert(load_io16(0x140) == 0);
    assert(load_io16(0x150) == 0);
    assert(load_io16(0x152) == 0);
    assert(load_io16(0x154) == 0);
    assert(load_io16(0x156) == 0);
    assert(load_io16(0x130) == UINT16_C(0xA5A5));
}

static void test_sound_flag_resets_registers_bias_and_wave_ram(void) {
    reset_fixture(0x40);
    invoke_reset();
    static const uint16_t sound_registers[] = {
        0x060, 0x062, 0x064, 0x068, 0x06C, 0x070, 0x072,
        0x074, 0x078, 0x07C, 0x080, 0x082, 0x084,
    };
    for (size_t i = 0; i < sizeof(sound_registers) / sizeof(sound_registers[0]); ++i) {
        const uint16_t expected = sound_registers[i] == 0x084 ? 0x0005 : 0;
        assert(load_io16(sound_registers[i]) == expected);
    }
    assert(load_io16(0x088) == UINT16_C(0x0200));
    for (size_t i = 0; i < sizeof(machine.wave_ram); ++i) {
        assert(machine.wave_ram[i] == 0);
    }
    assert(load_io16(0x08A) == UINT16_C(0xA5A5));
}

static void test_video_dma_timer_and_interrupt_flag(void) {
    reset_fixture(0x80);
    store_io16(0x004, UINT16_C(0xFFFF));
    store_io16(0x006, UINT16_C(0x002A));
    store_io16(0x200, UINT16_C(0xFFFF));
    store_io16(0x202, UINT16_C(0x0005));
    store_io16(0x204, UINT16_C(0xFFFF));
    store_io16(0x208, UINT16_C(0xFFFF));
    invoke_reset();
    assert(load_io16(0x004) == UINT16_C(0x0003));
    assert(load_io16(0x006) == UINT16_C(0x002A));
    assert(load_io16(0x020) == UINT16_C(0x0100));
    assert(load_io16(0x026) == UINT16_C(0x0100));
    assert(load_io16(0x030) == UINT16_C(0x0100));
    assert(load_io16(0x036) == UINT16_C(0x0100));
    for (uint32_t offset = 0x008; offset <= 0x054; offset += 2) {
        if (offset != 0x020 && offset != 0x026 &&
            offset != 0x030 && offset != 0x036) {
            assert(load_io16(offset) == 0);
        }
    }
    for (uint32_t offset = 0x056; offset < 0x060; offset += 2) {
        assert(load_io16(offset) == UINT16_C(0xA5A5));
    }
    for (uint32_t offset = 0x0B0; offset <= 0x0DE; offset += 2) {
        assert(load_io16(offset) == 0);
    }
    for (uint32_t offset = 0x100; offset <= 0x10E; offset += 2) {
        assert(load_io16(offset) == 0);
    }
    assert(load_io16(0x200) == 0);
    assert(load_io16(0x202) == 0);
    assert(load_io16(0x204) == 0);
    assert(load_io16(0x208) == 0);
}

static void test_vcounter_status_is_recomputed_from_new_compare(void) {
    reset_fixture(0x80);
    store_io16(0x004, UINT16_C(0xFFFF));
    store_io16(0x006, 0);
    invoke_reset();
    assert(load_io16(0x004) == UINT16_C(0x0007));
    assert(load_io16(0x006) == 0);
}

static void test_combined_mask_ignores_high_bits_and_preserves_unselected_ewram(void) {
    reset_fixture(UINT32_C(0xFFFFFFFE));
    invoke_reset();
    assert(machine.memory.ewram[0] == 0xA5);
    assert(machine.memory.iwram[0] == 0);
    assert(machine.palette_ram[0] == 0);
    assert(machine.vram[0] == 0);
    assert(machine.oam[0] == 0);
    assert(load_io16(0x134) == UINT16_C(0x8000));
    assert(load_io16(0x084) == UINT16_C(0x0005));
    assert(load_io16(0x088) == UINT16_C(0x0200));
    assert(machine.wave_ram[0] == 0);
    assert(load_io16(0x202) == 0);
}

static void assert_fault_is_atomic(uint32_t expected_fault) {
    GbaRuntimeV2 before = runtime.base.cpu;
    const uint8_t ewram_byte = machine.memory.ewram[0];
    const uint8_t iwram_byte = machine.memory.iwram[0];
    const uint8_t palette_byte = machine.palette_ram[0];
    const uint8_t io_byte = machine.io[0];
    assert(gba_register_ram_reset_v1(&runtime, &exit_state, 1) ==
           GBA_EXIT_FAULT_V2);
    assert(exit_state.reason == GBA_EXIT_FAULT_V2);
    assert(exit_state.fault_code == expected_fault);
    assert_cpu_unchanged(before);
    assert(machine.memory.ewram[0] == ewram_byte);
    assert(machine.memory.iwram[0] == iwram_byte);
    assert(machine.palette_ram[0] == palette_byte);
    assert(machine.io[0] == io_byte);
}

static void test_invalid_abi_context_and_swi_fail_without_mutation(void) {
    reset_fixture(0xFF);
    runtime.abi_version++;
    assert_fault_is_atomic(GBA_HLE_FAULT_RUNTIME_ABI_V1);

    reset_fixture(0xFF);
    runtime.machine = NULL;
    assert_fault_is_atomic(GBA_HLE_FAULT_MACHINE_V1);

    reset_fixture(0xFF);
    machine.abi_version++;
    assert_fault_is_atomic(GBA_HLE_FAULT_MACHINE_ABI_V1);

    reset_fixture(0xFF);
    runtime.base.memory = NULL;
    assert_fault_is_atomic(GBA_HLE_FAULT_MEMORY_MISMATCH_V1);

    reset_fixture(0xFF);
    GbaRuntimeV2 before = runtime.base.cpu;
    assert(gba_register_ram_reset_v1(&runtime, &exit_state, 2) ==
           GBA_EXIT_FAULT_V2);
    assert(exit_state.fault_code == GBA_HLE_FAULT_UNSUPPORTED_SWI_V1);
    assert_cpu_unchanged(before);
    assert(machine.memory.ewram[0] == 0xA5);
    assert(machine.io[0] == 0xA5);
}

int main(void) {
    reset_fixture(0);
    invoke_reset();
    assert(load_io16(0) == UINT16_C(0x0080));
    test_ewram_flag();
    test_iwram_flag_preserves_last_0x200_bytes();
    test_palette_vram_and_oam_flags();
    test_serial_flag_resets_all_documented_latches();
    test_sound_flag_resets_registers_bias_and_wave_ram();
    test_video_dma_timer_and_interrupt_flag();
    test_vcounter_status_is_recomputed_from_new_compare();
    test_combined_mask_ignores_high_bits_and_preserves_unselected_ewram();
    test_invalid_abi_context_and_swi_fail_without_mutation();
    return 0;
}

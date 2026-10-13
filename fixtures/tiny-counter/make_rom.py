#!/usr/bin/env python3
"""Generate the project's small, deterministic GBA homebrew ROM fixture."""

import argparse
import struct
from pathlib import Path


def branch(pc_offset: int, target_offset: int, condition: int = 0xE) -> int:
    displacement = target_offset - (pc_offset + 8)
    if displacement % 4:
        raise ValueError("ARM branch target must be word aligned")
    immediate = (displacement // 4) & 0x00FFFFFF
    return (condition << 28) | 0x0A000000 | immediate


def header_checksum(image: bytearray) -> int:
    return (-sum(image[0xA0:0xBD]) - 0x19) & 0xFF


def generate(mode: str = "valid") -> bytes:
    image = bytearray(256)
    entry = branch(0, 0xC0 if mode != "out-of-range" else 0x1C0)
    image[0:4] = struct.pack("<I", entry)
    image[0xA0:0xAC] = b"TINY COUNTER"
    image[0xAC:0xB0] = b"TCGE"
    image[0xB0:0xB2] = b"00"
    image[0xB2] = 0x96
    image[0xBC] = 1
    if mode == "msr-mode":
        image[0xC0:0xCC] = struct.pack(
            "<3I",
            0xE3A00013,  # MOV r0, #Supervisor mode
            0xE129F000,  # MSR CPSR_fc, r0 (x/s fields clear)
            branch(0xC8, 0xC8),  # B finished
        )
    elif mode in ("thumb-swi-reset", "thumb-swi-unsupported"):
        swi = 0xDF02 if mode == "thumb-swi-unsupported" else 0xDF01
        image[0x84:0x8E] = struct.pack(
            "<5H",
            0xB401,  # PUSH {r0}: place a nonzero word at IWRAM[0].
            0x2002,  # MOVS r0,#2: request the IWRAM reset area.
            swi,
            0x21A5,  # MOVS r1,#0xA5 after the SWI.
            0x4718,  # BX r3 to the ARM self-loop.
        )
        image[0xC0:0xD8] = struct.pack(
            "<6I",
            0xE3A00042,  # MOV r0,#0x42
            0xE59FD014,  # LDR sp,[pc,#0x14] -> IWRAM stack at 0xE0
            0xE59F2008,  # LDR r2,[pc,#8] -> Thumb entry | 1 at 0xD8
            0xE59F3008,  # LDR r3,[pc,#8] -> ARM return at 0xDC
            0xE12FFF12,  # BX r2
            0xEAFFFFFE,  # B self (unreachable after BX).
        )
        image[0xD8:0xE4] = struct.pack(
            "<3I", 0x08000085, 0x080000E4, 0x03000004
        )
        image[0xE4:0xE8] = struct.pack("<I", 0xEAFFFFFE)
    elif mode == "thumb-push":
        image[0x84:0x88] = struct.pack("<2H", 0xB5F0, 0x4710)  # PUSH {r4-r7,lr}; BX r2.
        image[0xC0:0xF0] = struct.pack(
            "<12I",
            0xE3A04044,  # MOV r4, #0x44
            0xE3A05055,  # MOV r5, #0x55
            0xE3A06066,  # MOV r6, #0x66
            0xE3A07077,  # MOV r7, #0x77
            0xE3A0E088,  # MOV lr, #0x88
            0xE59F0014,  # LDR r0, [pc, #0x14] -> IWRAM stack top
            0xE1A0D000,  # MOV sp, r0
            0xE59F1010,  # LDR r1, [pc, #0x10] -> Thumb entry | 1
            0xE59F2010,  # LDR r2, [pc, #0x10] -> ARM return
            0xE12FFF11,  # BX r1
            0xE1A0000D,  # MOV r0, sp, to observe push writeback
            0xEAFFFFFE,  # B finished
        )
        image[0xF0:0xFC] = struct.pack(
            "<3I", 0x03000014, 0x08000085, 0x080000E8
        )
    elif mode == "thumb-mov":
        image[0x84:0x8A] = struct.pack(
            "<3H", 0x4647, 0x467B, 0x4710
        )  # MOV r7,r8; MOV r3,pc; BX r2.
        image[0xC0:0xDC] = struct.pack(
            "<7I",
            0xE3A08077,  # MOV r8, #0x77
            0xE59F1014,  # LDR r1, [pc, #0x14] -> Thumb entry | 1
            0xE59F2014,  # LDR r2, [pc, #0x14] -> ARM return
            0xE12FFF11,  # BX r1
            0xE1A00007,  # MOV r0, r7, to observe Thumb MOV
            0xE1A01003,  # MOV r1, r3, to observe Thumb PC+4
            0xEAFFFFFE,  # B finished
        )
        image[0xE0:0xE8] = struct.pack("<2I", 0x08000085, 0x080000D0)
    elif mode in ("thumb-movs-immediate", "thumb-movs-zero"):
        immediate = 0 if mode == "thumb-movs-zero" else 0xFF
        image[0x84:0x88] = struct.pack(
            "<2H", 0x2100 | immediate, 0x4718
        )  # MOVS r1,#imm8; BX r3.
        image[0xC0:0xDC] = struct.pack(
            "<7I",
            0xE59F001C,  # LDR r0, [pc, #0x1C] -> 0x80000000
            0xE3500001,  # CMP r0, #1 -> C=1, V=1
            0xE59F2020,  # LDR r2, [pc, #0x20] -> Thumb entry | 1
            0xE59F3020,  # LDR r3, [pc, #0x20] -> ARM return
            0xE12FFF12,  # BX r2
            0xE1A00001,  # MOV r0, r1 to observe the immediate
            0xEAFFFFFE,  # B finished
        )
        image[0xE4:0xE8] = struct.pack("<I", 0x80000000)
        image[0xF0:0xF8] = struct.pack("<2I", 0x08000085, 0x080000D4)
    elif mode == "thumb-lsl-bx":
        image[0x84:0x88] = struct.pack(
            "<2H", 0x0041, 0x4708
        )  # LSLS r1,r0,#1; BX r1.
        image[0xC0:0xCC] = struct.pack(
            "<3I",
            0xE59F0008,  # LDR r0, [pc,#8] -> 0x04000074 at 0xD0.
            0xE59F2008,  # LDR r2, [pc,#8] -> Thumb entry | 1 at 0xD4.
            0xE12FFF12,  # BX r2.
        )
        image[0xD0:0xD8] = struct.pack("<2I", 0x04000074, 0x08000085)
        image[0xE8:0xEC] = struct.pack("<I", 0xEAFFFFFE)  # B self.
    elif mode in (
        "thumb-lsl-zero-carry-clear",
        "thumb-lsl-zero-carry-set",
        "thumb-lsl-one-carry",
        "thumb-lsl-nineteen",
        "thumb-lsl-thirtyone",
        "thumb-lsl-nonalias",
        "thumb-lsr-unsupported",
        "thumb-asr-unsupported",
        "thumb-lsl-register-unsupported",
    ):
        cases = {
            "thumb-lsl-zero-carry-clear": (0x0009, 0x80000000, 0x00000000),
            "thumb-lsl-zero-carry-set": (0x0009, 0x00000000, 0x80000000),
            "thumb-lsl-one-carry": (0x0049, 0x80000001, 0x00000000),
            "thumb-lsl-nineteen": (0x04C9, 0x00001000, 0x80000000),
            "thumb-lsl-thirtyone": (0x07C9, 0x00000002, 0x00000000),
            "thumb-lsl-nonalias": (0x04C8, 0x00001000, 0x80000000),
            "thumb-lsr-unsupported": (0x0849, 0x00000001, 0x00000000),
            "thumb-asr-unsupported": (0x1049, 0x00000001, 0x00000000),
            "thumb-lsl-register-unsupported": (0x4089, 0x00000001, 0x00000000),
        }
        shift, source, compare = cases[mode]
        image[0x84:0x88] = struct.pack(
            "<2H", shift, 0x4718
        )  # Thumb shift; BX r3.
        image[0xC0:0xD8] = struct.pack(
            "<6I",
            0xE59F1018,  # LDR r1, [pc,#0x18] -> source at 0xE0.
            0xE59F0018,  # LDR r0, [pc,#0x18] -> compare lhs at 0xE4.
            0xE3500001,  # CMP r0,#1 prepares C/V for the shift.
            0xE59F2004,  # LDR r2, [pc,#4] -> Thumb entry | 1 at 0xD8.
            0xE59F3004,  # LDR r3, [pc,#4] -> ARM loop at 0xE8.
            0xE12FFF12,  # BX r2.
        )
        image[0xD8:0xE8] = struct.pack(
            "<4I", 0x08000085, 0x080000E8, source, compare
        )
        image[0xE8:0xEC] = struct.pack("<I", 0xEAFFFFFE)  # B self.
    elif mode in (
        "thumb-addsub-add-register",
        "thumb-addsub-sub-register",
        "thumb-addsub-add-imm-zero",
        "thumb-addsub-add-imm-seven",
        "thumb-addsub-sub-imm-zero",
        "thumb-addsub-sub-imm-seven",
        "thumb-addsub-add-imm-carry",
        "thumb-addsub-sub-imm-overflow",
        "thumb-addsub-out-of-family",
    ):
        cases = {
            "thumb-addsub-add-register": (0x1808, 0x7FFFFFFF, 1),
            "thumb-addsub-sub-register": (0x1A09, 0, 1),
            "thumb-addsub-add-imm-zero": (0x1C09, 0x80000000, 0x80000000),
            "thumb-addsub-add-imm-seven": (0x1DC8, 0xFFFFFFF8, 0x80000000),
            "thumb-addsub-sub-imm-zero": (0x1E09, 0x80000000, 0x80000000),
            "thumb-addsub-sub-imm-seven": (0x1FC8, 6, 0x80000000),
            "thumb-addsub-add-imm-carry": (0x1C48, 0xFFFFFFFF, 0x80000000),
            "thumb-addsub-sub-imm-overflow": (0x1E48, 0x80000000, 0x80000000),
            "thumb-addsub-out-of-family": (0x4008, 0x12345678, 0x80000000),
        }
        encoding, source, compare = cases[mode]
        image[0x84:0x88] = struct.pack(
            "<2H", encoding, 0x4718
        )  # ADD/SUB Format 2; BX r3.
        image[0xC0:0xD8] = struct.pack(
            "<6I",
            0xE59F1018,  # LDR r1,[pc,#0x18] -> source at 0xE0.
            0xE59F0018,  # LDR r0,[pc,#0x18] -> compare seed at 0xE4.
            0xE3500001,  # CMP r0,#1 prepares initial C/V.
            0xE59F2004,  # LDR r2,[pc,#4] -> Thumb entry | 1 at 0xD8.
            0xE59F3004,  # LDR r3,[pc,#4] -> ARM loop at 0xE8.
            0xE12FFF12,  # BX r2.
        )
        image[0xD8:0xE8] = struct.pack(
            "<4I", 0x08000085, 0x080000E8, source, compare
        )
        image[0xE8:0xEC] = struct.pack("<I", 0xEAFFFFFE)  # B self.
    elif mode == "thumb-addsub-literal-bx":
        image[0x84:0x8A] = struct.pack(
            "<3H", 0x4A14, 0x1C10, 0x4700
        )  # LDR r2 literal; ADDS r0,r2,#0; BX r0.
        image[0xC0:0xCC] = struct.pack(
            "<3I",
            0xE59F2008,  # LDR r2,[pc,#8] -> Thumb entry | 1 at 0xD0.
            0xE12FFF12,  # BX r2.
            0xEAFFFFFE,  # ARM target: B self.
        )
        image[0xD0:0xD4] = struct.pack("<I", 0x08000085)
        image[0xD8:0xDC] = struct.pack("<I", 0x080000C8)
    elif mode in ("thumb-ldr-literal-bx", "thumb-ldr-literal-out-of-rom"):
        literal_load = 0x4916 if mode == "thumb-ldr-literal-bx" else 0x49FF
        image[0x86:0x8A] = struct.pack(
            "<2H", literal_load, 0x4708
        )  # LDR r1,[PC,#imm]; BX r1.
        image[0xC0:0xC8] = struct.pack(
            "<2I",
            0xE59F2008,  # LDR r2,[pc,#8] -> Thumb entry | 1 at 0xD0.
            0xE12FFF12,  # BX r2.
        )
        image[0xD0:0xD8] = struct.pack(
            "<2I", 0x08000087, 0xEAFFFFFE
        )  # Thumb entry pointer; ARM B self at 0xD4.
        if mode == "thumb-ldr-literal-bx":
            image[0xE0:0xE4] = struct.pack("<I", 0x080000D4)
    elif mode in (
        "thumb-bl",
        "thumb-bl-broken-pair",
        "thumb-bl-suffix-without-prefix",
        "thumb-bl-out-of-range",
    ):
        if mode == "thumb-bl":
            image[0x84:0x94] = struct.pack(
                "<8H",
                0xF000,  # BL prefix at 0x08000084; partial LR = 0x08000088.
                0xF804,  # BL suffix at 0x08000086; target = 0x08000090.
                0x4610,  # MOV r0,r2 after return.
                0x4708,  # BX r1 -> ARM return block.
                0x46C0,  # Unreachable padding: MOV r8,r8.
                0x46C0,
                0x2242,  # Callee: MOVS r2,#0x42.
                0x4770,  # BX LR -> 0x08000088.
            )
            image[0xC0:0xCC] = struct.pack(
                "<3I",
                0xE59F0010,  # LDR r0, [pc,#0x10] -> Thumb entry | 1 at 0xD8.
                0xE59F1010,  # LDR r1, [pc,#0x10] -> ARM return at 0xDC.
                0xE12FFF10,  # BX r0.
            )
            image[0xD0:0xD4] = struct.pack("<I", 0xEAFFFFFE)
            image[0xD8:0xE0] = struct.pack("<2I", 0x08000085, 0x080000D0)
        elif mode == "thumb-bl-broken-pair":
            image[0x84:0x88] = struct.pack("<2H", 0xF000, 0x2001)
            image[0xC0:0xC8] = struct.pack("<2I", 0xE59F0008, 0xE12FFF10)
            image[0xD0:0xD4] = struct.pack("<I", 0x08000085)
        elif mode == "thumb-bl-suffix-without-prefix":
            image[0x84:0x86] = struct.pack("<H", 0xF800)
            image[0xC0:0xC8] = struct.pack("<2I", 0xE59F0008, 0xE12FFF10)
            image[0xD0:0xD4] = struct.pack("<I", 0x08000085)
        else:
            image[0x84:0x88] = struct.pack("<2H", 0xF000, 0xF87F)
            image[0xC0:0xC8] = struct.pack("<2I", 0xE59F0008, 0xE12FFF10)
            image[0xD0:0xD4] = struct.pack("<I", 0x08000085)
    elif mode == "bx-interwork":
        image[0x84:0x86] = struct.pack("<H", 0x4708)  # Thumb BX r1 -> ARM code.
        image[0xC0:0xD0] = struct.pack(
            "<4I",
            0xE59F0008,  # LDR r0, [pc, #8] -> Thumb target at 0x08000084 | 1
            0xE59F1008,  # LDR r1, [pc, #8] -> misaligned ARM target at 0x080000CE
            0xE12FFF10,  # BX r0 -> Thumb
            branch(0xCC, 0xCC),  # B finished
        )
        image[0xD0:0xD8] = struct.pack("<2I", 0x08000085, 0x080000CE)
    elif mode == "bx-out-of-range":
        image[0xC0:0xCC] = struct.pack(
            "<3I",
            0xE59F0008,  # LDR r0, [pc, #8] -> address beyond the source ROM
            branch(0xC4, 0xC8),
            0xE12FFF10,  # BX r0
        )
        image[0xD0:0xD4] = struct.pack("<I", 0x09000001)
    elif mode == "bx-condition-false":
        image[0x84:0x86] = struct.pack("<H", 0x4708)  # Statically reachable Thumb BX r1.
        image[0xC0:0xD0] = struct.pack(
            "<4I",
            0xE59F0010,  # LDR r0, [pc, #16] -> Thumb address | 1
            0xE59F1010,  # LDR r1, [pc, #16] -> misaligned ARM target
            0x012FFF10,  # BXEQ r0; false with initial Z=0
            0xEAFFFFFE,  # B finished
        )
        image[0xD8:0xE0] = struct.pack("<2I", 0x08000085, 0x080000CE)
    elif mode == "literal-load":
        image[0xC0:0xC8] = struct.pack(
            "<2I",
            0xE59F0004,  # LDR r0, [pc, #4] -> ROM word at 0x080000CC
            branch(0xC4, 0xC4),  # B finished
        )
        image[0xCC:0xD0] = struct.pack("<I", 0x12345678)
    elif mode == "literal-condition-false":
        image[0xC0:0xCC] = struct.pack(
            "<3I",
            0xE3A000A5,  # MOV r0, #0xA5
            0x059F0004,  # LDREQ r0, [pc, #4], false with initial Z=0
            branch(0xC8, 0xC8),  # B finished
        )
        image[0xD0:0xD4] = struct.pack("<I", 0x12345678)
    elif mode == "literal-out-of-range":
        image[0xC0:0xC4] = struct.pack("<I", 0xE59F0138)  # Address is beyond this image.
    elif mode == "add-pc":
        image[0xC0:0xC8] = struct.pack(
            "<2I",
            0xE28F0020,  # ADD r0, pc, #0x20
            branch(0xC4, 0xC4),  # B finished
        )
    elif mode == "add-pc-condition-false":
        image[0xC0:0xCC] = struct.pack(
            "<3I",
            0xE3A000A5,  # MOV r0, #0xA5
            0x028F0020,  # ADDEQ r0, pc, #0x20, false with initial Z=0
            branch(0xC8, 0xC8),  # B finished
        )
    elif mode == "mov-register-pc":
        image[0xC0:0xCC] = struct.pack(
            "<3I",
            0xE1A0E00F,  # MOV lr, pc -> visible PC is 0x080000C8
            0xE1A0000E,  # MOV r0, lr
            branch(0xC8, 0xC8),  # B finished
        )
    elif mode == "mov-register-condition-false":
        image[0xC0:0xCC] = struct.pack(
            "<3I",
            0xE3A000A5,  # MOV r0, #0xA5
            0x01A0000F,  # MOVEQ r0, pc; false with initial Z=0
            branch(0xC8, 0xC8),  # B finished
        )
    elif mode == "mov-register-shift":
        image[0xC0:0xC4] = struct.pack("<I", 0xE1A00081)  # MOV r0, r1, LSL #1
    elif mode in (
        "thumb-strh-ewram-zero-even",
        "thumb-strh-ewram-thirtyone-odd-alias",
        "thumb-strh-iwram-zero-odd",
        "thumb-strh-iwram-thirtyone-even",
        "thumb-strh-invalid-odd",
        "thumb-ldrh-unsupported",
        "thumb-strh-register-unsupported",
    ):
        cases = {
            # Format 10: 1000 0 imm5 Rb Rd. The loop keeps the final PC
            # stable so the runner can inspect state immediately after STRH.
            "thumb-strh-ewram-zero-even": (0x8008, 0x0000ABCD, 0x02000000),
            "thumb-strh-ewram-thirtyone-odd-alias": (
                0x87C0,
                0x01FFFFC3,
                0,
            ),
            "thumb-strh-iwram-zero-odd": (0x8008, 0x0000BEEF, 0x03000001),
            "thumb-strh-iwram-thirtyone-even": (
                0x87C8,
                0x00001357,
                0x02FFFFC2,
            ),
            "thumb-strh-invalid-odd": (0x8008, 0x0000A55A, 0x04000001),
            "thumb-ldrh-unsupported": (0x8808, 0x0000ABCD, 0x02000000),
            # STRH r0,[r1,r2], Format 7 register-offset, which stays out of
            # the immediate-only Format 10 contract.
            "thumb-strh-register-unsupported": (0x5288, 0x0000ABCD, 0x02000000),
        }
        encoding, value, base = cases[mode]
        image[0x84:0x88] = struct.pack(
            "<2H", encoding, 0x4718
        )  # STRH/negative form; BX r3 returns to the ARM self-loop.
        image[0xC0:0xDC] = struct.pack(
            "<7I",
            0xE59F001C,  # LDR r0,[pc,#0x1C] -> value at 0xE4.
            0xE59F101C,  # LDR r1,[pc,#0x1C] -> base at 0xE8.
            0xE3A04080,  # MOV r4,#0x80.
            0xE3540080,  # CMP r4,#0x80 -> Z=1,C=1; STRH must preserve them.
            0xE59F2004,  # LDR r2,[pc,#4] -> Thumb entry | 1 at 0xDC.
            0xE59F3004,  # LDR r3,[pc,#4] -> ARM return loop at 0xE0.
            0xE12FFF12,  # BX r2.
        )
        image[0xDC:0xEC] = struct.pack(
            "<4I", 0x08000085, 0x080000EC, value, base
        )
        image[0xEC:0xF0] = struct.pack("<I", branch(0xEC, 0xEC))
    elif mode in (
        "thumb-strb-ewram-zero-even",
        "thumb-strb-ewram-thirtyone-odd",
        "thumb-strb-iwram-zero-odd",
        "thumb-strb-iwram-thirtyone-even",
        "thumb-strb-ewram-thirtyone-even-alias",
        "thumb-strb-invalid-odd",
        "thumb-strb-wraparound",
        "thumb-ldrb-unsupported",
        "thumb-strb-register-unsupported",
    ):
        cases = {
            # Format 9: 01110 imm5 Rb Rd. The word initialized below makes
            # the exact target byte and its untouched neighbors observable.
            "thumb-strb-ewram-zero-even": (0x7008, 0xAABBCCDD, 0x02000000, 0x02000000),
            "thumb-strb-ewram-thirtyone-odd": (
                0x77C8,
                0x12345678,
                0x01FFFFE2,
                0x02000000,
            ),
            "thumb-strb-iwram-zero-odd": (
                0x7008,
                0x0000CDEF,
                0x03000001,
                0x03000000,
            ),
            "thumb-strb-iwram-thirtyone-even": (
                0x77C8,
                0x000000BE,
                0x02FFFFE1,
                0x03000000,
            ),
            # Rd==Rb: old r1 is both the address base and store value.
            "thumb-strb-ewram-thirtyone-even-alias": (
                0x77C9,
                0x00000000,
                0x01FFFFE1,
                0x02000000,
            ),
            "thumb-strb-invalid-odd": (
                0x77C8,
                0x000000A5,
                0x03FFFFE2,
                0x02000000,
            ),
            # 0xFFFFFFE1 + 31 wraps to the exact guest EA 0.
            "thumb-strb-wraparound": (
                0x77C8,
                0x0000005A,
                0xFFFFFFE1,
                0x02000000,
            ),
            "thumb-ldrb-unsupported": (
                0x7808,
                0x000000A5,
                0x02000000,
                0x02000000,
            ),
            # STRB r0,[r1,r2], Format 7 register-offset.
            "thumb-strb-register-unsupported": (
                0x5488,
                0x000000A5,
                0x02000000,
                0x02000000,
            ),
        }
        encoding, value, base, initialized_region = cases[mode]
        image[0x84:0x88] = struct.pack(
            "<2H", encoding, 0x4718
        )  # STRB/negative form; BX r3 returns to the ARM self-loop.
        image[0xC0:0xE8] = struct.pack(
            "<10I",
            0xE59F0020,  # LDR r0,[pc,#0x20] -> sentinel word at 0xE8.
            0xE59F1020,  # LDR r1,[pc,#0x20] -> EWRAM/IWRAM start at 0xEC.
            0xE5810000,  # STR sentinel,[r1], preserving three adjacent bytes.
            0xE3A04080,  # MOV r4,#0x80.
            0xE3540080,  # CMP r4,#0x80 -> Z=1,C=1; STRB must preserve flags.
            0xE59F0014,  # LDR r0,[pc,#0x14] -> byte value at 0xF0.
            0xE59F1014,  # LDR r1,[pc,#0x14] -> byte-store base at 0xF4.
            0xE59F2018,  # LDR r2,[pc,#0x18] -> Thumb entry | 1 at 0xFC.
            0xE28F3010,  # ADD r3,pc,#0x10 -> ARM loop at 0x080000F8.
            0xE12FFF12,  # BX r2.
        )
        image[0xE8:0xF8] = struct.pack(
            "<4I", 0x44332211, initialized_region, value, base
        )
        image[0xF8:0xFC] = struct.pack("<I", branch(0xF8, 0xF8))
        image[0xFC:0x100] = struct.pack("<I", 0x08000085)
    elif mode.startswith("store-"):
        guest_address = {
            "store-ewram": 0x02000000,
            "store-iwram": 0x03000000,
            "store-out-of-range": 0x04000000,
            "store-unaligned": 0x02000002,
            "store-condition-false": 0x04000000,
        }[mode]
        store = 0x05810000 if mode == "store-condition-false" else 0xE5810000
        image[0xC0:0xD0] = struct.pack(
            "<4I",
            0xE3A00078,  # MOV r0, #0x78
            0xE59F1004,  # LDR r1, [pc, #4] -> guest address at 0x080000D0
            store,  # STR(EQ) r0, [r1]
            branch(0xCC, 0xCC),  # B finished
        )
        image[0xD0:0xD4] = struct.pack("<I", guest_address)
    else:
        image[0xC0:0xD4] = struct.pack(
            "<5I",
            0xE3A00000,  # MOV r0, #0
            0xE2800001,  # ADD r0, r0, #1
            0xE350000A,  # CMP r0, #10
            branch(0xCC, 0xC4, condition=1),  # BNE counter_loop
            branch(0xD0, 0xD0),  # B finished
        )
    image[0xBD] = header_checksum(image)
    if mode == "unsupported":
        image[0xC0:0xC4] = struct.pack("<I", 0xE5910000)  # LDR r0, [r1]
    elif mode == "bad-checksum":
        image[0xBD] ^= 1
    return bytes(image)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument(
        "--mode",
        choices=(
            "valid",
            "thumb-mov",
            "thumb-movs-immediate",
            "thumb-movs-zero",
            "thumb-lsl-bx",
            "thumb-lsl-zero-carry-clear",
            "thumb-lsl-zero-carry-set",
            "thumb-lsl-one-carry",
            "thumb-lsl-nineteen",
            "thumb-lsl-thirtyone",
            "thumb-lsl-nonalias",
            "thumb-lsr-unsupported",
            "thumb-asr-unsupported",
            "thumb-lsl-register-unsupported",
            "thumb-addsub-add-register",
            "thumb-addsub-sub-register",
            "thumb-addsub-add-imm-zero",
            "thumb-addsub-add-imm-seven",
            "thumb-addsub-sub-imm-zero",
            "thumb-addsub-sub-imm-seven",
            "thumb-addsub-add-imm-carry",
            "thumb-addsub-sub-imm-overflow",
            "thumb-addsub-out-of-family",
            "thumb-addsub-literal-bx",
            "thumb-ldr-literal-bx",
            "thumb-ldr-literal-out-of-rom",
            "thumb-bl",
            "thumb-bl-broken-pair",
            "thumb-bl-suffix-without-prefix",
            "thumb-bl-out-of-range",
            "thumb-swi-reset",
            "thumb-swi-unsupported",
            "thumb-push",
            "bx-interwork",
            "bx-out-of-range",
            "bx-condition-false",
            "msr-mode",
            "literal-load",
            "literal-condition-false",
            "literal-out-of-range",
            "add-pc",
            "add-pc-condition-false",
            "mov-register-pc",
            "mov-register-condition-false",
            "mov-register-shift",
            "thumb-strh-ewram-zero-even",
            "thumb-strh-ewram-thirtyone-odd-alias",
            "thumb-strh-iwram-zero-odd",
            "thumb-strh-iwram-thirtyone-even",
            "thumb-strh-invalid-odd",
            "thumb-ldrh-unsupported",
            "thumb-strh-register-unsupported",
            "thumb-strb-ewram-zero-even",
            "thumb-strb-ewram-thirtyone-odd",
            "thumb-strb-iwram-zero-odd",
            "thumb-strb-iwram-thirtyone-even",
            "thumb-strb-ewram-thirtyone-even-alias",
            "thumb-strb-invalid-odd",
            "thumb-strb-wraparound",
            "thumb-ldrb-unsupported",
            "thumb-strb-register-unsupported",
            "store-ewram",
            "store-iwram",
            "store-out-of-range",
            "store-unaligned",
            "store-condition-false",
            "unsupported",
            "out-of-range",
            "bad-checksum",
        ),
        default="valid",
    )
    args = parser.parse_args()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    image = generate(args.mode)
    args.out.write_bytes(image)
    print(f"wrote {args.out} ({len(image)} bytes, mode={args.mode})")


if __name__ == "__main__":
    main()

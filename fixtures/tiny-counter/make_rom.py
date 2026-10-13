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
            "msr-mode",
            "literal-load",
            "literal-condition-false",
            "literal-out-of-range",
            "add-pc",
            "add-pc-condition-false",
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

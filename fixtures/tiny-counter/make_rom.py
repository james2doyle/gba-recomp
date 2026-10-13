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
        choices=("valid", "unsupported", "out-of-range", "bad-checksum"),
        default="valid",
    )
    args = parser.parse_args()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    image = generate(args.mode)
    args.out.write_bytes(image)
    print(f"wrote {args.out} ({len(image)} bytes, mode={args.mode})")


if __name__ == "__main__":
    main()

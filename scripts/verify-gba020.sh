#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

: "${LLVM_SYS_231_PREFIX:?set LLVM_SYS_231_PREFIX to the LLVM 23.1.3 installation}"
if [ "$("$LLVM_SYS_231_PREFIX/bin/llvm-config" --version)" != "23.1.3" ]; then
    echo "LLVM 23.1.3 is required" >&2
    exit 2
fi
os=$(uname -s)
arch=$(uname -m)
case "$os/$arch" in
    Darwin/arm64)
        export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="$LLVM_SYS_231_PREFIX/bin/clang"
        ;;
    Linux/x86_64)
        export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$LLVM_SYS_231_PREFIX/bin/clang"
        ;;
    *)
        echo "unsupported GBA-020 host: $os/$arch" >&2
        exit 2
        ;;
esac

rom="$root/target/gba020/tiny-counter.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$rom"

for optimization in O0 O2; do
    package="$root/target/gba020/package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$rom" --out "$package" --opt "$optimization" --steps 100
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":100,"r0":10,"pc":"0x080000D0","cpsr":"0x6000001F","guest_cycle":100}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization produced an unexpected state: $actual" >&2
        exit 1
    fi
    python3 - "$package/manifest.json" "$rom" "$optimization" <<'PY'
import hashlib
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
rom = pathlib.Path(sys.argv[2]).read_bytes()
assert manifest["rom_sha256"] == hashlib.sha256(rom).hexdigest()
assert manifest["execution_mode"] == "strict-aot"
assert manifest["instructions_aot"] == 6
assert manifest["optimization"] == sys.argv[3]
assert manifest["run_instruction_budget"] == 100
PY
    if find "$package" -type f \( -name '*.gba' -o -name '*.o' -o -name '*.bc' \) | grep . >/dev/null; then
        echo "package contains source ROM or build-only code" >&2
        exit 1
    fi
    case "$os" in
        Darwin)
            if otool -L "$package/game" | grep -E 'libLLVM|libLTO' >/dev/null; then
                echo "game executable unexpectedly depends on LLVM" >&2
                exit 1
            fi
            ;;
        Linux)
            if ldd "$package/game" | grep -E 'libLLVM|libLTO' >/dev/null; then
                echo "game executable unexpectedly depends on LLVM" >&2
                exit 1
            fi
            ;;
    esac
done

bad_rom="$root/target/gba020/unsupported.gba"
bad_package="$root/target/gba020/unsupported-package"
python3 fixtures/tiny-counter/make_rom.py --out "$bad_rom" --mode unsupported
rm -rf "$bad_package"
if cargo run --locked --quiet -p gba-recomp -- compile \
    --rom "$bad_rom" --out "$bad_package" --opt O0 --steps 100; then
    echo "unsupported instruction unexpectedly produced a package" >&2
    exit 1
fi
if [ -e "$bad_package" ]; then
    echo "failed compile left a partial package" >&2
    exit 1
fi

range_rom="$root/target/gba020/out-of-range.gba"
range_package="$root/target/gba020/out-of-range-package"
python3 fixtures/tiny-counter/make_rom.py --out "$range_rom" --mode out-of-range
rm -rf "$range_package"
if cargo run --locked --quiet -p gba-recomp -- compile \
    --rom "$range_rom" --out "$range_package" --opt O0 --steps 100; then
    echo "out-of-ROM branch unexpectedly produced a package" >&2
    exit 1
fi
if [ -e "$range_package" ]; then
    echo "failed compile left a partial package" >&2
    exit 1
fi

checksum_rom="$root/target/gba020/bad-checksum.gba"
checksum_package="$root/target/gba020/bad-checksum-package"
python3 fixtures/tiny-counter/make_rom.py --out "$checksum_rom" --mode bad-checksum
rm -rf "$checksum_package"
if cargo run --locked --quiet -p gba-recomp -- compile \
    --rom "$checksum_rom" --out "$checksum_package" --opt O0 --steps 100; then
    echo "invalid header checksum unexpectedly produced a package" >&2
    exit 1
fi
if [ -e "$checksum_package" ]; then
    echo "failed intake left a partial package" >&2
    exit 1
fi

echo "GBA-020 headless AOT fixture passed on $os/$arch"

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
    expected='{"status":"budget_exhausted","steps":100,"r0":10,"r1":0,"pc":"0x080000D0","cpsr":"0x6000001F","guest_cycle":100,"ewram_word0":0,"iwram_word0":0}'
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
assert manifest["abi_version"] == 3
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

runtime_test="$root/target/gba020/runtime-v2-test"
if [ "$os" = Darwin ]; then
    sdkroot=${SDKROOT:-$(xcrun --sdk macosx --show-sdk-path)}
    "$LLVM_SYS_231_PREFIX/bin/clang" -std=c11 -Wall -Wextra -Werror \
        -isysroot "$sdkroot" -mmacosx-version-min=14.0.0 \
        -I crates/gba-runtime-abi/include \
        crates/gba-runtime-abi/tests/runtime_v2_test.c \
        crates/gba-runtime-abi/src/runtime_v2.c \
        -o "$runtime_test"
else
    "$LLVM_SYS_231_PREFIX/bin/clang" -std=c11 -Wall -Wextra -Werror \
        -I crates/gba-runtime-abi/include \
        crates/gba-runtime-abi/tests/runtime_v2_test.c \
        crates/gba-runtime-abi/src/runtime_v2.c \
        -o "$runtime_test"
fi
"$runtime_test"

runtime_v3_test="$root/target/gba020/runtime-v3-test"
if [ "$os" = Darwin ]; then
    "$LLVM_SYS_231_PREFIX/bin/clang" -std=c11 -Wall -Wextra -Werror \
        -isysroot "$sdkroot" -mmacosx-version-min=14.0.0 \
        -I crates/gba-runtime-abi/include \
        crates/gba-runtime-abi/tests/runtime_v3_test.c \
        crates/gba-runtime-abi/src/runtime_v3.c \
        -o "$runtime_v3_test"
else
    "$LLVM_SYS_231_PREFIX/bin/clang" -std=c11 -Wall -Wextra -Werror \
        -I crates/gba-runtime-abi/include \
        crates/gba-runtime-abi/tests/runtime_v3_test.c \
        crates/gba-runtime-abi/src/runtime_v3.c \
        -o "$runtime_v3_test"
fi
"$runtime_v3_test"

msr_rom="$root/target/gba020/msr-mode.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$msr_rom" --mode msr-mode
for optimization in O0 O2; do
    package="$root/target/gba020/msr-package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$msr_rom" --out "$package" --opt "$optimization" --steps 4
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":4,"r0":19,"r1":0,"pc":"0x080000C8","cpsr":"0x00000013","guest_cycle":4,"ewram_word0":0,"iwram_word0":0}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization MSR fixture produced an unexpected state: $actual" >&2
        exit 1
    fi
    python3 - "$package/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["abi_version"] == 3
assert manifest["instructions_aot"] == 4
PY
done

literal_rom="$root/target/gba020/literal-load.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$literal_rom" --mode literal-load
for optimization in O0 O2; do
    package="$root/target/gba020/literal-package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$literal_rom" --out "$package" --opt "$optimization" --steps 3
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":3,"r0":305419896,"r1":0,"pc":"0x080000C4","cpsr":"0x0000001F","guest_cycle":3,"ewram_word0":0,"iwram_word0":0}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization ROM literal fixture produced an unexpected state: $actual" >&2
        exit 1
    fi
    python3 - "$package/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["abi_version"] == 3
assert manifest["instructions_aot"] == 3
PY
done

condition_rom="$root/target/gba020/literal-condition-false.gba"
condition_package="$root/target/gba020/literal-condition-false-package"
python3 fixtures/tiny-counter/make_rom.py --out "$condition_rom" --mode literal-condition-false
rm -rf "$condition_package"
cargo run --locked --quiet -p gba-recomp -- compile \
    --rom "$condition_rom" --out "$condition_package" --opt O2 --steps 4
actual=$("$condition_package/game")
expected='{"status":"budget_exhausted","steps":4,"r0":165,"r1":0,"pc":"0x080000C8","cpsr":"0x0000001F","guest_cycle":4,"ewram_word0":0,"iwram_word0":0}'
if [ "$actual" != "$expected" ]; then
    echo "false-predicate ROM literal fixture produced an unexpected state: $actual" >&2
    exit 1
fi

literal_range_rom="$root/target/gba020/literal-out-of-range.gba"
literal_range_package="$root/target/gba020/literal-out-of-range-package"
literal_range_output="$root/target/gba020/literal-out-of-range.txt"
python3 fixtures/tiny-counter/make_rom.py --out "$literal_range_rom" --mode literal-out-of-range
rm -rf "$literal_range_package"
if cargo run --locked --quiet -p gba-recomp -- compile \
    --rom "$literal_range_rom" --out "$literal_range_package" --opt O0 --steps 4 \
    >"$literal_range_output" 2>&1; then
    echo "out-of-ROM literal unexpectedly produced a package" >&2
    exit 1
fi
if ! grep -q 'rom_literal_out_of_rom at 0x080000C0' "$literal_range_output"; then
    cat "$literal_range_output" >&2
    echo "out-of-ROM literal did not report its guest PC" >&2
    exit 1
fi
if [ -e "$literal_range_package" ]; then
    echo "failed literal compile left a partial package" >&2
    exit 1
fi

add_pc_rom="$root/target/gba020/add-pc.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$add_pc_rom" --mode add-pc
for optimization in O0 O2; do
    package="$root/target/gba020/add-pc-package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$add_pc_rom" --out "$package" --opt "$optimization" --steps 3
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":3,"r0":134217960,"r1":0,"pc":"0x080000C4","cpsr":"0x0000001F","guest_cycle":3,"ewram_word0":0,"iwram_word0":0}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization PC-relative ADD fixture produced an unexpected state: $actual" >&2
        exit 1
    fi
    python3 - "$package/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["instructions_aot"] == 3
PY
done

add_pc_condition_rom="$root/target/gba020/add-pc-condition-false.gba"
add_pc_condition_package="$root/target/gba020/add-pc-condition-false-package"
python3 fixtures/tiny-counter/make_rom.py --out "$add_pc_condition_rom" --mode add-pc-condition-false
rm -rf "$add_pc_condition_package"
cargo run --locked --quiet -p gba-recomp -- compile \
    --rom "$add_pc_condition_rom" --out "$add_pc_condition_package" --opt O2 --steps 4
actual=$("$add_pc_condition_package/game")
expected='{"status":"budget_exhausted","steps":4,"r0":165,"r1":0,"pc":"0x080000C8","cpsr":"0x0000001F","guest_cycle":4,"ewram_word0":0,"iwram_word0":0}'
if [ "$actual" != "$expected" ]; then
    echo "false-predicate PC-relative ADD fixture produced an unexpected state: $actual" >&2
    exit 1
fi

mov_rom="$root/target/gba020/mov-register-pc.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$mov_rom" --mode mov-register-pc
for optimization in O0 O2; do
    package="$root/target/gba020/mov-register-pc-package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$mov_rom" --out "$package" --opt "$optimization" --steps 4
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":4,"r0":134217928,"r1":0,"pc":"0x080000C8","cpsr":"0x0000001F","guest_cycle":4,"ewram_word0":0,"iwram_word0":0}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization MOV PC fixture produced an unexpected state: $actual" >&2
        exit 1
    fi
    python3 - "$package/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["instructions_aot"] == 4
assert manifest["execution_mode"] == "strict-aot"
PY
done

mov_false_rom="$root/target/gba020/mov-register-condition-false.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$mov_false_rom" --mode mov-register-condition-false
for optimization in O0 O2; do
    package="$root/target/gba020/mov-register-condition-false-package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$mov_false_rom" --out "$package" --opt "$optimization" --steps 4
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":4,"r0":165,"r1":0,"pc":"0x080000C8","cpsr":"0x0000001F","guest_cycle":4,"ewram_word0":0,"iwram_word0":0}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization false-predicate MOV fixture produced an unexpected state: $actual" >&2
        exit 1
    fi
done

mov_shift_rom="$root/target/gba020/mov-register-shift.gba"
mov_shift_package="$root/target/gba020/mov-register-shift-package"
mov_shift_output="$root/target/gba020/mov-register-shift.txt"
python3 fixtures/tiny-counter/make_rom.py --out "$mov_shift_rom" --mode mov-register-shift
rm -rf "$mov_shift_package"
if cargo run --locked --quiet -p gba-recomp -- compile \
    --rom "$mov_shift_rom" --out "$mov_shift_package" --opt O0 --steps 4 \
    >"$mov_shift_output" 2>&1; then
    echo "shifted MOV unexpectedly produced a package" >&2
    exit 1
fi
if ! grep -q 'only MOV Rd,Rm with LSL #0' "$mov_shift_output"; then
    cat "$mov_shift_output" >&2
    echo "shifted MOV did not report the narrow decoder restriction" >&2
    exit 1
fi
if [ -e "$mov_shift_package" ]; then
    echo "failed shifted MOV compile left a partial package" >&2
    exit 1
fi

for mode in store-ewram store-iwram; do
    store_rom="$root/target/gba020/$mode.gba"
    python3 fixtures/tiny-counter/make_rom.py --out "$store_rom" --mode "$mode"
    for optimization in O0 O2; do
        package="$root/target/gba020/$mode-package-$optimization"
        rm -rf "$package"
        cargo run --locked --quiet -p gba-recomp -- compile \
            --rom "$store_rom" --out "$package" --opt "$optimization" --steps 4
        actual=$("$package/game")
        if [ "$mode" = store-ewram ]; then
            expected='{"status":"budget_exhausted","steps":4,"r0":120,"r1":33554432,"pc":"0x080000CC","cpsr":"0x0000001F","guest_cycle":4,"ewram_word0":120,"iwram_word0":0}'
        else
            expected='{"status":"budget_exhausted","steps":4,"r0":120,"r1":50331648,"pc":"0x080000CC","cpsr":"0x0000001F","guest_cycle":4,"ewram_word0":0,"iwram_word0":120}'
        fi
        if [ "$actual" != "$expected" ]; then
            echo "$optimization $mode fixture produced an unexpected state: $actual" >&2
            exit 1
        fi
        python3 - "$package/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["abi_version"] == 3
assert manifest["instructions_aot"] == 5
PY
    done
done

store_false_rom="$root/target/gba020/store-condition-false.gba"
store_false_package="$root/target/gba020/store-condition-false-package"
python3 fixtures/tiny-counter/make_rom.py --out "$store_false_rom" --mode store-condition-false
rm -rf "$store_false_package"
cargo run --locked --quiet -p gba-recomp -- compile \
    --rom "$store_false_rom" --out "$store_false_package" --opt O2 --steps 4
actual=$("$store_false_package/game")
expected='{"status":"budget_exhausted","steps":4,"r0":120,"r1":67108864,"pc":"0x080000CC","cpsr":"0x0000001F","guest_cycle":4,"ewram_word0":0,"iwram_word0":0}'
if [ "$actual" != "$expected" ]; then
    echo "false-predicate STR fixture produced an unexpected state: $actual" >&2
    exit 1
fi

for mode in store-out-of-range store-unaligned; do
    store_rom="$root/target/gba020/$mode.gba"
    store_package="$root/target/gba020/$mode-package"
    store_output="$root/target/gba020/$mode.txt"
    python3 fixtures/tiny-counter/make_rom.py --out "$store_rom" --mode "$mode"
    rm -rf "$store_package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$store_rom" --out "$store_package" --opt O2 --steps 4
    if "$store_package/game" >"$store_output" 2>&1; then
        echo "$mode STR unexpectedly executed without a fault" >&2
        exit 1
    fi
    if [ "$mode" = store-out-of-range ]; then
        expected_fault='guest stopped: reason=4 pc=0x080000C8 address=0x04000000 fault_code=5'
    else
        expected_fault='guest stopped: reason=4 pc=0x080000C8 address=0x02000002 fault_code=4'
    fi
    if ! grep -q "$expected_fault" "$store_output"; then
        cat "$store_output" >&2
        echo "$mode STR did not report the expected guest fault" >&2
        exit 1
    fi
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

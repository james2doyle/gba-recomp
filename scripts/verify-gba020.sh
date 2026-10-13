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
assert manifest["abi_version"] == 4
assert manifest["machine_state_version"] == 1
assert manifest["bios_hle_version"] == 1
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

runtime_hle_test="$root/target/gba020/runtime-hle-v1-test"
if [ "$os" = Darwin ]; then
    "$LLVM_SYS_231_PREFIX/bin/clang" -std=c11 -Wall -Wextra -Werror \
        -isysroot "$sdkroot" -mmacosx-version-min=14.0.0 \
        -I crates/gba-runtime-abi/include \
        crates/gba-runtime-abi/tests/runtime_hle_v1_test.c \
        crates/gba-runtime-abi/src/runtime_hle_v1.c \
        -o "$runtime_hle_test"
else
    "$LLVM_SYS_231_PREFIX/bin/clang" -std=c11 -Wall -Wextra -Werror \
        -I crates/gba-runtime-abi/include \
        crates/gba-runtime-abi/tests/runtime_hle_v1_test.c \
        crates/gba-runtime-abi/src/runtime_hle_v1.c \
        -o "$runtime_hle_test"
fi
"$runtime_hle_test"

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
assert manifest["abi_version"] == 4
assert manifest["machine_state_version"] == 1
assert manifest["bios_hle_version"] == 1
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
assert manifest["abi_version"] == 4
assert manifest["machine_state_version"] == 1
assert manifest["bios_hle_version"] == 1
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
assert manifest["abi_version"] == 4
assert manifest["machine_state_version"] == 1
assert manifest["bios_hle_version"] == 1
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

push_rom="$root/target/gba020/thumb-push.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$push_rom" --mode thumb-push
for optimization in O0 O2; do
    package="$root/target/gba020/thumb-push-package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$push_rom" --out "$package" --opt "$optimization" --steps 15
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":15,"r0":50331648,"r1":134217861,"pc":"0x080000EC","cpsr":"0x0000001F","guest_cycle":15,"ewram_word0":0,"iwram_word0":68}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization Thumb PUSH fixture produced an unexpected state: $actual" >&2
        exit 1
    fi
    python3 - "$package/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["execution_mode"] == "strict-aot"
assert manifest["instructions_aot"] == 15
PY
done

thumb_mov_rom="$root/target/gba020/thumb-mov.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$thumb_mov_rom" --mode thumb-mov
for optimization in O0 O2; do
    package="$root/target/gba020/thumb-mov-package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$thumb_mov_rom" --out "$package" --opt "$optimization" --steps 11
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":11,"r0":119,"r1":134217866,"pc":"0x080000D8","cpsr":"0x0000001F","guest_cycle":11,"ewram_word0":0,"iwram_word0":0}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization Thumb high-register MOV fixture produced an unexpected state: $actual" >&2
        exit 1
    fi
    python3 - "$package/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["execution_mode"] == "strict-aot"
assert manifest["instructions_aot"] == 11
PY
done

verify_thumb_movs() {
    mode=$1
    expected=$2
    thumb_movs_rom="$root/target/gba020/$mode.gba"
    python3 fixtures/tiny-counter/make_rom.py --out "$thumb_movs_rom" --mode "$mode"
    for optimization in O0 O2; do
        package="$root/target/gba020/$mode-package-$optimization"
        rm -rf "$package"
        cargo run --locked --quiet -p gba-recomp -- compile \
            --rom "$thumb_movs_rom" --out "$package" --opt "$optimization" --steps 9
        actual=$("$package/game")
        if [ "$actual" != "$expected" ]; then
            echo "$optimization $mode fixture produced an unexpected state: $actual" >&2
            exit 1
        fi
        if ! grep -Eq '"execution_mode"[[:space:]]*:[[:space:]]*"strict-aot"' "$package/manifest.json"; then
            echo "$optimization $mode package is not strict AOT" >&2
            exit 1
        fi
    done
}
verify_thumb_movs thumb-movs-immediate '{"status":"budget_exhausted","steps":9,"r0":255,"r1":255,"pc":"0x080000D8","cpsr":"0x3000001F","guest_cycle":9,"ewram_word0":0,"iwram_word0":0}'
verify_thumb_movs thumb-movs-zero '{"status":"budget_exhausted","steps":9,"r0":0,"r1":0,"pc":"0x080000D8","cpsr":"0x7000001F","guest_cycle":9,"ewram_word0":0,"iwram_word0":0}'

thumb_bl_rom="$root/target/gba020/thumb-bl.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$thumb_bl_rom" --mode thumb-bl
for optimization in O0 O2; do
    package="$root/target/gba020/thumb-bl-package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$thumb_bl_rom" --out "$package" --opt "$optimization" --steps 10
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":10,"r0":66,"r1":134217936,"pc":"0x080000D0","cpsr":"0x0000001F","guest_cycle":10,"ewram_word0":0,"iwram_word0":0}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization Thumb BL call/return fixture produced an unexpected state: $actual" >&2
        exit 1
    fi
    python3 - "$package/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["execution_mode"] == "strict-aot"
assert manifest["instructions_aot"] == 11
PY
done

for mode in thumb-bl-broken-pair thumb-bl-suffix-without-prefix thumb-bl-out-of-range; do
    rom="$root/target/gba020/$mode.gba"
    package="$root/target/gba020/$mode-package"
    output="$root/target/gba020/$mode.txt"
    python3 fixtures/tiny-counter/make_rom.py --out "$rom" --mode "$mode"
    rm -rf "$package"
    if cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$rom" --out "$package" --opt O0 --steps 10 >"$output" 2>&1; then
        echo "$mode unexpectedly produced a package" >&2
        exit 1
    fi
    case "$mode" in
        thumb-bl-broken-pair)
            expected_error='thumb_bl_pair_broken at 0x08000084'
            ;;
        thumb-bl-suffix-without-prefix)
            expected_error='thumb_bl_suffix_without_prefix at 0x08000084'
            ;;
        thumb-bl-out-of-range)
            expected_error='branch_target_out_of_rom at 0x08000086'
            ;;
    esac
    if ! grep -q "$expected_error" "$output"; then
        cat "$output" >&2
        echo "$mode did not report the expected static BL rejection" >&2
        exit 1
    fi
    if [ -e "$package" ]; then
        echo "$mode failure left a partial package" >&2
        exit 1
    fi
done

bx_rom="$root/target/gba020/bx-interwork.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$bx_rom" --mode bx-interwork
for optimization in O0 O2; do
    package="$root/target/gba020/bx-interwork-package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$bx_rom" --out "$package" --opt "$optimization" --steps 6
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":6,"r0":134217861,"r1":134217934,"pc":"0x080000CC","cpsr":"0x0000001F","guest_cycle":6,"ewram_word0":0,"iwram_word0":0}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization ARM/Thumb BX fixture produced an unexpected state: $actual" >&2
        exit 1
    fi
    python3 - "$package/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["execution_mode"] == "strict-aot"
assert manifest["instructions_aot"] == 6
PY
done

bx_false_rom="$root/target/gba020/bx-condition-false.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$bx_false_rom" --mode bx-condition-false
for optimization in O0 O2; do
    package="$root/target/gba020/bx-condition-false-package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$bx_false_rom" --out "$package" --opt "$optimization" --steps 4
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":4,"r0":134217861,"r1":134217934,"pc":"0x080000CC","cpsr":"0x0000001F","guest_cycle":4,"ewram_word0":0,"iwram_word0":0}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization false-condition BX fixture produced an unexpected state: $actual" >&2
        exit 1
    fi
    python3 - "$package/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["execution_mode"] == "strict-aot"
assert manifest["instructions_aot"] == 6
PY
done

bx_range_rom="$root/target/gba020/bx-out-of-range.gba"
bx_range_package="$root/target/gba020/bx-out-of-range-package"
bx_range_output="$root/target/gba020/bx-out-of-range.txt"
python3 fixtures/tiny-counter/make_rom.py --out "$bx_range_rom" --mode bx-out-of-range
rm -rf "$bx_range_package"
if cargo run --locked --quiet -p gba-recomp -- compile \
    --rom "$bx_range_rom" --out "$bx_range_package" --opt O0 --steps 4 \
    >"$bx_range_output" 2>&1; then
    echo "out-of-ROM BX target unexpectedly produced a package" >&2
    exit 1
fi
if ! grep -q 'branch_target_out_of_rom at 0x080000C8' "$bx_range_output"; then
    cat "$bx_range_output" >&2
    echo "out-of-ROM BX target did not report its source PC" >&2
    exit 1
fi
if [ -e "$bx_range_package" ]; then
    echo "failed BX compile left a partial package" >&2
    exit 1
fi

swi_rom="$root/target/gba020/thumb-swi-reset.gba"
python3 fixtures/tiny-counter/make_rom.py --out "$swi_rom" --mode thumb-swi-reset
for optimization in O0 O2; do
    package="$root/target/gba020/thumb-swi-reset-package-$optimization"
    rm -rf "$package"
    cargo run --locked --quiet -p gba-recomp -- compile \
        --rom "$swi_rom" --out "$package" --opt "$optimization" --steps 9
    actual=$("$package/game")
    expected='{"status":"budget_exhausted","steps":9,"r0":2,"r1":0,"pc":"0x0800008A","cpsr":"0x0000003F","guest_cycle":9,"ewram_word0":0,"iwram_word0":0}'
    if [ "$actual" != "$expected" ]; then
        echo "$optimization Thumb SWI RegisterRamReset fixture produced an unexpected state: $actual" >&2
        exit 1
    fi
    python3 - "$package/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["abi_version"] == 4
assert manifest["machine_state_version"] == 1
assert manifest["bios_hle_version"] == 1
assert manifest["execution_mode"] == "strict-aot"
assert manifest["instructions_aot"] == 12
PY
done

swi_unsupported_rom="$root/target/gba020/thumb-swi-unsupported.gba"
swi_unsupported_package="$root/target/gba020/thumb-swi-unsupported-package"
swi_unsupported_output="$root/target/gba020/thumb-swi-unsupported.txt"
python3 fixtures/tiny-counter/make_rom.py \
    --out "$swi_unsupported_rom" --mode thumb-swi-unsupported
rm -rf "$swi_unsupported_package"
if cargo run --locked --quiet -p gba-recomp -- compile \
    --rom "$swi_unsupported_rom" --out "$swi_unsupported_package" --opt O2 --steps 9 \
    >"$swi_unsupported_output" 2>&1; then
    echo "unsupported Thumb SWI unexpectedly produced a package" >&2
    exit 1
fi
if ! grep -q 'swi_unsupported at 0x08000088: Thumb SWI #2' "$swi_unsupported_output"; then
    cat "$swi_unsupported_output" >&2
    echo "unsupported Thumb SWI did not report its source PC" >&2
    exit 1
fi
if [ -e "$swi_unsupported_package" ]; then
    echo "failed unsupported SWI compile left a partial package" >&2
    exit 1
fi

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

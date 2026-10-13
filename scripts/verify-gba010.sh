#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

if [ -z "${LLVM_SYS_231_PREFIX:-}" ]; then
    echo "set LLVM_SYS_231_PREFIX to the LLVM 23.1.3 installation" >&2
    exit 2
fi
llvm_config=${LLVM_CONFIG:-"$LLVM_SYS_231_PREFIX/bin/llvm-config"}
if [ "$("$llvm_config" --version)" != "23.1.3" ]; then
    echo "LLVM 23.1.3 is required; found $("$llvm_config" --version)" >&2
    exit 2
fi
os=$(uname -s)
arch=$(uname -m)
case "$os/$arch" in
    Darwin/arm64)
        host=macos-arm64
        expected='Mach-O 64-bit object arm64'
        rust_target_linker=CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER
        llvm_linker="$LLVM_SYS_231_PREFIX/bin/ld64.lld"
        export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="$LLVM_SYS_231_PREFIX/bin/clang"
        ;;
    Linux/x86_64)
        host=linux-x86_64
        expected='ELF 64-bit LSB relocatable, x86-64'
        rust_target_linker=CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER
        llvm_linker="$LLVM_SYS_231_PREFIX/bin/ld.lld"
        export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$LLVM_SYS_231_PREFIX/bin/clang"
        ;;
    *)
        echo "unsupported GBA-010 native host: $os/$arch" >&2
        exit 2
        ;;
esac
if [ ! -x "$LLVM_SYS_231_PREFIX/bin/clang" ] || [ ! -x "$llvm_linker" ]; then
    echo "LLVM clang/lld from the locked package are required" >&2
    exit 2
fi

cc=${CC:-cc}
build_dir="$root/target/gba010/$host"
mkdir -p "$build_dir"
c_linker=$("$cc" -print-prog-name=ld)
if [ "$os" = Darwin ]; then
    rust_sysroot=${SDKROOT:-"$(xcrun --sdk macosx --show-sdk-path)"}
    export SDKROOT="$rust_sysroot"
else
    rust_sysroot=$("$LLVM_SYS_231_PREFIX/bin/clang" --print-sysroot)
fi

for optimization in O0 O2; do
    object="$build_dir/alu-$optimization.o"
    executable="$build_dir/fixture-$optimization"
    cargo run --manifest-path Cargo.toml -p gba-recomp -- \
        emit-fixture --out "$object" --opt "$optimization"

    file_output=$(file "$object")
    case "$file_output" in
        *"$expected"*) ;;
        *) echo "unexpected object format: $file_output" >&2; exit 1 ;;
    esac

    "$cc" -std=c11 -Wall -Wextra -Werror \
        -I crates/gba-runtime-abi/include \
        "$object" crates/gba-runtime-abi/tests/fixture_runner.c \
        -o "$executable"
    "$executable"
    case "$os" in
        Darwin)
            if otool -L "$executable" | grep -E 'libLLVM|libLTO' >/dev/null; then
                echo "fixture executable unexpectedly depends on LLVM" >&2
                exit 1
            fi
            ;;
        Linux)
            if ldd "$executable" | grep -E 'libLLVM|libLTO' >/dev/null; then
                echo "fixture executable unexpectedly depends on LLVM" >&2
                exit 1
            fi
            ;;
    esac
done

{
    echo "host=$os/$arch"
    echo "llvm_config=$llvm_config"
    "$llvm_config" --version
    echo "llvm_shared_mode=$("$llvm_config" --shared-mode)"
    echo "rust_target_linker_var=$rust_target_linker"
    echo "rust_target_linker=$LLVM_SYS_231_PREFIX/bin/clang"
    "$llvm_linker" --version
    echo "rust_sysroot=$rust_sysroot"
    rustc --version --verbose
    cargo --version
    "$cc" --version
    echo "c_linker=$c_linker"
    if [ "$os" = Darwin ]; then
        "$c_linker" -v
    else
        "$c_linker" --version
    fi
    echo "c_sysroot=$rust_sysroot"
} > "$build_dir/toolchain-report.txt"

echo "GBA-010 report: $build_dir/toolchain-report.txt"

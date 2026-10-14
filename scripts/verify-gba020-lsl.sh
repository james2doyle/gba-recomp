#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

if [ "$#" -ne 2 ]; then
    echo "usage: verify-gba020-lsl.sh <jobs: 1|2|4|5|7|8|9|10> <lld-threads: 1..10>" >&2
    exit 2
fi
gba020_jobs=$1
gba020_link_threads=$2
case "$gba020_jobs" in
    1|2|4|5|7|8|9|10) ;;
    *)
        echo "GBA020_JOBS must be 1, 2, 4, 5, 7, 8, 9, or 10" >&2
        exit 2
        ;;
esac
case "$gba020_link_threads" in
    1|2|3|4|5|6|7|8|9|10) ;;
    *)
        echo "GBA020_LINK_THREADS must be an integer from 1 to 10" >&2
        exit 2
        ;;
esac
if [ "$((gba020_jobs * gba020_link_threads))" -gt 10 ]; then
    echo "GBA020_JOBS times GBA020_LINK_THREADS cannot exceed 10" >&2
    exit 2
fi

: "${LLVM_SYS_231_PREFIX:?set LLVM_SYS_231_PREFIX to LLVM 23.1.3}"
if [ "$("$LLVM_SYS_231_PREFIX/bin/llvm-config" --version)" != "23.1.3" ]; then
    echo "LLVM 23.1.3 is required" >&2
    exit 2
fi
recomp_bin="$root/target/debug/gba-recomp"
if [ ! -x "$recomp_bin" ]; then
    echo "built gba-recomp binary is missing: $recomp_bin" >&2
    exit 2
fi
group_timing_start_ns=$(python3 -c 'import time; print(time.monotonic_ns())')

verify_thumb_lsl() {
    mode=$1
    expected=$2
    steps=$3
    rom="$root/target/gba020/$mode.gba"
    python3 fixtures/tiny-counter/make_rom.py --out "$rom" --mode "$mode" >/dev/null
    for optimization in O0 O2; do
        package="$root/target/gba020/$mode-package-$optimization"
        rm -rf "$package"
        GBA_RECOMP_LINK_THREADS=$gba020_link_threads "$recomp_bin" compile \
            --rom "$rom" --out "$package" --opt "$optimization" --steps "$steps"
        actual=$("$package/game")
        if [ "$actual" != "$expected" ]; then
            echo "$optimization $mode fixture produced an unexpected state: $actual" >&2
            return 1
        fi
        python3 - "$package/manifest.json" "$optimization" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["execution_mode"] == "strict-aot"
assert manifest["optimization"] == sys.argv[2]
assert manifest["abi_version"] == 4
PY
    done
}

active_pids=
active_modes=
active_count=0
wait_batch() {
    failed=0
    for pid in $active_pids; do
        if ! wait "$pid"; then
            failed=1
        fi
    done
    if [ "$failed" -ne 0 ]; then
        for mode in $active_modes; do
            cat "$root/target/gba020/$mode-worker.log" >&2
        done
        return 1
    fi
    for mode in $active_modes; do
        cat "$root/target/gba020/$mode-worker.log"
        rm -f "$root/target/gba020/$mode-worker.log"
    done
    active_pids=
    active_modes=
    active_count=0
}
schedule_thumb_lsl() {
    mode=$1
    expected=$2
    steps=$3
    if [ "$gba020_jobs" -eq 1 ]; then
        verify_thumb_lsl "$mode" "$expected" "$steps"
        return
    fi
    log="$root/target/gba020/$mode-worker.log"
    (verify_thumb_lsl "$mode" "$expected" "$steps") >"$log" 2>&1 &
    active_pids="$active_pids $!"
    active_modes="$active_modes $mode"
    active_count=$((active_count + 1))
    if [ "$active_count" -ge "$gba020_jobs" ]; then
        wait_batch
    fi
}

schedule_thumb_lsl thumb-lsl-zero-carry-clear '{"status":"budget_exhausted","steps":8,"r0":0,"r1":2147483648,"pc":"0x08000086","cpsr":"0x8000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
schedule_thumb_lsl thumb-lsl-zero-carry-set '{"status":"budget_exhausted","steps":8,"r0":2147483648,"r1":0,"pc":"0x08000086","cpsr":"0x7000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
schedule_thumb_lsl thumb-lsl-one-carry '{"status":"budget_exhausted","steps":8,"r0":0,"r1":2,"pc":"0x08000086","cpsr":"0x2000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
schedule_thumb_lsl thumb-lsl-nineteen '{"status":"budget_exhausted","steps":8,"r0":2147483648,"r1":2147483648,"pc":"0x08000086","cpsr":"0x9000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
schedule_thumb_lsl thumb-lsl-thirtyone '{"status":"budget_exhausted","steps":8,"r0":0,"r1":0,"pc":"0x08000086","cpsr":"0x6000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
schedule_thumb_lsl thumb-lsl-nonalias '{"status":"budget_exhausted","steps":8,"r0":2147483648,"r1":4096,"pc":"0x08000086","cpsr":"0x9000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
schedule_thumb_lsl thumb-lsl-bx '{"status":"budget_exhausted","steps":6,"r0":67108980,"r1":134217960,"pc":"0x080000E8","cpsr":"0x0000001F","guest_cycle":6,"ewram_word0":0,"iwram_word0":0}' 6
schedule_thumb_lsl thumb-lsr-zero-carry-clear '{"status":"budget_exhausted","steps":8,"r0":2147483648,"r1":0,"pc":"0x08000086","cpsr":"0x5000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
schedule_thumb_lsl thumb-lsr-zero-carry-set '{"status":"budget_exhausted","steps":8,"r0":2147483648,"r1":0,"pc":"0x08000086","cpsr":"0x7000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
schedule_thumb_lsl thumb-lsr-one-carry '{"status":"budget_exhausted","steps":8,"r0":2147483648,"r1":1073741824,"pc":"0x08000086","cpsr":"0x3000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
schedule_thumb_lsl thumb-lsr-sixteen '{"status":"budget_exhausted","steps":8,"r0":4660,"r1":305419896,"pc":"0x08000086","cpsr":"0x1000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
schedule_thumb_lsl thumb-lsr-thirtyone '{"status":"budget_exhausted","steps":8,"r0":2147483648,"r1":1,"pc":"0x08000086","cpsr":"0x1000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
schedule_thumb_lsl thumb-lsr-alias '{"status":"budget_exhausted","steps":8,"r0":2147483648,"r1":4660,"pc":"0x08000086","cpsr":"0x1000003F","guest_cycle":8,"ewram_word0":0,"iwram_word0":0}' 8
if [ "$active_count" -gt 0 ]; then
    wait_batch
fi

python3 - "$group_timing_start_ns" "$gba020_jobs" "$gba020_link_threads" <<'PY'
import sys
import time

start_ns, jobs, link_threads = sys.argv[1:]
elapsed_ms = (time.monotonic_ns() - int(start_ns)) / 1_000_000
print(
    f"GBA020_GROUP_TIMING group=thumb-lsl-lsr jobs={jobs} "
    f"link_threads={link_threads} wall_ms={elapsed_ms:.1f}"
)
PY
echo "GBA-020 LSL/LSR fixtures passed: jobs=$gba020_jobs LLD=$gba020_link_threads"

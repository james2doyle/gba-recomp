#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$root"
if [ "$#" -ne 2 ]; then
    echo "usage: gba020-thumb-byte-benchmark.sh <jobs: 1|10> <lld-threads: 1|10>" >&2
    exit 2
fi
gba020_jobs=$1
gba020_link_threads=$2
case "$gba020_jobs:$gba020_link_threads" in
    1:10|10:1) ;;
    *) echo "benchmark accepts only 1x10 and 10x1 on the ten-CPU reference host" >&2; exit 2 ;;
esac
recomp_bin="$root/target/debug/gba-recomp"
if [ ! -x "$recomp_bin" ]; then
    echo "built gba-recomp binary is missing: $recomp_bin" >&2
    exit 2
fi
begin_group_timing() {
    group_timing_name=$1
    group_timing_start_ns=$(python3 -c 'import time; print(time.monotonic_ns())')
}
end_group_timing() {
    python3 - "$group_timing_name" "$group_timing_start_ns" "$gba020_jobs" "$gba020_link_threads" <<'PY'
import sys
import time

group, start_ns, jobs, link_threads = sys.argv[1:]
elapsed_ms = (time.monotonic_ns() - int(start_ns)) / 1_000_000
print(
    f"GBA020_GROUP_TIMING group={group} jobs={jobs} "
    f"link_threads={link_threads} wall_ms={elapsed_ms:.1f}"
)
PY
}

verify_thumb_strb() {
    mode=$1
    expected=$2
    rom="$root/target/gba020/$mode.gba"
    python3 fixtures/tiny-counter/make_rom.py --out "$rom" --mode "$mode"
    for optimization in O0 O2; do
        package="$root/target/gba020/$mode-package-$optimization"
        rm -rf "$package"
        GBA_RECOMP_LINK_THREADS=$gba020_link_threads "$recomp_bin" compile \
            --rom "$rom" --out "$package" --opt "$optimization" --steps 12
        actual=$("$package/game")
        if [ "$actual" != "$expected" ]; then
            echo "$optimization $mode Thumb STRB fixture produced an unexpected state: $actual" >&2
            exit 1
        fi
        python3 - "$package/manifest.json" "$optimization" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["execution_mode"] == "strict-aot"
assert manifest["abi_version"] == 4
assert manifest["machine_state_version"] == 1
assert manifest["bios_hle_version"] == 1
assert manifest["instructions_aot"] == 14
assert manifest["optimization"] == sys.argv[2]
assert manifest["run_instruction_budget"] == 12
PY
    done
}

verify_thumb_ldrb() {
    mode=$1
    expected=$2
    rom="$root/target/gba020/$mode.gba"
    python3 fixtures/tiny-counter/make_rom.py --out "$rom" --mode "$mode"
    for optimization in O0 O2; do
        package="$root/target/gba020/$mode-package-$optimization"
        rm -rf "$package"
        GBA_RECOMP_LINK_THREADS=$gba020_link_threads "$recomp_bin" compile \
            --rom "$rom" --out "$package" --opt "$optimization" --steps 12
        actual=$("$package/game")
        if [ "$actual" != "$expected" ]; then
            echo "$optimization $mode Thumb LDRB fixture produced an unexpected state: $actual" >&2
            exit 1
        fi
        python3 - "$package/manifest.json" "$optimization" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest["execution_mode"] == "strict-aot"
assert manifest["abi_version"] == 4
assert manifest["machine_state_version"] == 1
assert manifest["bios_hle_version"] == 1
assert manifest["instructions_aot"] == 14
assert manifest["optimization"] == sys.argv[2]
assert manifest["run_instruction_budget"] == 12
PY
    done
}

thumb_byte_active_pids=
thumb_byte_active_modes=
thumb_byte_active_count=0
wait_thumb_byte_batch() {
    failed=0
    for pid in $thumb_byte_active_pids; do
        if ! wait "$pid"; then
            failed=1
        fi
    done
    if [ "$failed" -ne 0 ]; then
        for mode in $thumb_byte_active_modes; do
            cat "$root/target/gba020/$mode-byte-worker.log" >&2
        done
        return 1
    fi
    for mode in $thumb_byte_active_modes; do
        cat "$root/target/gba020/$mode-byte-worker.log"
        rm -f "$root/target/gba020/$mode-byte-worker.log"
    done
    thumb_byte_active_pids=
    thumb_byte_active_modes=
    thumb_byte_active_count=0
}
schedule_thumb_byte_case() {
    mode=$1
    kind=$2
    expected=$3
    if [ "$gba020_jobs" -eq 1 ]; then
        case "$kind" in
            STRB) verify_thumb_strb "$mode" "$expected" ;;
            LDRB) verify_thumb_ldrb "$mode" "$expected" ;;
        esac
        return
    fi
    log="$root/target/gba020/$mode-byte-worker.log"
    rm -f "$log"
    (
        case "$kind" in
            STRB) verify_thumb_strb "$mode" "$expected" ;;
            LDRB) verify_thumb_ldrb "$mode" "$expected" ;;
        esac
    ) >"$log" 2>&1 &
    thumb_byte_active_pids="$thumb_byte_active_pids $!"
    thumb_byte_active_modes="$thumb_byte_active_modes $mode"
    thumb_byte_active_count=$((thumb_byte_active_count + 1))
    if [ "$thumb_byte_active_count" -ge "$gba020_jobs" ]; then
        wait_thumb_byte_batch
    fi
}

begin_group_timing thumb-byte-load-store
schedule_thumb_byte_case thumb-strb-ewram-zero-even STRB \
    '{"status":"budget_exhausted","steps":12,"r0":2864434397,"r1":33554432,"pc":"0x08000086","cpsr":"0x6000003F","guest_cycle":12,"ewram_word0":1144201949,"iwram_word0":0}'
schedule_thumb_byte_case thumb-strb-ewram-thirtyone-odd STRB \
    '{"status":"budget_exhausted","steps":12,"r0":305419896,"r1":33554402,"pc":"0x08000086","cpsr":"0x6000003F","guest_cycle":12,"ewram_word0":1144223761,"iwram_word0":0}'
schedule_thumb_byte_case thumb-strb-iwram-zero-odd STRB \
    '{"status":"budget_exhausted","steps":12,"r0":52719,"r1":50331649,"pc":"0x08000086","cpsr":"0x6000003F","guest_cycle":12,"ewram_word0":0,"iwram_word0":1144254225}'
schedule_thumb_byte_case thumb-strb-iwram-thirtyone-even STRB \
    '{"status":"budget_exhausted","steps":12,"r0":190,"r1":50331617,"pc":"0x08000086","cpsr":"0x6000003F","guest_cycle":12,"ewram_word0":0,"iwram_word0":1144201918}'
schedule_thumb_byte_case thumb-strb-ewram-thirtyone-even-alias STRB \
    '{"status":"budget_exhausted","steps":12,"r0":0,"r1":33554401,"pc":"0x08000086","cpsr":"0x6000003F","guest_cycle":12,"ewram_word0":1144201953,"iwram_word0":0}'

schedule_thumb_byte_case thumb-ldrb-ewram-zero-even-80 LDRB \
    '{"status":"budget_exhausted","steps":12,"r0":128,"r1":33554434,"pc":"0x08000086","cpsr":"0x6000003F","guest_cycle":12,"ewram_word0":4286611200,"iwram_word0":0}'
schedule_thumb_byte_case thumb-ldrb-ewram-thirtyone-odd-ff LDRB \
    '{"status":"budget_exhausted","steps":12,"r0":255,"r1":33554404,"pc":"0x08000086","cpsr":"0x6000003F","guest_cycle":12,"ewram_word0":4286611200,"iwram_word0":0}'
schedule_thumb_byte_case thumb-ldrb-iwram-zero-odd-7f LDRB \
    '{"status":"budget_exhausted","steps":12,"r0":127,"r1":50331649,"pc":"0x08000086","cpsr":"0x6000003F","guest_cycle":12,"ewram_word0":0,"iwram_word0":4286611200}'
schedule_thumb_byte_case thumb-ldrb-iwram-thirtyone-even-zero LDRB \
    '{"status":"budget_exhausted","steps":12,"r0":0,"r1":50331617,"pc":"0x08000086","cpsr":"0x6000003F","guest_cycle":12,"ewram_word0":0,"iwram_word0":4286611200}'
schedule_thumb_byte_case thumb-ldrb-ewram-thirtyone-alias-80 LDRB \
    '{"status":"budget_exhausted","steps":12,"r0":2864434397,"r1":128,"pc":"0x08000086","cpsr":"0x6000003F","guest_cycle":12,"ewram_word0":4286611200,"iwram_word0":0}'
if [ "$thumb_byte_active_count" -gt 0 ]; then
    wait_thumb_byte_batch
fi
end_group_timing

#!/bin/sh
# Frame differential gba-core (gba-run, release) vs mGBA oracle (SPEC-011 VAL-03).
#
# usage: scripts/diff-mgba.sh --rom R [--frames N] [--every N] [--window K]
#                             [--out DIR] [--json FILE] [--mask] [--input FRAME:LEN:KEYS ...]
#
# Both runners get the same ROM, frame count and inputs, HLE BIOS, no save file.
# PPMs (ROM-derived, never commit them) go to DIR (default: a fresh mktemp dir).
# Requires: scripts/build-mgba-oracle.sh already run (or MGBA_ORACLE set), cargo on PATH.
set -eu

ROOT=$(CDPATH= command cd -- "$(dirname -- "$0")/.." && pwd)
COMMON=$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)
MAIN=$(dirname -- "$COMMON")
ORACLE=${MGBA_ORACLE:-$MAIN/target/mgba-build/mgba-oracle}

ROM= FRAMES=3000 EVERY=100 WINDOW=10 OUT= JSON= MASK=
INPUTS=
while [ $# -gt 0 ]; do
    case "$1" in
        --rom) ROM=$2; shift 2 ;;
        --frames) FRAMES=$2; shift 2 ;;
        --every) EVERY=$2; shift 2 ;;
        --window) WINDOW=$2; shift 2 ;;
        --out) OUT=$2; shift 2 ;;
        --json) JSON=$2; shift 2 ;;
        --mask) MASK=1; shift ;;
        --input) INPUTS="$INPUTS --input $2"; shift 2 ;;
        *) echo "diff-mgba: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -n "$ROM" ] || { echo "diff-mgba: --rom is required" >&2; exit 2; }
[ -x "$ORACLE" ] || { echo "diff-mgba: oracle not built; run scripts/build-mgba-oracle.sh" >&2; exit 1; }
[ -n "$OUT" ] || OUT=$(mktemp -d "${TMPDIR:-/tmp}/diff-mgba.XXXXXX")
mkdir -p "$OUT/ours" "$OUT/mgba"

(cd "$ROOT"; cargo build --release -q -p gba-run)
RUN="$ROOT/target/release/gba-run"

echo "rom sha256: $(shasum -a 256 "$ROM" | cut -d' ' -f1)"
# shellcheck disable=SC2086 # INPUTS is a deliberately split list of flags.
"$RUN" --rom "$ROM" --frames "$FRAMES" $INPUTS --ppm-every "$EVERY" \
    --ppm "$OUT/ours/f.ppm" --hashes >"$OUT/ours.json" 2>"$OUT/ours.hashes"
# shellcheck disable=SC2086
"$ORACLE" --rom "$ROM" --frames "$FRAMES" $INPUTS --ppm-every "$EVERY" --ppm-window "$WINDOW" \
    --ppm "$OUT/mgba/f.ppm" --hashes >"$OUT/mgba.json" 2>"$OUT/mgba.hashes"
echo "ours: $(cat "$OUT/ours.json")"
echo "mgba: $(cat "$OUT/mgba.json")"

set -- --ours "$OUT/ours/f" --ref "$OUT/mgba/f" --every "$EVERY" --last "$FRAMES" --window "$WINDOW" \
    --ours-hashes "$OUT/ours.hashes" --ref-hashes "$OUT/mgba.hashes"
[ -n "$JSON" ] && set -- "$@" --json "$JSON"
[ -n "$MASK" ] && set -- "$@" --mask-dir "$OUT/mask"
python3 -I "$ROOT/scripts/diff-frames.py" "$@"
echo "frames in $OUT"

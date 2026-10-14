#!/bin/sh
# Builds a minimal static libmgba out-of-tree and links tools/mgba-oracle against it.
# mGBA is an external test oracle (SPEC-011 VAL-03, ADR-041 item 4). No mGBA code is
# copied into product crates; vendor-ref/ is read-only (AGENTS.md rule 11).
#
# Env:
#   MGBA_SRC    mGBA source tree (default: <main checkout>/vendor-ref/mgba)
#   MGBA_BUILD  build dir (default: <main checkout>/target/mgba-build)
#   JOBS        parallel make jobs (default 10)
#
# libmgba is compiled with -DCOLOR_16_BIT (mColor = raw BGR555, no 5:6:5) so the
# software renderer blends in 5-bit precision like hardware and like gba-core;
# the oracle then expands BGR555 exactly as gba-run does: (v<<3)|(v>>2).
set -eu

ROOT=$(CDPATH= command cd -- "$(dirname -- "$0")/.." && pwd)
# vendor-ref/ and target/ live in the main checkout, also when run from a worktree.
COMMON=$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)
MAIN=$(dirname -- "$COMMON")
MGBA_SRC=${MGBA_SRC:-$MAIN/vendor-ref/mgba}
MGBA_BUILD=${MGBA_BUILD:-$MAIN/target/mgba-build}
JOBS=${JOBS:-10}
CMAKE=${CMAKE:-cmake}
command -v "$CMAKE" >/dev/null 2>&1 || CMAKE=/opt/homebrew/bin/cmake

if [ ! -f "$MGBA_SRC/version.cmake" ]; then
    echo "build-mgba-oracle: mGBA source not found at $MGBA_SRC" >&2
    exit 1
fi

"$CMAKE" -S "$MGBA_SRC" -B "$MGBA_BUILD" -G "Unix Makefiles" \
    -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_STATIC=ON -DBUILD_SHARED=OFF \
    -DBUILD_QT=OFF -DBUILD_SDL=OFF -DBUILD_LIBRETRO=OFF -DBUILD_HEADLESS=OFF \
    -DBUILD_PERF=OFF -DBUILD_TEST=OFF -DBUILD_SUITE=OFF -DBUILD_CINEMA=OFF \
    -DBUILD_EXAMPLE=OFF -DBUILD_PYTHON=OFF -DBUILD_DOCGEN=OFF \
    -DBUILD_GL=OFF -DBUILD_GLES2=OFF -DBUILD_GLES3=OFF -DBUILD_LTO=OFF \
    -DENABLE_DEBUGGERS=OFF -DENABLE_GDB_STUB=OFF -DUSE_EDITLINE=OFF \
    -DENABLE_SCRIPTING=OFF -DUSE_LUA=OFF -DUSE_JSON_C=OFF \
    -DUSE_FFMPEG=OFF -DUSE_ZLIB=OFF -DUSE_MINIZIP=OFF -DUSE_PNG=OFF -DUSE_LIBZIP=OFF \
    -DUSE_SQLITE3=OFF -DUSE_ELF=OFF -DUSE_LZMA=OFF -DUSE_DISCORD_RPC=OFF \
    -DUSE_FREETYPE=OFF -DUSE_EPOXY=OFF \
    -DM_CORE_GBA=ON -DM_CORE_GB=OFF \
    -DCOLOR_16_BIT=ON -DCOLOR_5_6_5=OFF -DCMAKE_C_FLAGS=-DCOLOR_16_BIT \
    -DSKIP_GIT=ON >/dev/null
make -C "$MGBA_BUILD" -j"$JOBS" mgba >/dev/null

LIB=$(find "$MGBA_BUILD" -maxdepth 1 -name 'libmgba*.a' | head -n 1)
if [ -z "$LIB" ]; then
    echo "build-mgba-oracle: static libmgba not produced in $MGBA_BUILD" >&2
    exit 1
fi

CC=${CC:-cc}
EXTRA_LIBS=
[ "$(uname -s)" = Darwin ] && EXTRA_LIBS="-framework CoreFoundation"
# Public structs (struct mCore etc.) have #ifdef'd members, so the tool must be
# compiled with the same feature defines libmgba was built with. COLOR_16_BIT
# reaches libmgba's sources only via CMAKE_C_FLAGS (generated flags.h is for
# installed consumers), so it is repeated here explicitly.
DEFINES=$(sed -n 's/^C_DEFINES = //p' "$MGBA_BUILD/CMakeFiles/mgba.dir/flags.make")
OUT="$MGBA_BUILD/mgba-oracle"
# shellcheck disable=SC2086 # DEFINES is a deliberately split list of -D flags.
"$CC" -O2 -std=c11 -Wall -Wextra -DCOLOR_16_BIT $DEFINES \
    -I"$MGBA_BUILD/include" -I"$MGBA_SRC/include" \
    -o "$OUT" "$ROOT/tools/mgba-oracle/mgba_oracle.c" "$LIB" \
    -lm -lpthread $EXTRA_LIBS
echo "$OUT"

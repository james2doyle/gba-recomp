#!/usr/bin/env bash
# Package the gba-recomp toolchain and the recompiled free demo game for one
# platform (SPEC-068 CI-68-01). Usage: package.sh <platform-name>
set -euo pipefail
name="$1"
exe=""; case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) exe=".exe";; esac
dist=dist; mkdir -p "$dist"

# 1. Toolchain: CLI, runners, companion apps and the runtime library.
tool="$dist/gba-recomp-$name"
mkdir -p "$tool"
for bin in gba-recomp gba-run gba-desktop gba-settings gba-party; do
  cp "target/release/$bin$exe" "$tool/"
done
cp target/release/libgba_native_rt.a "$tool/" 2>/dev/null || cp target/release/gba_native_rt.lib "$tool/"
cp README.md "$tool/"
cat > "$tool/INSTALL.txt" <<TXT
gba-recomp toolchain ($name)
Requires LLVM 23.1.3 (set LLVM_SYS_231_PREFIX) and SDL 3.4 (set SDL3_LIB_DIR).
Example: ./gba-recomp bundle --rom game.gba --out out/Game
TXT

# 2. Demo: recompile every free game with the ROM embedded, verify, archive.
inputs=(--profile-input 200:5:START --profile-input 300:5:A --profile-input 400:60:LEFT --profile-input 500:60:RIGHT)
for rom in games/*.gba; do
  game=$(basename "$rom" .gba)
  out="$dist/$game-$name"
  "target/release/gba-recomp$exe" bundle --rom "$rom" --out "$out" "${inputs[@]}"
  if [ -d "$out.app" ]; then
    game_bin="$out.app/Contents/MacOS/game"
  else
    game_bin="$out/game$exe"
  fi
  "$game_bin" --headless --frames 1200 --input 200:5:START --input 300:5:A \
    --input 400:60:LEFT --input 500:60:RIGHT --verify-interpreter
done

# 3. Archives for the release.
cd "$dist"
for item in *; do
  case "$item" in *.zip|*.tar.gz) continue;; esac
  case "$(uname -s)" in
    Darwin) ditto -c -k --keepParent "$item" "$item.zip" ;;
    MINGW*|MSYS*|CYGWIN*) 7z a -tzip -bd "$item.zip" "$item" > /dev/null ;;
    *) tar -czf "$item.tar.gz" "$item" ;;
  esac
done
ls -la

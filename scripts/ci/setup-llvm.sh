#!/usr/bin/env bash
# Download the pinned LLVM 23.1.3 release for this runner (SPEC-068 CI-68-01)
# and export LLVM_SYS_231_PREFIX. The archive is streamed into tar and only
# what gba-recomp needs is extracted (the full Linux tree exceeds 10 GB).
# Hashes come from toolchain.lock.toml when pinned there; other assets
# print their hash so it can be locked.
set -euo pipefail
asset="$1"
version=23.1.3
dest="${RUNNER_TEMP:-/tmp}/llvm"
url="https://github.com/llvm/llvm-project/releases/download/llvmorg-${version}/${asset}"
mkdir -p "$dest"

if [ "$(uname -s)" = "Linux" ] && [ -n "${GITHUB_ACTIONS:-}" ]; then
  # Hosted runners keep ~20 GB of SDKs we never use.
  # KEEP_ANDROID_SDK=1 keeps the preinstalled Android SDK/NDK.
  android=/usr/local/lib/android
  [ "${KEEP_ANDROID_SDK:-}" = "1" ] && android=
  sudo rm -rf /usr/share/dotnet $android /opt/ghc /usr/local/share/boost \
    /usr/local/.ghcup /opt/hostedtoolcache/CodeQL || true
fi

config=$(find "$dest" -maxdepth 3 -path '*/bin/llvm-config*' -print -quit 2>/dev/null || true)
if [ -z "$config" ]; then
  echo "downloading $url"
  patterns=(
    '*/bin/llvm-config' '*/bin/llvm-config.exe'
    '*/bin/clang' '*/bin/clang-[0-9]*' '*/bin/clang.exe' '*/bin/clang-cl.exe'
    '*/bin/lld' '*/bin/ld.lld' '*/bin/ld64.lld' '*/bin/lld-link' '*/bin/wasm-ld'
    '*/bin/lld.exe' '*/bin/ld.lld.exe' '*/bin/lld-link.exe'
    '*/bin/llvm-otool' '*/bin/llvm-install-name-tool' '*/bin/llvm-objcopy' '*/bin/llvm-ar'
    '*/bin/llvm-objdump' '*/bin/llvm-ar.exe' '*/bin/llvm-objdump.exe'
    '*/lib/libLLVM*.a' '*/lib/LLVM*.lib'
    '*/lib/libLLVM.so*' '*/lib/libLLVM-*.so*' '*/lib/libclang-cpp.so*'
    '*/lib/libLLVM.dylib' '*/lib/libclang-cpp.dylib'
    '*/lib/libc++*' '*/lib/libunwind*'
    '*/lib/clang/*'
    '*/include/llvm-c/*'
  )
  # GNU tar (Linux, Windows Git Bash): extract only the patterns above.
  # bsdtar (macOS runners, plenty of disk): extract everything.
  if tar --version 2>/dev/null | grep -q 'GNU tar'; then
    selection=(--wildcards "${patterns[@]}")
  else
    selection=()
  fi
  sum_file="$dest/sha256"
  case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    # Git Bash's GNU tar cannot stream xz reliably; the runner's D: drive has
    # room, so download, hash and extract with Windows' own bsdtar.
    curl -fsSL --retry 3 -o "$dest/$asset" "$url"
    sha256sum "$dest/$asset" | cut -d' ' -f1 > "$sum_file"
    /c/Windows/System32/tar.exe -xf "$(cygpath -w "$dest/$asset")" -C "$(cygpath -w "$dest")"
    rm -f "$dest/$asset"
    ;;
  *)
  curl -fsSL --retry 3 "$url" \
    | tee >( (sha256sum 2>/dev/null || shasum -a 256) | cut -d' ' -f1 > "$sum_file") \
    | tar -xJf - -C "$dest" ${selection[@]+"${selection[@]}"} 2>/dev/null || {
      # Some patterns have no match on a given platform; tar then exits 2
      # after extracting everything else. Fail only if llvm-config is missing.
      true
    }
    ;;
  esac
  sleep 1
  actual=$(cat "$sum_file" 2>/dev/null || true)
  expected=$( (grep -A3 "asset = \"$asset\"" toolchain.lock.toml || true) | sed -n 's/^sha256 = "\(.*\)"/\1/p' | head -1)
  if [ -n "$expected" ]; then
    [ "$actual" = "$expected" ] || { echo "sha256 mismatch for $asset: $actual != $expected"; exit 1; }
  else
    echo "::notice::unpinned LLVM asset $asset sha256=$actual (add it to toolchain.lock.toml)"
  fi
  config=$(find "$dest" -maxdepth 3 -path '*/bin/llvm-config*' -print -quit)
  [ -n "$config" ] || { echo "llvm-config was not extracted from $asset"; exit 1; }
fi
prefix=$(cd "$(dirname "$config")/.." && pwd)
"$config" --version
du -sh "$prefix" || true
echo "LLVM_SYS_231_PREFIX=$prefix" >> "$GITHUB_ENV"
echo "$prefix/bin" >> "$GITHUB_PATH"

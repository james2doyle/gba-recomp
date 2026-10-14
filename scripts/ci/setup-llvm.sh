#!/usr/bin/env bash
# Download the pinned LLVM 23.1.3 release for this runner (SPEC-068 CI-68-01)
# and export LLVM_SYS_231_PREFIX. Hashes come from toolchain.lock.toml when
# pinned there; other assets print their hash so it can be locked.
set -euo pipefail
asset="$1"
version=23.1.3
dest="${RUNNER_TEMP:-/tmp}/llvm"
url="https://github.com/llvm/llvm-project/releases/download/llvmorg-${version}/${asset}"
mkdir -p "$dest"
if [ ! -x "$(find "$dest" -maxdepth 3 -path '*/bin/llvm-config*' -print -quit 2>/dev/null || true)" ]; then
  echo "downloading $url"
  curl -fsSL --retry 3 -o "$dest/$asset" "$url"
  actual=$( (sha256sum "$dest/$asset" 2>/dev/null || shasum -a 256 "$dest/$asset") | cut -d' ' -f1)
  expected=$(grep -A3 "asset = \"$asset\"" toolchain.lock.toml | sed -n 's/^sha256 = "\(.*\)"/\1/p' | head -1)
  if [ -n "$expected" ]; then
    [ "$actual" = "$expected" ] || { echo "sha256 mismatch for $asset: $actual != $expected"; exit 1; }
  else
    echo "::notice::unpinned LLVM asset $asset sha256=$actual (add it to toolchain.lock.toml)"
  fi
  tar -xf "$dest/$asset" -C "$dest"
  rm -f "$dest/$asset"
fi
config=$(find "$dest" -maxdepth 3 -path '*/bin/llvm-config*' -print -quit)
prefix=$(cd "$(dirname "$config")/.." && pwd)
"$config" --version
echo "LLVM_SYS_231_PREFIX=$prefix" >> "$GITHUB_ENV"
echo "$prefix/bin" >> "$GITHUB_PATH"

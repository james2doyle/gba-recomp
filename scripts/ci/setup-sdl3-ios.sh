#!/usr/bin/env bash
# Build SDL 3.4.12 as static libraries for iOS device (arm64, iphoneos) and
# the iOS simulator (arm64, iphonesimulator) (SPEC-068 TGT-68-02).
#
# usage: scripts/ci/setup-sdl3-ios.sh [dest]
#   dest defaults to $RUNNER_TEMP/sdl3-ios (or /tmp/sdl3-ios). Produces
#   <dest>/iphoneos/lib/libSDL3.a and <dest>/iphonesimulator/lib/libSDL3.a.
# On GitHub Actions it exports SDL3_IOS_DIR and SDL3_IOS_SIM_DIR (the lib
# directories, ready for `gba-recomp bundle --sdl-dir`).
set -euo pipefail
version=3.4.12
dest="${1:-${RUNNER_TEMP:-/tmp}/sdl3-ios}"
mkdir -p "$dest"
dest=$(cd "$dest" && pwd)
src="$dest/src"
if [ ! -f "$src/CMakeLists.txt" ]; then
  git clone --depth 1 --branch "release-$version" https://github.com/libsdl-org/SDL.git "$src"
fi
generator=()
if command -v ninja >/dev/null 2>&1; then generator=(-G Ninja); fi
for sdk in iphoneos iphonesimulator; do
  build="$dest/build-$sdk"
  cmake -S "$src" -B "$build" ${generator[@]+"${generator[@]}"} \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_SYSTEM_NAME=iOS \
    -DCMAKE_OSX_SYSROOT="$sdk" \
    -DCMAKE_OSX_ARCHITECTURES=arm64 \
    -DCMAKE_OSX_DEPLOYMENT_TARGET=14.0 \
    -DSDL_SHARED=OFF -DSDL_STATIC=ON \
    -DSDL_TESTS=OFF -DSDL_EXAMPLES=OFF -DSDL_TEST_LIBRARY=OFF
  cmake --build "$build" --config Release --parallel
  cmake --install "$build" --config Release --prefix "$dest/$sdk"
  test -f "$dest/$sdk/lib/libSDL3.a"
done
echo "SDL3 $version for iOS: $dest/iphoneos/lib, $dest/iphonesimulator/lib"
if [ -n "${GITHUB_ENV:-}" ]; then
  echo "SDL3_IOS_DIR=$dest/iphoneos/lib" >> "$GITHUB_ENV"
  echo "SDL3_IOS_SIM_DIR=$dest/iphonesimulator/lib" >> "$GITHUB_ENV"
fi

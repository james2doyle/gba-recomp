#!/usr/bin/env bash
# Build SDL 3.4.12 for Android arm64-v8a with the NDK (SPEC-068 TGT-68-02).
#
# Usage: scripts/ci/setup-sdl3-android.sh [dest]
#   dest defaults to $RUNNER_TEMP/sdl3-android (or /tmp/sdl3-android).
# Needs ANDROID_NDK_HOME or ANDROID_NDK_LATEST_HOME (or ANDROID_NDK_ROOT),
# cmake and git. Layout produced (outside the repository):
#   <dest>/SDL/                  SDL source (android-project Java sources)
#   <dest>/SDL/build-android/    libSDL3.so for arm64-v8a  -> pass as --sdl-dir
# Exports SDL3_ANDROID_DIR and SDL3_ANDROID_SOURCE to $GITHUB_ENV on CI.
set -euo pipefail
version=3.4.12
dest="${1:-${RUNNER_TEMP:-/tmp}/sdl3-android}"
ndk="${ANDROID_NDK_HOME:-${ANDROID_NDK_LATEST_HOME:-${ANDROID_NDK_ROOT:-}}}"
if [ -z "$ndk" ] || [ ! -f "$ndk/build/cmake/android.toolchain.cmake" ]; then
  echo "Android NDK not found: set ANDROID_NDK_HOME (or ANDROID_NDK_LATEST_HOME)" >&2
  exit 1
fi
src="$dest/SDL"
build="$src/build-android"
mkdir -p "$dest"
if [ ! -f "$src/CMakeLists.txt" ]; then
  git clone --depth 1 --branch "release-$version" https://github.com/libsdl-org/SDL.git "$src"
fi
generator=(-G "Unix Makefiles")
if command -v ninja >/dev/null 2>&1; then
  generator=(-G Ninja)
fi
cmake -S "$src" -B "$build" "${generator[@]}" \
  -DCMAKE_TOOLCHAIN_FILE="$ndk/build/cmake/android.toolchain.cmake" \
  -DANDROID_ABI=arm64-v8a \
  -DANDROID_PLATFORM=24 \
  -DCMAKE_BUILD_TYPE=Release \
  -DSDL_SHARED=ON -DSDL_STATIC=OFF \
  -DSDL_TESTS=OFF -DSDL_EXAMPLES=OFF
cmake --build "$build" --parallel
test -f "$build/libSDL3.so"
test -f "$src/android-project/app/src/main/java/org/libsdl/app/SDLActivity.java"
echo "SDL3 for Android: $build/libSDL3.so (Java sources in $src/android-project)"
if [ -n "${GITHUB_ENV:-}" ]; then
  echo "SDL3_ANDROID_DIR=$build" >> "$GITHUB_ENV"
  echo "SDL3_ANDROID_SOURCE=$src" >> "$GITHUB_ENV"
fi

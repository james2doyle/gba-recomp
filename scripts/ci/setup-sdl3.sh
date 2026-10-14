#!/usr/bin/env bash
# Install SDL 3.4.12 for the runner and export SDL3_LIB_DIR (SPEC-068).
set -euo pipefail
version=3.4.12
case "$(uname -s)" in
  Linux)
    sudo apt-get update -qq
    sudo apt-get install -y -qq cmake ninja-build pkg-config patchelf \
      libasound2-dev libpulse-dev libx11-dev libxext-dev libxrandr-dev libxcursor-dev \
      libxi-dev libxss-dev libxkbcommon-dev libwayland-dev wayland-protocols \
      libgl1-mesa-dev libegl1-mesa-dev libdbus-1-dev libudev-dev
    src="${RUNNER_TEMP:-/tmp}/SDL"
    git clone --depth 1 --branch "release-$version" https://github.com/libsdl-org/SDL.git "$src"
    cmake -S "$src" -B "$src/build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DSDL_TESTS=OFF -DSDL_EXAMPLES=OFF
    cmake --build "$src/build"
    sudo cmake --install "$src/build" --prefix /usr/local
    sudo ldconfig
    echo "SDL3_LIB_DIR=/usr/local/lib" >> "$GITHUB_ENV"
    ;;
  Darwin)
    brew install sdl3
    echo "SDL3_LIB_DIR=$(brew --prefix)/lib" >> "$GITHUB_ENV"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    dest="${RUNNER_TEMP:-/tmp}/sdl3"
    mkdir -p "$dest"
    curl -fsSL --retry 3 -o "$dest/sdl.zip" \
      "https://github.com/libsdl-org/SDL/releases/download/release-$version/SDL3-devel-$version-VC.zip"
    unzip -q "$dest/sdl.zip" -d "$dest"
    lib=$(cygpath -w "$dest/SDL3-$version/lib/x64")
    echo "SDL3_LIB_DIR=$lib" >> "$GITHUB_ENV"
    echo "$dest/SDL3-$version/lib/x64" >> "$GITHUB_PATH"
    ;;
esac

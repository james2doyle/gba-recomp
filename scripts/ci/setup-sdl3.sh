#!/usr/bin/env bash
# Install SDL 3.4.12 for the runner and export SDL3_LIB_DIR (SPEC-068).
set -euo pipefail
version=3.4.12
case "$(uname -s)" in
  Linux)
    sudo apt-get update -qq
    sudo apt-get install -y -qq cmake ninja-build pkg-config patchelf \
      libasound2-dev libpulse-dev libx11-dev libxext-dev libxrandr-dev libxcursor-dev \
      libxi-dev libxss-dev libxtst-dev libxfixes-dev libxkbcommon-dev libwayland-dev wayland-protocols \
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
    if [ "${SDL_ARCH:-}" = "x86_64" ]; then
      # Intel build on an Apple Silicon runner (cross target x86_64-apple-darwin).
      src="${RUNNER_TEMP:-/tmp}/SDL"
      prefix="${RUNNER_TEMP:-/tmp}/sdl3-x86_64"
      git clone --depth 1 --branch "release-$version" https://github.com/libsdl-org/SDL.git "$src"
      cmake -S "$src" -B "$src/build" -DCMAKE_BUILD_TYPE=Release -DCMAKE_OSX_ARCHITECTURES=x86_64 \
        -DCMAKE_OSX_DEPLOYMENT_TARGET=11.0 -DSDL_TESTS=OFF -DSDL_EXAMPLES=OFF -DCMAKE_INSTALL_PREFIX="$prefix"
      cmake --build "$src/build" --parallel
      cmake --install "$src/build"
      echo "SDL3_LIB_DIR=$prefix/lib" >> "$GITHUB_ENV"
      # Test binaries of the cross target load this libSDL3 under Rosetta.
      echo "DYLD_FALLBACK_LIBRARY_PATH=$prefix/lib" >> "$GITHUB_ENV"
    else
      brew install sdl3
      echo "SDL3_LIB_DIR=$(brew --prefix)/lib" >> "$GITHUB_ENV"
    fi
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

#!/bin/bash

# Exit on error
set -e

# Add log level selector with default to INFO
LOG_LEVEL=${1:-info}

case $LOG_LEVEL in
    info|debug|trace)
        export RUST_LOG=$LOG_LEVEL
        ;;
    *)
        echo "Invalid log level: $LOG_LEVEL. Valid options: info, debug, trace"
        exit 1
        ;;
esac

# Check and install CMake if needed
echo "Checking CMake version..."
if ! command -v cmake &> /dev/null; then
    echo "CMake not found. Installing via Homebrew..."
    brew install cmake
else
    CMAKE_VERSION=$(cmake --version | head -n1 | cut -d" " -f3)
    if [[ "$CMAKE_VERSION" < "3.5" ]]; then
        echo "CMake version $CMAKE_VERSION is too old. Updating via Homebrew..."
        brew upgrade cmake
    fi
fi

# Clean up previous builds
echo "Cleaning up previous builds..."
rm -rf target/
rm -rf src-tauri/target
rm -rf src-tauri/gen

# Clean up npm, pnp and next
echo "Cleaning up npm, pnp and next..."
rm -rf node_modules
rm -rf .next
rm -rf .pnp.cjs
rm -rf out

echo "Installing dependencies..."
pnpm install

# Build the Next.js application first
echo "Building Next.js application..."
pnpm run build

# Set environment variables for the build

# `tauri:build`, not `tauri build`. The npm script runs scripts/tauri-auto.js,
# which builds the llama-helper sidecar into
# src-tauri/binaries/llama-helper-<triple> first. Tauri resolves externalBin at
# build-script time and aborts with "resource path 'binaries/llama-helper-...'
# doesn't exist" when it is missing, so the bare form works only on a machine
# that happens to have built the sidecar before — which is every machine that
# has already built once, and no fresh clone. The same line was wrong in
# clean_run.sh and in both Windows batch files.
echo "Building Tauri app..."
pnpm run tauri:build


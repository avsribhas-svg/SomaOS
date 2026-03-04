#!/bin/bash
# SomaOS — Cross-compile Rust binaries for x86_64 Linux
# Requires: rustup target add x86_64-unknown-linux-gnu
#           Docker (for the linker, or a cross-compilation toolchain)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "╔══════════════════════════════════════╗"
echo "║   Cross-compiling SomaOS for x86_64  ║"
echo "╚══════════════════════════════════════╝"

cd "$PROJECT_DIR"

# Option 1: Use cargo cross (recommended)
if command -v cross &> /dev/null; then
    echo "Using 'cross' for cross-compilation..."
    cross build --release --target x86_64-unknown-linux-gnu
else
    echo "Tip: Install 'cross' for easy cross-compilation:"
    echo "  cargo install cross"
    echo ""
    echo "Attempting native cargo build for host target..."
    cargo build --release
fi

echo ""
echo "Build artifacts:"
ls -la "$PROJECT_DIR/target/release/soma-agent" 2>/dev/null || true
ls -la "$PROJECT_DIR/target/release/soma-compositor" 2>/dev/null || true
echo ""
echo "Done!"

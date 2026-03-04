#!/bin/bash
# SomaOS — Full Build Pipeline
# Builds the complete bootable OS image using Docker + Buildroot
#
# Usage: ./build.sh [--rust-only] [--image-only]
#   --rust-only   Only cross-compile Rust binaries
#   --image-only  Only build the Buildroot image (assumes binaries exist)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="$SCRIPT_DIR/output"

echo "╔══════════════════════════════════════════╗"
echo "║    SomaOS Build Pipeline                  ║"
echo "╚══════════════════════════════════════════╝"
echo ""

mkdir -p "$OUTPUT_DIR"

# ──────────────────────────────────────────────
# Step 1: Cross-compile Rust binaries
# ──────────────────────────────────────────────
if [ "$1" != "--image-only" ]; then
    echo "▸ Step 1: Cross-compiling Rust binaries for x86_64-linux..."
    echo ""

    # Use Docker to cross-compile (avoids needing a local cross toolchain)
    docker run --rm --platform linux/amd64 \
        -v "$PROJECT_DIR:/project" \
        -w /project \
        rust:latest \
        bash -c "
            apt-get update -qq && apt-get install -y -qq pkg-config libssl-dev > /dev/null 2>&1
            cargo build --release -p soma-agent -p soma-compositor 2>&1
        "

    # Copy binaries to the overlay
    mkdir -p "$SCRIPT_DIR/overlay/usr/bin"
    cp "$PROJECT_DIR/target/release/soma-agent" "$SCRIPT_DIR/overlay/usr/bin/" 2>/dev/null || true
    cp "$PROJECT_DIR/target/release/soma-compositor" "$SCRIPT_DIR/overlay/usr/bin/" 2>/dev/null || true

    echo "✓ Rust binaries compiled and placed in overlay"
    echo ""
fi

if [ "$1" == "--rust-only" ]; then
    echo "Done (Rust-only mode)."
    exit 0
fi

# ──────────────────────────────────────────────
# Step 2: Build the Buildroot image
# ──────────────────────────────────────────────
echo "▸ Step 2: Building SomaOS image via Buildroot (this takes 20-40 min first time)..."
echo ""

# Build the Docker builder image
docker build --platform linux/amd64 -t soma-builder "$SCRIPT_DIR"

# Run the build and extract the output images
CONTAINER_ID=$(docker create --platform linux/amd64 soma-builder)
docker start -a "$CONTAINER_ID"

# Extract built images
echo ""
echo "▸ Step 3: Extracting build artifacts..."
docker cp "$CONTAINER_ID:/opt/buildroot/output/images/rootfs.ext4" "$OUTPUT_DIR/soma-os.img" 2>/dev/null || true
docker cp "$CONTAINER_ID:/opt/buildroot/output/images/rootfs.iso9660" "$OUTPUT_DIR/soma-os.iso" 2>/dev/null || true
docker cp "$CONTAINER_ID:/opt/buildroot/output/images/bzImage" "$OUTPUT_DIR/bzImage" 2>/dev/null || true
docker rm "$CONTAINER_ID" > /dev/null

echo ""
echo "╔══════════════════════════════════════════╗"
echo "║    Build Complete!                        ║"
echo "╚══════════════════════════════════════════╝"
echo ""
echo "Output files:"
ls -lh "$OUTPUT_DIR/"
echo ""
echo "Next steps:"
echo "  1. Copy soma-os.iso to your Windows laptop"
echo "  2. Open VirtualBox → New → Linux (Other 64-bit)"
echo "  3. Attach soma-os.iso as optical drive"
echo "  4. Boot → SomaOS will auto-login as root"
echo ""
echo "Or test locally with QEMU:"
echo "  qemu-system-x86_64 -m 2G -cdrom $OUTPUT_DIR/soma-os.iso -display sdl"

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
BUILD_JOBS="${SOMA_BUILD_JOBS:-1}"

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

    # Use Docker to cross-compile with musl for fully static binaries
    # This avoids glibc version mismatches with Buildroot's toolchain
    docker run --rm --platform linux/amd64 \
        -v "$PROJECT_DIR:/project" \
        -w /project \
        rust:latest \
        bash -c "
            apt-get update -qq && apt-get install -y -qq \
                musl-tools libdrm-dev libevdev-dev > /dev/null 2>&1
            rustup target add x86_64-unknown-linux-musl
            # Agent: default features
            cargo build --release --target x86_64-unknown-linux-musl \
                -p soma-agent -p soma-cli 2>&1
            # Compositor: DRM/KMS backend (no winit)
            cargo build --release --target x86_64-unknown-linux-musl \
                -p soma-compositor \
                --no-default-features --features drm-backend 2>&1
        "

    # Copy binaries to the overlay
    mkdir -p "$SCRIPT_DIR/overlay/usr/bin"
    cp "$PROJECT_DIR/target/x86_64-unknown-linux-musl/release/soma-agent" "$SCRIPT_DIR/overlay/usr/bin/" 2>/dev/null || true
    cp "$PROJECT_DIR/target/x86_64-unknown-linux-musl/release/soma-compositor" "$SCRIPT_DIR/overlay/usr/bin/" 2>/dev/null || true
    cp "$PROJECT_DIR/target/x86_64-unknown-linux-musl/release/soma-cli" "$SCRIPT_DIR/overlay/usr/bin/" 2>/dev/null || true

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
echo "  Buildroot package parallelism: $BUILD_JOBS"
echo ""

# Build the Docker builder image
docker build --platform linux/amd64 -t soma-builder "$SCRIPT_DIR"

# Run the build and extract the output images
CONTAINER_ID=$(docker create --platform linux/amd64 \
    -e SOMA_BUILD_JOBS="$BUILD_JOBS" \
    soma-builder)
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
echo "Next steps (VirtualBox on Windows):"
echo "  1. Copy $OUTPUT_DIR/soma-os.img to Windows"
echo "  2. In Windows PowerShell (as admin):"
echo "       VBoxManage convertfromraw soma-os.img soma-os.vdi --format VDI"
echo "  3. VirtualBox → New → Linux / Other 64-bit → 4 GB RAM"
echo "     → Use existing VHD → select soma-os.vdi"
echo "  4. VM Settings → Display → VMSVGA, 128 MB VRAM, 3D on"
echo "  5. Boot → login: soma"
echo ""
echo "Or test locally with QEMU:"
echo "  qemu-system-x86_64 -m 4G -drive file=$OUTPUT_DIR/soma-os.img,if=virtio,format=raw -device virtio-vga -display sdl"

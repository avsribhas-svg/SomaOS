#!/bin/bash
# SomaOS — Full Build Pipeline
# Builds the complete bootable OS image using Docker + Buildroot
#
# Usage: ./build.sh [--rust-only] [--image-only] [--arch=x86_64|aarch64]
#   --rust-only        Only cross-compile Rust binaries
#   --image-only       Only build the Buildroot image (assumes binaries exist)
#   --arch=x86_64      Target x86_64 (default)
#   --arch=aarch64     Target ARM64 (native on Apple Silicon VMs)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_JOBS="${SOMA_BUILD_JOBS:-1}"

# Parse arguments
ARCH="x86_64"
MODE=""
for arg in "$@"; do
    case "$arg" in
        --arch=*) ARCH="${arg#*=}" ;;
        --rust-only) MODE="rust-only" ;;
        --image-only) MODE="image-only" ;;
    esac
done

# Arch-specific settings
if [ "$ARCH" = "aarch64" ]; then
    RUST_TARGET="aarch64-unknown-linux-musl"
    SOMA_DEFCONFIG="soma_aarch64_defconfig"
    KERNEL_IMAGE="Image"
else
    RUST_TARGET="x86_64-unknown-linux-musl"
    SOMA_DEFCONFIG="soma_defconfig"
    KERNEL_IMAGE="bzImage"
fi

OUTPUT_DIR="$SCRIPT_DIR/output/$ARCH"

echo "╔══════════════════════════════════════════╗"
echo "║    SomaOS Build Pipeline                  ║"
echo "╚══════════════════════════════════════════╝"
echo "  Arch: $ARCH  |  Target: $RUST_TARGET"
echo ""

mkdir -p "$OUTPUT_DIR"

# ──────────────────────────────────────────────
# Step 1: Cross-compile Rust binaries
# ──────────────────────────────────────────────
if [ "$MODE" != "image-only" ]; then
    echo "▸ Step 1: Cross-compiling Rust binaries for $RUST_TARGET..."
    echo ""

    # Use Docker to cross-compile with musl for fully static binaries
    # This avoids glibc version mismatches with Buildroot's toolchain
    if [ "$ARCH" = "aarch64" ]; then
        CROSS_ENV="CC_aarch64_unknown_linux_musl=aarch64-linux-gnu-gcc \
            CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc"
    else
        CROSS_ENV=""
    fi

    docker run --rm --platform linux/amd64 \
        -v "$PROJECT_DIR:/project" \
        -w /project \
        rust:latest \
        bash -c "
            apt-get update -qq && apt-get install -y -qq \
                musl-tools libdrm-dev libevdev-dev gcc-aarch64-linux-gnu > /dev/null 2>&1
            rustup target add $RUST_TARGET
            export $CROSS_ENV
            # Agent: default features
            cargo build --release --target $RUST_TARGET \
                -p soma-agent -p soma-cli 2>&1
            # Compositor: DRM/KMS backend (no winit)
            cargo build --release --target $RUST_TARGET \
                -p soma-compositor \
                --no-default-features --features drm-backend 2>&1
        "

    # Copy binaries to the overlay
    mkdir -p "$SCRIPT_DIR/overlay/usr/bin"
    cp "$PROJECT_DIR/target/$RUST_TARGET/release/soma-agent" "$SCRIPT_DIR/overlay/usr/bin/" 2>/dev/null || true
    cp "$PROJECT_DIR/target/$RUST_TARGET/release/soma-compositor" "$SCRIPT_DIR/overlay/usr/bin/" 2>/dev/null || true
    cp "$PROJECT_DIR/target/$RUST_TARGET/release/soma-cli" "$SCRIPT_DIR/overlay/usr/bin/" 2>/dev/null || true

    echo "✓ Rust binaries compiled and placed in overlay"
    echo ""
fi

if [ "$MODE" = "rust-only" ]; then
    echo "Done (Rust-only mode)."
    exit 0
fi

# ──────────────────────────────────────────────
# Step 2: Build the Buildroot image
# ──────────────────────────────────────────────
echo "▸ Step 2: Building SomaOS $ARCH image via Buildroot..."
echo "  Buildroot package parallelism: $BUILD_JOBS"
echo "  Defconfig: $SOMA_DEFCONFIG"
echo ""

# Build the Docker builder image (tagged by arch to avoid conflicts)
docker build --platform linux/amd64 -t soma-builder-$ARCH "$SCRIPT_DIR"

# Run the build and extract the output images
CONTAINER_ID=$(docker create --platform linux/amd64 \
    -e SOMA_BUILD_JOBS="$BUILD_JOBS" \
    -e SOMA_DEFCONFIG="$SOMA_DEFCONFIG" \
    soma-builder-$ARCH)
docker start -a "$CONTAINER_ID"

# Extract built images
echo ""
echo "▸ Step 3: Extracting build artifacts..."
# Copy the entire images/ directory so symlink targets are included
mkdir -p "$OUTPUT_DIR/images-tmp"
docker cp "$CONTAINER_ID:/opt/buildroot/output/images/." "$OUTPUT_DIR/images-tmp/"
docker rm "$CONTAINER_ID" > /dev/null

# Resolve rootfs — Buildroot may produce ext2, ext3, or ext4
for ext in ext2 ext4 ext3; do
    if [ -e "$OUTPUT_DIR/images-tmp/rootfs.$ext" ]; then
        cp -L "$OUTPUT_DIR/images-tmp/rootfs.$ext" "$OUTPUT_DIR/soma-os.img"
        echo "  rootfs format: $ext"
        break
    fi
done
# Copy kernel (name differs by arch: bzImage for x86, Image for arm64)
cp -L "$OUTPUT_DIR/images-tmp/$KERNEL_IMAGE" "$OUTPUT_DIR/kernel" 2>/dev/null || true
cp -L "$OUTPUT_DIR/images-tmp/rootfs.iso9660" "$OUTPUT_DIR/soma-os.iso" 2>/dev/null || true
rm -rf "$OUTPUT_DIR/images-tmp"

echo ""
echo "╔══════════════════════════════════════════╗"
echo "║    Build Complete!                        ║"
echo "╚══════════════════════════════════════════╝"
echo ""
echo "Output files (arch: $ARCH):"
ls -lh "$OUTPUT_DIR/"
echo ""
if [ "$ARCH" = "aarch64" ]; then
    echo "Test with UTM (Apple Silicon — native speed):"
    echo "  New VM → Virtualize → Linux → Kernel: $OUTPUT_DIR/kernel"
    echo "  Root disk: $OUTPUT_DIR/soma-os.img  |  Login: soma"
else
    echo "Test with QEMU (x86_64):"
    echo "  qemu-system-x86_64 -m 4G -drive file=$OUTPUT_DIR/soma-os.img,if=virtio,format=raw -device virtio-vga -display sdl"
fi

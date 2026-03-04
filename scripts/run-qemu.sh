#!/bin/bash
# SomaOS — QEMU launch script
# Boots the SomaOS image in QEMU with virtio GPU, keyboard, and mouse

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
IMAGE="$PROJECT_DIR/buildroot/output/soma-os.img"

if [ ! -f "$IMAGE" ]; then
    echo "Error: SomaOS image not found at $IMAGE"
    echo "Run './buildroot/build.sh' first to build the image."
    exit 1
fi

echo "╔══════════════════════════════════════╗"
echo "║     Launching SomaOS in QEMU         ║"
echo "╚══════════════════════════════════════╝"

qemu-system-x86_64 \
    -m 2G \
    -smp 2 \
    -drive file="$IMAGE",format=raw \
    -device virtio-gpu-pci \
    -device virtio-keyboard-pci \
    -device virtio-mouse-pci \
    -display default \
    -serial stdio \
    -net nic -net user

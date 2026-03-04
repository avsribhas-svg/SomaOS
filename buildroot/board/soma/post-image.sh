#!/bin/bash
# Post-image script: creates a bootable disk image from Buildroot output
# This runs AFTER Buildroot creates rootfs.ext2 and bzImage
set -e

BOARD_DIR="$(dirname "$0")"
IMAGES_DIR="${BINARIES_DIR:-output/images}"

echo "=== SomaOS: Creating bootable disk image ==="

# Install GRUB config into the rootfs
mkdir -p "${TARGET_DIR}/boot/grub"
cp "${BOARD_DIR}/grub.cfg" "${TARGET_DIR}/boot/grub/grub.cfg"

# Copy kernel into rootfs /boot
if [ -f "${IMAGES_DIR}/bzImage" ]; then
    cp "${IMAGES_DIR}/bzImage" "${TARGET_DIR}/boot/bzImage"
fi

echo "=== SomaOS: Post-image complete ==="

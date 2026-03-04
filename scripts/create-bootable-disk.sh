#!/bin/bash
# create-bootable-disk.sh
# Assembles a bootable disk image from Buildroot output files
# Run this in WSL2 on Windows
#
# Usage: sudo bash create-bootable-disk.sh <output-dir> <dest-file>
# Example: sudo bash create-bootable-disk.sh ./output ~/soma-disk.img

set -e

OUTPUT_DIR="${1:-.output}"
DEST="${2:-soma-disk.img}"
ROOTFS="${OUTPUT_DIR}/rootfs.ext2"
KERNEL="${OUTPUT_DIR}/bzImage"
GRUB_IMG="${OUTPUT_DIR}/grub.img"

echo "╔══════════════════════════════════════════╗"
echo "║  SomaOS — Creating Bootable Disk Image   ║"
echo "╚══════════════════════════════════════════╝"

# Verify inputs
for f in "$ROOTFS" "$KERNEL"; do
    if [ ! -f "$f" ]; then
        echo "ERROR: Missing $f"
        exit 1
    fi
done

# Get rootfs size and calculate disk size (rootfs + 64MB overhead for boot/MBR)
ROOTFS_SIZE=$(stat -c%s "$ROOTFS")
DISK_SIZE=$(( ROOTFS_SIZE + 64 * 1024 * 1024 ))
echo "Rootfs: $(( ROOTFS_SIZE / 1024 / 1024 )) MB"
echo "Disk:   $(( DISK_SIZE / 1024 / 1024 )) MB"

# 1. Create empty disk image
echo ""
echo "▸ Creating disk image..."
dd if=/dev/zero of="$DEST" bs=1M count=$(( DISK_SIZE / 1024 / 1024 )) status=progress

# 2. Create MBR partition table with one Linux partition
echo ""
echo "▸ Creating partition table..."
# Start partition at sector 2048 (1MB offset) for alignment
echo -e "o\nn\np\n1\n2048\n\na\nw" | fdisk "$DEST" > /dev/null 2>&1 || true

# 3. Set up loop device
echo ""
echo "▸ Setting up loop device..."
LOOP=$(losetup --find --show --partscan "$DEST")
PARTITION="${LOOP}p1"

# Wait for partition device to appear
sleep 1
if [ ! -b "$PARTITION" ]; then
    partprobe "$LOOP" 2>/dev/null || true
    sleep 1
fi

# 4. Format the partition as ext4
echo "▸ Formatting partition as ext4..."
mkfs.ext4 -q -L "somaos" "$PARTITION"

# 5. Mount and copy rootfs contents
echo "▸ Copying rootfs..."
MOUNT_DIR=$(mktemp -d)
mount "$PARTITION" "$MOUNT_DIR"

# Extract rootfs.ext2 contents into the partition
ROOTFS_MOUNT=$(mktemp -d)
mount -o loop,ro "$ROOTFS" "$ROOTFS_MOUNT"
cp -a "$ROOTFS_MOUNT"/* "$MOUNT_DIR"/
umount "$ROOTFS_MOUNT"
rmdir "$ROOTFS_MOUNT"

# 6. Copy kernel to /boot
echo "▸ Installing kernel..."
mkdir -p "$MOUNT_DIR/boot/grub"
cp "$KERNEL" "$MOUNT_DIR/boot/bzImage"

# 7. Create GRUB config
cat > "$MOUNT_DIR/boot/grub/grub.cfg" << 'EOF'
set default=0
set timeout=3

menuentry "SomaOS v0.1.0" {
    linux /boot/bzImage root=/dev/sda1 console=tty1 console=ttyS0,115200 rw
}
EOF

# 8. Install GRUB bootloader
echo "▸ Installing GRUB bootloader..."
if command -v grub-install > /dev/null 2>&1; then
    grub-install --target=i386-pc --boot-directory="$MOUNT_DIR/boot" --modules="part_msdos ext2 linux normal biosdisk" "$LOOP"
elif command -v grub2-install > /dev/null 2>&1; then
    grub2-install --target=i386-pc --boot-directory="$MOUNT_DIR/boot" --modules="part_msdos ext2 linux normal biosdisk" "$LOOP"
else
    echo "ERROR: grub-install not found. Install with: sudo apt install grub-pc-bin grub2-common"
    umount "$MOUNT_DIR"
    losetup -d "$LOOP"
    exit 1
fi

# 9. Cleanup
echo "▸ Cleaning up..."
umount "$MOUNT_DIR"
rmdir "$MOUNT_DIR"
losetup -d "$LOOP"

echo ""
echo "╔══════════════════════════════════════════╗"
echo "║  Done! Bootable disk: $DEST"
echo "╚══════════════════════════════════════════╝"
echo ""
echo "Next: copy to Windows and convert for VirtualBox:"
echo "  cp $DEST /mnt/c/Users/\$USER/OneDrive/Desktop/"
echo "  VBoxManage convertfromraw soma-disk.img soma-os.vdi --format VDI"

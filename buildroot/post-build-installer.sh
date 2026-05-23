#!/bin/sh
# Post-build script for the SomaOS installer image.
# Enables the soma-installer systemd service and sets up grub config.
set -e

TARGET="$1"

# Enable installer service
ln -sf /etc/systemd/system/soma-installer.service \
    "${TARGET}/etc/systemd/system/multi-user.target.wants/soma-installer.service"

# Disable getty on tty1 (installer takes the console)
rm -f "${TARGET}/etc/systemd/system/getty.target.wants/getty@tty1.service"

# Mark this as an installer image
echo "soma_installer" > "${TARGET}/etc/soma/image_type"

echo "[post-build-installer] Installer rootfs configured."

#!/bin/bash
# Post-build script for SomaOS
# Runs after Buildroot populates the rootfs but before the image is created

TARGET_DIR="$1"

echo "=== SomaOS post-build ==="

# Enable soma services
mkdir -p "${TARGET_DIR}/etc/systemd/system/multi-user.target.wants"
ln -sf /etc/systemd/system/soma-agent.service \
    "${TARGET_DIR}/etc/systemd/system/multi-user.target.wants/soma-agent.service"

# Set up auto-login on tty1 (boots straight to a shell, not a login prompt)
mkdir -p "${TARGET_DIR}/etc/systemd/system/getty@tty1.service.d"
cat > "${TARGET_DIR}/etc/systemd/system/getty@tty1.service.d/autologin.conf" << 'EOF'
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin root --noclear %I $TERM
EOF

# Create a login message
cat > "${TARGET_DIR}/etc/motd" << 'EOF'

  ╔══════════════════════════════════════════╗
  ║         Welcome to SomaOS v0.1.0        ║
  ║                                          ║
  ║  AI-Native Operating System for Agents   ║
  ║                                          ║
  ║  • soma-agent: running as systemd svc    ║
  ║  • To start compositor: soma-compositor  ║
  ║                                          ║
  ╚══════════════════════════════════════════╝

EOF

# Set hostname
echo "somaos" > "${TARGET_DIR}/etc/hostname"

echo "=== SomaOS post-build complete ==="

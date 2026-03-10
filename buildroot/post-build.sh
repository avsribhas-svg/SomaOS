#!/bin/bash
# Post-build script for SomaOS v0.8
# Runs after Buildroot populates the rootfs but before the image is created

TARGET_DIR="$1"
set -e

echo "=== SomaOS v0.8 post-build ==="

# ── Service auto-start ───────────────────────────────────────────────────────
MU="${TARGET_DIR}/etc/systemd/system/multi-user.target.wants"
mkdir -p "$MU"

for svc in soma-ollama soma-agent soma-compositor soma-first-boot; do
    ln -sf "/etc/systemd/system/${svc}.service" "${MU}/${svc}.service"
done

# ── Auto-login on tty1 ──────────────────────────────────────────────────────
GETTY_DROP="${TARGET_DIR}/etc/systemd/system/getty@tty1.service.d"
mkdir -p "$GETTY_DROP"
cat > "${GETTY_DROP}/autologin.conf" << 'EOF'
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin root --noclear %I $TERM
EOF

# ── SomaOS login password file ───────────────────────────────────────────────
# Plain-text for now; soma-compositor login screen reads this.
mkdir -p "${TARGET_DIR}/etc/soma"
echo "soma" > "${TARGET_DIR}/etc/soma/passwd"
chmod 600 "${TARGET_DIR}/etc/soma/passwd"

# ── Model + Ollama storage directory ─────────────────────────────────────────
mkdir -p "${TARGET_DIR}/var/lib/soma/ollama-models"

# ── First-boot model download script ─────────────────────────────────────────
cat > "${TARGET_DIR}/usr/local/bin/soma-first-boot.sh" << 'SCRIPT'
#!/bin/bash
# Downloads qwen2.5-coder:7b on first boot. Skips on subsequent boots.
DONE_FLAG=/var/lib/soma/.first-boot-done
[ -f "$DONE_FLAG" ] && exit 0

echo "[soma-first-boot] Waiting for Ollama to start..."
for i in $(seq 1 30); do
    curl -sf http://127.0.0.1:11434/api/tags > /dev/null 2>&1 && break
    sleep 2
done

echo "[soma-first-boot] Pulling qwen2.5-coder:7b (this may take a while)..."
OLLAMA_MODELS=/var/lib/soma/ollama-models /usr/local/bin/ollama pull qwen2.5-coder:7b

touch "$DONE_FLAG"
echo "[soma-first-boot] Model ready."
SCRIPT
chmod +x "${TARGET_DIR}/usr/local/bin/soma-first-boot.sh"

# ── First-boot systemd one-shot ───────────────────────────────────────────────
cat > "${TARGET_DIR}/etc/systemd/system/soma-first-boot.service" << 'EOF'
[Unit]
Description=SomaOS First Boot — Pull LLM Model
After=soma-ollama.service network-online.target
Wants=network-online.target
ConditionPathExists=!/var/lib/soma/.first-boot-done

[Service]
Type=oneshot
ExecStart=/usr/local/bin/soma-first-boot.sh
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

# ── MOTD ────────────────────────────────────────────────────────────────────
cat > "${TARGET_DIR}/etc/motd" << 'EOF'

  ╔══════════════════════════════════════════╗
  ║         SomaOS v0.8.0                   ║
  ║  AI-Native OS — Agent is primary user   ║
  ╚══════════════════════════════════════════╝

EOF

echo "somaos" > "${TARGET_DIR}/etc/hostname"
echo "=== SomaOS v0.8 post-build complete ==="

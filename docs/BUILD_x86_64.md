# SomaOS — x86_64 Build Guide

Builds a bootable x86_64 SomaOS image and runs it in VirtualBox (Windows) or QEMU (Linux / Intel Mac).

---

## Overview

```
Docker (builder)                Host
────────────────                ─────────────────────────────────
./build.sh           →  buildroot/output/x86_64/soma-os.img
                                  ↓
                         Windows: VBoxManage convertfromraw → VDI → VirtualBox VM
                         Linux / Mac: qemu-system-x86_64 directly
```

---

## Prerequisites

- **Docker** (running — `docker run hello-world` should work)
- **Git**
- **Windows only**: VirtualBox installed on the host; WSL2 with Docker Desktop WSL integration enabled
- **Linux / Intel Mac only**: QEMU (`brew install qemu` / `apt install qemu-system-x86`)

---

## Step 1 — Get the Code

```bash
git clone https://github.com/avsribhas-svg/SomaOS.git
cd SomaOS
chmod +x buildroot/build.sh buildroot/post-build.sh
```

---

## Step 2 — Build the OS Image

```bash
cd buildroot
./build.sh --arch=x86_64
```

> `x86_64` is the default — `./build.sh` works too.

**What this does:**

1. Cross-compiles `soma-agent`, `soma-compositor` (DRM/KMS backend), and `soma-cli` inside a Docker container targeting `x86_64-unknown-linux-musl`
2. Builds a full Linux image via Buildroot (kernel + GRUB2 + systemd + Mesa + ALSA)
3. Runs `post-build.sh` to install systemd services and first-boot scripts

**Output** — `buildroot/output/x86_64/`:
- `soma-os.img` — raw bootable disk image
- `kernel` — Linux kernel (`bzImage`)

> **Time**: ~40 min first build. Subsequent Rust-only rebuilds are ~2–5 min — use `./build.sh --rust-only` then `./build.sh --image-only`.

---

## Running — Windows (VirtualBox via WSL2)

### Convert to VDI (inside WSL2)

```bash
cp buildroot/output/x86_64/soma-os.img /mnt/c/Users/$USER/Desktop/soma-os.img
```

Then open **PowerShell** (not WSL2):

```powershell
cd "$env:USERPROFILE\Desktop"
& "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe" convertfromraw soma-os.img soma-os.vdi --format VDI
```

### Create VirtualBox VM

VirtualBox → **New**:

| Field | Value |
|-------|-------|
| Name | SomaOS |
| Type | Linux |
| Version | Other Linux (64-bit) |
| Memory | 4096 MB |
| Hard disk | Use an existing virtual hard disk file → `soma-os.vdi` |

Click **Finish**, then open **Settings** before booting:

**Display** (critical for DRM/KMS):

| Setting | Value |
|---------|-------|
| Graphics Controller | **VMSVGA** |
| Video Memory | 128 MB |
| Enable 3D Acceleration | ✓ |

> VMSVGA exposes `/dev/dri/card0` via the `vmwgfx` driver. VBoxVGA does not work.

**System → Processor**: 2 CPUs

**Network → Adapter 1**: NAT (needed for first-boot model download)

**Optional — serial log** (System → Serial Ports → Port 1):

| Setting | Value |
|---------|-------|
| Enable Serial Port | ✓ |
| Port Mode | Raw File |
| Path | `C:\Users\<you>\soma-serial.log` |

Click **Start**.

---

## Running — Linux / Intel Mac (QEMU)

```bash
qemu-system-x86_64 \
  -m 4G \
  -smp 2 \
  -drive file=buildroot/output/x86_64/soma-os.img,if=virtio,format=raw \
  -device virtio-vga \
  -display sdl \
  -net nic -net user
```

---

## Boot Sequence

```
GRUB (auto-selects after 3 sec)
  └→ Linux kernel
       └→ systemd
            ├→ soma-ollama.service     — Ollama LLM server
            ├→ soma-agent.service      — Agent daemon (waits for Ollama)
            ├→ soma-first-boot.service — (first boot only) pulls qwen2.5-coder:7b
            └→ soma-compositor.service — Compositor on tty1
                  └→ [Login screen]
```

**Login**: type `soma`, press **Enter**.

---

## First Boot — Model Download

`soma-first-boot.service` pulls `qwen2.5-coder:7b` (~4 GB) in the background. This takes 5–15 minutes depending on connection speed. The UI is usable during this time; agent tasks will fail until the model is ready.

Watch progress (VirtualBox: press `Right Ctrl + F2` to switch to a text console):

```bash
journalctl -f -u soma-first-boot
```

Press `Right Ctrl + F1` to return to the compositor.

---

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `F1` | Open / focus Terminal |
| `F2` | Close focused window |
| `F3` | Toggle AI sidebar |
| `F4` | Toggle desktop agent mode |
| `F5` | Toggle private mode (pauses workflow observation) |
| `Enter` | Submit command (sidebar) / confirm (terminal) |
| `Escape` | Reject pending HITL approval |
| `Tab` | Shell completion (terminal) / switch to terminal (sidebar) |
| `Ctrl+C` | Interrupt process (terminal) |
| `Ctrl+D` | EOF / logout (terminal) |
| `Ctrl+L` | Clear terminal |
| `Right Ctrl + F2` | Switch to debug console (VirtualBox) |

---

## Iterative Development

**Fastest — SSH into a running VM:**

```bash
# Find VM IP: run `ip addr show` in the VM console
./build.sh --rust-only

scp buildroot/overlay/usr/bin/soma-compositor root@<VM-IP>:/usr/bin/
scp buildroot/overlay/usr/bin/soma-agent root@<VM-IP>:/usr/bin/

ssh root@<VM-IP> "systemctl restart soma-agent soma-compositor"
```

**Full image rebuild** (only needed when changing defconfig, post-build.sh, or systemd units):

```bash
./build.sh --rust-only && ./build.sh --image-only
```

Then re-convert and replace the VDI (Windows), or point QEMU at the new image.

**VirtualBox — replace VDI (PowerShell):**

```powershell
cp buildroot/output/x86_64/soma-os.img /mnt/c/Users/$USER/Desktop/soma-os.img  # (WSL2)

# PowerShell:
Remove-Item soma-os.vdi -ErrorAction SilentlyContinue
& "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe" convertfromraw soma-os.img soma-os.vdi --format VDI
```

Delete the old VM (Machine → Remove → Delete all files) and create a new one pointing to the updated VDI.

---

## Changing the Login Password

```bash
echo "yournewpassword" > buildroot/overlay/etc/soma/passwd
./build.sh --image-only
```

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Black screen after boot | Wrong graphics controller | Settings → Display → **VMSVGA**, not VBoxVGA |
| `No DRM card found` in journal | vmwgfx not loaded | Ensure VMSVGA + 3D Acceleration enabled; `dmesg \| grep vmwgfx` |
| Keyboard not working at login | evdev device not found | `ls /dev/input/` — should have `event0`, `event1` |
| Agent tasks fail with "model not found" | First-boot pull incomplete | `journalctl -f -u soma-first-boot` |
| `build.sh: Permission denied` | Script not executable | `chmod +x buildroot/build.sh buildroot/post-build.sh` |
| Docker not found (WSL2) | Docker Desktop WSL integration off | Docker Desktop → Settings → Resources → WSL Integration → enable your distro |
| `VBoxManage` not found (WSL2) | Use PowerShell, not WSL2 | Run `VBoxManage.exe` from PowerShell |
| No network in VM | DHCP not started | `systemctl status dhcpcd` |
| soma-compositor crashes | Service error | `journalctl -u soma-compositor -n 50` |

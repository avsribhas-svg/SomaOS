# Building and Running SomaOS on Windows (VirtualBox)

Complete guide for building the SomaOS v1.0.1 image inside WSL2 and running it as a new VirtualBox VM on Windows.

---

## Overview

The build runs entirely inside **WSL2** (where Docker is already set up). The output is a `.vdi` disk image you import into VirtualBox as a brand new VM.

```
WSL2 (builder)               Windows host
──────────────               ──────────────
git clone repo
./buildroot/build.sh  ──→  soma-os.img
VBoxManage convertfromraw ──→  soma-os.vdi ──→  New VirtualBox VM
```

---

## Prerequisites

Inside WSL2:
- Docker installed and running (`docker run hello-world` works)
- Git

On your Windows host:
- VirtualBox installed

---

## Step 1 — Get the Code

Open your **WSL2 terminal**:

```bash
git clone https://github.com/avsribhas-svg/SomaOS.git
cd SomaOS
```

---

## Step 2 — Build the OS Image

```bash
cd buildroot
./build.sh
```

**What this does:**

1. **Cross-compiles Rust binaries** inside a Docker container (`rust:latest` with musl):
   - `soma-agent` — agent daemon (default features)
   - `soma-compositor` — compositor with `--no-default-features --features drm-backend` (DRM/KMS, no winit)
   - `soma-cli` — terminal test client
   - Copies binaries to `buildroot/overlay/usr/bin/`

2. **Builds the full Linux image** via Buildroot inside Docker:
   - Downloads Linux kernel, GRUB2, systemd, Mesa, ALSA (~1 GB sources, cached after first run)
   - Compiles everything into a 4 GB ext4 disk image
   - Runs `post-build.sh` to install systemd services and first-boot scripts

3. **Outputs** to `buildroot/output/`:
   - `soma-os.img` — raw disk image (bootable)
   - `bzImage` — Linux kernel (for reference)

> **Time**: ~40 min first build. Subsequent builds are fast if only Rust code changed — run `./build.sh --rust-only` then `./build.sh --image-only`.

---

## Step 3 — Convert to VDI

Still inside WSL2, copy the image to a Windows-accessible path and convert:

```bash
# Copy raw image to your Windows Desktop
cp buildroot/output/soma-os.img /mnt/c/Users/$USER/Desktop/soma-os.img
```

Then open **PowerShell** (not WSL2) and run:

```powershell
cd "$env:USERPROFILE\Desktop"
& "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe" convertfromraw soma-os.img soma-os.vdi --format VDI
```

This produces `soma-os.vdi` (~4 GB) on your Desktop.

---

## Step 4 — Create New VirtualBox VM on Windows

Open **VirtualBox** → click **New**.

### Wizard settings

| Field | Value |
|-------|-------|
| Name | SomaOS v1.0.1 |
| Type | Linux |
| Version | Other Linux (64-bit) |
| Memory | **4096 MB** |
| Hard disk | **Use an existing virtual hard disk file** |
| → Browse | select `soma-os.vdi` from Desktop |

Click **Finish**.

### Configure before booting

Right-click **SomaOS v1.0.1** → **Settings**:

**Display tab** — this is critical for DRM/KMS:

| Setting | Value |
|---------|-------|
| Graphics Controller | **VMSVGA** |
| Video Memory | **128 MB** |
| Enable 3D Acceleration | ✓ checked |

> VMSVGA exposes `/dev/dri/card0` via the `vmwgfx` kernel driver, which the DRM compositor requires. VBoxVGA does not work.

**System → Processor tab:**

| Setting | Value |
|---------|-------|
| Processors | 2 |

**Network tab:**

| Setting | Value |
|---------|-------|
| Adapter 1 | NAT |

NAT is needed so the first-boot service can download the Ollama model.

**Serial Ports → Port 1** (optional but useful for debug):

| Setting | Value |
|---------|-------|
| Enable Serial Port | ✓ |
| Port Mode | Raw File |
| Path | `C:\Users\<you>\soma-serial.log` |

---

## Step 5 — Boot

Click **Start**.

### Expected boot sequence

```
GRUB bootloader (auto-selects after 3 sec)
  └→ Linux kernel
       └→ systemd
            ├→ soma-ollama.service    — Ollama LLM server starts
            ├→ soma-agent.service     — Agent daemon (waits for Ollama)
            ├→ soma-first-boot.service — (first boot only) pulls qwen2.5-coder:7b
            └→ soma-compositor.service — Compositor takes over tty1
                  └→ [Login screen appears on the VM display]
```

### Login

Type `soma` and press **Enter**.

The compositor UI loads: PTY terminal on the left, chat sidebar on the right.

---

## First Boot — Model Download

On the **very first boot**, `soma-first-boot.service` runs in the background and pulls `qwen2.5-coder:7b` (~4 GB). This takes 5–15 minutes depending on your connection.

**During this time:**
- The compositor UI is fully usable
- The terminal and sidebar work
- Agent tasks will fail until the model finishes downloading

**Watch the download progress:**

Press `Right Ctrl + F2` in VirtualBox to switch to a text console:
```bash
journalctl -f -u soma-first-boot
```

Press `Right Ctrl + F1` to return to the compositor display.

Once the download finishes you'll see a success toast in the compositor.

---

## Keyboard Shortcuts Inside the Compositor

| Key | Action |
|-----|--------|
| `F1` | Open / focus Terminal |
| `F2` | Close focused window |
| `F3` | Toggle AI sidebar |
| `F4` | Toggle desktop agent mode |
| `F5` | Toggle private mode (pauses workflow observation) |
| `Enter` | Submit command (sidebar) / confirm shell input (terminal) |
| `Escape` | Reject a pending HITL approval |
| `Tab` | Shell completion (terminal) / switch to terminal (sidebar) |
| `F1` | Open or focus the Terminal |
| `F2` | Close the currently focused window |
| `F3` | Toggle the AI Sidebar |
| `F4` | Toggle Agent Mode (accent border) |
| `F5` | Toggle Private Mode |
| `Enter` | Submit command (sidebar) / confirm input (terminal) |
| `Escape` | Dismiss modal, reject HITL, or close sidebar |
| `Tab` | Shell completion (terminal) |
| `Ctrl+C` | Interrupt running process (terminal) |
| `Ctrl+D` | EOF / logout (terminal) |
| `Ctrl+L` | Clear terminal |
| `Right Ctrl + F2` | Switch to debug console (VirtualBox) |

Scroll with mouse wheel or trackpad in any window or panel.

There is a **Dock** at the bottom to launch apps (Terminal, Browser, Settings) and toggle states (Sidebar, Agent Mode). Settings allows you to configure the default LLM provider directly in-OS.

Click any **result card** or **error card** in the sidebar to open a full detail modal. Click anywhere to dismiss.

---

## Rebuilding After Code Changes

For iterative development, you don't need to rebuild the full image every time:

```bash
# In WSL2 — recompile Rust only (~2 min)
./build.sh --rust-only

# Then rebuild image with new binaries (~5 min, uses cached Buildroot)
./build.sh --image-only

# Re-convert and replace the VDI
cp buildroot/output/soma-os.img /mnt/c/Users/$USER/Desktop/soma-os.img
```

Then in PowerShell:
```powershell
cd "$env:USERPROFILE\Desktop"
Remove-Item soma-os.vdi -ErrorAction SilentlyContinue
& "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe" convertfromraw soma-os.img soma-os.vdi --format VDI
```

Delete the old VirtualBox VM (Machine → Remove → Delete all files), then create a new one pointing to the updated VDI.

For the fastest iteration — copy binaries directly into a running VM over SSH:

```bash
# In WSL2
./build.sh --rust-only

# Find the VM's IP: in the VM console → ip addr show
scp buildroot/overlay/usr/bin/soma-compositor root@<VM-IP>:/usr/bin/
scp buildroot/overlay/usr/bin/soma-agent root@<VM-IP>:/usr/bin/

# Restart services
ssh root@<VM-IP> "systemctl restart soma-agent soma-compositor"
```

---

## Changing the Login Password

The default password is `soma`. To change it, edit the overlay before building:

```bash
echo "yournewpassword" > buildroot/overlay/etc/soma/passwd
./build.sh --image-only
```

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Black screen after boot | Wrong graphics controller | VM Settings → Display → set **VMSVGA**, not VBoxVGA |
| `No DRM card found` in journal | vmwgfx driver not loaded | Ensure VMSVGA + 3D Acceleration enabled; check `dmesg \| grep vmwgfx` |
| Login screen appears but keyboard doesn't work | evdev device not found | Check `ls /dev/input/` — should have `event0`, `event1` |
| Agent tasks fail with "model not found" | First-boot pull incomplete | Wait for `soma-first-boot.service`; watch: `journalctl -f -u soma-first-boot` |
| `build.sh: Permission denied` | Script not executable | `chmod +x buildroot/build.sh buildroot/post-build.sh` |
| Docker not found in WSL2 | Docker Desktop WSL integration off | Docker Desktop → Settings → Resources → WSL Integration → enable your distro |
| `VBoxManage` not found in WSL2 | Use PowerShell for conversion | Run the `VBoxManage.exe` command in PowerShell, not WSL2 |
| No network in VM after boot | DHCP not started | `systemctl status dhcpcd` — should auto-start with NAT adapter |
| soma-compositor crashes immediately | Service error | `journalctl -u soma-compositor -n 50` for details |

---

## Development Workflow

```
Edit code (Mac or Windows)
  → git push
    → WSL2: git pull
      → cd buildroot && ./build.sh
        → cp soma-os.img /mnt/c/Users/$USER/Desktop/
          → PowerShell: VBoxManage convertfromraw ...
            → VirtualBox: new VM → attach VDI → Start
```

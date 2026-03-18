# SomaOS — ARM64 Build Guide (Apple Silicon)

Builds a native ARM64 SomaOS image and runs it on Apple Silicon Mac using UTM.

> For x86_64 (Windows / Linux / Intel Mac), see [BUILD_x86_64.md](BUILD_x86_64.md).
> For a full local CI workflow (replaces GitHub Actions), see [LOCAL_CI.md](LOCAL_CI.md).

---

## Getting a Pre-Built Image

You don't have to build from scratch. Two options:

**Option A — GitHub Actions artifact** (when CI minutes are available):
1. Go to the repo → **Actions** → latest successful run on `main`
2. Download the `soma-os-aarch64-<sha>` artifact (contains `soma-os.img.gz` + `kernel`)
3. `gunzip soma-os.img.gz` → proceed to Step 3 (Run in UTM) below

**Option B — Build locally** (always available, no CI minutes needed):
Follow Steps 1–2 below. Docker does all the heavy lifting — no native Linux toolchain needed.

---

## Overview

```
Docker (builder)               Mac host
────────────────               ─────────────────────────────────
./build.sh \
  --arch=aarch64    →  buildroot/output/aarch64/soma-os.img
                                 ↓
                        UTM → Virtualize → Linux (native ARM64 speed)
```

Because UTM can virtualize ARM64 natively on Apple Silicon, the VM runs at near-native speed — no emulation penalty.

---

## Prerequisites

- **Docker Desktop** for Mac (Apple Silicon), running
- **Git**
- **UTM** — [mac.getutm.app](https://mac.getutm.app) (free)

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
./build.sh --arch=aarch64
```

**What this does:**

1. Cross-compiles `soma-agent`, `soma-compositor` (DRM/KMS backend), and `soma-cli` inside a Docker container targeting `aarch64-unknown-linux-musl`
2. Builds a full Linux ARM64 image via Buildroot (kernel + systemd + Mesa + ALSA)
3. Installs systemd services and first-boot scripts

**Output** — `buildroot/output/aarch64/`:
- `soma-os.img` — raw bootable disk image
- `kernel` — Linux kernel (`Image`)

> **Time**: ~40 min first build. Subsequent Rust-only rebuilds: `./build.sh --arch=aarch64 --rust-only`.

---

## Step 3 — Run in UTM

### Create a new VM

1. Open UTM → click **+** → **Virtualize**
2. Select **Linux**
3. On the **Linux** screen:
   - **Kernel Image**: browse to `buildroot/output/aarch64/kernel`
   - **Boot arguments**: `root=/dev/vda rw console=tty0 console=ttyAMA0`
   - Leave initramfs blank
4. **Hardware**:
   - Memory: **4096 MB**
   - CPU Cores: 2
5. **Storage**: skip (we'll attach the raw image manually)
6. **Shared Directory**: skip
7. Click **Save**

### Attach the disk image

Before booting, open the VM settings in UTM:

- **Drives** → click **+** → **Import Drive**
- Select `buildroot/output/aarch64/soma-os.img`
- Interface: **VirtIO**

### Display settings (for DRM/KMS)

In VM settings → **Display**:

| Setting | Value |
|---------|-------|
| Display Card | **virtio-gpu-gl** |
| Resolution | 1280×720 or higher |

> `virtio-gpu-gl` exposes a DRM device the SomaOS compositor can render to directly.

Click **Play** to boot.

---

## Boot Sequence

```
Linux kernel
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

`soma-first-boot.service` pulls `qwen2.5-coder:7b` (~4 GB) in the background on the first boot. The UI is usable during this time; agent tasks will fail until the model finishes.

Watch progress in the UTM serial console (or SSH into the VM):

```bash
journalctl -f -u soma-first-boot
```

---

## Dev Mode — No VM Required

For fast iteration on macOS, run the compositor directly in a native window using the `winit` backend. No VM or image build needed.

```bash
# 1. Start Ollama with the model
ollama pull qwen2.5-coder:7b
ollama serve &

# 2. Start the agent daemon
cargo run -p soma-agent

# 3. In another terminal, start the compositor (opens a native window)
cargo run -p soma-compositor
```

> The login screen does not appear in `winit` mode — it goes straight to the compositor UI.

Or use the CLI test client to interact with the agent directly:

```bash
cargo run -p soma-cli
```

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

---

## Iterative Development

**Fastest — SSH into the running UTM VM:**

First, ensure the VM has NAT networking and SSH is running. Then:

```bash
./build.sh --arch=aarch64 --rust-only

scp buildroot/overlay/usr/bin/soma-compositor root@<VM-IP>:/usr/bin/
scp buildroot/overlay/usr/bin/soma-agent root@<VM-IP>:/usr/bin/

ssh root@<VM-IP> "systemctl restart soma-agent soma-compositor"
```

**Full image rebuild** (only needed when changing defconfig, post-build.sh, or systemd units):

```bash
./build.sh --arch=aarch64 --rust-only && ./build.sh --arch=aarch64 --image-only
```

Then detach the old disk in UTM and re-attach the updated `soma-os.img`.

---

## Changing the Login Password

```bash
echo "yournewpassword" > buildroot/overlay/etc/soma/passwd
./build.sh --arch=aarch64 --image-only
```

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Black screen after boot | Wrong display card | UTM Display → set **virtio-gpu-gl** |
| `No DRM card found` in journal | GPU not exposed | Ensure virtio-gpu-gl is selected, not VGA |
| Keyboard not working at login | evdev device not found | Check `ls /dev/input/` in console — should have `event0`, `event1` |
| Agent tasks fail with "model not found" | First-boot pull incomplete | `journalctl -f -u soma-first-boot` |
| `build.sh: Permission denied` | Script not executable | `chmod +x buildroot/build.sh buildroot/post-build.sh` |
| Docker not found | Docker Desktop not running | Start Docker Desktop and wait for it to be ready |
| soma-compositor crashes | Service error | `journalctl -u soma-compositor -n 50` |

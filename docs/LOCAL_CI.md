# SomaOS — Running CI Locally (No GitHub Actions Needed)

When GitHub Actions minutes are exhausted, this guide replicates the full CI pipeline on your local machine — both **Mac (Apple Silicon or Intel)** and **Windows (WSL2)** — and explains how to get the resulting images into a VM for testing.

The CI builds two images per commit: `x86_64` and `aarch64`. You can build one or both depending on what you're testing.

---

## What the CI Actually Does (simplified)

```
./build.sh --rust-only --arch=<arch>   # cross-compile Rust binaries in Docker
./build.sh --image-only --arch=<arch>  # build full OS image via Buildroot in Docker
gzip -c soma-os.img > soma-os.img.gz   # compress for upload
```

That's it. Everything runs inside Docker — no native Linux toolchain needed on your host.

---

## Prerequisites

### Mac (Apple Silicon or Intel)

- **Docker Desktop** for Mac — running (`docker run hello-world` should work)
- **Git**
- For running the image:
  - **Apple Silicon**: [UTM](https://mac.getutm.app) (free) — native ARM64, near-native speed
  - **Intel Mac**: QEMU (`brew install qemu`) for x86_64; UTM also works

### Windows

- **WSL2** with a Linux distro (Ubuntu 22.04 recommended)
- **Docker Desktop** with WSL2 integration enabled:
  - Docker Desktop → Settings → Resources → WSL Integration → enable your distro
- **Git** (inside WSL2)
- **VirtualBox** on the Windows host (for running the image)

---

## Step 1 — Get the Code

**Mac:**
```bash
git clone https://github.com/avsribhas-svg/SomaOS.git
cd SomaOS
chmod +x buildroot/build.sh buildroot/post-build.sh
```

**Windows (inside WSL2):**
```bash
git clone https://github.com/avsribhas-svg/SomaOS.git
cd SomaOS
chmod +x buildroot/build.sh buildroot/post-build.sh
```

If you already have the repo cloned, just `git pull`.

---

## Step 2 — Build the Image

Choose your target architecture.

### Option A — aarch64 (ARM64)

Best for: Apple Silicon Mac (native speed in UTM). Also works on Windows (builds via QEMU emulation inside Docker — slower).

```bash
cd buildroot
./build.sh --arch=aarch64
```

Output: `buildroot/output/aarch64/soma-os.img` + `buildroot/output/aarch64/kernel`

### Option B — x86_64

Best for: Windows (VirtualBox) or Intel Mac (QEMU). Also works on Apple Silicon Mac (Docker handles cross-compilation; ~1.5× slower than native but works fine).

```bash
cd buildroot
./build.sh --arch=x86_64
```

Output: `buildroot/output/x86_64/soma-os.img` + `buildroot/output/x86_64/kernel`

---

### Build Times

| Stage | First build | Subsequent (Rust changed) | Subsequent (no Rust change) |
|-------|------------|--------------------------|------------------------------|
| `--rust-only` | ~5 min | ~2–5 min | ~30 sec (cache hit) |
| `--image-only` | ~35 min | ~5 min (Buildroot cached) | ~5 min |
| Combined | ~40 min | ~7–10 min | ~5 min |

**Tip**: When you only changed Rust code, split the build:

```bash
./build.sh --arch=x86_64 --rust-only    # recompile Rust only (~2 min)
./build.sh --arch=x86_64 --image-only   # rebuild image with new binaries (~5 min)
```

---

## Step 3 — Run the Image

### Mac — Apple Silicon — aarch64 in UTM

1. Open UTM → **+** → **Virtualize** → **Linux**
2. **Linux** screen:
   - **Kernel Image**: `buildroot/output/aarch64/kernel`
   - **Boot arguments**: `root=/dev/vda rw console=tty0 console=ttyAMA0`
   - Leave initramfs blank
3. **Hardware**: Memory 4096 MB, CPU Cores 2
4. **Storage**: skip for now → Save
5. VM settings → **Drives** → **+** → **Import Drive** → select `buildroot/output/aarch64/soma-os.img` → Interface: **VirtIO**
6. VM settings → **Display** → Display Card: **virtio-gpu-gl**, Resolution: 1280×720
7. **Play**

Login: `soma` → Enter.

> To update without rebuilding the full image, SSH into the VM and copy binaries directly — see [Iterative Development](#iterative-development-no-full-rebuild) below.

---

### Mac — Intel — x86_64 in QEMU

```bash
qemu-system-x86_64 \
  -m 4G \
  -smp 2 \
  -drive file=buildroot/output/x86_64/soma-os.img,if=virtio,format=raw \
  -device virtio-vga \
  -display sdl \
  -net nic -net user
```

Login: `soma` → Enter.

---

### Mac — Apple Silicon — x86_64 in QEMU (cross-arch, slower)

If you need to test the x86_64 image on Apple Silicon:

```bash
brew install qemu   # if not already installed

qemu-system-x86_64 \
  -m 4G \
  -smp 2 \
  -drive file=buildroot/output/x86_64/soma-os.img,if=virtio,format=raw \
  -device virtio-vga \
  -display sdl \
  -net nic -net user
```

> This emulates x86_64 on ARM — boots and runs correctly but ~3–5× slower than native. Fine for smoke-testing; use UTM + aarch64 for everyday dev.

---

### Windows — x86_64 in VirtualBox

**Inside WSL2** — copy the image to Windows Desktop:

```bash
cp buildroot/output/x86_64/soma-os.img /mnt/c/Users/$USER/Desktop/soma-os.img
```

**In PowerShell** (not WSL2) — convert to VDI:

```powershell
cd "$env:USERPROFILE\Desktop"
& "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe" convertfromraw soma-os.img soma-os.vdi --format VDI
```

**Create VM in VirtualBox:**

VirtualBox → New:

| Field | Value |
|-------|-------|
| Name | SomaOS |
| Type | Linux |
| Version | Other Linux (64-bit) |
| Memory | 4096 MB |
| Hard disk | Use existing → `soma-os.vdi` |

Before booting, open Settings:

| Tab | Setting | Value |
|-----|---------|-------|
| Display | Graphics Controller | **VMSVGA** |
| Display | Video Memory | 128 MB |
| Display | 3D Acceleration | ✓ |
| System → Processor | Processors | 2 |
| Network → Adapter 1 | Attached to | NAT |

Click **Start**. Login: `soma` → Enter.

> VMSVGA is required — it exposes `/dev/dri/card0` via the `vmwgfx` driver. VBoxVGA does not work with DRM/KMS.

---

## Iterative Development — No Full Rebuild

For code-change → test cycles, skip the full image rebuild. SSH binaries directly into the running VM:

**1. Find the VM's IP address** (in the VM console):
```bash
ip addr show
```

**2. Recompile Rust only** (on your host):
```bash
# Mac:
cd buildroot && ./build.sh --arch=aarch64 --rust-only

# Windows (WSL2):
cd buildroot && ./build.sh --arch=x86_64 --rust-only
```

**3. Copy binaries and restart services:**
```bash
scp buildroot/overlay/usr/bin/soma-compositor root@<VM-IP>:/usr/bin/
scp buildroot/overlay/usr/bin/soma-agent root@<VM-IP>:/usr/bin/

ssh root@<VM-IP> "systemctl restart soma-agent soma-compositor"
```

Total turnaround: ~2–3 minutes from code change to running in VM.

---

## Pre-Commit Checks (Replaces CI Lint Step)

Before committing, run these locally to catch cross-compile issues:

```bash
# Native build (quick sanity check)
cargo build -p soma-agent
cargo build -p soma-compositor

# Cross-compile check — what CI actually validates
cargo check -p soma-agent --target x86_64-unknown-linux-musl
cargo check -p soma-agent --target aarch64-unknown-linux-musl
cargo check -p soma-compositor --features drm-backend --target x86_64-unknown-linux-musl
cargo check -p soma-compositor --features drm-backend --target aarch64-unknown-linux-musl
```

**Required targets** (install once):
```bash
rustup target add x86_64-unknown-linux-musl
rustup target add aarch64-unknown-linux-musl

# macOS also needs the musl linker:
brew install filosottile/musl-cross/musl-cross
```

Then add to `~/.cargo/config.toml`:
```toml
[target.x86_64-unknown-linux-musl]
linker = "x86_64-linux-musl-gcc"

[target.aarch64-unknown-linux-musl]
linker = "aarch64-linux-musl-gcc"
```

> On Windows (WSL2), `musl-gcc` is already available inside the Docker builder. The `cargo check` commands above work natively in WSL2 if the musl targets are installed.

---

## Producing a Compressed Image (Mirrors CI Artifact)

If you want the same compressed output that CI uploads:

```bash
gzip -c buildroot/output/x86_64/soma-os.img > buildroot/output/x86_64/soma-os.img.gz
gzip -c buildroot/output/aarch64/soma-os.img > buildroot/output/aarch64/soma-os.img.gz
```

To share with someone: send them `soma-os.img.gz` + the matching `kernel` file. They decompress and run:

```bash
gunzip soma-os.img.gz
# then follow the UTM / QEMU / VirtualBox steps above
```

---

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `build.sh: Permission denied` | `chmod +x buildroot/build.sh buildroot/post-build.sh` |
| `docker: command not found` (WSL2) | Docker Desktop → Settings → Resources → WSL Integration → enable your distro |
| `VBoxManage not found` in WSL2 | Run `VBoxManage.exe` from PowerShell, not WSL2 |
| Black screen in VirtualBox | Display → Graphics Controller must be **VMSVGA**, not VBoxVGA |
| Black screen in UTM | Display Card must be **virtio-gpu-gl** |
| `cargo check` fails with linker error | Install musl-cross (`brew install filosottile/musl-cross/musl-cross`) and add `~/.cargo/config.toml` entries above |
| Agent fails "model not found" | First boot only: `journalctl -f -u soma-first-boot` to watch the pull |
| `No DRM card found` in VM | Wrong graphics controller — see above |
| SSH to VM fails | Ensure NAT adapter is set; `systemctl status sshd` inside VM |

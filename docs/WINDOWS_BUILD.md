# Building SomaOS on Windows

Step-by-step guide to build and run SomaOS on your Windows laptop using WSL2 + Docker.

---

## Prerequisites

### 1. Install WSL2

Open **PowerShell as Administrator** and run:
```powershell
wsl --install
```
Restart your computer when prompted. After restart, WSL will finish setup and ask you to create a Linux username/password.

### 2. Install Docker Desktop

1. Download from [docker.com/products/docker-desktop](https://www.docker.com/products/docker-desktop/)
2. Install and launch Docker Desktop
3. In Settings → General → ensure **"Use the WSL 2 based engine"** is checked
4. In Settings → Resources → WSL Integration → enable for your distro (Ubuntu)

### 3. Install VirtualBox (for running the image)

Download from [virtualbox.org](https://www.virtualbox.org/wiki/Downloads) → "Windows hosts"

---

## Build the Image

### Clone the repo

Open **WSL2 terminal** (search "Ubuntu" in Start menu):

```bash
cd ~
git clone https://github.com/<your-username>/somaos.git
cd somaos
```

### Build (one command)

```bash
cd buildroot
./build.sh
```

This will:
1. **Cross-compile** the Rust binaries (`soma-agent`, `soma-compositor`) inside Docker
2. **Build** the full Linux image via Buildroot (~30 min on native x86_64)
3. **Output** the image to `buildroot/output/soma-os.img`

> **Note**: The first build downloads ~1GB of source tarballs (GCC, kernel, glibc, Mesa, etc.). 
> Subsequent builds reuse the cache and are much faster.

---

## Run in VirtualBox

### Convert the image

In WSL2:
```bash
# Convert raw image to VDI format for VirtualBox
# First, find where VirtualBox is installed
VBoxManage="/mnt/c/Program Files/Oracle/VirtualBox/VBoxManage.exe"

# Copy the image to a Windows-accessible location
cp buildroot/output/soma-os.img /mnt/c/Users/$USER/Desktop/soma-os.img

# Convert (run from PowerShell or CMD instead)
```

In **PowerShell**:
```powershell
cd "$env:USERPROFILE\Desktop"
& "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe" convertfromraw soma-os.img soma-os.vdi --format VDI
```

### Create the VM

1. Open **VirtualBox** → click **New**
2. **Name**: SomaOS
3. **Type**: Linux
4. **Version**: Other Linux (64-bit)
5. Click **Next**

6. **Memory**: 2048 MB (2 GB minimum)
7. **Processors**: 2

8. **Hard disk**: "Use an existing virtual hard disk file"
   → Browse → select `soma-os.vdi` from your Desktop

9. Click **Finish**

### Configure the VM

Right-click SomaOS → **Settings**:

| Tab | Setting | Value |
|-----|---------|-------|
| **System → Processor** | Processors | 2 |
| **Display** | Video Memory | 128 MB |
| **Display** | Graphics Controller | VMSVGA |
| **Network** | Attached to | NAT |
| **Serial Ports → Port 1** | Enable | ✓ |
| **Serial Ports → Port 1** | Port Mode | Raw File |
| **Serial Ports → Port 1** | Path | `C:\Users\<you>\soma-serial.log` |

### Boot

Click **Start**. You should see:
1. GRUB bootloader (auto-selects after 3 seconds)
2. Linux kernel boot messages
3. systemd startup
4. Auto-login as root
5. The SomaOS welcome banner:

```
  ╔══════════════════════════════════════════╗
  ║         Welcome to SomaOS v0.1.0        ║
  ║                                          ║
  ║  AI-Native Operating System for Agents   ║
  ║                                          ║
  ║  • soma-agent: running as systemd svc    ║
  ║  • To start compositor: soma-compositor  ║
  ║                                          ║
  ╚══════════════════════════════════════════╝
```

### First commands

```bash
# Check the agent daemon is running
systemctl status soma-agent

# Start the compositor (requires DRM/KMS — may need framebuffer mode)
soma-compositor

# Or use the terminal directly
ls /
uname -a
```

---

## Alternative: Run in QEMU (Windows)

If you prefer QEMU over VirtualBox:

1. Install QEMU: [qemu.weilnetz.de](https://qemu.weilnetz.de/w64/)
2. In PowerShell:
```powershell
qemu-system-x86_64.exe -m 2G -smp 2 `
  -drive file=soma-os.img,format=raw `
  -device virtio-gpu-pci `
  -device virtio-keyboard-pci `
  -device virtio-mouse-pci `
  -display sdl `
  -serial stdio `
  -net nic -net user
```

---

## Troubleshooting

| Issue | Fix |
|-------|-----|
| `build.sh: Permission denied` | Run `chmod +x buildroot/build.sh buildroot/post-build.sh` |
| Docker not found in WSL2 | Ensure Docker Desktop → Settings → WSL Integration is enabled |
| VirtualBox black screen | Try VBoxSVGA or VMSVGA graphics controller |
| No network in VM | Use NAT adapter; run `dhcpcd` after boot |
| Compositor doesn't start | Expected in v0.1 — requires DRM device. Use the CLI shell. |

---

## Development Workflow

After initial setup, your workflow is:

```
Edit code (any machine)
  → git push
    → Windows WSL2: git pull && cd buildroot && ./build.sh
      → VirtualBox: boot soma-os.vdi
```

For rapid iteration, you can also cross-compile just the Rust binaries and scp them into the running VM:

```bash
# In WSL2
./buildroot/build.sh --rust-only
scp buildroot/overlay/usr/bin/soma-* root@<VM-IP>:/usr/bin/
```

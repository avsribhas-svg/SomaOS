<p align="center">
  <img src="docs/assets/soma-logo.png" alt="SomaOS" width="120" />
</p>

<h1 align="center">SomaOS</h1>

<p align="center">
  <strong>An AI-Native Operating System Built from the Ground Up for Autonomous Agents</strong>
</p>

<p align="center">
  <a href="#architecture">Architecture</a> ·
  <a href="#capabilities">Capabilities</a> ·
  <a href="#getting-started">Getting Started</a> ·
  <a href="#hardware-requirements">Hardware</a> ·
  <a href="#roadmap">Roadmap</a>
</p>

---

## Abstract

SomaOS is a purpose-built Linux distribution designed as the native execution environment for AI agents. Unlike conventional desktop operating systems retrofitted with AI assistants, SomaOS inverts the paradigm: **the agent is the primary user of the system, with humans serving as supervisors through a structured approval interface.**

The system provides:
- A **custom DRM/KMS compositor** that renders directly to GPU framebuffer — no X11 or Wayland server required
- A **login screen** that boots straight into Soma, with no traditional desktop
- An **agent daemon** with 29 structured capability actions across 5 modules
- **On-device LLM** (qwen2.5-coder:7b via Ollama) with a three-layer intent pipeline for robust natural language understanding
- A **Human-in-the-Loop (HITL) approval system** enforcing mandatory human review before any action
- **Conversation memory** — the agent remembers recent exchanges for follow-up commands
- **Rich result cards** — image thumbnails, line-numbered text previews, directory tree views
- **Auto-start services** — Ollama + agent + compositor all start on boot via systemd
- A **minimal Linux image** built with Buildroot, targeting x86_64 hardware

---

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                      SomaOS Image                        │
│  ┌───────────────────────────────────────────────────┐   │
│  │                    systemd                        │   │
│  │  ┌─────────────────┐  ┌────────────────────────┐  │   │
│  │  │  soma-ollama    │  │   soma-compositor      │  │   │
│  │  │  (LLM server)   │  │   (DRM/KMS backend)    │  │   │
│  │  └────────┬────────┘  │                        │  │   │
│  │           │           │  ┌──────────────────┐  │  │   │
│  │  ┌────────┴────────┐  │  │  Login Screen    │  │  │   │
│  │  │  soma-agent     │◄─┤  ├──────────────────┤  │  │   │
│  │  │  (daemon)       │  │  │  Chat Sidebar    │  │  │   │
│  │  │                 │  │  │  (right panel)   │  │  │   │
│  │  │ ┌─────────────┐ │  │  ├──────────────────┤  │  │   │
│  │  │ │Capabilities │ │  │  │  PTY Terminal    │  │  │   │
│  │  │ │ ├─filesystem│ │  │  │  (left panel)    │  │  │   │
│  │  │ │ ├─process   │ │  │  ├──────────────────┤  │  │   │
│  │  │ │ ├─system    │ │  │  │  HITL Overlay    │  │  │   │
│  │  │ │ ├─network   │ │  │  └──────────────────┘  │  │   │
│  │  │ │ └─package   │ │  │                        │  │   │
│  │  │ └─────────────┘ │  │  evdev input           │  │   │
│  │  └─────────────────┘  │  /dev/dri/card0        │  │   │
│  │                       └────────────────────────┘  │   │
│  └───────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────┘
```

### System Components

| Component | Role | Technology |
|-----------|------|------------|
| **soma-common** | Shared types, IPC protocol, capability types | Rust, serde |
| **soma-agent** | Intent parsing, capability execution, conversation context | Rust, reqwest, tokio |
| **soma-compositor** | DRM/KMS display, chat UI, PTY terminal, login screen | Rust, drm, evdev, tiny-skia, cosmic-text |
| **soma-cli** | Terminal test client for agent interaction | Rust, tokio |
| **Buildroot Image** | Minimal Linux rootfs, bootloader, systemd services | Buildroot, GRUB2, systemd |

### Data Flow

```
User types in sidebar
        │
        ▼
  Compositor sends NaturalLanguageInput
  (Unix Socket: /tmp/soma-agent.sock)
        │
        ▼
  Agent: Layer 0 — Rust keyword preprocessor
    (direct-maps obvious commands before hitting LLM)
        │
        ▼
  Agent: Layer 1 — Free-text LLM parse
    Ollama qwen2.5-coder:7b → intent + capability hint
        │
        ▼
  Agent: Layer 2 — Structured JSON planner
    LLM generates full TaskPlan JSON
        │
        ▼
  TaskPlanReady → Compositor shows HITL approval modal
        │
        ▼
  User approves (Enter) or rejects (Esc)
        │
        ▼
  Agent executes via CapabilityRegistry
    │  StepResult (incremental)
    │  ExecutionComplete (final, with structured data)
    ▼
  Results displayed in chat sidebar
    → Text files: line numbers + file stats
    → Images: decoded thumbnail rendered inline
    → Directories: tree view with sizes
```

---

## Capabilities

The agent executes actions through a structured **capability system** — not raw shell commands. Each capability is a Rust module that validates parameters and returns structured JSON data.

### Filesystem (9 actions)

| Action | Description | Risk |
|--------|-------------|------|
| `list_dir` | List directory contents with sizes | Low |
| `read_file` | Read file (text with line count; images as base64 thumbnail) | Low |
| `write_file` | Write/create a file | Medium |
| `create_dir` | Create a directory | Medium |
| `delete` | Delete a file or directory | High |
| `copy` | Copy file | Medium |
| `move_item` | Move/rename file | Medium |
| `find` | Search for files by glob pattern | Low |
| `file_info` | Get file metadata | Low |

All paths support `~` expansion.

### Process (5 actions)

| Action | Description | Risk |
|--------|-------------|------|
| `list_processes` | List running processes | Low |
| `kill` | Kill a process by PID | High |
| `service_status` | Check systemd service status | Low |
| `service_start` | Start a service | Medium |
| `service_stop` | Stop a service | Medium |

### System (6 actions)

| Action | Description | Risk |
|--------|-------------|------|
| `hostname` | Get system hostname | Low |
| `uptime` | Get system uptime | Low |
| `disk_usage` | Check disk space | Low |
| `memory_info` | Show memory usage | Low |
| `cpu_info` | Show CPU information | Low |
| `network_status` | Check network connectivity | Low |

### Network (5 actions)

| Action | Description | Risk |
|--------|-------------|------|
| `ping` | Ping a host | Low |
| `dns_lookup` | Resolve hostname to IP | Low |
| `curl` | Make HTTP request | Low |
| `ifconfig` | List network interfaces | Low |
| `port_check` | Check if TCP port is open | Low |

### Package (4 actions)

| Action | Description | Risk |
|--------|-------------|------|
| `list_installed` | List installed packages | Low |
| `search` | Search available packages | Low |
| `install` | Install a package | Medium |
| `remove` | Remove a package | High |

Auto-detects package manager: `brew`, `apt`, `apk`, `dnf`, `pacman`.

---

## Software Specifications

### Intent Pipeline (Three-Layer)

```
Input: "show me what's in Downloads"
  │
  ▼
Layer 0 — Rust keyword preprocessor
  Matches known patterns: list/show/ls → filesystem.list_dir
  Resolves: "Downloads" → ~/Downloads (expand tilde)
  If matched: skip LLM entirely → instant response
  │
  ▼ (if no keyword match)
Layer 1 — Free-text LLM parse
  Prompt: "What capability and action does this request use?"
  Response: { "capability": "filesystem", "action": "list_dir", "hint": "..." }
  │
  ▼
Layer 2 — Structured JSON planner
  Prompt: "Generate a TaskPlan JSON for: <input> using <capability>.<action>"
  Response: full TaskPlan with steps, params, risk_level
```

### IPC Protocol

Communication uses **newline-delimited JSON** over a **Unix domain socket** (`/tmp/soma-agent.sock`).

#### Compositor → Agent Messages

| Message Type | Fields | Description |
|-------------|--------|-------------|
| `NaturalLanguageInput` | `text` | Natural language command |
| `Approve` | `id` | User approved a pending task plan |
| `Reject` | `id` | User rejected a pending task plan |
| `DirectExec` | `id`, `command` | Execute a raw shell command |
| `Ping` | — | Health check |

#### Agent → Compositor Messages

| Message Type | Fields | Description |
|-------------|--------|-------------|
| `TaskPlanReady` | `id`, `plan` | LLM returned a structured plan |
| `StepResult` | `id`, `step_index`, `result` | One step completed |
| `ExecutionComplete` | `id`, `results` | All steps finished |
| `Error` | `id`, `message` | An error occurred |
| `DirectOutput` | `id`, `result` | Terminal command output |
| `Pong` | — | Health check response |

### Rendering Pipeline

```
DRM/KMS main loop (bare metal) OR winit event loop (dev)
  → tiny-skia Pixmap (software rasterization)
    → Login screen (if not yet authenticated)
      OR
    → Terminal panel (left, variable width, PTY)
    → Chat Sidebar panel (right, 380px)
      → Title bar + status pill
      → Scrollable message history
        → User bubbles
        → Plan cards (with capability steps)
        → Result cards
          → Image thumbnail (base64 decode → premul RGBA → draw_pixmap)
          → Text file (line numbers + file stats, 15 lines)
          → Directory tree (|-- connectors, sizes, dirs first)
        → Error cards (click to expand detail modal)
      → Input field + send button
    → HITL approval overlay (modal, centered)
    → Detail modal (click any result/error card)
    → Toast notifications (top-right, fade out)
  → DRM dumb buffer blit + page flip
     OR softbuffer Surface (winit dev mode)
```

Text rendering via **cosmic-text** with Unicode shaping.

### Conversation Context

The agent maintains a per-client conversation history (last 5 exchanges):

```
User: "list files in /tmp"           → filesystem.list_dir {path: "/tmp"}
User: "now show /var"                → filesystem.list_dir {path: "/var"}
User: "delete the boot folder there" → filesystem.delete {path: "/var/boot"}
```

### Risk Classification

| Level | Criteria | Example Actions |
|-------|----------|-----------------|
| **Low** | Read-only operations | `list_dir`, `hostname`, `ping` |
| **Medium** | Create/modify operations | `write_file`, `create_dir`, `install` |
| **High** | Destructive operations | `delete`, `kill`, `remove` |

---

## Hardware Requirements

### Minimum (VM Testing)

| Component | Requirement |
|-----------|-------------|
| **CPU** | x86_64, 2 cores |
| **RAM** | 4 GB (OS + LLM inference) |
| **Disk** | 10 GB (OS + Ollama models) |
| **GPU** | DRM/KMS capable (virtio-gpu, VMSVGA, or real GPU) |

### Recommended (Physical Hardware)

| Component | Requirement |
|-----------|-------------|
| **CPU** | x86_64, 4+ cores (Intel i5/Ryzen 5+) |
| **RAM** | 16 GB (4 GB OS + 8 GB LLM + headroom) |
| **Disk** | 30 GB SSD |
| **GPU** | Any DRM/KMS-capable GPU |

---

## Getting Started

### Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| **Rust** | 1.80+ | Compile soma crates |
| **Ollama** | Latest | Local LLM inference |
| **Docker** | 20+ | Build the Buildroot image |

### Dev Mode (macOS/Linux — winit window)

Run the winit backend for fast iteration. No VM needed.

```bash
# 1. Clone
git clone https://github.com/avsribhas-svg/SomaOS.git && cd SomaOS

# 2. Install and start Ollama
ollama pull qwen2.5-coder:7b
ollama serve &

# 3. Start the agent daemon
cargo run -p soma-agent

# 4. In another terminal, start the compositor
cargo run -p soma-compositor
# Opens a window — login screen does NOT appear in winit mode

# 5. Or use the CLI test client
cargo run -p soma-cli
```

### VM Build (Full OS Image — DRM/KMS + login screen)

```bash
# Cross-compile Rust binaries + build full OS image
cd buildroot
./build.sh

# Output: buildroot/output/soma-os.img

# Convert for VirtualBox (run in WSL2 or Ubuntu terminal)
VBoxManage convertfromraw buildroot/output/soma-os.img soma-os.vdi --format VDI

# Or boot directly in QEMU
qemu-system-x86_64 -m 4G -smp 2 \
  -drive file=buildroot/output/soma-os.img,if=virtio,format=raw \
  -device virtio-vga -display sdl
```

See [docs/WINDOWS_BUILD.md](docs/WINDOWS_BUILD.md) for the full VirtualBox setup guide.

---

## Project Structure

```
Native OS Project/
├── Cargo.toml                          # Workspace root
│
├── soma-common/                        # Shared library crate
│   └── src/lib.rs                      # Types: TaskPlan, CapabilityResult, IPC messages
│
├── soma-agent/                         # Agent daemon binary
│   └── src/
│       ├── main.rs                     # Entry point
│       ├── intent.rs                   # Three-layer intent pipeline (keyword → LLM → JSON)
│       ├── executor.rs                 # Capability dispatch
│       ├── ipc.rs                      # Unix socket server + conversation context
│       └── capabilities/
│           ├── mod.rs                  # Capability trait + registry
│           ├── filesystem.rs           # 9 actions (read_file supports image base64, ~ expansion)
│           ├── process.rs              # 5 process management actions
│           ├── system.rs               # 6 system info actions
│           ├── network.rs              # 5 network diagnostic actions
│           └── package.rs              # 4 package management actions
│
├── soma-compositor/                    # Compositor binary
│   └── src/
│       ├── main.rs                     # Backend selection, DRM main loop, winit event loop
│       ├── login.rs                    # Full-screen login screen (reads /etc/soma/passwd)
│       ├── renderer.rs                 # tiny-skia + cosmic-text renderer
│       ├── sidebar.rs                  # Chat UI, result cards, image thumbnails, HITL overlay
│       ├── terminal.rs                 # PTY terminal emulator
│       ├── ipc_client.rs               # Agent daemon connection
│       ├── backend/
│       │   ├── mod.rs                  # InputEvent types (KeyCode, MouseBtn)
│       │   └── drm.rs                  # DRM/KMS: open card, dumb buffer, page flip
│       └── input/
│           ├── mod.rs                  # Input module
│           └── evdev_input.rs          # evdev keyboard + mouse (/dev/input/event*)
│
├── soma-cli/                           # CLI test client
│   └── src/main.rs
│
├── docs/
│   └── WINDOWS_BUILD.md               # Full VirtualBox setup guide for Windows
│
└── buildroot/                          # OS image build system
    ├── Dockerfile                      # Build environment (Ubuntu 22.04 + musl + Buildroot)
    ├── soma_defconfig                  # Buildroot config (DRM, evdev, ALSA, espeak-ng)
    ├── post-build.sh                   # Rootfs setup (services, passwd, first-boot script)
    ├── build.sh                        # Build pipeline (cross-compile DRM binary + image)
    └── overlay/
        ├── usr/bin/                    # soma-agent, soma-compositor, soma-cli binaries
        └── etc/systemd/system/
            ├── soma-ollama.service     # Ollama LLM server
            ├── soma-agent.service      # Agent daemon (after Ollama)
            ├── soma-compositor.service # Compositor on tty1 (after agent)
            └── soma-first-boot.service # One-shot: pulls qwen2.5-coder:7b on first boot
```

---

## Roadmap

### v0.1 — Foundation ✅
- [x] Rust workspace with 3 crates
- [x] Software-rendered compositor (winit + tiny-skia)
- [x] Agent daemon with Ollama integration
- [x] HITL approval modal
- [x] Buildroot image pipeline

### v0.2 — Capability System ✅
- [x] Structured capability modules (filesystem, process, system)
- [x] Capability registry with dynamic LLM prompt generation
- [x] soma-cli test client
- [x] musl static linking for cross-compilation

### v0.3 — Compositor Chat UI ✅
- [x] Chat-style sidebar with user bubbles and agent cards
- [x] Agent connection from compositor

### v0.4 — UI Polish ✅
- [x] Right-side sidebar layout
- [x] Scrollable message history
- [x] Rich result cards
- [x] Agent-triggered redraws

### v0.5 — Smarter Agent ✅
- [x] Network + package capabilities
- [x] Conversation context memory (5 exchanges)
- [x] LLM: qwen2.5-coder:7b
- [x] Few-shot examples in system prompt

### v0.6 — Rich Compositor ✅
- [x] Real PTY terminal (nix, connected to live shell)
- [x] Click-to-focus panels
- [x] Resizable panels (drag divider)
- [x] Toast notifications
- [x] Trackpad scroll

### v0.7 — Multimodal Previews ✅
- [x] Image thumbnails in result cards (base64 IPC → tiny-skia draw_pixmap)
- [x] Text file preview with line numbers and file stats
- [x] Directory tree view with sizes
- [x] Tilde expansion for all filesystem paths
- [x] Clickable error/result cards with detail modal
- [x] Three-layer intent pipeline (keyword preprocessor → LLM parse → JSON planner)

### v0.8 — VM Production Boot ✅
- [x] DRM/KMS bare-metal backend (drm crate, dumb buffer, double-buffering)
- [x] evdev input (keyboard + mouse direct from /dev/input)
- [x] Login screen (boots straight into Soma, reads /etc/soma/passwd)
- [x] Feature-gated builds: `winit-backend` (dev) / `drm-backend` (production)
- [x] soma-ollama.service auto-start
- [x] soma-first-boot.service (one-shot model pull on first boot)
- [x] 4 GB rootfs, ALSA + espeak-ng in Buildroot image
- [x] build.sh updated for DRM feature flag + VDI output instructions

### v1.0 — Ship It
- [ ] USB bootable installer image
- [ ] GPU passthrough for QEMU (faster LLM inference)
- [ ] Plugin API for third-party capabilities
- [ ] Security hardening (seccomp, AppArmor, audit log)
- [ ] OTA updates

---

## Design Decisions

### Why a custom compositor instead of GNOME/KDE?

Traditional desktops are designed for mouse-and-keyboard humans. For an AI agent:
- **The HITL modal is a first-class OS primitive** — rendered by the compositor itself, cannot be bypassed
- **Minimal footprint** — SomaOS boots in seconds with ~100 MB RAM baseline
- **The chat sidebar is the primary interface**, not an afterthought
- **DRM/KMS direct rendering** — no Wayland/X11 server needed, nothing between the agent and the display

### Why a capability system instead of raw shell commands?

- **Type safety** — Each action validates parameters before execution
- **Structured output** — Results are JSON, enabling rich UI (thumbnails, trees, line numbers)
- **Extensibility** — New capabilities are just Rust structs implementing a trait
- **Auditability** — Every action logged with capability, action, and parameters

### Why a three-layer intent pipeline?

A single LLM call for every command is slow and unreliable for common patterns:
- **Layer 0** (Rust keywords) handles ~60% of commands instantly with zero LLM latency
- **Layer 1** (free-text parse) identifies the right capability module without generating full JSON
- **Layer 2** (JSON planner) generates the complete TaskPlan only once the capability is known

### Why local LLM (Ollama)?

- **Latency** — No network round-trips
- **Privacy** — Commands and file contents never leave the machine
- **Reliability** — No internet dependency during operation
- **Cost** — No per-token billing

### Why Rust?

- **Memory safety without GC** — Critical for a compositor and system daemon
- **Single static binary** — musl target, no runtime dependencies in the OS image
- **Direct DRM/evdev access** — Rust crates wrap kernel ioctls cleanly
- **Cross-compilation** — `cargo build --target x86_64-unknown-linux-musl`

---

## Security Model

```
┌────────────────────────────────────────────┐
│              Human Operator                │
│         (physical keyboard access)         │
└───────────────────┬────────────────────────┘
                    │ Login (soma-compositor)
                    ▼
┌────────────────────────────────────────────┐
│            Login Screen                    │
│  Password checked against /etc/soma/passwd │
│  No compositor UI until authenticated.     │
└───────────────────┬────────────────────────┘
                    │ Approve / Reject
                    ▼
┌────────────────────────────────────────────┐
│            HITL Gate (Compositor)          │
│  Every plan must be explicitly approved.  │
│  No action executes without human consent.│
└───────────────────┬────────────────────────┘
                    │
                    ▼
┌────────────────────────────────────────────┐
│       Capability Registry (Agent)          │
│  Only registered capability actions run.  │
│  Parameters validated per action schema.  │
│  Structured JSON results, not raw output. │
└───────────────────┬────────────────────────┘
                    │
                    ▼
┌────────────────────────────────────────────┐
│           OS-Level Isolation               │
│  (Future: namespaces, seccomp, AppArmor)  │
└────────────────────────────────────────────┘
```

**Key invariant**: No LLM output results in action execution without passing through the login gate, the HITL approval gate, and the capability registry.

---

## License

MIT

---

<p align="center">
  <sub>SomaOS — Where the agent is the interface.</sub>
</p>

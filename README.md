<p align="center">
  <img src="docs/assets/soma-logo.png" alt="SomaOS" width="120" />
</p>

<h1 align="center">SomaOS</h1>

<p align="center">
  <strong>An AI-Native Operating System Built from the Ground Up for Autonomous Agents</strong>
</p>

<p align="center">
  <a href="#vision">Vision</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="#capabilities">Capabilities</a> ·
  <a href="#getting-started">Getting Started</a> ·
  <a href="#roadmap">Roadmap</a>
</p>

---

## Vision

SomaOS is a purpose-built operating system that inverts the traditional computing paradigm: **the AI agent is the primary user of the system. Humans supervise through a structured approval interface.**

Traditional operating systems were designed around how humans interact with machines — through file managers, menus, and graphical applications built for manual navigation. SomaOS is designed bottom-up for how AI agents create and execute tasks. Every workflow, every interface primitive, and every application is architected to be agent-inspectable and agent-drivable by default.

This is a precedent system for future automation infrastructure. Instead of using programming as the medium to understand and control a machine, SomaOS uses AI agents as the native execution layer — with humans as supervisors, not operators.

**The long-term stack:**
- **Native Rust applications** (soma-sheets, soma-docs, soma-media, soma-canvas) where every piece of state is directly readable and writable by the agent via a shared `AgentAPI` trait
- **Embedded browser** (WebKitGTK offscreen) for web-shaped apps and existing web tooling, driven by the agent via DOM/JS bridge
- **Structured perception** — agents read app state directly; fall back to vision model (multimodal LLM) for unknown UIs
- **Federation** — each SomaOS node is autonomous; nodes can delegate tasks to each other over network IPC

---

## Abstract

Current state (v0.8): SomaOS runs as a bootable Linux image with a custom bare-metal compositor.

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

Communication uses **newline-delimited JSON** over a **Unix domain socket** (`/tmp/soma-agent.sock`). The protocol is transport-agnostic — designed to extend to TCP/TLS for federation in v1.2.

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
    → Chat Sidebar panel (right, 380px default, resizable)
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
git clone git@github.com:avsribhas-svg/SomaOS.git && cd SomaOS

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

### VM Build — Mac + VMware Fusion

```bash
# Prerequisites: Docker Desktop running, VMware Fusion installed
brew install qemu

# Build OS image (~40 min first time)
git clone git@github.com:avsribhas-svg/SomaOS.git && cd SomaOS
chmod +x buildroot/build.sh buildroot/post-build.sh
cd buildroot && ./build.sh

# Convert to VMDK
qemu-img convert -f raw -O vmdk buildroot/output/soma-os.img ~/Desktop/soma-os.vmdk
```

VMware Fusion → New → Custom VM → Linux (Other 64-bit) → Use existing disk → select `soma-os.vmdk`
Settings: 4096 MB RAM, 2 CPUs, NAT network, 3D acceleration on.

### VM Build — Windows (WSL2) + VirtualBox

```bash
# In WSL2 terminal
sudo apt install -y git qemu-utils
git clone git@github.com:avsribhas-svg/SomaOS.git && cd SomaOS
chmod +x buildroot/build.sh buildroot/post-build.sh
cd buildroot && ./build.sh

# Convert and copy to Windows Desktop
qemu-img convert -f raw -O vmdk \
  buildroot/output/soma-os.img \
  /mnt/c/Users/$USER/Desktop/soma-os.vmdk
```

VirtualBox → New → Linux (Other 64-bit, 64-bit) → 4096 MB → Use existing disk → `soma-os.vmdk`
Settings → Display: **VMSVGA**, 128 MB VRAM, 3D acceleration on.

See [docs/WINDOWS_BUILD.md](docs/WINDOWS_BUILD.md) for the full setup guide.

### Iterative Dev (after first build)

```bash
# Recompile Rust only (~2 min)
./build.sh --rust-only

# Rebuild image (~5 min, uses cached Buildroot)
./build.sh --image-only
```

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
│           ├── mod.rs                  # Input abstraction layer
│           └── evdev_input.rs          # evdev keyboard + mouse (/dev/input/event*)
│
├── soma-cli/                           # CLI test client
│   └── src/main.rs
│
├── docs/
│   └── WINDOWS_BUILD.md               # Full VirtualBox/VMware setup guide for Windows
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

### v0.9 — Browser Panel + Vision
- [ ] Embed WebKitGTK offscreen into compositor framebuffer (no Wayland/X11 needed)
- [ ] `browser` agent capability: navigate, query_selector, eval, screenshot_region
- [ ] Vision fallback: multimodal LLM (qwen2.5-vl or llava) for unknown UIs
- [ ] Basic tab management in compositor browser panel

### v1.0 — Native App Framework
- [ ] `AgentAPI` trait in soma-common: `describe_state`, `execute_action`, `subscribe_changes`
- [ ] `soma-sheets`: spreadsheet with full agent read/write API (cells, formulas, ranges)
- [ ] `soma-docs`: document editor with structured agent API (paragraphs, headings, tables)
- [ ] Compositor multi-panel layout (terminal | browser | native app | sidebar)
- [ ] App launcher driven by agent intent ("open a spreadsheet for this data")

### v1.1 — Media + Generation
- [ ] `soma-media`: image/video generation pipeline (local diffusion via Ollama or separate runtime)
- [ ] Media result cards in sidebar (video playback, image gallery)
- [ ] Agent capabilities: generate_image, generate_video, generate_audio

### v1.2 — Federation
- [ ] Network IPC: TCP/TLS transport wrapper over existing JSON socket protocol
- [ ] Node registry: agents discover peers via config or DNS-SD
- [ ] `delegate` capability: orchestrator sends task to remote node, streams results back
- [ ] Unified HITL queue: all node approvals aggregated on orchestrator node
- [ ] Node auth: token-based, scoped permissions per node

### v2.0 — USB Bootable + Plugin API
- [ ] USB installer (GRUB + EFI, fits on 8 GB USB)
- [ ] Plugin API: third-party capabilities as shared Rust dylibs or WASM modules
- [ ] Seccomp + AppArmor sandboxing per agent capability
- [ ] OTA update system (delta images, signed, agent-driven)

---

## Design Decisions

### Why a custom compositor instead of GNOME/KDE?

Traditional desktops are designed for mouse-and-keyboard humans. For an AI agent:
- **The HITL modal is a first-class OS primitive** — rendered by the compositor itself, cannot be bypassed
- **Minimal footprint** — SomaOS boots in seconds with ~100 MB RAM baseline
- **The chat sidebar is the primary interface**, not an afterthought
- **DRM/KMS direct rendering** — no Wayland/X11 server needed, nothing between the agent and the display
- **Future browser embedding** — WebKitGTK offscreen renders to an RGBA pixmap that composites into the existing DRM framebuffer; no architectural change needed

### Why Native Rust apps + embedded browser (not just a browser shell)?

Two categories of applications need two different models:
- **Native Rust apps** (soma-sheets, soma-docs, soma-canvas) expose a typed `AgentAPI` where the agent reads and writes state directly — no screen-scraping, no DOM parsing, zero ambiguity
- **Embedded browser** (WebKitGTK offscreen) handles web-shaped apps and existing web tooling via a DOM/JS bridge — the agent queries CSS selectors and evaluates JS
- Both produce RGBA pixmaps that the compositor composites identically — the boundary is only in how the agent *talks* to them

### Why a capability system instead of raw shell commands?

- **Type safety** — Each action validates parameters before execution
- **Structured output** — Results are JSON, enabling rich UI (thumbnails, trees, line numbers)
- **Extensibility** — New capabilities are just Rust structs implementing a trait
- **Auditability** — Every action logged with capability, action, and parameters
- **Future AgentAPI parity** — The same structured-data philosophy will extend to application-level APIs

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
- **Model-agnostic** — Any Ollama-compatible model can be swapped in; vision models (qwen2.5-vl) will be added alongside the task model in v0.9

### Why federation by design?

Each SomaOS node is autonomous — it runs its own agent, its own LLM, its own HITL queue. Federation is an additive transport layer: the existing Unix socket protocol (newline-delimited JSON) wraps in TCP/TLS and gains a node routing header. No protocol redesign, no breaking changes. This means a single-user workstation today can become an orchestrator node tomorrow without architectural rework.

### Why Rust?

- **Memory safety without GC** — Critical for a compositor and system daemon
- **Single static binary** — musl target, no runtime dependencies in the OS image
- **Direct DRM/evdev access** — Rust crates wrap kernel ioctls cleanly
- **Cross-compilation** — `cargo build --target x86_64-unknown-linux-musl`
- **`AgentAPI` as a Rust trait** — Applications that implement the trait get agent integration for free, with compiler-enforced interface contracts

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
  <sub>SomaOS — The machine, built for the agent.</sub>
</p>

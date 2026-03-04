<p align="center">
  <img src="docs/assets/soma-logo.png" alt="SomaOS" width="120" />
</p>

<h1 align="center">SomaOS</h1>

<p align="center">
  <strong>An AI-Native Operating System Built from the Ground Up for Autonomous Agents</strong>
</p>

<p align="center">
  <a href="#architecture">Architecture</a> ·
  <a href="#getting-started">Getting Started</a> ·
  <a href="#hardware-requirements">Hardware</a> ·
  <a href="#software-specifications">Software Specs</a> ·
  <a href="#roadmap">Roadmap</a>
</p>

---

## Abstract

SomaOS is a purpose-built Linux distribution designed as the native execution environment for AI agents. Unlike conventional desktop operating systems retrofitted with AI assistants, SomaOS inverts the paradigm: **the agent is the primary user of the system, with humans serving as supervisors through a structured approval interface.**

The system provides:
- A **custom Wayland compositor** with an integrated agent sidebar and terminal
- An **agent daemon** that translates natural language into executable task plans via local LLMs
- A **Human-in-the-Loop (HITL) approval system** that enforces mandatory human review before any command execution
- A **minimal Linux image** (< 1GB) built with Buildroot, running on commodity x86_64 hardware

---

## Problem Statement

Current AI integration in operating systems follows an "assistant" model — AI is bolted onto existing UIs as chatbots, copilots, or accessibility tools. This approach is fundamentally limited by:

1. **Lack of system-level access** — Assistants operate in sandboxed application contexts, unable to orchestrate cross-process workflows
2. **No structured approval** — When AI does take action, approval is either implicit (dangerous) or interruptive (unusable)
3. **Overhead** — Full desktop environments (GNOME, KDE, Windows Shell) carry hundreds of megabytes of UI infrastructure irrelevant to agent operation
4. **No audit trail** — Actions taken by AI are not systematically logged, versioned, or reversible

SomaOS addresses all four by building the agent interface into the compositor itself, making the approval workflow a first-class operating system primitive.

---

## Architecture

```
┌──────────────────────────────────────────────────────┐
│                    SomaOS Image                      │
│  ┌─────────────┐   ┌──────────────────────────────┐  │
│  │  Buildroot   │   │        Linux Kernel          │  │
│  │  Toolchain   │   │   (x86_64, virtio, DRM)     │  │
│  └─────────────┘   └──────────────────────────────┘  │
│  ┌─────────────────────────────────────────────────┐  │
│  │                  systemd                        │  │
│  │  ┌──────────────────┐  ┌─────────────────────┐  │  │
│  │  │   soma-agent     │  │  soma-compositor    │  │  │
│  │  │   (daemon)       │←→│  (Wayland/Winit)    │  │  │
│  │  └────────┬─────────┘  └───────┬─────────────┘  │  │
│  │           │                    │                 │  │
│  │     ┌─────┴──────┐      ┌─────┴──────┐          │  │
│  │     │  Ollama    │      │  tiny-skia  │          │  │
│  │     │  (LLM)    │      │  cosmic-text│          │  │
│  │     └────────────┘      └────────────┘          │  │
│  └─────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────┘
```

### System Components

| Component | Role | Technology |
|-----------|------|------------|
| **soma-common** | Shared types, IPC protocol | Rust, serde |
| **soma-agent** | Intent parsing, command execution, approval state | Rust, reqwest, tokio |
| **soma-compositor** | Display server, UI rendering, input handling | Rust, winit, tiny-skia, cosmic-text |
| **Buildroot Image** | Minimal Linux rootfs with bootloader | Buildroot 2024.02, GRUB2, systemd |

### Data Flow

```
                     ┌─────────────┐
User Input ────────→ │ Compositor  │
(keyboard)           │  Sidebar    │
                     └──────┬──────┘
                            │ CompositorMessage::ParseIntent
                            │ (Unix Socket IPC)
                            ▼
                     ┌─────────────┐        ┌──────────┐
                     │ Agent       │──HTTP──→│ Ollama   │
                     │ Daemon      │←───────│ (LLM)    │
                     └──────┬──────┘        └──────────┘
                            │ AgentMessage::TaskPlanReady
                            ▼
                     ┌─────────────┐
                     │ HITL Modal  │ ← User reviews plan
                     │ (Compositor)│   [Approve / Reject]
                     └──────┬──────┘
                            │ CompositorMessage::Approve
                            ▼
                     ┌─────────────┐
                     │ Executor    │ → Sandboxed command execution
                     │ (Agent)     │   (whitelisted commands only)
                     └──────┬──────┘
                            │ AgentMessage::StepResult (incremental)
                            │ AgentMessage::ExecutionComplete
                            ▼
                     ┌─────────────┐
                     │ Results     │ → Displayed in sidebar history
                     │ (Compositor)│
                     └─────────────┘
```

---

## Software Specifications

### IPC Protocol

Communication between the compositor and agent daemon uses a **newline-delimited JSON protocol** over a **Unix domain socket** (`/tmp/soma-agent.sock`).

#### Compositor → Agent Messages

| Message Type | Fields | Description |
|-------------|--------|-------------|
| `ParseIntent` | `id`, `input` | Send natural language for LLM parsing |
| `Approve` | `id` | User approved a pending task plan |
| `Reject` | `id` | User rejected a pending task plan |
| `DirectExec` | `id`, `command` | Execute a raw shell command (terminal) |
| `ReadClipboard` | `id` | Read system clipboard contents |
| `Ping` | — | Health check |

#### Agent → Compositor Messages

| Message Type | Fields | Description |
|-------------|--------|-------------|
| `TaskPlanReady` | `id`, `plan` | LLM returned a structured plan |
| `StepResult` | `id`, `step_index`, `result` | One step completed |
| `ExecutionComplete` | `id`, `results` | All steps finished |
| `Error` | `id`, `message` | An error occurred |
| `DirectOutput` | `id`, `result` | Terminal command output |
| `ClipboardContent` | `id`, `content` | Clipboard read result |
| `Pong` | — | Health check response |

#### TaskPlan Schema

```json
{
  "intent": "create_project_structure",
  "description": "Create a new project directory with src and docs folders",
  "steps": [
    { "action": "execute_command", "command": "mkdir", "args": ["-p", "project/src"] },
    { "action": "execute_command", "command": "mkdir", "args": ["-p", "project/docs"] },
    { "action": "execute_command", "command": "touch", "args": ["project/README.md"] }
  ],
  "risk_level": "medium"
}
```

### Risk Classification

| Level | Color | Criteria | Example |
|-------|-------|----------|---------|
| **Low** | 🟢 Green | Read-only operations | `ls`, `cat`, `pwd`, `whoami` |
| **Medium** | 🟡 Yellow | Create/modify operations | `mkdir`, `touch`, `cp`, `mv` |
| **High** | 🔴 Red | Destructive operations | `rm`, `rm -rf` |

### Command Whitelist

The executor enforces a strict whitelist. Only these commands are allowed:

```
ls  mkdir  open  cat  echo  pwd  rm  cp  mv  touch
head  tail  wc  find  grep  which  whoami  date  uname
```

Any command not on this list is rejected at the executor level, regardless of LLM output.

### Rendering Pipeline

```
winit event loop (60 fps)
  → tiny-skia Pixmap (software rasterization)
    → Sidebar panel (380px fixed width)
      → Title bar + status indicator
      → Content area (history, results, welcome)
      → Input field + send button
    → Terminal panel (remaining width)
      → Title bar
      → Scrollback buffer (2000 lines max)
      → Input prompt
    → HITL overlay (modal, centered)
  → softbuffer Surface (copy pixels to window)
```

Text rendering is handled by **cosmic-text** (the text engine from the COSMIC desktop), providing proper Unicode shaping, fallback fonts, and sub-pixel layout.

---

## Hardware Requirements

### Minimum (VM Testing)

| Component | Requirement |
|-----------|-------------|
| **CPU** | x86_64, 2 cores |
| **RAM** | 2 GB |
| **Disk** | 2 GB (1 GB image + working space) |
| **GPU** | Software rendering (Mesa llvmpipe/swrast) |
| **Network** | Optional (for Ollama API, SSH) |

### Recommended (Physical Hardware)

| Component | Requirement |
|-----------|-------------|
| **CPU** | x86_64, 4+ cores (Intel i5/Ryzen 5 or better) |
| **RAM** | 8 GB (4 GB for OS + 4 GB for LLM inference) |
| **Disk** | 20 GB SSD (OS + Ollama models) |
| **GPU** | Any GPU with DRM/KMS support (Intel, AMD, or NVIDIA with nouveau) |
| **Network** | Ethernet or WiFi (for initial model download) |

### VM Configuration (VirtualBox)

| Setting | Value |
|---------|-------|
| Type | Linux / Other 64-bit |
| Base Memory | 2048 MB |
| Processors | 2 |
| Video Memory | 128 MB |
| Graphics Controller | VMSVGA |
| Boot | Hard Disk (attach `soma-os.img` as VDI) |
| Network | NAT (for internet) or Bridged |

---

## Getting Started

### Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| **Rust** | 1.80+ | Compile soma crates |
| **Docker** | 20+ | Build the Buildroot image |
| **QEMU** or **VirtualBox** | Latest | Boot and test the image |

### Build

```bash
# 1. Clone the repository
git clone <repo-url> && cd "Native OS Project"

# 2. Cross-compile Rust binaries for x86_64 Linux
./buildroot/build.sh --rust-only

# 3. Build the full OS image (takes 20-60 min first time)
./buildroot/build.sh --image-only

# 4. Boot in QEMU (for quick testing)
qemu-system-x86_64 -m 2G -smp 2 \
  -drive file=buildroot/output/soma-os.img,format=raw \
  -device virtio-gpu-pci -serial stdio

# 5. Or convert for VirtualBox
VBoxManage convertfromraw buildroot/output/soma-os.img soma-os.vdi --format VDI
```

### Dev Mode (macOS, no VM)

For rapid iteration, run the compositor and agent directly on your dev machine:

```bash
# Terminal 1: Agent daemon
cargo run -p soma-agent

# Terminal 2: Compositor window
cargo run -p soma-compositor
```

---

## Project Structure

```
Native OS Project/
├── Cargo.toml                          # Workspace root
│
├── soma-common/                        # Shared library crate
│   └── src/lib.rs                      # Types: TaskPlan, IPC messages, AgentStatus
│
├── soma-agent/                         # Agent daemon binary
│   └── src/
│       ├── main.rs                     # Entry point, logger init
│       ├── intent.rs                   # Ollama NL → TaskPlan parser
│       ├── executor.rs                 # Whitelisted command executor
│       └── ipc.rs                      # Unix socket server
│
├── soma-compositor/                    # Compositor binary
│   └── src/
│       ├── main.rs                     # Winit event loop, input routing
│       ├── renderer.rs                 # tiny-skia + cosmic-text renderer
│       ├── sidebar.rs                  # Agent sidebar UI + HITL overlay
│       ├── terminal.rs                 # Embedded terminal emulator
│       └── ipc_client.rs              # Agent daemon connection
│
├── buildroot/                          # OS image build system
│   ├── Dockerfile                      # Docker build environment
│   ├── soma_defconfig                  # Buildroot configuration
│   ├── post-build.sh                   # Rootfs customization script
│   ├── build.sh                        # Full build pipeline
│   └── overlay/                        # Files injected into rootfs
│       └── etc/systemd/system/
│           ├── soma-agent.service      # Agent daemon unit
│           └── soma-compositor.service # Compositor unit
│
└── scripts/
    ├── run-qemu.sh                     # QEMU launch helper
    └── cross-build.sh                  # Cross-compilation helper
```

---

## Roadmap

### v0.1 — Foundation ✅
- [x] Rust workspace with 3 crates
- [x] Software-rendered compositor (winit + tiny-skia)
- [x] Agent daemon with Ollama integration
- [x] HITL approval modal
- [x] Embedded terminal
- [x] Buildroot image pipeline
- [x] Cross-compilation for x86_64

### v0.2 — Agent Intelligence
- [ ] Semantic context memory (SQLite + vector embeddings)
- [ ] Multi-step plan execution with rollback
- [ ] Command output piping between steps
- [ ] Persistent agent history across sessions

### v0.3 — Compositor Maturity
- [ ] DRM/KMS backend (run as real display server, not inside winit)
- [ ] Wayland client protocol support (run native Wayland apps)
- [ ] Mouse input and clickable UI elements
- [ ] Window management for spawned applications

### v0.4 — OS Integration
- [ ] Filesystem monitoring and semantic indexing
- [ ] Network-aware agent (HTTP requests, API calls)
- [ ] Package management (install/update tools)
- [ ] Secure agent sandboxing (namespaces, capabilities)
- [ ] Encrypted audit log of all agent actions

### v1.0 — Production
- [ ] Real-time voice input (Whisper)
- [ ] Screen understanding (vision model integration)
- [ ] Multi-agent collaboration (agent-to-agent IPC)
- [ ] OTA updates
- [ ] Hardware driver support matrix (WiFi, Bluetooth, USB)

---

## Design Decisions

### Why a custom compositor instead of GNOME/KDE?

Traditional desktop environments are designed for human mouse-and-keyboard interaction — window chrome, taskbars, application launchers, and notification systems. For an AI agent:
- **Window management is unnecessary** — the agent operates through structured commands, not GUI clicks
- **The HITL modal is a first-class primitive** — it's rendered by the compositor itself, not a separate application, so it cannot be bypassed
- **Resource efficiency** — SomaOS boots in seconds with ~100MB RAM, vs. 1GB+ for GNOME

### Why Buildroot instead of Yocto/NixOS?

- **Buildroot** produces the smallest images with the least configuration overhead
- Single `defconfig` file describes the entire system
- Faster build times (~20 min vs. hours for Yocto)
- No package manager overhead at runtime — everything is compiled in

### Why local LLM (Ollama) instead of cloud APIs?

- **Latency** — Local inference avoids network round-trips
- **Privacy** — Commands and file contents never leave the machine
- **Reliability** — No dependency on internet connectivity or API availability
- **Cost** — No per-token billing

### Why Rust?

- **Memory safety without GC** — Critical for a compositor and system daemon
- **Single binary deployment** — No runtime dependencies to manage
- **Ecosystem** — Smithay, winit, tiny-skia, cosmic-text are all mature Rust libraries
- **Cross-compilation** — `cargo build --target x86_64-unknown-linux-gnu` just works

---

## Security Model

```
┌────────────────────────────────────────────┐
│              Human Operator                │
│         (physical keyboard access)         │
└───────────────────┬────────────────────────┘
                    │ Approve / Reject
                    ▼
┌────────────────────────────────────────────┐
│            HITL Gate (Compositor)           │
│  Every plan must be explicitly approved.   │
│  No command executes without human consent.│
└───────────────────┬────────────────────────┘
                    │
                    ▼
┌────────────────────────────────────────────┐
│         Command Whitelist (Executor)       │
│  Only 19 pre-approved commands accepted.   │
│  Arbitrary binaries cannot be invoked.     │
└───────────────────┬────────────────────────┘
                    │
                    ▼
┌────────────────────────────────────────────┐
│           OS-Level Isolation               │
│  (Future: namespaces, seccomp, apparmor)   │
└────────────────────────────────────────────┘
```

**Key invariant**: No LLM output can result in command execution without passing through both the HITL approval gate and the command whitelist filter.

---

## License

MIT

---

<p align="center">
  <sub>SomaOS — Where the agent is the interface.</sub>
</p>

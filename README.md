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
- A **custom compositor** with an integrated chat sidebar and embedded terminal
- An **agent daemon** with 29 structured capability actions across 5 modules
- **On-device LLM** (qwen2.5-coder:7b via Ollama) that converts natural language to executable task plans
- A **Human-in-the-Loop (HITL) approval system** enforcing mandatory human review before any action
- **Conversation memory** — the agent remembers recent exchanges for follow-up commands
- A **minimal Linux image** (< 1GB) built with Buildroot, targeting x86_64 hardware

---

## Architecture

```
┌──────────────────────────────────────────────────────┐
│                    SomaOS Image                      │
│  ┌─────────────────────────────────────────────────┐ │
│  │                  systemd                        │ │
│  │  ┌───────────────────┐  ┌────────────────────┐  │ │
│  │  │   soma-agent      │  │  soma-compositor   │  │ │
│  │  │   (daemon)        │←→│  (winit/tiny-skia) │  │ │
│  │  │                   │  │                    │  │ │
│  │  │ ┌───────────────┐ │  │ ┌────────────────┐ │  │ │
│  │  │ │ Capabilities  │ │  │ │ Chat Sidebar   │ │  │ │
│  │  │ │ ├─filesystem  │ │  │ │ (right panel)  │ │  │ │
│  │  │ │ ├─process     │ │  │ ├────────────────┤ │  │ │
│  │  │ │ ├─system      │ │  │ │ Terminal       │ │  │ │
│  │  │ │ ├─network     │ │  │ │ (left panel)   │ │  │ │
│  │  │ │ └─package     │ │  │ └────────────────┘ │  │ │
│  │  │ └───────────────┘ │  └────────────────────┘  │ │
│  │  │       │           │                          │ │
│  │  │  ┌────┴────┐      │                          │ │
│  │  │  │ Ollama  │      │                          │ │
│  │  │  │ (LLM)   │      │                          │ │
│  │  │  └─────────┘      │                          │ │
│  │  └───────────────────┘                          │ │
│  └─────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────┘
```

### System Components

| Component | Role | Technology |
|-----------|------|------------|
| **soma-common** | Shared types, IPC protocol, capability types | Rust, serde |
| **soma-agent** | Intent parsing, capability execution, conversation context | Rust, reqwest, tokio |
| **soma-compositor** | Display server, chat UI, terminal, HITL overlay | Rust, winit, tiny-skia, cosmic-text |
| **soma-cli** | Terminal test client for agent interaction | Rust, tokio |
| **Buildroot Image** | Minimal Linux rootfs with bootloader | Buildroot, GRUB2, systemd |

### Data Flow

```
User types in sidebar
        │
        ▼
  Compositor sends NaturalLanguageInput
  (Unix Socket: /tmp/soma-agent.sock)
        │
        ▼
  Agent Daemon ──HTTP──→ Ollama (qwen2.5-coder:7b)
    │                     Converts NL → TaskPlan JSON
    │◄────────────────────
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
    │  ExecutionComplete (final)
    ▼
  Results displayed in chat sidebar
```

---

## Capabilities

The agent executes actions through a structured **capability system** — not raw shell commands. Each capability is a Rust module that validates parameters and returns structured JSON data.

### Filesystem (9 actions)

| Action | Description | Risk |
|--------|-------------|------|
| `list_dir` | List directory contents | Low |
| `read_file` | Read file contents | Low |
| `write_file` | Write/create a file | Medium |
| `create_dir` | Create a directory | Medium |
| `delete` | Delete a file or directory | High |
| `copy` | Copy file/directory | Medium |
| `move` | Move/rename file | Medium |
| `find` | Search for files by pattern | Low |
| `file_info` | Get file metadata | Low |

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

### IPC Protocol

Communication uses **newline-delimited JSON** over a **Unix domain socket** (`/tmp/soma-agent.sock`).

#### Compositor → Agent Messages

| Message Type | Fields | Description |
|-------------|--------|-------------|
| `NaturalLanguageInput` | `text` | Natural language command |
| `ParseIntent` | `id`, `input` | Send NL for LLM parsing (legacy) |
| `Approve` | `id` | User approved a pending task plan |
| `Reject` | `id` | User rejected a pending task plan |
| `DirectExec` | `id`, `command` | Execute a raw shell command |
| `ListCapabilities` | — | Request capability list |
| `Ping` | — | Health check |

#### Agent → Compositor Messages

| Message Type | Fields | Description |
|-------------|--------|-------------|
| `TaskPlanReady` | `id`, `plan` | LLM returned a structured plan |
| `StepResult` | `id`, `step_index`, `result` | One step completed |
| `ExecutionComplete` | `id`, `results` | All steps finished |
| `Error` | `id`, `message` | An error occurred |
| `DirectOutput` | `id`, `result` | Terminal command output |
| `Capabilities` | `capabilities` | List of registered capabilities |
| `Pong` | — | Health check response |

#### TaskPlan Schema (v0.5)

```json
{
  "intent": "list_directory",
  "description": "List files in /home",
  "steps": [
    {
      "capability": "filesystem",
      "action": "list_dir",
      "params": { "path": "/home" },
      "description": "List directory contents of /home"
    }
  ],
  "risk_level": "low"
}
```

Multi-step plans are supported:

```json
{
  "intent": "inspect_system",
  "description": "Find log files and check disk usage",
  "steps": [
    {
      "capability": "filesystem",
      "action": "find",
      "params": { "path": "/var", "pattern": "*.log" },
      "description": "Find .log files in /var"
    },
    {
      "capability": "system",
      "action": "disk_usage",
      "params": {},
      "description": "Check disk usage"
    }
  ],
  "risk_level": "low"
}
```

### Conversation Context

The agent maintains a per-client conversation history (last 5 exchanges). This enables follow-up commands:

```
User: "list files in /tmp"           → filesystem.list_dir {path: "/tmp"}
User: "now show /var"                → filesystem.list_dir {path: "/var"}  (context resolves "now show")
User: "delete the boot folder there" → filesystem.delete {path: "/var/boot"}  (context + "there")
```

### Risk Classification

| Level | Criteria | Example Actions |
|-------|----------|-----------------|
| **Low** | Read-only operations | `list_dir`, `hostname`, `ping`, `ifconfig` |
| **Medium** | Create/modify operations | `write_file`, `create_dir`, `install` |
| **High** | Destructive operations | `delete`, `kill`, `remove` |

### Rendering Pipeline

```
winit event loop (conditional redraw)
  → tiny-skia Pixmap (software rasterization)
    → Terminal panel (left, variable width)
    → Chat Sidebar panel (right, 380px fixed)
      → Title bar + status pill
      → Scrollable message history
        → User bubbles (right-aligned, accent)
        → Plan cards (left-aligned, with steps)
        → Result cards (with formatted data)
        → Error cards
      → Input field + send button
    → HITL approval overlay (modal, centered)
  → softbuffer Surface (copy pixels to window)
```

Text rendering via **cosmic-text** (COSMIC desktop text engine) with Unicode shaping and fallback fonts.

---

## Hardware Requirements

### Minimum (VM Testing)

| Component | Requirement |
|-----------|-------------|
| **CPU** | x86_64, 2 cores |
| **RAM** | 4 GB (OS + LLM inference) |
| **Disk** | 10 GB (OS + Ollama models) |
| **GPU** | Software rendering (Mesa llvmpipe) |

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
| **Docker** | 20+ | Build the Buildroot image (optional) |

### Dev Mode (Recommended — macOS/Linux)

Run everything natively for fast iteration:

```bash
# 1. Clone
git clone <repo-url> && cd "Native OS Project"

# 2. Install the LLM model
ollama pull qwen2.5-coder:7b

# 3. Start Ollama (if not already running)
ollama serve &

# 4. Start the agent daemon
cargo run -p soma-agent

# 5. In another terminal, start the compositor
cargo run -p soma-compositor

# 6. Or use the CLI test client instead
cargo run -p soma-cli
```

### VM Build (Full OS Image)

```bash
# 1. Cross-compile Rust binaries for x86_64 Linux (musl, static)
./buildroot/build.sh --rust-only

# 2. Build the full OS image (~20 min first time)
./buildroot/build.sh --image-only

# 3. Boot in QEMU
qemu-system-x86_64 -m 4G -smp 2 \
  -drive file=buildroot/output/soma-os.img,format=raw \
  -device virtio-gpu-pci -serial stdio

# 4. Or convert for VMware Fusion / VirtualBox
qemu-img convert -f raw -O vmdk soma-os.img soma-os.vmdk
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
│       ├── main.rs                     # Entry point, logger init
│       ├── intent.rs                   # Ollama NL → TaskPlan parser (with context)
│       ├── executor.rs                 # Capability dispatch executor
│       ├── ipc.rs                      # Unix socket server + conversation context
│       └── capabilities/
│           ├── mod.rs                  # Capability trait + registry
│           ├── filesystem.rs           # 9 file/directory actions
│           ├── process.rs              # 5 process management actions
│           ├── system.rs               # 6 system info actions
│           ├── network.rs              # 5 network diagnostic actions
│           └── package.rs              # 4 package management actions
│
├── soma-compositor/                    # Compositor binary
│   └── src/
│       ├── main.rs                     # Winit event loop, input routing, scroll
│       ├── renderer.rs                 # tiny-skia + cosmic-text renderer
│       ├── sidebar.rs                  # Chat UI, result cards, HITL overlay
│       ├── terminal.rs                 # Embedded terminal emulator
│       └── ipc_client.rs              # Agent daemon connection
│
├── soma-cli/                           # CLI test client
│   └── src/main.rs                     # Interactive NL client for testing
│
└── buildroot/                          # OS image build system
    ├── Dockerfile                      # Docker build environment
    ├── soma_defconfig                  # Buildroot configuration
    ├── post-build.sh                   # Rootfs customization
    ├── build.sh                        # Build pipeline (musl cross-compile)
    └── overlay/                        # Files injected into rootfs
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

### v0.2 — Capability System ✅
- [x] Structured capability modules (filesystem, process, system)
- [x] Capability registry with dynamic LLM prompt generation
- [x] soma-cli test client
- [x] musl static linking for cross-compilation

### v0.3 — Compositor Chat UI ✅
- [x] Chat-style sidebar with user bubbles and agent cards
- [x] Agent connection from compositor
- [x] LLM upgrade to llama3.2:3b

### v0.4 — UI Polish ✅
- [x] Right-side sidebar layout
- [x] Inline thinking indicator
- [x] Scrollable message history (trackpad + mouse wheel)
- [x] Rich result cards (file listings, processes, network data)
- [x] Agent-triggered redraws
- [x] ASCII-safe rendering (no broken emoji)

### v0.5 — Smarter Agent ✅
- [x] Network capability (ping, curl, dns, ifconfig, port_check)
- [x] Package capability (list, search, install, remove)
- [x] Conversation context memory (5 exchanges)
- [x] LLM upgrade to qwen2.5-coder:7b
- [x] 10 few-shot examples in system prompt

### v0.6 — Rich Compositor (Next)
- [ ] Real PTY terminal (connect to actual shell)
- [ ] Click interaction on UI elements
- [ ] Resizable panels (drag divider)
- [ ] Notification toasts
- [ ] Theme switcher

### v0.7 — Voice & Multimodal
- [ ] Voice input (Whisper STT)
- [ ] Voice output (TTS)
- [ ] File previews in result cards

### v0.8 — VM Production Boot
- [ ] DRM/KMS framebuffer compositor
- [ ] Auto-start services on boot
- [ ] Model bundling in image
- [ ] GPU passthrough for inference

### v1.0 — Ship It
- [ ] USB bootable image
- [ ] Plugin system for third-party capabilities
- [ ] Security hardening (sandboxing, audit log)
- [ ] OTA updates

---

## Design Decisions

### Why a custom compositor instead of GNOME/KDE?

Traditional desktops are designed for mouse-and-keyboard humans. For an AI agent:
- **The HITL modal is a first-class OS primitive** — rendered by the compositor itself, cannot be bypassed
- **Minimal footprint** — SomaOS boots in seconds with ~100MB RAM
- **The chat sidebar is the primary interface**, not an afterthought bolted onto a desktop

### Why a capability system instead of raw shell commands?

- **Type safety** — Each action validates its parameters before execution
- **Structured output** — Results are JSON, not raw text, enabling rich UI rendering
- **Extensibility** — New capabilities are just Rust modules implementing a trait
- **Auditability** — Every action is logged with its capability, action, and parameters

### Why local LLM (Ollama) instead of cloud?

- **Latency** — No network round-trips
- **Privacy** — Commands and file contents never leave the machine
- **Reliability** — No internet dependency
- **Cost** — No per-token billing

### Why Rust?

- **Memory safety without GC** — Critical for a compositor and system daemon
- **Single binary** — No runtime dependencies
- **Cross-compilation** — `cargo build --target x86_64-unknown-linux-musl` for static binaries
- **Ecosystem** — winit, tiny-skia, cosmic-text are mature Rust libraries

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
│  No action executes without human consent. │
└───────────────────┬────────────────────────┘
                    │
                    ▼
┌────────────────────────────────────────────┐
│       Capability Registry (Agent)          │
│  Only registered capability actions run.   │
│  Parameters validated per action schema.   │
│  Structured JSON results, not raw output.  │
└───────────────────┬────────────────────────┘
                    │
                    ▼
┌────────────────────────────────────────────┐
│           OS-Level Isolation               │
│  (Future: namespaces, seccomp, apparmor)   │
└────────────────────────────────────────────┘
```

**Key invariant**: No LLM output results in action execution without passing through the HITL approval gate and the capability registry.

---

## License

MIT

---

<p align="center">
  <sub>SomaOS — Where the agent is the interface.</sub>
</p>

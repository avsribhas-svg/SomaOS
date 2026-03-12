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
  <a href="#agent-assisted-capability-authoring">Self-Improvement</a> ·
  <a href="#getting-started">Getting Started</a> ·
  <a href="ROADMAP.md">Roadmap</a>
</p>

<p align="center">
  <a href="https://github.com/avsribhas-svg/SomaOS/actions/workflows/build.yml">
    <img src="https://github.com/avsribhas-svg/SomaOS/actions/workflows/build.yml/badge.svg" alt="Build SomaOS Image" />
  </a>
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

Current state (v0.9 + v0.8.1): SomaOS runs as a bootable Linux image with a custom bare-metal compositor.

The system provides:
- A **custom DRM/KMS compositor** that renders directly to GPU framebuffer — no X11 or Wayland server required
- A **login screen** that boots straight into Soma, with no traditional desktop
- An **agent daemon** with 35 built-in capability actions across 9 modules, plus unlimited user-defined capabilities (loaded from `~/.soma/capabilities/*.json`)
- **Browser panel** — headless Chromium integration with F2 toggle in compositor; agent can navigate, scrape, and screenshot
- **Vision capability** — image understanding via Ollama qwen2.5-vl:7b; agent can analyze images with natural language queries
- **On-device LLM** (qwen2.5-coder:7b via Ollama) with a three-layer intent pipeline for robust natural language understanding
- A **Human-in-the-Loop (HITL) approval system** enforcing mandatory human review before any action
- **Self-improvement loop** — agent proposes new capabilities via `meta.propose`; human approves through HITL; registry grows without a Rust rebuild
- **Conversation memory** — the agent remembers recent exchanges for follow-up commands
- **Rich result cards** — image thumbnails, line-numbered text previews, directory tree views
- **Auto-start services** — Ollama + agent + compositor all start on boot via systemd
- A **minimal Linux image** built with Buildroot, targeting x86_64 and ARM64 hardware
- **GitHub Actions CI** — bootable image built and published as artifact on every push to `main`

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
│  │  │ │Capabilities │ │  │  │  PTY Terminal OR │  │  │   │
│  │  │ │ ├─filesystem│ │  │  │  Browser Panel   │  │  │   │
│  │  │ │ ├─process   │ │  │  │  (left, F2 swap) │  │  │   │
│  │  │ │ ├─system    │ │  │  ├──────────────────┤  │  │   │
│  │  │ │ ├─network   │ │  │  │  HITL Overlay    │  │  │   │
│  │  │ │ ├─package   │ │  │  └──────────────────┘  │  │   │
│  │  │ │ ├─browser   │ │  │                        │  │   │
│  │  │ │ ├─vision    │ │  │                        │  │   │
│  │  │ │ ├─meta      │ │  │                        │  │   │
│  │  │ │ └─[user]    │ │  │                        │  │   │
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

### Browser (4 actions)

| Action | Description | Risk |
|--------|-------------|------|
| `navigate` | Load a URL in headless Chromium | Low |
| `get_content` | Scrape and return text content of current page | Low |
| `search` | Web search and return results | Low |
| `screenshot` | Take a screenshot of the current page (saved to `/tmp/soma-browser.png`) | Low |

Browser screenshots are also sent to the compositor as a `BrowserUpdate` message, which auto-switches the left panel to Browser view.

### Vision (1 action)

| Action | Description | Risk |
|--------|-------------|------|
| `analyze_image` | Describe or query an image using Ollama qwen2.5-vl:7b | Low |

Env: `SOMA_VISION_MODEL` overrides the vision model (default: `qwen2.5-vl:7b`).

### Meta (3 actions)

| Action | Description | Risk |
|--------|-------------|------|
| `propose` | Propose a new shell-backed capability; saves JSON definition to `~/.soma/capabilities/` after HITL approval | Medium |
| `list_proposed` | List all user-defined capabilities currently on disk | Low |
| `describe_gap` | Record a capability gap to `~/.soma/gaps.log` for later review | Low |

### User-Defined Capabilities (dynamic)

Any JSON files in `~/.soma/capabilities/` are loaded at agent startup as **ScriptCapabilities** — shell-command-backed capabilities proposed by the agent and approved by the human. These appear in the capability registry and are available to the LLM exactly like built-in capabilities.

---

## Agent-Assisted Capability Authoring

SomaOS is designed to improve itself over time. When the agent encounters a task it cannot complete with existing capabilities, it can propose a new one — and the human approves it through the same HITL gate used for every other action.

### The self-improvement loop

```
1. Human: "I want you to be able to compress PDFs"
2. Agent: generates a capability definition (name, actions, shell templates)
3. HITL: human reviews the full definition before it is saved
4. Human approves → capability saved to ~/.soma/capabilities/compress_pdf.json
5. Agent restarts → new capability available in registry
6. Agent: now uses compress_pdf for the current and all future tasks
```

The HITL gate is what makes this safe. The agent **proposes**, the human **decides**. Each new capability is a discrete, reviewable artifact (a JSON file with explicit shell commands) — not an opaque modification. The human can inspect exactly what the capability does, and remove the JSON file at any time to revoke it.

### Capability definition format

User-defined capabilities are stored as JSON in `~/.soma/capabilities/`:

```json
{
  "name": "compress_pdf",
  "description": "Compress PDF files using Ghostscript",
  "actions": [
    {
      "name": "compress",
      "description": "Compress a PDF to reduce file size",
      "params": [
        { "name": "input",  "param_type": "string", "required": true, "description": "Input PDF path"  },
        { "name": "output", "param_type": "string", "required": true, "description": "Output PDF path" }
      ],
      "shell_template": "gs -sDEVICE=pdfwrite -dNOPAUSE -dBATCH -sOutputFile={output} {input}"
    }
  ]
}
```

`{param_name}` placeholders are substituted at execution time. The definition is validated before saving — unknown fields and missing required params are caught and surfaced to the human during review.

### How the human's role shifts over time

| Stage | Human role |
|---|---|
| Early | Writes capabilities in Rust; agent uses them |
| Middle | Agent proposes capabilities; human reviews JSON and approves |
| Later | Human specifies intent in natural language; agent handles implementation |
| Mature | Human curates the registry; rarely touches implementation |

The system gradually shifts the barrier to building software from **implementation skill** (writing Rust) to **specification skill** (knowing what you want and recognising good work). The registry grows through use, and the coverage of tasks the agent can handle without human intervention increases over time.

### Activating a proposed capability

After `meta.propose` saves a capability to disk, restart the agent to load it:

```bash
# From the terminal panel, or via the agent:
systemctl restart soma-agent
```

Or tell the agent: _"restart the agent service"_ — it will use the `process` capability to do it.

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
| `BrowserUpdate` | `url`, `title`, `screenshot_base64` | Browser navigated to a new page |
| `Pong` | — | Health check response |

### Rendering Pipeline

```
DRM/KMS main loop (bare metal) OR winit event loop (dev)
  → tiny-skia Pixmap (software rasterization)
    → Login screen (if not yet authenticated)
      OR
    → Terminal panel OR Browser panel (left, variable width; F2 to toggle)
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

### Download a Pre-Built Image (Fastest)

Every push to `main` automatically builds a bootable image via GitHub Actions (native x86_64 Linux, no Rosetta). No local build required.

1. Go to [Actions](https://github.com/avsribhas-svg/SomaOS/actions/workflows/build.yml) → click the latest passing run
2. Download `soma-os-x86_64-<sha>.zip` from **Artifacts**
3. Extract → `gunzip soma-os.img.gz`
4. Follow the VirtualBox setup in [docs/BUILD_x86_64.md](docs/BUILD_x86_64.md) (skip the build steps)

For tagged releases, images are also attached directly to the [GitHub Release](https://github.com/avsribhas-svg/SomaOS/releases).

---

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

### VM Build — Apple Silicon Mac (UTM, ARM64)

```bash
cd buildroot && ./build.sh --arch=aarch64
# Output: buildroot/output/aarch64/soma-os.img + kernel
```

UTM → New → Virtualize → Linux → point Kernel at `output/aarch64/kernel` → attach `soma-os.img` as VirtIO drive → Display: **virtio-gpu-gl**.

See [docs/BUILD_ARM64.md](docs/BUILD_ARM64.md) for the full setup guide.

### VM Build — Windows (WSL2) + VirtualBox, or Linux/Intel Mac (QEMU)

```bash
cd buildroot && ./build.sh --arch=x86_64
# Output: buildroot/output/x86_64/soma-os.img
```

Windows: convert to VDI in PowerShell, create a VirtualBox VM with **VMSVGA** display, 128 MB VRAM, 4 GB RAM.
Linux / Intel Mac: run directly with `qemu-system-x86_64`.

See [docs/BUILD_x86_64.md](docs/BUILD_x86_64.md) for the full setup guide.

### Iterative Dev (fastest loop — no image rebuild)

After any Rust code change, skip the full image rebuild entirely. Recompile and deploy directly to a running VM:

```bash
# 1. Recompile Rust binaries only (~5-15 min)
./build.sh --rust-only

# 2. Copy new binaries into the running VM
scp overlay/usr/bin/soma-compositor root@<VM-IP>:/usr/bin/
scp overlay/usr/bin/soma-agent root@<VM-IP>:/usr/bin/

# 3. Restart services
ssh root@<VM-IP> "systemctl restart soma-agent soma-compositor"
```

Full image rebuilds (`--image-only`) are only needed when changing `soma_defconfig`, `post-build.sh`, or systemd units — roughly once per major version. Those builds run automatically on CI.

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
│           ├── mod.rs                  # Capability trait + registry (loads built-in + user-defined)
│           ├── filesystem.rs           # 9 actions (read_file supports image base64, ~ expansion)
│           ├── process.rs              # 5 process management actions
│           ├── system.rs               # 6 system info actions
│           ├── network.rs              # 5 network diagnostic actions
│           ├── package.rs              # 4 package management actions
│           ├── meta.rs                 # 3 actions: propose, list_proposed, describe_gap
│           └── script.rs              # ScriptCapability: runtime caps from ~/.soma/capabilities/*.json
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
│   ├── BUILD_x86_64.md                # x86_64 build guide (Windows/WSL2 + VirtualBox, Linux/QEMU)
│   └── BUILD_ARM64.md                 # ARM64 build guide (Apple Silicon + UTM)
│
├── .github/
│   └── workflows/
│       └── build.yml                  # CI: builds x86_64 image on every push to main
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
- **Extensibility** — New capabilities are Rust structs implementing a trait (built-in) or JSON files with shell templates (user-defined)
- **Auditability** — Every action logged with capability, action, and parameters
- **Self-improvement** — The agent can propose new capabilities via `meta.propose`; the human approves through HITL; the registry grows over time
- **Future AgentAPI parity** — The same structured-data philosophy will extend to application-level APIs

### Why JSON-defined capabilities instead of requiring Rust for extensions?

The built-in capabilities are compiled Rust — fast, type-safe, fully integrated. But requiring Rust for every extension creates a high floor: you need a compiler, a language, and a rebuild cycle.

User-proposed capabilities use shell-command templates (`{param}` substitution) stored as JSON. This means:
- **The agent can author them** — the LLM generates valid JSON; it cannot generate valid Rust without a compiler
- **The human can read them** — a JSON file with explicit shell commands is reviewable in 30 seconds; a Rust module requires context
- **No rebuild needed** — JSON capabilities load at agent startup; the iteration cycle is propose → approve → restart, not write → compile → deploy
- **The same HITL gate applies** — the human reviews the shell commands before they are saved, not after

When a use case proves stable and performance matters, a JSON capability can be promoted to a built-in Rust capability. The JSON definition serves as the spec.

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
- **Model-agnostic** — Any Ollama-compatible model can be swapped in; vision models (qwen2.5-vl) run alongside the task model (qwen2.5-coder:7b) for the `vision` capability

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

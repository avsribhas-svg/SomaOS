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
  <a href="#desktop-environment">Desktop</a> ·
  <a href="#desktop-agent-mode">Agent Mode</a> ·
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

Current state (v1.0 in progress, v0.9 stable): SomaOS runs as a bootable Linux image with a custom bare-metal compositor.

The system provides:
- A **full macOS-style desktop environment** — floating windows, a centred dock, a menu bar, and an AI sidebar as a slide-in overlay. The terminal and browser are applications, not panels.
- A **desktop agent mode** — the AI can take full control of the desktop (open/close/focus windows, type text, drive the browser) via IPC. Humans watch and can interrupt at any time through the HITL gate.
- **Dynamic app spawning** — the agent creates new application windows at runtime with a declarative widget tree (Label, Button, ProgressBar, TextDisplay) — no Rust rebuild required.
- **Workflow learning** — the compositor passively observes window focus and open/close events; humans or the agent can annotate sequences as named workflows persisted to `~/.soma/workflows.json`. Observation pauses automatically in private mode.
- **Private mode** — one keystroke disables observation; the menu bar shows a `[pvt]` indicator. The agent still responds to prompts but learns nothing from the session.
- A **custom DRM/KMS compositor** that renders directly to GPU framebuffer — no X11 or Wayland server required
- A **login screen** that boots straight into Soma, with no traditional desktop
- An **agent daemon** with 35 built-in capability actions across 9 modules, plus unlimited user-defined capabilities (loaded from `~/.soma/capabilities/*.json`)
- **Browser panel** — headless Chromium integration; agent can navigate, scrape, and screenshot. Browser opens as a floating window in the desktop environment.
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
┌──────────────────────────────────────────────────────────────────┐
│                          SomaOS Image                            │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                         systemd                             │ │
│  │  ┌─────────────────┐  ┌──────────────────────────────────┐  │ │
│  │  │  soma-ollama    │  │         soma-compositor           │  │ │
│  │  │  (LLM server)   │  │         (DRM/KMS backend)         │  │ │
│  │  └────────┬────────┘  │                                  │  │ │
│  │           │           │  Menu bar (28px): clock · AI      │  │ │
│  │  ┌────────┴────────┐  │  ┌────────────────────────────┐  │  │ │
│  │  │  soma-agent     │◄─┤  │  Floating Windows          │  │  │ │
│  │  │  (daemon)       │  │  │  Terminal.app / Browser.app │  │  │ │
│  │  │                 │  │  │  DynamicApp (agent-spawned) │  │  │ │
│  │  │ ┌─────────────┐ │  │  ├────────────────────────────┤  │  │ │
│  │  │ │Capabilities │ │  │  │  AI Sidebar (slide overlay) │  │  │ │
│  │  │ │ ├─filesystem│ │  │  │  Chat · HITL · Workflow     │  │  │ │
│  │  │ │ ├─process   │ │  │  ├────────────────────────────┤  │  │ │
│  │  │ │ ├─system    │ │  │  │  Dock (72px)               │  │  │ │
│  │  │ │ ├─network   │ │  │  │  Terminal · Browser · AI   │  │  │ │
│  │  │ │ ├─package   │ │  │  ├────────────────────────────┤  │  │ │
│  │  │ │ ├─browser   │ │  │  │  HITL Overlay              │  │  │ │
│  │  │ │ ├─vision    │ │  │  └────────────────────────────┘  │  │ │
│  │  │ │ ├─meta      │ │  │                                  │  │ │
│  │  │ │ ├─desktop   │ │  │  evdev input / /dev/dri/card0    │  │ │
│  │  │ │ └─[user]    │ │  └──────────────────────────────────┘  │ │
│  │  │ └─────────────┘ │                                        │ │
│  │  └─────────────────┘                                        │ │
│  └─────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
```

### System Components

| Component | Role | Technology |
|-----------|------|------------|
| **soma-common** | Shared types, IPC protocol, capability types | Rust, serde |
| **soma-agent** | Intent parsing, capability execution, conversation context, desktop observer | Rust, reqwest, tokio |
| **soma-compositor** | DRM/KMS display, desktop environment, floating windows, dock, menu bar, sidebar overlay | Rust, drm, evdev, tiny-skia, cosmic-text |
| **soma-cli** | Terminal test client for agent interaction | Rust, tokio |
| **Buildroot Image** | Minimal Linux rootfs, bootloader, systemd services | Buildroot, GRUB2, systemd |

---

## Desktop Environment

v1.0 replaces the fixed terminal+sidebar split with a full floating-window desktop. The AI and the human work in the same environment — the AI is a native user, not an external tool.

```
Menu bar: "Soma  ●  Researching competitor1.com...  [pvt]  10:42"
┌─────────────────────────────────────────────────────────────────┐
│                         Desktop wallpaper                        │
│  ┌────────────────────────┐   ┌─────────────────────────────┐   │
│  │  Terminal.app          │   │  Browser.app                │   │
│  │  ●  ────────────────   │   │  ●  ──────────────────────  │   │
│  │  $ cargo build         │   │  [competitor1.com]          │   │
│  └────────────────────────┘   │  [screenshot content]       │   │
│                               └─────────────────────────────┘   │
│           ┌──────────────────────────────────────┐              │
│           │  Competitor Summary (AI)             │              │
│           │  Label: "Top 3 competitors..."       │              │
│           │  [ Save as PDF ]  [ Open in Docs ]   │              │
│           └──────────────────────────────────────┘              │
│                                      ┌─────────────────────┐    │
│                                      │   AI Sidebar        │    │
│                                      │  (slide-in overlay) │    │
│                                      └─────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
       Dock:  [>_ Terminal]  [W Browser]  [AI Agent ●]  [>> Sidebar]  [PVT]
```

### Floating Windows

- Each app (`Terminal`, `Browser`, or agent-spawned `DynamicApp`) lives in a draggable `FloatingWindow`
- Window chrome: shadow, rounded body, title bar, traffic-light close button, centered title text
- Agent-spawned windows show a small teal **AI** badge in the top-right of the title bar
- Windows stack in focus order; clicking a window brings it to front

### Dock

- Always-visible 72px pill at the bottom of the screen
- App launchers: **Terminal** (`>_`), **Browser** (`W`), **AI Agent** (`AI`), **Sidebar** (`>>`), **Private** (`PVT`)
- Open-state indicator dots below each icon; agent-mode glow ring when active
- Click to open/focus a window or toggle a mode

### Menu Bar

- Always-visible 28px bar at the top
- Left: "Soma" label
- Centre-right: coloured status dot + live activity text from the agent (truncated to fit)
- Right: `[pvt]` indicator when private mode is on · clock
- Status dot colour: green=idle, blue=thinking, bright-blue=executing, yellow=awaiting approval

### AI Sidebar

- Slide-in overlay panel (800px/s tween), not a fixed split — the desktop is always full-width
- Toggled by clicking the Sidebar dock icon or pressing **Cmd+Space** (macOS) / **F3** (DRM)
- Contains the full chat history, plan cards, result cards, HITL modal, and workflow annotation buttons

---

## Desktop Agent Mode

Agent mode lets the AI take full control of the desktop — similar to ChatGPT Atlas, Gemini in Chrome, or Perplexity Comet. The human watches and can interrupt at any point via the HITL gate.

### Entering Agent Mode

```
Human: "Research our top 3 competitors and make me a summary doc"
Agent: "I'll take over. Starting agent mode."
  → AgentModeStarted { task: "research competitors" }
  → Dock AI icon glows blue · menu bar shows "● Researching competitors..."
  → DesktopAction: "open_window:browser"
  → DesktopAction: "type_text:competitor1.com"
  → ActivityUpdate: "● Reading competitor1.com..."
  → (navigate three competitors)
  → SpawnApp { title: "Competitor Summary", widgets: [...] }
  → New DynamicApp window appears with summary + action buttons
  → AgentModeEnded
  → Dock glow off · menu bar clears
```

### Desktop Actions (Agent → Compositor)

| Action string | Effect |
|---|---|
| `open_window:terminal` | Open or focus the Terminal window |
| `open_window:browser` | Open or focus the Browser window |
| `focus_window:<title>` | Bring a named window to front |
| `close_window:<title>` | Close a window by title |
| `type_text:<text>` | Send text to the focused window's input |
| `click:<x>,<y>` | Simulate a click at absolute screen coordinates |
| `press_key:<key>` | Send a keypress to the focused window |

### Dynamic App Spawning

Agent sends `SpawnApp { title, app_id, widgets_json }` over IPC. The compositor creates a floating `DynamicApp` window from the widget tree with no Rust rebuild:

```json
{
  "app_id": "competitor_summary",
  "description": "Competitor research summary",
  "widgets": [
    { "type": "label",        "text": "Top 3 Competitors", "x": 16, "y": 16, "font_size": 14 },
    { "type": "text_display", "content": "1. Acme Corp...", "x": 16, "y": 40, "w": 440, "h": 200 },
    { "type": "button",       "text": "Save as PDF",       "x": 16, "y": 256, "w": 120, "h": 32, "action_id": "save_pdf" }
  ]
}
```

Widget button clicks send `DynamicAppAction { app_id, action_id, window_id }` back to the agent for handling.

### Private Mode

- **Cmd+Shift+P** (macOS) / **F5** (DRM) toggles private mode
- Menu bar shows `[pvt]` indicator with a slightly dimmed tint
- `PrivateModeChanged { active: true }` sent to agent → `DesktopObserver` deactivates → no events recorded
- Agent still responds to explicit prompts; it does not learn from the session

### Workflow Learning

The compositor passively observes desktop events and the agent uses them to understand and eventually automate human workflows:

- Events recorded: window focus changes, window open/close, text-input context (char count only — no actual text)
- Observation pauses automatically in private mode
- "Save as workflow" link appears below plan cards in the sidebar
- Named workflows saved to `~/.soma/workflows.json`
- Agent retrieves history via `desktop_agent.get_workflow_history` to reason about automation opportunities

---

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
| `DesktopEvent` | `event_type`, `window_title`, `timestamp` | Window focus/open/close observation |
| `AnnotateWorkflow` | `name` | Mark recent events as a named workflow |
| `PrivateModeChanged` | `active` | Private mode toggled — disable/enable observer |
| `DynamicAppAction` | `app_id`, `action_id`, `window_id` | Button clicked in a DynamicApp window |
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
| `AgentModeStarted` | `task` | Agent is taking over the desktop |
| `AgentModeEnded` | — | Agent has finished desktop control |
| `SpawnApp` | `title`, `app_id`, `description`, `widgets_json` | Create a new DynamicApp window |
| `UpdateAppWidget` | `window_id`, `widget_updates` | Patch widget state in an open DynamicApp |
| `DesktopAction` | `action` | Drive the desktop (open/close/focus/type/click) |
| `ActivityUpdate` | `text` | Update the menu bar activity strip |
| `Pong` | — | Health check response |

### Rendering Pipeline

Nine-layer compositor render order (back to front):

```
DRM/KMS main loop (bare metal) OR winit event loop (dev)
  → tiny-skia Pixmap (software rasterization)
    → Login screen (if not yet authenticated)
      OR
    1. Desktop wallpaper (two-tone dark gradient)
    2. Floating windows (back to front)
         → Window chrome: shadow · body · title bar · close button · title text · AI badge
         → Window content:
             Terminal → PTY surface
             Browser  → URL bar + headless screenshot
             DynamicApp → widget tree (Label/Button/ProgressBar/TextDisplay)
    3. Agent mode tint (2px accent border around screen when agent_mode=true)
    4. Menu bar (28px): "Soma" · activity dot + text · [pvt] · clock
    5. Dock (72px pill): icons · open dots · hover highlight · agent glow
    6. AI Sidebar overlay (slide animation, positioned at slide_x)
         → Chat history · plan cards · result cards · "Save as workflow" links
         → HITL approval modal (overlaid when plan pending)
    7. Expanded message detail modal
    8. Toast notifications (top-right, fade out)
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
│       ├── observer.rs                 # DesktopObserver: passive workflow recording, persists to ~/.soma/workflows.json
│       └── capabilities/
│           ├── mod.rs                  # Capability trait + registry (loads built-in + user-defined)
│           ├── filesystem.rs           # 9 actions (read_file supports image base64, ~ expansion)
│           ├── process.rs              # 5 process management actions
│           ├── system.rs               # 6 system info actions
│           ├── network.rs              # 5 network diagnostic actions
│           ├── package.rs              # 4 package management actions
│           ├── browser.rs              # 4 browser actions (navigate, get_content, search, screenshot)
│           ├── vision.rs               # analyze_image via qwen2.5-vl:7b
│           ├── meta.rs                 # 3 actions: propose, list_proposed, describe_gap
│           ├── desktop_agent.rs        # Desktop control: start/end agent mode, spawn_app, desktop_action
│           └── script.rs              # ScriptCapability: runtime caps from ~/.soma/capabilities/*.json
│
├── soma-compositor/                    # Compositor binary
│   └── src/
│       ├── main.rs                     # Desktop event loop, AppState, 9-layer redraw, input routing
│       ├── login.rs                    # Full-screen login screen (reads /etc/soma/passwd)
│       ├── renderer.rs                 # tiny-skia + cosmic-text renderer + Theme palette
│       ├── sidebar.rs                  # Chat UI, slide animation, result cards, HITL overlay, workflow annotation
│       ├── terminal.rs                 # PTY terminal emulator
│       ├── browser_panel.rs            # Browser panel (URL bar + headless screenshot)
│       ├── desktop.rs                  # Wallpaper rendering, menu bar rendering
│       ├── dock.rs                     # Dock struct, DockApp, render_dock, hit testing
│       ├── window_manager.rs           # FloatingWindow, WindowContent, AppDef, Widget, chrome render
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
- **The HITL modal is a first-class OS primitive** — rendered by the compositor itself, cannot be bypassed by any app or agent action
- **The desktop is built for agents** — every window, dock state, and menu bar indicator is driven by structured IPC, not pixel-scraping
- **Minimal footprint** — SomaOS boots in seconds with ~100 MB RAM baseline
- **DRM/KMS direct rendering** — no Wayland/X11 server needed, nothing between the agent and the display
- **Agent mode is native** — the same compositor that renders the desktop also routes `DesktopAction` IPC messages. No accessibility API, no OS hooks — the agent is an equal participant at the compositor layer.
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

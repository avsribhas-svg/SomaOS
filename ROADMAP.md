# SomaOS Roadmap

## Completed

### v0.1 — Foundation
- Rust workspace (soma-common, soma-agent, soma-compositor, soma-cli)
- Software-rendered compositor (winit + tiny-skia)
- Agent daemon with Ollama integration
- HITL approval modal
- Buildroot image pipeline

### v0.2 — Capability System
- Structured capability modules (filesystem, process, system)
- Capability registry with dynamic LLM prompt generation
- soma-cli test client
- musl static linking for cross-compilation

### v0.3 — Compositor Chat UI
- Chat-style sidebar with user bubbles and agent cards
- Agent connection from compositor

### v0.4 — UI Polish
- Right-side sidebar layout, scrollable message history
- Rich result cards, agent-triggered redraws

### v0.5 — Smarter Agent
- Network + package capabilities
- Conversation context memory (5 exchanges)
- LLM: qwen2.5-coder:7b with few-shot system prompt

### v0.6 — Rich Compositor
- Real PTY terminal (nix, connected to live shell)
- Click-to-focus panels, resizable panel divider
- Toast notifications, trackpad scroll

### v0.7 — Multimodal Previews
- Image thumbnails in result cards (base64 IPC → tiny-skia draw_pixmap)
- Text file preview with line numbers and file stats
- Directory tree view with sizes, tilde expansion
- Clickable error/result cards with detail modal
- Three-layer intent pipeline (keyword preprocessor → LLM parse → JSON planner)

### v0.8 — Bare-Metal Boot
- DRM/KMS bare-metal backend (drm crate, dumb buffer, double-buffering)
- evdev input (keyboard + mouse, exclusive grab from /dev/input)
- Login screen (boots straight into Soma, reads /etc/soma/passwd)
- Feature-gated builds: `winit-backend` (dev) / `drm-backend` (production)
- Auto-start systemd services (Ollama, agent, compositor)
- First-boot model pull (soma-first-boot.service)

### v0.8.1 — Agent-Assisted Capability Authoring
- `meta` capability: `propose` (generate + save JSON capability definitions), `list_proposed`, `describe_gap`
- `ScriptCapability`: runtime capabilities backed by shell-command templates (`{param}` substitution), loaded from `~/.soma/capabilities/*.json`
- Registry auto-loads user-defined capabilities at startup — no rebuild required
- Human-reviewed capability proposals: the full shell-command definition passes through the HITL gate before being saved to disk
- The self-improvement loop: agent encounters a gap → proposes a capability → human approves via HITL → registry grows → agent's task coverage expands over time
- `~/.soma/gaps.log`: agent records capability gaps it cannot fill, for human review

### v0.9 — Browser Panel + Vision
- Browser panel in compositor left panel (F2 to toggle from PTY terminal)
- URL bar + headless Chromium screenshot rendered in panel
- `browser` agent capability: `navigate`, `get_content`, `search`, `screenshot` (curl + scraper + Chromium headless)
- `vision` agent capability: `analyze_image` via Ollama qwen2.5-vl:7b (tokio block_in_place + async reqwest)
- `BrowserUpdate { url, title, screenshot_base64 }` IPC message; compositor auto-switches to browser panel on navigation
- Capabilities grow to 35 built-in actions (+4 browser, +1 vision)

---

## Planned

### v1.0 — Desktop Environment + Desktop Agent Mode *(in progress)*

This version transforms SomaOS from a fixed terminal+sidebar split into a full macOS-style desktop where the AI operates as a native user — not an external tool bolted on. The human supervises from the same environment.

**Desktop Environment**
- Floating window manager: Terminal, Browser, and agent-spawned `DynamicApp` windows with drag, focus, and close
- macOS-style dock at the bottom: app launchers (Terminal, Browser, AI Agent, Sidebar, Private), open-state indicator dots, hover highlights, agent-mode glow ring
- Menu bar (28px): "Soma" label · live activity strip (agent status dot + task text) · private-mode lock · clock
- Desktop wallpaper (two-tone dark gradient)
- AI sidebar becomes a slide-in overlay (800px/s tween animation), not a fixed panel — toggled via dock or Cmd+Space
- Terminal and Browser are applications, not panels — agents and humans use the same floating-window primitives

**Desktop Agent Mode**
- Agent can take full control of the desktop: open/close/focus windows, type text, click coordinates, navigate the browser — via `DesktopAction` IPC messages
- Agent signals entry/exit: `AgentModeStarted { task }` → dock AI icon glows blue, menu bar shows live task text; `AgentModeEnded` → all indicators clear
- `desktop_agent` capability: `start_agent_mode`, `end_agent_mode`, `spawn_app`, `desktop_action`, `get_workflow_history`
- HITL gate continues to apply — dangerous actions surface an approval modal mid-agent-session

**Dynamic App Spawning**
- Agent can spawn new application windows at runtime without any Rust rebuild: `SpawnApp { title, app_id, widgets_json }`
- `DynamicApp` windows contain a declarative widget tree (Label, Button, ProgressBar, TextDisplay) serialised as JSON over IPC
- Agent-owned windows display a teal "AI" badge in their title bar
- `UpdateAppWidget` IPC allows the agent to patch widget state (e.g. update a progress bar, swap text) while the window is open
- Apps can be promoted to persistent definitions saved at `~/.soma/apps/<app_id>.json`

**Workflow Learning**
- `DesktopObserver`: passive observation of window focus, open, close, and text-input events — never records actual text content, only context and char counts
- Observation automatically pauses in private mode (`PrivateModeChanged { active: true }`)
- Human or agent can annotate a sequence of events as a named workflow (`AnnotateWorkflow { name }`) — stored at `~/.soma/workflows.json`
- "Save as workflow" link appears below plan cards and execution-complete cards in the sidebar
- `get_workflow_history` capability returns structured workflow patterns to the agent for reasoning about automation opportunities

**Private Mode**
- Cmd+Shift+P (macOS dev) / F5 (DRM bare metal) toggles private mode
- Menu bar shows `[pvt]` indicator and a slightly dimmed bar tint
- `PrivateModeChanged` sent to agent → observer deactivates → no events recorded
- Agent still responds to explicit prompts; it just doesn't learn from the session

**New IPC messages**
- Compositor → Agent: `DesktopEvent`, `AnnotateWorkflow`, `PrivateModeChanged`, `DynamicAppAction`
- Agent → Compositor: `AgentModeStarted`, `AgentModeEnded`, `SpawnApp`, `UpdateAppWidget`, `DesktopAction`, `ActivityUpdate`

**New Keyboard Shortcuts**

| Action | macOS dev | DRM bare metal |
|--------|-----------|----------------|
| Toggle AI sidebar | Cmd+Space | F3 |
| Open Terminal | Cmd+T | F1 |
| Close window | Cmd+W | F2 |
| Enter/exit agent mode | Cmd+Shift+A | F4 |
| Toggle private mode | Cmd+Shift+P | F5 |

**New Compositor Modules**
- `window_manager.rs` — `FloatingWindow`, `WindowContent`, `AppDef`, `Widget`, chrome rendering, dynamic app rendering
- `dock.rs` — `Dock`, `DockApp`, `DockAction`, pill geometry, hit testing, sync state, rendering
- `desktop.rs` — wallpaper rendering, menu bar rendering
- `soma-agent/src/observer.rs` — `DesktopObserver`, `WorkflowPattern`, `DesktopEvent`, persistence
- `soma-agent/src/capabilities/desktop_agent.rs` — agent desktop control capability

### v1.0.5 — AgentAPI + Native App Framework
- `AgentAPI` trait in soma-common: `describe_state`, `execute_action`, `subscribe_changes`
- `soma-sheets`: spreadsheet with full agent read/write API (cells, formulas, ranges)
- `soma-docs`: document editor with structured agent API (paragraphs, headings, tables)
- Apps register with the compositor via `AgentAPI`; agent reads and writes app state directly — no screen-scraping

### v1.1 — Capability Registry Governance
- Hot-reload: agent reloads user-defined capabilities without a full restart
- Registry UI: compositor panel showing all capabilities (built-in + user-defined), with enable/disable and delete
- Gap detection during task execution: when a step fails due to a missing capability, agent automatically proposes a fix and surfaces it for HITL review
- Capability promotion: convert a stable JSON-defined capability into a built-in Rust capability via scaffolded code generation
- Version tracking: each capability definition carries a version field; updates require fresh HITL approval

### v1.2 — Media + Generation
- `soma-media`: image/video generation pipeline (local diffusion)
- Media result cards in sidebar (video playback, image gallery)
- Agent capabilities: `generate_image`, `generate_video`, `generate_audio`

### v1.3 — Federation
- Network IPC: TCP/TLS transport over the existing JSON socket protocol
- Node registry: agents discover peers via config or DNS-SD
- `delegate` capability: orchestrator sends tasks to remote nodes, streams results back
- Unified HITL queue aggregated on the orchestrator node
- Token-based node auth with scoped permissions

### v2.0 — USB Bootable + Plugin API
- USB installer (GRUB + EFI, fits on 8 GB USB)
- Plugin API: third-party capabilities as shared Rust dylibs or WASM modules
- Seccomp + AppArmor sandboxing per capability
- OTA update system (delta images, signed, agent-driven)

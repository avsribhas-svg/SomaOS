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

### v0.8.1 — Agent-Assisted Capability Authoring *(current)*
- `meta` capability: `propose` (generate + save JSON capability definitions), `list_proposed`, `describe_gap`
- `ScriptCapability`: runtime capabilities backed by shell-command templates (`{param}` substitution), loaded from `~/.soma/capabilities/*.json`
- Registry auto-loads user-defined capabilities at startup — no rebuild required
- Human-reviewed capability proposals: the full shell-command definition passes through the HITL gate before being saved to disk
- The self-improvement loop: agent encounters a gap → proposes a capability → human approves via HITL → registry grows → agent's task coverage expands over time
- `~/.soma/gaps.log`: agent records capability gaps it cannot fill, for human review

---

## Planned

### v0.9 — Browser Panel + Vision
- Embed WebKitGTK offscreen into compositor framebuffer (no Wayland/X11 needed)
- `browser` agent capability: `navigate`, `query_selector`, `eval`, `screenshot_region`
- Vision fallback: multimodal LLM (qwen2.5-vl) for unknown UIs
- Basic tab management in compositor browser panel

### v1.0 — Native App Framework
- `AgentAPI` trait in soma-common: `describe_state`, `execute_action`, `subscribe_changes`
- `soma-sheets`: spreadsheet with full agent read/write API (cells, formulas, ranges)
- `soma-docs`: document editor with structured agent API (paragraphs, headings, tables)
- Compositor multi-panel layout (terminal | browser | native app | sidebar)
- App launcher driven by agent intent

### v1.0.5 — Capability Registry Governance
- Hot-reload: agent reloads user-defined capabilities without a full restart
- Registry UI: compositor panel showing all capabilities (built-in + user-defined), with enable/disable and delete
- Gap detection during task execution: when a step fails due to a missing capability, agent automatically proposes a fix and surfaces it for HITL review
- Capability promotion: convert a stable JSON-defined capability into a built-in Rust capability via scaffolded code generation
- Version tracking: each capability definition carries a version field; updates require fresh HITL approval

### v1.1 — Media + Generation
- `soma-media`: image/video generation pipeline (local diffusion)
- Media result cards in sidebar (video playback, image gallery)
- Agent capabilities: `generate_image`, `generate_video`, `generate_audio`

### v1.2 — Federation
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

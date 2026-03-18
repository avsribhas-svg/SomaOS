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

### v1.0 — Desktop Environment + Desktop Agent Mode
- Full floating-window desktop: dock, menu bar, agent mode, private mode, dynamic app spawning
- Desktop agent mode: AI takes control via `DesktopAction` IPC, `AgentModeStarted`/`AgentModeEnded`
- Dynamic app spawning: `SpawnApp` with JSON widget trees (Label, Button, ProgressBar, TextDisplay)
- Workflow learning: `DesktopObserver`, workflow annotation, persistence to `~/.soma/workflows.json`
- Private mode: `Cmd+Shift+P` / F5, observation pauses, `[pvt]` indicator
- Multi-provider LLM brain: native tool calling across Ollama, Anthropic, OpenAI, Gemini
- 36 built-in capability actions across 10 modules (filesystem, process, system, network, package, browser, vision, meta, desktop_agent, + user-defined)

### v1.0.1 — Compositor Extraction + Typed Failure

- `event_handler.rs` extracted — all keyboard/mouse/scroll input dispatch
- `compositor.rs` extracted — 9-layer render stack + animation/state sync
- `settings_app.rs` extracted — settings floating window UI
- `main.rs` shrunk from ~1,340 → 712 lines
- `CapabilityError { reason, context, alternatives }` — structured typed errors across all 11 capability modules; replaces raw strings
- Agent can programmatically reason about failure: retry, escalate via HITL, or try an alternative path

### v1.0.2 — Bug Fixes + Code Health

- **Settings dock indicator**: Settings window `is_open` dot now correctly syncs via `has_settings` param
- **UTF-8 safety**: `truncate_str` switched from byte-index to char-based truncation (was a panic risk on multi-byte characters)
- **Terminal**: `CString::new().unwrap()` in PTY spawn replaced with graceful `/bin/sh` fallback; version string updated to v1.0
- **IPC reconnection**: Compositor now detects agent disconnection via `tx.is_closed()` and retries every 5 seconds automatically
- **Observer save errors**: `DesktopObserver::save()` now logs `warn!` on `create_dir_all` or `write` failure instead of silently swallowing errors
- **Shared config loader**: Duplicate `load_config_values()` extracted to `config_loader.rs`; both `sidebar.rs` and `settings_app.rs` now import it
- **Nested unwraps**: Replaced silent `.unwrap()` on hardcoded CSS selectors (browser.rs) and socket address (network.rs) with descriptive `.expect()`

---

## Planned

### v1.1 — AgentAPI + soma-sheets + Session Model ← THESIS MILESTONE

This is where SomaOS becomes a different computing paradigm. Both the human and the AI are first-class users of the same desktop — apps must be **dual-interface**: a good GUI for the human *and* a structured API for the agent.

**AgentAPI**
- `AgentAPI` trait in soma-common: `describe_state`, `execute_action`, `subscribe_changes`
- `WindowContent::NativeApp` variant wrapping `Box<dyn AgentAPI>` in the compositor
- New IPC: `AppStateQuery`, `AppAction`, `AppStateChanged`

**soma-sheets (first dual-interface app)**
- **Human**: click cells, type values, Tab/Enter navigation, formula bar — standard spreadsheet UX
- **Agent**: `read_range`, `write_cell`, `apply_formula` via structured `AgentAPI` — no screen-scraping
- Both share the same data model; edits from either side are immediately visible to the other
- Agent capability: `sheets` with actions mapped to `AgentAPI::execute_action`

**Agent-native primitive: Session Model**
- Agent sessions become first-class OS objects: intent, scope, history, affected resources
- `AgentModeStarted { task }` extended with scope (capability whitelist, directory whitelist)
- Session history persists beyond the 5-exchange conversation window
- Human can inspect any active session from the sidebar

### v1.2 — soma-docs + Capability Governance + Semantic FS

Once AgentAPI exists and soma-sheets proves the dual-interface pattern:
- `soma-docs`: document editor (paragraphs, headings, tables, code blocks) — same dual-interface pattern
- Capability governance: hot-reload, registry UI, gap detection, capability promotion, version tracking

**Agent-native primitive: Semantic File System Layer**
- Lightweight metadata per file: what created it, what workflows touched it, agent-generated tags/descriptions
- Agent navigates by intent ("the spreadsheet I was working on") rather than path
- Implementation: either `.soma-meta` sidecars (portable) or `~/.soma/index.db` (queryable) — TBD
- New capability: `semantic_fs` with `describe_file`, `find_by_intent`, `tag`, `get_history`

### v1.3 — Media + Generation + Parallel Task Contexts
- `soma-media`: image/video generation pipeline (local diffusion)
- Media as `AgentAPI` apps with structured state, not just chat cards
- Agent capabilities: `generate_image`, `generate_video`, `generate_audio`

**Agent-native primitive: Parallel Task Contexts**
- Multiple concurrent agent sessions with isolated capability scopes
- Human can see and interrupt any session from the dock/sidebar
- Sessions share data through the semantic FS layer
- HITL queue aggregates approvals from all active sessions
- IPC protocol extended with session IDs and scope tokens

### v1.4 — Federation
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



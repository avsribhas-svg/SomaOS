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

### v1.1 — AgentAPI + Dual-Interface Apps + Semantic FS ← THESIS MILESTONE ✅

**AgentAPI Core**
- `NativeAppContent` trait: `describe_state`, `execute_action`, input/render hooks
- `WindowContent::NativeApp(Box<dyn NativeAppContent>)` — compositor dual-interface window type
- `AppState { summary, cells, dirty }` + `AppStateCache` shared between IPC and capabilities
- `AppStateChanged` + `AppAction` IPC variants — compositor pushes state to agent on every edit

**Dual-Interface Apps**
- `soma-sheets`: spreadsheet with formula evaluator (SUM/AVG/MIN/MAX/COUNT, A1 notation), formula bar, Tab/Enter/Arrow navigation, number right-align
- `soma-docs`: block-based document editor (paragraphs, headings, code blocks)
- Both expose the same data model to human (GUI) and agent (structured API) simultaneously

**New Agent Capabilities (13 modules total)**
- `sheets`: create, describe, read_range, write_cell, apply_formula
- `docs`: create, describe, write_block, read_blocks
- `semantic_fs`: tag, annotate, find_by_intent, list_tagged, describe_file, get_history
- Session tracking: `Session` + `SessionStep` persisted to `~/.soma/sessions/<id>.json`

**Test Suite + Agent Robustness**
- 61/61 scenario integration test suite covering all 13 modules (100% pass rate)
- Layer 0 fast-path: 30+ keyword interceptors, 0ms latency for unambiguous intents
- Ollama text-fallback parser: bare params, `{"function":}`, `{"cmd":}`, `{"functions":[]}` wrappers
- macOS capability fixes: hostname/uptime/network_status/kernel_info (was Linux-only `/proc/*`)
- `--filter <prefix>`, `--concurrency N`, grouped failure summary in soma-cli

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

### v1.2 — Capability Governance + Advanced Sessions + soma-media

Building on the dual-interface pattern proven by v1.1:
- Capability governance: hot-reload UI, version tracking, capability promotion (script → built-in)
- Advanced session model: scope boundaries (capability whitelist, directory whitelist), parallel contexts
- Semantic FS persistence: decide `.soma-meta` sidecars (portable) vs. `~/.soma/index.db` (queryable)
- `soma-media`: image/video generation as a third dual-interface app (local diffusion)

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



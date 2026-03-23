# CLAUDE.md — SomaOS

> Read this before touching any code. Updated 2026-03-22 (v1.2 complete — Capability Governance + Session Scope + Semantic FS SQLite + soma-media, 70/70 tests passing).

---

## What This Is

**SomaOS** is an AI-native operating system where the agent is a first-class user of the desktop — not a chatbot bolted on top of Linux. Both the human and the agent are **peer co-inhabitants** of the same environment:

- **Human interface**: pixels — dock clicks, window drags, keyboard shortcuts
- **Agent interface**: structured APIs — `AgentAPI`, IPC messages, capability actions
- **Shared**: same apps, same windows, same data models
- **HITL**: conflict resolution for a shared space, not just a permission gate

This is a thesis project. Every architectural decision should serve the core thesis, not general-purpose OS engineering.

---

## Language & Stack

- **Primary**: Rust workspace (`soma-common`, `soma-agent`, `soma-compositor`, `soma-cli`)
- **Secondary frontend**: React + Tauri 2 in `/soma/` — macOS dev only, not production
- **OS image**: Buildroot (x86_64 + ARM64) in `/buildroot/`
- **CI**: GitHub Actions (`.github/workflows/build.yml`) — builds both arch images on push to main

---

## Workspace Layout

```
soma-common/        Shared IPC types (TaskPlan, BrowserUpdate, CompositorMessage, AgentMessage)
soma-agent/         Agent daemon (tokio, LlmProvider trait, capability registry)
soma-compositor/    Compositor binary (DRM/KMS + tiny-skia + cosmic-text)
soma-cli/           CLI test client
buildroot/          OS image build system (soma_defconfig)
docs/               ANALYSIS.md, BUILD_ARM64.md, BUILD_x86_64.md, WINDOWS_BUILD.md
soma/               React + Tauri 2 macOS dev frontend
```

---

## Current Version: v1.2 (as of 2026-03-22)

### What was built in v1.2

**Capability Governance**
- `meta.list_governance`: lists all built-in + script capabilities with type/version/load metadata
- `meta.promote`: generates a Rust stub at `~/.soma/promotions/<name>.rs` for promoting script caps to built-ins
- Sidebar Settings tab: Reload button for hot-reloading script capabilities from `~/.soma/capabilities/`

**Advanced Session Model**
- `SessionScope { capability_whitelist: Option<Vec<String>>, path_whitelist: Option<Vec<String>> }` added to soma-common
- `AgentModeStarted` now carries `scope: Option<SessionScope>`
- `GetSessionStatus` / `SessionStatusResponse` IPC round-trip
- Scope enforcement in `ipc.rs`: capability whitelist + path whitelist checked before each step execution
- `desktop_agent.start_agent_mode` accepts optional `scope` param
- `desktop_agent.get_session_status` action added
- Sidebar Chat tab shows active session card (task name + scope)

**Semantic FS — SQLite backend**
- `rusqlite` (bundled) replaces `.soma-meta` sidecar files
- `~/.soma/index.db`: SQLite database with `files` table (path, description, tags, created_by, created_at, history)
- Best-effort migration from existing `.soma-meta` sidecar files on first run
- Same `Capability` interface (action names/params unchanged) — only the storage backend changed

**soma-media (third dual-interface app)**
- `MediaApp` implementing `NativeAppContent`: prompt bar (top), image display (center), status strip (bottom)
- `on_key("Enter")` submits prompt; `set_image` accepts base64 PNG bytes
- `media` capability: `generate` (command pattern → `AppAction`), `describe`, `save`
- `WindowContentType::Media`, dock entry, event routing in `event_handler.rs`
- Layer 0 fast-path patterns for `media_generate`, `media_describe`, `media_save`

**Test Suite**
- 61 → 70 scenarios covering all 14 capability modules (70/70 passing)

---

### What was built in v1.1

**AgentAPI + Dual-Interface Apps**
- `AgentAPI` / `NativeAppContent` trait: `describe_state`, `execute_action`, `on_char/on_key/on_click/render`
- `WindowContent::NativeApp(Box<dyn NativeAppContent>)` — first dual-interface window type
- `AppState`, `AppStateCache`, `AppStateChanged`, `AppAction` IPC — compositor pushes state to agent on every edit
- `soma-sheets`: spreadsheet with formula evaluator (SUM/AVG/MIN/MAX/COUNT, cell refs, ranges), Tab/Enter/Arrow navigation, formula bar, number right-align, text left-align
- `soma-docs`: block-based document editor (paragraphs, headings, code blocks), same dual-interface pattern
- `sheets` agent capability: `create`, `describe`, `read_range`, `write_cell`, `apply_formula`
- `docs` agent capability: `create`, `describe`, `write_block`, `read_blocks`
- `semantic_fs` capability: `tag`, `annotate`, `find_by_intent`, `list_tagged`, `describe_file`, `get_history`

**Test Suite + Agent Robustness**
- 61/61 scenario integration test suite covering all 13 capability modules
- Layer 0 fast-path: 30+ keyword interceptors — unambiguous intents never hit the LLM (0ms)
- Ollama text-fallback parser: handles bare params, `{"function":}`, `{"cmd":}`, `{" functions":[]}` wrappers
- Session tracking in `ipc.rs`: `Session` + `SessionStep` persisted to `~/.soma/sessions/`

---

### What was built in v1.0

**Multi-provider LLM brain**
- `soma-agent/src/providers/`: `LlmProvider` trait + Ollama, Anthropic, OpenAI, Gemini impls using native tool/function calling
- Tool names use `capability__action` namespace; `build_tools()` converts `CapabilityRegistry` to function schemas
- Replaced 3-layer JSON prompt pipeline with single `provider.tool_call()` — significantly more reliable
- `soma-agent/src/config.rs`: `SomaConfig` persisted to `~/.soma/config.toml`
- Settings tab in sidebar: provider radio, model/key/url fields, save sends `UpdateConfig` IPC

**Desktop Environment (Phases 1–5)**
- Floating window manager: Terminal, Browser, DynamicApp windows with drag, focus, close
- macOS-style dock (bottom): app launchers, open-state dots, agent-mode glow ring
- Menu bar (28px): Soma label · activity dot+text · [pvt] · clock
- 9-layer compositor render stack: wallpaper → windows → agent tint → menu bar → dock → sidebar → HITL → modal → toasts
- Sidebar is a slide-in overlay (800px/s tween), not a fixed panel
- Desktop agent capability (`desktop_agent`): start/end agent mode, spawn apps, desktop actions, workflow history
- `DesktopObserver` (`observer.rs`): passive event recording, workflow annotation, `~/.soma/workflows.json`
- Private mode: `Cmd+Shift+P` (winit) / F5 (DRM) — observer deactivates, menu bar shows `[pvt]`
- Command pattern in `desktop_agent.rs`: capability returns `ipc_message` key in data; IPC handler in `ipc.rs` forwards it

**Capabilities**: 36 built-in actions across 10 modules (filesystem, process, system, network, package, browser, vision, meta, desktop_agent, + user-defined JSON)

*(v1.1 adds sheets, docs, semantic_fs → 40+ actions across 13 modules)*

---

## Architecture Diagram

```
soma-compositor (DRM/KMS + tiny-skia + cosmic-text)
  ├── Login screen (evdev keyboard, /etc/soma/passwd)
  ├── Floating windows: Terminal, Browser, DynamicApp (drag, focus, close)
  ├── Dock (bottom): Terminal, Browser, AI Agent, Sidebar, Private
  ├── Menu bar (28px): Soma · activity · [pvt] · clock
  ├── Slide-in Sidebar (overlay): Chat | Settings tabs
  └── Center overlay: HITL approval modal
       ↓ Unix socket /tmp/soma-agent.sock (newline-delimited JSON)
soma-agent (tokio → LlmProvider trait)
  ├── providers/: Ollama, Anthropic, OpenAI, Gemini (native tool calling)
  ├── config.rs: SomaConfig → ~/.soma/config.toml
  ├── observer.rs: DesktopObserver (event recording, workflow patterns)
  └── capabilities/: filesystem, process, system, network, package,
                     browser, vision, meta, script, desktop_agent,
                     sheets, docs, semantic_fs, media
```

---

## Key Files

| File | Role |
|------|------|
| `soma-compositor/src/main.rs` | Event loop, 9-layer render, priority mouse routing, floating windows (712 lines post-v1.0.1 split) |
| `soma-compositor/src/compositor.rs` | 9-layer render stack, animation/state sync, dock sync |
| `soma-compositor/src/event_handler.rs` | Keyboard/mouse/scroll input dispatch |
| `soma-compositor/src/settings_app.rs` | Settings floating window UI |
| `soma-compositor/src/config_loader.rs` | Shared `load_config_values()` — reads `~/.soma/config.toml` |
| `soma-compositor/src/window_manager.rs` | FloatingWindow, WindowContent, AppDef, Widget, chrome rendering |
| `soma-compositor/src/dock.rs` | Dock, DockApp, pill geometry, hit testing, open-state sync |
| `soma-compositor/src/desktop.rs` | Wallpaper gradient, menu bar rendering |
| `soma-compositor/src/sidebar.rs` | Chat UI, Settings tab, HITL modal, slide-in animation |
| `soma-compositor/src/renderer.rs` | tiny-skia pipeline (dimension guards) |
| `soma-compositor/src/browser_panel.rs` | URL bar + headless screenshot panel |
| `soma-compositor/src/audio.rs` | AudioRecorder (cpal), transcribe_in_background, speak_async |
| `soma-compositor/src/terminal.rs` | PTY terminal emulator |
| `soma-agent/src/providers/mod.rs` | LlmProvider trait, ToolDef, ToolCall, make_provider() factory |
| `soma-agent/src/config.rs` | SomaConfig, Provider enum, persisted to ~/.soma/config.toml |
| `soma-agent/src/intent.rs` | Tool call pipeline (Layer 0 fast path + provider.tool_call()) |
| `soma-agent/src/ipc.rs` | Unix socket server, IPC message dispatch, BrowserUpdate/DesktopAction emission |
| `soma-agent/src/observer.rs` | DesktopObserver, WorkflowPattern, observe/annotate/persist |
| `soma-agent/src/capabilities/desktop_agent.rs` | 7-action desktop control (command pattern → IPC layer sends); includes get_session_status |
| `soma-agent/src/capabilities/meta.rs` | propose/list/gap-log/list_governance/promote for self-improvement loop + capability governance |
| `soma-agent/src/capabilities/script.rs` | JSON-defined shell-template caps, hot-loaded from ~/.soma/capabilities/ |
| `soma-agent/src/capabilities/sheets.rs` | 5-action spreadsheet control (create, describe, read_range, write_cell, apply_formula) |
| `soma-agent/src/capabilities/docs.rs` | Document editing capability (create, describe, write_block, read_blocks) |
| `soma-agent/src/capabilities/semantic_fs.rs` | File metadata, intent-based discovery (tag, annotate, find_by_intent, list_tagged, describe_file, get_history) — SQLite-backed via rusqlite |
| `soma-compositor/src/sheets.rs` | SheetsApp: dual-interface spreadsheet (NativeAppContent + formula evaluator + GUI) |
| `soma-compositor/src/docs.rs` | DocsApp: dual-interface document editor (NativeAppContent + block model + GUI) |
| `soma-compositor/src/media.rs` | MediaApp: dual-interface image generation (NativeAppContent + prompt bar + image display) |
| `soma-agent/src/capabilities/media.rs` | media capability: generate/describe/save (command pattern for generate) |
| `soma-common/src/lib.rs` | All shared IPC types (TaskPlan, BrowserUpdate, CompositorMessage, AgentMessage, AppState) |
| `buildroot/soma_defconfig` | Buildroot OS config |
| `.github/workflows/build.yml` | GitHub Actions CI (x86_64 + ARM64 image builds) |
| `docs/ANALYSIS.md` | Living architecture analysis — read this for context on design decisions |
| `ROADMAP.md` | Full version history and planned milestones |

---

## Important Gotchas

1. **`cpal::Stream` is NOT `Send` on macOS CoreAudio** — must drop stream on main thread before spawning a background thread.
2. **tiny-skia expects premultiplied RGBA** — must convert straight alpha from the `image` crate before creating a `Pixmap`.
3. **Vision capability uses `tokio::task::block_in_place` + `Handle::current().block_on(...)`** — requires the multi-thread runtime.
4. **Browser screenshots** saved to `/tmp/soma-browser.png`; env `SOMA_VISION_MODEL` overrides vision model (default `qwen2.5-vl:7b`).
5. **Ollama must be running locally** at `http://localhost:11434` with `qwen2.5-coder:7b` + `qwen2.5-vl:7b` pulled.
6. **`desktop_agent` command pattern**: the capability returns an `ipc_message` key in its data map; the IPC handler in `ipc.rs` reads that key and forwards it to the compositor. Don't bypass this — it's intentional.
7. **Feature-gated backends**: `winit-backend` for macOS/Linux dev, `drm-backend` for production bare-metal. Every new path that touches display/input must be gated correctly.
8. **`main.rs` is 712 lines** — split was done in v1.0.1 into `compositor.rs` + `event_handler.rs` + `settings_app.rs` + `config_loader.rs`. Keep it below 800 lines; don't consolidate back.

---

## Self-Improvement Loop

1. Agent calls `meta.propose` → generates JSON capability definition → saves to `~/.soma/capabilities/`
2. Human reviews/approves via HITL gate
3. `ScriptCapability` hot-loads it at next agent startup
4. `meta.describe_gap` logs unmet capability requests to `~/.soma/gaps.log` for later promotion to built-in

Never remove the HITL gate from the `meta.propose` flow — it's a safety invariant.

---

## Before Every Commit — Required Checklist

### 1. Pre-flight CI scan

See [docs/LOCAL_CI.md](docs/LOCAL_CI.md) for the full local build + test workflow (replaces GitHub Actions when minutes are exhausted).

```bash
# Native build (quick sanity check)
cargo build -p soma-agent
cargo build -p soma-compositor

# Cross-compile check (what CI actually runs)
cargo check -p soma-agent --target x86_64-unknown-linux-musl
cargo check -p soma-agent --target aarch64-unknown-linux-musl
cargo check -p soma-compositor --features drm-backend --target x86_64-unknown-linux-musl
cargo check -p soma-compositor --features drm-backend --target aarch64-unknown-linux-musl
```

Things to verify:
- New Cargo deps: do they cross-compile? Do they require system libs (pkg-config)?
- New files: does the buildroot overlay need updating?
- macOS-only APIs used outside `#[cfg(target_os = "macos")]` guards?
- All `#[cfg(feature = "drm-backend")]` paths compile for both musl targets?

### 2. Update docs with every change
Always update these to reflect the current state of the code:
- `README.md` — architecture diagram, capability count, new features
- `ROADMAP.md` — move completed items from Planned → Done
- `docs/BUILD_ARM64.md` — new deps or build steps for ARM64
- `docs/BUILD_x86_64.md` — new deps or build steps for x86_64

---

## End of Every Session — Required

Before closing out any session (whether or not a commit was made), recheck and update **all `.md` files** in the repo:

- `README.md` — does it still accurately describe the codebase? Update architecture, capabilities, features.
- `ROADMAP.md` — mark anything completed; update in-progress items.
- `docs/ANALYSIS.md` — update architecture risks, codebase health metrics, open design questions if anything changed.
- `docs/BUILD_ARM64.md` / `docs/BUILD_x86_64.md` — any new deps, env vars, or build steps?
- `CLAUDE.md` (this file) — update key files table, gotchas, what's next, or anything else that changed.

This applies even if the session was read-only or exploratory — if understanding of the codebase improved, the docs should reflect it.

---

## What's Next

### v1.3 — Parallel Task Contexts + soma-media Backend

v1.2 proved capability governance, session scope, SQLite semantic FS, and the soma-media dual-interface shell. Next:
- `soma-media` backend: actual diffusion model integration (local Stable Diffusion or similar) replacing stub
- Parallel Task Contexts: multiple concurrent agent sessions with isolated capability scopes, HITL queue aggregation, session IDs in IPC protocol
- Server-side session scope enforcement (move from advisory/client-side to compositor-enforced)
- Session scope UI: human can see and interrupt any active session from the dock/sidebar

### v1.4 — Federation
- Network IPC: TCP/TLS transport over the existing JSON socket protocol
- Node registry, `delegate` capability, unified HITL queue on orchestrator

---

## Architecture Risks (from docs/ANALYSIS.md)

| Risk | Severity | Mitigation |
|---|---|---|
| ~~`main.rs` complexity (1,342 lines)~~ | ~~**High**~~ | ✅ Resolved v1.0.1 — extracted to compositor.rs + event_handler.rs; main.rs is 712 lines |
| ~~AgentAPI `describe_state` design~~ | ~~**High**~~ | ✅ Resolved v1.1 — SheetsApp + DocsApp prove the pattern; summary + full cells in AppState |
| Human + agent editing same data model concurrently | **Medium** | v1.1 uses last-write-wins; v1.2 adds session scope enforcement; OT/CRDT deferred to v1.3 |
| Session scope enforcement is advisory (client-side) | **Medium** | The agent enforces scope but a compromised agent could bypass it; server-side enforcement needed in v1.3 |
| DynamicApp widget tree growing into a full UI framework | Medium | Keep minimal: status surfaces for agent, not apps for humans |
| ~~No automated tests~~ | ~~Medium~~ | ✅ Resolved — 70-scenario integration test suite (soma-cli --test), 100% pass rate |

---

## Open Design Questions

1. ~~What should `AgentAPI::describe_state` return for a spreadsheet?~~ ✅ Answered — `AppState { summary, cells }` with compact summary for orchestrators + full cell map for workers.
2. ~~How do human edits and agent writes coexist on the same data model?~~ v1.2 adds session scope enforcement; last-write-wins still applies within a session. OT/CRDT deferred to v1.3.
3. ~~What does an agent session scope look like?~~ ✅ Answered — `SessionScope { capability_whitelist, path_whitelist }` in soma-common; enforced in `ipc.rs` before each step execution.
4. ~~Should semantic FS metadata live as sidecars or a central index?~~ ✅ Answered — SQLite at `~/.soma/index.db` (queryable, sidecar migration on first run).
5. When should the agent auto-recover from typed failure vs. escalate to HITL? Low-risk retries auto; anything touching user data escalates.

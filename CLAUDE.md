# CLAUDE.md — SomaOS

> Read this before touching any code. Updated post v1.0 (2026-03-12).

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

## Current Version: v1.0.2 (stable as of 2026-03-15)

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
                     browser, vision, meta, script, desktop_agent
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
| `soma-agent/src/capabilities/desktop_agent.rs` | 6-action desktop control (command pattern → IPC layer sends) |
| `soma-agent/src/capabilities/meta.rs` | propose/list/gap-log for self-improvement loop |
| `soma-agent/src/capabilities/script.rs` | JSON-defined shell-template caps, hot-loaded from ~/.soma/capabilities/ |
| `soma-common/src/lib.rs` | All shared IPC types (TaskPlan, BrowserUpdate, CompositorMessage, AgentMessage) |
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

### Thesis Milestone: v1.1 — AgentAPI + soma-sheets + Session Model
- `AgentAPI` trait in soma-common: `describe_state`, `execute_action`, `subscribe_changes`
- `WindowContent::NativeApp` variant wrapping `Box<dyn AgentAPI>` in the compositor
- `soma-sheets`: first dual-interface app — good GUI for human + structured `AgentAPI` for agent, same data model
- Session model: agent sessions become first-class OS objects with intent, scope, history, affected resources

---

## Architecture Risks (from docs/ANALYSIS.md)

| Risk | Severity | Mitigation |
|---|---|---|
| ~~`main.rs` complexity (1,342 lines)~~ | ~~**High**~~ | ✅ Resolved v1.0.1 — extracted to compositor.rs + event_handler.rs; main.rs is 712 lines |
| AgentAPI `describe_state` design | **High** | Prototype with soma-sheets; the answer shapes all future apps |
| Human + agent editing same data model concurrently | **High** | Design conflict resolution in v1.1 (cell-level locking? last-write-wins? OT?) |
| DynamicApp widget tree growing into a full UI framework | Medium | Keep minimal: status surfaces for agent, not apps for humans |
| No automated tests | Medium | Add capability unit tests before v1.2 |

---

## Open Design Questions

1. What should `AgentAPI::describe_state` return for a spreadsheet? This defines the contract for all future apps.
2. How do human edits and agent writes coexist on the same data model? Cell-level locking? Last-write-wins? OT/CRDT?
3. What does an agent session scope look like? JSON config? Capability whitelist? Directory whitelist?
4. Should semantic FS metadata live as sidecars (`.soma-meta`) or in a central index (`~/.soma/index.db`)? Sidecars are portable; index is queryable.
5. When should the agent auto-recover from typed failure vs. escalate to HITL? Low-risk retries auto; anything touching user data escalates.

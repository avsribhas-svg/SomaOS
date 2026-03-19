# SomaOS — Architecture Analysis

> **Living document.** Updated across agent sessions for cross-agent transparency. Any AI agent working on SomaOS should read this first.

*Last updated: 2026-03-19 — post v1.1 completion (AgentAPI + soma-sheets + soma-docs + semantic_fs, 61/61 tests)*

---

## Core Thesis

Both the human and the AI are **first-class co-inhabitants of the same desktop**. Not master-servant. Not tool-user. Peer operators with different interface modalities into one shared environment.

- **Human interface**: pixels — dock clicks, window drags, keyboard shortcuts
- **Agent interface**: structured APIs — `AgentAPI`, IPC messages, capability actions
- **Shared**: same apps, same windows, same data models
- **HITL**: conflict resolution for a shared space, not a permission gate

---

## What Exists (v1.1)

### Supervision Layer (human's interface)
| Component | Status | Notes |
|---|---|---|
| Floating window manager | ✅ | Terminal, Browser, DynamicApp windows with drag/focus/close |
| Dock (72px pill) | ✅ | App launchers, open-state dots, agent-mode glow |
| Menu bar (28px) | ✅ | Soma label, activity strip, private mode, clock |
| AI Sidebar (slide overlay) | ✅ | Chat, plan cards, result cards, HITL modal, workflow annotation |
| Desktop wallpaper | ✅ | Two-tone dark gradient |
| Keyboard shortcuts | ✅ | Cmd+Space/T/W/Shift+A/Shift+P (winit), F1–F5 (DRM) |

### Agent Layer (agent's interface)
| Component | Status | Notes |
|---|---|---|
| Capability registry | ✅ | 13 modules (filesystem, process, system, network, package, browser, vision, meta, script, desktop_agent, docs, semantic_fs, sheets) + user-defined JSON |
| LLM brain | ✅ | Native tool calling: Ollama, Anthropic, OpenAI, Gemini via `LlmProvider` trait |
| Self-improvement loop | ✅ | `meta.propose` → HITL → `ScriptCapability` → hot-load |
| Desktop agent mode | ✅ | `AgentModeStarted`/`Ended`, `DesktopAction`, `SpawnApp`, `ActivityUpdate` |
| Workflow observer | ✅ | `DesktopObserver` records events, annotates workflows, persists to JSON |
| Private mode | ✅ | Observation pauses, `[pvt]` indicator, IPC notification |
| Conversation context | ✅ | 5-exchange memory per client session |

### Infrastructure
| Component | Status | Notes |
|---|---|---|
| IPC protocol | ✅ | Newline-delimited JSON over Unix socket, transport-agnostic |
| DRM/KMS backend | ✅ | Bare-metal rendering, dumb buffer, page flip |
| Winit backend | ✅ | macOS/Linux dev mode |
| Buildroot image | ✅ | x86_64 + ARM64, auto-start systemd services |
| CI/CD | ✅ | GitHub Actions builds image on push to main |

---

## What's Missing — Agent-Native OS Primitives

These four primitives are what separate "Linux + AI chat" from an OS genuinely built for agents:

### 1. Session Model
**Current**: Agent has 5-exchange conversation memory. No concept of task scope, session history, or capability boundaries per session.

**Needed**: When the agent starts a task, the OS should know:
- What it's trying to accomplish (intent)
- What it has touched (affected files, windows, capabilities used)
- What it has permission to affect this session (scoped capabilities)
- Session history that persists beyond the 5-exchange window

**Status**: Partially implemented in v1.1 — `Session` + `SessionStep` structs in `ipc.rs` track capability/action history and affected paths per agent mode session, persisted to `~/.soma/sessions/<id>.json`. Scope boundaries (capability whitelist, directory whitelist) and parallel contexts deferred to v1.2.

### 2. Typed Failure ✅ DONE (v1.0.1)
~~**Current**: Capability errors are `{ success: false, error: Some("string") }`. Agent can't reason about recovery.~~

**Implemented**: `CapabilityError { reason: ErrorReason, context, alternatives }` in soma-common. All 11 capability modules migrated. Agent can programmatically decide: retry, escalate via HITL, or try an alternative path.

### 3. Semantic File System Layer ✅ DONE (v1.1)
~~**Where it fits**: v1.2 (after AgentAPI proves the structured-state pattern).~~

**Implemented**: `semantic_fs` capability with `tag`, `annotate`, `find_by_intent`, `list_tagged`, `describe_file`, `get_history`. Metadata stored as `.soma-meta` sidecar files (portable). Persistence strategy (sidecars vs. central index) still open — current impl uses sidecars but a queryable index may be needed at scale.

### 4. Parallel Task Contexts
**Current**: Agent handles one task per client connection. Single thread of attention.

**Needed**: Multiple concurrent agent sessions with:
- Isolated capability scopes per session
- Human can see and interrupt any session from the dock/sidebar
- Sessions can share data through the semantic FS layer
- HITL queue aggregates approvals from all active sessions

**Where it fits**: v1.3+ (after single-session is solid). The IPC protocol already supports multiple client connections — extend with session IDs and scope tokens.

---

## Architecture Risks

| Risk | Severity | Mitigation |
|---|---|---|
| ~~`main.rs` complexity (1,342 lines)~~ | ~~**High**~~ | ✅ Resolved v1.0.1 — extracted to compositor.rs + event_handler.rs; main.rs is 712 lines |
| DynamicApp widget tree growing into a UI framework | Medium | Keep minimal: status surfaces for agent, not apps for humans |
| AgentAPI `describe_state` design | **High** | Prototype with soma-sheets first; the answer shapes all future apps |
| Concurrency: human + agent editing same data model | **High** | Design conflict resolution in v1.1 (cell-level locking? last-write-wins? operational transform?) |
| Agent reliability on complex multi-step tasks | Medium | Native tool calling helps; session model will help more |
| ~~No automated tests~~ | ~~Medium~~ | ✅ Resolved v1.1 — 61-scenario integration test suite (soma-cli --test), 100% pass rate |

---

## Codebase Health

| Metric | Value | Assessment |
|---|---|---|
| Total workspace crates | 4 | Good separation |
| soma-compositor/src/main.rs | 712 lines | ✅ Extracted in v1.0.1 (was 1,342) |
| soma-compositor modules | 16 files | compositor.rs, event_handler.rs, settings_app.rs, config_loader.rs, sheets.rs, docs.rs added |
| soma-agent capabilities | 13 modules (incl. docs, semantic_fs, sheets) | Healthy — typed errors on all modules |
| IPC reconnection | ✅ | Compositor detects `tx.is_closed()`, retries every 5s |
| IPC message variants | 13 compositor→agent, 13 agent→compositor | Clean protocol |
| Feature-gated backends | 2 (winit, drm) | Good architecture |
| Test coverage | 0% | ⚠️ No tests at all |

---

## Milestone Sequencing

```
v1.0    ━━━━━━━━━━━━━━━━━━━  DONE   Desktop shell (shared environment)
v1.0.1  ━━━━━━               DONE   main.rs split + typed failure
v1.0.2  ━━━━━━               DONE   Bug fix pass (dock, UTF-8, IPC reconnect, shared config loader)
v1.1    ━━━━━━━━━━━━━━━━━━━  DONE   AgentAPI + dual-interface apps (sheets, docs) + semantic_fs
v1.2    ━━━━━━━━━━━━━━        NEXT   Capability governance + advanced sessions + soma-media
v1.3    ━━━━━━━━━━━━                  soma-media + parallel tasks
v1.4    ━━━━━━━━━━                    Federation
v2.0    ━━━━━━━━━━━━━━━               USB + plugin API
```

---

## Key Design Questions (Open)

1. ~~**What should `describe_state` return for a spreadsheet?**~~ ✅ Answered — `AppState { summary, cells }`: compact summary for orchestrator-level decisions + full cell map for worker reads.
2. **How do human edits and agent writes coexist on the same data model?** v1.1 uses last-write-wins. Is that sufficient, or do we need OT/CRDT for parallel agents?
3. **What does an agent session scope look like?** JSON config? Capability whitelist? Directory whitelist?
4. **Should semantic FS metadata scale from sidecars to a central index?** Current impl uses `.soma-meta` sidecars (portable). A queryable `~/.soma/index.db` may be needed once file counts grow.
5. **When should the agent auto-recover from typed failure vs. escalate to HITL?** Low-risk retries auto; anything touching user data escalates.

# SomaOS — Architecture Analysis

> **Living document.** Updated across agent sessions for cross-agent transparency. Any AI agent working on SomaOS should read this first.

*Last updated: 2026-03-22 — post v1.2 completion (Capability Governance + Session Scope + Semantic FS SQLite + soma-media, 70/70 tests)*

---

## Core Thesis

Both the human and the AI are **first-class co-inhabitants of the same desktop**. Not master-servant. Not tool-user. Peer operators with different interface modalities into one shared environment.

- **Human interface**: pixels — dock clicks, window drags, keyboard shortcuts
- **Agent interface**: structured APIs — `AgentAPI`, IPC messages, capability actions
- **Shared**: same apps, same windows, same data models
- **HITL**: conflict resolution for a shared space, not a permission gate

---

## What Exists (v1.2)

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
| Capability registry | ✅ | 14 modules (filesystem, process, system, network, package, browser, vision, meta, script, desktop_agent, docs, semantic_fs, sheets, media) + user-defined JSON |
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

### 1. Session Model ✅ DONE (v1.2, scoped)
**Implemented**: `SessionScope { capability_whitelist, path_whitelist }` in soma-common. `AgentModeStarted` carries `scope: Option<SessionScope>`. Scope enforced in `ipc.rs` before each step execution (capability + path whitelist). `GetSessionStatus` / `SessionStatusResponse` IPC round-trip. Sidebar Chat tab shows active session card.

**Remaining**: Enforcement is advisory (agent-side). Server-side compositor enforcement and parallel task contexts (multiple concurrent sessions) deferred to v1.3.

### 2. Typed Failure ✅ DONE (v1.0.1)
~~**Current**: Capability errors are `{ success: false, error: Some("string") }`. Agent can't reason about recovery.~~

**Implemented**: `CapabilityError { reason: ErrorReason, context, alternatives }` in soma-common. All 11 capability modules migrated. Agent can programmatically decide: retry, escalate via HITL, or try an alternative path.

### 3. Semantic File System Layer ✅ DONE (v1.2 — SQLite)
**Implemented**: `semantic_fs` capability with `tag`, `annotate`, `find_by_intent`, `list_tagged`, `describe_file`, `get_history`. v1.1 used `.soma-meta` sidecar files. v1.2 migrated to `rusqlite` (bundled) with `~/.soma/index.db`; best-effort sidecar migration on first run. Persistence question resolved — central index wins for queryability.

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
| ~~AgentAPI `describe_state` design~~ | ~~**High**~~ | ✅ Resolved v1.1 — SheetsApp + DocsApp + MediaApp prove the pattern |
| Concurrency: human + agent editing same data model | **Medium** | v1.1 last-write-wins; v1.2 adds session scope; OT/CRDT deferred to v1.3 |
| Session scope enforcement is advisory (client-side) | **Medium** | Agent enforces scope; a compromised agent could bypass; compositor-side enforcement needed in v1.3 |
| Agent reliability on complex multi-step tasks | Medium | Native tool calling helps; session model will help more |
| ~~No automated tests~~ | ~~Medium~~ | ✅ Resolved v1.2 — 70-scenario integration test suite (soma-cli --test), 100% pass rate |

---

## Codebase Health

| Metric | Value | Assessment |
|---|---|---|
| Total workspace crates | 4 | Good separation |
| soma-compositor/src/main.rs | 712 lines | ✅ Extracted in v1.0.1 (was 1,342) |
| soma-compositor modules | 18 files | media.rs added in v1.2; compositor.rs, event_handler.rs, settings_app.rs, config_loader.rs, sheets.rs, docs.rs added in prior versions |
| soma-agent capabilities | 14 modules (incl. docs, semantic_fs, sheets, media) | Healthy — typed errors on all modules |
| IPC reconnection | ✅ | Compositor detects `tx.is_closed()`, retries every 5s |
| IPC message variants | ~15 compositor→agent, ~15 agent→compositor | SessionScope + GetSessionStatus/SessionStatusResponse added in v1.2 |
| Feature-gated backends | 2 (winit, drm) | Good architecture |
| Test coverage | 70 integration scenarios | ✅ 70/70 passing (soma-cli --test) |

---

## Milestone Sequencing

```
v1.0    ━━━━━━━━━━━━━━━━━━━  DONE   Desktop shell (shared environment)
v1.0.1  ━━━━━━               DONE   main.rs split + typed failure
v1.0.2  ━━━━━━               DONE   Bug fix pass (dock, UTF-8, IPC reconnect, shared config loader)
v1.1    ━━━━━━━━━━━━━━━━━━━  DONE   AgentAPI + dual-interface apps (sheets, docs) + semantic_fs
v1.2    ━━━━━━━━━━━━━━━━━━━  DONE   Capability governance + session scope + semantic FS SQLite + soma-media
v1.3    ━━━━━━━━━━━━          NEXT   soma-media backend (diffusion) + parallel task contexts
v1.4    ━━━━━━━━━━                    Federation
v2.0    ━━━━━━━━━━━━━━━               USB + plugin API
```

---

## Key Design Questions (Open)

1. ~~**What should `describe_state` return for a spreadsheet?**~~ ✅ Answered — `AppState { summary, cells }`: compact summary for orchestrator-level decisions + full cell map for worker reads.
2. **How do human edits and agent writes coexist on the same data model?** v1.2 adds session scope enforcement; last-write-wins still applies within a session. OT/CRDT deferred to v1.3.
3. ~~**What does an agent session scope look like?**~~ ✅ Answered — `SessionScope { capability_whitelist, path_whitelist }` in soma-common; enforced in `ipc.rs` before each step. Remaining: compositor-side enforcement and parallel contexts (v1.3).
4. ~~**Should semantic FS metadata scale from sidecars to a central index?**~~ ✅ Answered — SQLite at `~/.soma/index.db` (queryable, sidecar migration on first run).
5. **When should the agent auto-recover from typed failure vs. escalate to HITL?** Low-risk retries auto; anything touching user data escalates.

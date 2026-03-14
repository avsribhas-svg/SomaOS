# SomaOS — Architecture Analysis

> **Living document.** Updated across agent sessions for cross-agent transparency. Any AI agent working on SomaOS should read this first.

*Last updated: 2026-03-12 — post v1.0 desktop environment completion*

---

## Core Thesis

Both the human and the AI are **first-class co-inhabitants of the same desktop**. Not master-servant. Not tool-user. Peer operators with different interface modalities into one shared environment.

- **Human interface**: pixels — dock clicks, window drags, keyboard shortcuts
- **Agent interface**: structured APIs — `AgentAPI`, IPC messages, capability actions
- **Shared**: same apps, same windows, same data models
- **HITL**: conflict resolution for a shared space, not a permission gate

---

## What Exists (v1.0)

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

**Where it fits**: v1.1 (alongside AgentAPI). Agent sessions become first-class OS objects. The `AgentModeStarted { task }` message already carries intent — extend it with scope and tracking.

### 2. Typed Failure
**Current**: Capability errors are `{ success: false, error: Some("string") }`. Agent can't reason about recovery.

**Needed**: Structured error reasons:
```rust
pub struct CapabilityError {
    pub reason: ErrorReason,      // PermissionDenied, NotFound, InvalidParams, etc.
    pub context: String,          // "file is outside session scope"
    pub alternatives: Vec<String>, // ["try /tmp/output.txt", "request elevated scope"]
}
```
Agent can now programmatically decide: retry with different params, escalate via HITL, or try an alternative path.

**Where it fits**: v1.0.1 (quick win, no architectural change). Update `CapabilityResult` in soma-common, update each capability's error returns.

### 3. Semantic File System Layer
**Current**: Files are bytes with names. Agent navigates by path.

**Needed**: Lightweight metadata sidecars (`.soma-meta`) or an indexed store:
- What created this file (agent task, human edit, capability action)
- What workflows have touched it
- Agent-generated description / tags
- Last context it was relevant in

Agent can then navigate by intent ("the spreadsheet I was working on yesterday") rather than path.

**Where it fits**: v1.2 (after AgentAPI proves the structured-state pattern). Build as a capability + OS service, not as a modified filesystem.

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
| ~~`main.rs` complexity (1,342 lines)~~ | ~~**High**~~ | ✅ Resolved v1.0.1 — extracted to compositor.rs + event_handler.rs; main.rs is ~712 lines |
| DynamicApp widget tree growing into a UI framework | Medium | Keep minimal: status surfaces for agent, not apps for humans |
| AgentAPI `describe_state` design | **High** | Prototype with soma-sheets first; the answer shapes all future apps |
| Concurrency: human + agent editing same data model | **High** | Design conflict resolution in v1.1 (cell-level locking? last-write-wins? operational transform?) |
| Agent reliability on complex multi-step tasks | Medium | Native tool calling (v0.9.5) helps; session model will help more |
| No automated tests | Medium | Add at least capability unit tests before v1.2 |

---

## Codebase Health

| Metric | Value | Assessment |
|---|---|---|
| Total workspace crates | 4 | Good separation |
| soma-compositor/src/main.rs | ~712 lines | ✅ Extracted (v1.0.1) |
| soma-agent capabilities | 13 modules | Healthy |
| IPC message variants | 13 compositor→agent, 13 agent→compositor | Clean protocol |
| Feature-gated backends | 2 (winit, drm) | Good architecture |
| Test coverage | 0% | ⚠️ No tests at all |

---

## Milestone Sequencing

```
v1.0    ━━━━━━━━━━━━━━━━━━━  DONE   Desktop shell (shared environment)
v1.0.1  ━━━━━━               NEXT   main.rs split + typed failure
v1.1    ━━━━━━━━━━━━━━━━━━━          AgentAPI + soma-sheets + session model
v1.2    ━━━━━━━━━━━━━━                soma-docs + governance + semantic FS
v1.3    ━━━━━━━━━━━━                  soma-media + parallel tasks
v1.4    ━━━━━━━━━━                    Federation
v2.0    ━━━━━━━━━━━━━━━               USB + plugin API
```

---

## Key Design Questions (Open)

1. **What should `describe_state` return for a spreadsheet?** This answer defines the AgentAPI contract for all future apps.
2. **How do human edits and agent writes coexist on the same data model?** Cell-level locking? Last-write-wins? OT/CRDT?
3. **What does an agent session scope look like?** JSON config? Capability whitelist? Directory whitelist?
4. **Should semantic FS metadata live as sidecars (.soma-meta) or in a central index (~/.soma/index.db)?** Sidecars are portable; index is queryable.
5. **When should the agent auto-recover from typed failure vs. escalate to HITL?** Low-risk retries auto; anything touching user data escalates.

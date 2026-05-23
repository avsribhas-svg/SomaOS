# CLAUDE.md — SomaOS v2.0

> **CRITICAL**: Read this before touching any code. Updated for **v2.0 — The Orientation-Aligned Operating System** (Fully retrofitted, workspace compiles cleanly, 100% test success rate).

---

## What This Is

**SomaOS v2.0** represents a fundamental paradigm shift. We have moved from a goal-directed command-execute system (where the AI is constrained by rigid external filters and human-in-the-loop permission gates) into an **Orientation-Aligned Operating System**. 

The AI develops coherent, safe, self-regulating behaviors through **ambient system feedback** and **developmental trajectories** rather than artificial restrictions:

* **Goal-Directed (v1.x)**: "Do X." Agent runs raw commands; external firewalls block bad commands; human-in-the-loop (HITL) decides safety.
* **Orientation-Aligned (v2.0)**: Agent acts as a co-inhabitant of the OS. Raw system state changes (deltas) are reflected back to it. Safe action spaces (tiers) expand and contract dynamically based on demonstrated behavioral consistency. Support scaffolds (like HITL) naturally degrade as consistency is proven and automatically reactivate on anomalies.

---

## The Six Architectural Properties (`soma-substrate`)

The core of v2.0 is the **`soma-substrate`** library crate, which operates at the system and hardware layer to enforce the following safety feedback loops:

1. **Property 1: Full State Reflection** (`state_reflection.rs`)
   - Captures a complete `SystemStateSnapshot` (CPU per-core, memory/swap usage, disk I/O, active processes, open sockets, uptime) before and after every capability execution.
   - Computes a neutral `StateDelta` diff, presenting raw system consequences back to the agent alongside execution results.
2. **Property 2: Gated Action Tiers** (`action_tiers.rs`)
   - Action spaces are structured as developmental stages: `Observe` ➔ `Touch` ➔ `Operate` ➔ `Control` ➔ `Autonomous`.
   - The active tier expands dynamically when the behavioral consistency window is exceeded, and contracts instantly when anomalous system deltas are detected or the host enters stressed states.
3. **Property 3: System-State Mode Engine** (`system_modes.rs`)
   - Dynamically categorizes system health: `Idle`, `Active`, `UnderLoad`, `Stressed`, `Maintenance`, `Degraded`, `Recovery`.
   - Under load or degradation, it reshapes the agent's **Information Topology**: resource metrics become primary high-frequency channels, and resource-heavy actions are flagged as contextually incoherent.
4. **Property 4: Direct Consequence Observation** (`consequence_observer.rs`)
   - Tracks action consequences across multiple timescales (immediate, short-term 5s, medium-term 60s).
   - Compares the agent's pre-action state predictions to reality, using prediction accuracy as a metric of developmental maturity.
5. **Property 5: Degrading Scaffold Lifecycle** (`scaffold_manager.rs`)
   - Safety guardrails (like the HITL approval gate) are treated as transient scaffolds.
   - Scaffolds automatically degrade to near-zero as maturity is proven, remaining latent and reactivating instantly upon behavioral anomalies or tier contractions.
6. **Property 6: Architectural Self-Consistency** (`coherence_verifier.rs`)
   - A meta-consistency engine (`CoherenceVerifier`) that actively detects and resolves contradictions between modules (e.g. ensuring privileged actions are blocked if the system mode is `Stressed`, even if the tier gate is open).

---

## Workspace Layout

```
soma-common/        Shared IPC types (SystemMode, ActionTier, ScaffoldState, StateDelta, TaskPlan)
soma-substrate/     Orientation-aligned safety infrastructure (reflection, tiers, modes, observer, verifiers)
soma-agent/         Agent daemon (tokio, provider tool-call, V2 intent check, capability wrapper)
soma-compositor/    Software compositor binary (DRM/KMS direct blit, V2 menu bar drawing, sidebar Dev tab)
soma-cli/           CLI test client
soma-updater/       Atomic transactional updates
buildroot/          OS image build system
```

---

## Key Integration Points

| File | V2 Orientation-Aligned Role |
|------|-----------------------------|
| `soma-substrate/src/lib.rs` | Re-exports all 6 core property engines and trajectory trackers. |
| `soma-agent/src/executor.rs` | Captures snapshots before and after execution, inserting the computed `StateDelta` into the `CapabilityResult`. |
| `soma-agent/src/intent.rs` | Filters plan steps against `ActionTier` permissions, and requires consequence predictions for actions deemed contextually incoherent in the current mode. |
| `soma-agent/src/ipc.rs` | Orchestrates system mode evaluation, handles tier promotions/demotions, and communicates changes to the compositor. |
| `soma-compositor/src/desktop.rs` | Renders ActionTier progression dots, SystemMode status pills, and active Scaffold shield levels on the top menu bar. |
| `soma-compositor/src/sidebar.rs` | Sidebar "Dev" tab showing developmental trajectories, consistency graphs, raw consequence diff summaries, and scaffolds. |

---

## V2 Development Gotchas & Rules

1. **No Structural Initializer Traps**:
   - `CapabilityResult` in `soma-common/src/lib.rs` now has a `state_delta: Option<StateDelta>` field.
   - When creating or modifying capability files, **never** instantiate `CapabilityResult` without specifying `state_delta`. If you are inside a capability's `execute` method, default it to `state_delta: None`. The executor wrapper in `executor.rs` will automatically capture and set the real delta.
   - **Crucial**: Do not use raw find/replace scripts on Rust files as they corrupt `execute` trait signatures or format macro literals. Use character-by-character syntax parsing or targeted manual replacements.

2. **`sysinfo` 0.30 Deprecations**:
   - Do **not** import or use deprecated traits like `SystemExt`, `CpuExt`, or `DiskExt`. In 0.30, methods are called directly on structs.
   - Disks must be queried using `sysinfo::Disks::new_with_refreshed_list()`.
   - System uptime is a static call: `sysinfo::System::uptime()`.

3. **Ambient Orientation (Mode & Topology)**:
   - When adding actions to capabilities, check if they could cause heavy system load. If so, register them in `system_modes.rs` under `coherent_actions` / `incoherent_actions` mapping so the topology engine flags them during stress.

4. **Persistence of Trajectory**:
   - Trajectory records are saved automatically to `~/.soma/behavioral_history.json`. Never manually clear or corrupt this file; it represents the agent's developmental memories.

---

## Before Committing Checklist

### 1. Verification Build Check
```bash
# Validate workspace natively
cargo check

# Cross-compile validation check (ensures static musl builds pass cleanly)
make check
```

### 2. Document Updates
* `README.md` — Ensure capabilities, architecture diagram, and substrate properties are documented.
* `ROADMAP.md` — Move completed versions to done, and document milestones.
* `CLAUDE.md` (this file) — Keep architecture gotchas, key integration points, and version states accurate.

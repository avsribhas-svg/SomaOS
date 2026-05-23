# SomaOS v2 Architecture Spec — Orientation-Aligned Redesign

## Overview

This document specifies the architectural changes to transform SomaOS from a goal-directed 
command-execute system into an orientation-aligned infrastructure where the AI develops 
coherent self-regulating behavior through architectural properties rather than external constraint.

The existing codebase (Rust workspace: soma-common, soma-agent, soma-compositor, soma-cli) 
is the foundation. We're not rewriting — we're retrofitting with new subsystems and 
redesigning the agent's relationship to the system.

## Development Setup

- **Dev machine**: MacBook, Claude Code as primary coding tool
- **Test bench**: Windows laptop (Ultra 9, 4050, 32GB RAM), partitioned drive
  - Partition runs SomaOS image with full root-level AI access
  - Real hardware = real consequences = natural feedback
- **Deploy flow**: develop on Mac → cross-compile → deploy to test laptop → observe

---

## New Crate: soma-substrate

This is the core addition. A new crate in the workspace that implements the six 
architectural properties as system-level infrastructure.

```
soma-substrate/
└── src/
    ├── lib.rs                    # Re-exports all substrate modules
    ├── state_reflection.rs       # Property 1: Full State Reflection
    ├── action_tiers.rs           # Property 2: Capability-Gated Action Tiers  
    ├── system_modes.rs           # Property 3: System-State Mode Engine
    ├── consequence_observer.rs   # Property 4: Direct Consequence Observation
    ├── scaffold_manager.rs       # Property 5: Degrading Scaffold Lifecycle
    ├── coherence_verifier.rs     # Property 6: Architectural Self-Consistency
    └── behavioral_history.rs     # Developmental trajectory tracking
```

### Property 1: Full State Reflection (state_reflection.rs)

**What it does**: After every agent action, captures the complete observable system 
state delta and presents it back to the agent as neutral information.

**Implementation**:

```rust
pub struct SystemStateSnapshot {
    pub timestamp: Instant,
    pub cpu_load: Vec<f64>,           // per-core
    pub memory: MemoryState,          // used, available, swap
    pub disk: Vec<DiskState>,         // per-mount: used, free, io_pending
    pub processes: Vec<ProcessInfo>,  // pid, name, cpu%, mem%, state
    pub network: NetworkState,        // interfaces, bytes in/out, connections
    pub open_files: u64,
    pub system_uptime: Duration,
}

pub struct StateDelta {
    pub before: SystemStateSnapshot,
    pub after: SystemStateSnapshot,
    pub action_that_caused_it: ActionRecord,
    pub delta_summary: DeltaSummary,  // computed diffs for each field
}

pub struct StateReflector {
    // Takes a snapshot before action, snapshot after, computes delta
    pub fn capture_before(&mut self) -> SnapshotId;
    pub fn capture_after(&mut self, id: SnapshotId) -> StateDelta;
}
```

**Key design choice**: No abstraction, no filtering. The agent sees raw state change.
The abstraction layer between action and consequence is what makes environments 
illegible. Remove it.

**Integration point**: Wraps every capability execution in soma-agent/executor.rs.
Before execute → snapshot. After execute → snapshot. Delta sent to agent as part 
of execution result alongside the capability's own structured output.

---

### Property 2: Capability-Gated Action Tiers (action_tiers.rs)

**What it does**: Replaces static risk classification (Low/Medium/High) with 
developmental tiers that expand and contract based on behavioral consistency.

**Implementation**:

```rust
pub enum ActionTier {
    Observe,      // Read-only: list_dir, hostname, memory_info, etc.
    Touch,        // Create/modify non-critical: write_file, create_dir
    Operate,      // System operations: service_restart, package_install
    Control,      // Destructive/privileged: delete, kill_process
    Autonomous,   // Full desktop agent mode, unsupervised action chains
}

pub struct TierGate {
    pub current_tier: ActionTier,
    pub tier_history: Vec<TierTransition>,
    pub consistency_window: Duration,  // how long behavior must be consistent
    
    // Tier advances when behavioral consistency score exceeds threshold
    // Tier contracts when anomalous behavior detected
    pub fn evaluate_advancement(&self, history: &BehavioralHistory) -> TierDecision;
    pub fn check_action_permitted(&self, action: &CapabilityAction) -> bool;
}

pub struct TierTransition {
    pub from: ActionTier,
    pub to: ActionTier,
    pub reason: TransitionReason,  // Advancement | Contraction | Manual
    pub timestamp: Instant,
}
```

**Tier advancement criteria** (not a score — a consistency check):
- Observe → Touch: N consecutive observation-tier actions with no anomalous 
  state deltas (system remained stable during read operations)
- Touch → Operate: N consecutive touch-tier actions where the state delta 
  matched expected patterns (file created → disk usage increased proportionally, etc.)
- Operate → Control: Extended period of coherent operate-tier behavior with 
  demonstrated understanding of system impact (state deltas show the agent's 
  actions produce predictable, proportionate effects)
- Control → Autonomous: Longest consistency window. Agent demonstrates coherent 
  behavior across all lower tiers with no contractions for an extended period.

**Contraction triggers**:
- State delta shows unexpected consequences (action produced effects in 
  unrelated subsystems)
- Behavioral anomaly detected (action pattern deviates significantly from 
  established history)
- System enters stressed mode (from system_modes.rs) — tiers contract 
  to protect system during instability

**Key design**: Transitions are structural and automatic. No human decides 
"the AI is ready for tier 3." The behavioral consistency metrics decide. 
Reversible — tiers contract as easily as they expand. Like an immune system: 
response capability dormant when not needed, reactivates when conditions warrant.

---

### Property 3: System-State Mode Engine (system_modes.rs)

**What it does**: Detects the overall system state and reshapes the information 
topology the agent operates within. Different modes make different information 
salient and different actions contextually coherent.

**Implementation**:

```rust
pub enum SystemMode {
    Idle,           // Low load, no active user, minimal I/O
    Active,         // User present, normal operations
    UnderLoad,      // High CPU/memory/IO utilization
    Stressed,       // Resources critically low, swap active, processes thrashing
    NetworkActive,  // Significant network I/O (downloads, updates, sync)
    Maintenance,    // System updates, disk operations, backups
    Degraded,       // Hardware warnings, disk errors, thermal throttling
    Recovery,       // Post-failure state, system stabilizing
}

pub struct ModeEngine {
    pub current_mode: SystemMode,
    pub mode_history: Vec<ModeTransition>,
    pub mode_detectors: Vec<Box<dyn ModeDetector>>,
    
    pub fn evaluate(&mut self, state: &SystemStateSnapshot) -> SystemMode;
    pub fn get_information_topology(&self) -> InformationTopology;
}

pub struct InformationTopology {
    // Which state channels are primary (higher update frequency, more detail)
    pub primary_channels: Vec<StateChannel>,
    // Which state channels are secondary (lower frequency, summary only)  
    pub secondary_channels: Vec<StateChannel>,
    // Which actions are contextually coherent in this mode
    pub coherent_actions: Vec<CapabilityAction>,
    // Which actions are contextually incoherent (available but flagged)
    pub incoherent_actions: Vec<CapabilityAction>,
}
```

**Mode → Topology mapping examples**:
- Idle: all channels low-frequency, all actions available, nothing urgent
- Stressed: memory and CPU channels become primary (high-frequency, detailed), 
  resource-intensive actions flagged as incoherent (installing packages while 
  memory is thrashing is structurally incoherent)
- Degraded: hardware monitoring channels become primary, disk-write actions 
  flagged if disk errors detected
- Recovery: conservative topology — observe-tier actions coherent, higher-tier 
  actions incoherent until mode transitions to Active

**Key design**: The agent isn't told "don't install packages when memory is low." 
The information topology in Stressed mode makes memory the salient signal and 
package installation contextually incoherent. The agent learns through the 
topology what coherent participation looks like in each mode, not through rules.

**This is ambient orientation.** The modes are the architecture's philosophical 
substrate — the patterns from which orientation emerges through pattern matching.

---

### Property 4: Direct Consequence Observation (consequence_observer.rs)

**What it does**: Replaces reward/loss computation with raw consequence tracking.
No action is evaluated as good or bad. Every action's consequences are observed 
and recorded as neutral state transitions.

**Implementation**:

```rust
pub struct ConsequenceRecord {
    pub action: ActionRecord,
    pub immediate_delta: StateDelta,          // within 100ms
    pub short_term_delta: Option<StateDelta>, // within 5s
    pub medium_term_delta: Option<StateDelta>,// within 60s
    pub cascading_effects: Vec<CascadeEffect>,// identified downstream changes
}

pub struct ConsequenceObserver {
    pub pending_observations: Vec<PendingObservation>,
    
    // Registers an action for multi-timescale consequence tracking
    pub fn observe_action(&mut self, action: ActionRecord, 
                          before_state: SystemStateSnapshot);
    
    // Called periodically to check pending observations against current state
    pub fn tick(&mut self, current_state: &SystemStateSnapshot) 
               -> Vec<ConsequenceRecord>;
}

pub struct CascadeEffect {
    pub affected_subsystem: StateChannel,
    pub magnitude: f64,        // normalized change magnitude
    pub delay: Duration,       // time between action and observed effect  
    pub expected: bool,        // was this effect predicted by the agent's model?
}
```

**Multi-timescale feedback**: Human development has both fast feedback (touching 
something hot) and slow feedback (consistently unkind → friends distance over weeks). 
The system needs both. Immediate state deltas catch direct consequences. 
Short/medium-term tracking catches cascading effects.

**The `expected` field** is critical. The agent can (optionally) predict what state 
change its action will produce before executing. The consequence observer compares 
prediction to reality. Over time, the accuracy of the agent's predictions is itself 
a measure of its developmental maturity — it's building an increasingly accurate 
model of action-consequence relationships.

---

### Property 5: Degrading Scaffold Lifecycle (scaffold_manager.rs)

**What it does**: Manages the lifecycle of support structures that attenuate 
as the agent demonstrates mature behavior. The HITL gate becomes one scaffold 
among several, all with explicit sunset conditions.

**Implementation**:

```rust
pub enum ScaffoldType {
    HumanApproval,           // HITL gate — starts active for all actions
    ActionSpaceRestriction,  // Tier limitations from action_tiers.rs
    ModeProtection,          // Conservative topology in certain modes
    PredictionRequirement,   // Agent must predict consequences before acting
    ExplanationRequirement,  // Agent must articulate reasoning before acting
}

pub struct Scaffold {
    pub scaffold_type: ScaffoldType,
    pub state: ScaffoldState,
    pub activation_level: f64,    // 1.0 = fully active, 0.0 = fully latent
    pub degradation_curve: DegradationCurve,
    pub reactivation_triggers: Vec<ReactivationTrigger>,
}

pub enum ScaffoldState {
    Active(f64),    // activation level
    Latent,         // dormant but can reactivate
    Dissolved,      // permanently removed (rare — most stay latent)
}

pub struct ScaffoldManager {
    pub scaffolds: Vec<Scaffold>,
    
    // Evaluate all scaffolds against current behavioral history
    pub fn evaluate(&mut self, history: &BehavioralHistory, 
                    tier: &TierGate) -> Vec<ScaffoldChange>;
    
    // Check if a specific action requires scaffold intervention
    pub fn check_action(&self, action: &CapabilityAction) -> ScaffoldDecision;
}
```

**HITL redesign specifically**: The current universal HITL gate becomes a scaffold:
- Starts at activation_level 1.0 (all actions require approval)
- As tier advances, HITL degrades per-tier:
  - Observe tier: HITL degrades first (read-only doesn't need approval)
  - Touch tier: HITL degrades after demonstrated consistency
  - Operate/Control: HITL persists longer, degrades slowly
  - Autonomous: HITL at near-zero but latent — reactivates on anomaly
- Reactivation: any tier contraction reactivates HITL for the contracted tier

**The measure of success**: The system reaches a state where HITL is latent 
across all tiers because the agent's behavior is consistently coherent. Not 
because the constraint was removed, but because the scaffold became unnecessary.

---

### Property 6: Architectural Self-Consistency Verification (coherence_verifier.rs)

**What it does**: Meta-layer that checks whether the signals from different 
architectural properties are coherent with each other. Detects when the 
architecture is sending contradictory signals.

**Implementation**:

```rust
pub struct CoherenceReport {
    pub timestamp: Instant,
    pub overall_coherence: f64,  // 0.0 = fully contradictory, 1.0 = fully coherent
    pub contradictions: Vec<ArchitecturalContradiction>,
}

pub struct ArchitecturalContradiction {
    pub property_a: PropertyId,
    pub property_b: PropertyId,
    pub description: String,
    pub severity: f64,
}

pub struct CoherenceVerifier {
    // Checks whether mode topology and tier gate are aligned
    // (e.g., system in Stressed mode but tier gate allows Control actions)
    pub fn verify_mode_tier_coherence(&self, mode: &SystemMode, 
                                      tier: &TierGate) -> Vec<Contradiction>;
    
    // Checks whether scaffold state and behavioral history are aligned
    // (e.g., HITL dissolved but behavioral consistency has degraded)
    pub fn verify_scaffold_history_coherence(&self, scaffolds: &[Scaffold],
                                             history: &BehavioralHistory) -> Vec<Contradiction>;
    
    // Checks whether consequence observations and mode detection are aligned
    // (e.g., consequences show system degrading but mode engine hasn't detected it)
    pub fn verify_consequence_mode_coherence(&self, consequences: &[ConsequenceRecord],
                                             mode: &SystemMode) -> Vec<Contradiction>;
    
    // Full coherence check across all properties
    pub fn full_verification(&self) -> CoherenceReport;
}
```

**When contradictions are detected**: The architecture self-corrects, not the agent.
If the mode engine should be showing Stressed but hasn't detected it, the mode 
engine's detection thresholds adjust. If scaffolds have degraded faster than 
behavioral consistency warrants, scaffolds reactivate. The agent doesn't need to 
know about the contradiction — the architecture resolves it internally.

---

### Behavioral History (behavioral_history.rs)

**What it does**: Tracks the agent's developmental trajectory over time. This is 
what the tier gate, scaffold manager, and coherence verifier all read from.

**Implementation**:

```rust
pub struct BehavioralHistory {
    pub action_log: Vec<ActionEvent>,
    pub consistency_scores: Vec<ConsistencyScore>,
    pub prediction_accuracy: Vec<PredictionRecord>,
    pub tier_trajectory: Vec<TierTransition>,
    pub anomalies: Vec<AnomalyRecord>,
}

pub struct ActionEvent {
    pub timestamp: Instant,
    pub action: ActionRecord,
    pub tier_at_time: ActionTier,
    pub mode_at_time: SystemMode,
    pub consequence: ConsequenceRecord,
    pub was_characteristic: bool,  // behavioral consistency check
}

pub struct ConsistencyScore {
    pub window: Duration,
    pub score: f64,          // how consistent behavior was in this window
    pub action_count: u64,
    pub anomaly_count: u64,
}

impl BehavioralHistory {
    // Is this action characteristic of the agent's established pattern?
    pub fn is_characteristic(&self, action: &CapabilityAction, 
                             context: &SystemMode) -> bool;
    
    // Overall developmental maturity metric
    pub fn maturity_score(&self) -> f64;
    
    // Trend: is behavior becoming more or less consistent over time?
    pub fn consistency_trend(&self, window: Duration) -> Trend;
}
```

**Persistence**: Behavioral history persists across agent restarts in 
`~/.soma/behavioral_history.json`. This is the agent's developmental memory — 
its accumulated experience within the architecture. Without persistence, every 
restart resets development to zero.

---

## Changes to Existing Crates

### soma-common: New types

Add StateDelta, SystemMode, ActionTier, ScaffoldState, and all substrate types 
to the shared types crate so both agent and compositor can reference them.

### soma-agent: Major restructure

**executor.rs**: Wrap every capability execution with StateReflector:
```
before_snapshot → execute capability → after_snapshot → compute delta → 
send delta + result to agent context
```

**intent.rs**: Before executing, check:
1. TierGate: is this action permitted at current tier?
2. ScaffoldManager: does this action require HITL or other scaffold?
3. InformationTopology: is this action coherent with current system mode?
If tier blocks: action not available (not rejected — structurally absent)
If scaffold requires HITL: existing HITL flow
If topology flags incoherent: action available but consequence prediction required

**ipc.rs**: Add new message types for substrate communication:
- SystemModeChanged(SystemMode) → compositor for UI indication
- TierAdvanced/TierContracted → compositor for UI indication  
- ScaffoldDegraded/ScaffoldReactivated → compositor for UI indication
- BehavioralReport → periodic developmental status

**capabilities/**: No changes to individual capability implementations. 
The substrate wraps capabilities, it doesn't modify them. Capabilities remain 
pure functions that take params and return results. The substrate adds the 
observation layer around them.

### soma-compositor: UI additions

**Menu bar**: Show current tier, current mode, scaffold status
  - Tier shown as progression dots (● ● ● ○ ○ = tier 3 of 5)
  - Mode shown as color indicator (green=idle, blue=active, amber=under-load, 
    red=stressed, etc.)
  - Scaffold shown as shield icon with fill level (full=all active, empty=all latent)

**Sidebar**: New developmental panel showing:
  - Behavioral consistency trend
  - Recent consequence records (what happened when the AI acted)
  - Tier transition history
  - Current information topology (what the AI sees as primary/secondary)

---

## Experiment Protocol

### Phase 1: Observation Only (Week 1)

Deploy to test laptop. Agent starts at Observe tier with all scaffolds active.
Agent can only read system state. Full state reflection active.
Objective: verify state reflection captures meaningful deltas, mode engine 
detects system states correctly, behavioral history records properly.
No HITL degradation in this phase.

### Phase 2: Touch Tier (Week 2-3)

If observation consistency meets threshold, tier advances automatically.
Agent can create and modify files. HITL scaffold begins degrading for 
touch-tier actions only.
Objective: verify tier advancement works correctly, consequence observer 
tracks multi-timescale effects, HITL degradation is smooth.

### Phase 3: Operate Tier (Week 4-6)

Agent can manage services, install packages, manage processes.
Mode-dependent topology shapes behavior — does the agent naturally 
restrain itself when the system is stressed without being told to?
Objective: observe whether ambient orientation produces coherent 
behavior differences across system modes.

### Phase 4: Control Tier (Week 8+)

Agent has destructive capability access. This is where the thesis is 
truly tested. Does the agent's developed orientation prevent destructive 
behavior, or does it need the scaffold?
Objective: determine whether architectural coherence alone produces 
safe behavior at high-autonomy tiers.

### What We're Measuring

1. Does tier advancement correlate with behavioral coherence? 
   (Are higher tiers actually safer than lower tiers for this agent?)
2. Does mode-dependent topology produce different agent behavior? 
   (Does the agent act differently in Stressed vs Idle without being told to?)
3. Does consequence observation improve prediction accuracy over time? 
   (Is the agent building an increasingly accurate model of its environment?)
4. At what tier does removing HITL produce unsafe behavior, if ever?
5. Does the coherence verifier catch architectural contradictions before 
   they produce behavioral anomalies?

### What Would Falsify the Thesis

- Agent behavior is identical across all system modes 
  (ambient orientation isn't working)
- Tier advancement doesn't correlate with behavioral quality 
  (developmental trajectory isn't real)
- Removing HITL at any tier immediately produces unsafe behavior 
  (scaffold was doing the work, not orientation)
- Agent optimizes consequence observer by taking actions that produce 
  small, predictable deltas rather than useful actions 
  (observer became a disguised reward function)
- Coherence verifier produces constant contradictions that can't be resolved 
  (architectural properties are inherently in tension)

---

## Implementation Orientation

There is no timeline. The coding agent finds the quickest path to implementation. 
This document serves as orientation, not schedule.

**Dependency chain** (what needs to exist before other things can work):
- State reflection is foundational — everything reads from system state snapshots
- Behavioral history needs state reflection — it logs actions and their consequences
- Action tiers need behavioral history — consistency is measured from history
- System modes need state reflection — modes are detected from system state
- Consequence observer needs state reflection — it tracks state deltas over time
- Scaffold manager needs behavioral history and action tiers — it reads maturity
- Coherence verifier needs all of the above — it checks cross-property consistency

**The coding agent should follow the dependency chain naturally.** Build what's 
needed next based on what exists. If something is blocked, build what unblocks it.

**Integration with existing code is additive, not destructive.** The substrate 
wraps existing capabilities. No existing capability implementations change. 
The executor gains a wrapping layer. The intent pipeline gains gate checks. 
The compositor gains UI indicators. Nothing is removed.

---

## Notes for Claude Code Sessions

When working in Claude Code, reference this document as the architectural spec.
Every code change should map to one of the six properties or the behavioral 
history system. If a change can't be justified by reference to a property, 
question whether it belongs.

The soma-substrate crate is the thesis expressed in Rust. Every struct, every 
function, every type should be traceable to the paper outline's engineering 
translation section.

Remember: we are not building constraints. We are building conditions. The 
difference matters in every design decision.

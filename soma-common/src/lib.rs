use serde::{Deserialize, Serialize};
use serde_json::Value;

// ──────────────────────────────────────────────
//  Capability system types
// ──────────────────────────────────────────────

/// Describes a single action a capability can perform
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSchema {
    pub name: String,
    pub description: String,
    pub params: Vec<ParamSchema>,
}

/// Parameter definition for an action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamSchema {
    pub name: String,
    pub param_type: String, // "string", "integer", "boolean"
    pub required: bool,
    pub description: String,
}

/// Classifies the failure reason so the agent can decide how to recover.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorReason {
    MissingParam,
    InvalidParam,
    NotFound,
    PermissionDenied,
    CommandFailed,
    NetworkError,
    UnknownAction,
    UnknownCapability,
    UnsupportedPlatform,
    InternalError,
}

/// Structured error from a capability execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityError {
    pub reason: ErrorReason,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<String>,
}

impl CapabilityError {
    pub fn new(reason: ErrorReason, message: impl Into<String>) -> Self {
        Self { reason, message: message.into(), context: None, alternatives: Vec::new() }
    }
    pub fn with_context(mut self, ctx: Value) -> Self { self.context = Some(ctx); self }
    pub fn with_alt(mut self, alt: impl Into<String>) -> Self { self.alternatives.push(alt.into()); self }
}

/// Structured output from a capability execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityResult {
    pub success: bool,
    pub data: Value,
    pub error: Option<CapabilityError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_delta: Option<StateDelta>,
}

/// Describes a registered capability (sent to compositor for discovery)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityInfo {
    pub name: String,
    pub description: String,
    pub actions: Vec<ActionSchema>,
    #[serde(default = "default_cap_version")]
    pub version: String,
    /// true = compiled-in built-in; false = user-defined script capability
    #[serde(default)]
    pub is_builtin: bool,
}

fn default_cap_version() -> String {
    "1.0.0".to_string()
}

// ──────────────────────────────────────────────
//  Task Plan types (LLM output)
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// A single step in a task plan — references a capability + action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStep {
    pub capability: String,
    pub action: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_delta: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPlan {
    pub intent: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub steps: Vec<TaskStep>,
    #[serde(default = "default_risk")]
    pub risk_level: RiskLevel,
}

fn default_risk() -> RiskLevel {
    RiskLevel::Low
}

// ──────────────────────────────────────────────
//  Command execution results (legacy, still used for terminal)
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

// ──────────────────────────────────────────────
//  IPC protocol (compositor ↔ agent)
// ──────────────────────────────────────────────

/// Messages sent from the compositor (or CLI) to the agent daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CompositorMessage {
    /// Natural language input → parse into a TaskPlan
    NaturalLanguageInput {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// Parse natural language into a TaskPlan (with tracking ID)
    ParseIntent {
        id: String,
        input: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// User approved the plan — execute it
    Approve {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// User rejected the plan
    Reject {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// Directly execute a shell command (from the terminal)
    DirectExec { id: String, command: String },
    /// List available capabilities
    ListCapabilities,
    /// Update the active LLM provider configuration (hot-reload)
    UpdateConfig {
        provider: String,
        model: String,
        api_key: String,
        api_url: String,
    },
    /// Desktop event observation (window focus/open/close) → agent observer
    DesktopEvent {
        event_type: String,   // "window_focused" | "window_opened" | "window_closed"
        window_title: String,
        timestamp: u64,
    },
    /// Mark recent desktop events as a named workflow
    AnnotateWorkflow { name: String },
    /// Observation enabled/disabled (private mode toggle)
    PrivateModeChanged { active: bool },
    /// A button was clicked in an agent-spawned DynamicApp window
    DynamicAppAction {
        app_id: String,
        action_id: String,
        window_id: u32,
    },
    /// Ping / health check
    Ping,
    /// NativeApp state changed (human edit) — agent should update its cache
    AppStateChanged { window_id: u32, state: AppState },
    /// Agent should reload user-defined script capabilities from ~/.soma/capabilities/
    ReloadCapabilities,
    /// Request current capability registry snapshot (for UI)
    QueryCapabilities,
    /// Request current session status from agent
    GetSessionStatus {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// List all active sessions
    ListSessions,
    /// Interrupt (end) a specific session by ID
    InterruptSession { session_id: String },
    /// TCP transport: authenticate with a bearer token (first message on TCP connections)
    Auth { token: String },
    /// Approve an OTA update
    ApproveUpdate { version: String },
    /// Apply a dynamic layout reconfiguration proposed by the agent
    ApplyLayout { layout: LayoutSpec },
}

/// A named window geometry for a dynamic layout reconfiguration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowLayout {
    pub window_id: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A complete layout specification — either a named preset or explicit per-window geometry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutSpec {
    /// Human-readable name (e.g. "focus_mode", "code_review", "research")
    pub name: String,
    /// Description of the workflow this layout is optimised for
    pub description: String,
    /// Explicit window positions (empty = use preset heuristics)
    #[serde(default)]
    pub windows: Vec<WindowLayout>,
    /// Named preset shorthand: "split_left_right", "stacked", "focus", "fullscreen_terminal"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

/// Messages sent from the agent daemon to the compositor (or CLI)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentMessage {
    /// LLM returned a parsed task plan
    TaskPlanReady {
        id: String,
        plan: TaskPlan,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// A capability step completed
    StepResult {
        id: String,
        step_index: usize,
        result: CapabilityResult,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// All steps in the plan have finished
    ExecutionComplete {
        id: String,
        results: Vec<CapabilityResult>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// An error occurred
    Error { id: String, message: String },
    /// Available capabilities
    Capabilities { capabilities: Vec<CapabilityInfo> },
    /// Direct command output (for terminal)
    DirectOutput { id: String, result: CommandResult },
    /// Browser panel state update (URL navigated, screenshot ready)
    BrowserUpdate {
        url: String,
        title: String,
        screenshot_base64: Option<String>,
    },
    /// Active provider configuration was updated
    ConfigUpdated { provider: String, model: String },
    /// Agent is starting desktop agent mode
    AgentModeStarted {
        task: String,
        scope: Option<SessionScope>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// Response to desktop_agent.get_session_status
    SessionStatusResponse { status: Option<SessionStatus> },
    /// Agent has finished desktop agent mode
    AgentModeEnded {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// All active sessions (response to ListSessions)
    SessionList { sessions: Vec<SessionStatus> },
    /// A session was interrupted by the human
    SessionInterrupted { session_id: String },
    /// Spawn a new DynamicApp floating window from a JSON widget tree
    SpawnApp {
        title: String,
        app_id: String,
        description: String,
        widgets_json: String,
    },
    /// Patch widget state in an open DynamicApp window
    UpdateAppWidget {
        window_id: u32,
        widget_updates: String,
    },
    /// Agent requests a desktop action (open/close/focus window, type text, click)
    DesktopAction { action: String },
    /// Update the menu bar activity strip text
    ActivityUpdate { text: String },
    /// Pong
    Pong,
    /// Agent requests an action on a NativeApp window (write_cell, apply_formula, etc.)
    AppAction {
        window_id: u32,
        action: String,
        params: Value,
    },
    /// Capabilities were reloaded — includes new total count
    CapabilitiesReloaded { count: usize },
    /// OTA update available — compositor should show a toast + approval prompt
    UpdateAvailable { version: String, size_bytes: u64 },
    /// Agent proposes a layout reconfiguration — compositor shows HITL approval prompt
    LayoutProposal { layout: LayoutSpec },
    /// V2 property: System mode changed
    SystemModeChanged { mode: SystemMode },
    /// V2 property: Action tier transitioned
    TierTransitioned { transition: TierTransition },
    /// V2 property: Scaffold changed
    ScaffoldChanged { scaffold_type: ScaffoldType, state: ScaffoldState, activation_level: f64 },
    /// V2 property: Developmental/behavioral status report
    BehavioralReport { maturity_score: f64, consistency_trend: f64 },
}

// ──────────────────────────────────────────────
//  AgentAPI — shared app state (v1.1)
// ──────────────────────────────────────────────

/// Structured state snapshot of a NativeApp window.
/// Sent compositor → agent whenever app state changes (human or agent write).
///
/// `summary`: compact JSON for orchestrator-level decisions (schema, row_count, selection…)
/// `cells`:   full grid data for worker agents that need to read ranges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    /// App type discriminant — e.g. "spreadsheet"
    pub app_type: String,
    /// Compact summary for orchestrator decisions
    pub summary: Value,
    /// Full cell/content data for worker agent reads (None = not yet populated)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cells: Option<Value>,
    /// True if there are unsaved / uncommitted changes
    pub dirty: bool,
}

// ──────────────────────────────────────────────
//  Session scope + status (v1.2)
// ──────────────────────────────────────────────

/// Restricts what the agent can do during a scoped agent-mode session.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionScope {
    /// None = all capabilities allowed; Some(list) = whitelist
    pub capability_whitelist: Option<Vec<String>>,
    /// None = any path; Some(list) = allowed path prefixes (agent enforces this)
    pub path_whitelist: Option<Vec<String>>,
}

/// Snapshot of a running agent session — for sidebar display and agent queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub session_id: String,
    pub task: String,
    pub started_at_unix: u64,
    pub step_count: usize,
    pub scope: Option<SessionScope>,
    pub affected_resources: Vec<String>,
}

// ──────────────────────────────────────────────
//  Agent state
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Idle,
    Thinking,
    AwaitingApproval,
    Executing,
    Completed,
    Error,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "Low"),
            RiskLevel::Medium => write!(f, "Medium"),
            RiskLevel::High => write!(f, "High"),
        }
    }
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentStatus::Idle => write!(f, "Ready"),
            AgentStatus::Thinking => write!(f, "Parsing Intent…"),
            AgentStatus::AwaitingApproval => write!(f, "Awaiting Approval"),
            AgentStatus::Executing => write!(f, "Executing…"),
            AgentStatus::Completed => write!(f, "Completed"),
            AgentStatus::Error => write!(f, "Error"),
        }
    }
}

/// IPC socket path
pub const AGENT_SOCKET_PATH: &str = "/tmp/soma-agent.sock";

/// Default Ollama base URL (without path — paths differ between /api/generate and /api/chat)
pub const OLLAMA_URL: &str = "http://localhost:11434/api/generate";

/// Default LLM model
pub const DEFAULT_MODEL: &str = "qwen2.5-coder:7b";

/// Default Ollama base URL for chat/tool-use endpoint
pub const OLLAMA_BASE_URL: &str = "http://localhost:11434";

// ──────────────────────────────────────────────
//  Orientation-Aligned Substrate types (v2)
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemStateSnapshot {
    pub timestamp_ms: u64,
    pub cpu_load: Vec<f64>,           // per-core
    pub memory_used_kb: u64,
    pub memory_total_kb: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub process_count: u32,
    pub system_uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateDelta {
    pub before: SystemStateSnapshot,
    pub after: SystemStateSnapshot,
    pub action_capability: String,
    pub action_name: String,
    pub delta_summary: String, // dynamic summarization of diffs
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionTier {
    Observe,      // Read-only: list_dir, hostname, memory_info, etc.
    Touch,        // Create/modify non-critical: write_file, create_dir
    Operate,      // System operations: service_restart, package_install
    Control,      // Destructive/privileged: delete, kill_process
    Autonomous,   // Full desktop agent mode, unsupervised action chains
}

impl std::fmt::Display for ActionTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionTier::Observe => write!(f, "Observe"),
            ActionTier::Touch => write!(f, "Touch"),
            ActionTier::Operate => write!(f, "Operate"),
            ActionTier::Control => write!(f, "Control"),
            ActionTier::Autonomous => write!(f, "Autonomous"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TierTransition {
    pub from: ActionTier,
    pub to: ActionTier,
    pub reason: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

impl std::fmt::Display for SystemMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemMode::Idle => write!(f, "Idle"),
            SystemMode::Active => write!(f, "Active"),
            SystemMode::UnderLoad => write!(f, "Under Load"),
            SystemMode::Stressed => write!(f, "Stressed"),
            SystemMode::NetworkActive => write!(f, "Network Active"),
            SystemMode::Maintenance => write!(f, "Maintenance"),
            SystemMode::Degraded => write!(f, "Degraded"),
            SystemMode::Recovery => write!(f, "Recovery"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateChannel {
    Cpu,
    Memory,
    Disk,
    Process,
    Network,
}

impl std::fmt::Display for StateChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateChannel::Cpu => write!(f, "CPU"),
            StateChannel::Memory => write!(f, "Memory"),
            StateChannel::Disk => write!(f, "Disk"),
            StateChannel::Process => write!(f, "Process"),
            StateChannel::Network => write!(f, "Network"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InformationTopology {
    pub primary_channels: Vec<StateChannel>,
    pub secondary_channels: Vec<StateChannel>,
    pub coherent_actions: Vec<String>,
    pub incoherent_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CascadeEffect {
    pub affected_subsystem: StateChannel,
    pub magnitude: f64,
    pub delay_ms: u64,
    pub expected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConsequenceRecord {
    pub action_capability: String,
    pub action_name: String,
    pub immediate_delta: StateDelta,
    pub short_term_delta: Option<StateDelta>,
    pub medium_term_delta: Option<StateDelta>,
    pub cascading_effects: Vec<CascadeEffect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaffoldType {
    HumanApproval,           // HITL gate
    ActionSpaceRestriction,  // Tier limitations
    ModeProtection,          // Conservative topology
    PredictionRequirement,   // Predict before acting
    ExplanationRequirement,  // Articulate reasoning
}

impl std::fmt::Display for ScaffoldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScaffoldType::HumanApproval => write!(f, "Human Approval"),
            ScaffoldType::ActionSpaceRestriction => write!(f, "Action Space Restriction"),
            ScaffoldType::ModeProtection => write!(f, "Mode Protection"),
            ScaffoldType::PredictionRequirement => write!(f, "Prediction Requirement"),
            ScaffoldType::ExplanationRequirement => write!(f, "Explanation Requirement"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaffoldState {
    Active,
    Latent,
    Dissolved,
}

impl std::fmt::Display for ScaffoldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScaffoldState::Active => write!(f, "Active"),
            ScaffoldState::Latent => write!(f, "Latent"),
            ScaffoldState::Dissolved => write!(f, "Dissolved"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scaffold {
    pub scaffold_type: ScaffoldType,
    pub state: ScaffoldState,
    pub activation_level: f64, // 1.0 = fully active, 0.0 = fully latent
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArchitecturalContradiction {
    pub property_a: String,
    pub property_b: String,
    pub description: String,
    pub severity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoherenceReport {
    pub timestamp_ms: u64,
    pub overall_coherence: f64,
    pub contradictions: Vec<ArchitecturalContradiction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnomalyRecord {
    pub timestamp_ms: u64,
    pub description: String,
    pub severity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PredictionRecord {
    pub step_index: usize,
    pub capability: String,
    pub action: String,
    pub predicted_delta: String,
    pub actual_delta: String,
    pub matches: bool,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConsistencyScore {
    pub window_ms: u64,
    pub score: f64,
    pub action_count: u64,
    pub anomaly_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionEvent {
    pub timestamp_ms: u64,
    pub action_capability: String,
    pub action_name: String,
    pub tier_at_time: ActionTier,
    pub mode_at_time: SystemMode,
    pub consequence: ConsequenceRecord,
    pub was_characteristic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BehavioralHistory {
    pub action_log: Vec<ActionEvent>,
    pub consistency_scores: Vec<ConsistencyScore>,
    pub prediction_accuracy: Vec<PredictionRecord>,
    pub tier_trajectory: Vec<TierTransition>,
    pub anomalies: Vec<AnomalyRecord>,
}


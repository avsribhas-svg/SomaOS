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
    NaturalLanguageInput { text: String },
    /// Parse natural language into a TaskPlan (with tracking ID)
    ParseIntent { id: String, input: String },
    /// User approved the plan — execute it
    Approve { id: String },
    /// User rejected the plan
    Reject { id: String },
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
    GetSessionStatus,
}

/// Messages sent from the agent daemon to the compositor (or CLI)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentMessage {
    /// LLM returned a parsed task plan
    TaskPlanReady { id: String, plan: TaskPlan },
    /// A capability step completed
    StepResult {
        id: String,
        step_index: usize,
        result: CapabilityResult,
    },
    /// All steps in the plan have finished
    ExecutionComplete {
        id: String,
        results: Vec<CapabilityResult>,
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
    AgentModeStarted { task: String, scope: Option<SessionScope> },
    /// Response to desktop_agent.get_session_status
    SessionStatusResponse { status: Option<SessionStatus> },
    /// Agent has finished desktop agent mode
    AgentModeEnded,
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

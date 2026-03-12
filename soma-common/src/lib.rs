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

/// Structured output from a capability execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityResult {
    pub success: bool,
    pub data: Value,
    pub error: Option<String>,
}

/// Describes a registered capability (sent to compositor for discovery)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityInfo {
    pub name: String,
    pub description: String,
    pub actions: Vec<ActionSchema>,
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
    AgentModeStarted { task: String },
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

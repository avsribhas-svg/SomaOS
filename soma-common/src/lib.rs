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

/// Default Ollama URL
pub const OLLAMA_URL: &str = "http://localhost:11434/api/generate";

/// Default LLM model
pub const DEFAULT_MODEL: &str = "qwen2.5-coder:7b";

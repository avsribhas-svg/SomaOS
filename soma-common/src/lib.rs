use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStep {
    pub action: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPlan {
    pub intent: String,
    pub description: String,
    pub steps: Vec<TaskStep>,
    pub risk_level: RiskLevel,
}

// ──────────────────────────────────────────────
//  Command execution results
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

/// Messages sent from the compositor to the agent daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CompositorMessage {
    /// Parse natural language into a TaskPlan
    ParseIntent { id: String, input: String },
    /// User approved the plan — execute it
    Approve { id: String },
    /// User rejected the plan
    Reject { id: String },
    /// Directly execute a shell command (from the terminal)
    DirectExec { id: String, command: String },
    /// Read clipboard contents
    ReadClipboard { id: String },
    /// Ping / health check
    Ping,
}

/// Messages sent from the agent daemon to the compositor
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentMessage {
    /// LLM returned a parsed task plan
    TaskPlanReady { id: String, plan: TaskPlan },
    /// A task step completed
    StepResult {
        id: String,
        step_index: usize,
        result: CommandResult,
    },
    /// All steps in the plan have finished
    ExecutionComplete { id: String, results: Vec<CommandResult> },
    /// An error occurred
    Error { id: String, message: String },
    /// Clipboard content
    ClipboardContent { id: String, content: String },
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
pub const DEFAULT_MODEL: &str = "deepseek-r1:7b";

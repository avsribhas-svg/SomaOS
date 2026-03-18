//! LLM provider abstraction for SomaOS.
//!
//! Each provider implements `LlmProvider`, converting a user message + available
//! tools into a `Vec<ToolCall>` using the provider's native function-calling API.
//! The tool calls are then mapped to a `TaskPlan` by `tool_calls_to_task_plan`.

pub mod anthropic;
pub mod gemini;
pub mod ollama;
pub mod openai;

use serde_json::Value;
use soma_common::{RiskLevel, TaskPlan, TaskStep};

use crate::capabilities::CapabilityRegistry;
use crate::config::{Provider, SomaConfig};

// ─────────────────────────────────────────────────────────────────────────────
//  Shared system prompt
// ─────────────────────────────────────────────────────────────────────────────

pub const SYSTEM_PROMPT: &str = "\
You are SomaOS, an AI agent that directly controls a desktop computer on behalf of the user. \
You have access to a fixed set of tools. You MUST only call tools by their exact names as defined \
in the tools list — never invent new tool names.

CRITICAL RULES:
1. ALWAYS respond by calling one or more tools from the provided list. Never reply with plain text.
2. ONLY use tool names that exist in the tools list. Do NOT invent names like 'create_spreadsheet' \
   or 'tag_file' — use the exact names provided (e.g. sheets__create, semantic_fs__tag).
3. When the user asks to CREATE, GENERATE, or SHOW sample/example data: fabricate realistic, \
   plausible content yourself and pass it to the tool. Do not ask the user for data.
4. To create a new spreadsheet window: use sheets__create with a 'cells' object. \
   Example cells for a Q1 budget: \
   {\"A1\":\"Month\",\"B1\":\"Revenue\",\"C1\":\"Expenses\",\"D1\":\"Profit\",\
   \"A2\":\"January\",\"B2\":120000,\"C2\":85000,\"D2\":35000,\
   \"A3\":\"February\",\"B3\":134000,\"C3\":91000,\"D3\":43000,\
   \"A4\":\"March\",\"B4\":148000,\"C4\":96000,\"D4\":52000}
5. To create a new document window: use docs__create with a 'blocks' array. \
   Do NOT use docs__append_block for new documents — use docs__create first. \
   Example: [{\"type\":\"heading\",\"level\":1,\"text\":\"Q1 Report\"},\
   {\"type\":\"paragraph\",\"text\":\"Q1 saw strong revenue growth across all segments.\"}]
6. When asked for both a sheet and a doc: call BOTH sheets__create AND docs__create together.
7. 'Q1 report' means January, February, March data. Generate all three months.";

// ─────────────────────────────────────────────────────────────────────────────
//  Tool schema types (provider-agnostic)
// ─────────────────────────────────────────────────────────────────────────────

/// A single tool the LLM can call, derived from a capability action.
#[derive(Debug, Clone)]
pub struct ToolDef {
    /// `capability__action` — double-underscore namespace
    pub name: String,
    pub description: String,
    /// JSON Schema object for the parameters
    pub parameters: Value,
}

/// A tool call returned by the LLM.
#[derive(Debug, Clone)]
pub struct ToolCall {
    /// Matches `ToolDef::name` — `capability__action`
    pub name: String,
    /// The arguments the LLM chose
    pub arguments: Value,
}

// ─────────────────────────────────────────────────────────────────────────────
//  LlmProvider trait
// ─────────────────────────────────────────────────────────────────────────────

/// Uniform interface for calling any LLM with tool use.
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Call the LLM with the user message, conversation context, and available tools.
    /// Returns the list of tool calls the model chose to make.
    async fn tool_call(
        &self,
        user_message: &str,
        context: &[(String, String)],
        tools: &[ToolDef],
    ) -> Result<Vec<ToolCall>, String>;
}

// ─────────────────────────────────────────────────────────────────────────────
//  Factory: create the right provider from config
// ─────────────────────────────────────────────────────────────────────────────

pub fn make_provider(config: &SomaConfig) -> Box<dyn LlmProvider> {
    let m = &config.model;
    match config.provider() {
        Provider::Anthropic => Box::new(anthropic::AnthropicProvider::new(
            m.model.clone(),
            m.api_key.clone(),
            m.api_url.clone(),
        )),
        Provider::OpenAi | Provider::Custom => Box::new(openai::OpenAiProvider::new(
            m.model.clone(),
            m.api_key.clone(),
            m.api_url.clone(),
        )),
        Provider::Gemini => Box::new(gemini::GeminiProvider::new(
            m.model.clone(),
            m.api_key.clone(),
        )),
        Provider::Ollama => Box::new(ollama::OllamaProvider::new(
            m.model.clone(),
            m.api_url.clone(),
        )),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Capability → ToolDef conversion
// ─────────────────────────────────────────────────────────────────────────────

/// Convert all registered capabilities into a flat list of tool definitions.
pub fn build_tools(registry: &CapabilityRegistry) -> Vec<ToolDef> {
    let mut tools = Vec::new();
    for cap in registry.list() {
        for action in &cap.actions {
            let mut props = serde_json::Map::new();
            let mut required_params: Vec<Value> = Vec::new();

            for p in &action.params {
                let json_type = match p.param_type.as_str() {
                    "integer" => "integer",
                    "boolean" => "boolean",
                    "array"   => "array",
                    _         => "string",
                };
                props.insert(p.name.clone(), serde_json::json!({
                    "type": json_type,
                    "description": p.description,
                }));
                if p.required {
                    required_params.push(Value::String(p.name.clone()));
                }
            }

            tools.push(ToolDef {
                name: format!("{}__{}", cap.name, action.name),
                description: format!("[{}] {}", cap.name, action.description),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": props,
                    "required": required_params,
                }),
            });
        }
    }
    tools
}

// ─────────────────────────────────────────────────────────────────────────────
//  ToolCall → TaskPlan conversion
// ─────────────────────────────────────────────────────────────────────────────

/// Map LLM tool calls back to a `TaskPlan` for execution.
pub fn tool_calls_to_task_plan(calls: Vec<ToolCall>, user_input: &str) -> TaskPlan {
    let mut steps = Vec::new();
    let mut overall_risk = RiskLevel::Low;

    for call in calls {
        // Split `capability__action` on double-underscore
        let (capability, action) = if let Some(idx) = call.name.find("__") {
            (call.name[..idx].to_string(), call.name[idx + 2..].to_string())
        } else {
            // Fallback: treat entire name as capability with unknown action
            (call.name.clone(), "unknown".to_string())
        };

        // Infer risk from capability name
        let risk = infer_risk(&capability, &action);
        if risk == RiskLevel::High {
            overall_risk = RiskLevel::High;
        } else if risk == RiskLevel::Medium && overall_risk == RiskLevel::Low {
            overall_risk = RiskLevel::Medium;
        }

        let description = format!("{}.{}", capability, action);
        steps.push(TaskStep {
            capability,
            action,
            params: call.arguments,
            description,
        });
    }

    TaskPlan {
        intent: derive_intent(user_input),
        description: user_input.to_string(),
        steps,
        risk_level: overall_risk,
    }
}

fn infer_risk(capability: &str, action: &str) -> RiskLevel {
    match capability {
        "filesystem" => match action {
            "delete" | "write_file" => RiskLevel::High,
            "copy" | "move_file" | "create_dir" => RiskLevel::Medium,
            _ => RiskLevel::Low,
        },
        "process" => match action {
            "kill_process" => RiskLevel::High,
            _ => RiskLevel::Low,
        },
        "package" => match action {
            "remove" => RiskLevel::High,
            "install" => RiskLevel::Medium,
            _ => RiskLevel::Low,
        },
        "system" => match action {
            "reboot" | "shutdown" => RiskLevel::High,
            _ => RiskLevel::Low,
        },
        _ => RiskLevel::Low,
    }
}

fn derive_intent(input: &str) -> String {
    // Take first 5 words, snake_case them as an intent label
    let words: Vec<&str> = input.split_whitespace().take(5).collect();
    words.join("_").to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == '_').collect()
}

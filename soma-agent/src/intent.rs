//! Intent parsing — converts natural language to a `TaskPlan` via native tool calling.
//!
//! The 3-layer pipeline (regex → free-text LLM → JSON plan) has been replaced with a
//! single tool-use call to the configured provider.  The model receives all capability
//! actions as proper function schemas and returns `tool_calls` directly — no fragile
//! JSON prompt engineering required.
//!
//! Layer 0 (deterministic keyword pre-processor) is retained for unambiguous one-word
//! system queries (disk, memory, uptime, etc.) where it avoids a full LLM round-trip.

use soma_common::TaskPlan;

use crate::capabilities::CapabilityRegistry;
use crate::config::SomaConfig;
use crate::providers::{self, LlmProvider, build_tools, tool_calls_to_task_plan};

// ─────────────────────────────────────────────────────────────────────────────
//  Layer 0: Deterministic keyword pre-processor
//  Fast path for unambiguous one-liners; returns None to fall through to LLM.
// ─────────────────────────────────────────────────────────────────────────────

fn preprocess_input(input: &str) -> Option<TaskPlan> {
    use soma_common::{RiskLevel, TaskStep, TaskPlan};

    let s = input.to_lowercase();

    macro_rules! sys_plan {
        ($intent:expr, $action:expr) => {
            Some(TaskPlan {
                intent: $intent.to_string(),
                description: input.to_string(),
                steps: vec![TaskStep {
                    capability: "system".to_string(),
                    action: $action.to_string(),
                    params: serde_json::Value::Object(Default::default()),
                    description: $intent.to_string(),
                }],
                risk_level: RiskLevel::Low,
            })
        };
    }

    // Disk space
    let is_real_image = s.contains(".img") || s.contains(".iso") || s.contains(".dmg") || s.contains(".vhd");
    if (s.contains("disk image") && !is_real_image)
        || s.contains("disk space") || s.contains("free space") || s.contains("storage space")
        || s.contains("disk full") || s.contains("how much room")
        || ((s.contains("how full") || s.contains("how much")) && (s.contains("disk") || s.contains("drive") || s.contains("storage")))
        || ((s.contains("disk") || s.contains("drive")) && s.contains("usage"))
    {
        return sys_plan!("check_disk_usage", "disk_usage");
    }

    // Memory
    let memory_kw = s.contains("ram") || s.contains("memory") || s.contains("mem ");
    let query_kw  = s.contains("usage") || s.contains("info") || s.contains("eating")
        || s.contains("how much") || s.contains("check") || s.contains("show")
        || s.contains("status") || s.contains("free");
    if memory_kw && query_kw {
        return sys_plan!("show_memory_usage", "memory_usage");
    }

    // Uptime
    if s.contains("uptime")
        || (s.contains("how long") && s.contains("running"))
        || s.contains("since boot") || s.contains("boot time")
    {
        return sys_plan!("show_uptime", "uptime");
    }

    // Hostname
    if s.contains("hostname") || s.contains("computer name") || s.contains("machine name")
        || s.contains("server name") || (s.contains("what") && s.contains("this machine"))
    {
        return sys_plan!("get_hostname", "hostname");
    }

    None // fall through to LLM
}

// ─────────────────────────────────────────────────────────────────────────────
//  IntentParser
// ─────────────────────────────────────────────────────────────────────────────

pub struct IntentParser {
    provider: Box<dyn LlmProvider>,
}

impl IntentParser {
    pub fn new(config: &SomaConfig) -> Self {
        Self { provider: providers::make_provider(config) }
    }

    /// Hot-reload the provider when the user updates settings.
    pub fn set_provider(&mut self, config: &SomaConfig) {
        self.provider = providers::make_provider(config);
        log::info!("Provider switched to {} / {}", config.model.provider, config.model.model);
    }

    /// Layer 0 fast path → LLM tool call.
    pub async fn parse(
        &self,
        input: &str,
        _system_prompt: &str, // kept for ipc.rs signature compatibility
        _context: Option<&str>,
        context_pairs: &[(String, String)],
        registry: &CapabilityRegistry,
    ) -> Result<TaskPlan, String> {
        // Layer 0 — instant, no LLM call
        if let Some(plan) = preprocess_input(input) {
            log::info!("Layer0 fast path: '{}'", input);
            return Ok(plan);
        }

        // Build tool schemas from registry
        let tools = build_tools(registry);

        // Single LLM tool-use call
        let calls = self.provider.tool_call(input, context_pairs, &tools).await?;
        log::info!("Tool calls returned: {:?}", calls.iter().map(|c| &c.name).collect::<Vec<_>>());

        Ok(tool_calls_to_task_plan(calls, input))
    }
}

use serde_json::Value;
use soma_common::{ActionSchema, CapabilityInfo, CapabilityResult, ParamSchema};
use std::collections::HashMap;

pub mod filesystem;
pub mod process;
pub mod system;

/// Trait that all capabilities must implement
pub trait Capability: Send + Sync {
    /// Unique name for this capability (e.g., "filesystem")
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// List of actions this capability supports
    fn actions(&self) -> Vec<ActionSchema>;

    /// Execute an action with the given parameters
    fn execute(&self, action: &str, params: &Value) -> CapabilityResult;
}

/// Registry of all available capabilities
pub struct CapabilityRegistry {
    capabilities: HashMap<String, Box<dyn Capability>>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            capabilities: HashMap::new(),
        };

        // Register all built-in capabilities
        registry.register(Box::new(filesystem::FileSystemCapability));
        registry.register(Box::new(process::ProcessCapability));
        registry.register(Box::new(system::SystemCapability));

        registry
    }

    fn register(&mut self, cap: Box<dyn Capability>) {
        self.capabilities.insert(cap.name().to_string(), cap);
    }

    /// Execute a capability action
    pub fn execute(&self, capability: &str, action: &str, params: &Value) -> CapabilityResult {
        match self.capabilities.get(capability) {
            Some(cap) => cap.execute(action, params),
            None => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(format!("Unknown capability: {}", capability)),
            },
        }
    }

    /// Get info about all registered capabilities (for LLM system prompt)
    pub fn list(&self) -> Vec<CapabilityInfo> {
        self.capabilities
            .values()
            .map(|cap| CapabilityInfo {
                name: cap.name().to_string(),
                description: cap.description().to_string(),
                actions: cap.actions(),
            })
            .collect()
    }

    /// Generate a capability schema string for the LLM system prompt
    pub fn schema_for_prompt(&self) -> String {
        let mut out = String::new();
        for cap in self.capabilities.values() {
            out.push_str(&format!("## {}: {}\n", cap.name(), cap.description()));
            for action in cap.actions() {
                out.push_str(&format!("  - {}: {}\n", action.name, action.description));
                for p in &action.params {
                    let req = if p.required { "required" } else { "optional" };
                    out.push_str(&format!(
                        "      param: {} ({}, {}) — {}\n",
                        p.name, p.param_type, req, p.description
                    ));
                }
            }
            out.push('\n');
        }
        out
    }
}

/// Helper to create a ParamSchema
pub fn param(name: &str, param_type: &str, required: bool, description: &str) -> ParamSchema {
    ParamSchema {
        name: name.to_string(),
        param_type: param_type.to_string(),
        required,
        description: description.to_string(),
    }
}

use serde_json::Value;
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityInfo, CapabilityResult, ParamSchema};
use std::collections::HashMap;

pub mod browser;
pub mod desktop_agent;
pub mod docs;
pub mod filesystem;
pub mod meta;
pub mod network;
pub mod package;
pub mod process;
pub mod script;
pub mod semantic_fs;
pub mod sheets;
pub mod system;
pub mod vision;

/// Hardcoded list of built-in capability names.
/// Any capability whose name is NOT in this list is considered a ScriptCapability.
const BUILTIN_NAMES: &[&str] = &[
    "filesystem",
    "process",
    "system",
    "network",
    "package",
    "browser",
    "vision",
    "meta",
    "desktop_agent",
    "sheets",
    "docs",
    "semantic_fs",
];

/// Trait that all capabilities must implement
pub trait Capability: Send + Sync {
    /// Unique name for this capability (e.g., "filesystem")
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// Semver version string — built-ins return "1.0.0" by default
    fn version(&self) -> &str { "1.0.0" }

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
        Self::new_with_cache(crate::ipc::AppStateCache::default())
    }

    pub fn new_with_cache(state_cache: crate::ipc::AppStateCache) -> Self {
        let mut registry = Self {
            capabilities: HashMap::new(),
        };

        // Register all built-in capabilities
        registry.register(Box::new(filesystem::FileSystemCapability));
        registry.register(Box::new(process::ProcessCapability));
        registry.register(Box::new(system::SystemCapability));
        registry.register(Box::new(network::NetworkCapability));
        registry.register(Box::new(package::PackageCapability));
        registry.register(Box::new(browser::BrowserCapability));
        registry.register(Box::new(vision::VisionCapability));
        registry.register(Box::new(meta::MetaCapability));
        registry.register(Box::new(desktop_agent::DesktopAgentCapability));
        registry.register(Box::new(sheets::SheetsCapability::new(state_cache.clone())));
        registry.register(Box::new(docs::DocsCapability::new(state_cache)));
        registry.register(Box::new(semantic_fs::SemanticFsCapability));

        // Load any user-proposed capabilities from ~/.soma/capabilities/
        load_script_capabilities(&mut registry.capabilities);

        registry
    }

    fn register(&mut self, cap: Box<dyn Capability>) {
        self.capabilities.insert(cap.name().to_string(), cap);
    }

    /// Rescan ~/.soma/capabilities/*.json and reload all ScriptCapabilities.
    /// Built-in capabilities are untouched.
    pub fn reload_scripts(&mut self) {
        // Remove all existing script capabilities (those not in the built-in list)
        self.capabilities.retain(|name, _| {
            BUILTIN_NAMES.contains(&name.as_str())
        });
        // Re-scan and load
        load_script_capabilities(&mut self.capabilities);
        log::info!(
            "Reloaded script capabilities — {} total registered",
            self.capabilities.len()
        );
    }

    /// Execute a capability action
    pub fn execute(&self, capability: &str, action: &str, params: &Value) -> CapabilityResult {
        match self.capabilities.get(capability) {
            Some(cap) => cap.execute(action, params),
            None => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownCapability, format!("Unknown capability: {}", capability))),
            },
        }
    }

    /// Get info about all registered capabilities (for LLM system prompt)
    pub fn list(&self) -> Vec<CapabilityInfo> {
        self.capabilities
            .values()
            .map(|cap| {
                let is_builtin = BUILTIN_NAMES.contains(&cap.name());
                CapabilityInfo {
                    name: cap.name().to_string(),
                    description: cap.description().to_string(),
                    actions: cap.actions(),
                    version: cap.version().to_string(),
                    is_builtin,
                }
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

/// Private helper: scan ~/.soma/capabilities/*.json and insert ScriptCapabilities
/// into the provided map. Called at startup and on hot-reload.
fn load_script_capabilities(caps: &mut HashMap<String, Box<dyn Capability>>) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let dir = std::path::PathBuf::from(home).join(".soma").join("capabilities");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|x| x == "json").unwrap_or(false) {
            match script::ScriptCapability::from_file(&path) {
                Ok(cap) => {
                    log::info!("Loaded user capability '{}' from {:?}", cap.name(), path);
                    caps.insert(cap.name().to_string(), Box::new(cap));
                }
                Err(e) => {
                    log::warn!("Skipping {:?}: {}", path, e);
                }
            }
        }
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

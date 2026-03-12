//! ScriptCapability — runtime capabilities defined by JSON + shell-command templates.
//!
//! Instead of compiling Rust, user-proposed capabilities are stored as JSON files in
//! `~/.soma/capabilities/<name>.json`. Each action has a `shell_template` field where
//! `{param_name}` placeholders are substituted at execution time.
//!
//! Example JSON definition:
//! ```json
//! {
//!   "name": "image_resize",
//!   "description": "Resize images using ImageMagick",
//!   "actions": [
//!     {
//!       "name": "resize",
//!       "description": "Resize an image to the given dimensions",
//!       "params": [
//!         { "name": "path",   "param_type": "string",  "required": true, "description": "Image path" },
//!         { "name": "width",  "param_type": "integer", "required": true, "description": "Width px"   },
//!         { "name": "height", "param_type": "integer", "required": true, "description": "Height px"  }
//!       ],
//!       "shell_template": "convert {path} -resize {width}x{height} {path}"
//!     }
//!   ]
//! }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use soma_common::{ActionSchema, CapabilityResult, ParamSchema};
use std::process::Command;

use super::Capability;

/// One action within a user-defined capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptAction {
    pub name: String,
    pub description: String,
    pub params: Vec<ParamSchema>,
    /// Shell command template; `{param_name}` is replaced with the param value at runtime.
    pub shell_template: String,
}

/// The full JSON definition of a user-proposed capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptCapabilityDef {
    pub name: String,
    pub description: String,
    pub actions: Vec<ScriptAction>,
}

/// A capability backed by shell-command templates, loaded from a JSON file.
pub struct ScriptCapability {
    def: ScriptCapabilityDef,
}

impl ScriptCapability {
    pub fn new(def: ScriptCapabilityDef) -> Self {
        Self { def }
    }

    /// Load a ScriptCapability from a JSON file on disk.
    pub fn from_file(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let def: ScriptCapabilityDef = serde_json::from_str(&content)?;
        Ok(Self::new(def))
    }
}

impl Capability for ScriptCapability {
    fn name(&self) -> &str {
        &self.def.name
    }

    fn description(&self) -> &str {
        &self.def.description
    }

    fn actions(&self) -> Vec<ActionSchema> {
        self.def
            .actions
            .iter()
            .map(|a| ActionSchema {
                name: a.name.clone(),
                description: a.description.clone(),
                params: a.params.clone(),
            })
            .collect()
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        let script_action = match self.def.actions.iter().find(|a| a.name == action) {
            Some(a) => a,
            None => {
                return CapabilityResult {
                    success: false,
                    data: Value::Null,
                    error: Some(format!("Unknown action '{}' in capability '{}'", action, self.def.name)),
                }
            }
        };

        // Substitute {param_name} placeholders in the shell template.
        let mut cmd = script_action.shell_template.clone();
        if let Some(obj) = params.as_object() {
            for (key, val) in obj {
                let placeholder = format!("{{{}}}", key);
                let value = match val {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                cmd = cmd.replace(&placeholder, &value);
            }
        }

        log::info!("ScriptCapability '{}': running: {}", self.def.name, cmd);

        match Command::new("sh").args(["-c", &cmd]).output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                if output.status.success() {
                    CapabilityResult {
                        success: true,
                        data: serde_json::json!({ "output": stdout.trim() }),
                        error: None,
                    }
                } else {
                    CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(if stderr.is_empty() { stdout } else { stderr }),
                    }
                }
            }
            Err(e) => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(format!("Failed to execute command: {}", e)),
            },
        }
    }
}

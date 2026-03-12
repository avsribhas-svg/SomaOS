//! MetaCapability — the agent proposes and manages new capabilities.
//!
//! This is the core of SomaOS's self-improvement loop. When the agent encounters a task
//! it cannot complete with existing capabilities, it can use `meta.propose` to generate
//! a new capability definition (name, description, shell-backed actions). The full definition
//! passes through the HITL gate before being saved — the human reviews and approves it like
//! any other plan step.
//!
//! Approved capabilities are stored as JSON in `~/.soma/capabilities/<name>.json` and loaded
//! automatically on the next agent startup. Over time the registry grows through use, and the
//! human's role shifts from writing capabilities to curating them.

use serde_json::Value;
use soma_common::{ActionSchema, CapabilityResult};

use super::{param, script::ScriptCapabilityDef, Capability};

pub struct MetaCapability;

impl Capability for MetaCapability {
    fn name(&self) -> &str {
        "meta"
    }

    fn description(&self) -> &str {
        "Propose and manage new shell-backed capabilities. Use this when you cannot complete a task with existing capabilities."
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "propose".to_string(),
                description: "Propose a new capability. Provide name, description, and a list of actions with shell_template commands. The proposal is saved to ~/.soma/capabilities/ after human approval.".to_string(),
                params: vec![
                    param("name",        "string", true,  "Capability identifier (snake_case, e.g. image_resize)"),
                    param("description", "string", true,  "What this capability does"),
                    param("actions",     "array",  true,  "Array of action objects: {name, description, params: [{name, param_type, required, description}], shell_template}"),
                ],
            },
            ActionSchema {
                name: "list_proposed".to_string(),
                description: "List all user-defined capabilities currently stored on disk in ~/.soma/capabilities/.".to_string(),
                params: vec![],
            },
            ActionSchema {
                name: "describe_gap".to_string(),
                description: "Record a capability gap — a description of something you wish you could do but cannot. Stored in ~/.soma/gaps.log for the human to review later.".to_string(),
                params: vec![
                    param("gap", "string", true, "Description of the missing capability"),
                ],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "propose" => execute_propose(params),
            "list_proposed" => execute_list_proposed(),
            "describe_gap" => execute_describe_gap(params),
            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(format!("Unknown meta action: {}", action)),
            },
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────

fn capabilities_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    std::path::PathBuf::from(home).join(".soma").join("capabilities")
}

/// Validate and save a proposed capability to disk.
fn execute_propose(params: &Value) -> CapabilityResult {
    let name = match params["name"].as_str() {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some("'name' is required".to_string()),
            }
        }
    };

    let description = match params["description"].as_str() {
        Some(d) if !d.is_empty() => d.to_string(),
        _ => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some("'description' is required".to_string()),
            }
        }
    };

    // Reconstruct ScriptCapabilityDef from the params Value so we can validate +
    // pretty-print a canonical JSON before saving.
    let actions_val = &params["actions"];
    if !actions_val.is_array() {
        return CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some("'actions' must be a JSON array".to_string()),
        };
    }

    // Re-serialise through ScriptCapabilityDef for schema validation.
    let def_value = serde_json::json!({
        "name": name,
        "description": description,
        "actions": actions_val,
    });

    let def: ScriptCapabilityDef = match serde_json::from_value(def_value) {
        Ok(d) => d,
        Err(e) => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(format!("Invalid capability definition: {}", e)),
            }
        }
    };

    let dir = capabilities_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(format!("Could not create capabilities directory: {}", e)),
        };
    }

    let path = dir.join(format!("{}.json", def.name));
    let json = match serde_json::to_string_pretty(&def) {
        Ok(j) => j,
        Err(e) => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(format!("Serialization error: {}", e)),
            }
        }
    };

    match std::fs::write(&path, &json) {
        Ok(_) => {
            log::info!("Proposed capability '{}' saved to {:?}", def.name, path);
            CapabilityResult {
                success: true,
                data: serde_json::json!({
                    "name": def.name,
                    "path": path.to_string_lossy(),
                    "actions": def.actions.len(),
                    "message": format!(
                        "Capability '{}' saved to {}. Restart the agent (`systemctl restart soma-agent`) to activate it.",
                        def.name,
                        path.display()
                    ),
                    "definition": json,
                }),
                error: None,
            }
        }
        Err(e) => CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(format!("Failed to save capability definition: {}", e)),
        },
    }
}

/// List all JSON-defined capabilities in ~/.soma/capabilities/.
fn execute_list_proposed() -> CapabilityResult {
    let dir = capabilities_dir();
    match std::fs::read_dir(&dir) {
        Ok(entries) => {
            let caps: Vec<serde_json::Value> = entries
                .flatten()
                .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
                .filter_map(|e| {
                    let path = e.path();
                    let content = std::fs::read_to_string(&path).ok()?;
                    let def: ScriptCapabilityDef = serde_json::from_str(&content).ok()?;
                    Some(serde_json::json!({
                        "name": def.name,
                        "description": def.description,
                        "actions": def.actions.iter().map(|a| &a.name).collect::<Vec<_>>(),
                        "path": path.to_string_lossy(),
                    }))
                })
                .collect();
            CapabilityResult {
                success: true,
                data: serde_json::json!({ "capabilities": caps }),
                error: None,
            }
        }
        Err(_) => CapabilityResult {
            success: true,
            data: serde_json::json!({ "capabilities": [] }),
            error: None,
        },
    }
}

/// Append a capability gap description to ~/.soma/gaps.log.
fn execute_describe_gap(params: &Value) -> CapabilityResult {
    let gap = match params["gap"].as_str() {
        Some(g) if !g.is_empty() => g.to_string(),
        _ => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some("'gap' is required".to_string()),
            }
        }
    };

    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let log_path = std::path::PathBuf::from(home).join(".soma").join("gaps.log");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = format!("[{}] {}\n", timestamp, gap);

    use std::io::Write;
    match std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        Ok(mut f) => {
            f.write_all(entry.as_bytes()).ok();
            CapabilityResult {
                success: true,
                data: serde_json::json!({
                    "message": format!("Gap recorded in {}", log_path.display()),
                    "gap": gap,
                }),
                error: None,
            }
        }
        Err(e) => CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(format!("Failed to write gap log: {}", e)),
        },
    }
}

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
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityResult};

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
            ActionSchema {
                name: "list_governance".to_string(),
                description: "List ALL registered capabilities (built-in and script) with their type, version, and action count.".to_string(),
                params: vec![],
            },
            ActionSchema {
                name: "promote".to_string(),
                description: "Promote a script capability to a Rust stub for built-in promotion. Reads ~/.soma/capabilities/<name>.json and generates a Rust stub at ~/.soma/promotions/<name>.rs.".to_string(),
                params: vec![
                    param("name", "string", true, "Name of the script capability to promote"),
                ],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "propose"         => execute_propose(params),
            "list_proposed"   => execute_list_proposed(),
            "describe_gap"    => execute_describe_gap(params),
            "list_governance" => execute_list_governance(),
            "promote"         => execute_promote(params),
            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownAction, format!("Unknown meta action: {}", action))),
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
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "'name' is required")),
            }
        }
    };

    let description = match params["description"].as_str() {
        Some(d) if !d.is_empty() => d.to_string(),
        _ => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "'description' is required")),
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
            error: Some(CapabilityError::new(ErrorReason::InvalidParam, "'actions' must be a JSON array")),
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
                error: Some(CapabilityError::new(ErrorReason::InvalidParam, format!("Invalid capability definition: {}", e))),
            }
        }
    };

    let dir = capabilities_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::InternalError, format!("Could not create capabilities directory: {}", e))),
        };
    }

    let path = dir.join(format!("{}.json", def.name));
    let json = match serde_json::to_string_pretty(&def) {
        Ok(j) => j,
        Err(e) => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::InternalError, format!("Serialization error: {}", e))),
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
            error: Some(CapabilityError::new(ErrorReason::InternalError, format!("Failed to save capability definition: {}", e))),
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

/// BUILTIN_NAMES list (mirrors mod.rs BUILTIN_NAMES for governance listing).
const BUILTIN_NAMES: &[&str] = &[
    "filesystem", "process", "system", "network", "package",
    "browser", "vision", "meta", "desktop_agent",
    "sheets", "docs", "semantic_fs", "media",
];

/// List all capabilities — built-ins from BUILTIN_NAMES + script caps from disk.
fn execute_list_governance() -> CapabilityResult {
    let mut capabilities: Vec<serde_json::Value> = BUILTIN_NAMES
        .iter()
        .map(|name| serde_json::json!({
            "name": name,
            "type": "builtin",
            "version": "1.0.0",
        }))
        .collect();

    // Scan ~/.soma/capabilities/ for script caps
    let dir = capabilities_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|x| x == "json").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(def) = serde_json::from_str::<ScriptCapabilityDef>(&content) {
                        capabilities.push(serde_json::json!({
                            "name": def.name,
                            "type": "script",
                            "version": "1.0.0",
                            "action_count": def.actions.len(),
                        }));
                    }
                }
            }
        }
    }

    CapabilityResult {
        success: true,
        data: serde_json::json!({ "capabilities": capabilities, "total": capabilities.len() }),
        error: None,
    }
}

/// Promote a script capability: load its JSON and generate a Rust stub.
fn execute_promote(params: &Value) -> CapabilityResult {
    let name = match params["name"].as_str() {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => return CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::MissingParam, "'name' is required")),
        },
    };

    let cap_path = capabilities_dir().join(format!("{}.json", name));
    let content = match std::fs::read_to_string(&cap_path) {
        Ok(c) => c,
        Err(_) => return CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::NotFound, format!("No script capability '{}' found", name))),
        },
    };
    let def: ScriptCapabilityDef = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(e) => return CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::InvalidParam, format!("Invalid JSON: {}", e))),
        },
    };

    // Generate Rust stub
    let struct_name = to_pascal_case(&name);
    let mut stub = format!(
        "//! Auto-generated stub for '{}' capability.\n//! Promote this to soma-agent/src/capabilities/{}.rs\n//! and fill in the execute() arms.\n\nuse serde_json::{{json, Value}};\nuse soma_common::{{ActionSchema, CapabilityError, CapabilityResult, ErrorReason}};\nuse super::{{param, Capability}};\n\npub struct {};\n\nimpl Capability for {} {{\n    fn name(&self) -> &str {{ \"{}\" }}\n    fn description(&self) -> &str {{ \"{}\" }}\n\n    fn actions(&self) -> Vec<ActionSchema> {{\n        vec![\n",
        name, name, struct_name, struct_name, name, def.description
    );
    for action in &def.actions {
        stub.push_str(&format!(
            "            // Shell template: {}\n            ActionSchema {{ name: \"{}\".to_string(), description: \"{}\".to_string(), params: vec![] }},\n",
            action.shell_template,
            action.name, action.description
        ));
    }
    stub.push_str("        ]\n    }\n\n    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {\n        match action {\n");
    for action in &def.actions {
        stub.push_str(&format!(
            "            \"{}\" => CapabilityResult {{ success: false, data: Value::Null, error: Some(CapabilityError::new(ErrorReason::InternalError, \"not implemented — fill in\")) }},\n",
            action.name
        ));
    }
    stub.push_str("            _ => CapabilityResult { success: false, data: Value::Null, error: Some(CapabilityError::new(ErrorReason::UnknownAction, format!(\"Unknown action: {}\", action))) },\n        }\n    }\n}\n");

    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let promotions_dir = std::path::PathBuf::from(home).join(".soma").join("promotions");
    if let Err(e) = std::fs::create_dir_all(&promotions_dir) {
        return CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::InternalError, format!("Could not create promotions dir: {}", e))),
        };
    }

    let out_path = promotions_dir.join(format!("{}.rs", name));
    match std::fs::write(&out_path, &stub) {
        Ok(_) => CapabilityResult {
            success: true,
            data: serde_json::json!({
                "path": out_path.to_string_lossy(),
                "instructions": format!("Copy {} to soma-agent/src/capabilities/{}.rs, implement execute() arms, register in mod.rs, and rebuild.", out_path.display(), name),
            }),
            error: None,
        },
        Err(e) => CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::InternalError, format!("Failed to write stub: {}", e))),
        },
    }
}

fn to_pascal_case(s: &str) -> String {
    s.split('_').map(|part| {
        let mut chars = part.chars();
        match chars.next() {
            None => String::new(),
            Some(c) => c.to_uppercase().to_string() + chars.as_str(),
        }
    }).collect()
}

/// Append a capability gap description to ~/.soma/gaps.log.
fn execute_describe_gap(params: &Value) -> CapabilityResult {
    let gap = match params["gap"].as_str() {
        Some(g) if !g.is_empty() => g.to_string(),
        _ => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "'gap' is required")),
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
            error: Some(CapabilityError::new(ErrorReason::InternalError, format!("Failed to write gap log: {}", e))),
        },
    }
}

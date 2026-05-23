//! Update capability — check and apply OTA updates via soma-updater.
//!
//! `check` → fetches the manifest and returns version info.
//! `apply` → HITL gate → calls soma-updater apply <version>.
//! `rollback` → calls soma-updater rollback (no HITL; restores .old binary).
//!
//! `apply` uses the command pattern so ipc.rs can inject the HITL gate.

use serde_json::{json, Value};
use soma_common::{ActionSchema, CapabilityError, CapabilityResult, ErrorReason};
use std::process::Command;

use super::{param, Capability};

pub struct UpdateCapability;

impl Capability for UpdateCapability {
    fn name(&self) -> &str { "update" }

    fn description(&self) -> &str {
        "Check for and apply OTA updates to SomaOS via the soma-updater binary"
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "check".into(),
                description: "Check for available updates. Returns version, channel, and size.".into(),
                params: vec![],
            },
            ActionSchema {
                name: "apply".into(),
                description: "Apply an available update. Requires human approval (HITL gate).".into(),
                params: vec![param("version", "string", true, "Version string to apply (e.g. '2.0.1')")],
            },
            ActionSchema {
                name: "rollback".into(),
                description: "Restore the previous binary from .old backup".into(),
                params: vec![],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult { state_delta: None,
        match action {
            "check" => {
                let output = Command::new("soma-updater").arg("check").output();
                match output {
                    Ok(o) => {
                        let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                        let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                        if o.status.success() {
                            // Parse "Available: v<version>" from output
                            let version = stdout.lines()
                                .find(|l| l.starts_with("Available:"))
                                .and_then(|l| l.split_whitespace().nth(1))
                                .map(|v| v.trim_start_matches('v').to_string())
                                .unwrap_or_default();

                            CapabilityResult {
                                success: true,
                                data: json!({
                                    "ipc_message": "UpdateAvailable",
                                    "version": version,
                                    "output": stdout,
                                }),
                                error: None,
                                state_delta: None,
                            }
                        } else {
                            CapabilityResult {
                                success: false,
                                data: Value::Null,
                                error: Some(CapabilityError::new(
                                    ErrorReason::CommandFailed,
                                    format!("soma-updater check failed: {}", stderr),
                                )),
                                state_delta: None,
                            }
                        }
                    }
                    Err(e) => CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(
                            ErrorReason::CommandFailed,
                            format!("Could not run soma-updater: {} — is it installed?", e),
                        )),
                        state_delta: None,
                    },
                }
            }

            "apply" => {
                let version = params["version"].as_str().unwrap_or("").to_string();
                if version.is_empty() {
                    return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::MissingParam, "version required")),
                        state_delta: None,
                    };
                }
                // Command pattern — ipc.rs handles HITL gate before forwarding.
                CapabilityResult {
                    success: true,
                    data: json!({
                        "ipc_message": "ApplyUpdate",
                        "version": version,
                    }),
                    error: None,
                    state_delta: None,
                }
            }

            "rollback" => {
                let output = Command::new("soma-updater").arg("rollback").output();
                match output {
                    Ok(o) if o.status.success() => CapabilityResult {
                        success: true,
                        data: json!({ "output": String::from_utf8_lossy(&o.stdout).to_string() }),
                        error: None,
                        state_delta: None,
                    },
                    Ok(o) => CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(
                            ErrorReason::CommandFailed,
                            String::from_utf8_lossy(&o.stderr).to_string(),
                        )),
                        state_delta: None,
                    },
                    Err(e) => CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::CommandFailed, e.to_string())),
                        state_delta: None,
                    },
                }
            }

            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(
                    ErrorReason::UnknownAction,
                    format!("Unknown update action: {}", action),
                )),
                state_delta: None,
            },
        }
    }
}

//! Desktop agent capability — allows the LLM to interact with the SomaOS
//! desktop environment (agent mode, dynamic apps, desktop actions, workflows).

use serde_json::{json, Value};
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityResult};

use super::{param, Capability};

pub struct DesktopAgentCapability;

impl Capability for DesktopAgentCapability {
    fn name(&self) -> &str {
        "desktop_agent"
    }

    fn description(&self) -> &str {
        "Interact with the SomaOS desktop: start/end agent mode, spawn dynamic apps, open/close/focus windows, and query workflow history"
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "start_agent_mode".into(),
                description: "Start desktop agent mode with a task description and optional scope".into(),
                params: vec![
                    param("task", "string", true, "Description of the agent task"),
                    param("scope", "object", false, "Optional scope: {capabilities: [string], paths: [string]}"),
                ],
            },
            ActionSchema {
                name: "end_agent_mode".into(),
                description: "End desktop agent mode".into(),
                params: vec![],
            },
            ActionSchema {
                name: "spawn_app".into(),
                description: "Spawn a dynamic app window from a JSON widget tree".into(),
                params: vec![
                    param("title", "string", true, "Window title"),
                    param("app_id", "string", true, "Unique app identifier"),
                    param("description", "string", true, "Short app description"),
                    param("widgets_json", "string", true, "JSON array of widget definitions"),
                ],
            },
            ActionSchema {
                name: "desktop_action".into(),
                description: "Perform a desktop action (open_window:terminal, close_window:title, focus_window:title)".into(),
                params: vec![param("action", "string", true, "Action string (e.g. 'open_window:terminal')")],
            },
            ActionSchema {
                name: "get_workflow_history".into(),
                description: "Get the recorded workflow history as JSON".into(),
                params: vec![],
            },
            ActionSchema {
                name: "update_activity".into(),
                description: "Update the menu bar activity text".into(),
                params: vec![param("text", "string", true, "Activity status text")],
            },
            ActionSchema {
                name: "get_session_status".into(),
                description: "Get the current agent session status (id, task, steps, scope, affected resources)".into(),
                params: vec![],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        // Most desktop_agent actions are IPC-mediated — the executor can't
        // directly send messages to the compositor. Instead, we return the
        // intended message payload in `data` so the IPC handler can forward it.
        //
        // This is a "command pattern" — the capability produces the command,
        // the IPC layer sends it.
        match action {
            "start_agent_mode" => {
                let task = params["task"].as_str().unwrap_or("agent task").to_string();
                // Optional scope: { capabilities: [...], paths: [...] }
                let scope_val = params.get("scope").cloned().unwrap_or(json!(null));
                // Build a SessionScope-compatible object if scope is present
                let scope_obj = if scope_val.is_object() {
                    let caps = scope_val.get("capabilities").cloned();
                    let paths = scope_val.get("paths").cloned();
                    json!({
                        "capability_whitelist": caps,
                        "path_whitelist": paths,
                    })
                } else {
                    json!(null)
                };
                CapabilityResult { state_delta: None,
                    success: true,
                    data: json!({
                        "ipc_message": "AgentModeStarted",
                        "task": task,
                        "scope": scope_obj,
                    }),
                    error: None,
                }
            }
            "get_session_status" => {
                CapabilityResult { state_delta: None,
                    success: true,
                    data: json!({ "ipc_message": "GetSessionStatus" }),
                    error: None,
                }
            }
            "end_agent_mode" => CapabilityResult { state_delta: None,
                success: true,
                data: json!({ "ipc_message": "AgentModeEnded" }),
                error: None,
            },
            "spawn_app" => {
                let title = params["title"].as_str().unwrap_or("App").to_string();
                let app_id = params["app_id"].as_str().unwrap_or("app").to_string();
                let description = params["description"].as_str().unwrap_or("").to_string();
                let widgets_json = params["widgets_json"].as_str().unwrap_or("[]").to_string();
                CapabilityResult { state_delta: None,
                    success: true,
                    data: json!({
                        "ipc_message": "SpawnApp",
                        "title": title,
                        "app_id": app_id,
                        "description": description,
                        "widgets_json": widgets_json,
                    }),
                    error: None,
                }
            }
            "desktop_action" => {
                let action_str = params["action"].as_str().unwrap_or("").to_string();
                CapabilityResult { state_delta: None,
                    success: true,
                    data: json!({
                        "ipc_message": "DesktopAction",
                        "action": action_str,
                    }),
                    error: None,
                }
            }
            "get_workflow_history" => {
                // The actual history is stored in the observer, which lives
                // in the IPC handler. We return a marker so the IPC layer
                // can inject the real data.
                CapabilityResult { state_delta: None,
                    success: true,
                    data: json!({ "ipc_message": "GetWorkflowHistory" }),
                    error: None,
                }
            }
            "update_activity" => {
                let text = params["text"].as_str().unwrap_or("").to_string();
                CapabilityResult { state_delta: None,
                    success: true,
                    data: json!({
                        "ipc_message": "ActivityUpdate",
                        "text": text,
                    }),
                    error: None,
                }
            }
            _ => CapabilityResult { state_delta: None,
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownAction, format!("Unknown desktop_agent action: {}", action))),
            },
        }
    }
}

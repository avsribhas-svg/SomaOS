//! MediaCapability — generate and manage AI-generated images in soma-media windows.

use serde_json::{json, Value};
use soma_common::{ActionSchema, CapabilityError, CapabilityResult, ErrorReason};
use super::{param, Capability};
use crate::ipc::AppStateCache;

pub struct MediaCapability {
    state_cache: AppStateCache,
}

impl MediaCapability {
    pub fn new(state_cache: AppStateCache) -> Self {
        Self { state_cache }
    }
}

impl Capability for MediaCapability {
    fn name(&self) -> &str { "media" }

    fn description(&self) -> &str {
        "Generate and manage AI-generated images in soma-media windows. Use generate to create, describe to read state, save to export."
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "generate".to_string(),
                description: "Generate an image from a text prompt in a media window".to_string(),
                params: vec![
                    param("prompt", "string", true, "Text description of the image to generate"),
                    param("window_id", "integer", false, "Target media window ID (default: 0 = first/create)"),
                ],
            },
            ActionSchema {
                name: "describe".to_string(),
                description: "Describe the current state of a media window (prompt, status, has_image)".to_string(),
                params: vec![
                    param("window_id", "integer", false, "Target media window ID"),
                ],
            },
            ActionSchema {
                name: "save".to_string(),
                description: "Save the current generated image to a file path".to_string(),
                params: vec![
                    param("path", "string", true, "Output file path (e.g. /tmp/soma-image.png)"),
                    param("window_id", "integer", false, "Source media window ID"),
                ],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        let window_id = params.get("window_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        match action {
            "describe" => {
                let cache = self.state_cache.lock().unwrap();
                match cache.get(&window_id) {
                    Some(state) => CapabilityResult {
                        success: true,
                        data: state.summary.clone(),
                        error: None,
                    },
                    None => CapabilityResult {
                        success: false,
                        data: json!({ "message": "No media window found. Open soma-media from the dock first." }),
                        error: Some(CapabilityError::new(
                            ErrorReason::NotFound,
                            format!("No media window with id {}", window_id),
                        )),
                    },
                }
            }
            "generate" => {
                let prompt = match params.get("prompt").and_then(|v| v.as_str()) {
                    Some(p) if !p.is_empty() => p.to_string(),
                    _ => return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::MissingParam, "prompt is required")),
                    },
                };
                CapabilityResult {
                    success: true,
                    data: json!({
                        "ipc_message": "AppAction",
                        "window_id": window_id,
                        "action": "set_prompt",
                        "params": { "prompt": prompt }
                    }),
                    error: None,
                }
            }
            "save" => {
                let path = match params.get("path").and_then(|v| v.as_str()) {
                    Some(p) if !p.is_empty() => p.to_string(),
                    _ => return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::MissingParam, "path is required")),
                    },
                };
                CapabilityResult {
                    success: true,
                    data: json!({
                        "ipc_message": "AppAction",
                        "window_id": window_id,
                        "action": "save_image",
                        "params": { "path": path }
                    }),
                    error: None,
                }
            }
            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(
                    ErrorReason::UnknownAction,
                    format!("Unknown media action: {}", action),
                )),
            },
        }
    }
}

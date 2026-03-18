//! DocsCapability — agent interface for soma-docs document windows.
//!
//! Reads from the `AppStateCache` (populated by `CompositorMessage::AppStateChanged`),
//! and writes via the command pattern (returns `ipc_message: "AppAction"` in data
//! so that ipc.rs forwards it to the compositor as `AgentMessage::AppAction`).

use super::{param, Capability};
use crate::ipc::AppStateCache;
use serde_json::{json, Value};
use soma_common::{ActionSchema, CapabilityError, CapabilityResult, ErrorReason};

pub struct DocsCapability {
    state_cache: AppStateCache,
}

impl DocsCapability {
    pub fn new(state_cache: AppStateCache) -> Self {
        Self { state_cache }
    }
}

impl Capability for DocsCapability {
    fn name(&self) -> &str {
        "docs"
    }

    fn description(&self) -> &str {
        "Interact with soma-docs document windows. Create documents, append and edit blocks, \
         read content, and set document titles."
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "create".to_string(),
                description: "Create a new Docs window pre-populated with blocks. \
                              The window appears on screen immediately. \
                              Pass all block data upfront.".to_string(),
                params: vec![
                    param("title", "string", true, "Document title (e.g. \"Project Notes\")"),
                    param("blocks", "array", false,
                          "Array of block objects: {\"type\": \"paragraph\"|\"heading\"|\"code\"|\"hline\", \
                           \"text\": \"...\", \"level\": 1-3 (heading), \"lang\": \"rust\" (code)}"),
                ],
            },
            ActionSchema {
                name: "describe".to_string(),
                description: "Return a summary of the document: title, block count, word count, \
                              and full block content.".to_string(),
                params: vec![
                    param("window_id", "number", true, "ID of the Docs window"),
                ],
            },
            ActionSchema {
                name: "append_block".to_string(),
                description: "Append a new block at the end of the document.".to_string(),
                params: vec![
                    param("window_id", "number", true, "ID of the Docs window"),
                    param("type", "string", true, "Block type: \"paragraph\", \"heading\", \"code\", \"hline\""),
                    param("text", "string", false, "Block text content"),
                    param("level", "number", false, "Heading level 1-3 (for heading blocks)"),
                    param("lang", "string", false, "Language hint (for code blocks, e.g. \"rust\")"),
                ],
            },
            ActionSchema {
                name: "write_block".to_string(),
                description: "Replace the text of an existing block by index.".to_string(),
                params: vec![
                    param("window_id", "number", true, "ID of the Docs window"),
                    param("index", "number", true, "0-based block index"),
                    param("text", "string", true, "New text content for the block"),
                ],
            },
            ActionSchema {
                name: "delete_block".to_string(),
                description: "Delete a block by index.".to_string(),
                params: vec![
                    param("window_id", "number", true, "ID of the Docs window"),
                    param("index", "number", true, "0-based block index to delete"),
                ],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        let window_id = params["window_id"].as_u64().unwrap_or(0) as u32;

        match action {
            // ── Create a new Docs window pre-populated with blocks ─────────
            // Command pattern: returns ipc_message so ipc.rs forwards as
            // AgentMessage::AppAction { window_id: 0 } to the compositor.
            "create" => {
                let title = params["title"].as_str().unwrap_or("Untitled Document").to_string();
                let blocks = params.get("blocks").cloned().unwrap_or(Value::Array(Vec::new()));
                CapabilityResult {
                    success: true,
                    data: json!({
                        "ipc_message": "AppAction",
                        "window_id": 0u32,  // 0 = sentinel: create new window
                        "action": "create_and_populate",
                        "params": {
                            "app_type": "docs",
                            "title": title,
                            "blocks": blocks,
                        },
                    }),
                    error: None,
                }
            }

            "describe" => {
                let cache = self.state_cache.lock().unwrap();
                match cache.get(&window_id) {
                    Some(state) => CapabilityResult {
                        success: true,
                        data: json!({
                            "summary": state.summary,
                            "dirty": state.dirty,
                        }),
                        error: None,
                    },
                    None => CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::NotFound,
                            format!("No docs window with id {} found in cache. Open a Docs window first.", window_id))),
                    },
                }
            }

            // ── Mutating actions use the command pattern ───────────────────
            "append_block" => {
                let block_type = params["type"].as_str().unwrap_or("paragraph").to_string();
                let text = params["text"].as_str().unwrap_or("").to_string();
                let mut block_params = json!({
                    "type": block_type,
                    "text": text,
                });
                if let Some(level) = params["level"].as_u64() {
                    block_params["level"] = json!(level);
                }
                if let Some(lang) = params["lang"].as_str() {
                    block_params["lang"] = json!(lang);
                }
                CapabilityResult {
                    success: true,
                    data: json!({
                        "ipc_message": "AppAction",
                        "window_id": window_id,
                        "action": "append_block",
                        "params": block_params,
                    }),
                    error: None,
                }
            }

            "write_block" => {
                let index = params["index"].as_u64().unwrap_or(0);
                let text = params["text"].as_str().unwrap_or("").to_string();
                CapabilityResult {
                    success: true,
                    data: json!({
                        "ipc_message": "AppAction",
                        "window_id": window_id,
                        "action": "write_block",
                        "params": { "index": index, "text": text },
                    }),
                    error: None,
                }
            }

            "delete_block" => {
                let index = params["index"].as_u64().unwrap_or(0);
                CapabilityResult {
                    success: true,
                    data: json!({
                        "ipc_message": "AppAction",
                        "window_id": window_id,
                        "action": "delete_block",
                        "params": { "index": index },
                    }),
                    error: None,
                }
            }

            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownAction,
                    format!("Unknown docs action: '{}'. Known: create, describe, append_block, write_block, delete_block", action))),
            },
        }
    }
}

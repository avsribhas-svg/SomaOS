//! Delegate capability — run a task on a peer SomaOS node.
//!
//! `run` returns a command-pattern sentinel; `ipc.rs` connects to the peer over
//! TCP, authenticates, sends the task, and streams results back.
//! `list_nodes` is synchronous and reads peers from config.

use serde_json::{json, Value};
use soma_common::{ActionSchema, CapabilityError, CapabilityResult, ErrorReason};

use super::{param, Capability};
use crate::config::SomaConfig;

pub struct DelegateCapability;

impl Capability for DelegateCapability {
    fn name(&self) -> &str {
        "delegate"
    }

    fn description(&self) -> &str {
        "Run a task on a peer SomaOS node or list reachable federation nodes"
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "run".into(),
                description: "Delegate a task to a named peer node".into(),
                params: vec![
                    param("node", "string", true, "Peer node name (from config)"),
                    param("task", "string", true, "Natural-language task to run on the peer"),
                ],
            },
            ActionSchema {
                name: "list_nodes".into(),
                description: "List configured peer nodes and their reachability status".into(),
                params: vec![],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult { state_delta: None,
        match action {
            "run" => {
                let node = params["node"].as_str().unwrap_or("").to_string();
                let task = params["task"].as_str().unwrap_or("").to_string();
                if node.is_empty() || task.is_empty() {
                    return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(
                            ErrorReason::MissingParam,
                            "node and task are required".to_string(),
                        )),
                        state_delta: None,
                    };
                }
                // Command pattern — ipc.rs handles the actual TCP connection.
                CapabilityResult {
                    success: true,
                    data: json!({
                        "ipc_message": "DelegateRun",
                        "node": node,
                        "task": task,
                    }),
                    error: None,
                    state_delta: None,
                }
            }

            "list_nodes" => {
                let config = SomaConfig::load();
                let peers: Vec<Value> = config.network.peers.iter().map(|p| {
                    // Synchronous TCP probe: try connect with 1 s timeout.
                    let status = probe_node(&p.addr);
                    json!({
                        "name": p.name,
                        "addr": p.addr,
                        "tls":  p.tls,
                        "status": status,
                    })
                }).collect();

                CapabilityResult {
                    success: true,
                    data: json!({ "nodes": peers }),
                    error: None,
                    state_delta: None,
                }
            }

            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(
                    ErrorReason::UnknownAction,
                    format!("Unknown delegate action: {}", action),
                )),
                state_delta: None,
            },
        }
    }
}

/// Quick TCP reachability probe (1 s timeout, std blocking).
fn probe_node(addr: &str) -> &'static str {
    use std::net::TcpStream;
    use std::time::Duration;
    match TcpStream::connect_timeout(
        &addr.parse().unwrap_or("0.0.0.0:0".parse().unwrap()),
        Duration::from_secs(1),
    ) {
        Ok(_) => "reachable",
        Err(_) => "unreachable",
    }
}

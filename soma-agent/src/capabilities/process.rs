use serde_json::{json, Value};
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityResult};
use std::process::Command;

use super::param;
use super::Capability;

pub struct ProcessCapability;

impl Capability for ProcessCapability {
    fn name(&self) -> &str {
        "process"
    }

    fn description(&self) -> &str {
        "Process and service management — list processes, kill, systemd service control"
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "list_processes".to_string(),
                description: "List running processes".to_string(),
                params: vec![param(
                    "filter",
                    "string",
                    false,
                    "Filter processes by name (optional)",
                )],
            },
            ActionSchema {
                name: "kill_process".to_string(),
                description: "Kill a process by PID".to_string(),
                params: vec![param("pid", "integer", true, "Process ID to kill")],
            },
            ActionSchema {
                name: "service_status".to_string(),
                description: "Get the status of a systemd service".to_string(),
                params: vec![param("name", "string", true, "Service name")],
            },
            ActionSchema {
                name: "service_restart".to_string(),
                description: "Restart a systemd service".to_string(),
                params: vec![param("name", "string", true, "Service name")],
            },
            ActionSchema {
                name: "service_list".to_string(),
                description: "List all systemd services and their states".to_string(),
                params: vec![],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "list_processes" => self.list_processes(params),
            "kill_process" => self.kill_process(params),
            "service_status" => self.service_status(params),
            "service_restart" => self.service_restart(params),
            "service_list" => self.service_list(),
            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownAction, format!("Unknown process action: {}", action))),
            },
        }
    }
}

impl ProcessCapability {
    fn list_processes(&self, params: &Value) -> CapabilityResult {
        let filter = params.get("filter").and_then(|v| v.as_str());

        match Command::new("ps").args(["-eo", "pid,ppid,comm,%mem,%cpu", "--no-headers"]).output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut processes: Vec<Value> = Vec::new();

                for line in stdout.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 5 {
                        let name = parts[2];
                        if let Some(f) = filter {
                            if !name.contains(f) {
                                continue;
                            }
                        }
                        processes.push(json!({
                            "pid": parts[0].parse::<i32>().unwrap_or(0),
                            "ppid": parts[1].parse::<i32>().unwrap_or(0),
                            "name": name,
                            "mem_percent": parts[3],
                            "cpu_percent": parts[4],
                        }));
                    }
                }

                ok(json!({ "processes": processes, "count": processes.len() }))
            }
            Err(e) => err(&format!("Failed to list processes: {}", e)),
        }
    }

    fn kill_process(&self, params: &Value) -> CapabilityResult {
        let pid = match params.get("pid").and_then(|v| v.as_i64()) {
            Some(p) => p,
            None => return err("Missing required param: pid"),
        };

        match Command::new("kill").arg(pid.to_string()).output() {
            Ok(output) => {
                if output.status.success() {
                    ok(json!({ "pid": pid, "killed": true }))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    err(&format!("Failed to kill PID {}: {}", pid, stderr.trim()))
                }
            }
            Err(e) => err(&format!("Failed to execute kill: {}", e)),
        }
    }

    fn service_status(&self, params: &Value) -> CapabilityResult {
        let name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return err("Missing required param: name"),
        };

        match Command::new("systemctl")
            .args(["show", name, "--no-pager", "-p", "ActiveState,SubState,Description,MainPID"])
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut info = json!({ "name": name });
                for line in stdout.lines() {
                    if let Some((key, val)) = line.split_once('=') {
                        info[key.to_lowercase()] = json!(val);
                    }
                }
                ok(info)
            }
            Err(e) => err(&format!("Failed to query service '{}': {}", name, e)),
        }
    }

    fn service_restart(&self, params: &Value) -> CapabilityResult {
        let name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return err("Missing required param: name"),
        };

        match Command::new("systemctl").args(["restart", name]).output() {
            Ok(output) => {
                if output.status.success() {
                    ok(json!({ "name": name, "restarted": true }))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    err(&format!("Failed to restart '{}': {}", name, stderr.trim()))
                }
            }
            Err(e) => err(&format!("Failed to restart service '{}': {}", name, e)),
        }
    }

    fn service_list(&self) -> CapabilityResult {
        match Command::new("systemctl")
            .args(["list-units", "--type=service", "--no-pager", "--no-legend"])
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut services: Vec<Value> = Vec::new();

                for line in stdout.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        services.push(json!({
                            "unit": parts[0],
                            "load": parts[1],
                            "active": parts[2],
                            "sub": parts[3],
                        }));
                    }
                }

                ok(json!({ "services": services, "count": services.len() }))
            }
            Err(e) => err(&format!("Failed to list services: {}", e)),
        }
    }
}

fn ok(data: Value) -> CapabilityResult {
    CapabilityResult {
        success: true,
        data,
        error: None,
    }
}

fn err(msg: &str) -> CapabilityResult {
    CapabilityResult {
        success: false,
        data: Value::Null,
        error: Some(CapabilityError::new(ErrorReason::InternalError, "msg")),
    }
}

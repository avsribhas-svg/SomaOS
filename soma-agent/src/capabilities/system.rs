use serde_json::{json, Value};
use soma_common::{ActionSchema, CapabilityResult};
use std::fs;

use super::param;
use super::Capability;

pub struct SystemCapability;

impl Capability for SystemCapability {
    fn name(&self) -> &str {
        "system"
    }

    fn description(&self) -> &str {
        "System information — hostname, uptime, disk usage, memory, network status"
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "hostname".to_string(),
                description: "Get the system hostname".to_string(),
                params: vec![],
            },
            ActionSchema {
                name: "uptime".to_string(),
                description: "Get system uptime".to_string(),
                params: vec![],
            },
            ActionSchema {
                name: "disk_usage".to_string(),
                description: "Get disk usage information".to_string(),
                params: vec![param("path", "string", false, "Mount point to check (default: /)")],
            },
            ActionSchema {
                name: "memory_info".to_string(),
                description: "Get memory usage (total, free, used)".to_string(),
                params: vec![],
            },
            ActionSchema {
                name: "network_status".to_string(),
                description: "Get network interface information".to_string(),
                params: vec![],
            },
            ActionSchema {
                name: "kernel_info".to_string(),
                description: "Get kernel version and OS information".to_string(),
                params: vec![],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "hostname" => self.hostname(),
            "uptime" => self.uptime(),
            "disk_usage" => self.disk_usage(params),
            "memory_info" => self.memory_info(),
            "network_status" => self.network_status(),
            "kernel_info" => self.kernel_info(),
            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(format!("Unknown system action: {}", action)),
            },
        }
    }
}

impl SystemCapability {
    fn hostname(&self) -> CapabilityResult {
        match fs::read_to_string("/etc/hostname") {
            Ok(h) => ok(json!({ "hostname": h.trim() })),
            Err(_) => {
                // Fallback to procfs
                match fs::read_to_string("/proc/sys/kernel/hostname") {
                    Ok(h) => ok(json!({ "hostname": h.trim() })),
                    Err(e) => err(&format!("Cannot read hostname: {}", e)),
                }
            }
        }
    }

    fn uptime(&self) -> CapabilityResult {
        match fs::read_to_string("/proc/uptime") {
            Ok(content) => {
                let parts: Vec<&str> = content.split_whitespace().collect();
                if let Some(secs_str) = parts.first() {
                    if let Ok(secs) = secs_str.parse::<f64>() {
                        let hours = (secs / 3600.0) as u64;
                        let mins = ((secs % 3600.0) / 60.0) as u64;
                        return ok(json!({
                            "uptime_seconds": secs as u64,
                            "uptime_human": format!("{}h {}m", hours, mins),
                        }));
                    }
                }
                err("Failed to parse /proc/uptime")
            }
            Err(e) => err(&format!("Cannot read uptime: {}", e)),
        }
    }

    fn disk_usage(&self, params: &Value) -> CapabilityResult {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("/");

        match std::process::Command::new("df")
            .args(["-h", path])
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = stdout.lines().collect();
                if lines.len() >= 2 {
                    let parts: Vec<&str> = lines[1].split_whitespace().collect();
                    if parts.len() >= 6 {
                        return ok(json!({
                            "filesystem": parts[0],
                            "size": parts[1],
                            "used": parts[2],
                            "available": parts[3],
                            "use_percent": parts[4],
                            "mount": parts[5],
                        }));
                    }
                }
                err("Failed to parse df output")
            }
            Err(e) => err(&format!("Failed to run df: {}", e)),
        }
    }

    fn memory_info(&self) -> CapabilityResult {
        match fs::read_to_string("/proc/meminfo") {
            Ok(content) => {
                let mut info = json!({});
                for line in content.lines() {
                    if let Some((key, val)) = line.split_once(':') {
                        let key = key.trim().to_lowercase().replace(' ', "_");
                        let val = val.trim().to_string();
                        // Parse common keys
                        match key.as_str() {
                            "memtotal" | "memfree" | "memavailable" | "buffers" | "cached"
                            | "swaptotal" | "swapfree" => {
                                info[&key] = json!(val);
                            }
                            _ => {}
                        }
                    }
                }
                ok(info)
            }
            Err(e) => err(&format!("Cannot read /proc/meminfo: {}", e)),
        }
    }

    fn network_status(&self) -> CapabilityResult {
        match std::process::Command::new("ip")
            .args(["addr", "show"])
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut interfaces: Vec<Value> = Vec::new();
                let mut current_iface = String::new();
                let mut current_addrs: Vec<String> = Vec::new();

                for line in stdout.lines() {
                    if !line.starts_with(' ') && !line.starts_with('\t') {
                        // New interface line
                        if !current_iface.is_empty() {
                            interfaces.push(json!({
                                "interface": current_iface,
                                "addresses": current_addrs,
                            }));
                        }
                        // Extract interface name
                        if let Some(name) = line.split(':').nth(1) {
                            current_iface = name.trim().to_string();
                        }
                        current_addrs = Vec::new();
                    } else if line.contains("inet ") {
                        if let Some(addr) = line.split_whitespace().nth(1) {
                            current_addrs.push(addr.to_string());
                        }
                    }
                }
                if !current_iface.is_empty() {
                    interfaces.push(json!({
                        "interface": current_iface,
                        "addresses": current_addrs,
                    }));
                }

                ok(json!({ "interfaces": interfaces }))
            }
            Err(e) => err(&format!("Failed to query network: {}", e)),
        }
    }

    fn kernel_info(&self) -> CapabilityResult {
        let version = fs::read_to_string("/proc/version")
            .unwrap_or_else(|_| "unknown".to_string());
        let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();

        let mut info = json!({
            "kernel_version": version.trim(),
        });

        for line in os_release.lines() {
            if let Some((key, val)) = line.split_once('=') {
                let val = val.trim_matches('"');
                match key {
                    "NAME" => info["os_name"] = json!(val),
                    "VERSION" => info["os_version"] = json!(val),
                    _ => {}
                }
            }
        }

        ok(info)
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
        error: Some(msg.to_string()),
    }
}

use serde_json::{json, Value};
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityResult};
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
            _ => CapabilityResult { state_delta: None,
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownAction, format!("Unknown system action: {}", action))),
            },
        }
    }
}

impl SystemCapability {
    fn hostname(&self) -> CapabilityResult {
        // Linux: /etc/hostname or /proc/sys/kernel/hostname
        if let Ok(h) = fs::read_to_string("/etc/hostname") {
            let h = h.trim().to_string();
            if !h.is_empty() { return ok(json!({ "hostname": h })); }
        }
        if let Ok(h) = fs::read_to_string("/proc/sys/kernel/hostname") {
            let h = h.trim().to_string();
            if !h.is_empty() { return ok(json!({ "hostname": h })); }
        }
        // macOS / fallback: hostname command
        match std::process::Command::new("hostname").output() {
            Ok(out) => {
                let h = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !h.is_empty() { ok(json!({ "hostname": h })) }
                else { err("hostname command returned empty output") }
            }
            Err(e) => err(&format!("Cannot get hostname: {}", e)),
        }
    }

    fn uptime(&self) -> CapabilityResult {
        // Linux: /proc/uptime
        if let Ok(content) = fs::read_to_string("/proc/uptime") {
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
        }
        // macOS / fallback: uptime command
        match std::process::Command::new("uptime").output() {
            Ok(out) => {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                ok(json!({ "uptime": s }))
            }
            Err(e) => err(&format!("Cannot get uptime: {}", e)),
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
        // Linux: read /proc/meminfo
        #[cfg(target_os = "linux")]
        {
            match fs::read_to_string("/proc/meminfo") {
                Ok(content) => {
                    let mut info = json!({});
                    for line in content.lines() {
                        if let Some((key, val)) = line.split_once(':') {
                            let key = key.trim().to_lowercase().replace(' ', "_");
                            let val = val.trim().to_string();
                            match key.as_str() {
                                "memtotal" | "memfree" | "memavailable" | "buffers" | "cached"
                                | "swaptotal" | "swapfree" => { info[&key] = json!(val); }
                                _ => {}
                            }
                        }
                    }
                    return ok(info);
                }
                Err(e) => return err(&format!("Cannot read /proc/meminfo: {}", e)),
            }
        }

        // macOS / other: vm_stat + sysctl
        #[cfg(not(target_os = "linux"))]
        {
            use std::process::Command;
            let mut info = json!({});

            // Total physical memory
            if let Ok(out) = Command::new("sysctl").arg("-n").arg("hw.memsize").output() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Ok(bytes) = s.parse::<u64>() {
                    info["memtotal"] = json!(format!("{} kB", bytes / 1024));
                    info["memtotal_bytes"] = json!(bytes);
                }
            }

            // Free/used from vm_stat
            if let Ok(out) = Command::new("vm_stat").output() {
                let text = String::from_utf8_lossy(&out.stdout);
                let mut page_size: u64 = 4096;
                let mut free_pages: u64 = 0;
                let mut inactive_pages: u64 = 0;
                for line in text.lines() {
                    if line.contains("page size of") {
                        if let Some(n) = line.split_whitespace()
                            .find(|w| w.parse::<u64>().is_ok())
                            .and_then(|w| w.parse().ok())
                        { page_size = n; }
                    }
                    let parse_pages = |l: &str| -> u64 {
                        l.split(':').nth(1)
                            .map(|s| s.trim().trim_end_matches('.'))
                            .and_then(|s| s.replace(',', "").parse().ok())
                            .unwrap_or(0)
                    };
                    if line.starts_with("Pages free")     { free_pages     = parse_pages(line); }
                    if line.starts_with("Pages inactive") { inactive_pages = parse_pages(line); }
                }
                let free_bytes = (free_pages + inactive_pages) * page_size;
                info["memfree"] = json!(format!("{} kB", free_bytes / 1024));
                if let Some(total) = info["memtotal_bytes"].as_u64() {
                    let used = total.saturating_sub(free_bytes);
                    info["memused"] = json!(format!("{} kB", used / 1024));
                }
            }

            ok(info)
        }
    }

    fn network_status(&self) -> CapabilityResult {
        // Try `ip` (Linux) first, then `ifconfig` (macOS/BSD)
        let output = std::process::Command::new("ip")
            .args(["addr", "show"])
            .output()
            .ok()
            .filter(|o| o.status.success() && !o.stdout.is_empty())
            .or_else(|| std::process::Command::new("ifconfig").output().ok());

        match output {
            Some(out) => {
                let text = String::from_utf8_lossy(&out.stdout).to_string();
                // Parse inet addresses for a compact summary
                let mut interfaces: Vec<Value> = Vec::new();
                let mut current_iface = String::new();
                let mut current_addrs: Vec<String> = Vec::new();
                for line in text.lines() {
                    if !line.starts_with(' ') && !line.starts_with('\t') {
                        if !current_iface.is_empty() {
                            interfaces.push(json!({ "interface": current_iface, "addresses": current_addrs }));
                        }
                        // `ip`: "2: eth0: <...>" — `ifconfig`: "en0: flags=..."
                        current_iface = line.split([':', ' ']).find(|s| !s.is_empty() && s.chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false)).unwrap_or("").trim().to_string();
                        current_addrs = Vec::new();
                    } else if line.contains("inet ") {
                        if let Some(addr) = line.split_whitespace().nth(1) {
                            current_addrs.push(addr.to_string());
                        }
                    }
                }
                if !current_iface.is_empty() {
                    interfaces.push(json!({ "interface": current_iface, "addresses": current_addrs }));
                }
                ok(json!({ "interfaces": interfaces }))
            }
            None => err("Cannot query network interfaces (no 'ip' or 'ifconfig' found)"),
        }
    }

    fn kernel_info(&self) -> CapabilityResult {
        // Linux: /proc/version; macOS fallback: uname -a
        let version = fs::read_to_string("/proc/version")
            .ok()
            .or_else(|| {
                std::process::Command::new("uname")
                    .arg("-a")
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
        let mut info = json!({ "kernel_version": version.trim() });
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
        // macOS: add sw_vers info if available
        if version.contains("Darwin") || cfg!(target_os = "macos") {
            if let Ok(out) = std::process::Command::new("sw_vers").output() {
                info["sw_vers"] = json!(String::from_utf8_lossy(&out.stdout).trim().to_string());
            }
        }
        ok(info)
    }
}

fn ok(data: Value) -> CapabilityResult {
    CapabilityResult { state_delta: None,
        success: true,
        data,
        error: None,
    }
}

fn err(msg: &str) -> CapabilityResult {
    CapabilityResult { state_delta: None,
        success: false,
        data: Value::Null,
        error: Some(CapabilityError::new(ErrorReason::InternalError, msg)),
    }
}

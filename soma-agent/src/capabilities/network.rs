use serde_json::{json, Value};
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityResult, ParamSchema};
use std::net::TcpStream;
use std::process::Command;
use std::time::Duration;

use super::{param, Capability};

pub struct NetworkCapability;

impl Capability for NetworkCapability {
    fn name(&self) -> &str {
        "network"
    }

    fn description(&self) -> &str {
        "Network diagnostics — ping, DNS, HTTP requests, interface listing, port checks"
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "ping".to_string(),
                description: "Ping a host".to_string(),
                params: vec![
                    param("host", "string", true, "Hostname or IP to ping"),
                    param("count", "integer", false, "Number of pings (default 4)"),
                ],
            },
            ActionSchema {
                name: "dns_lookup".to_string(),
                description: "Resolve a hostname to IP addresses".to_string(),
                params: vec![param("hostname", "string", true, "Hostname to resolve")],
            },
            ActionSchema {
                name: "curl".to_string(),
                description: "Make an HTTP request to a URL".to_string(),
                params: vec![
                    param("url", "string", true, "URL to request"),
                    param("method", "string", false, "HTTP method (default GET)"),
                ],
            },
            ActionSchema {
                name: "ifconfig".to_string(),
                description: "List network interfaces and addresses".to_string(),
                params: vec![],
            },
            ActionSchema {
                name: "port_check".to_string(),
                description: "Check if a TCP port is open on a host".to_string(),
                params: vec![
                    param("host", "string", true, "Host to check"),
                    param("port", "integer", true, "Port number"),
                ],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "ping" => execute_ping(params),
            "dns_lookup" => execute_dns_lookup(params),
            "curl" => execute_curl(params),
            "ifconfig" => execute_ifconfig(),
            "port_check" => execute_port_check(params),
            _ => CapabilityResult { state_delta: None,
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownAction, format!("Unknown network action: {}", action))),
            },
        }
    }
}

fn execute_ping(params: &Value) -> CapabilityResult {
    let host = match params.get("host").and_then(|v| v.as_str()) {
        Some(h) => h,
        None => {
            return CapabilityResult { state_delta: None,
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "Missing required param: host")),
            }
        }
    };

    let count = params
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(4);

    // macOS uses -c, Linux uses -c
    let output = Command::new("ping")
        .args(["-c", &count.to_string(), host])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();

            if out.status.success() {
                // Parse basic stats from output
                let lines: Vec<&str> = stdout.lines().collect();
                let stats_line = lines.iter().rev().find(|l| l.contains("packets"));

                CapabilityResult { state_delta: None,
                    success: true,
                    data: json!({
                        "host": host,
                        "count": count,
                        "output": stdout.lines().take(10).collect::<Vec<_>>().join("\n"),
                        "stats": stats_line.unwrap_or(&""),
                    }),
                    error: None,
                }
            } else {
                CapabilityResult { state_delta: None,
                    success: false,
                    data: json!({"output": stderr}),
                    error: Some(CapabilityError::new(ErrorReason::NetworkError, format!("Ping failed: {}", stderr.lines().next().unwrap_or("")))),
                }
            }
        }
        Err(e) => CapabilityResult { state_delta: None,
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::CommandFailed, format!("Failed to run ping: {}", e))),
        },
    }
}

fn execute_dns_lookup(params: &Value) -> CapabilityResult {
    let hostname = match params.get("hostname").and_then(|v| v.as_str()) {
        Some(h) => h,
        None => {
            return CapabilityResult { state_delta: None,
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "Missing required param: hostname")),
            }
        }
    };

    // Use system resolver via `host` or `nslookup` command
    let output = Command::new("host").arg(hostname).output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            if out.status.success() {
                let addresses: Vec<String> = stdout
                    .lines()
                    .filter(|l| l.contains("has address") || l.contains("has IPv6 address"))
                    .map(|l| {
                        l.split_whitespace().last().unwrap_or("").to_string()
                    })
                    .collect();

                CapabilityResult { state_delta: None,
                    success: true,
                    data: json!({
                        "hostname": hostname,
                        "addresses": addresses,
                        "raw": stdout.trim(),
                    }),
                    error: None,
                }
            } else {
                CapabilityResult { state_delta: None,
                    success: false,
                    data: Value::Null,
                    error: Some(CapabilityError::new(ErrorReason::NetworkError, format!("DNS lookup failed for {}", hostname))),
                }
            }
        }
        Err(_) => {
            // Fallback: try nslookup
            match Command::new("nslookup").arg(hostname).output() {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    CapabilityResult { state_delta: None,
                        success: out.status.success(),
                        data: json!({"hostname": hostname, "raw": stdout.trim()}),
                        error: if out.status.success() {
                            None
                        } else {
                            Some(CapabilityError::new(ErrorReason::NetworkError, "DNS lookup failed"))
                        },
                    }
                }
                Err(e) => CapabilityResult { state_delta: None,
                    success: false,
                    data: Value::Null,
                    error: Some(CapabilityError::new(ErrorReason::UnsupportedPlatform, format!("No DNS tools available: {}", e))),
                },
            }
        }
    }
}

fn execute_curl(params: &Value) -> CapabilityResult {
    let url = match params.get("url").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => {
            return CapabilityResult { state_delta: None,
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "Missing required param: url")),
            }
        }
    };

    let method = params
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET");

    let output = Command::new("curl")
        .args([
            "-s",               // silent
            "-o", "/dev/null",  // discard body for now
            "-w", "%{http_code} %{time_total} %{size_download} %{content_type}",
            "-X", method,
            "--max-time", "10",
            url,
        ])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let parts: Vec<&str> = stdout.splitn(4, ' ').collect();

            let status_code = parts.first().unwrap_or(&"0");
            let time = parts.get(1).unwrap_or(&"0");
            let size = parts.get(2).unwrap_or(&"0");
            let content_type = parts.get(3).unwrap_or(&"unknown");

            CapabilityResult { state_delta: None,
                success: out.status.success(),
                data: json!({
                    "url": url,
                    "method": method,
                    "status_code": status_code,
                    "time_seconds": time,
                    "size_bytes": size,
                    "content_type": content_type.trim(),
                }),
                error: None,
            }
        }
        Err(e) => CapabilityResult { state_delta: None,
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::CommandFailed, format!("Failed to run curl: {}", e))),
        },
    }
}

fn execute_ifconfig() -> CapabilityResult {
    // Try ifconfig first, then ip addr
    let output = Command::new("ifconfig").output().or_else(|_| {
        Command::new("ip").args(["addr", "show"]).output()
    });

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();

            // Parse interfaces (basic)
            let mut interfaces = Vec::new();
            let mut current_name = String::new();
            let mut current_addrs = Vec::new();

            for line in stdout.lines() {
                if !line.starts_with(' ') && !line.starts_with('\t') && !line.is_empty() {
                    // New interface
                    if !current_name.is_empty() {
                        interfaces.push(json!({
                            "name": current_name,
                            "addresses": current_addrs,
                        }));
                        current_addrs = Vec::new();
                    }
                    current_name = line.split(':').next().unwrap_or("").trim().to_string();
                    // Remove interface index (ip addr format: "2: en0:")
                    if let Some(stripped) = current_name.strip_prefix(|c: char| c.is_ascii_digit()) {
                        current_name = stripped.trim_start_matches(": ").to_string();
                    }
                }
                let trimmed = line.trim();
                if trimmed.starts_with("inet ") {
                    let addr = trimmed.split_whitespace().nth(1).unwrap_or("");
                    current_addrs.push(addr.to_string());
                } else if trimmed.starts_with("inet6 ") {
                    let addr = trimmed.split_whitespace().nth(1).unwrap_or("");
                    current_addrs.push(format!("ipv6:{}", addr));
                }
            }
            if !current_name.is_empty() {
                interfaces.push(json!({
                    "name": current_name,
                    "addresses": current_addrs,
                }));
            }

            CapabilityResult { state_delta: None,
                success: true,
                data: json!({
                    "count": interfaces.len(),
                    "interfaces": interfaces,
                }),
                error: None,
            }
        }
        Err(e) => CapabilityResult { state_delta: None,
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::UnsupportedPlatform, format!("No network tools available: {}", e))),
        },
    }
}

fn execute_port_check(params: &Value) -> CapabilityResult {
    let host = match params.get("host").and_then(|v| v.as_str()) {
        Some(h) => h,
        None => {
            return CapabilityResult { state_delta: None,
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "Missing required param: host")),
            }
        }
    };

    let port = match params.get("port").and_then(|v| v.as_u64()) {
        Some(p) => p as u16,
        None => {
            return CapabilityResult { state_delta: None,
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "Missing required param: port")),
            }
        }
    };

    let addr = format!("{}:{}", host, port);
    let is_open = TcpStream::connect_timeout(
        &addr.parse().unwrap_or_else(|_| {
            // Fallback: try resolving via ToSocketAddrs
            use std::net::ToSocketAddrs;
            addr.to_socket_addrs()
                .ok()
                .and_then(|mut addrs| addrs.next())
                .unwrap_or_else(|| "0.0.0.0:0".parse().expect("hardcoded valid socket address"))
        }),
        Duration::from_secs(3),
    )
    .is_ok();

    CapabilityResult { state_delta: None,
        success: true,
        data: json!({
            "host": host,
            "port": port,
            "open": is_open,
            "status": if is_open { "open" } else { "closed/filtered" },
        }),
        error: None,
    }
}

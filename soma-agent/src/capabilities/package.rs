use serde_json::{json, Value};
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityResult};
use std::process::Command;

use super::{param, Capability};

pub struct PackageCapability;

/// Validate a package name or search query.
/// Allows alphanumerics and common package name characters; rejects anything
/// that could be interpreted as flags or shell metacharacters by package managers.
fn validate_package_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("Package name must not be empty");
    }
    if name.starts_with('-') {
        return Err("Package name must not start with '-' (looks like a flag)");
    }
    let all_valid = name.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '+' | ':' | '@' | '/'));
    if !all_valid {
        return Err("Package name contains invalid characters (allowed: alphanumeric, -, _, ., +, :, @, /)");
    }
    Ok(())
}

/// Detect the system's package manager
fn detect_package_manager() -> Option<&'static str> {
    // Check in order of likelihood
    for pm in &["brew", "apt", "apk", "dnf", "yum", "pacman"] {
        if Command::new("which")
            .arg(pm)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(pm);
        }
    }
    None
}

impl Capability for PackageCapability {
    fn name(&self) -> &str {
        "package"
    }

    fn description(&self) -> &str {
        "Package management — list, search, install, remove packages"
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "list_installed".to_string(),
                description: "List installed packages".to_string(),
                params: vec![param(
                    "filter",
                    "string",
                    false,
                    "Filter packages by name (substring match)",
                )],
            },
            ActionSchema {
                name: "search".to_string(),
                description: "Search for available packages".to_string(),
                params: vec![param("query", "string", true, "Package name to search for")],
            },
            ActionSchema {
                name: "install".to_string(),
                description: "Install a package (requires approval)".to_string(),
                params: vec![param("name", "string", true, "Package name to install")],
            },
            ActionSchema {
                name: "remove".to_string(),
                description: "Remove an installed package".to_string(),
                params: vec![param("name", "string", true, "Package name to remove")],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        let pm = match detect_package_manager() {
            Some(pm) => pm,
            None => {
                return CapabilityResult {
                    success: false,
                    data: Value::Null,
                    error: Some(CapabilityError::new(ErrorReason::UnsupportedPlatform, "No supported package manager found")),
                }
            }
        };

        match action {
            "list_installed" => execute_list(pm, params),
            "search" => execute_search(pm, params),
            "install" => execute_install(pm, params),
            "remove" => execute_remove(pm, params),
            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownAction, format!("Unknown package action: {}", action))),
            },
        }
    }
}

fn execute_list(pm: &str, params: &Value) -> CapabilityResult {
    let filter = params
        .get("filter")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let output = match pm {
        "brew" => Command::new("brew").args(["list", "--formula"]).output(),
        "apt" => Command::new("dpkg").args(["--get-selections"]).output(),
        "apk" => Command::new("apk").args(["list", "--installed"]).output(),
        "dnf" | "yum" => Command::new(pm).args(["list", "installed"]).output(),
        "pacman" => Command::new("pacman").args(["-Q"]).output(),
        _ => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnsupportedPlatform, format!("Unsupported package manager: {}", pm))),
            }
        }
    };

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let mut packages: Vec<String> = stdout
                .lines()
                .filter(|l| !l.is_empty())
                .filter(|l| filter.is_empty() || l.to_lowercase().contains(&filter.to_lowercase()))
                .map(|l| {
                    // Normalize: take first column (package name)
                    l.split_whitespace().next().unwrap_or(l).to_string()
                })
                .collect();

            packages.sort();

            CapabilityResult {
                success: true,
                data: json!({
                    "package_manager": pm,
                    "count": packages.len(),
                    "packages": packages.iter().take(50).collect::<Vec<_>>(),
                    "truncated": packages.len() > 50,
                }),
                error: None,
            }
        }
        Err(e) => CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::CommandFailed, format!("Failed to list packages: {}", e))),
        },
    }
}

fn execute_search(pm: &str, params: &Value) -> CapabilityResult {
    let query = match params.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "Missing required param: query")),
            }
        }
    };
    if let Err(e) = validate_package_name(query) {
        return CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::InvalidParam, e)),
        };
    }

    let output = match pm {
        "brew" => Command::new("brew").args(["search", query]).output(),
        "apt" => Command::new("apt-cache").args(["search", query]).output(),
        "apk" => Command::new("apk").args(["search", query]).output(),
        "dnf" | "yum" => Command::new(pm).args(["search", query]).output(),
        "pacman" => Command::new("pacman").args(["-Ss", query]).output(),
        _ => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnsupportedPlatform, format!("Unsupported package manager: {}", pm))),
            }
        }
    };

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let results: Vec<String> = stdout
                .lines()
                .filter(|l| !l.is_empty())
                .take(20)
                .map(|l| l.to_string())
                .collect();

            CapabilityResult {
                success: true,
                data: json!({
                    "package_manager": pm,
                    "query": query,
                    "count": results.len(),
                    "results": results,
                }),
                error: None,
            }
        }
        Err(e) => CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::CommandFailed, format!("Failed to search packages: {}", e))),
        },
    }
}

fn execute_install(pm: &str, params: &Value) -> CapabilityResult {
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "Missing required param: name")),
            }
        }
    };
    if let Err(e) = validate_package_name(name) {
        return CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::InvalidParam, e)),
        };
    }

    let output = match pm {
        "brew" => Command::new("brew").args(["install", name]).output(),
        "apt" => Command::new("apt").args(["install", "-y", name]).output(),
        "apk" => Command::new("apk").args(["add", name]).output(),
        "dnf" | "yum" => Command::new(pm).args(["install", "-y", name]).output(),
        "pacman" => Command::new("pacman").args(["-S", "--noconfirm", name]).output(),
        _ => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnsupportedPlatform, format!("Unsupported package manager: {}", pm))),
            }
        }
    };

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();

            CapabilityResult {
                success: out.status.success(),
                data: json!({
                    "package_manager": pm,
                    "package": name,
                    "action": "install",
                    "output": if stdout.is_empty() { &stderr } else { &stdout },
                }),
                error: if out.status.success() {
                    None
                } else {
                    Some(CapabilityError::new(ErrorReason::CommandFailed, format!("Install failed: {}", stderr.lines().last().unwrap_or(""))))
                },
            }
        }
        Err(e) => CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::CommandFailed, format!("Failed to install package: {}", e))),
        },
    }
}

fn execute_remove(pm: &str, params: &Value) -> CapabilityResult {
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "Missing required param: name")),
            }
        }
    };
    if let Err(e) = validate_package_name(name) {
        return CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::InvalidParam, e)),
        };
    }

    let output = match pm {
        "brew" => Command::new("brew").args(["uninstall", name]).output(),
        "apt" => Command::new("apt").args(["remove", "-y", name]).output(),
        "apk" => Command::new("apk").args(["del", name]).output(),
        "dnf" | "yum" => Command::new(pm).args(["remove", "-y", name]).output(),
        "pacman" => Command::new("pacman").args(["-R", "--noconfirm", name]).output(),
        _ => {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnsupportedPlatform, format!("Unsupported package manager: {}", pm))),
            }
        }
    };

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();

            CapabilityResult {
                success: out.status.success(),
                data: json!({
                    "package_manager": pm,
                    "package": name,
                    "action": "remove",
                    "output": if stdout.is_empty() { &stderr } else { &stdout },
                }),
                error: if out.status.success() {
                    None
                } else {
                    Some(CapabilityError::new(ErrorReason::CommandFailed, format!("Remove failed: {}", stderr.lines().last().unwrap_or(""))))
                },
            }
        }
        Err(e) => CapabilityResult {
            success: false,
            data: Value::Null,
            error: Some(CapabilityError::new(ErrorReason::CommandFailed, format!("Failed to remove package: {}", e))),
        },
    }
}

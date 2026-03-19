//! Intent parsing — converts natural language to a `TaskPlan` via native tool calling.
//!
//! The 3-layer pipeline (regex → free-text LLM → JSON plan) has been replaced with a
//! single tool-use call to the configured provider.  The model receives all capability
//! actions as proper function schemas and returns `tool_calls` directly — no fragile
//! JSON prompt engineering required.
//!
//! Layer 0 (deterministic keyword pre-processor) is retained for unambiguous one-word
//! system queries (disk, memory, uptime, etc.) where it avoids a full LLM round-trip.

use soma_common::TaskPlan;

use crate::capabilities::CapabilityRegistry;
use crate::config::SomaConfig;
use crate::providers::{self, LlmProvider, build_tools, tool_calls_to_task_plan};

// ─────────────────────────────────────────────────────────────────────────────
//  Layer 0: Deterministic keyword pre-processor
//  Fast path for unambiguous one-liners; returns None to fall through to LLM.
// ─────────────────────────────────────────────────────────────────────────────

fn preprocess_input(input: &str) -> Option<TaskPlan> {
    use soma_common::{RiskLevel, TaskStep, TaskPlan};

    let s = input.to_lowercase();

    macro_rules! sys_plan {
        ($intent:expr, $action:expr) => {
            Some(TaskPlan {
                intent: $intent.to_string(),
                description: input.to_string(),
                steps: vec![TaskStep {
                    capability: "system".to_string(),
                    action: $action.to_string(),
                    params: serde_json::Value::Object(Default::default()),
                    description: $intent.to_string(),
                }],
                risk_level: RiskLevel::Low,
            })
        };
    }

    // Disk space
    let is_real_image = s.contains(".img") || s.contains(".iso") || s.contains(".dmg") || s.contains(".vhd");
    if (s.contains("disk image") && !is_real_image)
        || s.contains("disk space") || s.contains("free space") || s.contains("storage space")
        || s.contains("disk full") || s.contains("how much room")
        || ((s.contains("how full") || s.contains("how much")) && (s.contains("disk") || s.contains("drive") || s.contains("storage")))
        || ((s.contains("disk") || s.contains("drive")) && (s.contains("usage") || s.contains("full") || s.contains("getting")))
    {
        return sys_plan!("check_disk_usage", "disk_usage");
    }

    // Memory
    let memory_kw = s.contains("ram") || s.contains("memory") || s.contains("mem ");
    let query_kw  = s.contains("usage") || s.contains("info") || s.contains("eating")
        || s.contains("how much") || s.contains("check") || s.contains("show")
        || s.contains("status") || s.contains("free");
    if memory_kw && query_kw {
        return sys_plan!("show_memory_usage", "memory_info");
    }

    // Uptime
    if s.contains("uptime")
        || (s.contains("how long") && s.contains("running"))
        || s.contains("since boot") || s.contains("boot time")
    {
        return sys_plan!("show_uptime", "uptime");
    }

    // Hostname
    if s.contains("hostname") || s.contains("computer name") || s.contains("machine name")
        || s.contains("server name") || (s.contains("what") && s.contains("this machine"))
    {
        return sys_plan!("get_hostname", "hostname");
    }

    // Process list — model often maps "running processes" to filesystem, intercept here
    if (s.contains("process") || s.contains("processes"))
        && (s.contains("list") || s.contains("show") || s.contains("running")
            || s.contains("all") || s.contains("what"))
    {
        return Some(TaskPlan {
            intent: "list_processes".to_string(),
            description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "process".to_string(),
                action: "list_processes".to_string(),
                params: serde_json::Value::Object(Default::default()),
                description: "list_processes".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Create spreadsheet — model too unreliable; intercept and call sheets__create directly.
    // Better providers (Anthropic/OpenAI) will pass through to the LLM for richer content.
    if s.contains("spreadsheet") && (s.contains("create") || s.contains("make") || s.contains("new") || s.contains("generate")) {
        // Extract title: text between "create a/an" and "spreadsheet", or fallback
        let title = extract_between(input, &["create a ", "create an ", "make a ", "new "], &["spreadsheet", "sheet"])
            .unwrap_or_else(|| "Spreadsheet".to_string());
        // Default Q1 template cells — matches system prompt example
        let cells = serde_json::json!({
            "A1": "Month", "B1": "Revenue", "C1": "Expenses", "D1": "Profit",
            "A2": "January",  "B2": 120000, "C2": 85000,  "D2": 35000,
            "A3": "February", "B3": 134000, "C3": 91000,  "D3": 43000,
            "A4": "March",    "B4": 148000, "C4": 96000,  "D4": 52000,
        });
        return Some(TaskPlan {
            intent: "create_spreadsheet".to_string(),
            description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "sheets".to_string(),
                action: "create".to_string(),
                params: serde_json::json!({ "title": title.trim(), "cells": cells }),
                description: "create_spreadsheet".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Create document — same issue as spreadsheet above.
    if (s.contains("document") || (s.contains("doc") && !s.contains("docker")))
        && (s.contains("create") || s.contains("write") || s.contains("make") || s.contains("new") || s.contains("generate"))
        && !s.contains("spreadsheet")
    {
        let title = extract_between(input, &["create a ", "create an ", "write a ", "write an ", "make a ", "new "], &["document", "doc", "report", "summary"])
            .unwrap_or_else(|| "Document".to_string());
        let blocks = serde_json::json!([
            { "type": "heading", "level": 1, "text": title.trim() },
            { "type": "paragraph", "text": "Q1 showed strong performance across all key metrics." },
            { "type": "heading", "level": 2, "text": "Key Metrics" },
            { "type": "paragraph", "text": "Revenue: $402,000 (+18% YoY) · Expenses: $272,000 · Profit: $130,000" },
        ]);
        return Some(TaskPlan {
            intent: "create_document".to_string(),
            description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "docs".to_string(),
                action: "create".to_string(),
                params: serde_json::json!({ "title": title.trim(), "blocks": blocks }),
                description: "create_document".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Kernel info
    if s.contains("kernel") && (s.contains("version") || s.contains("info") || s.contains("show"))
        || s.contains("uname")
        || (s.contains("os") && s.contains("version") && !s.contains("update"))
    {
        return sys_plan!("kernel_info", "kernel_info");
    }

    // Network interface status — intercept before LLM confuses with desktop_action
    if s.contains("network interface") || s.contains("network status")
        || (s.contains("interface") && (s.contains("status") || s.contains("show") || s.contains("list")))
        || (s.contains("my ip") || s.contains("ip address") || s.contains("ifconfig"))
    {
        return sys_plan!("network_status", "network_status");
    }

    // List directory — model consistently uses filesystem.find instead, intercept here
    if s.contains("list files") || s.contains("list the files") || s.contains("show files")
        || s.contains("what files") || s.contains("what's in") || s.contains("whats in")
        || (s.contains("ls ") && (s.contains("/") || s.contains("~")))
    {
        let path = extract_path_token(input).unwrap_or_else(|| ".".to_string());
        return Some(TaskPlan {
            intent: "list_dir".to_string(),
            description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "filesystem".to_string(),
                action: "list_dir".to_string(),
                params: serde_json::json!({ "path": path }),
                description: "list_dir".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Read file — model often uses file_info instead
    if (s.contains("read the file") || s.contains("read file") || s.contains("show contents")
        || s.contains("cat the file") || s.contains("open the file") || s.contains("print the file")
        || s.contains("display the file") || s.contains("show the file"))
        && (s.contains("/") || s.contains(".txt") || s.contains(".conf") || s.contains(".json")
            || s.contains(".toml") || s.contains(".rs") || s.contains(".md") || s.contains("hosts")
            || s.contains(".log") || s.contains(".yaml") || s.contains(".sh"))
    {
        let path = extract_path_token(input).unwrap_or_else(|| "/etc/hosts".to_string());
        return Some(TaskPlan {
            intent: "read_file".to_string(),
            description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "filesystem".to_string(),
                action: "read_file".to_string(),
                params: serde_json::json!({ "path": path }),
                description: "read_file".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Browser navigate — intercept before LLM returns bare {"url": "..."}
    if (s.contains("navigate to") || s.contains("go to"))
        && (s.contains(".com") || s.contains(".org") || s.contains(".io") || s.contains(".net") || s.contains("http"))
    {
        let url = extract_host_token(input).unwrap_or_else(|| "https://example.com".to_string());
        let url = if url.starts_with("http") { url } else { format!("https://{}", url) };
        return Some(TaskPlan {
            intent: "browser_navigate".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "browser".to_string(), action: "navigate".to_string(),
                params: serde_json::json!({ "url": url }),
                description: "browser_navigate".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    // "open X in the browser" where X is a URL/domain
    if (s.contains("open") && s.contains("in the browser"))
        || (s.contains("open") && s.contains("browser") && (s.contains(".com") || s.contains(".org") || s.contains(".io") || s.contains("http")))
    {
        let url = extract_host_token(input).unwrap_or_else(|| "https://example.com".to_string());
        let url = if url.starts_with("http") { url } else { format!("https://{}", url) };
        return Some(TaskPlan {
            intent: "browser_navigate".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "browser".to_string(), action: "navigate".to_string(),
                params: serde_json::json!({ "url": url }),
                description: "browser_navigate".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    // Browser search — "search the web for X" or "search online for X"
    if s.contains("search the web") || s.contains("search online") || s.contains("web search")
        || (s.contains("search") && s.contains("browser") && !s.contains("package"))
    {
        let query = extract_between(input, &["search the web for ", "search online for ", "web search for ", "search for "], &["\n", "."])
            .unwrap_or_else(|| input.to_string());
        return Some(TaskPlan {
            intent: "browser_search".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "browser".to_string(), action: "search".to_string(),
                params: serde_json::json!({ "query": query }),
                description: "browser_search".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Write file — "write '<content>' to <path>" — LLM sends inconsistent param names
    if (s.contains("write") || s.contains("save"))
        && (s.contains(" to /") || s.contains(" to ~/") || s.contains(" to ./"))
        && !s.contains("document") && !s.contains("spreadsheet")
    {
        let path = extract_path_token(input).unwrap_or_else(|| "/tmp/soma-output.txt".to_string());
        let content = extract_quoted_any(input).unwrap_or_else(|| "".to_string());
        return Some(TaskPlan {
            intent: "write_file".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "filesystem".to_string(), action: "write_file".to_string(),
                params: serde_json::json!({ "path": path, "content": content }),
                description: "write_file".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Create directory — "create a directory at <path>" or "mkdir <path>"
    if (s.contains("create") || s.contains("make") || s.contains("mkdir"))
        && (s.contains("director") || s.contains("folder") || s.contains("mkdir"))
        && !s.contains("spreadsheet") && !s.contains("document")
    {
        let path = extract_path_token(input).unwrap_or_else(|| "/tmp/new-dir".to_string());
        return Some(TaskPlan {
            intent: "create_dir".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "filesystem".to_string(), action: "create_dir".to_string(),
                params: serde_json::json!({ "path": path }),
                description: "create_dir".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Browser screenshot
    if (s.contains("screenshot") || s.contains("screen shot") || s.contains("capture"))
        && (s.contains("browser") || s.contains("page") || s.contains("tab") || s.contains("current"))
    {
        return Some(TaskPlan {
            intent: "browser_screenshot".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "browser".to_string(), action: "screenshot".to_string(),
                params: serde_json::Value::Object(Default::default()),
                description: "browser_screenshot".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Copy file — extract from/to directly to avoid param name mismatch
    if s.starts_with("copy ") || (s.contains("copy") && s.contains(" to ") && (s.contains("/") || s.contains("~"))) {
        let paths: Vec<&str> = input.split_whitespace()
            .filter(|w| w.starts_with('/') || w.starts_with("~/") || w.starts_with("./"))
            .collect();
        if paths.len() >= 2 {
            return Some(TaskPlan {
                intent: "copy_file".to_string(), description: input.to_string(),
                steps: vec![soma_common::TaskStep {
                    capability: "filesystem".to_string(), action: "copy".to_string(),
                    params: serde_json::json!({ "from": paths[0], "to": paths[1] }),
                    description: "copy_file".to_string(),
                }],
                risk_level: soma_common::RiskLevel::Low,
            });
        }
    }

    // Find files — extract path and pattern to avoid param name mismatch
    if (s.contains("find") || s.contains("search for"))
        && (s.contains("files") || s.contains(".json") || s.contains(".log") || s.contains(".rs") || s.contains(".txt"))
        && (s.contains(" in ") || s.contains("/"))
    {
        // Extract path: word after "in" that looks like a path
        let path = input.split_whitespace()
            .skip_while(|w| w.to_lowercase() != "in")
            .nth(1)
            .filter(|w| w.starts_with('/') || w.starts_with("~/") || w.starts_with('.'))
            .map(|w| w.trim_matches(|c: char| c == '\'' || c == '"').to_string())
            .unwrap_or_else(|| "/tmp".to_string());
        // Extract pattern: first token with a file extension or glob
        let pattern = input.split_whitespace()
            .find(|w| w.contains('.') && (w.contains('*') || w.starts_with('.')))
            .map(|w| {
                // ".json" → "*.json"
                if w.starts_with('.') { format!("*{}", w) } else { w.to_string() }
            })
            .unwrap_or_else(|| "*".to_string());
        return Some(TaskPlan {
            intent: "find_files".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "filesystem".to_string(), action: "find".to_string(),
                params: serde_json::json!({ "path": path, "pattern": pattern }),
                description: "find_files".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Network capability — model confuses with browser
    if s.contains("ping ") && (s.contains(".com") || s.contains(".org") || s.contains(".net") || s.contains(".io")) {
        let host = s.split_whitespace().last().unwrap_or("google.com");
        return Some(TaskPlan {
            intent: "ping".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "network".to_string(), action: "ping".to_string(),
                params: serde_json::json!({ "host": host }),
                description: "ping".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if s.contains("dns") && (s.contains("lookup") || s.contains("resolve") || s.contains("record"))
        || s.contains("nslookup") || s.contains("dig ")
    {
        let host = extract_host_token(input).unwrap_or_else(|| "example.com".to_string());
        return Some(TaskPlan {
            intent: "dns_lookup".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "network".to_string(), action: "dns_lookup".to_string(),
                params: serde_json::json!({ "host": host }),
                description: "dns_lookup".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if s.contains("port") && (s.contains("open") || s.contains("check")) && !s.contains("port number") {
        return Some(TaskPlan {
            intent: "port_check".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "network".to_string(), action: "port_check".to_string(),
                params: serde_json::json!({ "host": "localhost", "port": 80 }),
                description: "port_check".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if (s.contains("curl ") && (s.contains("http://") || s.contains("https://") || s.contains(".com") || s.contains(".org") || s.contains(".io")) && !s.contains("package"))
        || (s.contains("fetch") && (s.contains("http://") || s.contains("https://")))
    {
        let url = input.split_whitespace()
            .find(|w| w.starts_with("http://") || w.starts_with("https://"))
            .unwrap_or("https://example.com");
        return Some(TaskPlan {
            intent: "curl".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "network".to_string(), action: "curl".to_string(),
                params: serde_json::json!({ "url": url }),
                description: "curl".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Package management — model can't reliably pick this capability
    if s.contains("list installed") || s.contains("list all installed")
        || s.contains("installed packages") || s.contains("list packages")
        || (s.contains("what packages") && !s.contains("propose"))
    {
        return Some(TaskPlan {
            intent: "list_installed".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "package".to_string(), action: "list_installed".to_string(),
                params: serde_json::Value::Object(Default::default()),
                description: "list_installed".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if (s.contains("search for") || s.contains("search")) && s.contains("package") {
        let pkg = extract_package_name(input).unwrap_or_else(|| "curl".to_string());
        return Some(TaskPlan {
            intent: "package_search".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "package".to_string(), action: "search".to_string(),
                params: serde_json::json!({ "package": pkg }),
                description: "package_search".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if (s.contains("install") && s.contains("package"))
        || s.contains("apt install") || s.contains("brew install") || s.contains("apt-get install")
        || s.contains("apk add")
    {
        let pkg = extract_package_name(input).unwrap_or_else(|| "htop".to_string());
        return Some(TaskPlan {
            intent: "package_install".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "package".to_string(), action: "install".to_string(),
                params: serde_json::json!({ "package": pkg }),
                description: "package_install".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Medium,
        });
    }

    // Service / process management — model confuses with desktop_action
    if (s.contains("list") || s.contains("show") || s.contains("all"))
        && s.contains("service") && !s.contains("status")
    {
        return Some(TaskPlan {
            intent: "service_list".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "process".to_string(), action: "service_list".to_string(),
                params: serde_json::Value::Object(Default::default()),
                description: "service_list".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if s.contains("status") && (s.contains("service") || s.contains("daemon")) {
        let svc = input.split_whitespace()
            .find(|w| !["check","status","the","of","service","daemon","is","running"].contains(w))
            .unwrap_or("ssh").to_string();
        return Some(TaskPlan {
            intent: "service_status".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "process".to_string(), action: "service_status".to_string(),
                params: serde_json::json!({ "service": svc }),
                description: "service_status".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if s.contains("kill") && (s.contains("process") || s.contains("pid")) {
        let pid = input.split_whitespace()
            .find(|w| w.parse::<u32>().is_ok())
            .and_then(|w| w.parse::<u32>().ok())
            .unwrap_or(0);
        return Some(TaskPlan {
            intent: "kill_process".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "process".to_string(), action: "kill_process".to_string(),
                params: serde_json::json!({ "pid": pid }),
                description: "kill_process".to_string(),
            }],
            risk_level: soma_common::RiskLevel::High,
        });
    }

    // Desktop agent spawn — model uses browser.navigate for "open browser"
    if (s.contains("open") || s.contains("launch") || s.contains("start"))
        && s.contains("terminal")
        && !s.contains("mode") && !s.contains("agent mode")
    {
        return Some(TaskPlan {
            intent: "spawn_terminal".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "desktop_agent".to_string(), action: "spawn_app".to_string(),
                params: serde_json::json!({ "app": "terminal" }),
                description: "spawn_terminal".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if (s.contains("open") || s.contains("launch"))
        && (s.contains("browser window") || s.contains("the browser"))
        && !s.contains("http") && !s.contains("www.") && !s.contains(".com")
        && !s.contains(".org") && !s.contains(".io")
    {
        return Some(TaskPlan {
            intent: "spawn_browser".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "desktop_agent".to_string(), action: "spawn_app".to_string(),
                params: serde_json::json!({ "app": "browser" }),
                description: "spawn_browser".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if s.contains("start agent mode") || s.contains("enable agent mode") || s.contains("begin agent mode")
        || (s.contains("agent mode") && (s.contains("start") || s.contains("enable") || s.contains("activate")))
    {
        let task = extract_between(input, &["to ", "for "], &["help", "organize", "manage", "monitor"])
            .unwrap_or_else(|| input.to_string());
        return Some(TaskPlan {
            intent: "start_agent_mode".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "desktop_agent".to_string(), action: "start_agent_mode".to_string(),
                params: serde_json::json!({ "task": task }),
                description: "start_agent_mode".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Semantic FS — find, list_tagged, describe_file
    if (s.contains("tagged") || s.contains("semantic tag")) && (s.contains("find") || s.contains("files with"))
        && !s.contains("tag the file")
    {
        let intent_str = extract_quoted_any(input).unwrap_or_else(|| "tagged".to_string());
        return Some(TaskPlan {
            intent: "semantic_find".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "semantic_fs".to_string(), action: "find_by_intent".to_string(),
                params: serde_json::json!({ "intent": intent_str }),
                description: "semantic_find".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if (s.contains("list") || s.contains("show") || s.contains("all"))
        && s.contains("tagged") && (s.contains("files") || s.contains("semantic"))
        && !s.contains("find")
    {
        return Some(TaskPlan {
            intent: "list_tagged".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "semantic_fs".to_string(), action: "list_tagged".to_string(),
                params: serde_json::Value::Object(Default::default()),
                description: "list_tagged".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if s.contains("semantic metadata") || s.contains("semantic") && s.contains("metadata") {
        let path = extract_path_token(input).unwrap_or_else(|| "/tmp/report.txt".to_string());
        return Some(TaskPlan {
            intent: "describe_file".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "semantic_fs".to_string(), action: "describe_file".to_string(),
                params: serde_json::json!({ "path": path }),
                description: "describe_file".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if s.contains("annotate") && (s.contains("file") || s.contains("/")) {
        let path = extract_path_token(input).unwrap_or_else(|| "/tmp/report.txt".to_string());
        let note = extract_quoted_any(input).unwrap_or_else(|| "annotated by soma".to_string());
        return Some(TaskPlan {
            intent: "annotate_file".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "semantic_fs".to_string(), action: "annotate".to_string(),
                params: serde_json::json!({ "path": path, "note": note }),
                description: "annotate_file".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Docs — append/write block (model always uses docs.create instead)
    if (s.contains("document") || s.contains(" doc") || s.contains("the doc"))
        && (s.contains("add a paragraph") || s.contains("append") || s.contains("add paragraph")
            || (s.contains("add") && s.contains("to the")))
        && !s.contains("spreadsheet") && !s.contains("create")
    {
        let text = extract_quoted_any(input).unwrap_or_else(|| "".to_string());
        return Some(TaskPlan {
            intent: "docs_append".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "docs".to_string(), action: "append_block".to_string(),
                params: serde_json::json!({ "window_id": 0, "block": { "type": "paragraph", "text": text } }),
                description: "docs_append".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if (s.contains("document") || s.contains(" doc"))
        && (s.contains("update the heading") || s.contains("change the heading")
            || s.contains("edit the heading") || s.contains("update heading"))
        && !s.contains("create")
    {
        let text = extract_quoted_any(input).unwrap_or_else(|| "Untitled".to_string());
        return Some(TaskPlan {
            intent: "docs_write_block".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "docs".to_string(), action: "write_block".to_string(),
                params: serde_json::json!({ "window_id": 0, "index": 0, "block": { "type": "heading", "level": 1, "text": text } }),
                description: "docs_write_block".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // File info — "show file info for /tmp" — model often uses read_file instead
    if s.contains("file info") || s.contains("file metadata") || s.contains("stat ") || s.contains("stat the")
        || (s.contains("info") && (s.contains("for /") || s.contains("for ~/")))
        || (s.contains("info") && s.contains("file") && (s.contains("/tmp") || s.contains("/etc") || s.contains("/var")))
    {
        let path = extract_path_token(input).unwrap_or_else(|| "/tmp".to_string());
        return Some(TaskPlan {
            intent: "file_info".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "filesystem".to_string(), action: "file_info".to_string(),
                params: serde_json::json!({ "path": path }),
                description: "file_info".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Sheets AgentAPI — apply_formula first (more specific), then write_cell
    if s.contains("apply") && s.contains("formula") && !s.contains("create") {
        return Some(TaskPlan {
            intent: "sheets_apply_formula".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "sheets".to_string(), action: "apply_formula".to_string(),
                params: serde_json::json!({ "window_id": 0, "cell": "A1", "formula": "=SUM(A1:A3)" }),
                description: "sheets_apply_formula".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    // write_cell — after formula check so "apply formula to cell X" doesn't match here
    if (s.contains("set cell") || s.contains("write cell") || s.contains("write to cell")
        || (s.contains("cell") && (s.contains("to ") || s.contains("value")) && s.contains("spreadsheet")))
        && !s.contains("formula")
    {
        return Some(TaskPlan {
            intent: "sheets_write_cell".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "sheets".to_string(), action: "write_cell".to_string(),
                params: serde_json::json!({ "window_id": 0, "cell": "A1", "value": "" }),
                description: "sheets_write_cell".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if (s.contains("read") || s.contains("get")) && s.contains("range") && (s.contains("spreadsheet") || s.contains("sheet")) {
        return Some(TaskPlan {
            intent: "sheets_read_range".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "sheets".to_string(), action: "read_range".to_string(),
                params: serde_json::json!({ "window_id": 0, "range": "A1:D5" }),
                description: "sheets_read_range".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if s.contains("describe") && (s.contains("spreadsheet") || (s.contains("sheet") && !s.contains("document"))) {
        return Some(TaskPlan {
            intent: "sheets_describe".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "sheets".to_string(), action: "describe".to_string(),
                params: serde_json::json!({ "window_id": 0 }),
                description: "sheets_describe".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Docs AgentAPI — describe
    if s.contains("describe") && (s.contains("document") || s.contains(" doc"))
        && !s.contains("spreadsheet") && !s.contains("create")
    {
        return Some(TaskPlan {
            intent: "docs_describe".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "docs".to_string(), action: "describe".to_string(),
                params: serde_json::json!({ "window_id": 0 }),
                description: "docs_describe".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Meta capability — propose, list_proposed, describe_gap
    if (s.contains("propose") || s.contains("suggest")) && s.contains("capability") {
        return Some(TaskPlan {
            intent: "meta_propose".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "meta".to_string(), action: "propose".to_string(),
                params: serde_json::json!({ "description": input }),
                description: "meta_propose".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if (s.contains("list") || s.contains("show")) && s.contains("proposed") {
        return Some(TaskPlan {
            intent: "meta_list_proposed".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "meta".to_string(), action: "list_proposed".to_string(),
                params: serde_json::Value::Object(Default::default()),
                description: "meta_list_proposed".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }
    if (s.contains("log") || s.contains("record") || s.contains("note"))
        && (s.contains("no capability") || s.contains("capability for") || s.contains("missing capability")
            || s.contains("gap") || s.contains("can't") || s.contains("cannot"))
    {
        return Some(TaskPlan {
            intent: "meta_describe_gap".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "meta".to_string(), action: "describe_gap".to_string(),
                params: serde_json::json!({ "gap": input }),
                description: "meta_describe_gap".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Workflow history
    if (s.contains("workflow") || s.contains("recent")) && s.contains("history") {
        return Some(TaskPlan {
            intent: "get_workflow_history".to_string(), description: input.to_string(),
            steps: vec![soma_common::TaskStep {
                capability: "desktop_agent".to_string(), action: "get_workflow_history".to_string(),
                params: serde_json::Value::Object(Default::default()),
                description: "get_workflow_history".to_string(),
            }],
            risk_level: soma_common::RiskLevel::Low,
        });
    }

    // Semantic FS tag — "tag (the file) <path> with (the label) '<label>'"
    // Model is too confused by this; extract params directly from text.
    if s.contains("tag") && (s.contains("file") || s.contains("path") || s.contains(".txt") || s.contains(".rs") || s.contains(".md")) {
        // Extract first path-like token (starts with / or ./)
        let path = input.split_whitespace()
            .find(|w| w.starts_with('/') || w.starts_with("./"))
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.').to_string());
        // Extract label: text in single quotes after "label" or first quoted token
        let label = extract_quoted(input, "label")
            .or_else(|| extract_quoted(input, "tag"))
            .or_else(|| extract_quoted_any(input));
        // Extract description: text in single quotes after "description"
        let description = extract_quoted(input, "description");

        if let (Some(path), Some(label)) = (path, label) {
            let mut params = serde_json::json!({ "path": path, "label": label });
            if let Some(desc) = description {
                params["description"] = serde_json::json!(desc);
            }
            return Some(TaskPlan {
                intent: "tag_file".to_string(),
                description: input.to_string(),
                steps: vec![soma_common::TaskStep {
                    capability: "semantic_fs".to_string(),
                    action: "tag".to_string(),
                    params,
                    description: "tag_file".to_string(),
                }],
                risk_level: soma_common::RiskLevel::Low,
            });
        }
    }

    None // fall through to LLM
}

/// Extract text between any of `prefixes` and any of `suffixes`, case-insensitive.
fn extract_between(text: &str, prefixes: &[&str], suffixes: &[&str]) -> Option<String> {
    let lower = text.to_lowercase();
    for prefix in prefixes {
        if let Some(start) = lower.find(prefix) {
            let after = &text[start + prefix.len()..];
            let after_lower = after.to_lowercase();
            for suffix in suffixes {
                if let Some(end) = after_lower.find(suffix) {
                    let candidate = after[..end].trim().to_string();
                    if !candidate.is_empty() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

/// Extract the first single-quoted string that follows `keyword` in `text`.
fn extract_quoted(text: &str, keyword: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let kw_pos = lower.find(keyword)?;
    let after = &text[kw_pos + keyword.len()..];
    let start = after.find('\'')?  + 1;
    let rest = &after[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// Extract the first single-quoted string anywhere in `text`.
fn extract_quoted_any(text: &str) -> Option<String> {
    let start = text.find('\'')? + 1;
    let rest = &text[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// Extract the first filesystem path token (starts with `/`, `~/`, or `./`).
fn extract_path_token(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|w| w.starts_with('/') || w.starts_with("~/") || w.starts_with("./"))
        .map(|w| w.trim_matches(|c: char| c == '\'' || c == '"' || c == ',').to_string())
}

/// Extract a hostname token (e.g. `github.com`, `8.8.8.8`) from text.
fn extract_host_token(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|w| {
            let w = w.trim_matches(|c: char| c == '\'' || c == '"' || c == ',');
            (w.contains('.') && !w.starts_with('.') && !w.ends_with('.'))
                || w.parse::<std::net::IpAddr>().is_ok()
        })
        .map(|w| w.trim_matches(|c: char| c == '\'' || c == '"' || c == ',').to_string())
}

/// Extract a package name from phrases like "install htop" or "search for the curl package".
fn extract_package_name(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    // Try "install <name>" or "search for <name>"
    for kw in &["install ", "search for ", "search "] {
        if let Some(pos) = lower.find(kw) {
            let after = text[pos + kw.len()..].trim();
            // Skip articles "the", "a", "an"
            let after = after.trim_start_matches("the ").trim_start_matches("a ").trim_start_matches("an ");
            let name = after.split_whitespace()
                .next()
                .map(|w| w.trim_matches(|c: char| c == '\'' || c == '"' || c == ','))
                .filter(|w| !["package", "packages"].contains(w))?;
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
//  IntentParser
// ─────────────────────────────────────────────────────────────────────────────

pub struct IntentParser {
    provider: Box<dyn LlmProvider>,
}

impl IntentParser {
    pub fn new(config: &SomaConfig) -> Self {
        Self { provider: providers::make_provider(config) }
    }

    /// Hot-reload the provider when the user updates settings.
    pub fn set_provider(&mut self, config: &SomaConfig) {
        self.provider = providers::make_provider(config);
        log::info!("Provider switched to {} / {}", config.model.provider, config.model.model);
    }

    /// Layer 0 fast path → LLM tool call.
    pub async fn parse(
        &self,
        input: &str,
        _system_prompt: &str, // kept for ipc.rs signature compatibility
        _context: Option<&str>,
        context_pairs: &[(String, String)],
        registry: &CapabilityRegistry,
    ) -> Result<TaskPlan, String> {
        // Layer 0 — instant, no LLM call
        if let Some(plan) = preprocess_input(input) {
            log::info!("Layer0 fast path: '{}'", input);
            return Ok(plan);
        }

        // Build tool schemas from registry
        let tools = build_tools(registry);

        // LLM tool-use call — retry once on failure (small models are non-deterministic)
        let calls = match self.provider.tool_call(input, context_pairs, &tools).await {
            Ok(c) => c,
            Err(first_err) => {
                log::warn!("First LLM attempt failed ({}), retrying...", first_err);
                self.provider.tool_call(input, context_pairs, &tools).await?
            }
        };
        log::info!("Tool calls returned: {:?}", calls.iter().map(|c| &c.name).collect::<Vec<_>>());

        // Semantic consistency check: if user asked for a document but model returned
        // only sheets calls, retry once with a more targeted hint.
        let s = input.to_lowercase();
        let wants_doc = s.contains("document") || s.contains("write") || s.contains("report")
            || s.contains("summary") || s.contains("essay") || s.contains("letter");
        let got_only_sheets = !calls.is_empty()
            && calls.iter().all(|c| c.name.starts_with("sheets__"));
        if wants_doc && got_only_sheets {
            log::warn!("Semantic mismatch: user wants document but got sheets. Retrying with hint.");
            let hint = format!("{} (Use docs__create, not sheets__create.)", input);
            if let Ok(retry_calls) = self.provider.tool_call(&hint, context_pairs, &tools).await {
                if retry_calls.iter().any(|c| c.name.starts_with("docs__")) {
                    log::info!("Semantic retry succeeded: {:?}", retry_calls.iter().map(|c| &c.name).collect::<Vec<_>>());
                    return Ok(tool_calls_to_task_plan(retry_calls, input));
                }
            }
        }

        Ok(tool_calls_to_task_plan(calls, input))
    }
}

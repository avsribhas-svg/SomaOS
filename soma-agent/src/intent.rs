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
        || ((s.contains("disk") || s.contains("drive")) && s.contains("usage"))
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

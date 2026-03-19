//! Ollama provider — uses /api/chat with native tool use.
//!
//! Requires a model that supports function calling:
//! llama3.1, qwen2.5, mistral-nemo, etc.

use reqwest::Client;
use serde_json::Value;

use super::{LlmProvider, ToolCall, ToolDef};

pub struct OllamaProvider {
    client: Client,
    model: String,
    base_url: String,
}

impl OllamaProvider {
    pub fn new(model: String, base_url: String) -> Self {
        let base_url = if base_url.is_empty() {
            "http://localhost:11434".to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        };
        Self { client: Client::new(), model, base_url }
    }

    fn chat_url(&self) -> String {
        format!("{}/api/chat", self.base_url)
    }
}

#[async_trait::async_trait]
impl LlmProvider for OllamaProvider {
    async fn tool_call(
        &self,
        user_message: &str,
        context: &[(String, String)],
        tools: &[ToolDef],
    ) -> Result<Vec<ToolCall>, String> {
        // Build messages array
        let mut messages: Vec<Value> = Vec::new();

        // System prompt
        messages.push(serde_json::json!({
            "role": "system",
            "content": super::SYSTEM_PROMPT,
        }));

        // Conversation context
        for (user, agent) in context {
            messages.push(serde_json::json!({ "role": "user",      "content": user  }));
            messages.push(serde_json::json!({ "role": "assistant", "content": agent }));
        }

        // Current request
        messages.push(serde_json::json!({ "role": "user", "content": user_message }));

        // Build tools array in Ollama format
        let tools_json: Vec<Value> = tools.iter().map(|t| serde_json::json!({
            "type": "function",
            "function": {
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters,
            }
        })).collect();

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "tools": tools_json,
            "stream": false,
        });

        let resp = self.client
            .post(&self.chat_url())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Ollama request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Ollama returned {}: {}", status, text));
        }

        let data: Value = resp.json().await
            .map_err(|e| format!("Failed to parse Ollama response: {}", e))?;

        parse_tool_calls(&data)
    }
}

fn parse_tool_calls(data: &Value) -> Result<Vec<ToolCall>, String> {
    let tool_calls = data
        .get("message")
        .and_then(|m| m.get("tool_calls"))
        .and_then(|tc| tc.as_array());

    if let Some(calls) = tool_calls {
        if !calls.is_empty() {
            return parse_native_tool_calls(calls);
        }
    }

    // Fallback: model replied with plain text — try to parse it as a JSON tool call.
    // qwen2.5-coder:7b often ignores the native tool-call API and replies with
    // raw JSON like {"name": "capability__action", "params": {...}} or an array thereof.
    let text = data
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    if let Some(calls) = try_parse_text_as_tool_calls(text) {
        if !calls.is_empty() {
            log::info!("Ollama text-fallback: parsed {} tool call(s) from plain text", calls.len());
            return Ok(calls);
        }
    }

    Err(format!(
        "Model did not use tools. Try a model with tool support (llama3.1, qwen2.5, mistral-nemo). Reply: {}",
        &text[..text.len().min(200)]
    ))
}

fn parse_native_tool_calls(calls: &[Value]) -> Result<Vec<ToolCall>, String> {
    let mut result = Vec::new();
    for call in calls {
        let name = call
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            .ok_or("Missing tool call name")?
            .to_string();

        let arguments = call
            .get("function")
            .and_then(|f| f.get("arguments"))
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        // Ollama may return arguments as a JSON string — parse if so
        let arguments = match arguments {
            Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::Object(Default::default())),
            other => other,
        };

        result.push(ToolCall { name: normalize_tool_name(&name), arguments });
    }
    if result.is_empty() {
        return Err("Model returned empty tool_calls array".to_string());
    }
    Ok(result)
}

/// Try to extract tool calls from a plain-text reply.
/// Handles the common patterns qwen2.5-coder:7b uses:
///   {"name": "cap__action", "params": {...}}
///   {"name": "cap::action", "arguments": {...}}
///   [{"name": ..., "params": ...}, ...]
///   {"functions": [{"name": ..., "parameters": {...}}]}   ← model wraps in "functions" key
fn try_parse_text_as_tool_calls(text: &str) -> Option<Vec<ToolCall>> {
    // Find the first '[' or '{' to locate the JSON blob
    let start = text.find(|c: char| c == '[' || c == '{')?;
    let json_str = &text[start..];

    // Try to parse as array first, then as single object
    let parsed: Value = if json_str.starts_with('[') {
        serde_json::from_str(json_str)
            .or_else(|_| serde_json::from_str(&trim_to_balanced(json_str, '[', ']')))
            .ok()?
    } else {
        serde_json::from_str(json_str)
            .or_else(|_| serde_json::from_str(&trim_to_balanced(json_str, '{', '}')))
            .ok()?
    };

    // Unwrap {"functions": [...]} or {"tools": [...]} wrapper patterns
    let parsed = match &parsed {
        Value::Object(map) => {
            if let Some(inner) = map.get("functions").or_else(|| map.get("tools")) {
                if inner.is_array() {
                    inner.clone()
                } else {
                    parsed.clone()
                }
            } else {
                parsed.clone()
            }
        }
        _ => parsed.clone(),
    };

    let objects: Vec<Value> = match parsed {
        Value::Array(arr) => arr,
        obj @ Value::Object(_) => vec![obj],
        _ => return None,
    };

    let mut calls = Vec::new();
    for obj in &objects {
        // Bare params inference: model returned just the args without a tool name.
        // Detect by recognizable top-level keys.
        if obj.get("name").is_none() {
            // {"url": "..."} bare → browser__navigate
            if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
                calls.push(ToolCall {
                    name: "browser__navigate".to_string(),
                    arguments: serde_json::json!({ "url": url }),
                });
                continue;
            }
            // {"query": "..."} bare → browser__search (most common bare-query intent)
            if let Some(query) = obj.get("query").and_then(|v| v.as_str()) {
                calls.push(ToolCall {
                    name: "browser__search".to_string(),
                    arguments: serde_json::json!({ "query": query }),
                });
                continue;
            }
            // {"gap": "..."} bare → meta__describe_gap
            if let Some(gap) = obj.get("gap").and_then(|v| v.as_str()) {
                calls.push(ToolCall {
                    name: "meta__describe_gap".to_string(),
                    arguments: serde_json::json!({ "gap": gap }),
                });
                continue;
            }
            // {"package": "..."} bare → package__install (install is the more common action)
            if let Some(pkg) = obj.get("package").and_then(|v| v.as_str()) {
                calls.push(ToolCall {
                    name: "package__install".to_string(),
                    arguments: serde_json::json!({ "package": pkg }),
                });
                continue;
            }
            // {"function": "cap__action", "args": {...}} — model uses "function" as key instead of "name"
            if let Some(fn_name) = obj.get("function").and_then(|v| v.as_str()) {
                let arguments = obj.get("args")
                    .or_else(|| obj.get("arguments"))
                    .or_else(|| obj.get("params"))
                    .cloned()
                    .unwrap_or(Value::Object(Default::default()));
                let name = normalize_tool_name(fn_name);
                if name.contains("__") {
                    calls.push(ToolCall { name, arguments });
                    continue;
                }
            }
            if let Some(cells) = obj.get("cells") {
                // cells JSON string → parse it first if needed
                let cells = match cells {
                    Value::String(s) => serde_json::from_str(s).unwrap_or(cells.clone()),
                    other => other.clone(),
                };
                calls.push(ToolCall {
                    name: "sheets__create".to_string(),
                    arguments: serde_json::json!({ "title": "Sheet 1", "cells": cells }),
                });
                continue;
            }
            if let Some(blocks) = obj.get("blocks") {
                if blocks.is_array() {
                    calls.push(ToolCall {
                        name: "docs__create".to_string(),
                        arguments: serde_json::json!({ "title": "Untitled Document", "blocks": blocks }),
                    });
                    continue;
                }
            }
            continue; // no name and no recognizable bare params — skip
        }
        let name = match obj.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => continue,
        };
        // Accept capability__action or capability::action
        let name = normalize_tool_name(name);
        // Must look like a known namespace (contain __)
        if !name.contains("__") {
            continue; // skip invented names, don't abort the whole parse
        }
        // Arguments can be under "params", "parameters", "arguments", "args", or "kwargs"
        let arguments = obj.get("params")
            .or_else(|| obj.get("parameters"))
            .or_else(|| obj.get("arguments"))
            .or_else(|| obj.get("args"))
            .or_else(|| obj.get("kwargs"))
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        calls.push(ToolCall { name, arguments });
    }

    if calls.is_empty() { None } else { Some(calls) }
}

/// Normalize separator: capability::action → capability__action
fn normalize_tool_name(name: &str) -> String {
    name.replace("::", "__")
}

/// Trim a JSON string to its outermost balanced bracket pair.
/// Handles truncated responses from the model.
fn trim_to_balanced(s: &str, open: char, close: char) -> String {
    let mut depth = 0i32;
    let mut end = s.len();
    for (i, c) in s.char_indices() {
        if c == open  { depth += 1; }
        if c == close { depth -= 1; if depth == 0 { end = i + 1; break; } }
    }
    s[..end].to_string()
}

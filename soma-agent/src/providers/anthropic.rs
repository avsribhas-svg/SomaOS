//! Anthropic Claude provider — uses /v1/messages with native tool use.

use reqwest::Client;
use serde_json::Value;

use super::{LlmProvider, ToolCall, ToolDef};

pub struct AnthropicProvider {
    client: Client,
    model: String,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(model: String, api_key: String, base_url: String) -> Self {
        let base_url = if base_url.is_empty() || base_url == "http://localhost:11434" {
            "https://api.anthropic.com".to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        };
        let model = if model.is_empty() {
            "claude-haiku-4-5-20251001".to_string()
        } else {
            model
        };
        Self { client: Client::new(), model, api_key, base_url }
    }

    fn messages_url(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicProvider {
    async fn tool_call(
        &self,
        user_message: &str,
        context: &[(String, String)],
        tools: &[ToolDef],
    ) -> Result<Vec<ToolCall>, String> {
        if self.api_key.is_empty() {
            return Err("Anthropic API key is not set. Add it in the Settings tab.".to_string());
        }

        // Build messages array
        let mut messages: Vec<Value> = Vec::new();
        for (user, agent) in context {
            messages.push(serde_json::json!({ "role": "user",      "content": user  }));
            messages.push(serde_json::json!({ "role": "assistant", "content": agent }));
        }
        messages.push(serde_json::json!({ "role": "user", "content": user_message }));

        // Build tools array in Anthropic format
        let tools_json: Vec<Value> = tools.iter().map(|t| serde_json::json!({
            "name": t.name,
            "description": t.description,
            "input_schema": t.parameters,
        })).collect();

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "system": super::SYSTEM_PROMPT,
            "tools": tools_json,
            "messages": messages,
        });

        let resp = self.client
            .post(&self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Anthropic request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Anthropic returned {}: {}", status, text));
        }

        let data: Value = resp.json().await
            .map_err(|e| format!("Failed to parse Anthropic response: {}", e))?;

        parse_tool_calls(&data)
    }
}

fn parse_tool_calls(data: &Value) -> Result<Vec<ToolCall>, String> {
    let content = data
        .get("content")
        .and_then(|c| c.as_array())
        .ok_or("Missing content array in Anthropic response")?;

    let calls: Vec<ToolCall> = content
        .iter()
        .filter(|block| block.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
        .filter_map(|block| {
            let name = block.get("name")?.as_str()?.to_string();
            let arguments = block.get("input").cloned().unwrap_or(Value::Object(Default::default()));
            Some(ToolCall { name, arguments })
        })
        .collect();

    if calls.is_empty() {
        // Extract text reply for the error message
        let text = content.iter()
            .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
            .and_then(|b| b.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("no tool calls in response");
        return Err(format!("Claude did not use tools: {}", &text[..text.len().min(300)]));
    }

    Ok(calls)
}

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
            "content": "You are an AI agent that controls a computer. Use the provided tools to complete the user's request. Always use tools — never reply with plain text."
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

    let Some(calls) = tool_calls else {
        // Model replied without tool calls — extract text and return error
        let text = data
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("no tool calls returned");
        return Err(format!(
            "Model did not use tools. Try a model with tool support (llama3.1, qwen2.5, mistral-nemo). Reply: {}",
            &text[..text.len().min(200)]
        ));
    };

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

        result.push(ToolCall { name, arguments });
    }

    if result.is_empty() {
        return Err("Model returned empty tool_calls array".to_string());
    }

    Ok(result)
}

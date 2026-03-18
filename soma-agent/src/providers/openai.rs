//! OpenAI provider — uses /v1/chat/completions with function calling.
//! Also handles any OpenAI-compatible endpoint (Groq, Together, local vLLM, etc.)

use reqwest::Client;
use serde_json::Value;

use super::{LlmProvider, ToolCall, ToolDef};

pub struct OpenAiProvider {
    client: Client,
    model: String,
    api_key: String,
    base_url: String,
}

impl OpenAiProvider {
    pub fn new(model: String, api_key: String, base_url: String) -> Self {
        let base_url = if base_url.is_empty() {
            "https://api.openai.com".to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        };
        let model = if model.is_empty() {
            "gpt-4o-mini".to_string()
        } else {
            model
        };
        Self { client: Client::new(), model, api_key, base_url }
    }

    fn chat_url(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiProvider {
    async fn tool_call(
        &self,
        user_message: &str,
        context: &[(String, String)],
        tools: &[ToolDef],
    ) -> Result<Vec<ToolCall>, String> {
        if self.api_key.is_empty() {
            return Err("OpenAI API key is not set. Add it in the Settings tab.".to_string());
        }

        // Build messages array
        let mut messages: Vec<Value> = vec![serde_json::json!({
            "role": "system",
            "content": super::SYSTEM_PROMPT
        })];

        for (user, agent) in context {
            messages.push(serde_json::json!({ "role": "user",      "content": user  }));
            messages.push(serde_json::json!({ "role": "assistant", "content": agent }));
        }
        messages.push(serde_json::json!({ "role": "user", "content": user_message }));

        // Build tools array in OpenAI format
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
            "tool_choice": "required",
        });

        let mut req = self.client.post(&self.chat_url()).json(&body);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp = req.send().await
            .map_err(|e| format!("OpenAI request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("OpenAI returned {}: {}", status, text));
        }

        let data: Value = resp.json().await
            .map_err(|e| format!("Failed to parse OpenAI response: {}", e))?;

        parse_tool_calls(&data)
    }
}

fn parse_tool_calls(data: &Value) -> Result<Vec<ToolCall>, String> {
    let tool_calls = data
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("tool_calls"))
        .and_then(|tc| tc.as_array());

    let Some(calls) = tool_calls else {
        let text = data
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("no tool calls returned");
        return Err(format!("OpenAI model did not use tools: {}", &text[..text.len().min(300)]));
    };

    let mut result = Vec::new();
    for call in calls {
        let name = call
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            .ok_or("Missing tool call name")?
            .to_string();

        let arguments_raw = call
            .get("function")
            .and_then(|f| f.get("arguments"))
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        // OpenAI returns arguments as a JSON string
        let arguments = match arguments_raw {
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

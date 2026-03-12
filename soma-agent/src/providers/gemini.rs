//! Google Gemini provider — uses generateContent with function_declarations.

use reqwest::Client;
use serde_json::Value;

use super::{LlmProvider, ToolCall, ToolDef};

pub struct GeminiProvider {
    client: Client,
    model: String,
    api_key: String,
}

impl GeminiProvider {
    pub fn new(model: String, api_key: String) -> Self {
        let model = if model.is_empty() {
            "gemini-2.0-flash".to_string()
        } else {
            model
        };
        Self { client: Client::new(), model, api_key }
    }

    fn generate_url(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        )
    }
}

#[async_trait::async_trait]
impl LlmProvider for GeminiProvider {
    async fn tool_call(
        &self,
        user_message: &str,
        context: &[(String, String)],
        tools: &[ToolDef],
    ) -> Result<Vec<ToolCall>, String> {
        if self.api_key.is_empty() {
            return Err("Gemini API key is not set. Add it in the Settings tab.".to_string());
        }

        // Build contents array
        let mut contents: Vec<Value> = Vec::new();
        for (user, agent) in context {
            contents.push(serde_json::json!({ "role": "user",  "parts": [{ "text": user  }] }));
            contents.push(serde_json::json!({ "role": "model", "parts": [{ "text": agent }] }));
        }
        contents.push(serde_json::json!({ "role": "user", "parts": [{ "text": user_message }] }));

        // Build function_declarations — Gemini uses a slightly different schema
        let function_declarations: Vec<Value> = tools.iter().map(|t| serde_json::json!({
            "name": t.name,
            "description": t.description,
            "parameters": t.parameters,
        })).collect();

        let body = serde_json::json!({
            "system_instruction": {
                "parts": [{ "text": "You are an AI agent that controls a computer. Use the provided tools to complete the user's request. Always use tools — never reply with plain text." }]
            },
            "contents": contents,
            "tools": [{ "function_declarations": function_declarations }],
            "tool_config": { "function_calling_config": { "mode": "ANY" } },
        });

        let resp = self.client
            .post(&self.generate_url())
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Gemini request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Gemini returned {}: {}", status, text));
        }

        let data: Value = resp.json().await
            .map_err(|e| format!("Failed to parse Gemini response: {}", e))?;

        parse_tool_calls(&data)
    }
}

fn parse_tool_calls(data: &Value) -> Result<Vec<ToolCall>, String> {
    let parts = data
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array());

    let Some(parts) = parts else {
        return Err(format!("Unexpected Gemini response structure: {}", data));
    };

    let calls: Vec<ToolCall> = parts
        .iter()
        .filter_map(|part| {
            let fc = part.get("functionCall")?;
            let name = fc.get("name")?.as_str()?.to_string();
            let arguments = fc.get("args").cloned().unwrap_or(Value::Object(Default::default()));
            Some(ToolCall { name, arguments })
        })
        .collect();

    if calls.is_empty() {
        let text = parts.iter()
            .find_map(|p| p.get("text")?.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "no function calls in response".to_string());
        return Err(format!("Gemini did not use tools: {}", &text[..text.len().min(300)]));
    }

    Ok(calls)
}

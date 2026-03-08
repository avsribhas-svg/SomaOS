use reqwest::Client;
use serde::{Deserialize, Serialize};
use soma_common::{TaskPlan, DEFAULT_MODEL, OLLAMA_URL};

use crate::capabilities::CapabilityRegistry;

/// Build the system prompt dynamically from registered capabilities
pub fn build_system_prompt(registry: &CapabilityRegistry) -> String {
    let capabilities_schema = registry.schema_for_prompt();

    format!(
        r#"You are an AI assistant that converts natural language instructions into structured task plans.
You MUST respond with ONLY valid JSON — no preamble, no markdown, no explanation.

The JSON must follow this exact schema:
{{
  "intent": "string — a short snake_case identifier for the action",
  "description": "string — a human-readable summary of what will happen",
  "steps": [
    {{
      "capability": "string — the capability name",
      "action": "string — the action name within that capability",
      "params": {{ "key": "value" }},
      "description": "string — what this step does"
    }}
  ],
  "risk_level": "low | medium | high"
}}

Available capabilities and their actions:

{capabilities_schema}

Rules:
- ONLY use the capabilities and actions listed above
- Delete actions are always "high" risk
- Write/create/move/kill/restart actions are "medium" risk
- Read-only actions (list, read, status, info) are "low" risk
- If the user input cannot be mapped to available capabilities, return:
  {{"intent": "unsupported", "description": "Cannot perform this action with available capabilities", "steps": [], "risk_level": "low"}}
- Respond with ONLY the JSON object. No other text."#
    )
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    system: String,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

pub struct IntentParser {
    client: Client,
    model: String,
}

impl IntentParser {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            model: DEFAULT_MODEL.to_string(),
        }
    }

    pub async fn parse(&self, input: &str, system_prompt: &str) -> Result<TaskPlan, String> {
        let request = OllamaRequest {
            model: self.model.clone(),
            prompt: input.to_string(),
            system: system_prompt.to_string(),
            stream: false,
        };

        let response = self
            .client
            .post(OLLAMA_URL)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Failed to reach Ollama at {}: {}", OLLAMA_URL, e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Ollama returned status {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }

        let ollama_resp: OllamaResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Ollama response: {}", e))?;

        let raw = ollama_resp.response.trim();
        let json_str = extract_json(raw);

        serde_json::from_str::<TaskPlan>(&json_str).map_err(|e| {
            format!("Failed to parse TaskPlan: {}. Raw: {}", e, raw)
        })
    }
}

/// Extract a JSON object from text that might contain markdown fences or wrapper text
fn extract_json(text: &str) -> String {
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }
    text.to_string()
}

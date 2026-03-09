use reqwest::Client;
use serde::{Deserialize, Serialize};
use soma_common::{TaskPlan, DEFAULT_MODEL, OLLAMA_URL};

use crate::capabilities::CapabilityRegistry;

/// Build the system prompt dynamically from registered capabilities
pub fn build_system_prompt(registry: &CapabilityRegistry) -> String {
    let capabilities_schema = registry.schema_for_prompt();

    format!(
        r#"You convert natural language into JSON task plans. Respond with ONLY valid JSON.

Schema:
{{"intent":"string","description":"string","steps":[{{"capability":"string","action":"string","params":{{}},"description":"string"}}],"risk_level":"low"}}

Available capabilities:
{capabilities_schema}
Rules:
- Delete/kill = "high" risk. Write/create/move/restart = "medium" risk. Read-only = "low" risk.
- If unmappable: {{"intent":"unsupported","description":"Cannot perform this","steps":[],"risk_level":"low"}}

Examples:
Input: "list files in /home"
Output: {{"intent":"list_directory","description":"List files in /home","steps":[{{"capability":"filesystem","action":"list_dir","params":{{"path":"/home"}},"description":"List directory contents of /home"}}],"risk_level":"low"}}

Input: "what is the hostname"
Output: {{"intent":"get_hostname","description":"Get system hostname","steps":[{{"capability":"system","action":"hostname","params":{{}},"description":"Get the system hostname"}}],"risk_level":"low"}}

Input: "show running processes"
Output: {{"intent":"list_processes","description":"List running processes","steps":[{{"capability":"process","action":"list_processes","params":{{}},"description":"List all running processes"}}],"risk_level":"low"}}

Input: "delete /tmp/test.txt"
Output: {{"intent":"delete_file","description":"Delete /tmp/test.txt","steps":[{{"capability":"filesystem","action":"delete","params":{{"path":"/tmp/test.txt"}},"description":"Delete the file /tmp/test.txt"}}],"risk_level":"high"}}

RESPOND WITH ONLY THE JSON OBJECT. NO OTHER TEXT."#
    )
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    system: String,
    stream: bool,
    format: String,
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
            format: "json".to_string(),
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

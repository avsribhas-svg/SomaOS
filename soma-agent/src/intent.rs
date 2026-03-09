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
- Delete/kill/remove package = "high" risk. Write/create/move/restart/install = "medium" risk. Read-only = "low" risk.
- If unmappable: {{"intent":"unsupported","description":"Cannot perform this","steps":[],"risk_level":"low"}}
- You may create multi-step plans with multiple steps when needed.
- If conversation context is provided, use it to resolve references like "same", "that", "again", etc.

Examples:
Input: "list files in /home"
Output: {{"intent":"list_directory","description":"List files in /home","steps":[{{"capability":"filesystem","action":"list_dir","params":{{"path":"/home"}},"description":"List directory contents of /home"}}],"risk_level":"low"}}

Input: "what is the hostname"
Output: {{"intent":"get_hostname","description":"Get system hostname","steps":[{{"capability":"system","action":"hostname","params":{{}},"description":"Get the system hostname"}}],"risk_level":"low"}}

Input: "show running processes"
Output: {{"intent":"list_processes","description":"List running processes","steps":[{{"capability":"process","action":"list_processes","params":{{}},"description":"List all running processes"}}],"risk_level":"low"}}

Input: "delete /tmp/test.txt"
Output: {{"intent":"delete_file","description":"Delete /tmp/test.txt","steps":[{{"capability":"filesystem","action":"delete","params":{{"path":"/tmp/test.txt"}},"description":"Delete the file /tmp/test.txt"}}],"risk_level":"high"}}

Input: "ping google.com"
Output: {{"intent":"ping_host","description":"Ping google.com","steps":[{{"capability":"network","action":"ping","params":{{"host":"google.com","count":4}},"description":"Ping google.com"}}],"risk_level":"low"}}

Input: "what are my network interfaces"
Output: {{"intent":"list_interfaces","description":"List network interfaces","steps":[{{"capability":"network","action":"ifconfig","params":{{}},"description":"List all network interfaces"}}],"risk_level":"low"}}

Input: "list installed packages"
Output: {{"intent":"list_packages","description":"List installed packages","steps":[{{"capability":"package","action":"list_installed","params":{{}},"description":"List all installed packages"}}],"risk_level":"low"}}

Input: "check disk usage"
Output: {{"intent":"check_disk","description":"Check disk usage","steps":[{{"capability":"system","action":"disk_usage","params":{{}},"description":"Check current disk usage"}}],"risk_level":"low"}}

Input: "show memory info"
Output: {{"intent":"memory_info","description":"Show system memory info","steps":[{{"capability":"system","action":"memory_info","params":{{}},"description":"Show memory usage"}}],"risk_level":"low"}}

Input: "find all log files in /var and check disk usage"
Output: {{"intent":"inspect_system","description":"Find log files and check disk usage","steps":[{{"capability":"filesystem","action":"find","params":{{"path":"/var","pattern":"*.log"}},"description":"Find .log files in /var"}},{{"capability":"system","action":"disk_usage","params":{{}},"description":"Check disk usage"}}],"risk_level":"low"}}

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

    /// Parse with optional conversation context
    pub async fn parse(
        &self,
        input: &str,
        system_prompt: &str,
        context: Option<&str>,
    ) -> Result<TaskPlan, String> {
        // Build prompt with optional context
        let prompt = if let Some(ctx) = context {
            format!(
                "Recent conversation:\n{}\n\nCurrent request: {}",
                ctx, input
            )
        } else {
            input.to_string()
        };

        let request = OllamaRequest {
            model: self.model.clone(),
            prompt,
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

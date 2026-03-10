use reqwest::Client;
use serde::{Deserialize, Serialize};
use soma_common::{TaskPlan, DEFAULT_MODEL, OLLAMA_URL};

use crate::capabilities::CapabilityRegistry;

// ─────────────────────────────────────────────────────────────────────────────
//  Layer 0: Deterministic keyword pre-processor
//  Instantly canonicalises common colloquial phrasings before any LLM call.
//  Returns Some(canonical) when confident, None to fall through to the LLM.
// ─────────────────────────────────────────────────────────────────────────────

fn preprocess_input(input: &str) -> Option<String> {
    let s = input.to_lowercase();

    // ── Disk space ───────────────────────────────────────────────────────────
    // "disk image" without a real image extension → the user means disk usage
    let is_real_image = s.contains(".img") || s.contains(".iso") || s.contains(".dmg") || s.contains(".vhd");
    if s.contains("disk image") && !is_real_image {
        return Some("check disk usage".into());
    }
    if s.contains("disk space") || s.contains("free space") || s.contains("storage space")
        || s.contains("disk full") || s.contains("how much room")
    {
        return Some("check disk usage".into());
    }
    if (s.contains("how full") || s.contains("how much") ) && (s.contains("disk") || s.contains("drive") || s.contains("storage")) {
        return Some("check disk usage".into());
    }
    if (s.contains("disk") || s.contains("drive")) && s.contains("usage") {
        return Some("check disk usage".into());
    }

    // ── Memory ───────────────────────────────────────────────────────────────
    let memory_kw = s.contains("ram") || s.contains("memory") || s.contains("mem ");
    let query_kw  = s.contains("usage") || s.contains("info") || s.contains("eating")
        || s.contains("how much") || s.contains("check") || s.contains("show")
        || s.contains("status") || s.contains("free");
    if memory_kw && query_kw {
        return Some("show memory usage".into());
    }

    // ── Uptime ───────────────────────────────────────────────────────────────
    if s.contains("uptime") || s.contains("how long") && s.contains("running")
        || s.contains("since boot") || s.contains("boot time")
    {
        return Some("show system uptime".into());
    }

    // ── Hostname ─────────────────────────────────────────────────────────────
    if s.contains("hostname") || s.contains("computer name") || s.contains("machine name")
        || s.contains("server name") || (s.contains("what") && s.contains("this machine"))
    {
        return Some("what is the hostname".into());
    }

    // ── Processes ────────────────────────────────────────────────────────────
    if s.contains("background task") || s.contains("running task") || s.contains("running process")
        || (s.contains("cpu") && s.contains("eating"))
        || (s.contains("what") && s.contains("running"))
        || s.contains("active app") || s.contains("active process")
    {
        return Some("list running processes".into());
    }

    // ── Network ──────────────────────────────────────────────────────────────
    if s.contains("my ip") || s.contains("ip address") || s.contains("what is my ip")
        || (s.contains("network") && (s.contains("interface") || s.contains("config") || s.contains("info")))
    {
        return Some("list network interfaces".into());
    }

    // ── Connectivity / ping ───────────────────────────────────────────────────
    if s.contains("can i reach") || s.contains("can you reach") || s.contains("is reachable")
        || (s.contains("check") && s.contains("connection"))
        || (s.contains("test") && s.contains("connect"))
    {
        // Extract a hostname-like token after common trigger words
        let target = extract_target(&s, &["reach", "ping", "connect to", "check connection to"]);
        let host = target.unwrap_or_else(|| "8.8.8.8".into());
        return Some(format!("ping {host}"));
    }

    None // fall through to LLM
}

/// Pull the first token that looks like a hostname/IP after a trigger word.
fn extract_target(s: &str, triggers: &[&str]) -> Option<String> {
    for trigger in triggers {
        if let Some(pos) = s.find(trigger) {
            let after = s[pos + trigger.len()..].trim();
            let token = after.split_whitespace().next().unwrap_or("").trim_end_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '-');
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
//  Phase 1: Free-text intent interpretation
//  The LLM reads colloquial user input and rewrites it as a clear, canonical
//  description that Phase 2 (JSON planner) can reliably map to a capability.
// ─────────────────────────────────────────────────────────────────────────────

const INTERPRET_SYSTEM_PROMPT: &str = r#"You are an intent interpreter for a system management agent.
Your job is to translate vague, colloquial, or indirect user requests into a clear, specific description
of what system action to take.

Available system actions and their aliases:
- CHECK DISK SPACE: "disk image", "how much space", "storage", "disk full", "free space", "how much room", "disk usage", "check disk"
- CHECK MEMORY / RAM: "memory", "RAM", "how much memory", "memory usage", "RAM usage"
- SYSTEM UPTIME: "uptime", "how long running", "since when", "boot time"
- GET HOSTNAME: "hostname", "computer name", "machine name", "what is this machine", "server name"
- LIST RUNNING PROCESSES: "running processes", "background tasks", "what's running", "active apps", "processes", "what processes", "CPU usage", "tasks"
- KILL A PROCESS: "kill", "stop process", "end task", "terminate", "close app"
- LIST FILES IN DIRECTORY: "list files", "browse", "what's in", "show folder", "directory contents", "ls", "show files", "what files"
- READ FILE CONTENTS: "read file", "show file", "view file", "cat", "open file", "print file", "file contents", "what's in the file"
- WRITE / CREATE FILE: "write file", "create file", "save content", "make file", "new file"
- DELETE FILE OR FOLDER: "delete", "remove file", "trash", "rm", "wipe"
- FIND FILES MATCHING PATTERN: "find file", "search for file", "locate file", "glob", "search files"
- FILE METADATA / INFO: "file info", "file size", "permissions", "metadata", "when was modified"
- COPY FILE: "copy file", "duplicate", "cp"
- MOVE / RENAME FILE: "move file", "rename file", "mv"
- CREATE DIRECTORY: "create folder", "make directory", "mkdir", "new folder"
- LIST NETWORK INTERFACES: "network interfaces", "IP address", "what's my IP", "network config", "ifconfig", "ip addr", "network info"
- PING HOST: "ping", "check connection", "test connectivity", "can I reach", "is reachable", "latency to"
- DNS LOOKUP: "DNS lookup", "resolve hostname", "what IP is", "nslookup", "dig"
- CHECK IF PORT IS OPEN: "port check", "is port open", "check service", "is running on port"
- DOWNLOAD URL / HTTP REQUEST: "download", "fetch URL", "curl", "HTTP request", "get URL", "wget"
- LIST INSTALLED PACKAGES: "installed packages", "installed software", "what's installed", "list packages", "show packages"
- INSTALL PACKAGE: "install package", "add software", "apt install", "brew install", "get package"
- REMOVE / UNINSTALL PACKAGE: "remove package", "uninstall", "delete software", "purge"
- UPDATE PACKAGES: "update packages", "upgrade software", "apt update", "brew update"
- SEARCH FOR PACKAGE: "search package", "find package", "is package available"

Rules:
- ALWAYS pick the closest matching action from the list above.
- If the request mentions "disk image" without clearly meaning a .iso/.img file, assume they mean disk space.
- If the request is genuinely about something not in the list (e.g. playing music, opening a GUI app), say so clearly.
- Respond with exactly ONE sentence starting with "The user wants to".
- Be specific: include the target (path, hostname, process name, etc.) if mentioned.

Examples:
Input: "check disk image"
Output: The user wants to check disk space usage on the system.

Input: "how full is my drive"
Output: The user wants to check disk space usage on the system.

Input: "whats eating my ram"
Output: The user wants to check memory/RAM usage on the system.

Input: "are there any background tasks running"
Output: The user wants to list all currently running processes.

Input: "nuke /tmp/test.txt"
Output: The user wants to delete the file at /tmp/test.txt.

Input: "show me whats in the downloads folder"
Output: The user wants to list files in ~/Downloads.

Input: "can I reach github"
Output: The user wants to ping github.com to check connectivity.

RESPOND WITH EXACTLY ONE SENTENCE. NO OTHER TEXT."#;

// ─────────────────────────────────────────────────────────────────────────────
//  Phase 2: JSON plan generation
// ─────────────────────────────────────────────────────────────────────────────

/// Build the JSON-planner system prompt dynamically from registered capabilities
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
- You will receive the original user text AND an interpreted intent — use the interpreted intent to guide your mapping.

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

// ─────────────────────────────────────────────────────────────────────────────
//  Ollama wire types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    system: String,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

// ─────────────────────────────────────────────────────────────────────────────
//  IntentParser
// ─────────────────────────────────────────────────────────────────────────────

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

    /// Phase 1: Interpret the user's colloquial input into a clear canonical sentence.
    async fn interpret(&self, input: &str) -> String {
        let request = OllamaRequest {
            model: self.model.clone(),
            prompt: input.to_string(),
            system: INTERPRET_SYSTEM_PROMPT.to_string(),
            stream: false,
            format: None, // free text, not JSON
        };

        match self
            .client
            .post(OLLAMA_URL)
            .json(&request)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<OllamaResponse>().await {
                    Ok(r) => {
                        let interpreted = r.response.trim().to_string();
                        log::info!("Intent interpreted: {:?}", interpreted);
                        interpreted
                    }
                    Err(e) => {
                        log::warn!("Failed to parse interpret response: {e}");
                        input.to_string() // fall back to raw input
                    }
                }
            }
            Ok(resp) => {
                log::warn!("Interpret call returned status {}", resp.status());
                input.to_string()
            }
            Err(e) => {
                log::warn!("Interpret call failed: {e}");
                input.to_string()
            }
        }
    }

    /// Layer 0 + Phase 1 + Phase 2: preprocess → interpret → plan.
    /// `context` carries recent conversation history for follow-up resolution.
    pub async fn parse(
        &self,
        input: &str,
        system_prompt: &str,
        context: Option<&str>,
    ) -> Result<TaskPlan, String> {
        // Layer 0 — deterministic keyword matching (instant, no LLM needed)
        let canonical = if let Some(preprocessed) = preprocess_input(input) {
            log::info!("Preprocessed '{}' → '{}'", input, preprocessed);
            preprocessed
        } else {
            // Phase 1 — LLM free-text interpretation for anything not caught above
            self.interpret(input).await
        };

        // Phase 2 — structured JSON plan using the canonical description
        let prompt = {
            let core = if canonical == input {
                // No transformation happened; send raw input as before
                input.to_string()
            } else {
                format!("User said: \"{input}\"\nInterpreted intent: {canonical}")
            };
            if let Some(ctx) = context {
                format!("Recent conversation:\n{ctx}\n\n{core}")
            } else {
                core
            }
        };

        let request = OllamaRequest {
            model: self.model.clone(),
            prompt,
            system: system_prompt.to_string(),
            stream: false,
            format: Some("json".to_string()),
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

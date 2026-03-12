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
        let target = extract_target(&s, &["reach", "ping", "connect to", "check connection to"]);
        let host = target.unwrap_or_else(|| "8.8.8.8".into());
        return Some(format!("ping {host}"));
    }

    // ── Browser: navigate ─────────────────────────────────────────────────────
    let nav_triggers = ["navigate to ", "go to ", "browse to ", "visit ", "open website "];
    for trigger in &nav_triggers {
        if let Some(pos) = s.find(trigger) {
            let after = input[pos + trigger.len()..].trim();
            if let Some(url) = extract_url(after) {
                return Some(format!("navigate browser to {}", url));
            }
        }
    }
    // "open <domain>" where domain looks like a URL
    if s.starts_with("open ") {
        if let Some(url) = extract_url(&input[5..].trim()) {
            return Some(format!("navigate browser to {}", url));
        }
    }

    // ── Browser: web search ───────────────────────────────────────────────────
    let search_web_triggers = [
        "search the web for ", "search online for ", "google ", "look up online ",
        "find on the internet ", "find online ", "search the internet for ",
    ];
    for trigger in &search_web_triggers {
        if let Some(pos) = s.find(trigger) {
            let query = input[pos + trigger.len()..].trim();
            if !query.is_empty() {
                return Some(format!("search the web for {}", query));
            }
        }
    }
    // "search <something>" when web context is clear
    if (s.starts_with("search ") || s.starts_with("look up "))
        && (s.contains("web") || s.contains("internet") || s.contains("online"))
        && !s.contains("file") && !s.contains("folder") && !s.contains("directory")
    {
        let q = s
            .trim_start_matches("search ")
            .trim_start_matches("look up ")
            .trim_start_matches("the web for ")
            .trim_start_matches("online for ")
            .trim_start_matches("on the web for ")
            .trim_start_matches("on the internet for ");
        if !q.is_empty() {
            return Some(format!("search the web for {}", q));
        }
    }

    // ── Browser: get page content ─────────────────────────────────────────────
    let content_triggers = ["get content of ", "get the content of ", "read webpage ", "fetch webpage ", "scrape "];
    for trigger in &content_triggers {
        if let Some(pos) = s.find(trigger) {
            let after = input[pos + trigger.len()..].trim();
            if let Some(url) = extract_url(after) {
                return Some(format!("get web content of {}", url));
            }
        }
    }

    None // fall through to LLM
}

/// Extract the first URL-like token from a string.
/// Returns None if the token doesn't look like a domain or URL.
fn extract_url(text: &str) -> Option<String> {
    let token = text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(|c: char| matches!(c, ',' | '.' | '!' | '?' | ')' | ';'));
    if token.is_empty() {
        return None;
    }
    let looks_like_url = token.starts_with("http")
        || token.contains(".com") || token.contains(".org") || token.contains(".net")
        || token.contains(".io")  || token.contains(".gov") || token.contains(".edu")
        || token.contains(".dev") || token.contains(".ai")  || token.contains(".co/")
        || token.contains(".app");
    if !looks_like_url {
        return None;
    }
    let url = if token.starts_with("http") {
        token.to_string()
    } else {
        format!("https://{}", token)
    };
    Some(url)
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

const INTERPRET_SYSTEM_PROMPT: &str = r#"You are an intent interpreter for an AI agent that can control a computer AND browse the web.
Translate vague or colloquial user requests into one clear sentence describing the action to take.

Available actions:
SYSTEM:
- CHECK DISK SPACE: "disk usage", "free space", "how full", "storage"
- CHECK MEMORY: "memory", "RAM", "how much memory"
- UPTIME: "uptime", "how long running", "boot time"
- HOSTNAME: "hostname", "computer name", "machine name"
- KERNEL INFO: "kernel version", "OS info", "what OS"

PROCESSES:
- LIST PROCESSES: "running processes", "background tasks", "what's running", "active apps", "CPU usage"
- KILL PROCESS: "kill", "stop process", "end task", "terminate"

FILES:
- LIST FILES: "list files", "what's in folder", "show directory", "ls"
- READ FILE: "read file", "show file", "cat", "view file", "what's in the file"
- WRITE FILE: "write file", "create file", "save", "make file"
- DELETE: "delete", "remove", "trash", "nuke", "rm", "wipe"
- FIND FILES: "find file", "search for file", "locate", "glob"
- COPY FILE: "copy", "duplicate", "cp"
- MOVE/RENAME: "move", "rename", "mv"
- CREATE FOLDER: "create folder", "mkdir", "new folder"

NETWORK:
- LIST INTERFACES: "network interfaces", "IP address", "what's my IP", "ifconfig"
- PING: "ping", "can I reach", "check connection", "test connectivity", "is reachable"
- DNS LOOKUP: "DNS lookup", "resolve hostname", "nslookup"
- PORT CHECK: "is port open", "check service on port"
- HTTP REQUEST: "curl", "fetch URL", "HTTP request", "download URL"

PACKAGES:
- LIST PACKAGES: "installed software", "what's installed", "list packages"
- INSTALL: "install package", "brew install", "apt install"
- REMOVE: "uninstall", "remove package"
- SEARCH PACKAGE: "find package", "is package available"

BROWSER (use these for anything web/internet related):
- NAVIGATE TO WEBSITE: "navigate to", "go to", "open", "browse to", "visit", "take me to", "load", "pull up"
- SEARCH THE WEB: "search for", "google", "look up", "find online", "search the web", "search the internet", "what is", "who is", "how do I" (when clearly asking for web info)
- GET PAGE CONTENT: "get content of website", "read webpage", "what does site say", "scrape", "fetch page"
- SCREENSHOT WEBSITE: "screenshot of website", "capture webpage", "take screenshot of URL"

VISION (use for image analysis):
- ANALYZE IMAGE: "analyze image", "describe image", "what's in the image", "look at image", "read image", "what does image show"

Rules:
- For ANY website/URL/domain (e.g. "github.com", "google.com"), use BROWSER actions.
- For vague web queries ("what is X", "how does Y work"), use browser.search.
- Never say "unsupported" for web browsing — the agent CAN browse the internet.
- Respond with exactly ONE sentence starting with "The user wants to".
- Include the specific target (URL, query, path, hostname) in your response.

Examples:
Input: "navigate to github.com"
Output: The user wants to navigate the browser to https://github.com.

Input: "whats eating my ram"
Output: The user wants to check memory/RAM usage.

Input: "search the web for ollama models"
Output: The user wants to search the web for "ollama models".

Input: "what is rust programming language"
Output: The user wants to search the web for "rust programming language".

Input: "nuke /tmp/test.txt"
Output: The user wants to delete the file at /tmp/test.txt.

Input: "show me whats in the downloads folder"
Output: The user wants to list files in ~/Downloads.

Input: "go to news.ycombinator.com"
Output: The user wants to navigate the browser to https://news.ycombinator.com.

Input: "what does apple.com look like"
Output: The user wants to navigate to https://apple.com and take a screenshot.

Input: "analyze the image at /tmp/photo.png"
Output: The user wants to analyze the image file at /tmp/photo.png using the vision model.

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
- Browser and vision actions are always "low" risk.
- NEVER return empty steps. If the user mentions a URL, domain, or web topic, use browser capability.
- For web searches ("what is X", "find info on X"), use browser.search.
- For visiting a URL/domain, use browser.navigate.
- You may create multi-step plans (e.g. navigate + analyze image).
- If conversation context is provided, use it to resolve references like "same", "that", "again", etc.
- Only use unsupported if it is truly impossible (e.g. "play music", "open GUI app").

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

Input: "check disk usage"
Output: {{"intent":"check_disk","description":"Check disk usage","steps":[{{"capability":"system","action":"disk_usage","params":{{}},"description":"Check current disk usage"}}],"risk_level":"low"}}

Input: "navigate to github.com"
Output: {{"intent":"browse_website","description":"Navigate to github.com","steps":[{{"capability":"browser","action":"navigate","params":{{"url":"https://github.com"}},"description":"Open github.com in the browser"}}],"risk_level":"low"}}

Input: "go to news.ycombinator.com"
Output: {{"intent":"browse_website","description":"Navigate to Hacker News","steps":[{{"capability":"browser","action":"navigate","params":{{"url":"https://news.ycombinator.com"}},"description":"Open Hacker News"}}],"risk_level":"low"}}

Input: "navigate browser to https://apple.com"
Output: {{"intent":"browse_website","description":"Navigate to apple.com","steps":[{{"capability":"browser","action":"navigate","params":{{"url":"https://apple.com"}},"description":"Open apple.com in the browser"}}],"risk_level":"low"}}

Input: "search the web for rust programming"
Output: {{"intent":"web_search","description":"Search for rust programming","steps":[{{"capability":"browser","action":"search","params":{{"query":"rust programming"}},"description":"Search DuckDuckGo for rust programming"}}],"risk_level":"low"}}

Input: "what is the latest news"
Output: {{"intent":"web_search","description":"Search for latest news","steps":[{{"capability":"browser","action":"search","params":{{"query":"latest news today"}},"description":"Search the web for latest news"}}],"risk_level":"low"}}

Input: "get web content of https://example.com"
Output: {{"intent":"get_page_content","description":"Get content from example.com","steps":[{{"capability":"browser","action":"get_content","params":{{"url":"https://example.com"}},"description":"Fetch and extract page text"}}],"risk_level":"low"}}

Input: "take a screenshot of wikipedia.org"
Output: {{"intent":"screenshot_website","description":"Screenshot wikipedia.org","steps":[{{"capability":"browser","action":"screenshot","params":{{"url":"https://wikipedia.org"}},"description":"Take a headless screenshot of Wikipedia"}}],"risk_level":"low"}}

Input: "analyze the image at /tmp/photo.png"
Output: {{"intent":"analyze_image","description":"Analyze image file","steps":[{{"capability":"vision","action":"analyze_image","params":{{"path":"/tmp/photo.png"}},"description":"Analyze image using vision model"}}],"risk_level":"low"}}

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

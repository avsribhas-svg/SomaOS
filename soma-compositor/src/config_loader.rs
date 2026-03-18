/// Read current LLM config values from `~/.soma/config.toml`.
///
/// The compositor doesn't import the soma-agent crate, so we parse TOML directly.
/// Returns `(provider, model, api_key, api_url)` with sensible defaults.
pub fn load_config_values() -> (String, String, String, String) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let path = std::path::PathBuf::from(home).join(".soma").join("config.toml");
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(val) = content.parse::<toml::Value>() {
            let m = val.get("model");
            let get = |key: &str| -> String {
                m.and_then(|t| t.get(key))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            let provider = get("provider");
            let provider = if provider.is_empty() { "ollama".to_string() } else { provider };
            let model    = get("model");
            let model    = if model.is_empty() { "qwen2.5-coder:7b".to_string() } else { model };
            let api_key  = get("api_key");
            let api_url  = get("api_url");
            let api_url  = if api_url.is_empty() { "http://localhost:11434".to_string() } else { api_url };
            return (provider, model, api_key, api_url);
        }
    }
    (
        "ollama".to_string(),
        "qwen2.5-coder:7b".to_string(),
        String::new(),
        "http://localhost:11434".to_string(),
    )
}

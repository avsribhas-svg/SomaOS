//! SomaOS agent configuration — persisted to ~/.soma/config.toml
//!
//! Allows users to choose their LLM provider (Ollama, Anthropic, OpenAI,
//! Gemini, or any OpenAI-compatible endpoint) without restarting the OS.

use serde::{Deserialize, Serialize};

/// Which LLM provider powers the agent brain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Ollama,
    Anthropic,
    OpenAi,
    Gemini,
    Custom, // any OpenAI-compatible endpoint
}

impl Provider {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "anthropic" => Provider::Anthropic,
            "openai"    => Provider::OpenAi,
            "gemini"    => Provider::Gemini,
            "custom"    => Provider::Custom,
            _           => Provider::Ollama,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Ollama    => "ollama",
            Provider::Anthropic => "anthropic",
            Provider::OpenAi    => "openai",
            Provider::Gemini    => "gemini",
            Provider::Custom    => "custom",
        }
    }

    pub fn default_model(&self) -> &'static str {
        match self {
            Provider::Ollama    => "qwen2.5-coder:7b",
            Provider::Anthropic => "claude-haiku-4-5-20251001",
            Provider::OpenAi    => "gpt-4o-mini",
            Provider::Gemini    => "gemini-2.0-flash",
            Provider::Custom    => "gpt-4o-mini",
        }
    }

    pub fn default_api_url(&self) -> &'static str {
        match self {
            Provider::Ollama    => "http://localhost:11434",
            Provider::Anthropic => "https://api.anthropic.com",
            Provider::OpenAi    => "https://api.openai.com",
            Provider::Gemini    => "https://generativelanguage.googleapis.com",
            Provider::Custom    => "http://localhost:11434",
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub api_key: String,
    pub api_url: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            provider: "ollama".to_string(),
            model: "qwen2.5-coder:7b".to_string(),
            api_key: String::new(),
            api_url: "http://localhost:11434".to_string(),
        }
    }
}

/// A peer SomaOS node reachable over TCP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerNode {
    /// Human-readable name used in the `delegate` capability.
    pub name: String,
    /// TCP address of the peer agent daemon (e.g. "192.168.1.42:7878").
    pub addr: String,
    /// Raw bearer token sent to the peer for auth (peer must have its SHA-256 in accepted_tokens).
    pub token: Option<String>,
    /// Whether to use TLS when connecting to this peer.
    #[serde(default)]
    pub tls: bool,
}

/// Network/federation transport config.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkConfig {
    /// Optional TCP listen address (e.g. "0.0.0.0:7878"). Disabled if absent.
    pub tcp_listen_addr: Option<String>,
    /// Path to PEM certificate for TLS. Requires tls_key_path.
    pub tls_cert_path: Option<String>,
    /// Path to PEM private key for TLS.
    pub tls_key_path: Option<String>,
    /// SHA-256 hex digests of accepted bearer tokens (TCP connections only).
    #[serde(default)]
    pub accepted_tokens: Vec<String>,
    /// Known peer nodes for federation / delegation.
    #[serde(default)]
    pub peers: Vec<PeerNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SomaConfig {
    pub model: ModelConfig,
    #[serde(default)]
    pub network: NetworkConfig,
}

impl SomaConfig {
    fn config_path() -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        std::path::PathBuf::from(home).join(".soma").join("config.toml")
    }

    /// Load config from ~/.soma/config.toml, falling back to Ollama defaults.
    pub fn load() -> Self {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                log::warn!("Failed to parse config at {:?}: {} — using defaults", path, e);
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Save config to ~/.soma/config.toml (permissions 0o600).
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, &content)?;

        // Set permissions to 600 (owner read/write only) on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }

        log::info!("Config saved to {:?}", path);
        Ok(())
    }

    pub fn provider(&self) -> Provider {
        Provider::from_str(&self.model.provider)
    }
}

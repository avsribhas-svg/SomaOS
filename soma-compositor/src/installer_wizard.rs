//! First-boot installer wizard — DRM-only, full-screen, no dock/sidebar.
//!
//! Three steps:
//!   1. Username + password → /etc/soma/passwd
//!   2. LLM provider + API key → ~/.soma/config.toml
//!   3. Network interface + static/DHCP → ip commands
//!
//! Returns `true` from `handle_input` when the wizard is complete.

use crate::backend::event::{InputEvent, KeyCode};
use crate::renderer::Renderer;
use tiny_skia::Pixmap;

#[derive(Debug, Clone, PartialEq)]
pub enum WizardStep {
    Username,
    Password,
    LlmProvider,
    ApiKey,
    Network,
    Done,
}

pub struct InstallerWizard {
    pub step:     WizardStep,
    pub username: String,
    pub password: String,
    pub provider: String,
    pub api_key:  String,
    pub network:  String,
    input_buf:    String,
}

impl InstallerWizard {
    pub fn new() -> Self {
        Self {
            step:     WizardStep::Username,
            username: String::new(),
            password: String::new(),
            provider: "ollama".to_string(),
            api_key:  String::new(),
            network:  "dhcp".to_string(),
            input_buf: String::new(),
        }
    }

    /// Process a batch of input events. Returns `true` when wizard is complete.
    pub fn handle_input(&mut self, events: &[InputEvent]) -> bool {
        for ev in events {
            if let InputEvent::KeyPress { code, text } = ev {
                match code {
                    KeyCode::Enter => {
                        self.advance();
                        if self.step == WizardStep::Done {
                            return true;
                        }
                    }
                    KeyCode::Backspace => { self.input_buf.pop(); }
                    KeyCode::Char(c) | KeyCode::Ctrl(c) => {
                        if let KeyCode::Char(_) = code {
                            self.input_buf.push(*c);
                        }
                    }
                    _ => {
                        if let Some(ch) = text {
                            self.input_buf.push(*ch);
                        }
                    }
                }
            }
        }
        false
    }

    fn advance(&mut self) {
        match self.step {
            WizardStep::Username   => { self.username = self.input_buf.trim().to_string(); self.input_buf.clear(); self.step = WizardStep::Password; }
            WizardStep::Password   => { self.password = self.input_buf.clone(); self.input_buf.clear(); self.step = WizardStep::LlmProvider; }
            WizardStep::LlmProvider => { let p = self.input_buf.trim().to_lowercase(); if !p.is_empty() { self.provider = p; } self.input_buf.clear(); self.step = WizardStep::ApiKey; }
            WizardStep::ApiKey     => { self.api_key = self.input_buf.trim().to_string(); self.input_buf.clear(); self.step = WizardStep::Network; }
            WizardStep::Network    => { let n = self.input_buf.trim().to_lowercase(); if !n.is_empty() { self.network = n; } self.input_buf.clear(); self.step = WizardStep::Done; }
            WizardStep::Done       => {}
        }
    }

    /// Persist collected configuration to disk.
    pub fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        // 1. Write /etc/soma/passwd
        std::fs::create_dir_all("/etc/soma")?;
        std::fs::write("/etc/soma/passwd", format!("{}:{}", self.username, self.password))?;

        // 2. Write ~/.soma/config.toml
        let home = format!("/home/{}", self.username);
        let soma_dir = format!("{}/.soma", home);
        std::fs::create_dir_all(&soma_dir)?;

        let config = format!(
            "[model]\nprovider = \"{}\"\nmodel = \"{}\"\napi_key = \"{}\"\napi_url = \"{}\"\n",
            self.provider,
            default_model(&self.provider),
            self.api_key,
            default_url(&self.provider),
        );
        std::fs::write(format!("{}/config.toml", soma_dir), config)?;

        // 3. Network configuration
        if self.network == "dhcp" {
            let _ = std::process::Command::new("dhcpcd").arg("eth0").status();
        } else {
            let parts: Vec<&str> = self.network.split_whitespace().collect();
            if !parts.is_empty() {
                let _ = std::process::Command::new("ip")
                    .args(["addr", "add", parts[0], "dev", "eth0"])
                    .status();
            }
            if parts.len() >= 3 && parts[1] == "gw" {
                let _ = std::process::Command::new("ip")
                    .args(["route", "add", "default", "via", parts[2]])
                    .status();
            }
        }

        log::info!("Installer: committed configuration for user '{}'", self.username);
        Ok(())
    }

    /// Render the wizard UI — full-screen, no dock, no sidebar.
    pub fn render(&self, renderer: &mut Renderer, pixmap: &mut Pixmap, w: u32, h: u32) {
        let wf = w as f32;
        let hf = h as f32;

        // Background
        pixmap.fill(tiny_skia::Color::from_rgba8(10, 15, 30, 255));

        // Header bar
        renderer.fill_rect(pixmap, 0.0, 0.0, wf, 56.0, [22, 33, 62, 255]);
        renderer.draw_text(pixmap, "SomaOS Installer", 24.0, 16.0, wf - 48.0, 24.0, [255, 255, 255, 255]);

        // Step indicator
        let steps = ["Username", "Password", "LLM Provider", "API Key", "Network"];
        let step_idx = match self.step {
            WizardStep::Username    => 0,
            WizardStep::Password    => 1,
            WizardStep::LlmProvider => 2,
            WizardStep::ApiKey      => 3,
            WizardStep::Network     => 4,
            WizardStep::Done        => 5,
        };
        let step_w = wf / steps.len() as f32;
        for (i, label) in steps.iter().enumerate() {
            let color: [u8; 4] = if i < step_idx { [74, 222, 128, 255] }
                else if i == step_idx { [99, 102, 241, 255] }
                else { [60, 60, 80, 255] };
            renderer.fill_rect(pixmap, i as f32 * step_w, 56.0, step_w - 2.0, 4.0, color);
            renderer.draw_text(pixmap, label, i as f32 * step_w + 4.0, 64.0, step_w - 8.0, 14.0, color);
        }

        // Card
        let cx = wf / 2.0 - 250.0;
        let cy = hf / 2.0 - 120.0;
        renderer.fill_rounded_rect(pixmap, cx, cy, 500.0, 240.0, 12.0, [22, 33, 62, 255]);

        let (prompt, hint) = step_prompt(&self.step, &self.provider);
        renderer.draw_text(pixmap, &prompt, cx + 20.0, cy + 20.0, 460.0, 22.0, [255, 255, 255, 255]);
        if !hint.is_empty() {
            renderer.draw_text(pixmap, &hint, cx + 20.0, cy + 48.0, 460.0, 14.0, [150, 150, 170, 255]);
        }

        // Input field
        renderer.fill_rounded_rect(pixmap, cx + 20.0, cy + 80.0, 460.0, 44.0, 6.0, [30, 42, 70, 255]);
        let display = if self.step == WizardStep::Password {
            "•".repeat(self.input_buf.len())
        } else {
            self.input_buf.clone()
        };
        renderer.draw_text(pixmap, &format!("{}_", display), cx + 30.0, cy + 92.0, 440.0, 18.0, [255, 255, 255, 255]);
        renderer.draw_text(pixmap, "Press Enter to continue", cx + 20.0, cy + 200.0, 460.0, 14.0, [100, 100, 140, 255]);
    }
}

fn step_prompt(step: &WizardStep, provider: &str) -> (String, String) {
    match step {
        WizardStep::Username    => ("Choose a username".into(), "Primary user account.".into()),
        WizardStep::Password    => ("Set a password".into(), "Used for login. Choose something strong.".into()),
        WizardStep::LlmProvider => ("LLM Provider".into(), format!("Current: {}. Options: ollama, anthropic, openai, gemini.", provider)),
        WizardStep::ApiKey      => ("API Key".into(), "Leave blank for Ollama (local). Required for cloud providers.".into()),
        WizardStep::Network     => ("Network".into(), "Type 'dhcp' or '192.168.1.x/24 gw 192.168.1.1' for static.".into()),
        WizardStep::Done        => ("Setup complete!".into(), "Rebooting into SomaOS...".into()),
    }
}

fn default_model(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "claude-haiku-4-5-20251001",
        "openai"    => "gpt-4o-mini",
        "gemini"    => "gemini-2.0-flash",
        _           => "qwen2.5-coder:7b",
    }
}

fn default_url(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "https://api.anthropic.com",
        "openai"    => "https://api.openai.com",
        "gemini"    => "https://generativelanguage.googleapis.com",
        _           => "http://localhost:11434",
    }
}

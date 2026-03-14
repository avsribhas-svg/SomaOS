use crate::renderer::Renderer;
use soma_common::CompositorMessage;
use tiny_skia::Pixmap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    None,
    Model,
    ApiKey,
    ApiUrl,
}

pub const PROVIDERS: &[(&str, &str)] = &[
    ("ollama",    "Ollama (local)"),
    ("anthropic", "Anthropic (Claude)"),
    ("openai",    "OpenAI"),
    ("gemini",    "Gemini"),
    ("custom",    "Custom (OpenAI-compat)"),
];

#[derive(Debug, Clone)]
pub struct SettingsApp {
    pub provider_idx: usize,
    pub model_input: String,
    pub api_key_input: String,
    pub api_url_input: String,
    pub active_field: SettingsField,
    pub saved_toast: f32, // countdown timer for "Saved!" toast
}

impl SettingsApp {
    pub fn new() -> Self {
        let (provider, model, api_key, api_url) = load_config_values();
        let provider_idx = PROVIDERS.iter().position(|(k, _)| *k == provider.as_str()).unwrap_or(0);
        Self {
            provider_idx,
            model_input: model,
            api_key_input: api_key,
            api_url_input: api_url,
            active_field: SettingsField::None,
            saved_toast: 0.0,
        }
    }

    pub fn on_char(&mut self, c: char) {
        match self.active_field {
            SettingsField::Model  => self.model_input.push(c),
            SettingsField::ApiKey => self.api_key_input.push(c),
            SettingsField::ApiUrl => self.api_url_input.push(c),
            SettingsField::None   => {}
        }
    }

    pub fn on_backspace(&mut self) {
        match self.active_field {
            SettingsField::Model  => { self.model_input.pop(); }
            SettingsField::ApiKey => { self.api_key_input.pop(); }
            SettingsField::ApiUrl => { self.api_url_input.pop(); }
            SettingsField::None   => {}
        }
    }

    pub fn on_click(&mut self, rel_x: f32, rel_y: f32, w: f32) -> Option<CompositorMessage> {
        let radio_start_y = 60.0;
        let radio_h = 28.0;

        // Check radio buttons
        for (i, _) in PROVIDERS.iter().enumerate() {
            let ry = radio_start_y + i as f32 * radio_h;
            if rel_y >= ry && rel_y < ry + radio_h && rel_x >= 24.0 && rel_x <= w - 24.0 {
                self.provider_idx = i;
                let key = PROVIDERS[i].0;
                // Auto-fill default URL and model
                self.api_url_input = match key {
                    "anthropic" => "https://api.anthropic.com".to_string(),
                    "openai"    => "https://api.openai.com".to_string(),
                    "gemini"    => "https://generativelanguage.googleapis.com".to_string(),
                    _           => "http://localhost:11434".to_string(),
                };
                self.model_input = match key {
                    "anthropic" => "claude-haiku-4-5-20251001".to_string(),
                    "openai"    => "gpt-4o-mini".to_string(),
                    "gemini"    => "gemini-2.0-flash".to_string(),
                    _           => "llama3.1:8b".to_string(),
                };
                return None;
            }
        }

        // Check text fields
        let fields_start_y = radio_start_y + PROVIDERS.len() as f32 * radio_h + 8.0;
        let field_stride = 58.0;
        let field_rect_h = 30.0;

        for i in 0..3usize {
            let label_y = fields_start_y + i as f32 * field_stride;
            let field_y = label_y + 14.0;
            if rel_y >= label_y && rel_y < field_y + field_rect_h && rel_x >= 24.0 && rel_x <= w - 24.0 {
                self.active_field = match i {
                    0 => SettingsField::Model,
                    1 => SettingsField::ApiKey,
                    2 => SettingsField::ApiUrl,
                    _ => SettingsField::None,
                };
                return None;
            }
        }

        // Save button check
        let save_y = fields_start_y + 3.0 * field_stride;
        if rel_y >= save_y && rel_y < save_y + 28.0 && rel_x >= 24.0 && rel_x <= 24.0 + 120.0 {
            self.active_field = SettingsField::None;
            self.saved_toast = 2.5;
            return Some(CompositorMessage::UpdateConfig {
                provider: PROVIDERS[self.provider_idx].0.to_string(),
                model:    self.model_input.clone(),
                api_key:  self.api_key_input.clone(),
                api_url:  self.api_url_input.clone(),
            });
        }

        self.active_field = SettingsField::None;
        None
    }

    pub fn render(
        &self,
        renderer: &mut Renderer,
        pixmap: &mut Pixmap,
        cx: f32,
        cy: f32,
        cw: f32,
        ch: f32,
        cursor_visible: bool,
    ) {
        let t = renderer.theme.clone();
        renderer.fill_rect(pixmap, cx, cy, cw, ch, t.bg_window_chrome);

        let ox = cx;
        let mut y = cy + 20.0;

        // Header
        renderer.draw_text(pixmap, "LLM PROVIDER", ox + 24.0, y, cw - 48.0, 10.0, t.text_muted);
        y += 24.0;

        for (i, (_, label)) in PROVIDERS.iter().enumerate() {
            let selected = self.provider_idx == i;
            if selected {
                renderer.fill_rect(pixmap, ox + 10.0, y, 2.0, 24.0, t.accent);
                renderer.fill_rect(pixmap, ox + 12.0, y, cw - 36.0, 24.0, [255, 255, 255, 6]);
            }
            let dot_col = if selected { t.accent } else { [80, 80, 80, 255] };
            renderer.fill_rounded_rect(pixmap, ox + 26.0, y + 7.0, 10.0, 10.0, 5.0, dot_col);
            if selected {
                renderer.fill_rounded_rect(pixmap, ox + 29.0, y + 10.0, 4.0, 4.0, 2.0, [255, 255, 255, 255]);
            }
            let text_col = if selected { t.text_primary } else { t.text_secondary };
            renderer.draw_text(pixmap, label, ox + 42.0, y + 6.0, cw - 60.0, 10.0, text_col);
            y += 28.0;
        }

        y += 8.0;
        renderer.fill_rect(pixmap, ox + 24.0, y - 4.0, cw - 48.0, 1.0, t.border);

        let mut draw_field = |label: &str, value: &str, mask: bool, active: bool, fy: &mut f32| {
            renderer.draw_text(pixmap, label, ox + 24.0, *fy, cw - 48.0, 9.0, t.text_muted);
            *fy += 14.0;
            let border_col = if active { t.accent } else { [60, 60, 60, 255] };
            renderer.fill_rect(pixmap, ox + 24.0, *fy, cw - 48.0, 30.0, t.bg_input);
            renderer.stroke_rect(pixmap, ox + 24.0, *fy, cw - 48.0, 30.0, border_col);
            if active {
                renderer.fill_rect(pixmap, ox + 24.0, *fy + 29.0, cw - 48.0, 1.0, t.accent);
            }
            let display = if mask && !value.is_empty() {
                let shown = value.chars().count().saturating_sub(4);
                format!("{}{}", "*".repeat(shown), &value[value.len().saturating_sub(4)..])
            } else if active && cursor_visible {
                format!("{}|", value)
            } else if value.is_empty() {
                "(not set)".to_string()
            } else {
                value.to_string()
            };
            let text_col = if value.is_empty() { t.text_muted } else { t.text_primary };
            renderer.draw_text(pixmap, &display, ox + 30.0, *fy + 9.0, cw - 60.0, 10.0, text_col);
            *fy += 44.0;
        };

        draw_field("Model", &self.model_input, false, self.active_field == SettingsField::Model, &mut y);
        draw_field("API Key", &self.api_key_input, true, self.active_field == SettingsField::ApiKey, &mut y);
        draw_field("API URL", &self.api_url_input, false, self.active_field == SettingsField::ApiUrl, &mut y);

        let btn_w = 120.0;
        renderer.fill_rect(pixmap, ox + 24.0, y, btn_w, 28.0, t.accent);
        renderer.draw_text(pixmap, "Save Settings", ox + 24.0 + (btn_w - 90.0) / 2.0, y + 8.0, 100.0, 10.0, [255, 255, 255, 255]);
        
        if self.saved_toast > 0.0 {
            renderer.draw_text(pixmap, "v  Settings applied", ox + 34.0 + btn_w, y + 8.0, 150.0, 9.5, t.success);
        }

        let hint_y = cy + ch - 24.0;
        renderer.draw_text(pixmap, "~/.soma/config.toml", ox + 24.0, hint_y, cw - 48.0, 8.0, t.text_muted);
    }
}

/// Read current config values from disk (compositor doesn't import soma-agent crate,
/// so we parse the TOML directly).
fn load_config_values() -> (String, String, String, String) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let path = std::path::PathBuf::from(home).join(".soma").join("config.toml");
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(val) = content.parse::<toml::Value>() {
            let m = val.get("model");
            let get = |key: &str| -> String {
                m.and_then(|t| t.get(key)).and_then(|v| v.as_str()).unwrap_or("").to_string()
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
    ("ollama".to_string(), "qwen2.5-coder:7b".to_string(), String::new(), "http://localhost:11434".to_string())
}

use base64::Engine as _;
use soma_common::{AgentMessage, AgentStatus, CapabilityInfo, CapabilityResult, CompositorMessage, TaskPlan, RiskLevel};
use crate::renderer::Renderer;
use tiny_skia::Pixmap;

// ──────────────────────────────────────────────
//  Settings state
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Chat,
    Settings,
    Registry,
}

/// Which text field in the settings tab is focused for keyboard input
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    None,
    Model,
    ApiKey,
    ApiUrl,
}

const PROVIDERS: &[(&str, &str)] = &[
    ("ollama",    "Ollama (local)"),
    ("anthropic", "Anthropic (Claude)"),
    ("openai",    "OpenAI"),
    ("gemini",    "Gemini"),
    ("custom",    "Custom (OpenAI-compat)"),
];

pub struct SettingsState {
    pub provider_idx: usize,
    pub model_input: String,
    pub api_key_input: String,
    pub api_url_input: String,
    pub active_field: SettingsField,
    pub saved_toast: f32, // countdown timer for "Saved!" toast
}

impl SettingsState {
    fn new() -> Self {
        // Load current values from ~/.soma/config.toml
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
}

use crate::config_loader::load_config_values;

/// Sidebar panel width in pixels
pub const SIDEBAR_WIDTH: f32 = 380.0;

const MAX_HISTORY: usize = 100;
const MSG_PAD: f32 = 8.0;  // Padding between messages

// ──────────────────────────────────────────────
//  Chat message types
// ──────────────────────────────────────────────

/// Decoded image thumbnail ready for tiny-skia rendering (premultiplied RGBA)
#[derive(Clone)]
pub enum PreviewData {
    None,
    Image { pixels: Vec<u8>, width: u32, height: u32 },
}

#[derive(Clone)]
pub enum ChatMessage {
    UserInput { text: String },
    Thinking,
    PlanProposal {
        _id: String,
        plan: TaskPlan,
    },
    /// Final results (we skip per-step messages to avoid clutter)
    ExecutionDone {
        intent: String,
        success_count: usize,
        total: usize,
        results: Vec<CapabilityResult>,
        preview: PreviewData,
    },
    AgentError { message: String },
}

// ──────────────────────────────────────────────
//  Sidebar state
// ──────────────────────────────────────────────

pub struct Sidebar {
    pub input_text: String,
    pub status: AgentStatus,
    pub current_plan_id: Option<String>,
    pub current_plan: Option<TaskPlan>,
    current_step_total: usize,
    pub messages: Vec<ChatMessage>,
    pub scroll_offset: f32,
    pub cursor_visible: bool,
    cursor_timer: f32,
    /// Index of the message whose detail modal is open (None = closed)
    pub expanded_msg_idx: Option<usize>,
    /// Active tab (Chat or Settings)
    pub tab: SidebarTab,
    /// Settings tab state
    pub settings: SettingsState,
    /// Current animated x-offset for slide-in/out (f32::MAX = not yet initialised)
    pub slide_x: f32,
    /// Target x-offset (screen_w = hidden off-right, screen_w - SIDEBAR_WIDTH = visible)
    pub slide_target_x: f32,
    /// Capability list for the Registry tab (populated from AgentMessage::Capabilities)
    pub capabilities: Vec<CapabilityInfo>,
    /// Scroll offset for the Registry tab
    pub registry_scroll: f32,
}

impl Sidebar {
    pub fn new() -> Self {
        Self {
            input_text: String::new(),
            status: AgentStatus::Idle,
            current_plan_id: None,
            current_plan: None,
            current_step_total: 0,
            messages: Vec::new(),
            scroll_offset: 0.0,
            cursor_visible: true,
            cursor_timer: 0.0,
            expanded_msg_idx: None,
            tab: SidebarTab::Chat,
            settings: SettingsState::new(),
            slide_x: f32::MAX,
            slide_target_x: f32::MAX,
            capabilities: Vec::new(),
            registry_scroll: 0.0,
        }
    }

    pub fn on_char(&mut self, c: char) {
        if self.tab == SidebarTab::Settings {
            match self.settings.active_field {
                SettingsField::Model  => self.settings.model_input.push(c),
                SettingsField::ApiKey => self.settings.api_key_input.push(c),
                SettingsField::ApiUrl => self.settings.api_url_input.push(c),
                SettingsField::None   => {}
            }
            return;
        }
        if self.tab == SidebarTab::Registry {
            return; // Registry tab has no text input
        }
        if matches!(self.status, AgentStatus::Idle | AgentStatus::Completed | AgentStatus::Error) {
            self.input_text.push(c);
        }
    }

    pub fn on_backspace(&mut self) {
        if self.tab == SidebarTab::Settings {
            match self.settings.active_field {
                SettingsField::Model  => { self.settings.model_input.pop(); }
                SettingsField::ApiKey => { self.settings.api_key_input.pop(); }
                SettingsField::ApiUrl => { self.settings.api_url_input.pop(); }
                SettingsField::None   => {}
            }
            return;
        }
        if self.tab == SidebarTab::Registry {
            return; // Registry tab has no text input
        }
        self.input_text.pop();
    }

    pub fn scroll(&mut self, delta: f32) {
        if self.tab == SidebarTab::Registry {
            self.registry_scroll = (self.registry_scroll - delta).max(0.0);
        } else {
            self.scroll_offset = (self.scroll_offset - delta).max(0.0);
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        let estimated = self.messages.len() as f32 * 80.0;
        self.scroll_offset = estimated.max(0.0);
    }

    pub fn on_submit(&mut self) -> Option<CompositorMessage> {
        match self.status {
            AgentStatus::Idle | AgentStatus::Completed | AgentStatus::Error => {
                let text = self.input_text.trim().to_string();
                if text.is_empty() { return None; }
                self.messages.push(ChatMessage::UserInput { text: text.clone() });
                self.messages.push(ChatMessage::Thinking);
                self.input_text.clear();
                self.status = AgentStatus::Thinking;
                self.scroll_to_bottom();
                Some(CompositorMessage::NaturalLanguageInput { text })
            }
            AgentStatus::AwaitingApproval => self.on_approve(),
            _ => None,
        }
    }

    pub fn on_approve(&mut self) -> Option<CompositorMessage> {
        if let Some(id) = self.current_plan_id.take() {
            self.status = AgentStatus::Executing;
            if let Some(plan) = &self.current_plan {
                self.current_step_total = plan.steps.len();
            }
            Some(CompositorMessage::Approve { id })
        } else { None }
    }

    pub fn on_reject(&mut self) -> Option<CompositorMessage> {
        if let Some(id) = self.current_plan_id.take() {
            self.current_plan = None;
            self.status = AgentStatus::Idle;
            self.messages.retain(|m| !matches!(m, ChatMessage::Thinking));
            Some(CompositorMessage::Reject { id })
        } else { None }
    }

    pub fn handle_agent_message(&mut self, msg: AgentMessage) -> Option<CompositorMessage> {
        match msg {
            AgentMessage::Capabilities { capabilities } => {
                self.capabilities = capabilities;
                return None;
            }
            AgentMessage::CapabilitiesReloaded { .. } => {
                // Ask agent for the fresh list now
                return Some(CompositorMessage::QueryCapabilities);
            }
            _ => {}
        }
        self.handle_agent_message_inner(msg);
        None
    }

    fn handle_agent_message_inner(&mut self, msg: AgentMessage) {
        match msg {
            AgentMessage::TaskPlanReady { id, plan } => {
                self.messages.retain(|m| !matches!(m, ChatMessage::Thinking));
                self.current_step_total = plan.steps.len();
                self.messages.push(ChatMessage::PlanProposal { _id: id.clone(), plan: plan.clone() });
                self.status = AgentStatus::AwaitingApproval;
                self.current_plan = Some(plan);
                self.current_plan_id = Some(id);
                self.scroll_to_bottom();
            }
            AgentMessage::StepResult { .. } => {
                // Skip per-step messages — we show everything at ExecutionComplete
            }
            AgentMessage::ExecutionComplete { results, .. } => {
                let intent = self.current_plan.as_ref().map(|p| p.intent.clone()).unwrap_or_default();
                self.status = AgentStatus::Completed;
                self.current_plan = None;
                let ok = results.iter().filter(|r| r.success).count();
                let preview = decode_image_preview(&results);
                self.messages.push(ChatMessage::ExecutionDone {
                    intent,
                    success_count: ok,
                    total: results.len(),
                    results,
                    preview,
                });
                self.scroll_to_bottom();
            }
            AgentMessage::Error { message, .. } => {
                self.messages.retain(|m| !matches!(m, ChatMessage::Thinking));
                self.status = AgentStatus::Error;
                self.current_plan = None;
                self.current_plan_id = None;
                self.messages.push(ChatMessage::AgentError { message });
                self.scroll_to_bottom();
            }
            _ => {}
        }
        if self.messages.len() > MAX_HISTORY {
            self.messages.drain(0..20);
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.cursor_timer += dt;
        if self.cursor_timer > 0.53 {
            self.cursor_timer = 0.0;
            self.cursor_visible = !self.cursor_visible;
        }
        if self.settings.saved_toast > 0.0 {
            self.settings.saved_toast = (self.settings.saved_toast - dt).max(0.0);
        }
        // Slide animation — 800px/s tween toward slide_target_x
        if self.slide_x != f32::MAX && self.slide_target_x != f32::MAX {
            let diff = self.slide_target_x - self.slide_x;
            let step = 800.0 * dt;
            if diff.abs() <= step {
                self.slide_x = self.slide_target_x;
            } else {
                self.slide_x += step * diff.signum();
            }
        }
    }

    /// Handle a click on the settings tab UI.
    /// Returns an `UpdateConfig` message if the user clicked Save.
    pub fn on_settings_click(&mut self, rel_x: f32, rel_y: f32) -> Option<CompositorMessage> {
        // Tab bar: y=44..71 (44 tab_y + 26 tab_h + 1 separator)
        // Three equal-width tabs: Chat | Settings | Registry
        if rel_y >= 44.0 && rel_y < 71.0 {
            let third = SIDEBAR_WIDTH / 3.0;
            if rel_x < third {
                self.tab = SidebarTab::Chat;
            } else if rel_x < third * 2.0 {
                self.tab = SidebarTab::Settings;
            } else {
                self.tab = SidebarTab::Registry;
            }
            return None;
        }

        // Registry tab button handling
        if self.tab == SidebarTab::Registry {
            return self.on_registry_click(rel_y);
        }

        if self.tab != SidebarTab::Settings {
            return None;
        }

        // Coordinates derived from render_settings_tab():
        //   content_y = 71 (44 + 26 + 1)
        //   section label "LLM Provider" at y=81 (content_y+10), height=18 → radios start at y=99
        //   each radio row: 28px spacing; 5 providers → end at 99+140=239; +8 gap → fields at 247
        //   draw_field advances: 14 (label) + 44 (field+gap) = 58px per field
        //   save button: 247 + 3×58 = 421

        let radio_start_y = 99.0;
        let radio_h = 28.0;
        for (i, _) in PROVIDERS.iter().enumerate() {
            let ry = radio_start_y + i as f32 * radio_h;
            if rel_y >= ry && rel_y < ry + radio_h && rel_x >= 14.0 && rel_x <= SIDEBAR_WIDTH - 14.0 {
                self.settings.provider_idx = i;
                let key = PROVIDERS[i].0;
                // Auto-fill default URL and model for the selected provider
                self.settings.api_url_input = match key {
                    "anthropic" => "https://api.anthropic.com".to_string(),
                    "openai"    => "https://api.openai.com".to_string(),
                    "gemini"    => "https://generativelanguage.googleapis.com".to_string(),
                    _           => "http://localhost:11434".to_string(),
                };
                self.settings.model_input = match key {
                    "anthropic" => "claude-haiku-4-5-20251001".to_string(),
                    "openai"    => "gpt-4o-mini".to_string(),
                    "gemini"    => "gemini-2.0-flash".to_string(),
                    _           => "llama3.1:8b".to_string(),
                };
                return None;
            }
        }

        // Text fields start at 247; each field occupies 58px (14 label + 44 field+gap)
        let fields_start_y = radio_start_y + PROVIDERS.len() as f32 * radio_h + 8.0; // 99+140+8 = 247
        let field_stride = 58.0;
        let field_rect_h = 30.0;
        for i in 0..3usize {
            let label_y = fields_start_y + i as f32 * field_stride;
            let field_y = label_y + 14.0;
            // Hit the full row (label + input rect)
            if rel_y >= label_y && rel_y < field_y + field_rect_h && rel_x >= 14.0 {
                self.settings.active_field = match i {
                    0 => SettingsField::Model,
                    1 => SettingsField::ApiKey,
                    2 => SettingsField::ApiUrl,
                    _ => SettingsField::None,
                };
                return None;
            }
        }

        // Save button at 247 + 3×58 = 421, height 32
        let save_y = fields_start_y + 3.0 * field_stride;
        if rel_y >= save_y && rel_y < save_y + 36.0 && rel_x >= 14.0 {
            self.settings.active_field = SettingsField::None;
            self.settings.saved_toast = 2.5;
            return Some(self.build_update_config());
        }

        // Clicking elsewhere deselects text field focus
        self.settings.active_field = SettingsField::None;
        None
    }

    fn build_update_config(&self) -> CompositorMessage {
        CompositorMessage::UpdateConfig {
            provider: PROVIDERS[self.settings.provider_idx].0.to_string(),
            model:    self.settings.model_input.clone(),
            api_key:  self.settings.api_key_input.clone(),
            api_url:  self.settings.api_url_input.clone(),
        }
    }

    /// Handle a click in the Registry tab.
    /// `rel_y` is relative to the sidebar top (including title/tab bar).
    /// Returns a CompositorMessage if a button was clicked.
    pub fn on_registry_click_with_height(&mut self, rel_y: f32, sidebar_height: f32) -> Option<CompositorMessage> {
        // list_bottom = top_y + content_y_offset + (height - top_y - content_offset) - 74
        // In relative coordinates: content starts at 71, list_bottom at height - 74 (absolute)
        // But rel_y is relative to sidebar top. buttons are bottom-anchored.
        let list_bottom_rel = sidebar_height - 74.0;
        let reload_btn_y = list_bottom_rel + 4.0;
        let gap_btn_y = reload_btn_y + 32.0;

        if rel_y >= reload_btn_y && rel_y < reload_btn_y + 26.0 {
            return Some(CompositorMessage::ReloadCapabilities);
        }
        if rel_y >= gap_btn_y && rel_y < gap_btn_y + 26.0 {
            // Open terminal and run gap log — send a DirectExec for this
            return Some(CompositorMessage::DirectExec {
                id: "gap_log".to_string(),
                command: "cat ~/.soma/gaps.log 2>/dev/null || echo '(no gaps logged yet)'".to_string(),
            });
        }
        None
    }

    fn on_registry_click(&mut self, _rel_y: f32) -> Option<CompositorMessage> {
        // Placeholder: callers that have height should use on_registry_click_with_height
        None
    }

    /// Handle a click inside the sidebar panel.
    /// `rel_x/rel_y` are relative to the sidebar's left edge.
    /// Returns true if a message card was toggled (needs redraw).
    pub fn on_sidebar_click(&mut self, _rel_x: f32, rel_y: f32, height: f32) -> bool {
        // Don't process chat clicks when in the settings tab
        if self.tab != SidebarTab::Chat {
            return false;
        }
        // If detail modal is open, any click dismisses it
        if self.expanded_msg_idx.is_some() {
            self.expanded_msg_idx = None;
            return true;
        }

        let content_y = 48.0;
        let content_h = height - 48.0 - 68.0;
        if rel_y < content_y || rel_y > content_y + content_h { return false; }

        let scroll = self.scroll_offset;
        let mut accumulated = 0.0_f32;

        for (i, msg) in self.messages.iter().enumerate() {
            let h = message_height(msg);
            let msg_y = content_y + 4.0 + accumulated - scroll;
            accumulated += h + MSG_PAD;

            if msg_y + h < content_y { continue; }
            if msg_y > content_y + content_h { break; }

            if rel_y >= msg_y && rel_y <= msg_y + h {
                // Only error cards and execution done cards are expandable
                match msg {
                    ChatMessage::AgentError { .. } | ChatMessage::ExecutionDone { .. } => {
                        self.expanded_msg_idx = Some(i);
                        return true;
                    }
                    _ => {}
                }
                break;
            }
        }
        false
    }

    /// Render the full-detail modal for the expanded message.
    pub fn render_expanded_msg(&self, renderer: &mut Renderer, pixmap: &mut Pixmap, width: f32, height: f32) {
        let idx = match self.expanded_msg_idx {
            Some(i) => i,
            None => return,
        };
        let msg = match self.messages.get(idx) {
            Some(m) => m,
            None => return,
        };
        let t = renderer.theme.clone();

        // Backdrop
        renderer.fill_rect(pixmap, 0.0, 0.0, width, height, [0, 0, 0, 160]);

        let mw = (width * 0.65).min(540.0);
        let mh = (height * 0.72).min(480.0);
        let mx = (width - mw) / 2.0;
        let my = (height - mh) / 2.0;

        renderer.fill_rounded_rect(pixmap, mx, my, mw, mh, 14.0, [18, 18, 36, 252]);
        renderer.stroke_rect(pixmap, mx, my, mw, mh, t.border);

        match msg {
            ChatMessage::AgentError { message } => {
                renderer.draw_text(pixmap, "Error Details", mx + 18.0, my + 14.0, mw - 36.0, 12.0, t.error);
                renderer.fill_rect(pixmap, mx + 18.0, my + 36.0, mw - 36.0, 1.0, t.border);
                let mut ty = my + 50.0;
                for line in wrap_text(message, ((mw - 40.0) / 7.5) as usize) {
                    if ty > my + mh - 36.0 { break; }
                    renderer.draw_text(pixmap, &line, mx + 18.0, ty, mw - 36.0, 10.0, t.text_secondary);
                    ty += 17.0;
                }
            }
            ChatMessage::ExecutionDone { intent, results, success_count, total, .. } => {
                let header_color = if success_count == total { t.success } else { t.warning };
                renderer.draw_text(pixmap, &format!("Result: {}", intent), mx + 18.0, my + 14.0, mw - 36.0, 12.0, header_color);
                renderer.fill_rect(pixmap, mx + 18.0, my + 36.0, mw - 36.0, 1.0, t.border);
                let mut ty = my + 50.0;
                for r in results {
                    if !r.success {
                        if let Some(err) = &r.error {
                            renderer.draw_text(pixmap, "! Error:", mx + 18.0, ty, mw - 36.0, 9.0, t.error);
                            ty += 15.0;
                            for line in wrap_text(&err.message, ((mw - 56.0) / 7.5) as usize) {
                                if ty > my + mh - 36.0 { break; }
                                renderer.draw_text(pixmap, &format!("  {}", line), mx + 18.0, ty, mw - 36.0, 9.5, t.text_secondary);
                                ty += 15.0;
                            }
                            ty += 4.0;
                        }
                    } else if let Some(obj) = r.data.as_object() {
                        for (k, v) in obj.iter().take(12) {
                            if ty > my + mh - 36.0 { break; }
                            let val = match v {
                                serde_json::Value::String(s) => truncate_str(s, 60),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => continue,
                            };
                            renderer.draw_text(pixmap, &format!("  {}: {}", k, val), mx + 18.0, ty, mw - 36.0, 9.5, t.text_secondary);
                            ty += 15.0;
                        }
                    }
                }
            }
            _ => return,
        }

        renderer.draw_text(pixmap, "Click anywhere to dismiss", mx + 18.0, my + mh - 22.0, mw - 36.0, 9.0, t.text_muted);
    }

    // ──────────────────────────────────────────────
    //  Settings tab rendering
    // ──────────────────────────────────────────────

    fn render_settings_tab(
        &self,
        renderer: &mut Renderer,
        pixmap: &mut Pixmap,
        ox: f32,
        content_y: f32,
        height: f32,
        t: &crate::renderer::Theme,
    ) {
        let w = SIDEBAR_WIDTH;
        let s = &self.settings;

        // Section header — VS Code "SETTINGS" style uppercase muted label
        let mut y = content_y + 10.0;
        renderer.draw_text(pixmap, "LLM PROVIDER", ox + 14.0, y, w - 28.0, 9.0, t.text_muted);
        y += 18.0;

        // Provider radio buttons — VS Code list-item style
        for (i, (_, label)) in PROVIDERS.iter().enumerate() {
            let selected = s.provider_idx == i;
            // Active row: left accent bar + subtle bg
            if selected {
                renderer.fill_rect(pixmap, ox, y, 2.0, 24.0, t.accent);
                renderer.fill_rect(pixmap, ox + 2.0, y, w - 2.0, 24.0, [255, 255, 255, 6]);
            }
            // Radio dot
            let dot_col = if selected { t.accent } else { [80, 80, 80, 255] };
            renderer.fill_rounded_rect(pixmap, ox + 16.0, y + 7.0, 10.0, 10.0, 5.0, dot_col);
            if selected {
                renderer.fill_rounded_rect(pixmap, ox + 19.0, y + 10.0, 4.0, 4.0, 2.0, [255, 255, 255, 255]);
            }
            let text_col = if selected { t.text_primary } else { t.text_secondary };
            renderer.draw_text(pixmap, label, ox + 32.0, y + 6.0, w - 48.0, 10.0, text_col);
            y += 28.0;
        }

        // Separator
        y += 8.0;
        renderer.fill_rect(pixmap, ox + 14.0, y - 4.0, w - 28.0, 1.0, t.border);

        // Text field helper — VS Code-style labeled input
        let mut draw_field = |label: &str, value: &str, mask: bool, active: bool, fy: &mut f32| {
            renderer.draw_text(pixmap, label, ox + 14.0, *fy, w - 28.0, 9.0, t.text_muted);
            *fy += 14.0;
            let border_col = if active { t.accent } else { [60, 60, 60, 255] };
            renderer.fill_rect(pixmap, ox + 14.0, *fy, w - 28.0, 30.0, t.bg_input);
            renderer.stroke_rect(pixmap, ox + 14.0, *fy, w - 28.0, 30.0, border_col);
            // Bottom-only accent line when active (VS Code style)
            if active {
                renderer.fill_rect(pixmap, ox + 14.0, *fy + 29.0, w - 28.0, 1.0, t.accent);
            }
            let display = if mask && !value.is_empty() {
                let shown = value.chars().count().saturating_sub(4);
                format!("{}{}", "*".repeat(shown), &value[value.len().saturating_sub(4)..])
            } else if active && self.cursor_visible {
                format!("{}|", value)
            } else if value.is_empty() {
                "(not set)".to_string()
            } else {
                value.to_string()
            };
            let text_col = if value.is_empty() { t.text_muted } else { t.text_primary };
            renderer.draw_text(pixmap, &display, ox + 20.0, *fy + 9.0, w - 44.0, 10.0, text_col);
            *fy += 44.0;
        };

        draw_field("Model", &s.model_input, false, s.active_field == SettingsField::Model, &mut y);
        draw_field("API Key", &s.api_key_input, true, s.active_field == SettingsField::ApiKey, &mut y);
        draw_field("API URL", &s.api_url_input, false, s.active_field == SettingsField::ApiUrl, &mut y);

        // Save button — VS Code primary button style
        renderer.fill_rect(pixmap, ox + 14.0, y, w - 28.0, 28.0, t.accent);
        renderer.draw_text(pixmap, "Save Settings", ox + w / 2.0 - 42.0, y + 8.0, 100.0, 10.0, [255, 255, 255, 255]);
        y += 38.0;

        // Saved confirmation — inline, not a toast
        if s.saved_toast > 0.0 {
            renderer.draw_text(pixmap, "v  Settings applied", ox + 14.0, y, w - 28.0, 9.5, t.success);
        }

        // Bottom hint
        let hint_y = height - 20.0;
        renderer.draw_text(pixmap, "~/.soma/config.toml", ox + 14.0, hint_y, w - 28.0, 8.0, t.text_muted);
    }

    // ──────────────────────────────────────────────
    //  Registry tab rendering
    // ──────────────────────────────────────────────

    fn render_registry_tab(
        &self,
        renderer: &mut Renderer,
        pixmap: &mut Pixmap,
        ox: f32,
        content_y: f32,
        height: f32,
        t: &crate::renderer::Theme,
    ) {
        let w = SIDEBAR_WIDTH;
        let row_h = 22.0;
        let list_bottom = height - 74.0; // leave room for two bottom buttons

        // Section header
        let mut y = content_y + 10.0;
        let cap_count = self.capabilities.len();
        renderer.draw_text(
            pixmap,
            &format!("CAPABILITIES  ({} registered)", cap_count),
            ox + 14.0, y, w - 28.0, 9.0, t.text_muted,
        );
        y += 18.0;

        // Separator
        renderer.fill_rect(pixmap, ox + 14.0, y, w - 28.0, 1.0, t.border);
        y += 6.0;

        // Scrollable capability list
        let list_start_y = y;
        let scroll = self.registry_scroll;

        if self.capabilities.is_empty() {
            renderer.draw_text(
                pixmap,
                "No capabilities loaded",
                ox + 14.0, y + 10.0, w - 28.0, 9.5, t.text_muted,
            );
        } else {
            let mut sorted = self.capabilities.clone();
            sorted.sort_by(|a, b| a.name.cmp(&b.name));

            let mut accumulated = 0.0_f32;
            for cap in &sorted {
                let row_y = list_start_y + accumulated - scroll;
                accumulated += row_h;

                // Clip to list viewport
                if row_y + row_h < list_start_y { continue; }
                if row_y > list_bottom { break; }

                // Left accent bar for script caps; dim for built-in
                let badge_color = if cap.is_builtin {
                    [60, 60, 80, 255]
                } else {
                    t.accent
                };
                renderer.fill_rect(pixmap, ox, row_y, 2.0, row_h - 2.0, badge_color);

                // Capability name
                renderer.draw_text(
                    pixmap,
                    &cap.name,
                    ox + 10.0, row_y + 4.0, w * 0.45, 9.5, t.text_primary,
                );

                // Version
                renderer.draw_text(
                    pixmap,
                    &format!("v{}", cap.version),
                    ox + w * 0.48, row_y + 4.0, w * 0.2, 8.5, t.text_muted,
                );

                // Action count
                let action_label = format!("{}a", cap.actions.len());
                renderer.draw_text(
                    pixmap,
                    &action_label,
                    ox + w * 0.68, row_y + 4.0, w * 0.12, 8.5, t.text_secondary,
                );

                // built-in / script badge
                let badge_text = if cap.is_builtin { "builtin" } else { "script" };
                let badge_fg = if cap.is_builtin { t.text_muted } else { t.accent };
                renderer.draw_text(
                    pixmap,
                    badge_text,
                    ox + w * 0.80, row_y + 4.0, w * 0.18, 8.0, badge_fg,
                );

                // Row separator
                renderer.fill_rect(pixmap, ox + 10.0, row_y + row_h - 1.0, w - 20.0, 1.0, [255, 255, 255, 8]);
            }
        }

        // ── Bottom buttons ──
        let btn_area_y = list_bottom + 4.0;

        // "Reload Scripts" button
        renderer.fill_rect(pixmap, ox + 14.0, btn_area_y, w - 28.0, 26.0, t.accent);
        renderer.draw_text(
            pixmap,
            "Reload Scripts",
            ox + w / 2.0 - 38.0, btn_area_y + 7.0, 90.0, 9.5, [255, 255, 255, 255],
        );

        // "Gap Log" button (secondary style)
        let gap_btn_y = btn_area_y + 32.0;
        renderer.fill_rect(pixmap, ox + 14.0, gap_btn_y, w - 28.0, 26.0, [255, 255, 255, 10]);
        renderer.stroke_rect(pixmap, ox + 14.0, gap_btn_y, w - 28.0, 26.0, t.border);
        renderer.draw_text(
            pixmap,
            "cat ~/.soma/gaps.log",
            ox + w / 2.0 - 50.0, gap_btn_y + 7.0, 110.0, 8.5, t.text_secondary,
        );
    }

    // ──────────────────────────────────────────────
    //  Rendering
    // ──────────────────────────────────────────────

    pub fn render(&mut self, renderer: &mut Renderer, pixmap: &mut Pixmap, ox: f32, top_y: f32, height: f32) {
        let t = renderer.theme.clone();
        let w = SIDEBAR_WIDTH;

        // Background — glassy translucent panel
        renderer.fill_rect(pixmap, ox, top_y, w, height, t.bg_sidebar);
        
        // Deep drop shadow line on the left edge
        renderer.fill_rect(pixmap, ox - 2.0, top_y, 2.0, height, [0, 0, 0, 60]);
        // Crisp inner highlight on the left edge
        renderer.fill_rect(pixmap, ox, top_y, 1.0, height, [255, 255, 255, 12]);

        // ─── Title Bar ───
        // Sleeker dark translucent header strip
        renderer.fill_rect(pixmap, ox, top_y, w, 50.0, [10, 10, 12, 180]);
        // Soft bottom shadow and crisp highlight for header
        renderer.fill_rect(pixmap, ox, top_y + 49.0, w, 1.0, [0, 0, 0, 40]);
        renderer.fill_rect(pixmap, ox, top_y + 50.0, w, 1.0, [255, 255, 255, 8]);

        // Agent name — brighter, slightly larger for premium feel
        renderer.draw_text(pixmap, "Soma", ox + 20.0, top_y + 18.0, 80.0, 14.0, t.text_primary);

        // Status dot + label — right-aligned
        let status_color = match self.status {
            AgentStatus::Idle => t.success,
            AgentStatus::Thinking => t.accent,
            AgentStatus::AwaitingApproval => t.warning,
            AgentStatus::Executing => [255, 140, 60, 255], 
            AgentStatus::Completed => t.success,
            AgentStatus::Error => t.error,
        };
        let status_text = format!("{}", self.status);
        
        // Glowing dot indicator
        renderer.fill_rounded_rect(pixmap, ox + w - 100.0, top_y + 20.0, 8.0, 8.0, 4.0, [status_color[0], status_color[1], status_color[2], 60]);
        renderer.fill_rounded_rect(pixmap, ox + w - 98.0, top_y + 22.0, 4.0, 4.0, 2.0, status_color);
        
        renderer.draw_text(pixmap, &status_text, ox + w - 86.0, top_y + 18.0, 78.0, 11.0, status_color);

        // ─── Tab Bar — VS Code editor tab style (3 equal tabs) ───
        let tab_y = top_y + 44.0;
        let tab_h = 26.0;
        let third = w / 3.0;
        renderer.fill_rect(pixmap, ox, tab_y, w, tab_h, [0, 0, 0, 20]);

        // Chat tab
        if self.tab == SidebarTab::Chat {
            renderer.fill_rect(pixmap, ox, tab_y, third, tab_h, t.bg_sidebar);
            renderer.fill_rect(pixmap, ox, tab_y, third, 1.0, t.accent);
            renderer.draw_text(pixmap, "Chat", ox + third / 2.0 - 14.0, tab_y + 8.0, 60.0, 10.0, t.text_primary);
        } else {
            renderer.draw_text(pixmap, "Chat", ox + third / 2.0 - 14.0, tab_y + 8.0, 60.0, 10.0, t.text_muted);
        }
        renderer.fill_rect(pixmap, ox + third, tab_y + 6.0, 1.0, tab_h - 12.0, t.border);
        // Settings tab
        if self.tab == SidebarTab::Settings {
            renderer.fill_rect(pixmap, ox + third, tab_y, third, tab_h, t.bg_sidebar);
            renderer.fill_rect(pixmap, ox + third, tab_y, third, 1.0, t.accent);
            renderer.draw_text(pixmap, "Settings", ox + third + third / 2.0 - 22.0, tab_y + 8.0, 80.0, 10.0, t.text_primary);
        } else {
            renderer.draw_text(pixmap, "Settings", ox + third + third / 2.0 - 22.0, tab_y + 8.0, 80.0, 10.0, t.text_muted);
        }
        renderer.fill_rect(pixmap, ox + third * 2.0, tab_y + 6.0, 1.0, tab_h - 12.0, t.border);
        // Registry tab
        if self.tab == SidebarTab::Registry {
            renderer.fill_rect(pixmap, ox + third * 2.0, tab_y, third, tab_h, t.bg_sidebar);
            renderer.fill_rect(pixmap, ox + third * 2.0, tab_y, third, 1.0, t.accent);
            renderer.draw_text(pixmap, "Registry", ox + third * 2.0 + third / 2.0 - 22.0, tab_y + 8.0, 80.0, 10.0, t.text_primary);
        } else {
            renderer.draw_text(pixmap, "Registry", ox + third * 2.0 + third / 2.0 - 22.0, tab_y + 8.0, 80.0, 10.0, t.text_muted);
        }
        renderer.fill_rect(pixmap, ox, tab_y + tab_h, w, 1.0, t.border);

        // ─── Content Area (routed by tab) ───
        let content_y = top_y + 44.0 + tab_h + 1.0; // below tab bar

        if self.tab == SidebarTab::Settings {
            self.render_settings_tab(renderer, pixmap, ox, content_y, top_y + height, &t);
            return;
        }

        if self.tab == SidebarTab::Registry {
            self.render_registry_tab(renderer, pixmap, ox, content_y, top_y + height, &t);
            return;
        }

        let content_h = (top_y + height) - content_y - 68.0;

        if self.messages.is_empty() {
            let cy = content_y + content_h / 2.0 - 40.0;
            renderer.draw_text(pixmap, "Soma Agent", ox + 14.0, cy, w - 28.0, 12.0, t.text_secondary);
            renderer.draw_text(pixmap, "Describe a task. I'll plan it,", ox + 14.0, cy + 22.0, w - 28.0, 10.0, t.text_muted);
            renderer.draw_text(pixmap, "you approve, then I execute it.", ox + 14.0, cy + 36.0, w - 28.0, 10.0, t.text_muted);
        } else {
            // Calculate total content height
            let mut total_h = 0.0_f32;
            for msg in &self.messages {
                total_h += message_height(msg) + MSG_PAD;
            }

            // Clamp scroll
            let max_scroll = (total_h - content_h + 16.0).max(0.0);
            self.scroll_offset = self.scroll_offset.min(max_scroll);

            // Auto-scroll near bottom
            if max_scroll - self.scroll_offset < 40.0 {
                self.scroll_offset = max_scroll;
            }

            let scroll = self.scroll_offset;
            let mut accumulated = 0.0_f32;

            for msg in &self.messages {
                let h = message_height(msg);
                let msg_y = content_y + 4.0 + accumulated - scroll;
                accumulated += h + MSG_PAD;

                // Clip to viewport
                if msg_y + h < content_y { continue; }
                if msg_y > content_y + content_h { break; }

                self.render_message(renderer, pixmap, msg, msg_y, ox, w, &t);
            }
        }

        // ─── Input Area — VS Code chat input style ───
        let input_y = top_y + height - 68.0;
        renderer.fill_rect(pixmap, ox, input_y - 1.0, w, 1.0, t.border);
        renderer.fill_rect(pixmap, ox, input_y, w, 68.0, [0, 0, 0, 15]);

        // Input box — flat with 1px border (VS Code style, not rounded pill)
        let input_border = if self.status == AgentStatus::AwaitingApproval { t.warning } else { [60, 60, 60, 255] };
        renderer.fill_rect(pixmap, ox + 10.0, input_y + 10.0, w - 52.0, 38.0, t.bg_input);
        renderer.stroke_rect(pixmap, ox + 10.0, input_y + 10.0, w - 52.0, 38.0, input_border);

        let placeholder = match self.status {
            AgentStatus::AwaitingApproval => "Enter to approve, Esc to reject",
            _ => "Ask anything...",
        };
        let display = if self.input_text.is_empty() {
            placeholder.to_string()
        } else if self.cursor_visible {
            format!("{}|", self.input_text)
        } else {
            self.input_text.clone()
        };
        let text_color = if self.input_text.is_empty() { t.text_muted } else { t.text_primary };
        renderer.draw_text(pixmap, &display, ox + 16.0, input_y + 22.0, w - 68.0, 10.0, text_color);

        // Send button — VS Code primary action style (flat rectangle)
        let btn_color = if self.status == AgentStatus::AwaitingApproval { t.success } else { t.accent };
        renderer.fill_rect(pixmap, ox + w - 38.0, input_y + 10.0, 28.0, 38.0, btn_color);
        let btn_icon = if self.status == AgentStatus::AwaitingApproval { "OK" } else { ">" };
        renderer.draw_text(pixmap, btn_icon, ox + w - 32.0, input_y + 22.0, 20.0, 11.0, [255, 255, 255, 255]);
    }

    fn render_message(&self, renderer: &mut Renderer, pixmap: &mut Pixmap, msg: &ChatMessage, y: f32, ox: f32, w: f32, t: &crate::renderer::Theme) {
        match msg {
            ChatMessage::UserInput { text } => {
                // User message: right-aligned, VS Code chat user bubble
                let max_w = (w - 32.0).min(300.0);
                let text_display = truncate_str(text, 60);
                let x = ox + w - max_w - 10.0;
                renderer.fill_rect(pixmap, x, y, max_w, 28.0, [0, 122, 204, 28]);
                renderer.fill_rect(pixmap, x, y, 2.0, 28.0, t.accent); // left accent bar
                renderer.draw_text(pixmap, &text_display, x + 10.0, y + 8.0, max_w - 14.0, 10.0, t.text_primary);
            }

            ChatMessage::Thinking => {
                // Agent thinking — inline, no box, just muted italic-style text
                renderer.draw_text(pixmap, "Thinking...", ox + 14.0, y + 6.0, 160.0, 10.0, t.text_muted);
            }

            ChatMessage::PlanProposal { plan, .. } => {
                let card_w = w - 20.0;
                let step_h = plan.steps.len() as f32 * 18.0;
                let card_h = 32.0 + step_h.max(18.0) + 18.0; // +18 for workflow link

                // Card: subtle bg + left border in risk color
                let risk_color = match plan.risk_level {
                    RiskLevel::Low => t.success, RiskLevel::Medium => t.warning, RiskLevel::High => t.error
                };
                renderer.fill_rect(pixmap, ox + 10.0, y, card_w, card_h, t.bg_surface);
                renderer.fill_rect(pixmap, ox + 10.0, y, 2.0, card_h, risk_color);

                // Intent label
                let intent = if plan.description.is_empty() { &plan.intent } else { &plan.description };
                renderer.draw_text(pixmap, &truncate_str(intent, 48), ox + 18.0, y + 7.0, card_w - 28.0, 10.0, t.text_primary);

                // Steps list
                let mut sy = y + 24.0;
                for (i, step) in plan.steps.iter().enumerate() {
                    let label = format!("  {}  {}.{}", i + 1, step.capability, step.action);
                    renderer.draw_text(pixmap, &label, ox + 18.0, sy, card_w - 28.0, 9.0, t.text_secondary);
                    sy += 18.0;
                }

                // "Save as workflow" link
                renderer.draw_text(pixmap, "[^ Save as workflow]", ox + 18.0, sy + 2.0, card_w - 28.0, 8.0, t.accent);
            }

            ChatMessage::ExecutionDone { intent, success_count, total, results, preview } => {
                let result_lines = format_all_results(results);
                let visible_lines: Vec<&str> = result_lines.iter().map(|s| s.as_str()).take(15).collect();
                let more_count = result_lines.len().saturating_sub(15);

                let (thumb_w, thumb_h) = match preview {
                    PreviewData::Image { width, height, .. } => (*width as f32, *height as f32),
                    PreviewData::None => (0.0, 0.0),
                };

                let header_h = 26.0;
                let thumb_section_h = if thumb_h > 0.0 { thumb_h + 10.0 } else { 0.0 };
                let lines_h = visible_lines.len() as f32 * 14.0;
                let more_h = if more_count > 0 { 14.0 } else { 0.0 };
                let card_h = header_h + thumb_section_h + lines_h + more_h + 8.0;
                let card_w = w - 20.0;

                // Card: left border in success/warning color (VS Code diff-style)
                let line_color = if *success_count == *total { t.success } else { t.warning };
                renderer.fill_rect(pixmap, ox + 10.0, y, card_w, card_h, t.bg_surface);
                renderer.fill_rect(pixmap, ox + 10.0, y, 2.0, card_h, line_color);

                // Header
                let icon = if *success_count == *total { "v" } else { "!" };
                let header = format!("{} {} ({}/{})", icon, truncate_str(intent, 32), success_count, total);
                renderer.draw_text(pixmap, &header, ox + 18.0, y + 7.0, card_w - 24.0, 10.0, line_color);

                // Separator
                renderer.fill_rect(pixmap, ox + 18.0, y + 23.0, card_w - 24.0, 1.0, t.border);

                let mut ry = y + header_h;

                // Image thumbnail
                if let PreviewData::Image { pixels, width, height } = preview {
                    if let Some(mut thumb) = tiny_skia::Pixmap::new(*width, *height) {
                        if thumb.data_mut().len() == pixels.len() {
                            thumb.data_mut().copy_from_slice(pixels);
                            // Draw a subtle border around the thumbnail
                            renderer.fill_rounded_rect(
                                pixmap,
                                ox + 16.0, ry - 2.0,
                                thumb_w + 4.0, thumb_h + 4.0,
                                4.0, [255, 255, 255, 8],
                            );
                            pixmap.draw_pixmap(
                                (ox + 18.0) as i32,
                                ry as i32,
                                thumb.as_ref(),
                                &tiny_skia::PixmapPaint::default(),
                                tiny_skia::Transform::identity(),
                                None,
                            );
                        }
                    }
                    ry += thumb_h + 10.0;
                }

                // Result text lines
                for line in &visible_lines {
                    renderer.draw_text(pixmap, line, ox + 18.0, ry, card_w - 28.0, 9.0, t.text_secondary);
                    ry += 15.0;
                }
                if more_count > 0 {
                    renderer.draw_text(pixmap, &format!("  ... {} more lines", more_count), ox + 18.0, ry, card_w - 28.0, 9.0, t.text_muted);
                    ry += 14.0;
                }

                // "Save as workflow" link
                renderer.draw_text(pixmap, "[^ Save as workflow]", ox + 18.0, ry + 2.0, card_w - 28.0, 8.0, t.accent);
            }

            ChatMessage::AgentError { message } => {
                // Error card: red left border (VS Code problem panel style)
                let card_w = w - 20.0;
                renderer.fill_rect(pixmap, ox + 10.0, y, card_w, 36.0, t.bg_surface);
                renderer.fill_rect(pixmap, ox + 10.0, y, 2.0, 36.0, t.error);
                renderer.draw_text(pixmap, "Error", ox + 18.0, y + 5.0, 60.0, 9.0, t.error);
                renderer.draw_text(pixmap, &truncate_str(message, 55), ox + 18.0, y + 19.0, card_w - 28.0, 9.0, t.text_secondary);
            }
        }
    }

    pub fn render_approval_overlay(&mut self, renderer: &mut Renderer, pixmap: &mut Pixmap, width: f32, height: f32) {
        let plan = match &self.current_plan {
            Some(p) => p.clone(),
            None => return,
        };
        let t = renderer.theme.clone();

        // Backdrop
        renderer.fill_rect(pixmap, 0.0, 0.0, width, height, [0, 0, 0, 160]);

        // Modal — VS Code quick input / confirmation dialog style
        let step_count = plan.steps.len().max(1);
        let mh = (160.0 + step_count as f32 * 40.0).min(height - 60.0);
        let mw = 380.0_f32.min(width - 40.0);
        let mx = (width - mw) / 2.0;
        let my = (height - mh) / 2.0;

        renderer.fill_rect(pixmap, mx, my, mw, mh, [37, 37, 38, 252]);
        renderer.stroke_rect(pixmap, mx, my, mw, mh, t.border);

        renderer.draw_text(pixmap, "Approve Action?", mx + 18.0, my + 14.0, mw - 36.0, 13.0, t.text_primary);
        renderer.fill_rect(pixmap, mx + 18.0, my + 38.0, mw - 36.0, 1.0, t.border);

        // Description
        let desc = if plan.description.is_empty() { &plan.intent } else { &plan.description };
        renderer.draw_text(pixmap, desc, mx + 18.0, my + 48.0, mw - 36.0, 10.0, t.text_secondary);

        // Risk badge
        let (risk_color, risk_label) = match plan.risk_level {
            RiskLevel::Low => (t.success, "Low Risk"),
            RiskLevel::Medium => (t.warning, "Med Risk"),
            RiskLevel::High => (t.error, "High Risk"),
        };
        renderer.fill_rounded_rect(pixmap, mx + 18.0, my + 68.0, 80.0, 18.0, 9.0, [255, 255, 255, 8]);
        renderer.draw_text(pixmap, risk_label, mx + 26.0, my + 72.0, 64.0, 9.0, risk_color);

        // Steps
        let mut sy = my + 96.0;
        for (i, step) in plan.steps.iter().enumerate() {
            if sy > my + mh - 54.0 { break; }

            renderer.fill_rounded_rect(pixmap, mx + 18.0, sy, 18.0, 18.0, 9.0, t.accent);
            renderer.draw_text(pixmap, &format!("{}", i + 1), mx + 23.0, sy + 3.0, 10.0, 10.0, [255, 255, 255, 255]);

            let action_label = format!("{}.{}", step.capability, step.action);
            renderer.draw_text(pixmap, &action_label, mx + 42.0, sy + 2.0, mw - 64.0, 10.0, [196, 181, 253, 255]);

            if !step.params.is_null() && step.params != serde_json::json!({}) {
                let params = format_params_inline(&step.params);
                renderer.draw_text(pixmap, &truncate_str(&params, 45), mx + 42.0, sy + 18.0, mw - 64.0, 8.0, t.text_muted);
            }

            if !step.description.is_empty() {
                renderer.draw_text(pixmap, &truncate_str(&step.description, 45), mx + 42.0, sy + 30.0, mw - 64.0, 8.0, t.text_secondary);
            }

            sy += 46.0;
        }

        // Bottom buttons
        let btn_y = my + mh - 46.0;
        renderer.fill_rect(pixmap, mx + 18.0, btn_y - 6.0, mw - 36.0, 1.0, t.border);

        let half_w = (mw - 48.0) / 2.0;
        renderer.fill_rounded_rect(pixmap, mx + 18.0, btn_y, half_w, 32.0, 8.0, [255, 255, 255, 10]);
        renderer.draw_text(pixmap, "X  Reject (Esc)", mx + 28.0, btn_y + 9.0, half_w - 16.0, 10.0, t.text_secondary);

        let ax = mx + 24.0 + half_w;
        renderer.fill_rounded_rect(pixmap, ax, btn_y, half_w, 32.0, 8.0, t.accent);
        renderer.draw_text(pixmap, "v  Approve (Enter)", ax + 10.0, btn_y + 9.0, half_w - 16.0, 10.0, [255, 255, 255, 255]);
    }
}

// ──────────────────────────────────────────────
//  Helpers
// ──────────────────────────────────────────────

fn message_height(msg: &ChatMessage) -> f32 {
    match msg {
        ChatMessage::UserInput { .. } => 38.0,
        ChatMessage::Thinking => 34.0,
        ChatMessage::PlanProposal { plan, .. } => {
            36.0 + (plan.steps.len() as f32 * 22.0).max(22.0) + 8.0 + 18.0 // +18 workflow link
        }
        ChatMessage::ExecutionDone { results, preview, .. } => {
            let lines = format_all_results(results);
            let visible = lines.len().min(15);
            let more_h = if lines.len() > 15 { 15.0 } else { 0.0 };
            let thumb_h = match preview {
                PreviewData::Image { height, .. } => *height as f32 + 10.0,
                PreviewData::None => 0.0,
            };
            28.0 + thumb_h + (visible as f32 * 15.0) + more_h + 16.0 + 18.0 // +18 workflow link
        }
        ChatMessage::AgentError { .. } => 46.0,
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}...", truncated)
    } else {
        s.to_string()
    }
}

/// Format all results into display lines, using ASCII-safe characters
fn format_all_results(results: &[CapabilityResult]) -> Vec<String> {
    let mut lines = Vec::new();
    for r in results {
        if !r.success {
            if let Some(err) = &r.error {
                lines.push(format!("  ! {}", truncate_str(&err.message, 50)));
            }
            continue;
        }
        format_result_data(&r.data, &mut lines);
    }
    lines
}

fn format_result_data(data: &serde_json::Value, lines: &mut Vec<String>) {
    // Image base64 — rendered as thumbnail, nothing to show as text
    if data.get("type").and_then(|v| v.as_str()) == Some("image_base64") {
        let path = data.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let bytes = data.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
        let filename = std::path::Path::new(path)
            .file_name().and_then(|n| n.to_str()).unwrap_or(path);
        lines.push(format!("  {} ({})", filename, format_size(bytes)));
        return;
    }

    // File listings — tree-style with sizes
    if let Some(entries) = data.get("entries").and_then(|v| v.as_array()) {
        let count = data.get("count").and_then(|v| v.as_u64()).unwrap_or(entries.len() as u64);
        let path = data.get("path").and_then(|v| v.as_str()).unwrap_or("");
        lines.push(format!("  {} ({} items)", path, count));

        // Sort: dirs first, then files alphabetically
        let mut sorted: Vec<&serde_json::Value> = entries.iter().collect();
        sorted.sort_by(|a, b| {
            let a_dir = a.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false);
            let b_dir = b.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false);
            b_dir.cmp(&a_dir)
        });

        let show = sorted.len().min(15);
        for (i, e) in sorted.iter().take(show).enumerate() {
            let name = e.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let is_dir = e.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false);
            let size = e.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            let connector = if i == show - 1 && count <= 15 { "\\--" } else { "|--" };
            if is_dir {
                lines.push(format!("  {} {}/", connector, name));
            } else {
                lines.push(format!("  {} {}    {}", connector, name, format_size(size)));
            }
        }
        if count > 15 {
            lines.push(format!("  \\-- +{} more...", count - 15));
        }
        return;
    }

    // Process listings
    if let Some(procs) = data.get("processes").and_then(|v| v.as_array()) {
        for p in procs.iter().take(8) {
            let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let pid = p.get("pid").and_then(|v| v.as_i64()).unwrap_or(0);
            lines.push(format!("  {} (pid {})", name, pid));
        }
        if procs.len() > 8 { lines.push(format!("  +{} more...", procs.len() - 8)); }
        return;
    }

    // Network interfaces
    if let Some(interfaces) = data.get("interfaces").and_then(|v| v.as_array()) {
        for iface in interfaces.iter().take(8) {
            let name = iface.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let addrs = iface.get("addresses").and_then(|v| v.as_array());
            let addr_str = addrs.map(|a| {
                a.iter().filter_map(|v| v.as_str()).take(2).collect::<Vec<_>>().join(", ")
            }).unwrap_or_default();
            lines.push(format!("  {}: {}", name, addr_str));
        }
        return;
    }

    // Packages list
    if let Some(pkgs) = data.get("packages").and_then(|v| v.as_array()) {
        let count = data.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        lines.push(format!("  {} packages", count));
        for p in pkgs.iter().take(8) {
            if let Some(name) = p.as_str() {
                lines.push(format!("    {}", name));
            }
        }
        if count > 8 { lines.push(format!("  +{} more...", count - 8)); }
        return;
    }

    // DNS results
    if let Some(addrs) = data.get("addresses").and_then(|v| v.as_array()) {
        let hostname = data.get("hostname").and_then(|v| v.as_str()).unwrap_or("?");
        lines.push(format!("  {}", hostname));
        for a in addrs.iter().take(4) {
            if let Some(s) = a.as_str() { lines.push(format!("    -> {}", s)); }
        }
        return;
    }

    // Ping output
    if let Some(output) = data.get("output").and_then(|v| v.as_str()) {
        if data.get("host").is_some() {
            let stats = data.get("stats").and_then(|v| v.as_str()).unwrap_or("");
            if !stats.is_empty() {
                lines.push(format!("  {}", truncate_str(stats, 60)));
            } else {
                for line in output.lines().take(4) {
                    lines.push(format!("  {}", truncate_str(line, 55)));
                }
            }
            return;
        }
    }

    // Port check
    if let Some(status) = data.get("status").and_then(|v| v.as_str()) {
        let host = data.get("host").and_then(|v| v.as_str()).unwrap_or("?");
        let port = data.get("port").and_then(|v| v.as_u64()).unwrap_or(0);
        lines.push(format!("  {}:{} -> {}", host, port, status));
        return;
    }

    // Curl result
    if let Some(sc) = data.get("status_code").and_then(|v| v.as_str()) {
        let url = data.get("url").and_then(|v| v.as_str()).unwrap_or("?");
        let time = data.get("time_seconds").and_then(|v| v.as_str()).unwrap_or("?");
        lines.push(format!("  {} -> {} ({}s)", truncate_str(url, 35), sc, time));
        return;
    }

    // File content — with line numbers and file stats
    if let Some(content) = data.get("content").and_then(|v| v.as_str()) {
        let path = data.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let filename = std::path::Path::new(path)
            .file_name().and_then(|n| n.to_str()).unwrap_or(path);
        let total_lines = data.get("lines").and_then(|v| v.as_u64()).unwrap_or(content.lines().count() as u64);
        let byte_size = data.get("bytes").and_then(|v| v.as_u64()).unwrap_or(content.len() as u64);
        lines.push(format!("  {} -- {} lines, {}", filename, total_lines, format_size(byte_size)));
        lines.push(format!("  {}", "-".repeat(38)));
        for (i, line) in content.lines().enumerate().take(15) {
            lines.push(format!("  {:>3} | {}", i + 1, truncate_str(line, 46)));
        }
        if total_lines > 15 {
            lines.push(format!("  ... {} more lines", total_lines - 15));
        }
        return;
    }

    // Hostname
    if let Some(h) = data.get("hostname").and_then(|v| v.as_str()) {
        lines.push(format!("  hostname: {}", h));
        return;
    }

    // Uptime
    if let Some(u) = data.get("uptime_human").and_then(|v| v.as_str()) {
        lines.push(format!("  uptime: {}", u));
        return;
    }

    // Generic key-value fallback
    if let Some(obj) = data.as_object() {
        for (k, v) in obj.iter().take(8) {
            let val = match v {
                serde_json::Value::String(s) => truncate_str(s, 45),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            lines.push(format!("  {}: {}", k, val));
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Decode the first image_base64 result into a premultiplied-RGBA thumbnail
/// ready for tiny-skia rendering (max 220×150, aspect-preserved).
fn decode_image_preview(results: &[CapabilityResult]) -> PreviewData {
    for r in results {
        if !r.success { continue; }
        if r.data.get("type").and_then(|v| v.as_str()) != Some("image_base64") { continue; }
        let encoded = match r.data.get("data").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let bytes = match base64::engine::general_purpose::STANDARD.decode(encoded) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let img = match image::load_from_memory(&bytes) {
            Ok(i) => i,
            Err(_) => continue,
        };
        let orig_w = img.width();
        let orig_h = img.height();
        let scale = (220.0 / orig_w as f32).min(150.0 / orig_h as f32).min(1.0);
        let new_w = ((orig_w as f32 * scale) as u32).max(1);
        let new_h = ((orig_h as f32 * scale) as u32).max(1);
        let resized = img.resize(new_w, new_h, image::imageops::FilterType::Triangle);
        let rgba = resized.to_rgba8();
        // Convert straight alpha → premultiplied alpha for tiny-skia
        let mut premul: Vec<u8> = Vec::with_capacity((new_w * new_h * 4) as usize);
        for px in rgba.pixels() {
            let a = px[3] as f32 / 255.0;
            premul.push((px[0] as f32 * a) as u8);
            premul.push((px[1] as f32 * a) as u8);
            premul.push((px[2] as f32 * a) as u8);
            premul.push(px[3]);
        }
        return PreviewData::Image { pixels: premul, width: new_w, height: new_h };
    }
    PreviewData::None
}

/// Wrap text into lines of at most `max_chars` characters, breaking on whitespace.
fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(10);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current = word.to_string();
            } else if current.len() + 1 + word.len() <= max_chars {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        if !current.is_empty() || paragraph.is_empty() {
            lines.push(current);
        }
    }
    lines
}

fn format_params_inline(params: &serde_json::Value) -> String {
    if let Some(obj) = params.as_object() {
        let parts: Vec<String> = obj.iter().map(|(k, v)| {
            let val = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            format!("{}={}", k, val)
        }).collect();
        parts.join("  ")
    } else {
        params.to_string()
    }
}

use soma_common::{AgentMessage, AgentStatus, CapabilityResult, CompositorMessage, TaskPlan, RiskLevel};
use crate::renderer::Renderer;
use tiny_skia::Pixmap;

/// Sidebar panel width in pixels
pub const SIDEBAR_WIDTH: f32 = 380.0;

const MAX_HISTORY: usize = 100;

// ──────────────────────────────────────────────
//  Chat message types
// ──────────────────────────────────────────────

#[derive(Clone)]
pub enum ChatMessage {
    /// User's natural language input
    UserInput { text: String },
    /// Agent thinking indicator
    Thinking,
    /// Agent returned a plan for approval
    PlanProposal {
        id: String,
        plan: TaskPlan,
    },
    /// Step completed during execution
    StepProgress {
        step_index: usize,
        total_steps: usize,
        result: CapabilityResult,
    },
    /// All steps finished
    ExecutionDone {
        success_count: usize,
        total: usize,
        results: Vec<CapabilityResult>,
    },
    /// Error from agent
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
        }
    }

    pub fn on_char(&mut self, c: char) {
        if matches!(self.status, AgentStatus::Idle | AgentStatus::Completed | AgentStatus::Error) {
            self.input_text.push(c);
        }
    }

    pub fn on_backspace(&mut self) {
        self.input_text.pop();
    }

    /// Scroll the sidebar
    pub fn scroll(&mut self, delta: f32) {
        self.scroll_offset = (self.scroll_offset - delta).max(0.0);
    }

    /// Scroll to bottom of conversation
    pub fn scroll_to_bottom(&mut self) {
        // Estimate content height (rough)
        let estimated = self.messages.len() as f32 * 60.0;
        self.scroll_offset = estimated.max(0.0);
    }

    pub fn on_submit(&mut self) -> Option<CompositorMessage> {
        match self.status {
            AgentStatus::Idle | AgentStatus::Completed | AgentStatus::Error => {
                let text = self.input_text.trim().to_string();
                if text.is_empty() {
                    return None;
                }
                // Add user message
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
        } else {
            None
        }
    }

    pub fn on_reject(&mut self) -> Option<CompositorMessage> {
        if let Some(id) = self.current_plan_id.take() {
            self.current_plan = None;
            self.status = AgentStatus::Idle;
            // Remove the thinking message if still there
            self.messages.retain(|m| !matches!(m, ChatMessage::Thinking));
            Some(CompositorMessage::Reject { id })
        } else {
            None
        }
    }

    pub fn handle_agent_message(&mut self, msg: AgentMessage) {
        match msg {
            AgentMessage::TaskPlanReady { id, plan } => {
                // Remove thinking indicator
                self.messages.retain(|m| !matches!(m, ChatMessage::Thinking));
                self.current_step_total = plan.steps.len();
                self.messages.push(ChatMessage::PlanProposal { id: id.clone(), plan: plan.clone() });
                self.status = AgentStatus::AwaitingApproval;
                self.current_plan = Some(plan);
                self.current_plan_id = Some(id);
                self.scroll_to_bottom();
            }
            AgentMessage::StepResult { step_index, result, .. } => {
                self.messages.push(ChatMessage::StepProgress {
                    step_index,
                    total_steps: self.current_step_total,
                    result,
                });
                self.scroll_to_bottom();
            }
            AgentMessage::ExecutionComplete { results, .. } => {
                self.status = AgentStatus::Completed;
                self.current_plan = None;
                let ok = results.iter().filter(|r| r.success).count();
                self.messages.push(ChatMessage::ExecutionDone {
                    success_count: ok,
                    total: results.len(),
                    results,
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

        // Cap history
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
    }

    // ──────────────────────────────────────────────
    //  Rendering
    // ──────────────────────────────────────────────

    pub fn render(&mut self, renderer: &mut Renderer, pixmap: &mut Pixmap, height: f32) {
        let t = renderer.theme.clone();
        let w = SIDEBAR_WIDTH;

        // Background
        renderer.fill_rect(pixmap, 0.0, 0.0, w, height, t.bg_sidebar);
        renderer.fill_rect(pixmap, w - 1.0, 0.0, 1.0, height, t.border);

        // ─── Title Bar ───
        renderer.fill_rect(pixmap, 0.0, 0.0, w, 48.0, [255, 255, 255, 5]);
        renderer.fill_rect(pixmap, 0.0, 47.0, w, 1.0, t.border);
        renderer.draw_text(pixmap, "◈", 16.0, 14.0, 30.0, 18.0, t.accent);
        renderer.draw_text(pixmap, "SOMA", 38.0, 16.0, 80.0, 12.0, t.text_secondary);

        // Status indicator
        let status_color = match self.status {
            AgentStatus::Idle => t.success,
            AgentStatus::Thinking => t.accent,
            AgentStatus::AwaitingApproval => t.warning,
            AgentStatus::Executing => [249, 115, 22, 255],
            AgentStatus::Completed => t.success,
            AgentStatus::Error => t.error,
        };
        renderer.fill_rounded_rect(pixmap, w - 130.0, 20.0, 6.0, 6.0, 3.0, status_color);
        renderer.draw_text(pixmap, &format!("{}", self.status), w - 118.0, 17.0, 110.0, 10.0, status_color);

        // ─── Content Area ───
        let content_y = 56.0;
        let content_h = height - 56.0 - 80.0;

        // Welcome screen
        if self.messages.is_empty() {
            let cy = content_y + content_h / 2.0 - 70.0;
            renderer.draw_text(pixmap, "◈", w / 2.0 - 15.0, cy, 40.0, 36.0, t.accent);
            renderer.draw_text(pixmap, "Welcome to Soma", w / 2.0 - 75.0, cy + 50.0, 200.0, 16.0, t.text_primary);
            renderer.draw_text(pixmap, "Describe what you want to do.", 28.0, cy + 78.0, w - 56.0, 11.0, t.text_muted);
            renderer.draw_text(pixmap, "I'll create a plan for your approval.", 28.0, cy + 96.0, w - 56.0, 11.0, t.text_muted);
        } else {
            // Render chat messages with scrolling
            let mut y = content_y + 8.0;
            let scroll = self.scroll_offset;

            // Calculate total content height for scroll clamping
            let mut total_h = 0.0_f32;
            for msg in &self.messages {
                total_h += message_height(msg);
            }

            // Clamp scroll offset
            let max_scroll = (total_h - content_h + 20.0).max(0.0);
            self.scroll_offset = self.scroll_offset.min(max_scroll);
            let scroll = self.scroll_offset;

            // Auto-scroll: if near bottom, snap to bottom
            if max_scroll - scroll < 30.0 {
                self.scroll_offset = max_scroll;
            }

            // Start from scroll offset
            let mut accumulated = 0.0_f32;
            for msg in &self.messages {
                let h = message_height(msg);
                accumulated += h;

                // Skip messages above viewport
                if accumulated < scroll {
                    continue;
                }

                let msg_y = y + accumulated - scroll - h;

                // Skip messages below viewport
                if msg_y > content_y + content_h {
                    break;
                }

                // Only render if visible
                if msg_y + h >= content_y {
                    self.render_message(renderer, pixmap, msg, msg_y, w, &t);
                }
            }
        }

        // ─── Input Area ───
        let input_y = height - 72.0;
        renderer.fill_rect(pixmap, 0.0, input_y - 1.0, w, 1.0, t.border);
        renderer.fill_rect(pixmap, 0.0, input_y, w, 72.0, [255, 255, 255, 5]);

        renderer.fill_rounded_rect(pixmap, 12.0, input_y + 12.0, w - 64.0, 38.0, 10.0, t.bg_input);
        renderer.stroke_rect(pixmap, 12.0, input_y + 12.0, w - 64.0, 38.0, t.border);

        let display = if self.input_text.is_empty() {
            match self.status {
                AgentStatus::AwaitingApproval => "Press Enter to approve, Esc to reject".to_string(),
                _ => "Ask me anything…".to_string(),
            }
        } else if self.cursor_visible {
            format!("{}|", self.input_text)
        } else {
            self.input_text.clone()
        };
        let text_color = if self.input_text.is_empty() { t.text_muted } else { t.text_primary };
        renderer.draw_text(pixmap, &display, 22.0, input_y + 22.0, w - 90.0, 12.0, text_color);

        // Send button
        let btn_color = if self.status == AgentStatus::AwaitingApproval { t.success } else { t.accent };
        renderer.fill_rounded_rect(pixmap, w - 46.0, input_y + 14.0, 34.0, 34.0, 8.0, btn_color);
        let btn_icon = if self.status == AgentStatus::AwaitingApproval { "✓" } else { "↑" };
        renderer.draw_text(pixmap, btn_icon, w - 38.0, input_y + 22.0, 20.0, 16.0, [255, 255, 255, 255]);
    }

    fn render_message(&self, renderer: &mut Renderer, pixmap: &mut Pixmap, msg: &ChatMessage, y: f32, w: f32, t: &crate::renderer::Theme) {
        match msg {
            ChatMessage::UserInput { text } => {
                // Right-aligned user bubble
                let bubble_w = (w - 48.0).min(280.0);
                let x = w - bubble_w - 12.0;
                renderer.fill_rounded_rect(pixmap, x, y, bubble_w, 32.0, 12.0, t.accent);
                let truncated = if text.len() > 50 { format!("{}…", &text[..50]) } else { text.clone() };
                renderer.draw_text(pixmap, &truncated, x + 12.0, y + 9.0, bubble_w - 24.0, 11.0, [255, 255, 255, 255]);
            }

            ChatMessage::Thinking => {
                // Inline thinking indicator
                renderer.fill_rounded_rect(pixmap, 12.0, y, 160.0, 28.0, 8.0, t.bg_surface);
                renderer.draw_text(pixmap, "● ● ●  Thinking…", 22.0, y + 7.0, 140.0, 11.0, t.accent);
            }

            ChatMessage::PlanProposal { plan, .. } => {
                // Left-aligned plan card
                let card_w = w - 24.0;
                let card_h = 24.0 + (plan.steps.len() as f32 * 26.0).max(26.0) + 16.0;

                renderer.fill_rounded_rect(pixmap, 12.0, y, card_w, card_h, 10.0, t.bg_surface);

                // Intent header
                let intent = if plan.description.is_empty() { &plan.intent } else { &plan.description };
                let risk_color = match plan.risk_level {
                    RiskLevel::Low => t.success,
                    RiskLevel::Medium => t.warning,
                    RiskLevel::High => t.error,
                };

                // Risk dot + intent text
                renderer.fill_rounded_rect(pixmap, 20.0, y + 10.0, 6.0, 6.0, 3.0, risk_color);
                let intent_short = if intent.len() > 45 { format!("{}…", &intent[..45]) } else { intent.clone() };
                renderer.draw_text(pixmap, &intent_short, 32.0, y + 7.0, card_w - 44.0, 11.0, t.text_primary);

                // Steps
                let mut sy = y + 28.0;
                for (i, step) in plan.steps.iter().enumerate() {
                    let label = format!("{}  {}.{}", i + 1, step.capability, step.action);
                    renderer.draw_text(pixmap, &label, 24.0, sy + 4.0, card_w - 36.0, 9.0, [196, 181, 253, 255]);
                    sy += 26.0;
                }
            }

            ChatMessage::StepProgress { step_index, total_steps, result } => {
                // Inline step result
                let icon = if result.success { "✓" } else { "✗" };
                let color = if result.success { t.success } else { t.error };
                let label = format!("{} Step {}/{}", icon, step_index + 1, total_steps);

                renderer.fill_rounded_rect(pixmap, 12.0, y, w - 24.0, 24.0, 6.0, [255, 255, 255, 4]);
                renderer.draw_text(pixmap, &label, 20.0, y + 5.0, 120.0, 10.0, color);

                // Compact result summary
                if result.success {
                    let summary = compact_result_summary(&result.data);
                    renderer.draw_text(pixmap, &summary, 140.0, y + 5.0, w - 168.0, 9.0, t.text_muted);
                }
            }

            ChatMessage::ExecutionDone { success_count, total, results } => {
                // Results card
                let mut card_h = 32.0_f32;
                for r in results {
                    if r.success {
                        let lines = format_result_lines(&r.data);
                        card_h += (lines.len() as f32 * 14.0).max(14.0) + 8.0;
                    }
                }
                card_h = card_h.min(250.0);

                renderer.fill_rounded_rect(pixmap, 12.0, y, w - 24.0, card_h, 10.0, [74, 222, 128, 10]);

                // Header
                let header = format!("Done — {}/{} succeeded", success_count, total);
                let header_color = if *success_count == *total { t.success } else { t.warning };
                renderer.draw_text(pixmap, &header, 20.0, y + 8.0, w - 44.0, 10.0, header_color);

                // Result lines
                let mut ry = y + 28.0;
                let max_ry = y + card_h - 8.0;
                for r in results {
                    if ry >= max_ry { break; }
                    if r.success {
                        let lines = format_result_lines(&r.data);
                        for line in lines.iter().take(8) {
                            if ry >= max_ry { break; }
                            renderer.draw_text(pixmap, line, 20.0, ry, w - 44.0, 9.0, t.text_secondary);
                            ry += 14.0;
                        }
                        if lines.len() > 8 {
                            renderer.draw_text(pixmap, &format!("  +{} more…", lines.len() - 8), 20.0, ry, w - 44.0, 9.0, t.text_muted);
                            ry += 14.0;
                        }
                        ry += 4.0;
                    }
                }
            }

            ChatMessage::AgentError { message } => {
                // Error card
                renderer.fill_rounded_rect(pixmap, 12.0, y, w - 24.0, 40.0, 8.0, [248, 113, 113, 15]);
                renderer.draw_text(pixmap, "⚠ Error", 20.0, y + 5.0, 80.0, 10.0, t.error);
                let err_short = if message.len() > 55 { format!("{}…", &message[..55]) } else { message.clone() };
                renderer.draw_text(pixmap, &err_short, 20.0, y + 21.0, w - 44.0, 9.0, t.error);
            }
        }
    }

    /// Render the HITL approval overlay
    pub fn render_approval_overlay(&mut self, renderer: &mut Renderer, pixmap: &mut Pixmap, width: f32, height: f32) {
        let plan = match &self.current_plan {
            Some(p) => p.clone(),
            None => return,
        };
        let t = renderer.theme.clone();

        // Backdrop
        renderer.fill_rect(pixmap, 0.0, 0.0, width, height, [0, 0, 0, 150]);

        // Dynamic modal height
        let step_count = plan.steps.len().max(1);
        let base_h = 180.0;
        let steps_h = step_count as f32 * 50.0;
        let mh = (base_h + steps_h).min(height - 60.0);

        let mw = 360.0_f32.min(width - 40.0);
        let mx = (width - mw) / 2.0;
        let my = (height - mh) / 2.0;

        // Card
        renderer.fill_rounded_rect(pixmap, mx, my, mw, mh, 16.0, [22, 22, 42, 250]);
        renderer.stroke_rect(pixmap, mx, my, mw, mh, t.border);

        // Header
        renderer.draw_text(pixmap, "Approve Action?", mx + 20.0, my + 18.0, mw - 40.0, 14.0, t.text_primary);

        // Divider
        renderer.fill_rect(pixmap, mx + 20.0, my + 44.0, mw - 40.0, 1.0, t.border);

        // Intent description
        let desc = if plan.description.is_empty() { &plan.intent } else { &plan.description };
        renderer.draw_text(pixmap, desc, mx + 20.0, my + 56.0, mw - 40.0, 11.0, t.text_secondary);

        // Risk badge
        let (risk_color, risk_label) = match plan.risk_level {
            RiskLevel::Low => (t.success, "Low Risk"),
            RiskLevel::Medium => (t.warning, "Medium Risk"),
            RiskLevel::High => (t.error, "High Risk"),
        };
        renderer.fill_rounded_rect(pixmap, mx + 20.0, my + 78.0, 90.0, 20.0, 10.0, [255, 255, 255, 8]);
        renderer.draw_text(pixmap, risk_label, mx + 30.0, my + 82.0, 70.0, 10.0, risk_color);

        // Steps
        let mut sy = my + 110.0;
        for (i, step) in plan.steps.iter().enumerate() {
            if sy > my + mh - 60.0 { break; }

            // Step number circle
            renderer.fill_rounded_rect(pixmap, mx + 20.0, sy, 20.0, 20.0, 10.0, t.accent);
            renderer.draw_text(pixmap, &format!("{}", i + 1), mx + 26.0, sy + 4.0, 10.0, 10.0, [255, 255, 255, 255]);

            // Capability.action
            let action = format!("{}.{}", step.capability, step.action);
            renderer.draw_text(pixmap, &action, mx + 48.0, sy + 3.0, mw - 72.0, 10.0, [196, 181, 253, 255]);

            // Params
            if !step.params.is_null() && step.params != serde_json::json!({}) {
                let params = format_params_inline(&step.params);
                renderer.draw_text(pixmap, &params, mx + 48.0, sy + 20.0, mw - 72.0, 9.0, t.text_muted);
            }

            // Description
            if !step.description.is_empty() {
                renderer.draw_text(pixmap, &step.description, mx + 48.0, sy + 34.0, mw - 72.0, 9.0, t.text_secondary);
            }

            sy += 50.0;
        }

        // Bottom buttons
        let btn_y = my + mh - 52.0;
        renderer.fill_rect(pixmap, mx + 20.0, btn_y - 8.0, mw - 40.0, 1.0, t.border);

        let half_w = (mw - 52.0) / 2.0;
        // Reject
        renderer.fill_rounded_rect(pixmap, mx + 20.0, btn_y, half_w, 36.0, 8.0, [255, 255, 255, 10]);
        renderer.draw_text(pixmap, "✗ Reject  Esc", mx + 32.0, btn_y + 10.0, half_w - 16.0, 11.0, t.text_secondary);

        // Approve
        let ax = mx + 28.0 + half_w;
        renderer.fill_rounded_rect(pixmap, ax, btn_y, half_w, 36.0, 8.0, t.accent);
        renderer.draw_text(pixmap, "✓ Approve  ⏎", ax + 12.0, btn_y + 10.0, half_w - 16.0, 11.0, [255, 255, 255, 255]);
    }
}

// ──────────────────────────────────────────────
//  Helpers
// ──────────────────────────────────────────────

fn message_height(msg: &ChatMessage) -> f32 {
    match msg {
        ChatMessage::UserInput { .. } => 42.0,
        ChatMessage::Thinking => 36.0,
        ChatMessage::PlanProposal { plan, .. } => {
            24.0 + (plan.steps.len() as f32 * 26.0).max(26.0) + 24.0
        }
        ChatMessage::StepProgress { .. } => 32.0,
        ChatMessage::ExecutionDone { results, .. } => {
            let mut h = 36.0_f32;
            for r in results {
                if r.success {
                    let lines = format_result_lines(&r.data);
                    h += (lines.len() as f32 * 14.0).min(120.0) + 8.0;
                }
            }
            h.min(260.0)
        }
        ChatMessage::AgentError { .. } => 50.0,
    }
}

fn compact_result_summary(data: &serde_json::Value) -> String {
    if let Some(count) = data.get("count").and_then(|v| v.as_u64()) {
        if data.get("entries").is_some() { return format!("{} items", count); }
        if data.get("processes").is_some() { return format!("{} procs", count); }
        if data.get("services").is_some() { return format!("{} svcs", count); }
    }
    if let Some(h) = data.get("hostname").and_then(|v| v.as_str()) { return h.to_string(); }
    if let Some(u) = data.get("uptime_human").and_then(|v| v.as_str()) { return u.to_string(); }
    "ok".to_string()
}

fn format_result_lines(data: &serde_json::Value) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(entries) = data.get("entries").and_then(|v| v.as_array()) {
        for e in entries.iter().take(12) {
            let name = e.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let is_dir = e.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false);
            let icon = if is_dir { "📁" } else { "📄" };
            lines.push(format!("  {} {}", icon, name));
        }
        if entries.len() > 12 {
            lines.push(format!("  … +{} more", entries.len() - 12));
        }
        return lines;
    }

    if let Some(procs) = data.get("processes").and_then(|v| v.as_array()) {
        for p in procs.iter().take(10) {
            let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let pid = p.get("pid").and_then(|v| v.as_i64()).unwrap_or(0);
            let cpu = p.get("cpu_percent").and_then(|v| v.as_str()).unwrap_or("0");
            lines.push(format!("  {} (PID {}) {}%", name, pid, cpu));
        }
        return lines;
    }

    if let Some(h) = data.get("hostname").and_then(|v| v.as_str()) {
        lines.push(format!("  Hostname: {}", h));
        return lines;
    }

    if let Some(u) = data.get("uptime_human").and_then(|v| v.as_str()) {
        lines.push(format!("  Uptime: {}", u));
        return lines;
    }

    if let Some(content) = data.get("content").and_then(|v| v.as_str()) {
        for line in content.lines().take(10) {
            lines.push(format!("  {}", line));
        }
        return lines;
    }

    // Key-value fallback
    if let Some(obj) = data.as_object() {
        for (k, v) in obj.iter().take(10) {
            let val = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            lines.push(format!("  {}: {}", k, val));
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

use base64::Engine as _;
use soma_common::{AgentMessage, AgentStatus, CapabilityResult, CompositorMessage, TaskPlan, RiskLevel};
use crate::renderer::Renderer;
use tiny_skia::Pixmap;

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

    pub fn scroll(&mut self, delta: f32) {
        self.scroll_offset = (self.scroll_offset - delta).max(0.0);
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

    pub fn handle_agent_message(&mut self, msg: AgentMessage) {
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
    }

    // ──────────────────────────────────────────────
    //  Rendering
    // ──────────────────────────────────────────────

    pub fn render(&mut self, renderer: &mut Renderer, pixmap: &mut Pixmap, ox: f32, height: f32) {
        let t = renderer.theme.clone();
        let w = SIDEBAR_WIDTH;

        // Background
        renderer.fill_rect(pixmap, ox, 0.0, w, height, t.bg_sidebar);
        renderer.fill_rect(pixmap, ox, 0.0, 1.0, height, t.border);

        // ─── Title Bar ───
        renderer.fill_rect(pixmap, ox, 0.0, w, 44.0, [255, 255, 255, 5]);
        renderer.fill_rect(pixmap, ox, 43.0, w, 1.0, t.border);
        renderer.draw_text(pixmap, "* SOMA", ox + 14.0, 14.0, 100.0, 13.0, t.accent);

        // Status pill
        let status_color = match self.status {
            AgentStatus::Idle => t.success,
            AgentStatus::Thinking => t.accent,
            AgentStatus::AwaitingApproval => t.warning,
            AgentStatus::Executing => [249, 115, 22, 255],
            AgentStatus::Completed => t.success,
            AgentStatus::Error => t.error,
        };
        let status_text = format!("{}", self.status);
        renderer.fill_rounded_rect(pixmap, ox + w - 100.0, 12.0, 88.0, 20.0, 10.0, [255, 255, 255, 8]);
        renderer.fill_rounded_rect(pixmap, ox + w - 96.0, 16.0, 6.0, 6.0, 3.0, status_color);
        renderer.draw_text(pixmap, &status_text, ox + w - 86.0, 15.0, 72.0, 10.0, status_color);

        // ─── Content Area ───
        let content_y = 48.0;
        let content_h = height - 48.0 - 68.0;

        if self.messages.is_empty() {
            let cy = content_y + content_h / 2.0 - 50.0;
            renderer.draw_text(pixmap, "Welcome to Soma", ox + w / 2.0 - 70.0, cy, 200.0, 15.0, t.text_primary);
            renderer.draw_text(pixmap, "Describe what you want to do.", ox + 30.0, cy + 28.0, w - 60.0, 11.0, t.text_muted);
            renderer.draw_text(pixmap, "I'll plan, you approve, I execute.", ox + 30.0, cy + 46.0, w - 60.0, 11.0, t.text_muted);
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

        // ─── Input Area ───
        let input_y = height - 64.0;
        renderer.fill_rect(pixmap, ox, input_y - 1.0, w, 1.0, t.border);
        renderer.fill_rect(pixmap, ox, input_y, w, 64.0, [255, 255, 255, 5]);

        renderer.fill_rounded_rect(pixmap, ox + 10.0, input_y + 10.0, w - 58.0, 36.0, 10.0, t.bg_input);
        renderer.stroke_rect(pixmap, ox + 10.0, input_y + 10.0, w - 58.0, 36.0, t.border);

        let display = if self.input_text.is_empty() {
            match self.status {
                AgentStatus::AwaitingApproval => "Enter=approve  Esc=reject".to_string(),
                _ => "Ask me anything...".to_string(),
            }
        } else if self.cursor_visible {
            format!("{}|", self.input_text)
        } else {
            self.input_text.clone()
        };
        let text_color = if self.input_text.is_empty() { t.text_muted } else { t.text_primary };
        renderer.draw_text(pixmap, &display, ox + 18.0, input_y + 20.0, w - 76.0, 11.0, text_color);

        // Send button
        let btn_color = if self.status == AgentStatus::AwaitingApproval { t.success } else { t.accent };
        renderer.fill_rounded_rect(pixmap, ox + w - 42.0, input_y + 12.0, 32.0, 32.0, 8.0, btn_color);
        let btn_icon = if self.status == AgentStatus::AwaitingApproval { "OK" } else { ">" };
        renderer.draw_text(pixmap, btn_icon, ox + w - 36.0, input_y + 20.0, 22.0, 12.0, [255, 255, 255, 255]);
    }

    fn render_message(&self, renderer: &mut Renderer, pixmap: &mut Pixmap, msg: &ChatMessage, y: f32, ox: f32, w: f32, t: &crate::renderer::Theme) {
        match msg {
            ChatMessage::UserInput { text } => {
                // Right-aligned user bubble
                let max_w = (w - 40.0).min(280.0);
                let text_display = truncate_str(text, 60);
                let bubble_w = max_w;
                let x = ox + w - bubble_w - 10.0;
                renderer.fill_rounded_rect(pixmap, x, y, bubble_w, 30.0, 12.0, t.accent);
                renderer.draw_text(pixmap, &text_display, x + 10.0, y + 8.0, bubble_w - 20.0, 11.0, [255, 255, 255, 255]);
            }

            ChatMessage::Thinking => {
                renderer.fill_rounded_rect(pixmap, ox + 10.0, y, 150.0, 26.0, 8.0, t.bg_surface);
                renderer.draw_text(pixmap, "...  Thinking", ox + 20.0, y + 6.0, 130.0, 11.0, t.accent);
            }

            ChatMessage::PlanProposal { plan, .. } => {
                let card_w = w - 20.0;
                let step_h = plan.steps.len() as f32 * 22.0;
                let card_h = 36.0 + step_h.max(22.0);

                renderer.fill_rounded_rect(pixmap, ox + 10.0, y, card_w, card_h, 8.0, t.bg_surface);

                // Intent + risk
                let intent = if plan.description.is_empty() { &plan.intent } else { &plan.description };
                let risk_color = match plan.risk_level { RiskLevel::Low => t.success, RiskLevel::Medium => t.warning, RiskLevel::High => t.error };

                renderer.fill_rounded_rect(pixmap, ox + 16.0, y + 8.0, 6.0, 6.0, 3.0, risk_color);
                renderer.draw_text(pixmap, &truncate_str(intent, 45), ox + 28.0, y + 5.0, card_w - 40.0, 11.0, t.text_primary);

                // Steps list
                let mut sy = y + 26.0;
                for (i, step) in plan.steps.iter().enumerate() {
                    let label = format!("{}. {}.{}", i + 1, step.capability, step.action);
                    renderer.draw_text(pixmap, &label, ox + 22.0, sy, card_w - 36.0, 9.0, [196, 181, 253, 220]);
                    sy += 22.0;
                }
            }

            ChatMessage::ExecutionDone { intent, success_count, total, results, preview } => {
                let result_lines = format_all_results(results);
                let visible_lines: Vec<&str> = result_lines.iter().map(|s| s.as_str()).take(15).collect();
                let more_count = result_lines.len().saturating_sub(15);

                let (thumb_w, thumb_h) = match preview {
                    PreviewData::Image { width, height, .. } => (*width as f32, *height as f32),
                    PreviewData::None => (0.0, 0.0),
                };

                let header_h = 28.0;
                let thumb_section_h = if thumb_h > 0.0 { thumb_h + 10.0 } else { 0.0 };
                let lines_h = visible_lines.len() as f32 * 15.0;
                let more_h = if more_count > 0 { 15.0 } else { 0.0 };
                let card_h = header_h + thumb_section_h + lines_h + more_h + 8.0;
                let card_w = w - 20.0;

                // Card background
                let bg = if *success_count == *total { [74, 222, 128, 12] } else { [248, 113, 113, 12] };
                renderer.fill_rounded_rect(pixmap, ox + 10.0, y, card_w, card_h, 8.0, bg);

                // Header
                let icon = if *success_count == *total { "v" } else { "!" };
                let header_color = if *success_count == *total { t.success } else { t.warning };
                let header = format!("{} {} ({}/{})", icon, truncate_str(intent, 30), success_count, total);
                renderer.draw_text(pixmap, &header, ox + 18.0, y + 6.0, card_w - 24.0, 10.0, header_color);

                // Divider
                renderer.fill_rect(pixmap, ox + 18.0, y + 24.0, card_w - 24.0, 1.0, [255, 255, 255, 12]);

                let mut ry = y + header_h + 2.0;

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
                }
            }

            ChatMessage::AgentError { message } => {
                let card_w = w - 20.0;
                renderer.fill_rounded_rect(pixmap, ox + 10.0, y, card_w, 38.0, 8.0, [248, 113, 113, 15]);
                renderer.draw_text(pixmap, "! Error", ox + 18.0, y + 4.0, 80.0, 10.0, t.error);
                renderer.draw_text(pixmap, &truncate_str(message, 55), ox + 18.0, y + 20.0, card_w - 28.0, 9.0, t.error);
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
        renderer.fill_rect(pixmap, 0.0, 0.0, width, height, [0, 0, 0, 150]);

        // Modal size
        let step_count = plan.steps.len().max(1);
        let mh = (160.0 + step_count as f32 * 46.0).min(height - 60.0);
        let mw = 360.0_f32.min(width - 40.0);
        let mx = (width - mw) / 2.0;
        let my = (height - mh) / 2.0;

        renderer.fill_rounded_rect(pixmap, mx, my, mw, mh, 14.0, [22, 22, 42, 250]);
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
            36.0 + (plan.steps.len() as f32 * 22.0).max(22.0) + 8.0
        }
        ChatMessage::ExecutionDone { results, preview, .. } => {
            let lines = format_all_results(results);
            let visible = lines.len().min(15);
            let more_h = if lines.len() > 15 { 15.0 } else { 0.0 };
            let thumb_h = match preview {
                PreviewData::Image { height, .. } => *height as f32 + 10.0,
                PreviewData::None => 0.0,
            };
            28.0 + thumb_h + (visible as f32 * 15.0) + more_h + 16.0
        }
        ChatMessage::AgentError { .. } => 46.0,
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() > max_chars {
        format!("{}...", &s[..max_chars])
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
                lines.push(format!("  ! {}", truncate_str(err, 50)));
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

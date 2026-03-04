use soma_common::{AgentMessage, AgentStatus, CommandResult, CompositorMessage, TaskPlan, RiskLevel};
use crate::renderer::Renderer;
use tiny_skia::Pixmap;

/// Sidebar panel width in pixels
pub const SIDEBAR_WIDTH: f32 = 380.0;

/// Maximum history entries to keep
const MAX_HISTORY: usize = 50;

pub struct HistoryEntry {
    pub input: String,
    pub plan: Option<TaskPlan>,
    pub results: Vec<CommandResult>,
    pub approved: bool,
    pub error: Option<String>,
}

pub struct Sidebar {
    pub input_text: String,
    pub status: AgentStatus,
    pub current_plan: Option<TaskPlan>,
    pub current_plan_id: Option<String>,
    pub results: Vec<CommandResult>,
    pub error: Option<String>,
    pub history: Vec<HistoryEntry>,
    pub scroll_offset: f32,
    pub cursor_visible: bool,
    cursor_timer: f32,
}

impl Sidebar {
    pub fn new() -> Self {
        Self {
            input_text: String::new(),
            status: AgentStatus::Idle,
            current_plan: None,
            current_plan_id: None,
            results: Vec::new(),
            error: None,
            history: Vec::new(),
            scroll_offset: 0.0,
            cursor_visible: true,
            cursor_timer: 0.0,
        }
    }

    /// Handle a character input
    pub fn on_char(&mut self, c: char) {
        if self.status == AgentStatus::Idle || self.status == AgentStatus::Completed || self.status == AgentStatus::Error {
            self.input_text.push(c);
        }
    }

    /// Handle backspace
    pub fn on_backspace(&mut self) {
        self.input_text.pop();
    }

    /// Handle enter — returns a CompositorMessage to send to the agent if applicable
    pub fn on_submit(&mut self) -> Option<CompositorMessage> {
        match self.status {
            AgentStatus::Idle | AgentStatus::Completed | AgentStatus::Error => {
                let text = self.input_text.trim().to_string();
                if text.is_empty() {
                    return None;
                }
                self.input_text.clear();
                self.status = AgentStatus::Thinking;
                self.error = None;
                self.results.clear();

                let id = uuid::Uuid::new_v4().to_string();
                Some(CompositorMessage::ParseIntent { id, input: text })
            }
            AgentStatus::AwaitingApproval => {
                self.on_approve()
            }
            _ => None,
        }
    }

    /// Approve the current plan
    pub fn on_approve(&mut self) -> Option<CompositorMessage> {
        if let Some(id) = self.current_plan_id.take() {
            self.status = AgentStatus::Executing;
            Some(CompositorMessage::Approve { id })
        } else {
            None
        }
    }

    /// Reject the current plan (Escape key)
    pub fn on_reject(&mut self) -> Option<CompositorMessage> {
        if let Some(id) = self.current_plan_id.take() {
            let plan = self.current_plan.take();
            self.status = AgentStatus::Idle;

            self.history.push(HistoryEntry {
                input: plan.as_ref().map(|p| p.description.clone()).unwrap_or_default(),
                plan,
                results: vec![],
                approved: false,
                error: None,
            });
            if self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }

            Some(CompositorMessage::Reject { id })
        } else {
            None
        }
    }

    /// Handle incoming agent message
    pub fn handle_agent_message(&mut self, msg: AgentMessage) {
        match msg {
            AgentMessage::TaskPlanReady { id, plan } => {
                self.status = AgentStatus::AwaitingApproval;
                self.current_plan = Some(plan);
                self.current_plan_id = Some(id);
            }
            AgentMessage::StepResult { result, .. } => {
                self.results.push(result);
            }
            AgentMessage::ExecutionComplete { results, .. } => {
                self.status = AgentStatus::Completed;
                let plan = self.current_plan.take();
                self.history.push(HistoryEntry {
                    input: plan.as_ref().map(|p| p.description.clone()).unwrap_or_default(),
                    plan,
                    results: results.clone(),
                    approved: true,
                    error: None,
                });
                if self.history.len() > MAX_HISTORY {
                    self.history.remove(0);
                }
                self.results = results;
            }
            AgentMessage::Error { message, .. } => {
                self.status = AgentStatus::Error;
                self.error = Some(message.clone());
                let plan = self.current_plan.take();
                self.current_plan_id = None;
                self.history.push(HistoryEntry {
                    input: plan.as_ref().map(|p| p.description.clone()).unwrap_or_default(),
                    plan,
                    results: vec![],
                    approved: false,
                    error: Some(message),
                });
            }
            AgentMessage::DirectOutput { result, .. } => {
                self.results.push(result);
            }
            _ => {}
        }
    }

    /// Update animations (cursor blink)
    pub fn update(&mut self, dt: f32) {
        self.cursor_timer += dt;
        if self.cursor_timer > 0.53 {
            self.cursor_timer = 0.0;
            self.cursor_visible = !self.cursor_visible;
        }
    }

    /// Render the sidebar onto the pixmap
    pub fn render(&mut self, renderer: &mut Renderer, pixmap: &mut Pixmap, height: f32) {
        let t = renderer.theme.clone();
        let w = SIDEBAR_WIDTH;

        // Background
        renderer.fill_rect(pixmap, 0.0, 0.0, w, height, t.bg_sidebar);
        // Right border
        renderer.fill_rect(pixmap, w - 1.0, 0.0, 1.0, height, t.border);

        // ─── Title Bar ───
        renderer.fill_rect(pixmap, 0.0, 0.0, w, 48.0, [255, 255, 255, 5]);
        renderer.fill_rect(pixmap, 0.0, 47.0, w, 1.0, t.border);

        // Logo
        renderer.draw_text(pixmap, "◈", 16.0, 14.0, 30.0, 18.0, t.accent);
        renderer.draw_text(pixmap, "SOMA", 38.0, 16.0, 80.0, 12.0, t.text_secondary);

        // Status
        let status_color = match self.status {
            AgentStatus::Idle => t.success,
            AgentStatus::Thinking => t.accent,
            AgentStatus::AwaitingApproval => t.warning,
            AgentStatus::Executing => [249, 115, 22, 255],
            AgentStatus::Completed => t.success,
            AgentStatus::Error => t.error,
        };
        renderer.fill_rounded_rect(pixmap, w - 130.0, 16.0, 6.0, 6.0, 3.0, status_color);
        let status_str = format!("{}", self.status);
        renderer.draw_text(pixmap, &status_str, w - 118.0, 13.0, 110.0, 10.0, status_color);

        // ─── Content Area ───
        let content_y = 56.0;
        let content_h = height - 56.0 - 80.0; // Between titlebar and input
        let mut y = content_y + 8.0 - self.scroll_offset;

        // Welcome screen
        if self.history.is_empty() && self.status == AgentStatus::Idle && self.error.is_none() {
            let cy = content_y + content_h / 2.0 - 60.0;
            renderer.draw_text(pixmap, "◈", w / 2.0 - 15.0, cy, 40.0, 36.0, t.accent);
            renderer.draw_text(pixmap, "Welcome to Soma", w / 2.0 - 75.0, cy + 50.0, 200.0, 16.0, t.text_primary);
            renderer.draw_text(
                pixmap,
                "Describe what you want to do.\nI'll create a plan for your approval.",
                28.0,
                cy + 80.0,
                w - 56.0,
                11.0,
                t.text_muted,
            );
        } else {
            // History entries
            for entry in &self.history {
                if y > content_y + content_h {
                    break;
                }

                let icon = if entry.approved { "✓" } else { "✗" };
                let icon_color = if entry.approved { t.success } else { t.warning };

                // Input bubble
                renderer.fill_rounded_rect(pixmap, 12.0, y, w - 24.0, 32.0, 6.0, t.bg_surface);
                renderer.draw_text(pixmap, icon, 20.0, y + 8.0, 16.0, 12.0, icon_color);
                let display_text = if entry.input.len() > 40 {
                    format!("{}…", &entry.input[..40])
                } else {
                    entry.input.clone()
                };
                renderer.draw_text(pixmap, &display_text, 38.0, y + 9.0, w - 58.0, 11.0, t.text_primary);
                y += 38.0;

                // Results
                for result in &entry.results {
                    if !result.stdout.is_empty() {
                        let lines: Vec<&str> = result.stdout.lines().take(5).collect();
                        let text = lines.join("\n");
                        let block_h = (lines.len() as f32 * 15.0).max(24.0) + 12.0;
                        renderer.fill_rounded_rect(pixmap, 12.0, y, w - 24.0, block_h, 6.0, [74, 222, 128, 12]);
                        renderer.draw_text(pixmap, &text, 20.0, y + 6.0, w - 40.0, 10.0, t.success);
                        y += block_h + 4.0;
                    }
                }

                if let Some(err) = &entry.error {
                    let err_short = if err.len() > 60 { &err[..60] } else { err };
                    renderer.fill_rounded_rect(pixmap, 12.0, y, w - 24.0, 28.0, 6.0, [248, 113, 113, 12]);
                    renderer.draw_text(pixmap, err_short, 20.0, y + 7.0, w - 40.0, 10.0, t.error);
                    y += 34.0;
                }

                y += 4.0;
            }

            // Current results
            for result in &self.results {
                if y > content_y + content_h {
                    break;
                }
                if !result.stdout.is_empty() {
                    let lines: Vec<&str> = result.stdout.lines().take(8).collect();
                    let text = lines.join("\n");
                    let block_h = (lines.len() as f32 * 15.0).max(24.0) + 12.0;
                    renderer.fill_rounded_rect(pixmap, 12.0, y, w - 24.0, block_h, 6.0, [74, 222, 128, 12]);
                    renderer.draw_text(pixmap, &text, 20.0, y + 6.0, w - 40.0, 10.0, t.success);
                    y += block_h + 4.0;
                }
            }
        }

        // ─── Thinking indicator ───
        if self.status == AgentStatus::Thinking {
            let ty = content_y + content_h / 2.0 - 12.0;
            renderer.fill_rounded_rect(pixmap, 12.0, ty, w - 24.0, 32.0, 6.0, t.bg_surface);
            renderer.draw_text(pixmap, "● ● ●  Analyzing…", 24.0, ty + 9.0, w - 48.0, 11.0, t.accent);
        }

        // ─── Error display ───
        if let Some(err) = &self.error.clone() {
            let ey = content_y + content_h - 50.0;
            renderer.fill_rounded_rect(pixmap, 12.0, ey, w - 24.0, 44.0, 6.0, [248, 113, 113, 20]);
            renderer.draw_text(pixmap, "⚠ Error", 20.0, ey + 4.0, 100.0, 10.0, t.error);
            let err_short = if err.len() > 50 { &err[..50] } else { err };
            renderer.draw_text(pixmap, err_short, 20.0, ey + 20.0, w - 40.0, 9.0, t.error);
        }

        // ─── Input Area ───
        let input_y = height - 72.0;
        renderer.fill_rect(pixmap, 0.0, input_y - 1.0, w, 1.0, t.border);
        renderer.fill_rect(pixmap, 0.0, input_y, w, 72.0, [255, 255, 255, 5]);

        // Input field
        renderer.fill_rounded_rect(pixmap, 12.0, input_y + 12.0, w - 64.0, 38.0, 10.0, t.bg_input);
        renderer.stroke_rect(pixmap, 12.0, input_y + 12.0, w - 64.0, 38.0, t.border);

        let display = if self.input_text.is_empty() {
            "Describe what you want to do…".to_string()
        } else if self.cursor_visible {
            format!("{}|", self.input_text)
        } else {
            self.input_text.clone()
        };
        let text_color = if self.input_text.is_empty() { t.text_muted } else { t.text_primary };
        renderer.draw_text(pixmap, &display, 22.0, input_y + 22.0, w - 90.0, 12.0, text_color);

        // Send button
        renderer.fill_rounded_rect(pixmap, w - 46.0, input_y + 14.0, 34.0, 34.0, 8.0, t.accent);
        renderer.draw_text(pixmap, "↑", w - 38.0, input_y + 22.0, 20.0, 16.0, [255, 255, 255, 255]);
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

        // Modal
        let mw = 340.0_f32.min(width - 40.0);
        let mx = (width - mw) / 2.0;
        let my = height / 2.0 - 160.0;
        let mut mh = 0.0_f32;

        // Modal background
        renderer.fill_rounded_rect(pixmap, mx, my, mw, 340.0, 14.0, [26, 26, 46, 250]);
        renderer.stroke_rect(pixmap, mx, my, mw, 340.0, t.border);

        // Header
        mh += 20.0;
        renderer.draw_text(pixmap, "🛡️", mx + 16.0, my + mh, 24.0, 20.0, t.text_primary);
        renderer.draw_text(pixmap, "Action Requires Approval", mx + 42.0, my + mh + 2.0, mw - 60.0, 13.0, t.text_primary);
        mh += 32.0;

        // Divider
        renderer.fill_rect(pixmap, mx + 16.0, my + mh, mw - 32.0, 1.0, t.border);
        mh += 12.0;

        // Description
        let desc = plan.description.clone();
        let desc_h = renderer.draw_text(pixmap, &desc, mx + 16.0, my + mh, mw - 32.0, 12.0, t.text_secondary);
        mh += desc_h.max(18.0) + 8.0;

        // Risk badge
        let (risk_color, risk_label) = match plan.risk_level {
            RiskLevel::Low => (t.success, "✓ Low Risk"),
            RiskLevel::Medium => (t.warning, "⚠ Medium Risk"),
            RiskLevel::High => (t.error, "⛔ High Risk"),
        };
        renderer.fill_rounded_rect(pixmap, mx + 16.0, my + mh, 100.0, 22.0, 11.0, [255, 255, 255, 8]);
        renderer.draw_text(pixmap, risk_label, mx + 24.0, my + mh + 5.0, 90.0, 10.0, risk_color);
        mh += 32.0;

        // Steps
        renderer.draw_text(pixmap, "PLANNED ACTIONS", mx + 16.0, my + mh, mw - 32.0, 9.0, t.text_muted);
        mh += 18.0;

        for (i, step) in plan.steps.iter().enumerate() {
            let cmd = format!("{} {}", step.command, step.args.join(" "));
            renderer.fill_rounded_rect(pixmap, mx + 16.0, my + mh, mw - 32.0, 28.0, 6.0, [255, 255, 255, 6]);
            renderer.draw_text(pixmap, &format!("{}", i + 1), mx + 24.0, my + mh + 7.0, 12.0, 10.0, t.accent);
            renderer.draw_text(pixmap, &cmd, mx + 40.0, my + mh + 7.0, mw - 72.0, 10.0, [196, 181, 253, 255]);
            mh += 34.0;
        }

        if plan.steps.is_empty() {
            renderer.draw_text(pixmap, "No executable steps.", mx + 24.0, my + mh + 4.0, mw - 48.0, 10.0, t.text_muted);
            let _ = mh; // suppress unused warning
        }

        // Bottom buttons
        let btn_y = my + 340.0 - 52.0;
        renderer.fill_rect(pixmap, mx + 16.0, btn_y - 8.0, mw - 32.0, 1.0, t.border);

        // Reject button
        renderer.fill_rounded_rect(pixmap, mx + 16.0, btn_y, (mw - 40.0) / 2.0, 36.0, 8.0, [255, 255, 255, 12]);
        renderer.draw_text(pixmap, "Reject [Esc]", mx + 28.0, btn_y + 10.0, 100.0, 11.0, t.text_secondary);

        // Approve button
        let approve_x = mx + 24.0 + (mw - 40.0) / 2.0;
        renderer.fill_rounded_rect(pixmap, approve_x, btn_y, (mw - 40.0) / 2.0, 36.0, 8.0, t.accent);
        renderer.draw_text(pixmap, "Approve [⏎]", approve_x + 12.0, btn_y + 10.0, 100.0, 11.0, [255, 255, 255, 255]);
    }
}

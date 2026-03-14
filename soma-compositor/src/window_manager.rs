//! Floating window management for the SomaOS desktop environment.
//!
//! Each app (Terminal, Browser, or agent-spawned DynamicApp) lives in a
//! `FloatingWindow`. The compositor owns one `Terminal` and one `BrowserPanel`
//! instance; `WindowContent` is a discriminant tag used to route rendering
//! and input — it does NOT own the component by value.

use crate::renderer::Renderer;
use tiny_skia::Pixmap;

pub type WindowId = u32;

// ─────────────────────────────────────────────────────────────────────────────
//  Widget tree for agent-spawned dynamic apps
// ─────────────────────────────────────────────────────────────────────────────

/// A UI widget inside an agent-spawned DynamicApp window.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Widget {
    Label {
        text: String,
        x: f32,
        y: f32,
        font_size: f32,
        #[serde(default = "default_text_color")]
        color: [u8; 4],
    },
    Button {
        text: String,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        /// Identifier sent back to the agent when clicked
        action_id: String,
    },
    ProgressBar {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        /// 0.0–1.0
        progress: f32,
    },
    TextDisplay {
        content: String,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
}

fn default_text_color() -> [u8; 4] {
    [212, 212, 212, 255]
}

/// An agent-created app definition.
/// Saved to `~/.soma/apps/<app_id>.json` when promoted.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AppDef {
    pub app_id: String,
    pub description: String,
    pub widgets: Vec<Widget>,
}

impl AppDef {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  WindowContent discriminant
// ─────────────────────────────────────────────────────────────────────────────

/// Tags the kind of content a FloatingWindow shows.
/// Terminal and Browser map to singleton components on AppState.
/// DynamicApp carries its own widget tree.
#[derive(Debug, Clone)]
pub enum WindowContent {
    Terminal,
    Browser,
    DynamicApp(AppDef),
    Settings(crate::settings_app::SettingsApp),
}

/// Subset tag for use in Dock (no heap data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowContentType {
    Terminal,
    Browser,
    Settings,
}

impl WindowContent {
    pub fn content_type(&self) -> Option<WindowContentType> {
        match self {
            WindowContent::Terminal => Some(WindowContentType::Terminal),
            WindowContent::Browser  => Some(WindowContentType::Browser),
            WindowContent::DynamicApp(_) => None,
            WindowContent::Settings(_) => Some(WindowContentType::Settings),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  FloatingWindow
// ─────────────────────────────────────────────────────────────────────────────

pub struct FloatingWindow {
    pub id: WindowId,
    pub title: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub content: WindowContent,
    pub is_focused: bool,
    pub dragging: bool,
    pub drag_offset_x: f32,
    pub drag_offset_y: f32,
    pub is_minimized: bool,
    /// True when the agent spawned this window — renders a teal "AI" badge.
    pub agent_owned: bool,
}

impl FloatingWindow {
    pub const TITLE_BAR_H: f32 = 28.0;
    pub const CLOSE_BTN_R: f32  = 6.0;
    pub const CLOSE_BTN_X: f32  = 16.0; // centre x from window left
    pub const CLOSE_BTN_Y: f32  = 14.0; // centre y from window top

    pub fn new(id: WindowId, content: WindowContent, x: f32, y: f32) -> Self {
        let (title, width, height) = match &content {
            WindowContent::Terminal      => ("Terminal".to_string(), 780.0, 480.0),
            WindowContent::Browser       => ("Browser".to_string(),  960.0, 620.0),
            WindowContent::DynamicApp(d) => (d.app_id.clone(),       480.0, 360.0),
            WindowContent::Settings(_)   => ("System Settings".to_string(), 380.0, 420.0),
        };
        let agent_owned = matches!(content, WindowContent::DynamicApp(_));
        Self {
            id, title, x, y, width, height, content,
            is_focused: false,
            dragging: false,
            drag_offset_x: 0.0,
            drag_offset_y: 0.0,
            is_minimized: false,
            agent_owned,
        }
    }

    /// True if (px, py) lands in the draggable part of the title bar
    /// (excludes the close-button zone so a drag doesn't accidentally close).
    pub fn hit_titlebar(&self, px: f32, py: f32) -> bool {
        px >= self.x + Self::CLOSE_BTN_X * 2.2
            && px <= self.x + self.width
            && py >= self.y
            && py <  self.y + Self::TITLE_BAR_H
    }

    /// True if (px, py) is within the circular close button.
    pub fn hit_close(&self, px: f32, py: f32) -> bool {
        let cx = self.x + Self::CLOSE_BTN_X;
        let cy = self.y + Self::CLOSE_BTN_Y;
        let dx = px - cx;
        let dy = py - cy;
        dx * dx + dy * dy <= Self::CLOSE_BTN_R * Self::CLOSE_BTN_R
    }

    /// True if (px, py) is anywhere inside the full window frame.
    pub fn hit_frame(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width
            && py >= self.y && py < self.y + self.height
    }

    /// Content area rect: (x, y, w, h) — the region below the title bar.
    pub fn content_rect(&self) -> (f32, f32, f32, f32) {
        (
            self.x,
            self.y + Self::TITLE_BAR_H,
            self.width,
            (self.height - Self::TITLE_BAR_H).max(0.0),
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Window chrome rendering
// ─────────────────────────────────────────────────────────────────────────────

/// Render the window frame: shadow, body, title bar, close button, title text,
/// and (optionally) an "AI" ownership badge.
pub fn render_window_chrome(
    renderer: &mut Renderer,
    pixmap: &mut Pixmap,
    win: &FloatingWindow,
    is_hover_close: bool,
) {
    let t = renderer.theme.clone();

    // Multi-layer shadow for depth
    renderer.fill_rounded_rect(pixmap, win.x + 8.0, win.y + 16.0, win.width, win.height, 16.0, [0, 0, 0, 40]);
    renderer.fill_rounded_rect(pixmap, win.x + 2.0, win.y + 4.0, win.width, win.height, 12.0, [0, 0, 0, 70]);

    // Outer border (highlight) — crisp neon ring for focused, subtle glass rim for unfocused
    let border_color = if win.is_focused { t.accent } else { [255, 255, 255, 20] };
    renderer.fill_rounded_rect(pixmap, win.x - 1.0, win.y - 1.0, win.width + 2.0, win.height + 2.0, 11.0, border_color);

    // Window body (glassy translucent)
    let body_bg = if win.is_focused { t.bg_window_chrome } else { t.bg_window_inactive };
    renderer.fill_rounded_rect(pixmap, win.x, win.y, win.width, win.height, 10.0, body_bg);

    // Title bar
    let tb_bg = if win.is_focused { t.bg_titlebar } else { [28, 28, 34, 250] };
    renderer.fill_rounded_rect(pixmap, win.x, win.y, win.width, FloatingWindow::TITLE_BAR_H + 8.0, 10.0, tb_bg);
    
    // Mask bottom corners of title bar so they are flush with the body
    renderer.fill_rect(pixmap, win.x, win.y + FloatingWindow::TITLE_BAR_H, win.width, 8.0, body_bg);
    
    // Crisp hairline separator below title bar
    renderer.fill_rect(pixmap, win.x, win.y + FloatingWindow::TITLE_BAR_H, win.width, 1.0, [255, 255, 255, 15]);

    // Close button (traffic-light red circle)
    let close_color = if is_hover_close { t.close_btn_hover } else { t.close_btn };
    renderer.fill_rounded_rect(pixmap,
        win.x + FloatingWindow::CLOSE_BTN_X - FloatingWindow::CLOSE_BTN_R,
        win.y + FloatingWindow::CLOSE_BTN_Y - FloatingWindow::CLOSE_BTN_R,
        FloatingWindow::CLOSE_BTN_R * 2.0,
        FloatingWindow::CLOSE_BTN_R * 2.0,
        FloatingWindow::CLOSE_BTN_R,
        close_color,
    );

    // Title text — centered in title bar, bright white if focused
    let title_w = win.title.len() as f32 * 6.5;
    let title_x = win.x + (win.width - title_w) / 2.0;
    let title_color = if win.is_focused { t.text_primary } else { t.text_muted };
    renderer.draw_text(pixmap, &win.title, title_x, win.y + 8.0, title_w + 8.0, 10.0, title_color);

    // "AI" ownership badge — vibrant neon teal pill, top-right
    if win.agent_owned {
        let badge_x = win.x + win.width - 32.0;
        let badge_y = win.y + 6.0;
        renderer.fill_rounded_rect(pixmap, badge_x, badge_y, 22.0, 16.0, 5.0, t.success);
        renderer.draw_text(pixmap, "AI", badge_x + 5.0, badge_y + 3.0, 18.0, 8.0, [10, 15, 25, 255]);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Dynamic app rendering
// ─────────────────────────────────────────────────────────────────────────────

/// Render an agent-spawned app's widget tree inside the window content area.
pub fn render_dynamic_app(
    renderer: &mut Renderer,
    pixmap: &mut Pixmap,
    win: &FloatingWindow,
    mouse_x: f32,
    mouse_y: f32,
) {
    let (cx, cy, cw, ch) = win.content_rect();
    let t = renderer.theme.clone();

    // Content background
    renderer.fill_rect(pixmap, cx, cy, cw, ch, t.bg_window_chrome);

    if let WindowContent::DynamicApp(ref app) = win.content {
        for widget in &app.widgets {
            match widget {
                Widget::Label { text, x, y, font_size, color } => {
                    renderer.draw_text(pixmap, text, cx + x, cy + y, cw - x, *font_size, *color);
                }

                Widget::Button { text, x, y, w, h, action_id: _ } => {
                    let bx = cx + x;
                    let by = cy + y;
                    let hover = mouse_x >= bx && mouse_x < bx + w
                        && mouse_y >= by && mouse_y < by + h;
                    let bg = if hover { t.accent } else { [55, 57, 65, 255] };
                    renderer.fill_rounded_rect(pixmap, bx, by, *w, *h, 5.0, bg);
                    renderer.draw_text(pixmap, text, bx + 8.0, by + (*h - 12.0) / 2.0,
                        w - 16.0, 11.0, t.text_primary);
                }

                Widget::ProgressBar { x, y, w, h, progress } => {
                    let px = cx + x;
                    let py = cy + y;
                    // Track
                    renderer.fill_rounded_rect(pixmap, px, py, *w, *h, h / 2.0, [50, 52, 60, 255]);
                    // Fill
                    let fill_w = (w * progress.clamp(0.0, 1.0)).max(0.0);
                    if fill_w > 0.0 {
                        renderer.fill_rounded_rect(pixmap, px, py, fill_w, *h, h / 2.0, t.accent);
                    }
                }

                Widget::TextDisplay { content, x, y, w, h } => {
                    let tx = cx + x;
                    let ty = cy + y;
                    renderer.fill_rounded_rect(pixmap, tx, ty, *w, *h, 4.0, [30, 32, 36, 255]);
                    renderer.fill_rect(pixmap, tx, ty, *w, 1.0, t.border);
                    renderer.draw_text(pixmap, content, tx + 6.0, ty + 6.0, w - 12.0, 11.0, t.text_primary);
                }
            }
        }
    }
}

/// Hit-test a button widget inside a DynamicApp and return its action_id if hit.
pub fn hit_dynamic_button(
    win: &FloatingWindow,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<String> {
    let (cx, cy, _, _) = win.content_rect();
    if let WindowContent::DynamicApp(ref app) = win.content {
        for widget in &app.widgets {
            if let Widget::Button { x, y, w, h, action_id, .. } = widget {
                let bx = cx + x;
                let by = cy + y;
                if mouse_x >= bx && mouse_x < bx + w && mouse_y >= by && mouse_y < by + h {
                    return Some(action_id.clone());
                }
            }
        }
    }
    None
}

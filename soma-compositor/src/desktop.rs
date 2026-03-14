//! Desktop background and menu bar rendering for SomaOS.

use crate::renderer::Renderer;
use soma_common::AgentStatus;
use tiny_skia::{Color, GradientStop, Pixmap, Point};

/// Height of the always-visible top menu bar in pixels.
pub const MENU_BAR_H: f32 = 28.0;

/// Render the desktop wallpaper — a smooth, deep linear gradient.
pub fn render_desktop(renderer: &mut Renderer, pixmap: &mut Pixmap, w: f32, h: f32) {
    // Temporarily use a solid color instead of a gradient to avoid CPU rendering slowdowns in debug builds
    renderer.fill_rect(pixmap, 0.0, 0.0, w, h, [15, 16, 26, 255]);
}

/// Render the macOS-style top menu bar (28px tall).
///
/// Layout (left → right):
///   "Soma" label  |  (space)  |  activity text  |  [🔒]  |  clock
pub fn render_menu_bar(
    renderer: &mut Renderer,
    pixmap: &mut Pixmap,
    w: f32,
    agent_status: &AgentStatus,
    activity_text: &str,
    private_mode: bool,
    clock_str: &str,
) {
    let t = renderer.theme.clone();

    // Bar background (slightly dimmed in private mode)
    let bar_bg = if private_mode {
        [15, 16, 20, 240]
    } else {
        t.bg_menubar
    };
    renderer.fill_rect(pixmap, 0.0, 0.0, w, MENU_BAR_H, bar_bg);
    // Bottom hairline separator
    renderer.fill_rect(pixmap, 0.0, MENU_BAR_H - 1.0, w, 1.0, t.border);

    // "Soma" label — left side
    renderer.draw_text(pixmap, "Soma", 12.0, 8.0, 50.0, 10.0, t.text_secondary);

    // Clock — far right
    let clock_w = clock_str.len() as f32 * 6.5;
    renderer.draw_text(pixmap, clock_str, w - clock_w - 12.0, 8.0, clock_w + 4.0, 10.0, t.text_secondary);

    // Private mode lock icon — left of clock
    let mut right_cursor = w - clock_w - 18.0;
    if private_mode {
        renderer.draw_text(pixmap, "[pvt]", right_cursor - 34.0, 8.0, 34.0, 9.0, t.warning);
        right_cursor -= 40.0;
    }

    // Activity status dot + text — centre-right
    if !activity_text.is_empty() {
        let dot_color = match agent_status {
            AgentStatus::Idle      => t.success,
            AgentStatus::Thinking  => t.accent,
            AgentStatus::Executing => t.agent_active,
            AgentStatus::AwaitingApproval => t.warning,
            _                      => t.text_muted,
        };

        // Status dot
        let dot_x = right_cursor - activity_text.len() as f32 * 6.0 - 18.0;
        let dot_x = dot_x.max(70.0); // clamp so it doesn't overlap "Soma"
        renderer.fill_rounded_rect(pixmap, dot_x, 11.0, 6.0, 6.0, 3.0, dot_color);

        // Activity text
        renderer.draw_text(pixmap, activity_text, dot_x + 10.0, 8.0,
            right_cursor - dot_x - 14.0, 10.0, t.text_secondary);
    }
}

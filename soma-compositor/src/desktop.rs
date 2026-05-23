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
    agent_status: &soma_common::AgentStatus,
    activity_text: &str,
    private_mode: bool,
    clock_str: &str,
    current_tier: soma_common::ActionTier,
    current_mode: soma_common::SystemMode,
    active_scaffolds: &[soma_common::Scaffold],
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
    renderer.draw_text(pixmap, "Soma", 12.0, 8.0, 36.0, 10.0, t.text_secondary);

    // V2 ActionTier Progression dots
    let dots = match current_tier {
        soma_common::ActionTier::Observe => "● ○ ○ ○ ○",
        soma_common::ActionTier::Touch => "● ● ○ ○ ○",
        soma_common::ActionTier::Operate => "● ● ● ○ ○",
        soma_common::ActionTier::Control => "● ● ● ● ○",
        soma_common::ActionTier::Autonomous => "● ● ● ● ●",
    };
    renderer.draw_text(pixmap, dots, 56.0, 8.0, 70.0, 10.0, t.accent);

    // V2 SystemMode indicator
    let mode_col = match current_mode {
        soma_common::SystemMode::Idle => t.success,
        soma_common::SystemMode::Active => t.accent,
        soma_common::SystemMode::UnderLoad => t.warning,
        soma_common::SystemMode::Stressed | soma_common::SystemMode::Degraded => t.error,
        _ => t.text_muted,
    };
    let mode_str = format!("[{}]", current_mode);
    renderer.draw_text(pixmap, &mode_str, 134.0, 8.0, 90.0, 10.0, mode_col);

    // V2 Scaffold level shield indicator
    let avg_level: f64 = if active_scaffolds.is_empty() {
        1.0
    } else {
        active_scaffolds.iter().map(|s| s.activation_level).sum::<f64>() / active_scaffolds.len() as f64
    };
    let shield_pct = (avg_level * 100.0) as u32;
    let shield_str = format!("[Shield: {}%]", shield_pct);
    let shield_col = if shield_pct > 70 { t.success } else if shield_pct > 30 { t.warning } else { t.text_muted };
    renderer.draw_text(pixmap, &shield_str, 230.0, 8.0, 100.0, 10.0, shield_col);

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
            soma_common::AgentStatus::Idle      => t.success,
            soma_common::AgentStatus::Thinking  => t.accent,
            soma_common::AgentStatus::Executing => t.agent_active,
            soma_common::AgentStatus::AwaitingApproval => t.warning,
            _                      => t.text_muted,
        };

        // Status dot
        let dot_x = right_cursor - activity_text.len() as f32 * 6.0 - 18.0;
        let dot_x = dot_x.max(340.0); // clamp so it doesn't overlap left dashboard items
        renderer.fill_rounded_rect(pixmap, dot_x, 11.0, 6.0, 6.0, 3.0, dot_color);

        // Activity text
        renderer.draw_text(pixmap, activity_text, dot_x + 10.0, 8.0,
            right_cursor - dot_x - 14.0, 10.0, t.text_secondary);
    }
}

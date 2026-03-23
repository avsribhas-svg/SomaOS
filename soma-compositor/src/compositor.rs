//! Compositor render stack and state update.
//!
//! Extracted from main.rs — the 9-layer compositor stack and per-frame
//! state updates (animation ticks, toast decay, dock sync, etc.)

use crate::browser_panel::BrowserPanel;
use crate::desktop::{self, MENU_BAR_H};
use crate::dock::{self, Dock, DOCK_HEIGHT};
use crate::renderer::Renderer;
use crate::sidebar::{self, Sidebar};
use crate::terminal::Terminal;
use crate::window_manager::{
    self, FloatingWindow, WindowContent, WindowContentType,
    render_window_chrome,
};

/// Notification toast shown in the top-right corner.
pub struct Toast {
    pub message: String,
    pub color: [u8; 4],
    pub remaining: f32,
}

impl Toast {
    pub fn new(message: String, color: [u8; 4]) -> Self {
        Self { message, color, remaining: 3.5 }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Per-frame state update
// ─────────────────────────────────────────────────────────────────────────────

/// Tick animations, poll data sources, and update UI state for one frame.
///
/// Returns `(got_agent_msg, pty_data)` to inform the caller whether a
/// continuous redraw is needed.
pub fn update(
    dt: f32,
    sidebar: &mut Sidebar,
    terminal: &mut Terminal,
    toasts: &mut Vec<Toast>,
    windows: &mut [FloatingWindow],
    dock: &mut Dock,
    agent_mode: bool,
    sidebar_visible: bool,
    private_mode: bool,
    mouse_x: f32,
    mouse_y: f32,
    w: f32,
    h: f32,
) {
    // Tick animations
    sidebar.update(dt);
    terminal.update(dt);

    // Decay toasts
    toasts.retain_mut(|t| {
        t.remaining -= dt;
        t.remaining > 0.0
    });

    // Decay settings saved-toast
    for win in windows.iter_mut() {
        if let WindowContent::Settings(ref mut s) = win.content {
            if s.saved_toast > 0.0 {
                s.saved_toast -= dt;
            }
        }
        // NativeApp: no per-frame decay needed (state driven by events)
    }

    // Sync dock open-state indicators
    let has_terminal = windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Terminal));
    let has_browser  = windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Browser));
    let has_sheets   = windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Sheets));
    let has_docs     = windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Docs));
    let has_media    = windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Media));
    let has_settings = windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Settings));
    dock.sync_open_state(has_terminal, has_browser, has_sheets, has_docs, has_media, has_settings, agent_mode, sidebar_visible, private_mode);
    dock.hovered_idx = dock.hit_test(mouse_x, mouse_y, w, h);

    // Init / drive sidebar slide
    if sidebar.slide_x == f32::MAX {
        sidebar.slide_x = w;
        sidebar.slide_target_x = w;
    }
    let sidebar_w = sidebar::SIDEBAR_WIDTH;
    sidebar.slide_target_x = if sidebar_visible { w - sidebar_w } else { w };
}

// ─────────────────────────────────────────────────────────────────────────────
//  9-layer compositor render stack
// ─────────────────────────────────────────────────────────────────────────────

pub fn render(
    renderer: &mut Renderer,
    pixmap: &mut tiny_skia::Pixmap,
    sidebar: &mut Sidebar,
    terminal: &mut Terminal,
    browser_panel: &mut BrowserPanel,
    windows: &[FloatingWindow],
    toasts: &[Toast],
    dock: &Dock,
    agent_mode: bool,
    private_mode: bool,
    activity_text: &str,
    menubar_clock: &str,
    mouse_x: f32,
    mouse_y: f32,
    w: f32,
    h: f32,
) {
    let t = renderer.theme.clone();

    // Layer 1: Desktop wallpaper
    desktop::render_desktop(renderer, pixmap, w, h);

    // Layer 2: Floating windows (back to front)
    for i in 0..windows.len() {
        let win = &windows[i];
        if win.is_minimized { continue; }
        let is_hover_close = win.hit_close(mouse_x, mouse_y);
        render_window_chrome(renderer, pixmap, win, is_hover_close);

        let (cx, cy, cw, ch) = win.content_rect();
        match &win.content {
            WindowContent::Terminal => {
                terminal.render(renderer, pixmap, cx, cy, cw, ch);
            }
            WindowContent::Browser => {
                browser_panel.render(renderer, pixmap, cx, cy, cw, ch);
            }
            WindowContent::DynamicApp(_) => {
                let win_ref = &windows[i];
                window_manager::render_dynamic_app(renderer, pixmap, win_ref, mouse_x, mouse_y);
            }
            WindowContent::Settings(ref s) => {
                s.render(renderer, pixmap, cx, cy, cw, ch, sidebar.cursor_visible);
            }
            WindowContent::NativeApp(ref app) => {
                app.render(renderer, pixmap, cx, cy, cw, ch, sidebar.cursor_visible);
            }
        }
    }

    // Layer 3: Agent mode border (2px accent)
    if agent_mode {
        renderer.fill_rect(pixmap, 0.0, 0.0, w, 2.0, t.agent_active);
        renderer.fill_rect(pixmap, 0.0, h - 2.0, w, 2.0, t.agent_active);
        renderer.fill_rect(pixmap, 0.0, 0.0, 2.0, h, t.agent_active);
        renderer.fill_rect(pixmap, w - 2.0, 0.0, 2.0, h, t.agent_active);
    }

    // Layer 4: Menu bar
    let status = sidebar.status;
    desktop::render_menu_bar(renderer, pixmap, w, &status, activity_text, private_mode, menubar_clock);

    // Layer 5: Dock
    dock::render_dock(renderer, pixmap, dock, w, h);

    // Layer 6: AI Sidebar overlay
    if sidebar.slide_x < w {
        sidebar.render(renderer, pixmap, sidebar.slide_x, MENU_BAR_H, h - MENU_BAR_H - DOCK_HEIGHT);
    }

    // Layer 7: HITL approval overlay
    if sidebar.status == soma_common::AgentStatus::AwaitingApproval {
        sidebar.render_approval_overlay(renderer, pixmap, w, h);
    }

    // Layer 8: Detail modal
    if sidebar.expanded_msg_idx.is_some() {
        sidebar.render_expanded_msg(renderer, pixmap, w, h);
    }

    // Layer 9: Toast notifications (top-right)
    render_toasts(renderer, pixmap, toasts, w);
}

fn render_toasts(
    renderer: &mut Renderer,
    pixmap: &mut tiny_skia::Pixmap,
    toasts: &[Toast],
    w: f32,
) {
    let mut toast_y = MENU_BAR_H + 8.0;
    for toast in toasts {
        let alpha = if toast.remaining < 0.5 {
            (toast.remaining / 0.5 * 255.0) as u8
        } else {
            255
        };
        let tw = 260.0_f32.min(w - 20.0);
        let tx = w - tw - 10.0;
        renderer.fill_rounded_rect(
            pixmap, tx, toast_y, tw, 28.0, 8.0,
            [toast.color[0], toast.color[1], toast.color[2], (alpha / 4).max(20)],
        );
        renderer.draw_text(
            pixmap, &toast.message, tx + 10.0, toast_y + 7.0, tw - 20.0, 10.0,
            [toast.color[0], toast.color[1], toast.color[2], alpha],
        );
        toast_y += 34.0;
    }
}

/// Check whether a continuous redraw is needed.
pub fn needs_continuous_redraw(
    got_agent_msg: bool,
    pty_data: bool,
    toasts: &[Toast],
    sidebar: &Sidebar,
    agent_mode: bool,
) -> bool {
    let sidebar_animating = sidebar.slide_x != sidebar.slide_target_x;
    got_agent_msg
        || pty_data
        || !toasts.is_empty()
        || sidebar_animating
        || agent_mode
        || matches!(sidebar.status, soma_common::AgentStatus::Thinking | soma_common::AgentStatus::Executing)
}

/// Copy pixmap data to a winit softbuffer surface for presentation.
#[cfg(feature = "winit-backend")]
pub fn present_winit(
    surface: &mut softbuffer::Surface<std::sync::Arc<winit::window::Window>, std::sync::Arc<winit::window::Window>>,
    pixmap: &tiny_skia::Pixmap,
) {
    let mut buffer = surface.buffer_mut().unwrap();
    let pixels = pixmap.pixels();
    for (i, pixel) in pixels.iter().enumerate() {
        buffer[i] = ((pixel.red() as u32) << 16)
            | ((pixel.green() as u32) << 8)
            | (pixel.blue() as u32);
    }
    let _ = buffer.present();
}

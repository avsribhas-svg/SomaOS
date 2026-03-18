//! macOS-style dock bar for the SomaOS desktop environment.
//!
//! Renders a frosted-glass pill at the bottom of the screen with app icons.
//! Supports hover highlights, open-app indicator dots, and an agent-mode glow.

use crate::renderer::Renderer;
use crate::window_manager::WindowContentType;
use tiny_skia::Pixmap;

pub const DOCK_HEIGHT: f32 = 72.0;
pub const DOCK_ITEM_W: f32  = 56.0;
pub const DOCK_ITEM_PAD: f32 = 8.0;
pub const DOCK_RADIUS: f32  = 16.0;

// ─────────────────────────────────────────────────────────────────────────────
//  Dock action
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum DockAction {
    OpenWindow(WindowContentType),
    ToggleSidebar,
    ToggleAgentMode,
    TogglePrivateMode,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Dock data
// ─────────────────────────────────────────────────────────────────────────────

pub struct DockApp {
    pub name: String,
    pub icon_label: String, // short label painted on icon face
    pub action: DockAction,
    pub is_open: bool,      // show indicator dot
    pub is_active: bool,    // glow (agent mode or private mode active)
}

pub struct Dock {
    pub apps: Vec<DockApp>,
    pub hovered_idx: Option<usize>,
}

impl Dock {
    pub fn new() -> Self {
        Self {
            apps: vec![
                DockApp {
                    name: "Terminal".into(),
                    icon_label: ">_".into(),
                    action: DockAction::OpenWindow(WindowContentType::Terminal),
                    is_open: false,
                    is_active: false,
                },
                DockApp {
                    name: "Browser".into(),
                    icon_label: "W".into(),
                    action: DockAction::OpenWindow(WindowContentType::Browser),
                    is_open: false,
                    is_active: false,
                },
                DockApp {
                    name: "Sheets".into(),
                    icon_label: "Sh".into(),
                    action: DockAction::OpenWindow(WindowContentType::Sheets),
                    is_open: false,
                    is_active: false,
                },
                DockApp {
                    name: "Docs".into(),
                    icon_label: "Dc".into(),
                    action: DockAction::OpenWindow(WindowContentType::Docs),
                    is_open: false,
                    is_active: false,
                },
                DockApp {
                    name: "Settings".into(),
                    icon_label: "⚙".into(),
                    action: DockAction::OpenWindow(WindowContentType::Settings),
                    is_open: false,
                    is_active: false,
                },
                DockApp {
                    name: "AI Agent".into(),
                    icon_label: "AI".into(),
                    action: DockAction::ToggleAgentMode,
                    is_open: false,
                    is_active: false,
                },
                DockApp {
                    name: "Sidebar".into(),
                    icon_label: ">>".into(),
                    action: DockAction::ToggleSidebar,
                    is_open: false,
                    is_active: false,
                },
                DockApp {
                    name: "Private".into(),
                    icon_label: "PVT".into(),
                    action: DockAction::TogglePrivateMode,
                    is_open: false,
                    is_active: false,
                },
            ],
            hovered_idx: None,
        }
    }

    /// Returns the bounding rect (x, y, w, h) of the dock pill.
    pub fn pill_rect(screen_w: f32, screen_h: f32, n_apps: usize) -> (f32, f32, f32, f32) {
        let pill_w = n_apps as f32 * (DOCK_ITEM_W + DOCK_ITEM_PAD) + DOCK_ITEM_PAD;
        let pill_h = DOCK_HEIGHT - 8.0;
        let pill_x = (screen_w - pill_w) / 2.0;
        let pill_y = screen_h - DOCK_HEIGHT + 4.0;
        (pill_x, pill_y, pill_w, pill_h)
    }

    /// Returns which app index was hit at (mx, my), if any.
    pub fn hit_test(&self, mx: f32, my: f32, screen_w: f32, screen_h: f32) -> Option<usize> {
        let n = self.apps.len();
        let (px, py, _, ph) = Self::pill_rect(screen_w, screen_h, n);
        if my < py || my > py + ph {
            return None;
        }
        for (i, _) in self.apps.iter().enumerate() {
            let ix = px + DOCK_ITEM_PAD + i as f32 * (DOCK_ITEM_W + DOCK_ITEM_PAD);
            if mx >= ix && mx < ix + DOCK_ITEM_W {
                return Some(i);
            }
        }
        None
    }

    /// Update open-state of all dock apps based on the current window list and mode flags.
    pub fn sync_open_state(
        &mut self,
        has_terminal: bool,
        has_browser: bool,
        has_sheets: bool,
        has_docs: bool,
        has_settings: bool,
        agent_mode: bool,
        sidebar_visible: bool,
        private_mode: bool,
    ) {
        for app in &mut self.apps {
            match &app.action {
                DockAction::OpenWindow(WindowContentType::Terminal) => {
                    app.is_open = has_terminal;
                    app.is_active = false;
                }
                DockAction::OpenWindow(WindowContentType::Browser) => {
                    app.is_open = has_browser;
                    app.is_active = false;
                }
                DockAction::OpenWindow(WindowContentType::Sheets) => {
                    app.is_open = has_sheets;
                    app.is_active = false;
                }
                DockAction::OpenWindow(WindowContentType::Docs) => {
                    app.is_open = has_docs;
                    app.is_active = false;
                }
                DockAction::OpenWindow(WindowContentType::Settings) => {
                    app.is_open = has_settings;
                    app.is_active = false;
                }
                DockAction::ToggleAgentMode => {
                    app.is_open = agent_mode;
                    app.is_active = agent_mode;
                }
                DockAction::ToggleSidebar => {
                    app.is_open = sidebar_visible;
                    app.is_active = false;
                }
                DockAction::TogglePrivateMode => {
                    app.is_open = private_mode;
                    app.is_active = private_mode;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Dock rendering
// ─────────────────────────────────────────────────────────────────────────────

pub fn render_dock(
    renderer: &mut Renderer,
    pixmap: &mut Pixmap,
    dock: &Dock,
    screen_w: f32,
    screen_h: f32,
) {
    let t = renderer.theme.clone();
    let n = dock.apps.len();
    let (px, py, pw, ph) = Dock::pill_rect(screen_w, screen_h, n);

    // Drop shadow for the pill
    renderer.fill_rounded_rect(pixmap, px + 4.0, py + 8.0, pw, ph, DOCK_RADIUS, [0, 0, 0, 50]);
    renderer.fill_rounded_rect(pixmap, px + 1.0, py + 2.0, pw, ph, DOCK_RADIUS, [0, 0, 0, 40]);

    // Crisp outer highlight rim
    renderer.fill_rounded_rect(pixmap, px - 1.0, py - 1.0, pw + 2.0, ph + 2.0, DOCK_RADIUS + 1.0, [255, 255, 255, 20]);

    // Pill background — frosted glass 
    renderer.fill_rounded_rect(pixmap, px, py, pw, ph, DOCK_RADIUS, t.bg_dock);
    
    // Inner border styling
    renderer.stroke_rect(pixmap, px, py, pw, ph, [255, 255, 255, 8]);

    for (i, app) in dock.apps.iter().enumerate() {
        let ix = px + DOCK_ITEM_PAD + i as f32 * (DOCK_ITEM_W + DOCK_ITEM_PAD);
        let iy = py + 4.0;
        let item_h = ph - 14.0; // leave room for dot indicator below

        // Hover highlight
        let is_hovered = dock.hovered_idx == Some(i);
        if is_hovered {
            renderer.fill_rounded_rect(pixmap, ix - 2.0, iy, DOCK_ITEM_W + 4.0, item_h, 10.0,
                [255, 255, 255, 18]);
        }

        // Agent-mode or private-mode glow ring
        if app.is_active {
            renderer.fill_rounded_rect(pixmap, ix - 3.0, iy - 3.0,
                DOCK_ITEM_W + 6.0, item_h + 6.0, 12.0,
                [t.agent_active[0], t.agent_active[1], t.agent_active[2], 40]);
        }

        // Icon label (centred within item)
        let label_x = ix + (DOCK_ITEM_W - app.icon_label.len() as f32 * 7.5) / 2.0;
        let icon_color = if app.is_active { t.agent_active } else { t.text_primary };
        renderer.draw_text(pixmap, &app.icon_label, label_x, iy + 10.0, DOCK_ITEM_W, 14.0, icon_color);

        // App name label (small, below icon)
        let name_x = ix + (DOCK_ITEM_W - app.name.len() as f32 * 5.0) / 2.0;
        renderer.draw_text(pixmap, &app.name, name_x, iy + item_h - 14.0,
            DOCK_ITEM_W, 8.0, t.text_muted);

        // Open indicator dot (below pill)
        if app.is_open {
            let dot_color = if app.is_active { t.agent_active } else { t.accent };
            
            // Subtle neon glow
            renderer.fill_rounded_rect(pixmap,
                ix + DOCK_ITEM_W / 2.0 - 4.0,
                py + ph - 5.5,
                8.0, 8.0, 4.0,
                [dot_color[0], dot_color[1], dot_color[2], 80]);
                
            // Core bright dot
            renderer.fill_rounded_rect(pixmap,
                ix + DOCK_ITEM_W / 2.0 - 2.0,
                py + ph - 3.5,
                4.0, 4.0, 2.0, dot_color);
        }
    }
}

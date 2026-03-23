//! Input event dispatch for the compositor.
//!
//! Extracted from main.rs — all keyboard, mouse click, and scroll routing
//! lives here as free functions that borrow the compositor state.

use crate::dock::DockAction;
use crate::sidebar::Sidebar;
use crate::terminal::Terminal;
use crate::browser_panel::BrowserPanel;
use crate::desktop::MENU_BAR_H;
use crate::dock::{Dock, DOCK_HEIGHT};
use crate::settings_app;
use crate::window_manager::{
    FloatingWindow, NativeAppContent, WindowContent, WindowContentType, WindowId,
};
use crate::compositor::Toast;
use soma_common::CompositorMessage;

// ─────────────────────────────────────────────────────────────────────────────
//  Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

pub fn focused_content_type(windows: &[FloatingWindow]) -> Option<WindowContentType> {
    windows.iter().rev().find(|w| w.is_focused)
        .and_then(|w| w.content.content_type())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Window management helpers (shared between winit + DRM)
// ─────────────────────────────────────────────────────────────────────────────

pub fn open_or_focus_window(
    windows: &mut Vec<FloatingWindow>,
    next_id: &mut WindowId,
    content_type: WindowContentType,
    send: &dyn Fn(CompositorMessage),
    private_mode: bool,
) {
    if let Some(win) = windows.iter().find(|w| w.content.content_type() == Some(content_type)) {
        let id = win.id;
        bring_to_front(windows, id, send, private_mode);
        return;
    }
    let content = match content_type {
        WindowContentType::Terminal => WindowContent::Terminal,
        WindowContentType::Browser  => WindowContent::Browser,
        WindowContentType::Settings => WindowContent::Settings(settings_app::SettingsApp::new()),
        WindowContentType::Sheets   => WindowContent::NativeApp(
            Box::new(crate::sheets::SheetsApp::new("Sheet 1"))
        ),
        WindowContentType::Docs     => WindowContent::NativeApp(
            Box::new(crate::docs::DocsApp::new("Untitled Document"))
        ),
        WindowContentType::Media    => WindowContent::NativeApp(
            Box::new(crate::media::MediaApp::new("Soma Media"))
        ),
    };
    let id = *next_id;
    *next_id += 1;
    let offset = (windows.len() as f32 * 30.0) % 200.0;
    let win = FloatingWindow::new(id, content, 80.0 + offset, MENU_BAR_H + 20.0 + offset);
    windows.push(win);
    bring_to_front(windows, id, send, private_mode);
    if !private_mode {
        let title = windows.last().map(|w| w.title.clone()).unwrap_or_default();
        send(CompositorMessage::DesktopEvent {
            event_type: "window_opened".to_string(),
            window_title: title,
            timestamp: unix_secs(),
        });
    }
}

pub fn close_window(
    windows: &mut Vec<FloatingWindow>,
    window_id: WindowId,
    send: &dyn Fn(CompositorMessage),
    private_mode: bool,
) {
    if let Some(pos) = windows.iter().position(|w| w.id == window_id) {
        let title = windows[pos].title.clone();
        windows.remove(pos);
        if !private_mode {
            send(CompositorMessage::DesktopEvent {
                event_type: "window_closed".to_string(),
                window_title: title,
                timestamp: unix_secs(),
            });
        }
        if let Some(top) = windows.last_mut() {
            top.is_focused = true;
        }
    }
}

pub fn close_focused_window(
    windows: &mut Vec<FloatingWindow>,
    send: &dyn Fn(CompositorMessage),
    private_mode: bool,
) {
    if let Some(win) = windows.iter().rev().find(|w| w.is_focused) {
        let id = win.id;
        close_window(windows, id, send, private_mode);
    }
}

pub fn bring_to_front(
    windows: &mut Vec<FloatingWindow>,
    window_id: WindowId,
    send: &dyn Fn(CompositorMessage),
    private_mode: bool,
) {
    for w in windows.iter_mut() { w.is_focused = false; }
    if let Some(pos) = windows.iter().position(|w| w.id == window_id) {
        let mut win = windows.remove(pos);
        win.is_focused = true;
        let title = win.title.clone();
        windows.push(win);
        if !private_mode {
            send(CompositorMessage::DesktopEvent {
                event_type: "window_focused".to_string(),
                window_title: title,
                timestamp: unix_secs(),
            });
        }
    }
}

pub fn handle_desktop_action(
    windows: &mut Vec<FloatingWindow>,
    next_id: &mut WindowId,
    action: &str,
    send: &dyn Fn(CompositorMessage),
    private_mode: bool,
) {
    let parts: Vec<&str> = action.splitn(2, ':').collect();
    match parts[0] {
        "open_window" => {
            if parts.len() > 1 {
                match parts[1] {
                    "terminal" => open_or_focus_window(windows, next_id, WindowContentType::Terminal, send, private_mode),
                    "browser"  => open_or_focus_window(windows, next_id, WindowContentType::Browser, send, private_mode),
                    "settings" => open_or_focus_window(windows, next_id, WindowContentType::Settings, send, private_mode),
                    "sheets"   => open_or_focus_window(windows, next_id, WindowContentType::Sheets, send, private_mode),
                    "docs"     => open_or_focus_window(windows, next_id, WindowContentType::Docs, send, private_mode),
                    _ => {}
                }
            }
        }
        "close_window" => {
            if parts.len() > 1 {
                if let Some(w) = windows.iter().find(|w| w.title == parts[1]) {
                    let id = w.id;
                    close_window(windows, id, send, private_mode);
                }
            }
        }
        "focus_window" => {
            if parts.len() > 1 {
                if let Some(w) = windows.iter().find(|w| w.title == parts[1]) {
                    let id = w.id;
                    bring_to_front(windows, id, send, private_mode);
                }
            }
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Dock click handler
// ─────────────────────────────────────────────────────────────────────────────

/// Returns `true` if the dock consumed this click.
pub fn handle_dock_click(
    dock: &Dock,
    idx: usize,
    windows: &mut Vec<FloatingWindow>,
    next_id: &mut WindowId,
    sidebar_visible: &mut bool,
    agent_mode: &mut bool,
    private_mode: &mut bool,
    activity_text: &mut String,
    send: &dyn Fn(CompositorMessage),
) {
    if idx >= dock.apps.len() { return; }
    match &dock.apps[idx].action {
        DockAction::OpenWindow(ct) => {
            open_or_focus_window(windows, next_id, *ct, send, *private_mode);
        }
        DockAction::ToggleAgentMode => {
            *agent_mode = !*agent_mode;
            if *agent_mode {
                *activity_text = "Agent mode active".to_string();
            } else {
                activity_text.clear();
            }
        }
        DockAction::ToggleSidebar => {
            *sidebar_visible = !*sidebar_visible;
        }
        DockAction::TogglePrivateMode => {
            *private_mode = !*private_mode;
            send(CompositorMessage::PrivateModeChanged { active: *private_mode });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Mouse click handler (priority-based routing)
// ─────────────────────────────────────────────────────────────────────────────

/// Handle a mouse click at (mx, my). Returns an optional message to send.
#[allow(clippy::too_many_arguments)]
pub fn handle_mouse_click(
    mx: f32,
    my: f32,
    total_w: f32,
    total_h: f32,
    sidebar: &mut Sidebar,
    sidebar_visible: &mut bool,
    dock: &Dock,
    windows: &mut Vec<FloatingWindow>,
    next_id: &mut WindowId,
    agent_mode: &mut bool,
    private_mode: &mut bool,
    activity_text: &mut String,
    dragging_window: &mut Option<WindowId>,
    send: &dyn Fn(CompositorMessage),
) {
    // 1. HITL overlay consumes all clicks
    if sidebar.status == soma_common::AgentStatus::AwaitingApproval {
        if let Some(msg) = sidebar.on_approve() {
            send(msg);
        }
        return;
    }

    // 2. Expanded modal dismiss
    if sidebar.expanded_msg_idx.is_some() {
        sidebar.expanded_msg_idx = None;
        return;
    }

    // 3. Sidebar overlay
    if *sidebar_visible && sidebar.slide_x < total_w && mx >= sidebar.slide_x {
        let rel_x = mx - sidebar.slide_x;
        let sidebar_h = total_h - MENU_BAR_H - DOCK_HEIGHT;
        let rel_y = my - MENU_BAR_H;
        if let Some(msg) = sidebar.on_settings_click(rel_x, rel_y) {
            send(msg);
        } else if let Some(msg) = sidebar.on_registry_click_with_height(rel_y, sidebar_h) {
            send(msg);
        } else {
            sidebar.on_sidebar_click(rel_x, rel_y, sidebar_h);
        }
        return;
    }

    // 4. Dock
    if let Some(idx) = dock.hit_test(mx, my, total_w, total_h) {
        handle_dock_click(dock, idx, windows, next_id, sidebar_visible, agent_mode, private_mode, activity_text, send);
        return;
    }

    // 5. Menu bar
    if my < MENU_BAR_H {
        return;
    }

    // 6. Windows (top to back)
    let mut clicked_window: Option<(WindowId, bool)> = None;
    for win in windows.iter().rev() {
        if win.is_minimized { continue; }
        if !win.hit_frame(mx, my) { continue; }
        if win.hit_close(mx, my) {
            clicked_window = Some((win.id, true));
            break;
        }
        if win.hit_titlebar(mx, my) {
            clicked_window = Some((win.id, false));
            break;
        }
        clicked_window = Some((win.id, false));
        break;
    }

    if let Some((id, is_close)) = clicked_window {
        if is_close {
            close_window(windows, id, send, *private_mode);
        } else {
            bring_to_front(windows, id, send, *private_mode);
            if let Some(win) = windows.iter().find(|w| w.id == id) {
                if win.hit_titlebar(mx, my) {
                    if let Some(w) = windows.iter_mut().find(|w| w.id == id) {
                        w.drag_offset_x = mx - w.x;
                        w.drag_offset_y = my - w.y;
                    }
                    *dragging_window = Some(id);
                } else {
                    // Content area click — route to Settings or NativeApp
                    let rel_x = mx - win.x;
                    let rel_y = my - win.y - crate::window_manager::FloatingWindow::TITLE_BAR_H;
                    if let Some(w) = windows.last_mut() {
                        match w.content {
                            WindowContent::Settings(ref mut s) => {
                                if let Some(msg) = s.on_click(rel_x, rel_y, w.width) {
                                    send(msg);
                                }
                            }
                            WindowContent::NativeApp(ref mut app) => {
                                if let Some(state) = app.on_click(rel_x, rel_y) {
                                    send(CompositorMessage::AppStateChanged {
                                        window_id: w.id,
                                        state,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Scroll handler
// ─────────────────────────────────────────────────────────────────────────────

pub fn handle_scroll(
    scroll_amount: f32,
    mouse_x: f32,
    mouse_y: f32,
    sidebar_visible: bool,
    sidebar: &mut Sidebar,
    terminal: &mut Terminal,
    browser_panel: &mut BrowserPanel,
    windows: &[FloatingWindow],
) {
    if sidebar_visible && mouse_x >= sidebar.slide_x {
        sidebar.scroll(scroll_amount);
    } else {
        let content_type = windows.iter().rev()
            .find(|w| w.hit_frame(mouse_x, mouse_y))
            .and_then(|w| w.content.content_type());
        match content_type {
            Some(WindowContentType::Terminal) => terminal.scroll(scroll_amount),
            Some(WindowContentType::Browser) => browser_panel.scroll(scroll_amount),
            _ => {}
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Agent message handler
// ─────────────────────────────────────────────────────────────────────────────

/// Process all pending agent messages. Returns `true` if any messages were received.
#[allow(clippy::too_many_arguments)]
pub fn poll_agent_messages(
    agent_rx: &Option<std::sync::Arc<std::sync::Mutex<crate::ipc_client::AgentReceiver>>>,
    sidebar: &mut Sidebar,
    terminal: &mut Terminal,
    browser_panel: &mut BrowserPanel,
    toasts: &mut Vec<Toast>,
    windows: &mut Vec<FloatingWindow>,
    next_id: &mut WindowId,
    agent_mode: &mut bool,
    activity_text: &mut String,
    send: &dyn Fn(CompositorMessage),
    private_mode: bool,
) -> bool {
    let messages: Vec<soma_common::AgentMessage> = if let Some(rx) = agent_rx {
        if let Ok(mut rx) = rx.try_lock() {
            let mut msgs = Vec::new();
            while let Ok(msg) = rx.try_recv() {
                msgs.push(msg);
            }
            msgs
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let got_message = !messages.is_empty();
    for msg in messages {
        match &msg {
            soma_common::AgentMessage::DirectOutput { result, .. } => {
                terminal.add_output(&result.stdout, &result.stderr);
            }
            soma_common::AgentMessage::ExecutionComplete { results, .. } => {
                let ok = results.iter().filter(|r| r.success).count();
                let total = results.len();
                if ok == total {
                    toasts.push(Toast::new(format!("v Task done ({}/{})", ok, total), [74, 222, 128, 255]));
                } else {
                    toasts.push(Toast::new(format!("! Task done ({}/{})", ok, total), [248, 113, 113, 255]));
                }
            }
            soma_common::AgentMessage::Error { message, .. } => {
                toasts.push(Toast::new(format!("! {}", &message[..message.len().min(40)]), [248, 113, 113, 255]));
            }
            soma_common::AgentMessage::BrowserUpdate { url, title, screenshot_base64 } => {
                browser_panel.update(url.clone(), title.clone(), screenshot_base64.as_deref());
                if !windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Browser)) {
                    open_or_focus_window(windows, next_id, WindowContentType::Browser, send, private_mode);
                }
            }
            soma_common::AgentMessage::ConfigUpdated { provider, model } => {
                toasts.push(Toast::new(format!("Model: {} / {}", provider, model), [99, 102, 241, 255]));
            }
            soma_common::AgentMessage::AgentModeStarted { task, .. } => {
                *agent_mode = true;
                *activity_text = task.clone();
            }
            soma_common::AgentMessage::AgentModeEnded => {
                *agent_mode = false;
                activity_text.clear();
            }
            soma_common::AgentMessage::SpawnApp { title, app_id, description, widgets_json } => {
                spawn_dynamic_app(windows, next_id, toasts, title.clone(), app_id.clone(), description.clone(), widgets_json.clone());
            }
            soma_common::AgentMessage::DesktopAction { action } => {
                let action = action.clone();
                handle_desktop_action(windows, next_id, &action, send, private_mode);
            }
            soma_common::AgentMessage::ActivityUpdate { text } => {
                *activity_text = text.clone();
            }
            soma_common::AgentMessage::AppAction { window_id, action, params } => {
                let (wid, act, prm) = (*window_id, action.clone(), params.clone());

                if act == "create_and_populate" {
                    // Agent wants to create a new NativeApp window (window_id == 0 sentinel).
                    // Dispatch to Sheets or Docs based on the "app_type" param.
                    let app_type = prm["app_type"].as_str().unwrap_or("sheets");
                    let title_default = if app_type == "docs" { "Untitled Document" } else { "Sheet 1" };
                    let title = prm["title"].as_str().unwrap_or(title_default).to_string();

                    let mut app: Box<dyn NativeAppContent> = if app_type == "docs" {
                        let mut a = Box::new(crate::docs::DocsApp::new(&title));
                        if let Some(blocks) = prm["blocks"].as_array() {
                            for block in blocks {
                                a.execute_action("append_block", block);
                            }
                        }
                        a
                    } else {
                        let mut a = Box::new(crate::sheets::SheetsApp::new(&title));
                        if let Some(obj) = prm["cells"].as_object() {
                            for (cell_ref, cell_val) in obj {
                                let value_str = match cell_val {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                a.execute_action("write_cell", &serde_json::json!({
                                    "cell": cell_ref,
                                    "value": value_str,
                                }));
                            }
                        }
                        a
                    };

                    let state = app.describe_state();
                    let new_id = *next_id;
                    *next_id += 1;
                    let offset = (windows.len() as f32 * 30.0) % 200.0;
                    let mut win = FloatingWindow::new(
                        new_id,
                        WindowContent::NativeApp(app),
                        80.0 + offset,
                        crate::desktop::MENU_BAR_H + 20.0 + offset,
                    );
                    win.title = title;
                    win.agent_owned = true;
                    for w in windows.iter_mut() { w.is_focused = false; }
                    win.is_focused = true;
                    windows.push(win);

                    // Push state back so agent cache gets the real window_id
                    send(CompositorMessage::AppStateChanged { window_id: new_id, state });

                    if !private_mode {
                        send(CompositorMessage::DesktopEvent {
                            event_type: "window_opened".to_string(),
                            window_title: windows.last().map(|w| w.title.clone()).unwrap_or_default(),
                            timestamp: unix_secs(),
                        });
                    }
                } else if let Some(win) = windows.iter_mut().find(|w| w.id == wid) {
                    if let WindowContent::NativeApp(ref mut app) = win.content {
                        let result = app.execute_action(&act, &prm);
                        if result.success {
                            let state = app.describe_state();
                            send(CompositorMessage::AppStateChanged { window_id: wid, state });
                        }
                    }
                }
            }
            _ => {}
        }
        if let Some(follow_up) = sidebar.handle_agent_message(msg) {
            send(follow_up);
        }
    }
    got_message
}

fn spawn_dynamic_app(
    windows: &mut Vec<FloatingWindow>,
    next_id: &mut WindowId,
    toasts: &mut Vec<Toast>,
    title: String,
    app_id: String,
    description: String,
    widgets_json: String,
) {
    use crate::window_manager::AppDef;
    let app_def = match AppDef::from_json(&format!(
        r#"{{"app_id":"{}","description":"{}","widgets":{}}}"#,
        app_id, description, widgets_json
    )) {
        Ok(d) => d,
        Err(e) => {
            toasts.push(Toast::new(format!("SpawnApp error: {}", e), [248, 113, 113, 255]));
            return;
        }
    };
    let id = *next_id;
    *next_id += 1;
    let offset = (windows.len() as f32 * 30.0) % 200.0;
    let mut win = FloatingWindow::new(id, WindowContent::DynamicApp(app_def), 120.0 + offset, MENU_BAR_H + 40.0 + offset);
    win.title = title;
    win.agent_owned = true;
    for w in windows.iter_mut() { w.is_focused = false; }
    win.is_focused = true;
    windows.push(win);
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

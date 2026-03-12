mod backend;
mod browser_panel;
mod desktop;
mod dock;
mod input;
mod ipc_client;
mod login;
mod renderer;
mod sidebar;
mod terminal;
mod window_manager;

use browser_panel::BrowserPanel;
use dock::{Dock, DockAction, DOCK_HEIGHT};
use desktop::MENU_BAR_H;
use log::info;
use renderer::Renderer;
use sidebar::Sidebar;
use std::sync::{Arc, Mutex};
use terminal::Terminal;
use window_manager::{
    FloatingWindow, WindowContent, WindowContentType, WindowId,
    render_window_chrome, render_dynamic_app, hit_dynamic_button,
};

use soma_common::CompositorMessage;

#[cfg(feature = "drm-backend")]
use login::{LoginResult, LoginScreen};
#[cfg(feature = "winit-backend")]
use std::num::NonZeroU32;
#[cfg(feature = "winit-backend")]
use winit::application::ApplicationHandler;
#[cfg(feature = "winit-backend")]
use winit::dpi::{LogicalSize, PhysicalSize};
#[cfg(feature = "winit-backend")]
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
#[cfg(feature = "winit-backend")]
use winit::event_loop::{ActiveEventLoop, EventLoop};
#[cfg(feature = "winit-backend")]
use winit::keyboard::{Key, ModifiersState, NamedKey};
#[cfg(feature = "winit-backend")]
use winit::window::{Window, WindowAttributes, WindowId as WinitWindowId};

/// Notification toast
struct Toast {
    message: String,
    color: [u8; 4],
    remaining: f32,
}

#[cfg(feature = "winit-backend")]
struct SomaApp {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    renderer: Renderer,
    sidebar: Sidebar,
    terminal: Terminal,
    browser_panel: BrowserPanel,
    // IPC
    agent_tx: Option<ipc_client::AgentSender>,
    agent_rx: Option<Arc<Mutex<ipc_client::AgentReceiver>>>,
    runtime: tokio::runtime::Handle,
    // Desktop window manager
    windows: Vec<FloatingWindow>,
    next_window_id: WindowId,
    dock: Dock,
    sidebar_visible: bool,
    agent_mode: bool,
    private_mode: bool,
    activity_text: String,
    menubar_clock: String,
    // Mouse state
    mouse_x: f32,
    mouse_y: f32,
    dragging_window: Option<WindowId>,
    hover_close_window: Option<WindowId>,
    // Toasts
    toasts: Vec<Toast>,
    // Keyboard modifiers
    modifiers: ModifiersState,
}

#[cfg(feature = "winit-backend")]
impl SomaApp {
    fn new(runtime: tokio::runtime::Handle) -> Self {
        Self {
            window: None,
            surface: None,
            renderer: Renderer::new(),
            sidebar: Sidebar::new(),
            terminal: Terminal::new(),
            browser_panel: BrowserPanel::new(),
            agent_tx: None,
            agent_rx: None,
            runtime,
            windows: Vec::new(),
            next_window_id: 1,
            dock: Dock::new(),
            sidebar_visible: false,
            agent_mode: false,
            private_mode: false,
            activity_text: String::new(),
            menubar_clock: Self::clock_string(),
            mouse_x: 0.0,
            mouse_y: 0.0,
            dragging_window: None,
            hover_close_window: None,
            toasts: Vec::new(),
            modifiers: ModifiersState::empty(),
        }
    }

    fn try_connect_agent(&mut self) {
        let rt = self.runtime.clone();
        match rt.block_on(ipc_client::connect_to_agent()) {
            Ok((tx, rx)) => {
                info!("Connected to soma-agent daemon");
                self.agent_tx = Some(tx);
                self.agent_rx = Some(Arc::new(Mutex::new(rx)));
            }
            Err(e) => {
                info!("Agent not available ({}). Running in standalone mode.", e);
            }
        }
    }

    fn send_to_agent(&self, msg: CompositorMessage) {
        if let Some(tx) = &self.agent_tx {
            let _ = tx.send(msg);
        }
    }

    fn add_toast(&mut self, message: String, color: [u8; 4]) {
        self.toasts.push(Toast {
            message,
            color,
            remaining: 3.5,
        });
    }

    fn clock_string() -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let hours = ((now / 3600) % 24) as u32;
        let minutes = ((now / 60) % 60) as u32;
        // Adjust for local timezone offset (rough: get from env or default UTC)
        format!("{:02}:{:02}", hours, minutes)
    }

    // ── Window management helpers ──────────────────────────────────────────

    fn open_or_focus_window(&mut self, content_type: WindowContentType) {
        // If a window of this type already exists, bring it to front
        if let Some(win) = self.windows.iter().find(|w| w.content.content_type() == Some(content_type)) {
            let id = win.id;
            self.bring_to_front(id);
            return;
        }
        // Create new window
        let content = match content_type {
            WindowContentType::Terminal => WindowContent::Terminal,
            WindowContentType::Browser => WindowContent::Browser,
        };
        let id = self.next_window_id;
        self.next_window_id += 1;
        let offset = (self.windows.len() as f32 * 30.0) % 200.0;
        let win = FloatingWindow::new(id, content, 80.0 + offset, MENU_BAR_H + 20.0 + offset);
        self.windows.push(win);
        self.bring_to_front(id);
        self.send_desktop_event("window_opened", &self.windows.last().unwrap().title.clone());
    }

    fn close_window(&mut self, window_id: WindowId) {
        if let Some(pos) = self.windows.iter().position(|w| w.id == window_id) {
            let title = self.windows[pos].title.clone();
            self.windows.remove(pos);
            self.send_desktop_event("window_closed", &title);
            // Focus the new top window
            if let Some(top) = self.windows.last_mut() {
                top.is_focused = true;
            }
        }
    }

    fn close_focused_window(&mut self) {
        if let Some(win) = self.windows.iter().rev().find(|w| w.is_focused) {
            let id = win.id;
            self.close_window(id);
        }
    }

    fn bring_to_front(&mut self, window_id: WindowId) {
        // Unfocus all, then move target to end and focus it
        for w in &mut self.windows {
            w.is_focused = false;
        }
        if let Some(pos) = self.windows.iter().position(|w| w.id == window_id) {
            let mut win = self.windows.remove(pos);
            win.is_focused = true;
            let title = win.title.clone();
            self.windows.push(win);
            self.send_desktop_event("window_focused", &title);
        }
    }

    fn spawn_dynamic_app(&mut self, title: String, app_id: String, description: String, widgets_json: String) {
        use window_manager::AppDef;
        let app_def = match AppDef::from_json(&format!(
            r#"{{"app_id":"{}","description":"{}","widgets":{}}}"#,
            app_id, description, widgets_json
        )) {
            Ok(d) => d,
            Err(e) => {
                self.add_toast(format!("SpawnApp error: {}", e), [248, 113, 113, 255]);
                return;
            }
        };
        let id = self.next_window_id;
        self.next_window_id += 1;
        let offset = (self.windows.len() as f32 * 30.0) % 200.0;
        let mut win = FloatingWindow::new(id, WindowContent::DynamicApp(app_def), 120.0 + offset, MENU_BAR_H + 40.0 + offset);
        win.title = title;
        win.agent_owned = true;
        // Unfocus all, focus new
        for w in &mut self.windows {
            w.is_focused = false;
        }
        win.is_focused = true;
        self.windows.push(win);
    }

    fn handle_desktop_action(&mut self, action: &str) {
        let parts: Vec<&str> = action.splitn(2, ':').collect();
        match parts[0] {
            "open_window" => {
                if parts.len() > 1 {
                    match parts[1] {
                        "terminal" => self.open_or_focus_window(WindowContentType::Terminal),
                        "browser" => self.open_or_focus_window(WindowContentType::Browser),
                        _ => {}
                    }
                }
            }
            "close_window" => {
                if parts.len() > 1 {
                    let title = parts[1];
                    if let Some(w) = self.windows.iter().find(|w| w.title == title) {
                        let id = w.id;
                        self.close_window(id);
                    }
                }
            }
            "focus_window" => {
                if parts.len() > 1 {
                    let title = parts[1];
                    if let Some(w) = self.windows.iter().find(|w| w.title == title) {
                        let id = w.id;
                        self.bring_to_front(id);
                    }
                }
            }
            _ => {}
        }
    }

    fn send_desktop_event(&self, event_type: &str, window_title: &str) {
        if self.private_mode { return; }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.send_to_agent(CompositorMessage::DesktopEvent {
            event_type: event_type.to_string(),
            window_title: window_title.to_string(),
            timestamp: ts,
        });
    }

    fn focused_window_content_type(&self) -> Option<WindowContentType> {
        self.windows.iter().rev().find(|w| w.is_focused)
            .and_then(|w| w.content.content_type())
    }

    fn poll_agent_messages(&mut self) -> bool {
        let messages: Vec<soma_common::AgentMessage> = if let Some(rx) = &self.agent_rx {
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
                    self.terminal.add_output(&result.stdout, &result.stderr);
                }
                soma_common::AgentMessage::ExecutionComplete { results, .. } => {
                    let ok = results.iter().filter(|r| r.success).count();
                    let total = results.len();
                    if ok == total {
                        self.add_toast(format!("v Task done ({}/{})", ok, total), [74, 222, 128, 255]);
                    } else {
                        self.add_toast(format!("! Task done ({}/{})", ok, total), [248, 113, 113, 255]);
                    }
                }
                soma_common::AgentMessage::Error { message, .. } => {
                    self.add_toast(format!("! {}", &message[..message.len().min(40)]), [248, 113, 113, 255]);
                }
                soma_common::AgentMessage::BrowserUpdate { url, title, screenshot_base64 } => {
                    self.browser_panel.update(url.clone(), title.clone(), screenshot_base64.as_deref());
                    // Auto-open browser window if not already open
                    if !self.windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Browser)) {
                        self.open_or_focus_window(WindowContentType::Browser);
                    }
                }
                soma_common::AgentMessage::ConfigUpdated { provider, model } => {
                    self.add_toast(format!("Model: {} / {}", provider, model), [99, 102, 241, 255]);
                }
                soma_common::AgentMessage::AgentModeStarted { task } => {
                    self.agent_mode = true;
                    self.activity_text = task.clone();
                }
                soma_common::AgentMessage::AgentModeEnded => {
                    self.agent_mode = false;
                    self.activity_text.clear();
                }
                soma_common::AgentMessage::SpawnApp { title, app_id, description, widgets_json } => {
                    self.spawn_dynamic_app(title.clone(), app_id.clone(), description.clone(), widgets_json.clone());
                }
                soma_common::AgentMessage::DesktopAction { action } => {
                    let action = action.clone();
                    self.handle_desktop_action(&action);
                }
                soma_common::AgentMessage::ActivityUpdate { text } => {
                    self.activity_text = text.clone();
                }
                _ => {}
            }
            self.sidebar.handle_agent_message(msg);
        }
        got_message
    }

    fn redraw(&mut self) {
        let window = match &self.window {
            Some(w) => w,
            None => return,
        };

        let size = window.inner_size();
        let width = size.width;
        let height = size.height;

        if width == 0 || height == 0 {
            return;
        }

        if let Some(surface) = &mut self.surface {
            let _ = surface.resize(
                NonZeroU32::new(width).unwrap(),
                NonZeroU32::new(height).unwrap(),
            );
        } else {
            return;
        }

        let mut pixmap = match tiny_skia::Pixmap::new(width, height) {
            Some(p) => p,
            None => return,
        };

        let w = width as f32;
        let h = height as f32;
        let dt = 1.0 / 60.0;

        // Update animations
        self.sidebar.update(dt);
        self.terminal.update(dt);

        // Poll PTY
        let pty_data = self.terminal.poll();

        // Poll agent
        let got_agent_msg = self.poll_agent_messages();

        // Update toasts
        self.toasts.retain_mut(|t| {
            t.remaining -= dt;
            t.remaining > 0.0
        });

        // Update clock periodically
        self.menubar_clock = Self::clock_string();

        // Sync dock state
        let has_terminal = self.windows.iter().any(|win| win.content.content_type() == Some(WindowContentType::Terminal));
        let has_browser = self.windows.iter().any(|win| win.content.content_type() == Some(WindowContentType::Browser));
        self.dock.sync_open_state(has_terminal, has_browser, self.agent_mode, self.sidebar_visible, self.private_mode);

        // Update dock hover
        self.dock.hovered_idx = self.dock.hit_test(self.mouse_x, self.mouse_y, w, h);

        // Update window close hover
        self.hover_close_window = None;
        for win in self.windows.iter().rev() {
            if win.hit_close(self.mouse_x, self.mouse_y) {
                self.hover_close_window = Some(win.id);
                break;
            }
        }

        // Initialise sidebar slide position if not yet set
        if self.sidebar.slide_x == f32::MAX {
            self.sidebar.slide_x = w; // off-screen right
            self.sidebar.slide_target_x = w;
        }
        // Drive sidebar slide target
        let sidebar_w = sidebar::SIDEBAR_WIDTH;
        self.sidebar.slide_target_x = if self.sidebar_visible { w - sidebar_w } else { w };

        // ═══════════════════════════════════════════════════════════════════
        //  9-LAYER COMPOSITOR STACK
        // ═══════════════════════════════════════════════════════════════════

        // Layer 1: Desktop wallpaper
        desktop::render_desktop(&mut self.renderer, &mut pixmap, w, h);

        // Layer 2: Floating windows (back to front)
        for i in 0..self.windows.len() {
            let win = &self.windows[i];
            if win.is_minimized { continue; }
            let is_hover_close = self.hover_close_window == Some(win.id);
            render_window_chrome(&mut self.renderer, &mut pixmap, win, is_hover_close);

            // Render window content
            let (cx, cy, cw, ch) = win.content_rect();
            match &win.content {
                WindowContent::Terminal => {
                    self.terminal.render(&mut self.renderer, &mut pixmap, cx, cy, cw, ch);
                }
                WindowContent::Browser => {
                    self.browser_panel.render(&mut self.renderer, &mut pixmap, cx, cy, cw, ch);
                }
                WindowContent::DynamicApp(_) => {
                    let win_ref = &self.windows[i];
                    render_dynamic_app(&mut self.renderer, &mut pixmap, win_ref, self.mouse_x, self.mouse_y);
                }
            }
        }

        // Layer 3: Agent mode tint (2px accent border)
        if self.agent_mode {
            let t = self.renderer.theme.clone();
            self.renderer.fill_rect(&mut pixmap, 0.0, 0.0, w, 2.0, t.agent_active);
            self.renderer.fill_rect(&mut pixmap, 0.0, h - 2.0, w, 2.0, t.agent_active);
            self.renderer.fill_rect(&mut pixmap, 0.0, 0.0, 2.0, h, t.agent_active);
            self.renderer.fill_rect(&mut pixmap, w - 2.0, 0.0, 2.0, h, t.agent_active);
        }

        // Layer 4: Menu bar
        let status = self.sidebar.status;
        desktop::render_menu_bar(
            &mut self.renderer, &mut pixmap, w,
            &status, &self.activity_text, self.private_mode, &self.menubar_clock,
        );

        // Layer 5: Dock
        dock::render_dock(&mut self.renderer, &mut pixmap, &self.dock, w, h);

        // Layer 6: AI Sidebar overlay (slide animation)
        if self.sidebar.slide_x < w {
            self.sidebar.render(&mut self.renderer, &mut pixmap, self.sidebar.slide_x, MENU_BAR_H, h - MENU_BAR_H - DOCK_HEIGHT);
        }

        // Layer 7: HITL overlay
        if self.sidebar.status == soma_common::AgentStatus::AwaitingApproval {
            self.sidebar.render_approval_overlay(&mut self.renderer, &mut pixmap, w, h);
        }

        // Layer 8: Detail modal
        if self.sidebar.expanded_msg_idx.is_some() {
            self.sidebar.render_expanded_msg(&mut self.renderer, &mut pixmap, w, h);
        }

        // Layer 9: Toast notifications (top-right)
        let mut toast_y = MENU_BAR_H + 8.0;
        for toast in &self.toasts {
            let alpha = if toast.remaining < 0.5 { (toast.remaining / 0.5 * 255.0) as u8 } else { 255 };
            let tw = 260.0_f32.min(w - 20.0);
            let tx = w - tw - 10.0;
            self.renderer.fill_rounded_rect(&mut pixmap, tx, toast_y, tw, 28.0, 8.0,
                [toast.color[0], toast.color[1], toast.color[2], (alpha / 4).max(20)]);
            self.renderer.draw_text(&mut pixmap, &toast.message, tx + 10.0, toast_y + 7.0, tw - 20.0, 10.0,
                [toast.color[0], toast.color[1], toast.color[2], alpha]);
            toast_y += 34.0;
        }

        // ═══════════════════════════════════════════════════════════════════
        //  PRESENT
        // ═══════════════════════════════════════════════════════════════════

        if let Some(surface) = &mut self.surface {
            let mut buffer = surface.buffer_mut().unwrap();
            let pixels = pixmap.pixels();
            for (i, pixel) in pixels.iter().enumerate() {
                buffer[i] = ((pixel.red() as u32) << 16)
                    | ((pixel.green() as u32) << 8)
                    | (pixel.blue() as u32);
            }
            let _ = buffer.present();
        }

        // Continuous redraw triggers
        let sidebar_animating = self.sidebar.slide_x != self.sidebar.slide_target_x;
        let needs_redraw = got_agent_msg
            || pty_data
            || !self.toasts.is_empty()
            || sidebar_animating
            || self.agent_mode
            || matches!(self.sidebar.status, soma_common::AgentStatus::Thinking | soma_common::AgentStatus::Executing);
        if needs_redraw {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}

#[cfg(feature = "winit-backend")]
impl ApplicationHandler for SomaApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = WindowAttributes::default()
            .with_title("SomaOS")
            .with_inner_size(LogicalSize::new(1200u32, 760u32))
            .with_min_inner_size(LogicalSize::new(600u32, 400u32));

        let window = Arc::new(event_loop.create_window(attrs).expect("Failed to create window"));
        let context =
            softbuffer::Context::new(window.clone()).expect("Failed to create softbuffer context");
        let surface =
            softbuffer::Surface::new(&context, window.clone()).expect("Failed to create surface");

        self.window = Some(window);
        self.surface = Some(surface);

        self.try_connect_agent();

        info!("SomaOS compositor window created");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WinitWindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(PhysicalSize { width, height }) => {
                if let Some(surface) = &mut self.surface {
                    if width > 0 && height > 0 {
                        let _ = surface.resize(
                            NonZeroU32::new(width).unwrap(),
                            NonZeroU32::new(height).unwrap(),
                        );
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                self.redraw();
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }

            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, .. },
                ..
            } => {
                let cmd = self.modifiers.super_key();
                let shift = self.modifiers.shift_key();
                let ctrl = self.modifiers.control_key();

                match &logical_key {
                    // ── Desktop shortcuts (Cmd+key) ──────────────────────────
                    Key::Named(NamedKey::Space) if cmd => {
                        // Cmd+Space: toggle sidebar
                        self.sidebar_visible = !self.sidebar_visible;
                    }
                    Key::Character(c) if cmd && !shift => {
                        match c.as_str() {
                            "t" => self.open_or_focus_window(WindowContentType::Terminal),
                            "w" => self.close_focused_window(),
                            _ => {}
                        }
                    }
                    Key::Character(c) if cmd && shift => {
                        match c.as_str() {
                            "A" | "a" => {
                                self.agent_mode = !self.agent_mode;
                                if self.agent_mode {
                                    self.activity_text = "Agent mode active".to_string();
                                } else {
                                    self.activity_text.clear();
                                }
                            }
                            "P" | "p" => {
                                self.private_mode = !self.private_mode;
                                self.send_to_agent(CompositorMessage::PrivateModeChanged { active: self.private_mode });
                            }
                            _ => {}
                        }
                    }

                    // ── Tab ───────────────────────────────────────────────────
                    Key::Named(NamedKey::Tab) => {
                        if self.focused_window_content_type() == Some(WindowContentType::Terminal) {
                            self.terminal.on_tab();
                        } else if self.sidebar_visible {
                            // Could cycle focus; for now no-op in sidebar
                        }
                    }

                    // ── Escape ────────────────────────────────────────────────
                    Key::Named(NamedKey::Escape) => {
                        if self.sidebar.expanded_msg_idx.is_some() {
                            self.sidebar.expanded_msg_idx = None;
                        } else if let Some(msg) = self.sidebar.on_reject() {
                            self.send_to_agent(msg);
                        } else if self.sidebar_visible {
                            self.sidebar_visible = false;
                        }
                    }

                    // ── Enter ─────────────────────────────────────────────────
                    Key::Named(NamedKey::Enter) => {
                        if self.sidebar_visible {
                            if let Some(msg) = self.sidebar.on_submit() {
                                self.send_to_agent(msg);
                            }
                        } else if self.focused_window_content_type() == Some(WindowContentType::Terminal) {
                            self.terminal.on_submit();
                        }
                    }

                    // ── Backspace ─────────────────────────────────────────────
                    Key::Named(NamedKey::Backspace) => {
                        if self.sidebar_visible {
                            self.sidebar.on_backspace();
                        } else if self.focused_window_content_type() == Some(WindowContentType::Terminal) {
                            self.terminal.on_backspace();
                        }
                    }

                    // ── Arrow keys ────────────────────────────────────────────
                    Key::Named(NamedKey::ArrowUp) => {
                        if self.focused_window_content_type() == Some(WindowContentType::Terminal) {
                            self.terminal.on_key_up();
                        }
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        if self.focused_window_content_type() == Some(WindowContentType::Terminal) {
                            self.terminal.on_key_down();
                        }
                    }

                    // ── Space (non-cmd) ───────────────────────────────────────
                    Key::Named(NamedKey::Space) => {
                        if self.sidebar_visible {
                            self.sidebar.on_char(' ');
                        } else if self.focused_window_content_type() == Some(WindowContentType::Terminal) {
                            self.terminal.on_char(' ');
                        }
                    }

                    // ── Character input ───────────────────────────────────────
                    Key::Character(c) if !cmd => {
                        if ctrl && self.focused_window_content_type() == Some(WindowContentType::Terminal) {
                            match c.as_str() {
                                "c" => self.terminal.on_ctrl_c(),
                                "d" => self.terminal.on_ctrl_d(),
                                "l" => self.terminal.on_ctrl_l(),
                                _ => {}
                            }
                        } else if self.sidebar_visible {
                            for ch in c.chars() {
                                self.sidebar.on_char(ch);
                            }
                        } else if self.focused_window_content_type() == Some(WindowContentType::Terminal) {
                            for ch in c.chars() {
                                self.terminal.on_char(ch);
                            }
                        }
                    }

                    _ => {}
                }

                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_x = position.x as f32;
                self.mouse_y = position.y as f32;

                // Handle window dragging
                if let Some(drag_id) = self.dragging_window {
                    if let Some(win) = self.windows.iter_mut().find(|w| w.id == drag_id) {
                        win.x = self.mouse_x - win.drag_offset_x;
                        win.y = self.mouse_y - win.drag_offset_y;
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }

            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let win_size = self.window.as_ref().map(|w| w.inner_size());
                let (total_w, total_h) = match win_size {
                    Some(s) => (s.width as f32, s.height as f32),
                    None => return,
                };

                match state {
                    ElementState::Pressed => {
                        // ── Priority-based mouse routing ──────────────────
                        let mx = self.mouse_x;
                        let my = self.mouse_y;

                        // 1. HITL overlay
                        if self.sidebar.status == soma_common::AgentStatus::AwaitingApproval {
                            if let Some(msg) = self.sidebar.on_approve() {
                                self.send_to_agent(msg);
                            }
                            // HITL consumes all clicks
                        }
                        // 2. Expanded modal
                        else if self.sidebar.expanded_msg_idx.is_some() {
                            self.sidebar.expanded_msg_idx = None;
                        }
                        // 3. Sidebar overlay
                        else if self.sidebar_visible && self.sidebar.slide_x < total_w && mx >= self.sidebar.slide_x {
                            let rel_x = mx - self.sidebar.slide_x;
                            let sidebar_h = total_h - MENU_BAR_H - DOCK_HEIGHT;
                            let rel_y = my - MENU_BAR_H;
                            if let Some(msg) = self.sidebar.on_settings_click(rel_x, rel_y) {
                                self.send_to_agent(msg);
                            } else {
                                self.sidebar.on_sidebar_click(rel_x, rel_y, sidebar_h);
                            }
                        }
                        // 4. Dock
                        else if let Some(idx) = self.dock.hit_test(mx, my, total_w, total_h) {
                            if idx < self.dock.apps.len() {
                                match &self.dock.apps[idx].action {
                                    DockAction::OpenWindow(WindowContentType::Terminal) => self.open_or_focus_window(WindowContentType::Terminal),
                                    DockAction::OpenWindow(WindowContentType::Browser) => self.open_or_focus_window(WindowContentType::Browser),
                                    DockAction::ToggleAgentMode => {
                                        self.agent_mode = !self.agent_mode;
                                        if self.agent_mode { self.activity_text = "Agent mode active".to_string(); }
                                        else { self.activity_text.clear(); }
                                    }
                                    DockAction::ToggleSidebar => {
                                        self.sidebar_visible = !self.sidebar_visible;
                                    }
                                    DockAction::TogglePrivateMode => {
                                        self.private_mode = !self.private_mode;
                                        self.send_to_agent(CompositorMessage::PrivateModeChanged { active: self.private_mode });
                                    }
                                }
                            }
                        }
                        // 5. Menu bar (y < MENU_BAR_H) — no-op for now
                        else if my < MENU_BAR_H {
                            // Future: menu bar actions
                        }
                        // 6. Windows (top to back — reverse iterate)
                        else {
                            let mut clicked_window: Option<(WindowId, bool)> = None; // (id, is_close)
                            for win in self.windows.iter().rev() {
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
                                // hit content area
                                clicked_window = Some((win.id, false));
                                break;
                            }

                            if let Some((id, is_close)) = clicked_window {
                                if is_close {
                                    self.close_window(id);
                                } else {
                                    self.bring_to_front(id);
                                    // Start drag if titlebar
                                    if let Some(win) = self.windows.iter().find(|w| w.id == id) {
                                        if win.hit_titlebar(mx, my) {
                                            if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                                                w.drag_offset_x = mx - w.x;
                                                w.drag_offset_y = my - w.y;
                                            }
                                            self.dragging_window = Some(id);
                                        }
                                    }
                                }
                            }
                            // else: clicked desktop — deselect all
                        }
                    }
                    ElementState::Released => {
                        self.dragging_window = None;
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 40.0,
                    MouseScrollDelta::PixelDelta(pos) => -(pos.y as f32) * 1.5,
                };

                // Route scroll to whichever UI element mouse is over
                if self.sidebar_visible && self.mouse_x >= self.sidebar.slide_x {
                    self.sidebar.scroll(scroll_amount);
                } else {
                    // Check if mouse is over a window
                    let content_type = self.windows.iter().rev()
                        .find(|w| w.hit_frame(self.mouse_x, self.mouse_y))
                        .and_then(|w| w.content.content_type());
                    match content_type {
                        Some(WindowContentType::Terminal) => self.terminal.scroll(scroll_amount),
                        Some(WindowContentType::Browser) => self.browser_panel.scroll(scroll_amount),
                        _ => {}
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            _ => {}
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("╔══════════════════════════════════════╗");
    info!("║    SomaOS Compositor v0.9.0          ║");
    info!("╚══════════════════════════════════════╝");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    #[cfg(feature = "drm-backend")]
    {
        info!("Backend: DRM/KMS (bare metal)");
        drm_main(runtime.handle().clone());
    }

    #[cfg(feature = "winit-backend")]
    {
        info!("Backend: winit (dev)");
        let event_loop = EventLoop::new().expect("Failed to create event loop");
        let mut app = SomaApp::new(runtime.handle().clone());
        event_loop.run_app(&mut app).expect("Event loop failed");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  DRM/KMS main loop
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "drm-backend")]
fn drm_main(runtime: tokio::runtime::Handle) {
    use backend::drm::DrmDisplay;
    use input::EvdevInput;
    use backend::event::{InputEvent, KeyCode, MouseBtn};
    use std::time::{Duration, Instant};

    let mut display = DrmDisplay::open().expect("Failed to open DRM display");
    let mut evdev = EvdevInput::open(display.width, display.height);

    let mut renderer = Renderer::new();
    let mut sidebar = Sidebar::new();
    let mut terminal = Terminal::new();
    let mut browser_panel = BrowserPanel::new();
    let mut login = LoginScreen::new();

    // Desktop state
    let mut windows: Vec<FloatingWindow> = Vec::new();
    let mut next_window_id: WindowId = 1;
    let mut dock = Dock::new();
    let mut sidebar_visible = false;
    let mut agent_mode = false;
    let mut private_mode = false;
    let mut activity_text = String::new();
    let mut menubar_clock = String::new();

    let mut mouse_x = display.width as f32 / 2.0;
    let mut mouse_y = display.height as f32 / 2.0;
    let mut dragging_window: Option<WindowId> = None;
    let mut toasts: Vec<Toast> = Vec::new();

    // IPC
    let (agent_tx, agent_rx) = match runtime.block_on(ipc_client::connect_to_agent()) {
        Ok((tx, rx)) => {
            info!("Connected to soma-agent");
            (Some(tx), Some(Arc::new(Mutex::new(rx))))
        }
        Err(e) => {
            info!("Agent not available: {}", e);
            (None, None)
        }
    };

    // Helper closures for DRM (no self, functional style)
    let send_msg = |tx: &Option<ipc_client::AgentSender>, msg: CompositorMessage| {
        if let Some(tx) = tx { let _ = tx.send(msg); }
    };

    let target_frame = Duration::from_millis(16);
    let mut last = Instant::now();

    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;

        // ── Input ──────────────────────────────────────────────────────────
        let events = evdev.poll();
        mouse_x = evdev.mouse_x;
        mouse_y = evdev.mouse_y;

        let wf = display.width as f32;
        let hf = display.height as f32;

        for ev in events {
            // Login intercepts everything until granted
            if login.result != LoginResult::Granted {
                match &ev {
                    InputEvent::KeyPress { code, .. } => match code {
                        KeyCode::Enter => login.on_submit(),
                        KeyCode::Backspace => login.on_backspace(),
                        KeyCode::Escape => {}
                        KeyCode::Char(c) => login.on_char(*c),
                        KeyCode::Space => login.on_char(' '),
                        _ => {}
                    },
                    _ => {}
                }
                continue;
            }

            // Normal compositor input
            match ev {
                InputEvent::KeyPress { code, .. } => {
                    // Check focused window content type
                    let focused_is_terminal = windows.iter().rev()
                        .find(|w| w.is_focused)
                        .and_then(|w| w.content.content_type()) == Some(WindowContentType::Terminal);

                    match code {
                        // F1: Open/focus Terminal
                        KeyCode::F1 => {
                            if let Some(win) = windows.iter().find(|w| w.content.content_type() == Some(WindowContentType::Terminal)) {
                                let id = win.id;
                                for w in &mut windows { w.is_focused = false; }
                                if let Some(pos) = windows.iter().position(|w| w.id == id) {
                                    let mut w = windows.remove(pos);
                                    w.is_focused = true;
                                    windows.push(w);
                                }
                            } else {
                                let id = next_window_id; next_window_id += 1;
                                let offset = (windows.len() as f32 * 30.0) % 200.0;
                                let win = FloatingWindow::new(id, WindowContent::Terminal, 80.0 + offset, MENU_BAR_H + 20.0 + offset);
                                windows.push(win);
                                for w in &mut windows { w.is_focused = false; }
                                windows.last_mut().unwrap().is_focused = true;
                            }
                        }
                        // F2: Close focused window
                        KeyCode::F2 => {
                            if let Some(win) = windows.iter().rev().find(|w| w.is_focused) {
                                let id = win.id;
                                windows.retain(|w| w.id != id);
                                if let Some(top) = windows.last_mut() { top.is_focused = true; }
                            }
                        }
                        // F3: Toggle sidebar
                        KeyCode::F3 => { sidebar_visible = !sidebar_visible; }
                        // F4: Toggle agent mode
                        KeyCode::F4 => {
                            agent_mode = !agent_mode;
                            if agent_mode { activity_text = "Agent mode active".to_string(); }
                            else { activity_text.clear(); }
                        }
                        // F5: Toggle private mode
                        KeyCode::F5 => {
                            private_mode = !private_mode;
                            send_msg(&agent_tx, CompositorMessage::PrivateModeChanged { active: private_mode });
                        }
                        KeyCode::Tab => {
                            if focused_is_terminal { terminal.on_tab(); }
                        }
                        KeyCode::Enter => {
                            if sidebar_visible {
                                if let Some(msg) = sidebar.on_submit() {
                                    send_msg(&agent_tx, msg);
                                }
                            } else if focused_is_terminal {
                                terminal.on_submit();
                            }
                        }
                        KeyCode::Backspace => {
                            if sidebar_visible { sidebar.on_backspace(); }
                            else if focused_is_terminal { terminal.on_backspace(); }
                        }
                        KeyCode::Escape => {
                            if sidebar.expanded_msg_idx.is_some() {
                                sidebar.expanded_msg_idx = None;
                            } else if let Some(msg) = sidebar.on_reject() {
                                send_msg(&agent_tx, msg);
                            } else if sidebar_visible {
                                sidebar_visible = false;
                            }
                        }
                        KeyCode::ArrowUp => { if focused_is_terminal { terminal.on_key_up(); } }
                        KeyCode::ArrowDown => { if focused_is_terminal { terminal.on_key_down(); } }
                        KeyCode::Space => {
                            if sidebar_visible { sidebar.on_char(' '); }
                            else if focused_is_terminal { terminal.on_char(' '); }
                        }
                        KeyCode::Char(c) => {
                            if sidebar_visible { sidebar.on_char(c); }
                            else if focused_is_terminal { terminal.on_char(c); }
                        }
                        KeyCode::Ctrl(c) => {
                            if focused_is_terminal {
                                match c {
                                    'c' => terminal.on_ctrl_c(),
                                    'd' => terminal.on_ctrl_d(),
                                    'l' => terminal.on_ctrl_l(),
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
                InputEvent::MouseMove { x, y } => {
                    mouse_x = x; mouse_y = y;
                    // Handle window dragging
                    if let Some(drag_id) = dragging_window {
                        if let Some(win) = windows.iter_mut().find(|w| w.id == drag_id) {
                            win.x = mouse_x - win.drag_offset_x;
                            win.y = mouse_y - win.drag_offset_y;
                        }
                    }
                }
                InputEvent::MouseButton { button: MouseBtn::Left, pressed } => {
                    if pressed {
                        // Priority routing (simplified for DRM)
                        if sidebar.status == soma_common::AgentStatus::AwaitingApproval {
                            if let Some(msg) = sidebar.on_approve() {
                                send_msg(&agent_tx, msg);
                            }
                        } else if sidebar.expanded_msg_idx.is_some() {
                            sidebar.expanded_msg_idx = None;
                        } else if sidebar_visible && sidebar.slide_x < wf && mouse_x >= sidebar.slide_x {
                            let rel_x = mouse_x - sidebar.slide_x;
                            let rel_y = mouse_y - MENU_BAR_H;
                            let sidebar_h = hf - MENU_BAR_H - DOCK_HEIGHT;
                            if let Some(msg) = sidebar.on_settings_click(rel_x, rel_y) {
                                send_msg(&agent_tx, msg);
                            } else {
                                sidebar.on_sidebar_click(rel_x, rel_y, sidebar_h);
                            }
                        } else if let Some(idx) = dock.hit_test(mouse_x, mouse_y, wf, hf) {
                            if idx < dock.apps.len() {
                                match &dock.apps[idx].action {
                                    DockAction::OpenWindow(WindowContentType::Terminal) => {
                                        // Open/focus terminal
                                        if !windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Terminal)) {
                                            let id = next_window_id; next_window_id += 1;
                                            windows.push(FloatingWindow::new(id, WindowContent::Terminal, 80.0, MENU_BAR_H + 20.0));
                                            for w in &mut windows { w.is_focused = false; }
                                            windows.last_mut().unwrap().is_focused = true;
                                        }
                                    }
                                    DockAction::OpenWindow(WindowContentType::Browser) => {
                                        if !windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Browser)) {
                                            let id = next_window_id; next_window_id += 1;
                                            windows.push(FloatingWindow::new(id, WindowContent::Browser, 120.0, MENU_BAR_H + 40.0));
                                            for w in &mut windows { w.is_focused = false; }
                                            windows.last_mut().unwrap().is_focused = true;
                                        }
                                    }
                                    DockAction::ToggleAgentMode => {
                                        agent_mode = !agent_mode;
                                        if agent_mode { activity_text = "Agent mode active".to_string(); }
                                        else { activity_text.clear(); }
                                    }
                                    DockAction::ToggleSidebar => { sidebar_visible = !sidebar_visible; }
                                    DockAction::TogglePrivateMode => {
                                        private_mode = !private_mode;
                                        send_msg(&agent_tx, CompositorMessage::PrivateModeChanged { active: private_mode });
                                    }
                                }
                            }
                        } else if mouse_y < MENU_BAR_H {
                            // Menu bar — no-op
                        } else {
                            // Windows
                            let mut clicked: Option<(WindowId, bool)> = None;
                            for win in windows.iter().rev() {
                                if win.is_minimized || !win.hit_frame(mouse_x, mouse_y) { continue; }
                                if win.hit_close(mouse_x, mouse_y) {
                                    clicked = Some((win.id, true)); break;
                                }
                                clicked = Some((win.id, false)); break;
                            }
                            if let Some((id, is_close)) = clicked {
                                if is_close {
                                    windows.retain(|w| w.id != id);
                                    if let Some(top) = windows.last_mut() { top.is_focused = true; }
                                } else {
                                    for w in &mut windows { w.is_focused = false; }
                                    if let Some(pos) = windows.iter().position(|w| w.id == id) {
                                        let mut w = windows.remove(pos);
                                        w.is_focused = true;
                                        if w.hit_titlebar(mouse_x, mouse_y) {
                                            w.drag_offset_x = mouse_x - w.x;
                                            w.drag_offset_y = mouse_y - w.y;
                                            dragging_window = Some(id);
                                        }
                                        windows.push(w);
                                    }
                                }
                            }
                        }
                    } else {
                        dragging_window = None;
                    }
                }
                InputEvent::Scroll { delta_y } => {
                    if sidebar_visible && mouse_x >= sidebar.slide_x {
                        sidebar.scroll(delta_y);
                    } else {
                        let ct = windows.iter().rev()
                            .find(|w| w.hit_frame(mouse_x, mouse_y))
                            .and_then(|w| w.content.content_type());
                        match ct {
                            Some(WindowContentType::Terminal) => terminal.scroll(delta_y),
                            Some(WindowContentType::Browser) => browser_panel.scroll(delta_y),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // ── Agent messages ─────────────────────────────────────────────────
        if let Some(rx) = &agent_rx {
            if let Ok(mut rx) = rx.try_lock() {
                while let Ok(msg) = rx.try_recv() {
                    match &msg {
                        soma_common::AgentMessage::DirectOutput { result, .. } => {
                            terminal.add_output(&result.stdout, &result.stderr);
                        }
                        soma_common::AgentMessage::ExecutionComplete { results, .. } => {
                            let ok = results.iter().filter(|r| r.success).count();
                            let total = results.len();
                            let color = if ok == total { [74, 222, 128, 255] } else { [248, 113, 113, 255] };
                            toasts.push(Toast { message: format!("Task done ({}/{})", ok, total), color, remaining: 3.5 });
                        }
                        soma_common::AgentMessage::Error { message, .. } => {
                            toasts.push(Toast { message: format!("! {}", &message[..message.len().min(40)]), color: [248, 113, 113, 255], remaining: 3.5 });
                        }
                        soma_common::AgentMessage::BrowserUpdate { url, title, screenshot_base64 } => {
                            browser_panel.update(url.clone(), title.clone(), screenshot_base64.as_deref());
                        }
                        soma_common::AgentMessage::ConfigUpdated { provider, model } => {
                            toasts.push(Toast { message: format!("Model: {} / {}", provider, model), color: [99, 102, 241, 255], remaining: 3.5 });
                        }
                        soma_common::AgentMessage::AgentModeStarted { task } => {
                            agent_mode = true; activity_text = task.clone();
                        }
                        soma_common::AgentMessage::AgentModeEnded => {
                            agent_mode = false; activity_text.clear();
                        }
                        soma_common::AgentMessage::ActivityUpdate { text } => {
                            activity_text = text.clone();
                        }
                        _ => {}
                    }
                    sidebar.handle_agent_message(msg);
                }
            }
        }

        // ── Render ─────────────────────────────────────────────────────────
        let w = display.width;
        let h = display.height;
        let mut pixmap = match tiny_skia::Pixmap::new(w, h) {
            Some(p) => p,
            None => continue,
        };

        sidebar.update(dt);
        terminal.update(dt);
        toasts.retain_mut(|t| { t.remaining -= dt; t.remaining > 0.0 });

        // Update menubar clock
        {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
            menubar_clock = format!("{:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60);
        }

        if login.result != LoginResult::Granted {
            login.update(dt);
            login.render(&mut renderer, &mut pixmap);
        } else {
            terminal.poll();

            // Sync dock state
            let has_terminal = windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Terminal));
            let has_browser = windows.iter().any(|w| w.content.content_type() == Some(WindowContentType::Browser));
            dock.sync_open_state(has_terminal, has_browser, agent_mode, sidebar_visible, private_mode);
            dock.hovered_idx = dock.hit_test(mouse_x, mouse_y, wf, hf);

            // Init sidebar slide
            if sidebar.slide_x == f32::MAX {
                sidebar.slide_x = wf;
                sidebar.slide_target_x = wf;
            }
            let sidebar_w = sidebar::SIDEBAR_WIDTH;
            sidebar.slide_target_x = if sidebar_visible { wf - sidebar_w } else { wf };

            // 9-layer stack
            desktop::render_desktop(&mut renderer, &mut pixmap, wf, hf);

            for i in 0..windows.len() {
                let win = &windows[i];
                if win.is_minimized { continue; }
                let hover_close = win.hit_close(mouse_x, mouse_y);
                render_window_chrome(&mut renderer, &mut pixmap, win, hover_close);
                let (cx, cy, cw, ch) = win.content_rect();
                match &win.content {
                    WindowContent::Terminal => terminal.render(&mut renderer, &mut pixmap, cx, cy, cw, ch),
                    WindowContent::Browser => browser_panel.render(&mut renderer, &mut pixmap, cx, cy, cw, ch),
                    WindowContent::DynamicApp(_) => {
                        let win_ref = &windows[i];
                        render_dynamic_app(&mut renderer, &mut pixmap, win_ref, mouse_x, mouse_y);
                    }
                }
            }

            if agent_mode {
                let t = renderer.theme.clone();
                renderer.fill_rect(&mut pixmap, 0.0, 0.0, wf, 2.0, t.agent_active);
                renderer.fill_rect(&mut pixmap, 0.0, hf - 2.0, wf, 2.0, t.agent_active);
                renderer.fill_rect(&mut pixmap, 0.0, 0.0, 2.0, hf, t.agent_active);
                renderer.fill_rect(&mut pixmap, wf - 2.0, 0.0, 2.0, hf, t.agent_active);
            }

            let status = sidebar.status;
            desktop::render_menu_bar(&mut renderer, &mut pixmap, wf, &status, &activity_text, private_mode, &menubar_clock);
            dock::render_dock(&mut renderer, &mut pixmap, &dock, wf, hf);

            if sidebar.slide_x < wf {
                sidebar.render(&mut renderer, &mut pixmap, sidebar.slide_x, MENU_BAR_H, hf - MENU_BAR_H - DOCK_HEIGHT);
            }

            if sidebar.status == soma_common::AgentStatus::AwaitingApproval {
                sidebar.render_approval_overlay(&mut renderer, &mut pixmap, wf, hf);
            }
            if sidebar.expanded_msg_idx.is_some() {
                sidebar.render_expanded_msg(&mut renderer, &mut pixmap, wf, hf);
            }

            let mut toast_y = MENU_BAR_H + 8.0;
            for toast in &toasts {
                let alpha = if toast.remaining < 0.5 { (toast.remaining / 0.5 * 255.0) as u8 } else { 255 };
                let tw = 260.0_f32.min(wf - 20.0);
                let tx = wf - tw - 10.0;
                renderer.fill_rounded_rect(&mut pixmap, tx, toast_y, tw, 28.0, 8.0,
                    [toast.color[0], toast.color[1], toast.color[2], (alpha / 4).max(20)]);
                renderer.draw_text(&mut pixmap, &toast.message, tx + 10.0, toast_y + 7.0, tw - 20.0, 10.0,
                    [toast.color[0], toast.color[1], toast.color[2], alpha]);
                toast_y += 34.0;
            }
        }

        // Present to DRM
        display.present(pixmap.data());

        // Frame pacing
        let elapsed = Instant::now().duration_since(last);
        if elapsed < target_frame {
            std::thread::sleep(target_frame - elapsed);
        }
    }
}


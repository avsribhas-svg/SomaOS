mod backend;
mod browser_panel;
mod compositor;
mod config_loader;
mod desktop;
mod dock;
mod docs;
mod event_handler;
mod input;
mod installer_wizard;
mod ipc_client;
mod login;
mod media;
mod renderer;
mod settings_app;
mod sheets;
mod sidebar;
mod terminal;
mod window_manager;

use log::info;
use renderer::Renderer;
use compositor::Toast;
use std::sync::{Arc, Mutex};

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

use window_manager::{WindowContent, WindowContentType, WindowId};

// ─────────────────────────────────────────────────────────────────────────────
//  SomaApp — winit backend
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "winit-backend")]
struct SomaApp {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    renderer: Renderer,
    sidebar: sidebar::Sidebar,
    terminal: terminal::Terminal,
    browser_panel: browser_panel::BrowserPanel,
    // IPC
    agent_tx: Option<ipc_client::AgentSender>,
    agent_rx: Option<Arc<Mutex<ipc_client::AgentReceiver>>>,
    runtime: tokio::runtime::Handle,
    /// Counts up (seconds) when disconnected; attempt reconnect at 5s
    reconnect_timer: f32,
    // Desktop window manager
    windows: Vec<window_manager::FloatingWindow>,
    next_window_id: WindowId,
    dock: dock::Dock,
    sidebar_visible: bool,
    agent_mode: bool,
    private_mode: bool,
    activity_text: String,
    menubar_clock: String,
    // Mouse state
    mouse_x: f32,
    mouse_y: f32,
    dragging_window: Option<WindowId>,
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
            sidebar: sidebar::Sidebar::new(),
            terminal: terminal::Terminal::new(),
            browser_panel: browser_panel::BrowserPanel::new(),
            agent_tx: None,
            agent_rx: None,
            runtime,
            reconnect_timer: 0.0,
            windows: Vec::new(),
            next_window_id: 1,
            dock: dock::Dock::new(),
            sidebar_visible: false,
            agent_mode: false,
            private_mode: false,
            activity_text: String::new(),
            menubar_clock: Self::clock_string(),
            mouse_x: 0.0,
            mouse_y: 0.0,
            dragging_window: None,
            toasts: Vec::new(),
            modifiers: ModifiersState::empty(),
        }
    }

    fn try_connect_agent(&mut self) {
        let rt = self.runtime.clone();
        match rt.block_on(ipc_client::connect_to_agent()) {
            Ok((tx, rx)) => {
                info!("Connected to soma-agent daemon");
                // Prime the Registry tab with the current capability list
                let _ = tx.send(CompositorMessage::ListCapabilities);
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

    fn clock_string() -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{:02}:{:02}", (now / 3600) % 24, (now / 60) % 60)
    }



    // ── Render frame ────────────────────────────────────────────────────────

    fn redraw(&mut self) {
        let window = match &self.window {
            Some(w) => w,
            None => return,
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 { return; }

        if let Some(surface) = &mut self.surface {
            let _ = surface.resize(
                NonZeroU32::new(size.width).unwrap(),
                NonZeroU32::new(size.height).unwrap(),
            );
        } else {
            return;
        }

        let mut pixmap = match tiny_skia::Pixmap::new(size.width, size.height) {
            Some(p) => p,
            None => return,
        };

        let w = size.width as f32;
        let h = size.height as f32;
        let dt = 1.0 / 60.0;

        // Detect agent disconnection and attempt reconnection every 5 seconds
        let agent_disconnected = self.agent_tx.as_ref().map_or(true, |tx| tx.is_closed());
        if agent_disconnected {
            if self.agent_tx.is_some() {
                info!("Agent connection lost — will retry in 5s");
                self.agent_tx = None;
                self.agent_rx = None;
            }
            self.reconnect_timer += dt;
            if self.reconnect_timer >= 5.0 {
                self.reconnect_timer = 0.0;
                self.try_connect_agent();
            }
        } else {
            self.reconnect_timer = 0.0;
        }

        // Poll PTY
        let pty_data = self.terminal.poll();

        // Poll agent messages
        let tx = &self.agent_tx;
        let send = |msg: CompositorMessage| { if let Some(tx) = tx { let _ = tx.send(msg); } };
        let got_agent_msg = event_handler::poll_agent_messages(
            &self.agent_rx, &mut self.sidebar, &mut self.terminal,
            &mut self.browser_panel, &mut self.toasts, &mut self.windows,
            &mut self.next_window_id, &mut self.agent_mode,
            &mut self.activity_text, &send, self.private_mode,
        );
        let _ = send;

        // Per-frame update
        compositor::update(
            dt, &mut self.sidebar, &mut self.terminal, &mut self.toasts,
            &mut self.windows, &mut self.dock, self.agent_mode,
            self.sidebar_visible, self.private_mode, self.mouse_x, self.mouse_y, w, h,
        );
        self.menubar_clock = Self::clock_string();

        // Render
        compositor::render(
            &mut self.renderer, &mut pixmap,
            &mut self.sidebar, &mut self.terminal, &mut self.browser_panel,
            &self.windows, &self.toasts, &self.dock,
            self.agent_mode, self.private_mode,
            &self.activity_text, &self.menubar_clock,
            self.mouse_x, self.mouse_y, w, h,
        );

        // Present
        if let Some(surface) = &mut self.surface {
            compositor::present_winit(surface, &pixmap);
        }

        // Continuous redraw check
        if compositor::needs_continuous_redraw(got_agent_msg, pty_data, &self.toasts, &self.sidebar, self.agent_mode) {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Winit ApplicationHandler
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "winit-backend")]
impl ApplicationHandler for SomaApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() { return; }

        let attrs = WindowAttributes::default()
            .with_title("SomaOS")
            .with_inner_size(LogicalSize::new(1200u32, 760u32))
            .with_min_inner_size(LogicalSize::new(600u32, 400u32));

        let window = Arc::new(event_loop.create_window(attrs).expect("Failed to create window"));
        let context = softbuffer::Context::new(window.clone()).expect("Failed to create softbuffer context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("Failed to create surface");

        self.window = Some(window);
        self.surface = Some(surface);
        self.try_connect_agent();
        info!("SomaOS compositor window created");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WinitWindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(PhysicalSize { width, height }) => {
                if let Some(surface) = &mut self.surface {
                    if width > 0 && height > 0 {
                        let _ = surface.resize(
                            NonZeroU32::new(width).unwrap(),
                            NonZeroU32::new(height).unwrap(),
                        );
                    }
                }
                if let Some(w) = &self.window { w.request_redraw(); }
            }

            WindowEvent::RedrawRequested => self.redraw(),

            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }

            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, .. }, ..
            } => {
                let cmd = self.modifiers.super_key();
                let shift = self.modifiers.shift_key();
                let ctrl = self.modifiers.control_key();

                match &logical_key {
                    // Desktop shortcuts
                    Key::Named(NamedKey::Space) if cmd => {
                        self.sidebar_visible = !self.sidebar_visible;
                    }
                    Key::Character(c) if cmd && !shift => {
                        let tx = &self.agent_tx;
                        let send = |msg: CompositorMessage| { if let Some(tx) = tx { let _ = tx.send(msg); } };
                        match c.as_str() {
                            "t" => event_handler::open_or_focus_window(&mut self.windows, &mut self.next_window_id, WindowContentType::Terminal, &send, self.private_mode),
                            "w" => event_handler::close_focused_window(&mut self.windows, &send, self.private_mode),
                            _ => {}
                        }
                    }
                    Key::Character(c) if cmd && shift => {
                        match c.as_str() {
                            "A" | "a" => {
                                self.agent_mode = !self.agent_mode;
                                if self.agent_mode { self.activity_text = "Agent mode active".to_string(); }
                                else { self.activity_text.clear(); }
                            }
                            "P" | "p" => {
                                self.private_mode = !self.private_mode;
                                self.send_to_agent(CompositorMessage::PrivateModeChanged { active: self.private_mode });
                            }
                            _ => {}
                        }
                    }

                    Key::Named(NamedKey::Tab) => {
                        if event_handler::focused_content_type(&self.windows) == Some(WindowContentType::Terminal) {
                            self.terminal.on_tab();
                        }
                    }
                    Key::Named(NamedKey::Escape) => {
                        if self.sidebar.expanded_msg_idx.is_some() {
                            self.sidebar.expanded_msg_idx = None;
                        } else if let Some(msg) = self.sidebar.on_reject() {
                            self.send_to_agent(msg);
                        } else if self.sidebar_visible {
                            self.sidebar_visible = false;
                        }
                    }
                    Key::Named(NamedKey::Enter) => {
                        if self.sidebar_visible {
                            if let Some(msg) = self.sidebar.on_submit() {
                                self.send_to_agent(msg);
                            }
                        } else if let Some(win) = self.windows.iter_mut().find(|w| w.is_focused) {
                            let win_id = win.id;
                            match &mut win.content {
                                WindowContent::Terminal => { self.terminal.on_submit(); }
                                WindowContent::NativeApp(app) => {
                                    if let Some(state) = app.on_key("Enter") {
                                        self.send_to_agent(CompositorMessage::AppStateChanged { window_id: win_id, state });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Key::Named(NamedKey::Backspace) => {
                        if self.sidebar_visible {
                            self.sidebar.on_backspace();
                        } else if let Some(win) = self.windows.iter_mut().find(|w| w.is_focused) {
                            match &mut win.content {
                                WindowContent::Terminal => self.terminal.on_backspace(),
                                WindowContent::Settings(s) => s.on_backspace(),
                                WindowContent::NativeApp(app) => app.on_backspace(),
                                _ => {}
                            }
                        }
                    }
                    Key::Named(NamedKey::Tab) => {
                        if let Some(win) = self.windows.iter_mut().find(|w| w.is_focused) {
                            let win_id = win.id;
                            if let WindowContent::NativeApp(app) = &mut win.content {
                                if let Some(state) = app.on_key("Tab") {
                                    self.send_to_agent(CompositorMessage::AppStateChanged { window_id: win_id, state });
                                }
                            }
                        }
                    }
                    Key::Named(NamedKey::Escape) => {
                        if let Some(win) = self.windows.iter_mut().find(|w| w.is_focused) {
                            if let WindowContent::NativeApp(app) = &mut win.content {
                                app.on_key("Escape");
                            }
                        }
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        if let Some(win) = self.windows.iter_mut().find(|w| w.is_focused) {
                            let win_id = win.id;
                            match &mut win.content {
                                WindowContent::Terminal => self.terminal.on_key_up(),
                                WindowContent::NativeApp(app) => {
                                    if let Some(state) = app.on_key("ArrowUp") {
                                        self.send_to_agent(CompositorMessage::AppStateChanged { window_id: win_id, state });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        if let Some(win) = self.windows.iter_mut().find(|w| w.is_focused) {
                            let win_id = win.id;
                            match &mut win.content {
                                WindowContent::Terminal => self.terminal.on_key_down(),
                                WindowContent::NativeApp(app) => {
                                    if let Some(state) = app.on_key("ArrowDown") {
                                        self.send_to_agent(CompositorMessage::AppStateChanged { window_id: win_id, state });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        if let Some(win) = self.windows.iter_mut().find(|w| w.is_focused) {
                            let win_id = win.id;
                            if let WindowContent::NativeApp(app) = &mut win.content {
                                if let Some(state) = app.on_key("ArrowLeft") {
                                    self.send_to_agent(CompositorMessage::AppStateChanged { window_id: win_id, state });
                                }
                            }
                        }
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        if let Some(win) = self.windows.iter_mut().find(|w| w.is_focused) {
                            let win_id = win.id;
                            if let WindowContent::NativeApp(app) = &mut win.content {
                                if let Some(state) = app.on_key("ArrowRight") {
                                    self.send_to_agent(CompositorMessage::AppStateChanged { window_id: win_id, state });
                                }
                            }
                        }
                    }
                    Key::Named(NamedKey::Space) => {
                        if self.sidebar_visible {
                            self.sidebar.on_char(' ');
                        } else if let Some(win) = self.windows.iter_mut().find(|w| w.is_focused) {
                            match &mut win.content {
                                WindowContent::Terminal => self.terminal.on_char(' '),
                                WindowContent::Settings(s) => s.on_char(' '),
                                WindowContent::NativeApp(app) => app.on_char(' '),
                                _ => {}
                            }
                        }
                    }
                    Key::Character(c) if !cmd => {
                        if ctrl && event_handler::focused_content_type(&self.windows) == Some(WindowContentType::Terminal) {
                            match c.as_str() {
                                "c" => self.terminal.on_ctrl_c(),
                                "d" => self.terminal.on_ctrl_d(),
                                "l" => self.terminal.on_ctrl_l(),
                                _ => {}
                            }
                        } else if self.sidebar_visible {
                            for ch in c.chars() { self.sidebar.on_char(ch); }
                        } else if let Some(win) = self.windows.iter_mut().find(|w| w.is_focused) {
                            match &mut win.content {
                                WindowContent::Terminal => { for ch in c.chars() { self.terminal.on_char(ch); } }
                                WindowContent::Settings(s) => { for ch in c.chars() { s.on_char(ch); } }
                                WindowContent::NativeApp(app) => { for ch in c.chars() { app.on_char(ch); } }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }

                if let Some(w) = &self.window { w.request_redraw(); }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_x = position.x as f32;
                self.mouse_y = position.y as f32;
                if let Some(drag_id) = self.dragging_window {
                    if let Some(win) = self.windows.iter_mut().find(|w| w.id == drag_id) {
                        win.x = self.mouse_x - win.drag_offset_x;
                        win.y = self.mouse_y - win.drag_offset_y;
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
                        let tx = &self.agent_tx;
                        let send = |msg: CompositorMessage| { if let Some(tx) = tx { let _ = tx.send(msg); } };
                        event_handler::handle_mouse_click(
                            self.mouse_x, self.mouse_y, total_w, total_h,
                            &mut self.sidebar, &mut self.sidebar_visible,
                            &self.dock, &mut self.windows, &mut self.next_window_id,
                            &mut self.agent_mode, &mut self.private_mode,
                            &mut self.activity_text, &mut self.dragging_window,
                            &send,
                        );
                    }
                    ElementState::Released => {
                        self.dragging_window = None;
                    }
                }
                if let Some(w) = &self.window { w.request_redraw(); }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 40.0,
                    MouseScrollDelta::PixelDelta(pos) => -(pos.y as f32) * 1.5,
                };
                event_handler::handle_scroll(
                    scroll_amount, self.mouse_x, self.mouse_y,
                    self.sidebar_visible, &mut self.sidebar,
                    &mut self.terminal, &mut self.browser_panel, &self.windows,
                );
                if let Some(w) = &self.window { w.request_redraw(); }
            }

            _ => {}
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Entry point
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let installer_mode = std::env::args().any(|a| a == "--installer")
        || std::env::var("SOMA_MODE").as_deref() == Ok("installer");

    info!("╔══════════════════════════════════════╗");
    if installer_mode {
        info!("║  SomaOS Installer v2.0 — First Boot ║");
    } else {
        info!("║    SomaOS Compositor v1.0.1          ║");
    }
    info!("╚══════════════════════════════════════╝");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    #[cfg(feature = "drm-backend")]
    {
        if installer_mode {
            info!("Backend: DRM/KMS (installer)");
            installer_main(runtime.handle().clone());
        } else {
            info!("Backend: DRM/KMS (bare metal)");
            drm_main(runtime.handle().clone());
        }
    }

    #[cfg(feature = "winit-backend")]
    {
        if installer_mode {
            info!("Installer mode requires drm-backend; starting normal compositor instead.");
        }
        info!("Backend: winit (dev)");
        let event_loop = EventLoop::new().expect("Failed to create event loop");
        let mut app = SomaApp::new(runtime.handle().clone());
        event_loop.run_app(&mut app).expect("Event loop failed");
    }
}

/// First-boot installer wizard — DRM-only.
/// Runs a full-screen setup flow: username/password → LLM API key → network.
/// On completion writes /etc/soma/passwd and ~/.soma/config.toml, then reboots.
#[cfg(feature = "drm-backend")]
fn installer_main(_runtime: tokio::runtime::Handle) {
    use backend::drm::DrmDisplay;
    use input::EvdevInput;

    let mut drm = DrmDisplay::new().expect("DRM init failed");
    let mut evdev = EvdevInput::new();
    let mut renderer = Renderer::new();

    info!("Installer: entering first-boot wizard");

    let mut wizard = installer_wizard::InstallerWizard::new();

    loop {
        let (w, h) = drm.size();
        let mut pixmap = drm.pixmap(w, h);

        wizard.render(&mut renderer, &mut pixmap, w, h);
        drm.present(&pixmap);

        let events = evdev.poll();
        if wizard.handle_input(&events) {
            if let Err(e) = wizard.commit() {
                log::error!("Installer commit failed: {}", e);
            } else {
                info!("Installation complete — rebooting...");
                let _ = std::process::Command::new("reboot").status();
            }
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(16));
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
    let mut sidebar = sidebar::Sidebar::new();
    let mut terminal = terminal::Terminal::new();
    let mut browser_panel = browser_panel::BrowserPanel::new();
    let mut login = LoginScreen::new();

    let mut windows: Vec<window_manager::FloatingWindow> = Vec::new();
    let mut next_window_id: WindowId = 1;
    let mut dock = dock::Dock::new();
    let mut sidebar_visible = false;
    let mut agent_mode = false;
    let mut private_mode = false;
    let mut activity_text = String::new();
    let mut menubar_clock = String::new();

    let mut mouse_x = display.width as f32 / 2.0;
    let mut mouse_y = display.height as f32 / 2.0;
    let mut dragging_window: Option<WindowId> = None;
    let mut toasts: Vec<Toast> = Vec::new();

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

    let send_msg = |msg: CompositorMessage| {
        if let Some(tx) = &agent_tx { let _ = tx.send(msg); }
    };

    // Prime the Registry tab with the current capability list
    send_msg(CompositorMessage::ListCapabilities);

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

            match ev {
                InputEvent::KeyPress { code, .. } => {
                    let focused_is_terminal = event_handler::focused_content_type(&windows) == Some(WindowContentType::Terminal);

                    match code {
                        KeyCode::F1 => {
                            event_handler::open_or_focus_window(&mut windows, &mut next_window_id, WindowContentType::Terminal, &send_msg, private_mode);
                        }
                        KeyCode::F2 => {
                            event_handler::close_focused_window(&mut windows, &send_msg, private_mode);
                        }
                        KeyCode::F3 => { sidebar_visible = !sidebar_visible; }
                        KeyCode::F4 => {
                            agent_mode = !agent_mode;
                            if agent_mode { activity_text = "Agent mode active".to_string(); }
                            else { activity_text.clear(); }
                        }
                        KeyCode::F5 => {
                            private_mode = !private_mode;
                            send_msg(CompositorMessage::PrivateModeChanged { active: private_mode });
                        }
                        KeyCode::Tab => {
                            if focused_is_terminal {
                                terminal.on_tab();
                            } else if let Some(win) = windows.iter_mut().find(|w| w.is_focused) {
                                if let WindowContent::NativeApp(app) = &mut win.content {
                                    if let Some(state) = app.on_key("Tab") {
                                        send_msg(CompositorMessage::AppStateChanged { window_id: win.id, state });
                                    }
                                }
                            }
                        }
                        KeyCode::Enter => {
                            if sidebar_visible {
                                if let Some(msg) = sidebar.on_submit() {
                                    send_msg(msg);
                                }
                            } else if let Some(win) = windows.iter_mut().find(|w| w.is_focused) {
                                match &mut win.content {
                                    WindowContent::Terminal => terminal.on_submit(),
                                    WindowContent::NativeApp(app) => {
                                        if let Some(state) = app.on_key("Enter") {
                                            send_msg(CompositorMessage::AppStateChanged { window_id: win.id, state });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        KeyCode::Backspace => {
                            if sidebar_visible { sidebar.on_backspace(); }
                            else if let Some(win) = windows.iter_mut().find(|w| w.is_focused) {
                                match &mut win.content {
                                    WindowContent::Terminal => terminal.on_backspace(),
                                    WindowContent::Settings(s) => s.on_backspace(),
                                    WindowContent::NativeApp(app) => app.on_backspace(),
                                    _ => {}
                                }
                            }
                        }
                        KeyCode::Escape => {
                            if sidebar.expanded_msg_idx.is_some() {
                                sidebar.expanded_msg_idx = None;
                            } else if let Some(msg) = sidebar.on_reject() {
                                send_msg(msg);
                            } else if sidebar_visible {
                                sidebar_visible = false;
                            } else if let Some(win) = windows.iter_mut().find(|w| w.is_focused) {
                                if let WindowContent::NativeApp(app) = &mut win.content {
                                    app.on_key("Escape");
                                }
                            }
                        }
                        KeyCode::ArrowUp => {
                            if focused_is_terminal { terminal.on_key_up(); }
                            else if let Some(win) = windows.iter_mut().find(|w| w.is_focused) {
                                if let WindowContent::NativeApp(app) = &mut win.content {
                                    if let Some(state) = app.on_key("ArrowUp") {
                                        send_msg(CompositorMessage::AppStateChanged { window_id: win.id, state });
                                    }
                                }
                            }
                        }
                        KeyCode::ArrowDown => {
                            if focused_is_terminal { terminal.on_key_down(); }
                            else if let Some(win) = windows.iter_mut().find(|w| w.is_focused) {
                                if let WindowContent::NativeApp(app) = &mut win.content {
                                    if let Some(state) = app.on_key("ArrowDown") {
                                        send_msg(CompositorMessage::AppStateChanged { window_id: win.id, state });
                                    }
                                }
                            }
                        }
                        KeyCode::Space => {
                            if sidebar_visible { sidebar.on_char(' '); }
                            else if let Some(win) = windows.iter_mut().find(|w| w.is_focused) {
                                match &mut win.content {
                                    WindowContent::Terminal => terminal.on_char(' '),
                                    WindowContent::Settings(s) => s.on_char(' '),
                                    WindowContent::NativeApp(app) => app.on_char(' '),
                                    _ => {}
                                }
                            }
                        }
                        KeyCode::Char(c) => {
                            if sidebar_visible { sidebar.on_char(c); }
                            else if let Some(win) = windows.iter_mut().find(|w| w.is_focused) {
                                match &mut win.content {
                                    WindowContent::Terminal => terminal.on_char(c),
                                    WindowContent::Settings(s) => s.on_char(c),
                                    WindowContent::NativeApp(app) => app.on_char(c),
                                    _ => {}
                                }
                            }
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
                    if let Some(drag_id) = dragging_window {
                        if let Some(win) = windows.iter_mut().find(|w| w.id == drag_id) {
                            win.x = mouse_x - win.drag_offset_x;
                            win.y = mouse_y - win.drag_offset_y;
                        }
                    }
                }
                InputEvent::MouseButton { button: MouseBtn::Left, pressed } => {
                    if pressed {
                        event_handler::handle_mouse_click(
                            mouse_x, mouse_y, wf, hf,
                            &mut sidebar, &mut sidebar_visible,
                            &dock, &mut windows, &mut next_window_id,
                            &mut agent_mode, &mut private_mode,
                            &mut activity_text, &mut dragging_window,
                            &send_msg,
                        );
                    } else {
                        dragging_window = None;
                    }
                }
                InputEvent::Scroll { delta_y } => {
                    event_handler::handle_scroll(
                        delta_y, mouse_x, mouse_y, sidebar_visible,
                        &mut sidebar, &mut terminal, &mut browser_panel, &windows,
                    );
                }
                _ => {}
            }
        }

        // ── Agent messages ─────────────────────────────────────────────────
        event_handler::poll_agent_messages(
            &agent_rx, &mut sidebar, &mut terminal, &mut browser_panel,
            &mut toasts, &mut windows, &mut next_window_id,
            &mut agent_mode, &mut activity_text, &send_msg, private_mode,
        );

        // ── Render ─────────────────────────────────────────────────────────
        let w = display.width;
        let h = display.height;
        let mut pixmap = match tiny_skia::Pixmap::new(w, h) {
            Some(p) => p,
            None => continue,
        };

        terminal.poll();

        compositor::update(
            dt, &mut sidebar, &mut terminal, &mut toasts,
            &mut windows, &mut dock, agent_mode,
            sidebar_visible, private_mode, mouse_x, mouse_y, wf, hf,
        );

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
            compositor::render(
                &mut renderer, &mut pixmap,
                &mut sidebar, &mut terminal, &mut browser_panel,
                &windows, &toasts, &dock,
                agent_mode, private_mode,
                &activity_text, &menubar_clock,
                mouse_x, mouse_y, wf, hf,
            );
        }

        display.present(pixmap.data());

        // Frame pacing
        let elapsed = Instant::now().duration_since(last);
        if elapsed < target_frame {
            std::thread::sleep(target_frame - elapsed);
        }
    }
}

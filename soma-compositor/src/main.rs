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
use log::info;
use renderer::Renderer;
use sidebar::Sidebar;
use std::sync::{Arc, Mutex};
use terminal::Terminal;

#[cfg(feature = "drm-backend")]
use login::{LoginResult, LoginScreen};
#[cfg(feature = "winit-backend")]
use soma_common::CompositorMessage;
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
use winit::window::{Window, WindowAttributes, WindowId};

/// Which panel has keyboard focus
#[derive(Clone, Copy, PartialEq)]
pub enum FocusPanel {
    Sidebar,
    Terminal,
}

/// Which view is shown in the left (non-sidebar) area
#[derive(Clone, Copy, PartialEq)]
pub enum LeftPanel {
    Terminal,
    Browser,
}

/// Notification toast
struct Toast {
    message: String,
    color: [u8; 4],
    remaining: f32,
}

const MIN_SIDEBAR_W: f32 = 280.0;
const MAX_SIDEBAR_W: f32 = 600.0;
const DEFAULT_SIDEBAR_W: f32 = 380.0;
const DIVIDER_HIT: f32 = 6.0; // hit target width for divider drag

#[cfg(feature = "winit-backend")]
struct SomaApp {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    renderer: Renderer,
    sidebar: Sidebar,
    terminal: Terminal,
    browser_panel: BrowserPanel,
    focus: FocusPanel,
    left_panel: LeftPanel,
    // IPC
    agent_tx: Option<ipc_client::AgentSender>,
    agent_rx: Option<Arc<Mutex<ipc_client::AgentReceiver>>>,
    runtime: tokio::runtime::Handle,
    // Layout
    sidebar_width: f32,
    // Mouse state
    mouse_x: f32,
    mouse_y: f32,
    dragging_divider: bool,
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
            focus: FocusPanel::Sidebar,
            left_panel: LeftPanel::Terminal,
            agent_tx: None,
            agent_rx: None,
            runtime,
            sidebar_width: DEFAULT_SIDEBAR_W,
            mouse_x: 0.0,
            mouse_y: 0.0,
            dragging_divider: false,
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

    fn poll_agent_messages(&mut self) -> bool {
        // Collect messages first to avoid borrow conflicts
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
                        self.add_toast(
                            format!("v Task done ({}/{})", ok, total),
                            [74, 222, 128, 255],
                        );
                    } else {
                        self.add_toast(
                            format!("! Task done ({}/{})", ok, total),
                            [248, 113, 113, 255],
                        );
                    }
                }
                soma_common::AgentMessage::Error { message, .. } => {
                    self.add_toast(
                        format!("! {}", &message[..message.len().min(40)]),
                        [248, 113, 113, 255],
                    );
                }
                soma_common::AgentMessage::BrowserUpdate {
                    url,
                    title,
                    screenshot_base64,
                } => {
                    self.browser_panel.update(
                        url.clone(),
                        title.clone(),
                        screenshot_base64.as_deref(),
                    );
                    // Auto-switch the left panel to Browser so the user sees the page.
                    self.left_panel = LeftPanel::Browser;
                }
                soma_common::AgentMessage::ConfigUpdated { provider, model } => {
                    self.add_toast(
                        format!("Model: {} / {}", provider, model),
                        [99, 102, 241, 255],
                    );
                }
                _ => {}
            }
            self.sidebar.handle_agent_message(msg);
        }
        got_message
    }

    /// Get the X position of the divider (left edge of sidebar)
    fn divider_x(&self, total_w: f32) -> f32 {
        (total_w - self.sidebar_width).max(0.0)
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

        pixmap.fill(tiny_skia::Color::from_rgba8(30, 30, 30, 255));

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

        // Layout
        let divider = self.divider_x(w);
        let left_w = divider;
        let sidebar_x = divider;

        // Render left panel (Terminal or Browser)
        if left_w > 10.0 {
            match self.left_panel {
                LeftPanel::Terminal => {
                    self.terminal
                        .render(&mut self.renderer, &mut pixmap, 0.0, 0.0, left_w, h);
                }
                LeftPanel::Browser => {
                    self.browser_panel
                        .render(&mut self.renderer, &mut pixmap, 0.0, 0.0, left_w, h);
                    // F2 hint in bottom-left
                    self.renderer.draw_text(
                        &mut pixmap,
                        "F2=Terminal",
                        4.0,
                        h - 14.0,
                        80.0,
                        8.0,
                        [255, 255, 255, 40],
                    );
                }
            }
        }

        // Render sidebar on the RIGHT
        self.sidebar
            .render(&mut self.renderer, &mut pixmap, sidebar_x, 0.0, h);

        // Divider handle
        let divider_color = if self.dragging_divider {
            [129, 140, 248, 120]
        } else if (self.mouse_x - divider).abs() < DIVIDER_HIT {
            [129, 140, 248, 60]
        } else {
            [255, 255, 255, 15]
        };
        self.renderer
            .fill_rect(&mut pixmap, divider - 1.0, 0.0, 2.0, h, divider_color);

        // Focus indicator
        let focus_color = [129, 140, 248, 40];
        match self.focus {
            FocusPanel::Sidebar => {
                self.renderer
                    .fill_rect(&mut pixmap, sidebar_x, 0.0, 2.0, h, focus_color);
            }
            FocusPanel::Terminal => {
                self.renderer
                    .fill_rect(&mut pixmap, 0.0, 0.0, 2.0, h, focus_color);
            }
        }

        // Left panel tab indicator (top-left corner)
        {
            let tab_label = match self.left_panel {
                LeftPanel::Terminal => "[T] Terminal  F2=Browser",
                LeftPanel::Browser => "[B] Browser   F2=Terminal",
            };
            self.renderer.draw_text(
                &mut pixmap,
                tab_label,
                6.0,
                4.0,
                200.0,
                8.0,
                [255, 255, 255, 30],
            );
        }

        // HITL overlay
        if self.sidebar.status == soma_common::AgentStatus::AwaitingApproval {
            self.sidebar
                .render_approval_overlay(&mut self.renderer, &mut pixmap, w, h);
        }

        // Detail modal for clicked error/result cards
        if self.sidebar.expanded_msg_idx.is_some() {
            self.sidebar
                .render_expanded_msg(&mut self.renderer, &mut pixmap, w, h);
        }

        // Render toasts (top-right, above sidebar)
        let mut toast_y = 8.0;
        for toast in &self.toasts {
            let alpha = if toast.remaining < 0.5 {
                (toast.remaining / 0.5 * 255.0) as u8
            } else {
                255
            };
            let tw = 260.0_f32.min(w - 20.0);
            let tx = w - tw - 10.0;
            self.renderer.fill_rounded_rect(
                &mut pixmap,
                tx,
                toast_y,
                tw,
                28.0,
                8.0,
                [toast.color[0], toast.color[1], toast.color[2], (alpha / 4).max(20)],
            );
            self.renderer.draw_text(
                &mut pixmap,
                &toast.message,
                tx + 10.0,
                toast_y + 7.0,
                tw - 20.0,
                10.0,
                [toast.color[0], toast.color[1], toast.color[2], alpha],
            );
            toast_y += 34.0;
        }

        // Present
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

        // Request continuous redraw when needed
        let needs_redraw = got_agent_msg
            || pty_data
            || !self.toasts.is_empty()
            || matches!(
                self.sidebar.status,
                soma_common::AgentStatus::Thinking | soma_common::AgentStatus::Executing
            );
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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
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
                // Clamp sidebar width on resize
                let w = width as f32;
                let max_sidebar_w = (w - 200.0).min(MAX_SIDEBAR_W).max(MIN_SIDEBAR_W);
                self.sidebar_width = self.sidebar_width.clamp(MIN_SIDEBAR_W, max_sidebar_w);
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
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                let ctrl = self.modifiers.control_key();

                match &logical_key {
                    // Tab: switch focus ONLY when terminal doesn't need it
                    Key::Named(NamedKey::Tab) => {
                        if self.focus == FocusPanel::Terminal {
                            // Send tab to PTY for shell completion
                            self.terminal.on_tab();
                        } else {
                            // In sidebar, tab switches panels
                            self.focus = FocusPanel::Terminal;
                        }
                    }

                    // F1 toggles keyboard focus between sidebar and terminal
                    Key::Named(NamedKey::F1) => {
                        self.focus = match self.focus {
                            FocusPanel::Sidebar => FocusPanel::Terminal,
                            FocusPanel::Terminal => FocusPanel::Sidebar,
                        };
                    }

                    // F2 toggles left panel between Terminal and Browser
                    Key::Named(NamedKey::F2) => {
                        self.left_panel = match self.left_panel {
                            LeftPanel::Terminal => LeftPanel::Browser,
                            LeftPanel::Browser => LeftPanel::Terminal,
                        };
                    }

                    Key::Named(NamedKey::Enter) => match self.focus {
                        FocusPanel::Sidebar => {
                            if let Some(msg) = self.sidebar.on_submit() {
                                self.send_to_agent(msg);
                            }
                        }
                        FocusPanel::Terminal => {
                            self.terminal.on_submit();
                        }
                    },

                    Key::Named(NamedKey::Backspace) => match self.focus {
                        FocusPanel::Sidebar => self.sidebar.on_backspace(),
                        FocusPanel::Terminal => self.terminal.on_backspace(),
                    },

                    Key::Named(NamedKey::Escape) => {
                        if let Some(msg) = self.sidebar.on_reject() {
                            self.send_to_agent(msg);
                        } else if self.focus == FocusPanel::Terminal {
                            // Switch to sidebar
                            self.focus = FocusPanel::Sidebar;
                        }
                    }

                    Key::Named(NamedKey::ArrowUp) => {
                        if self.focus == FocusPanel::Terminal {
                            self.terminal.on_key_up();
                        }
                    }

                    Key::Named(NamedKey::ArrowDown) => {
                        if self.focus == FocusPanel::Terminal {
                            self.terminal.on_key_down();
                        }
                    }

                    Key::Named(NamedKey::Space) => match self.focus {
                        FocusPanel::Sidebar => self.sidebar.on_char(' '),
                        FocusPanel::Terminal => self.terminal.on_char(' '),
                    },

                    Key::Character(c) => {
                        // Handle Ctrl combos in terminal
                        if ctrl && self.focus == FocusPanel::Terminal {
                            match c.as_str() {
                                "c" => self.terminal.on_ctrl_c(),
                                "d" => self.terminal.on_ctrl_d(),
                                "l" => self.terminal.on_ctrl_l(),
                                _ => {}
                            }
                        } else {
                            for ch in c.chars() {
                                match self.focus {
                                    FocusPanel::Sidebar => self.sidebar.on_char(ch),
                                    FocusPanel::Terminal => self.terminal.on_char(ch),
                                }
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

                // Handle divider drag
                if self.dragging_divider {
                    if let Some(win) = &self.window {
                        let total_w = win.inner_size().width as f32;
                        let max_sidebar_w = MAX_SIDEBAR_W.min(total_w - 200.0).max(MIN_SIDEBAR_W);
                        let new_sidebar_w = (total_w - self.mouse_x).clamp(MIN_SIDEBAR_W, max_sidebar_w);
                        self.sidebar_width = new_sidebar_w;
                        win.request_redraw();
                    }
                }

                // Update cursor for divider hover
                if let Some(win) = &self.window {
                    let total_w = win.inner_size().width as f32;
                    let div = self.divider_x(total_w);
                    if (self.mouse_x - div).abs() < DIVIDER_HIT {
                        win.set_cursor(winit::window::Cursor::Icon(winit::window::CursorIcon::ColResize));
                    } else {
                        win.set_cursor(winit::window::Cursor::Icon(winit::window::CursorIcon::Default));
                    }
                }
            }

            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(win) = &self.window {
                    let total_w = win.inner_size().width as f32;
                    let div = self.divider_x(total_w);

                    match state {
                        ElementState::Pressed => {
                            if (self.mouse_x - div).abs() < DIVIDER_HIT {
                                // Start divider drag
                                self.dragging_divider = true;
                            } else if self.mouse_x < div {
                                // Left panel clicked: focus terminal (keyboard) regardless of view
                                self.focus = FocusPanel::Terminal;
                            } else {
                                self.focus = FocusPanel::Sidebar;
                                let rel_x = self.mouse_x - div;
                                let h = win.inner_size().height as f32;
                                // Route click into sidebar — settings tab or chat tab
                                if let Some(msg) = self.sidebar.on_settings_click(rel_x, self.mouse_y) {
                                    self.send_to_agent(msg);
                                } else {
                                    self.sidebar.on_sidebar_click(rel_x, self.mouse_y, h);
                                }
                            }
                        }
                        ElementState::Released => {
                            self.dragging_divider = false;
                        }
                    }
                    win.request_redraw();
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 40.0,
                    MouseScrollDelta::PixelDelta(pos) => -(pos.y as f32) * 1.5,
                };

                // Scroll whichever panel the mouse is over
                if let Some(win) = &self.window {
                    let total_w = win.inner_size().width as f32;
                    let div = self.divider_x(total_w);
                    if self.mouse_x < div {
                        match self.left_panel {
                            LeftPanel::Terminal => self.terminal.scroll(scroll_amount),
                            LeftPanel::Browser => self.browser_panel.scroll(scroll_amount),
                        }
                    } else {
                        self.sidebar.scroll(scroll_amount);
                    }
                    win.request_redraw();
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

    let mut focus = FocusPanel::Sidebar;
    let mut left_panel = LeftPanel::Terminal;
    let mut sidebar_width: f32 = 380.0;
    let mut mouse_x = display.width as f32 / 2.0;
    let mut mouse_y = display.height as f32 / 2.0;
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

    let target_frame = Duration::from_millis(16); // ~60 fps
    let mut last = Instant::now();

    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;

        // ── Input ──────────────────────────────────────────────────────────
        let events = evdev.poll();
        mouse_x = evdev.mouse_x;
        mouse_y = evdev.mouse_y;

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
                    match code {
                        KeyCode::F1 => {
                            focus = match focus {
                                FocusPanel::Sidebar => FocusPanel::Terminal,
                                FocusPanel::Terminal => FocusPanel::Sidebar,
                            };
                        }
                        KeyCode::F2 => {
                            left_panel = match left_panel {
                                LeftPanel::Terminal => LeftPanel::Browser,
                                LeftPanel::Browser => LeftPanel::Terminal,
                            };
                        }
                        KeyCode::Tab => {
                            if focus == FocusPanel::Terminal {
                                terminal.on_tab();
                            } else {
                                focus = FocusPanel::Terminal;
                            }
                        }
                        KeyCode::Enter => match focus {
                            FocusPanel::Sidebar => {
                                if let Some(msg) = sidebar.on_submit() {
                                    if let Some(tx) = &agent_tx { let _ = tx.send(msg); }
                                }
                            }
                            FocusPanel::Terminal => {
                                terminal.on_submit();
                            }
                        },
                        KeyCode::Backspace => match focus {
                            FocusPanel::Sidebar => sidebar.on_backspace(),
                            FocusPanel::Terminal => terminal.on_backspace(),
                        },
                        KeyCode::Escape => {
                            if let Some(msg) = sidebar.on_reject() {
                                if let Some(tx) = &agent_tx { let _ = tx.send(msg); }
                            }
                        }
                        KeyCode::ArrowUp => { if focus == FocusPanel::Terminal { terminal.on_key_up(); } }
                        KeyCode::ArrowDown => { if focus == FocusPanel::Terminal { terminal.on_key_down(); } }
                        KeyCode::Space => match focus {
                            FocusPanel::Sidebar => sidebar.on_char(' '),
                            FocusPanel::Terminal => terminal.on_char(' '),
                        },
                        KeyCode::Char(c) => match focus {
                            FocusPanel::Sidebar => sidebar.on_char(c),
                            FocusPanel::Terminal => terminal.on_char(c),
                        },
                        KeyCode::Ctrl(c) => {
                            if focus == FocusPanel::Terminal {
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
                }
                InputEvent::MouseButton { button: MouseBtn::Left, pressed: true } => {
                    let div = display.width as f32 - sidebar_width;
                    if mouse_x >= div {
                        focus = FocusPanel::Sidebar;
                        let rel_x = mouse_x - div;
                        let h = display.height as f32;
                        if let Some(msg) = sidebar.on_settings_click(rel_x, mouse_y) {
                            if let Some(tx) = &agent_tx { let _ = tx.send(msg); }
                        } else {
                            sidebar.on_sidebar_click(rel_x, mouse_y, h);
                        }
                    } else {
                        focus = FocusPanel::Terminal;
                    }
                }
                InputEvent::Scroll { delta_y } => {
                    let div = display.width as f32 - sidebar_width;
                    if mouse_x < div {
                        match left_panel {
                            LeftPanel::Terminal => terminal.scroll(delta_y),
                            LeftPanel::Browser => browser_panel.scroll(delta_y),
                        }
                    } else {
                        sidebar.scroll(delta_y);
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
                            toasts.push(Toast {
                                message: format!("! {}", &message[..message.len().min(40)]),
                                color: [248, 113, 113, 255],
                                remaining: 3.5,
                            });
                        }
                        soma_common::AgentMessage::BrowserUpdate { url, title, screenshot_base64 } => {
                            browser_panel.update(url.clone(), title.clone(), screenshot_base64.as_deref());
                            left_panel = LeftPanel::Browser;
                        }
                        soma_common::AgentMessage::ConfigUpdated { provider, model } => {
                            toasts.push(Toast {
                                message: format!("Model: {} / {}", provider, model),
                                color: [99, 102, 241, 255],
                                remaining: 3.5,
                            });
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
        pixmap.fill(tiny_skia::Color::from_rgba8(30, 30, 30, 255));

        sidebar.update(dt);
        terminal.update(dt);
        toasts.retain_mut(|t| { t.remaining -= dt; t.remaining > 0.0 });

        if login.result != LoginResult::Granted {
            login.update(dt);
            login.render(&mut renderer, &mut pixmap);
        } else {
            let wf = w as f32;
            let hf = h as f32;
            let div = wf - sidebar_width;

            terminal.poll();
            if div > 10.0 {
                match left_panel {
                    LeftPanel::Terminal => {
                        terminal.render(&mut renderer, &mut pixmap, 0.0, 0.0, div, hf);
                    }
                    LeftPanel::Browser => {
                        browser_panel.render(&mut renderer, &mut pixmap, 0.0, 0.0, div, hf);
                        renderer.draw_text(&mut pixmap, "F2=Terminal", 4.0, hf - 14.0, 80.0, 8.0, [255, 255, 255, 40]);
                    }
                }
            }
            sidebar.render(&mut renderer, &mut pixmap, div, 0.0, hf);

            // Divider
            renderer.fill_rect(&mut pixmap, div - 1.0, 0.0, 2.0, hf, [255, 255, 255, 15]);

            // HITL overlay
            if sidebar.status == soma_common::AgentStatus::AwaitingApproval {
                sidebar.render_approval_overlay(&mut renderer, &mut pixmap, wf, hf);
            }

            // Detail modal
            if sidebar.expanded_msg_idx.is_some() {
                sidebar.render_expanded_msg(&mut renderer, &mut pixmap, wf, hf);
            }

            // Toasts
            let mut toast_y = 8.0_f32;
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

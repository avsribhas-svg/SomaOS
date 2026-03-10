mod ipc_client;
mod renderer;
mod sidebar;
mod terminal;

use log::info;
use renderer::Renderer;
use sidebar::Sidebar;
use soma_common::CompositorMessage;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use terminal::Terminal;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

/// Which panel has keyboard focus
#[derive(Clone, Copy, PartialEq)]
enum FocusPanel {
    Sidebar,
    Terminal,
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

struct SomaApp {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    renderer: Renderer,
    sidebar: Sidebar,
    terminal: Terminal,
    focus: FocusPanel,
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

impl SomaApp {
    fn new(runtime: tokio::runtime::Handle) -> Self {
        Self {
            window: None,
            surface: None,
            renderer: Renderer::new(),
            sidebar: Sidebar::new(),
            terminal: Terminal::new(),
            focus: FocusPanel::Sidebar,
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

        pixmap.fill(tiny_skia::Color::from_rgba8(10, 10, 20, 255));

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
        let term_w = divider;
        let sidebar_x = divider;

        // Render terminal on the LEFT
        if term_w > 10.0 {
            self.terminal
                .render(&mut self.renderer, &mut pixmap, 0.0, 0.0, term_w, h);
        }

        // Render sidebar on the RIGHT
        self.sidebar
            .render(&mut self.renderer, &mut pixmap, sidebar_x, h);

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
                self.sidebar_width = self.sidebar_width.clamp(MIN_SIDEBAR_W, (w - 200.0).min(MAX_SIDEBAR_W));
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

                    // F1 toggles panels (always works)
                    Key::Named(NamedKey::F1) => {
                        self.focus = match self.focus {
                            FocusPanel::Sidebar => FocusPanel::Terminal,
                            FocusPanel::Terminal => FocusPanel::Sidebar,
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
                        let new_sidebar_w = (total_w - self.mouse_x).clamp(MIN_SIDEBAR_W, MAX_SIDEBAR_W.min(total_w - 200.0));
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
                                self.focus = FocusPanel::Terminal;
                            } else {
                                self.focus = FocusPanel::Sidebar;
                                // Route click into sidebar (card expand / modal dismiss)
                                let h = win.inner_size().height as f32;
                                self.sidebar.on_sidebar_click(self.mouse_x - div, self.mouse_y, h);
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
                        self.terminal.scroll(scroll_amount);
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
    info!("║    SomaOS Compositor v0.6.0 (dev)    ║");
    info!("╚══════════════════════════════════════╝");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let mut app = SomaApp::new(runtime.handle().clone());
    event_loop.run_app(&mut app).expect("Event loop failed");
}

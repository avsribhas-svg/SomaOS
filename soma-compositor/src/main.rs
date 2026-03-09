mod ipc_client;
mod renderer;
mod sidebar;
mod terminal;

use log::info;
use renderer::Renderer;
use sidebar::{Sidebar, SIDEBAR_WIDTH};
use soma_common::CompositorMessage;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use terminal::Terminal;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

/// Which panel has keyboard focus
#[derive(Clone, Copy, PartialEq)]
enum FocusPanel {
    Sidebar,
    Terminal,
}

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

    fn poll_agent_messages(&mut self) -> bool {
        let mut got_message = false;
        if let Some(rx) = &self.agent_rx {
            if let Ok(mut rx) = rx.try_lock() {
                while let Ok(msg) = rx.try_recv() {
                    got_message = true;
                    match &msg {
                        soma_common::AgentMessage::DirectOutput { result, .. } => {
                            self.terminal.add_output(&result.stdout, &result.stderr);
                        }
                        _ => {}
                    }
                    self.sidebar.handle_agent_message(msg);
                }
            }
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

        // Resize surface first (borrowing surface briefly)
        if let Some(surface) = &mut self.surface {
            let _ = surface.resize(
                NonZeroU32::new(width).unwrap(),
                NonZeroU32::new(height).unwrap(),
            );
        } else {
            return;
        }

        // Create pixmap for rendering
        let mut pixmap = match tiny_skia::Pixmap::new(width, height) {
            Some(p) => p,
            None => return,
        };

        // Clear
        pixmap.fill(tiny_skia::Color::from_rgba8(10, 10, 20, 255));

        let w = width as f32;
        let h = height as f32;

        // Update animations
        self.sidebar.update(1.0 / 60.0);
        self.terminal.update(1.0 / 60.0);

        // Poll agent messages (no surface borrow here)
        let got_agent_msg = self.poll_agent_messages();

        // Render terminal on the LEFT
        let term_w = (w - SIDEBAR_WIDTH).max(0.0);
        if term_w > 10.0 {
            self.terminal
                .render(&mut self.renderer, &mut pixmap, 0.0, 0.0, term_w, h);
        }

        // Render sidebar on the RIGHT
        let sidebar_x = (w - SIDEBAR_WIDTH).max(0.0);
        self.sidebar.render(&mut self.renderer, &mut pixmap, sidebar_x, h);

        // Focus indicator — highlight the border of the focused panel
        let focus_color = [129, 140, 248, 60]; // Subtle accent
        match self.focus {
            FocusPanel::Sidebar => {
                self.renderer.fill_rect(&mut pixmap, sidebar_x + SIDEBAR_WIDTH - 2.0, 0.0, 2.0, h, focus_color);
            }
            FocusPanel::Terminal => {
                self.renderer.fill_rect(&mut pixmap, 0.0, 0.0, 2.0, h, focus_color);
            }
        }

        // Render HITL overlay (if awaiting approval)
        if self.sidebar.status == soma_common::AgentStatus::AwaitingApproval {
            self.sidebar
                .render_approval_overlay(&mut self.renderer, &mut pixmap, w, h);
        }

        // Copy pixmap to softbuffer surface (borrow surface at the end)
        if let Some(surface) = &mut self.surface {
            let mut buffer = surface.buffer_mut().unwrap();
            let pixels = pixmap.pixels();
            for (i, pixel) in pixels.iter().enumerate() {
                // tiny-skia uses premultiplied RGBA, softbuffer expects 0RGB
                buffer[i] = ((pixel.red() as u32) << 16)
                    | ((pixel.green() as u32) << 8)
                    | (pixel.blue() as u32);
            }
            let _ = buffer.present();
        }

        // If agent sent messages, request another redraw to keep polling
        if got_agent_msg {
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
        let context = softbuffer::Context::new(window.clone()).expect("Failed to create softbuffer context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("Failed to create surface");

        self.window = Some(window);
        self.surface = Some(surface);

        // Try connecting to agent
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
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                self.redraw();
                // Continuous redraw when agent is working or we're waiting for a response
                let needs_poll = matches!(
                    self.sidebar.status,
                    soma_common::AgentStatus::Thinking
                        | soma_common::AgentStatus::Executing
                );
                if needs_poll {
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }

            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    logical_key,
                    state: ElementState::Pressed,
                    ..
                },
                ..
            } => {
                match &logical_key {
                    // Tab switches focus between sidebar and terminal
                    Key::Named(NamedKey::Tab) => {
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
                            if let Some(cmd) = self.terminal.on_submit() {
                                let id = uuid::Uuid::new_v4().to_string();
                                self.send_to_agent(CompositorMessage::DirectExec {
                                    id,
                                    command: cmd,
                                });
                            }
                        }
                    },

                    Key::Named(NamedKey::Backspace) => match self.focus {
                        FocusPanel::Sidebar => self.sidebar.on_backspace(),
                        FocusPanel::Terminal => self.terminal.on_backspace(),
                    },

                    Key::Named(NamedKey::Escape) => {
                        if let Some(msg) = self.sidebar.on_reject() {
                            self.send_to_agent(msg);
                        }
                    }

                    Key::Named(NamedKey::Space) => {
                        match self.focus {
                            FocusPanel::Sidebar => self.sidebar.on_char(' '),
                            FocusPanel::Terminal => self.terminal.on_char(' '),
                        }
                    }

                    Key::Character(c) => {
                        for ch in c.chars() {
                            match self.focus {
                                FocusPanel::Sidebar => self.sidebar.on_char(ch),
                                FocusPanel::Terminal => self.terminal.on_char(ch),
                            }
                        }
                    }

                    _ => {}
                }

                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 40.0,
                    // macOS trackpad: negate for natural scroll, scale up for responsiveness
                    MouseScrollDelta::PixelDelta(pos) => -(pos.y as f32) * 1.5,
                };
                // Scroll the focused panel
                match self.focus {
                    FocusPanel::Sidebar => self.sidebar.scroll(scroll_amount),
                    FocusPanel::Terminal => {}, // TODO: terminal scroll
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
    info!("║    SomaOS Compositor v0.3.0 (dev)    ║");
    info!("╚══════════════════════════════════════╝");

    // Create a tokio runtime for async IPC
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let mut app = SomaApp::new(runtime.handle().clone());
    event_loop.run_app(&mut app).expect("Event loop failed");
}

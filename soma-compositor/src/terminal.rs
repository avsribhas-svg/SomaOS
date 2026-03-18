use crate::renderer::Renderer;
use tiny_skia::Pixmap;

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{mpsc, Arc, Mutex};

/// A real PTY-backed terminal
pub struct Terminal {
    pub lines: Vec<TermLine>,
    pub input_buf: String, // not used for PTY mode, but kept for fallback
    pub scroll_offset: f32,
    pub cursor_visible: bool,
    cursor_timer: f32,
    // PTY
    master_fd: Option<Arc<Mutex<OwnedFd>>>,
    pty_rx: Option<mpsc::Receiver<PtyEvent>>,
    child_pid: Option<nix::unistd::Pid>,
    // Current incomplete line from PTY output
    partial_line: String,
    // Cursor position for basic tracking
    cursor_col: usize,
    current_line: String,
}

pub struct TermLine {
    pub content: String,
    pub is_input: bool, // user-typed line
    pub color: TermColor,
}

#[derive(Clone, Copy)]
pub enum TermColor {
    Default,
    Error,
    Dim,
    Green,
    Yellow,
    Blue,
    Cyan,
}

enum PtyEvent {
    Output(Vec<u8>),
    Closed,
}

const MAX_LINES: usize = 2000;
const DRAIN_LINES: usize = 500;

impl Terminal {
    pub fn new() -> Self {
        let mut term = Self {
            lines: Vec::new(),
            input_buf: String::new(),
            scroll_offset: 0.0,
            cursor_visible: true,
            cursor_timer: 0.0,
            master_fd: None,
            pty_rx: None,
            child_pid: None,
            partial_line: String::new(),
            cursor_col: 0,
            current_line: String::new(),
        };

        // Try to spawn PTY
        if let Err(e) = term.spawn_pty() {
            term.lines.push(TermLine {
                content: format!("Failed to spawn PTY: {}", e),
                is_input: false,
                color: TermColor::Error,
            });
        }

        term
    }

    fn spawn_pty(&mut self) -> Result<(), String> {
        // Open a PTY pair
        let pty = nix::pty::openpty(None, None).map_err(|e| format!("openpty: {}", e))?;

        let master_fd = pty.master;
        let slave_fd = pty.slave;

        // Fork
        match unsafe { nix::unistd::fork() } {
            Ok(nix::unistd::ForkResult::Child) => {
                // Child process: set up as session leader and connect to slave PTY
                drop(master_fd); // close master in child

                let _ = nix::unistd::setsid();

                // Set slave as controlling terminal
                unsafe {
                    libc::ioctl(slave_fd.as_raw_fd(), libc::TIOCSCTTY as _, 0);
                }

                // Redirect stdin/stdout/stderr to slave
                let slave_raw = slave_fd.as_raw_fd();
                let _ = nix::unistd::dup2(slave_raw, 0); // stdin
                let _ = nix::unistd::dup2(slave_raw, 1); // stdout
                let _ = nix::unistd::dup2(slave_raw, 2); // stderr
                if slave_raw > 2 {
                    drop(slave_fd);
                }

                // Set terminal size
                let winsize = nix::pty::Winsize {
                    ws_row: 40,
                    ws_col: 120,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                unsafe {
                    libc::ioctl(0, libc::TIOCSWINSZ as _, &winsize);
                }

                // Exec shell
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                // CString::new fails only if the string contains null bytes; fall back to /bin/sh
                let shell_cstr = std::ffi::CString::new(shell.clone())
                    .unwrap_or_else(|_| std::ffi::CString::new("/bin/sh").expect("hardcoded valid shell path"));
                let args = [shell_cstr.clone()];
                let _ = nix::unistd::execvp(&shell_cstr, &args);
                std::process::exit(1);
            }
            Ok(nix::unistd::ForkResult::Parent { child }) => {
                drop(slave_fd); // close slave in parent

                self.child_pid = Some(child);

                // Set master to non-blocking
                let flags = nix::fcntl::fcntl(master_fd.as_raw_fd(), nix::fcntl::FcntlArg::F_GETFL)
                    .map_err(|e| format!("fcntl get: {}", e))?;
                let mut oflags = nix::fcntl::OFlag::from_bits_truncate(flags);
                oflags |= nix::fcntl::OFlag::O_NONBLOCK;
                nix::fcntl::fcntl(master_fd.as_raw_fd(), nix::fcntl::FcntlArg::F_SETFL(oflags))
                    .map_err(|e| format!("fcntl set: {}", e))?;

                let master_arc = Arc::new(Mutex::new(master_fd));
                self.master_fd = Some(master_arc.clone());

                // Spawn reader thread
                let (tx, rx) = mpsc::channel();
                self.pty_rx = Some(rx);

                std::thread::spawn(move || {
                    // Create a blocking fd copy for reading
                    let raw_fd = master_arc.lock().unwrap().as_raw_fd();
                    let mut file = unsafe { std::fs::File::from_raw_fd(raw_fd) };
                    let mut buf = [0u8; 4096];

                    loop {
                        // Use poll to wait for data with timeout
                        let mut poll_fds = [nix::poll::PollFd::new(
                            unsafe { std::os::fd::BorrowedFd::borrow_raw(raw_fd) },
                            nix::poll::PollFlags::POLLIN,
                        )];

                        match nix::poll::poll(&mut poll_fds, nix::poll::PollTimeout::from(100u16)) {
                            Ok(n) if n > 0 => {
                                match file.read(&mut buf) {
                                    Ok(0) => {
                                        let _ = tx.send(PtyEvent::Closed);
                                        break;
                                    }
                                    Ok(n) => {
                                        let _ = tx.send(PtyEvent::Output(buf[..n].to_vec()));
                                    }
                                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                        continue;
                                    }
                                    Err(_) => {
                                        let _ = tx.send(PtyEvent::Closed);
                                        break;
                                    }
                                }
                            }
                            Ok(_) => continue, // timeout
                            Err(_) => {
                                let _ = tx.send(PtyEvent::Closed);
                                break;
                            }
                        }
                    }

                    // Don't drop the file — we don't own the fd
                    std::mem::forget(file);
                });

                self.lines.push(TermLine {
                    content: "SomaOS Terminal v1.0 (PTY)".to_string(),
                    is_input: false,
                    color: TermColor::Dim,
                });

                Ok(())
            }
            Err(e) => Err(format!("fork: {}", e)),
        }
    }

    /// Write a single character to the PTY
    pub fn on_char(&mut self, c: char) {
        self.write_to_pty(&[c as u8]);
    }

    /// Handle backspace — send to PTY
    pub fn on_backspace(&mut self) {
        self.write_to_pty(&[0x7f]); // DEL
    }

    /// Handle enter — send newline to PTY
    pub fn on_submit(&mut self) -> Option<String> {
        self.write_to_pty(b"\n");
        None // PTY handles everything, no need to return command
    }

    /// Handle special keys
    pub fn on_key_up(&mut self) {
        self.write_to_pty(b"\x1b[A"); // Up arrow
    }

    pub fn on_key_down(&mut self) {
        self.write_to_pty(b"\x1b[B"); // Down arrow
    }

    pub fn on_tab(&mut self) {
        self.write_to_pty(b"\t"); // Tab for completion
    }

    pub fn on_ctrl_c(&mut self) {
        self.write_to_pty(&[0x03]); // ETX
    }

    pub fn on_ctrl_d(&mut self) {
        self.write_to_pty(&[0x04]); // EOT
    }

    pub fn on_ctrl_l(&mut self) {
        self.write_to_pty(&[0x0c]); // Form feed (clear)
    }

    fn write_to_pty(&mut self, data: &[u8]) {
        if let Some(fd) = &self.master_fd {
            if let Ok(f) = fd.lock() {
                let raw = f.as_raw_fd();
                unsafe {
                    let mut file = std::fs::File::from_raw_fd(raw);
                    let _ = file.write_all(data);
                    let _ = file.flush();
                    std::mem::forget(file); // don't close the fd
                }
            }
        }
    }

    pub fn scroll(&mut self, delta: f32) {
        self.scroll_offset = (self.scroll_offset + delta).max(0.0);
    }

    /// Poll for PTY output — returns true if new data arrived
    pub fn poll(&mut self) -> bool {
        // Collect events first to avoid borrow conflict
        let events: Vec<PtyEvent> = if let Some(rx) = &self.pty_rx {
            let mut evts = Vec::new();
            while let Ok(event) = rx.try_recv() {
                evts.push(event);
            }
            evts
        } else {
            return false;
        };

        let mut got_data = false;
        for event in events {
            match event {
                PtyEvent::Output(data) => {
                    got_data = true;
                    self.process_output(&data);
                }
                PtyEvent::Closed => {
                    self.lines.push(TermLine {
                        content: "[shell exited]".to_string(),
                        is_input: false,
                        color: TermColor::Error,
                    });
                }
            }
        }

        if got_data {
            // Auto-scroll to bottom on new output
            let total_h = self.lines.len() as f32 * 16.0;
            self.scroll_offset = total_h; // will be clamped in render
        }

        got_data
    }

    /// Process raw PTY output bytes, handling basic ANSI escape sequences
    fn process_output(&mut self, data: &[u8]) {
        let text = String::from_utf8_lossy(data);

        let mut i = 0;
        let chars: Vec<char> = text.chars().collect();

        while i < chars.len() {
            let c = chars[i];

            if c == '\x1b' {
                // Escape sequence — consume it
                i += 1;
                if i < chars.len() && chars[i] == '[' {
                    i += 1;
                    // CSI sequence: consume until letter
                    while i < chars.len() && !chars[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    if i < chars.len() {
                        let cmd = chars[i];
                        match cmd {
                            'J' | 'K' => {
                                // Clear screen / clear line — just add a blank
                            }
                            'H' => {
                                // Cursor home
                                self.flush_partial();
                            }
                            _ => {} // Skip other CSI sequences
                        }
                        i += 1;
                    }
                } else if i < chars.len() {
                    i += 1; // Skip single-char escape
                }
                continue;
            }

            match c {
                '\n' => {
                    self.flush_partial();
                }
                '\r' => {
                    // Carriage return — reset partial line
                    self.partial_line.clear();
                }
                '\x08' => {
                    // Backspace
                    self.partial_line.pop();
                }
                '\t' => {
                    // Tab — expand to spaces
                    let spaces = 8 - (self.partial_line.len() % 8);
                    for _ in 0..spaces {
                        self.partial_line.push(' ');
                    }
                }
                '\x07' => {} // Bell — ignore
                c if c >= ' ' || c == '\t' => {
                    self.partial_line.push(c);
                }
                _ => {} // Ignore other control chars
            }

            i += 1;
        }

        // Cap lines
        if self.lines.len() > MAX_LINES {
            self.lines.drain(0..DRAIN_LINES);
        }
    }

    fn flush_partial(&mut self) {
        let content = std::mem::take(&mut self.partial_line);
        self.lines.push(TermLine {
            content,
            is_input: false,
            color: TermColor::Default,
        });
    }

    pub fn update(&mut self, dt: f32) {
        self.cursor_timer += dt;
        if self.cursor_timer > 0.53 {
            self.cursor_timer = 0.0;
            self.cursor_visible = !self.cursor_visible;
        }
    }

    /// For DirectExec output from the agent (fallback)
    pub fn add_output(&mut self, stdout: &str, stderr: &str) {
        for line in stdout.lines() {
            self.lines.push(TermLine {
                content: line.to_string(),
                is_input: false,
                color: TermColor::Default,
            });
        }
        for line in stderr.lines() {
            self.lines.push(TermLine {
                content: line.to_string(),
                is_input: false,
                color: TermColor::Error,
            });
        }
    }

    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        pixmap: &mut Pixmap,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let t = renderer.theme.clone();

        // Background
        renderer.fill_rect(pixmap, x, y, w, h, t.terminal_bg);

        // Title bar
        renderer.fill_rect(pixmap, x, y, w, 32.0, [255, 255, 255, 5]);
        renderer.fill_rect(pixmap, x, y + 31.0, w, 1.0, t.border);
        renderer.draw_text(pixmap, "> Terminal", x + 12.0, y + 9.0, 120.0, 11.0, t.text_secondary);

        // Shell name indicator
        let shell = std::env::var("SHELL")
            .ok()
            .and_then(|s| s.rsplit('/').next().map(String::from))
            .unwrap_or_else(|| "sh".to_string());
        renderer.draw_text(pixmap, &shell, x + w - 60.0, y + 9.0, 48.0, 10.0, t.text_muted);

        // Content area
        let content_y = y + 36.0;
        let content_h = h - 36.0;
        let line_height = 16.0;

        // Include partial line in display
        let total_lines = self.lines.len() + if self.partial_line.is_empty() { 0 } else { 1 };

        // Clamp scroll
        let max_scroll = ((total_lines as f32 * line_height) - content_h + 16.0).max(0.0);
        self.scroll_offset = self.scroll_offset.min(max_scroll);

        // Auto-scroll near bottom
        if max_scroll - self.scroll_offset < 30.0 {
            self.scroll_offset = max_scroll;
        }

        let scroll = self.scroll_offset;

        // Calculate which lines to show
        let skip = (scroll / line_height) as usize;
        let mut ly = content_y + 4.0 - (scroll % line_height);

        for (_i, line) in self.lines.iter().enumerate().skip(skip) {
            if ly > content_y + content_h { break; }
            if ly + line_height < content_y {
                ly += line_height;
                continue;
            }

            let color = match line.color {
                TermColor::Default => t.terminal_text,
                TermColor::Error => t.error,
                TermColor::Dim => t.text_muted,
                TermColor::Green => t.success,
                TermColor::Yellow => t.warning,
                TermColor::Blue => t.accent,
                TermColor::Cyan => [0, 188, 212, 255],
            };

            if !line.content.is_empty() {
                renderer.draw_text(pixmap, &line.content, x + 10.0, ly, w - 20.0, 11.0, color);
            }
            ly += line_height;
        }

        // Render partial line (current output being built) with cursor
        if !self.partial_line.is_empty() || self.lines.is_empty() {
            let display = if self.cursor_visible {
                format!("{}_", self.partial_line)
            } else {
                self.partial_line.clone()
            };
            if ly <= content_y + content_h {
                renderer.draw_text(pixmap, &display, x + 10.0, ly, w - 20.0, 11.0, t.terminal_text);
            }
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Kill child process
        if let Some(pid) = self.child_pid {
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM);
        }
    }
}

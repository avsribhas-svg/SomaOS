use crate::renderer::Renderer;
use tiny_skia::Pixmap;

/// A simple terminal panel that displays command history and output
pub struct Terminal {
    pub lines: Vec<TermLine>,
    pub input: String,
    pub scroll_offset: usize,
    pub cursor_visible: bool,
    cursor_timer: f32,
    pub prompt: String,
}

pub struct TermLine {
    pub content: String,
    pub is_command: bool,
    pub is_error: bool,
}

impl Terminal {
    pub fn new() -> Self {
        let mut term = Self {
            lines: Vec::new(),
            input: String::new(),
            scroll_offset: 0,
            cursor_visible: true,
            cursor_timer: 0.0,
            prompt: "soma λ ".to_string(),
        };
        term.lines.push(TermLine {
            content: "SomaOS Terminal v0.1.0".to_string(),
            is_command: false,
            is_error: false,
        });
        term.lines.push(TermLine {
            content: "Type commands directly. Use the sidebar for AI-assisted actions.".to_string(),
            is_command: false,
            is_error: false,
        });
        term.lines.push(TermLine {
            content: String::new(),
            is_command: false,
            is_error: false,
        });
        term
    }

    /// Handle character input
    pub fn on_char(&mut self, c: char) {
        self.input.push(c);
    }

    /// Handle backspace
    pub fn on_backspace(&mut self) {
        self.input.pop();
    }

    /// Handle enter — returns the command string to execute (if any)
    pub fn on_submit(&mut self) -> Option<String> {
        let cmd = self.input.trim().to_string();
        if cmd.is_empty() {
            return None;
        }

        self.lines.push(TermLine {
            content: format!("{}{}", self.prompt, cmd),
            is_command: true,
            is_error: false,
        });

        self.input.clear();
        Some(cmd)
    }

    /// Add output lines from a command result
    pub fn add_output(&mut self, stdout: &str, stderr: &str) {
        for line in stdout.lines() {
            self.lines.push(TermLine {
                content: line.to_string(),
                is_command: false,
                is_error: false,
            });
        }
        for line in stderr.lines() {
            self.lines.push(TermLine {
                content: line.to_string(),
                is_command: false,
                is_error: true,
            });
        }
        // Keep a blank line after output
        self.lines.push(TermLine {
            content: String::new(),
            is_command: false,
            is_error: false,
        });

        // Cap total lines
        if self.lines.len() > 2000 {
            self.lines.drain(0..500);
        }
    }

    /// Update animations
    pub fn update(&mut self, dt: f32) {
        self.cursor_timer += dt;
        if self.cursor_timer > 0.53 {
            self.cursor_timer = 0.0;
            self.cursor_visible = !self.cursor_visible;
        }
    }

    /// Render the terminal panel
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

        // Terminal title bar
        renderer.fill_rect(pixmap, x, y, w, 32.0, [255, 255, 255, 5]);
        renderer.fill_rect(pixmap, x, y + 31.0, w, 1.0, t.border);
        renderer.draw_text(pixmap, "▸ Terminal", x + 12.0, y + 9.0, 120.0, 11.0, t.text_secondary);

        // Content area
        let content_y = y + 36.0;
        let content_h = h - 36.0 - 36.0; // minus titlebar and input row
        let line_height = 16.0;
        let max_visible = (content_h / line_height) as usize;

        // Auto-scroll to bottom
        let total_lines = self.lines.len();
        let start = if total_lines > max_visible {
            total_lines - max_visible
        } else {
            0
        };

        let mut ly = content_y + 4.0;
        for line in &self.lines[start..] {
            if ly > content_y + content_h {
                break;
            }
            let color = if line.is_command {
                [180, 220, 255, 255] // Bright blue for commands
            } else if line.is_error {
                t.error
            } else {
                t.terminal_text
            };
            if !line.content.is_empty() {
                renderer.draw_text(pixmap, &line.content, x + 10.0, ly, w - 20.0, 11.0, color);
            }
            ly += line_height;
        }

        // Input row at bottom
        let input_y = y + h - 34.0;
        renderer.fill_rect(pixmap, x, input_y - 1.0, w, 1.0, t.border);
        renderer.fill_rect(pixmap, x, input_y, w, 34.0, [255, 255, 255, 3]);

        let display = if self.cursor_visible {
            format!("{}{}{}", self.prompt, self.input, "▊")
        } else {
            format!("{}{}", self.prompt, self.input)
        };
        renderer.draw_text(pixmap, &display, x + 10.0, input_y + 10.0, w - 20.0, 12.0, t.terminal_text);
    }
}

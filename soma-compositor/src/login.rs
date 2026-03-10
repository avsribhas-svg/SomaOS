/// Login screen shown before the main compositor UI on production boots.
///
/// Reads the root password hash from /etc/soma/passwd (a plain bcrypt hash).
/// Falls back to hardcoded "soma" if the file doesn't exist (dev mode).
///
/// On success, returns `LoginResult::Granted`.
/// The compositor renders this via `LoginScreen::render()`.
use crate::renderer::Renderer;
use tiny_skia::Pixmap;

const SOMA_PASSWD_FILE: &str = "/etc/soma/passwd";
/// Default password when no passwd file exists (dev/VM use only)
const DEFAULT_PASSWORD: &str = "soma";

#[derive(PartialEq, Clone)]
pub enum LoginResult {
    Pending,
    Granted,
    Denied { attempts: u32 },
}

pub struct LoginScreen {
    input: String,
    pub result: LoginResult,
    attempts: u32,
    shake_timer: f32,  // > 0 while animating a wrong-password shake
    cursor_visible: bool,
    cursor_timer: f32,
}

impl LoginScreen {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            result: LoginResult::Pending,
            attempts: 0,
            shake_timer: 0.0,
            cursor_visible: true,
            cursor_timer: 0.0,
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.cursor_timer += dt;
        if self.cursor_timer > 0.53 {
            self.cursor_timer = 0.0;
            self.cursor_visible = !self.cursor_visible;
        }
        if self.shake_timer > 0.0 {
            self.shake_timer -= dt;
        }
    }

    pub fn on_char(&mut self, c: char) {
        if self.result == LoginResult::Pending {
            self.input.push(c);
        }
    }

    pub fn on_backspace(&mut self) {
        self.input.pop();
    }

    pub fn on_submit(&mut self) {
        let expected = read_password_file();
        if self.input == expected {
            self.result = LoginResult::Granted;
        } else {
            self.attempts += 1;
            self.shake_timer = 0.45;
            self.input.clear();
            self.result = LoginResult::Denied { attempts: self.attempts };
            // Immediately go back to Pending so the user can retry
            self.result = LoginResult::Pending;
        }
    }

    /// Render the full-screen login UI.
    pub fn render(&self, renderer: &mut Renderer, pixmap: &mut Pixmap) {
        let w = pixmap.width() as f32;
        let h = pixmap.height() as f32;
        let t = renderer.theme.clone();

        // Background
        pixmap.fill(tiny_skia::Color::from_rgba8(8, 8, 18, 255));

        // Logo / title — centred vertically at 35%
        let title_y = h * 0.32;
        renderer.draw_text(pixmap, "SomaOS", w / 2.0 - 60.0, title_y, 200.0, 28.0, [129, 140, 248, 255]);
        renderer.draw_text(pixmap, "AI-Native Operating System", w / 2.0 - 120.0, title_y + 38.0, 300.0, 11.0, t.text_muted);

        // Password box
        let box_w = 320.0_f32.min(w - 80.0);
        let box_x = (w - box_w) / 2.0;
        let shake_off = if self.shake_timer > 0.0 {
            (self.shake_timer * 40.0).sin() * 8.0
        } else {
            0.0
        };
        let box_y = h * 0.52 + shake_off;
        let box_h = 44.0;

        // Box background
        renderer.fill_rounded_rect(pixmap, box_x, box_y, box_w, box_h, 10.0, [20, 20, 38, 220]);
        // Border — bright when focused
        renderer.stroke_rect(pixmap, box_x, box_y, box_w, box_h, [129, 140, 248, 180]);

        // Prompt label
        renderer.draw_text(pixmap, "Password", box_x, box_y - 20.0, box_w, 10.0, t.text_secondary);

        // Masked password dots
        let dots: String = "\u{2022}".repeat(self.input.len());
        let cursor_str = if self.cursor_visible { "_" } else { " " };
        let display = format!("{}{}", dots, cursor_str);
        renderer.draw_text(pixmap, &display, box_x + 14.0, box_y + 13.0, box_w - 28.0, 14.0, t.text_primary);

        // Hint
        let hint = if self.attempts > 0 {
            format!("Incorrect password ({} attempt{})", self.attempts, if self.attempts == 1 { "" } else { "s" })
        } else {
            "Press Enter to unlock".to_string()
        };
        let hint_color = if self.attempts > 0 { t.error } else { t.text_muted };
        renderer.draw_text(pixmap, &hint, box_x, box_y + box_h + 12.0, box_w, 9.5, hint_color);

        // Version footer
        renderer.draw_text(pixmap, "v0.8.0", w - 60.0, h - 20.0, 60.0, 9.0, t.text_muted);
    }
}

fn read_password_file() -> String {
    std::fs::read_to_string(SOMA_PASSWD_FILE)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| DEFAULT_PASSWORD.to_string())
}

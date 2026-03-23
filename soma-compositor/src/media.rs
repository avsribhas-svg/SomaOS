//! MediaApp — dual-interface media generation window.
//!
//! Human: types prompt, presses Enter to trigger generation.
//! Agent: media.generate, media.describe, media.save via AgentAPI.

use serde_json::{json, Value};
use soma_common::{AppState, CapabilityError, CapabilityResult, ErrorReason};
use crate::window_manager::{NativeAppContent, WindowContentType};
use crate::renderer::Renderer;
use tiny_skia::Pixmap;

#[derive(Debug, Clone, PartialEq)]
pub enum MediaStatus {
    Idle,
    Generating,
    Done,
    Error(String),
}

pub struct MediaApp {
    pub title: String,
    pub prompt: String,
    pub status: MediaStatus,
    pub image_bytes: Option<Vec<u8>>,  // PNG bytes of generated image
    pub edit_buffer: String,
    pub dirty: bool,
}

impl MediaApp {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            prompt: String::new(),
            status: MediaStatus::Idle,
            image_bytes: None,
            edit_buffer: String::new(),
            dirty: false,
        }
    }

    fn status_str(&self) -> &str {
        match &self.status {
            MediaStatus::Idle      => "idle",
            MediaStatus::Generating => "generating",
            MediaStatus::Done      => "done",
            MediaStatus::Error(_)  => "error",
        }
    }

    fn save_image(&mut self, path: &str) -> CapabilityResult {
        match &self.image_bytes {
            Some(bytes) => {
                match std::fs::write(path, bytes) {
                    Ok(_) => CapabilityResult {
                        success: true,
                        data: json!({ "path": path, "bytes": bytes.len() }),
                        error: None,
                    },
                    Err(e) => CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::InternalError, format!("Write error: {}", e))),
                    },
                }
            }
            None => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::NotFound, "No image to save — generate one first")),
            },
        }
    }
}

impl NativeAppContent for MediaApp {
    fn describe_state(&self) -> AppState {
        AppState {
            app_type: "media".to_string(),
            summary: json!({
                "title": self.title,
                "prompt": self.prompt,
                "status": self.status_str(),
                "has_image": self.image_bytes.is_some(),
            }),
            cells: None,
            dirty: self.dirty,
        }
    }

    fn execute_action(&mut self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "set_prompt" => {
                if let Some(p) = params.get("prompt").and_then(|v| v.as_str()) {
                    self.prompt = p.to_string();
                    self.status = MediaStatus::Generating;
                    self.dirty = true;
                    CapabilityResult { success: true, data: json!({ "prompt": self.prompt }), error: None }
                } else {
                    CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::MissingParam, "prompt required")),
                    }
                }
            }
            "set_status" => {
                let status = params.get("status").and_then(|v| v.as_str()).unwrap_or("idle");
                self.status = match status {
                    "generating" => MediaStatus::Generating,
                    "done"       => MediaStatus::Done,
                    "idle"       => MediaStatus::Idle,
                    s            => MediaStatus::Error(s.to_string()),
                };
                CapabilityResult { success: true, data: json!({ "status": status }), error: None }
            }
            "set_image" => {
                if let Some(b64) = params.get("image_base64").and_then(|v| v.as_str()) {
                    use base64::Engine;
                    match base64::engine::general_purpose::STANDARD.decode(b64) {
                        Ok(bytes) => {
                            self.image_bytes = Some(bytes);
                            self.status = MediaStatus::Done;
                            self.dirty = true;
                            let len = self.image_bytes.as_ref().unwrap().len();
                            CapabilityResult { success: true, data: json!({ "bytes": len }), error: None }
                        }
                        Err(e) => CapabilityResult {
                            success: false,
                            data: Value::Null,
                            error: Some(CapabilityError::new(ErrorReason::InvalidParam, format!("base64 decode error: {}", e))),
                        },
                    }
                } else {
                    CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::MissingParam, "image_base64 required")),
                    }
                }
            }
            "save_image" => {
                let path = match params.get("path").and_then(|v| v.as_str()) {
                    Some(p) if !p.is_empty() => p.to_string(),
                    _ => return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::MissingParam, "path required")),
                    },
                };
                self.save_image(&path)
            }
            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(
                    ErrorReason::UnknownAction,
                    format!("Unknown media action: {}", action),
                )),
            },
        }
    }

    fn render(&self, renderer: &mut Renderer, pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, _cursor_visible: bool) {
        use tiny_skia::*;

        let mut paint = Paint::default();

        // Background
        paint.set_color(Color::from_rgba8(18, 18, 24, 255));
        if let Some(rect) = Rect::from_xywh(x, y, w, h) {
            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }

        // Prompt bar (top 40px)
        let bar_h = 40.0;
        paint.set_color(Color::from_rgba8(30, 30, 40, 255));
        if let Some(rect) = Rect::from_xywh(x, y, w, bar_h) {
            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }

        let prompt_text = if !self.edit_buffer.is_empty() {
            format!("{}_", self.edit_buffer)
        } else if !self.prompt.is_empty() {
            self.prompt.clone()
        } else {
            "Type a prompt and press Enter to generate…".to_string()
        };
        renderer.draw_text(pixmap, &prompt_text, x + 12.0, y + 12.0, w - 24.0, 14.0, [200, 200, 210, 255]);

        // Main area
        let img_y = y + bar_h + 8.0;
        let img_h = h - bar_h - 48.0;

        if let Some(ref bytes) = self.image_bytes {
            if let Ok(img) = image::load_from_memory(bytes) {
                let rgba = img.to_rgba8();
                let iw = rgba.width();
                let ih = rgba.height();
                if iw > 0 && ih > 0 {
                    // Convert straight alpha → premultiplied for tiny-skia
                    let mut pre = rgba.into_raw();
                    for px in pre.chunks_exact_mut(4) {
                        let a = px[3] as f32 / 255.0;
                        px[0] = (px[0] as f32 * a) as u8;
                        px[1] = (px[1] as f32 * a) as u8;
                        px[2] = (px[2] as f32 * a) as u8;
                    }
                    if let Some(src_pm) = PixmapRef::from_bytes(&pre, iw, ih) {
                        let scale = (w / iw as f32).min(img_h / ih as f32).min(1.0);
                        let dw = iw as f32 * scale;
                        let dh = ih as f32 * scale;
                        let dx = (x + (w - dw) / 2.0) as i32;
                        let dy = (img_y + (img_h - dh) / 2.0) as i32;
                        let img_paint = PixmapPaint::default();
                        pixmap.draw_pixmap(dx, dy, src_pm, &img_paint, Transform::from_scale(scale, scale), None);
                    }
                }
            }
        } else {
            let placeholder = match &self.status {
                MediaStatus::Idle       => "No image generated yet",
                MediaStatus::Generating => "Generating…",
                MediaStatus::Done       => "Generation complete",
                MediaStatus::Error(e)   => e.as_str(),
            };
            renderer.draw_text(pixmap, placeholder, x + w / 4.0, y + h / 2.0 - 8.0, w / 2.0, 13.0, [100, 100, 120, 255]);
        }

        // Status bar (bottom 40px)
        let sb_y = y + h - 40.0;
        paint.set_color(Color::from_rgba8(22, 22, 32, 255));
        if let Some(rect) = Rect::from_xywh(x, sb_y, w, 40.0) {
            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
        let status_text = format!("Status: {} | Prompt: {}",
            self.status_str(),
            if self.prompt.is_empty() { "(none)" } else { &self.prompt },
        );
        renderer.draw_text(pixmap, &status_text, x + 12.0, sb_y + 12.0, w - 24.0, 12.0, [140, 140, 160, 255]);
    }

    fn on_char(&mut self, c: char) {
        self.edit_buffer.push(c);
    }

    fn on_backspace(&mut self) {
        self.edit_buffer.pop();
    }

    fn on_key(&mut self, key: &str) -> Option<AppState> {
        match key {
            "Enter" => {
                if !self.edit_buffer.is_empty() {
                    self.prompt = std::mem::take(&mut self.edit_buffer);
                    self.status = MediaStatus::Generating;
                    self.dirty = true;
                    Some(self.describe_state())
                } else {
                    None
                }
            }
            "Escape" => {
                self.edit_buffer.clear();
                None
            }
            _ => None,
        }
    }

    fn on_click(&mut self, _rel_x: f32, _rel_y: f32) -> Option<AppState> {
        None
    }

    fn content_type_id(&self) -> WindowContentType {
        WindowContentType::Media
    }
}

//! soma-docs — dual-interface document editor for SomaOS v1.2
//!
//! Both the human and the agent are first-class users:
//!   Human: click blocks, type text, Enter/Escape/Arrow navigation
//!   Agent: `append_block`, `write_block`, `delete_block`, `read_blocks`, `set_title`
//!          via `NativeAppContent`
//!
//! Both share the same `blocks` Vec; edits from either side are immediately
//! visible to the other via `AppStateChanged` IPC messages.

use serde_json::{json, Value};
use soma_common::{AppState, CapabilityError, CapabilityResult, ErrorReason};
use crate::renderer::Renderer;
use crate::window_manager::{NativeAppContent, WindowContentType};
use tiny_skia::Pixmap;

// ─────────────────────────────────────────────────────────────────────────────
//  Block model
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Block {
    Heading { level: u8, text: String }, // level 1-3
    Paragraph(String),
    CodeBlock { lang: String, code: String },
    HRule,
}

impl Block {
    /// Raw text content of the block (for edit buffer init).
    pub fn raw_text(&self) -> String {
        match self {
            Block::Heading { text, .. } => text.clone(),
            Block::Paragraph(t) => t.clone(),
            Block::CodeBlock { code, .. } => code.clone(),
            Block::HRule => String::new(),
        }
    }

    /// Modify the text in-place (for write_block / commit).
    pub fn set_text(&mut self, new_text: &str) {
        match self {
            Block::Heading { text, .. } => *text = new_text.to_string(),
            Block::Paragraph(t) => *t = new_text.to_string(),
            Block::CodeBlock { code, .. } => *code = new_text.to_string(),
            Block::HRule => {}
        }
    }

    /// Estimated height in pixels for layout / hit testing.
    pub fn height(&self, content_w: f32) -> f32 {
        match self {
            Block::Heading { .. } => 30.0,
            Block::Paragraph(text) => {
                let chars_per_line = ((content_w - 24.0) / 7.0).max(1.0) as usize;
                let lines = (text.len().max(1) + chars_per_line - 1) / chars_per_line;
                (lines as f32 * 18.0).max(24.0) + 8.0
            }
            Block::CodeBlock { code, .. } => {
                let line_count = code.lines().count().max(1);
                ((line_count + 1) as f32 * 16.0).max(40.0) + 16.0
            }
            Block::HRule => 16.0,
        }
    }

    /// Serialize to JSON for AppState summary.
    pub fn to_json(&self) -> Value {
        match self {
            Block::Heading { level, text } => json!({
                "type": "heading",
                "level": level,
                "text": text,
            }),
            Block::Paragraph(text) => json!({
                "type": "paragraph",
                "text": text,
            }),
            Block::CodeBlock { lang, code } => json!({
                "type": "code",
                "lang": lang,
                "text": code,
            }),
            Block::HRule => json!({ "type": "hline" }),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Layout constants
// ─────────────────────────────────────────────────────────────────────────────

const TITLE_BAR_H: f32  = 30.0;
const CONTENT_PAD: f32  = 16.0;
const BLOCK_GAP: f32    =  8.0;
const ACCENT: [u8; 4]   = [129, 140, 248, 255];
const ACCENT_DIM: [u8; 4] = [100, 110, 200, 255];
const ACCENT_FAINT: [u8; 4] = [80, 90, 170, 255];
const CODE_BG: [u8; 4]  = [20, 20, 38, 220];
const CODE_FG: [u8; 4]  = [167, 243, 208, 255];
const HLINE_CLR: [u8; 4] = [60, 60, 90, 200];

// ─────────────────────────────────────────────────────────────────────────────
//  DocsApp
// ─────────────────────────────────────────────────────────────────────────────

pub struct DocsApp {
    pub title: String,
    blocks: Vec<Block>,
    /// Index of the selected block.
    selection: usize,
    /// Text being typed into the selected block (None = not editing).
    edit_buffer: Option<String>,
    cursor_visible: bool,
    cursor_timer: f32,
    scroll_offset: f32,
    pub dirty: bool,
}

impl DocsApp {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            blocks: Vec::new(),
            selection: 0,
            edit_buffer: None,
            cursor_visible: true,
            cursor_timer: 0.0,
            scroll_offset: 0.0,
            dirty: false,
        }
    }

    // ── Internal helpers ───────────────────────────────────────────────────

    fn commit_edit_buffer(&mut self) {
        if let Some(buf) = self.edit_buffer.take() {
            if self.blocks.is_empty() {
                self.blocks.push(Block::Paragraph(buf));
            } else if let Some(blk) = self.blocks.get_mut(self.selection) {
                blk.set_text(&buf);
            }
            self.dirty = true;
        }
    }

    fn word_count(&self) -> usize {
        self.blocks.iter().map(|b| {
            match b {
                Block::Heading { text, .. } | Block::Paragraph(text) => {
                    text.split_whitespace().count()
                }
                Block::CodeBlock { code, .. } => code.split_whitespace().count(),
                Block::HRule => 0,
            }
        }).sum()
    }

    fn build_app_state(&self) -> AppState {
        let blocks_json: Vec<Value> = self.blocks.iter().map(|b| b.to_json()).collect();
        let summary = json!({
            "title": self.title,
            "block_count": self.blocks.len(),
            "word_count": self.word_count(),
            "selection": self.selection,
            "blocks": blocks_json,
        });
        AppState {
            app_type: "document".to_string(),
            summary,
            cells: None,
            dirty: self.dirty,
        }
    }

    /// y-offset of block `idx` within the scrollable content area.
    fn block_y_offset(&self, idx: usize, content_w: f32) -> f32 {
        let mut y = 0.0_f32;
        for (i, blk) in self.blocks.iter().enumerate() {
            if i == idx { break; }
            y += blk.height(content_w) + BLOCK_GAP;
        }
        y
    }

    /// Which block index contains scroll-relative y?
    fn block_at_y(&self, rel_y: f32, content_w: f32) -> usize {
        let mut y = 0.0_f32;
        for (i, blk) in self.blocks.iter().enumerate() {
            let h = blk.height(content_w) + BLOCK_GAP;
            if rel_y < y + h { return i; }
            y += h;
        }
        self.blocks.len().saturating_sub(1)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  NativeAppContent impl
// ─────────────────────────────────────────────────────────────────────────────

impl NativeAppContent for DocsApp {
    fn content_type_id(&self) -> WindowContentType {
        WindowContentType::Docs
    }

    fn describe_state(&self) -> AppState {
        self.build_app_state()
    }

    fn execute_action(&mut self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "append_block" => {
                let block_type = params["type"].as_str().unwrap_or("paragraph");
                let text = params["text"].as_str().unwrap_or("").to_string();
                let blk = match block_type {
                    "heading" => {
                        let level = params["level"].as_u64().unwrap_or(1).clamp(1, 3) as u8;
                        Block::Heading { level, text }
                    }
                    "code" => {
                        let lang = params["lang"].as_str().unwrap_or("").to_string();
                        Block::CodeBlock { lang, code: text }
                    }
                    "hline" => Block::HRule,
                    _ => Block::Paragraph(text),
                };
                self.blocks.push(blk);
                self.dirty = true;
                CapabilityResult {
                    success: true,
                    data: json!({ "block_count": self.blocks.len() }),
                    error: None,
                }
            }

            "write_block" => {
                let idx = params["index"].as_u64().unwrap_or(0) as usize;
                let text = params["text"].as_str().unwrap_or("");
                if idx >= self.blocks.len() {
                    return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::InvalidParam,
                            format!("Block index {} out of range (len={})", idx, self.blocks.len()))),
                    };
                }
                self.blocks[idx].set_text(text);
                self.dirty = true;
                CapabilityResult {
                    success: true,
                    data: json!({ "index": idx }),
                    error: None,
                }
            }

            "delete_block" => {
                let idx = params["index"].as_u64().unwrap_or(0) as usize;
                if idx >= self.blocks.len() {
                    return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::InvalidParam,
                            format!("Block index {} out of range (len={})", idx, self.blocks.len()))),
                    };
                }
                self.blocks.remove(idx);
                if self.selection >= self.blocks.len() && !self.blocks.is_empty() {
                    self.selection = self.blocks.len() - 1;
                }
                self.dirty = true;
                CapabilityResult {
                    success: true,
                    data: json!({ "block_count": self.blocks.len() }),
                    error: None,
                }
            }

            "read_blocks" => {
                let start = params["start"].as_u64().unwrap_or(0) as usize;
                let end = params["end"].as_u64()
                    .map(|e| e as usize)
                    .unwrap_or(self.blocks.len())
                    .min(self.blocks.len());
                let slice: Vec<Value> = self.blocks[start..end]
                    .iter()
                    .map(|b| b.to_json())
                    .collect();
                CapabilityResult {
                    success: true,
                    data: json!({ "blocks": slice }),
                    error: None,
                }
            }

            "set_title" => {
                let title = params["title"].as_str().unwrap_or("Untitled");
                self.title = title.to_string();
                self.dirty = true;
                CapabilityResult {
                    success: true,
                    data: json!({ "title": self.title }),
                    error: None,
                }
            }

            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownAction,
                    format!("docs: unknown action '{}'. Known: append_block, write_block, delete_block, read_blocks, set_title", action))),
            },
        }
    }

    fn render(
        &self,
        renderer: &mut Renderer,
        pixmap: &mut Pixmap,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        cursor_visible: bool,
    ) {
        let t = renderer.theme.clone();

        // ── Background ────────────────────────────────────────────────────
        renderer.fill_rect(pixmap, x, y, w, h, t.bg_window_chrome);

        // ── Title bar strip ───────────────────────────────────────────────
        renderer.fill_rect(pixmap, x, y, w, TITLE_BAR_H, [28, 30, 38, 255]);
        renderer.fill_rect(pixmap, x, y + TITLE_BAR_H - 1.0, w, 1.0, [255, 255, 255, 15]);

        // Title text (left-aligned, accent color)
        renderer.draw_text(pixmap, &self.title, x + CONTENT_PAD, y + 8.0,
            w * 0.6, 11.0, ACCENT);

        // Word count + block count (right-aligned, muted)
        let stats = format!("{} words · {} blocks", self.word_count(), self.blocks.len());
        let stats_w = stats.len() as f32 * 5.5;
        renderer.draw_text(pixmap, &stats, x + w - stats_w - CONTENT_PAD, y + 9.0,
            stats_w, 9.0, t.text_muted);

        // ── Scrollable content area ───────────────────────────────────────
        let content_x = x + CONTENT_PAD;
        let content_y = y + TITLE_BAR_H;
        let content_w = w - CONTENT_PAD * 2.0;
        let content_h = h - TITLE_BAR_H;

        // Clip rendering to content rect (simple: just offset by scroll)
        let mut block_y = content_y + 8.0 - self.scroll_offset;

        for (i, block) in self.blocks.iter().enumerate() {
            let blk_h = block.height(content_w);
            let blk_y = block_y;

            // Skip blocks entirely above or below viewport
            if blk_y + blk_h < content_y || blk_y > content_y + content_h {
                block_y += blk_h + BLOCK_GAP;
                continue;
            }

            let is_selected = i == self.selection;

            // Selected block: left accent border (3px)
            if is_selected {
                renderer.fill_rect(pixmap, x, blk_y, 3.0, blk_h, ACCENT);
            }

            match block {
                Block::Heading { level, text } => {
                    let (font_size, color) = match level {
                        1 => (20.0, ACCENT),
                        2 => (16.0, ACCENT_DIM),
                        _ => (13.0, ACCENT_FAINT),
                    };
                    let display = if is_selected {
                        if let Some(ref buf) = self.edit_buffer {
                            if cursor_visible { format!("{}|", buf) } else { buf.clone() }
                        } else {
                            text.clone()
                        }
                    } else {
                        text.clone()
                    };
                    renderer.draw_text(pixmap, &display, content_x, blk_y, content_w, font_size, color);
                    // Underline for H1
                    if *level == 1 {
                        renderer.fill_rect(pixmap, content_x, blk_y + font_size + 2.0,
                            content_w, 1.0, ACCENT);
                    }
                }

                Block::Paragraph(text) => {
                    let display = if is_selected {
                        if let Some(ref buf) = self.edit_buffer {
                            if cursor_visible { format!("{}|", buf) } else { buf.clone() }
                        } else {
                            text.clone()
                        }
                    } else {
                        text.clone()
                    };
                    renderer.draw_text(pixmap, &display, content_x, blk_y, content_w, 12.0, t.text_primary);
                }

                Block::CodeBlock { code, .. } => {
                    // Dark background
                    renderer.fill_rounded_rect(pixmap, content_x - 4.0, blk_y, content_w + 8.0, blk_h, 8.0, CODE_BG);
                    let display = if is_selected {
                        if let Some(ref buf) = self.edit_buffer {
                            if cursor_visible { format!("{}|", buf) } else { buf.clone() }
                        } else {
                            code.clone()
                        }
                    } else {
                        code.clone()
                    };
                    renderer.draw_text(pixmap, &display, content_x + 10.0, blk_y + 8.0,
                        content_w - 20.0, 12.0, CODE_FG);
                }

                Block::HRule => {
                    let mid_y = blk_y + 8.0;
                    renderer.fill_rect(pixmap, content_x, mid_y, content_w, 1.0, HLINE_CLR);
                }
            }

            block_y += blk_h + BLOCK_GAP;
        }

        // If no blocks, show placeholder
        if self.blocks.is_empty() {
            renderer.draw_text(pixmap, "Empty document. Start typing to add content.",
                content_x, content_y + 20.0, content_w, 11.0, t.text_muted);
        }
    }

    fn on_char(&mut self, c: char) {
        if self.blocks.is_empty() {
            self.blocks.push(Block::Paragraph(String::new()));
            self.selection = 0;
        }
        let current_text = if self.edit_buffer.is_none() {
            self.blocks.get(self.selection).map(|b| b.raw_text()).unwrap_or_default()
        } else {
            String::new()
        };
        let buf = self.edit_buffer.get_or_insert(current_text);
        buf.push(c);
    }

    fn on_backspace(&mut self) {
        if let Some(ref mut buf) = self.edit_buffer {
            buf.pop();
        } else if let Some(blk) = self.blocks.get_mut(self.selection) {
            let raw = blk.raw_text();
            if raw.is_empty() {
                if !self.blocks.is_empty() {
                    self.blocks.remove(self.selection);
                    if self.selection > 0 { self.selection -= 1; }
                }
            } else {
                let mut s = raw;
                s.pop();
                blk.set_text(&s);
            }
            self.dirty = true;
        }
    }

    fn on_key(&mut self, key: &str) -> Option<AppState> {
        match key {
            "Enter" => {
                self.commit_edit_buffer();
                if self.selection + 1 < self.blocks.len() {
                    self.selection += 1;
                } else {
                    // Append a new empty paragraph
                    self.blocks.push(Block::Paragraph(String::new()));
                    self.selection = self.blocks.len() - 1;
                }
                Some(self.build_app_state())
            }
            "Escape" => {
                self.edit_buffer = None;
                None
            }
            "ArrowUp" => {
                self.commit_edit_buffer();
                if self.selection > 0 { self.selection -= 1; }
                Some(self.build_app_state())
            }
            "ArrowDown" => {
                self.commit_edit_buffer();
                if self.selection + 1 < self.blocks.len() {
                    self.selection += 1;
                }
                Some(self.build_app_state())
            }
            _ => None,
        }
    }

    fn on_click(&mut self, _rel_x: f32, rel_y: f32) -> Option<AppState> {
        if self.blocks.is_empty() { return None; }
        // Adjust for title bar and scroll offset
        let adjusted_y = rel_y - TITLE_BAR_H - 8.0 + self.scroll_offset;
        if adjusted_y < 0.0 { return None; }
        // Approximate content_w — use a rough 688px (720 - 32 padding)
        let approx_content_w = 688.0;
        self.commit_edit_buffer();
        self.selection = self.block_at_y(adjusted_y, approx_content_w);
        Some(self.build_app_state())
    }

}

impl DocsApp {
    pub fn update(&mut self, dt: f32) {
        // Cursor blink at ~0.53s period
        self.cursor_timer += dt;
        if self.cursor_timer >= 0.53 {
            self.cursor_timer = 0.0;
            self.cursor_visible = !self.cursor_visible;
        }
    }
}

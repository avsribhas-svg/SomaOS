use crate::renderer::Renderer;
use base64::Engine;
use tiny_skia::Pixmap;

/// Browser panel — shows a headless screenshot with URL bar.
/// Receives updates via BrowserUpdate IPC messages from soma-agent.
pub struct BrowserPanel {
    pub url: String,
    pub title: String,
    /// Decoded + pre-scaled screenshot as a premultiplied-RGBA Pixmap.
    screenshot: Option<CachedScreenshot>,
    pub scroll_y: f32,
}

struct CachedScreenshot {
    pixmap: Pixmap,
    /// Source URL the screenshot was taken for (used to avoid redundant decodes).
    source_url: String,
}

impl BrowserPanel {
    pub fn new() -> Self {
        Self {
            url: String::new(),
            title: String::new(),
            screenshot: None,
            scroll_y: 0.0,
        }
    }

    /// Called when the agent sends a BrowserUpdate message.
    pub fn update(&mut self, url: String, title: String, screenshot_base64: Option<&str>) {
        let same_url = self.url == url;
        self.url = url.clone();
        self.title = if title.is_empty() {
            "Untitled".to_string()
        } else {
            title
        };

        if let Some(b64) = screenshot_base64 {
            // Only re-decode if the screenshot is for a different URL or we have none.
            let needs_decode = !same_url
                || self
                    .screenshot
                    .as_ref()
                    .map(|s| s.source_url != url)
                    .unwrap_or(true);

            if needs_decode {
                if let Some(pixmap) = decode_screenshot(b64) {
                    self.screenshot = Some(CachedScreenshot {
                        pixmap,
                        source_url: url,
                    });
                    // Reset scroll on new page.
                    self.scroll_y = 0.0;
                }
            }
        }
    }

    pub fn scroll(&mut self, delta: f32) {
        self.scroll_y = (self.scroll_y - delta).max(0.0);
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
        if w < 10.0 || h < 10.0 {
            return;
        }

        let t = renderer.theme.clone();

        // Panel background
        renderer.fill_rect(pixmap, x, y, w, h, [8, 8, 16, 255]);

        // ── URL bar ─────────────────────────────────────────────────────────
        let url_bar_h = 44.0;
        renderer.fill_rect(pixmap, x, y, w, url_bar_h, t.bg_sidebar);
        renderer.fill_rect(pixmap, x, y + url_bar_h - 1.0, w, 1.0, t.border);

        // Globe icon
        renderer.draw_text(pixmap, "o", x + 14.0, y + 15.0, 16.0, 12.0, t.text_muted);
        // URL text
        let url_display = if self.url.is_empty() {
            "about:blank"
        } else {
            &self.url
        };
        renderer.draw_text(
            pixmap,
            url_display,
            x + 34.0,
            y + 15.0,
            w - 48.0,
            11.0,
            t.text_secondary,
        );

        // ── Tab label (top-left badge) ───────────────────────────────────────
        renderer.draw_text(
            pixmap,
            if self.title.is_empty() {
                "Browser"
            } else {
                &self.title
            },
            x + 2.0,
            y + 3.0,
            w - 4.0,
            8.5,
            t.text_muted,
        );

        // ── Content area ─────────────────────────────────────────────────────
        let content_y = y + url_bar_h;
        let content_h = h - url_bar_h;

        if self.url.is_empty() {
            renderer.draw_text(
                pixmap,
                "No page loaded.\nAsk the agent to navigate to a URL\nor use browser.search.",
                x + w * 0.1,
                content_y + content_h * 0.38,
                w * 0.8,
                11.0,
                t.text_muted,
            );
            return;
        }

        if let Some(ref cached) = self.screenshot {
            let img_w = cached.pixmap.width() as f32;
            let img_h = cached.pixmap.height() as f32;
            // Scale to fit panel width (never up-scale beyond 1:1)
            let scale = (w / img_w).min(1.0);
            let dst_h = img_h * scale;

            // Clamp scroll
            let max_scroll = (dst_h - content_h).max(0.0);
            self.scroll_y = self.scroll_y.min(max_scroll);

            let draw_y = (content_y - self.scroll_y) as i32;

            // Clip to content area before drawing the pixmap.
            // tiny-skia draw_pixmap respects the clip mask if provided; we
            // handle vertical clipping manually by adjusting src offset.
            let src_y_offset = self.scroll_y / scale;
            let src_h_clip = (content_h / scale).min(img_h - src_y_offset);

            if src_h_clip > 0.0 {
                // Build a sub-pixmap view or just draw and let it clip by bounds.
                // tiny-skia will silently skip pixels outside the destination pixmap.
                let transform = tiny_skia::Transform::from_scale(scale, scale)
                    .post_translate(x, content_y - self.scroll_y);
                pixmap.draw_pixmap(
                    0,
                    -(src_y_offset as i32),
                    cached.pixmap.as_ref(),
                    &tiny_skia::PixmapPaint {
                        quality: tiny_skia::FilterQuality::Bilinear,
                        ..Default::default()
                    },
                    transform,
                    None,
                );
                let _ = (draw_y, src_h_clip); // suppress unused warnings
            }

            // Scrollbar
            if dst_h > content_h {
                let bar_h = ((content_h / dst_h) * content_h).max(20.0);
                let bar_y = content_y
                    + if max_scroll > 0.0 {
                        (self.scroll_y / max_scroll) * (content_h - bar_h)
                    } else {
                        0.0
                    };
                renderer.fill_rect(pixmap, x + w - 4.0, bar_y, 3.0, bar_h, [255, 255, 255, 35]);
            }
        } else {
            // Screenshot not yet available — show title or loading hint
            let label = if self.title.is_empty() || self.title == "Untitled" {
                "Loading…"
            } else {
                &self.title
            };
            renderer.draw_text(
                pixmap,
                label,
                x + 16.0,
                content_y + 20.0,
                w - 32.0,
                12.0,
                t.text_secondary,
            );
            renderer.draw_text(
                pixmap,
                "(Chromium screenshot pending)",
                x + 16.0,
                content_y + 42.0,
                w - 32.0,
                9.0,
                t.text_muted,
            );
        }
    }
}

/// Decode a base64 PNG screenshot into a premultiplied-RGBA Pixmap.
fn decode_screenshot(b64: &str) -> Option<Pixmap> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .ok()?;
    let img = image::load_from_memory(&bytes).ok()?.into_rgba8();
    let w = img.width();
    let h = img.height();
    let mut pm = Pixmap::new(w, h)?;
    let src = img.as_raw();
    for (i, px) in pm.pixels_mut().iter_mut().enumerate() {
        let b = i * 4;
        let r = src[b];
        let g = src[b + 1];
        let bl = src[b + 2];
        let a = src[b + 3];
        let alpha = a as f32 / 255.0;
        *px = tiny_skia::PremultipliedColorU8::from_rgba(
            (r as f32 * alpha) as u8,
            (g as f32 * alpha) as u8,
            (bl as f32 * alpha) as u8,
            a,
        )?;
    }
    Some(pm)
}

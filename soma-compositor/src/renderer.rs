use cosmic_text::{Attrs, Buffer, Color, FontSystem, Metrics, Shaping, SwashCache};
use tiny_skia::{Paint, PathBuilder, Pixmap, Rect, Transform};

/// Color palette for the compositor UI
#[derive(Clone)]
pub struct Theme {
    pub bg_primary: [u8; 4],       // Main background
    pub bg_sidebar: [u8; 4],       // Sidebar background
    pub bg_surface: [u8; 4],       // Cards / surfaces
    pub bg_input: [u8; 4],         // Input field background
    pub bg_hover: [u8; 4],         // Hover state
    pub text_primary: [u8; 4],     // Primary text
    pub text_secondary: [u8; 4],   // Secondary text
    pub text_muted: [u8; 4],       // Muted text
    pub accent: [u8; 4],           // Accent (indigo)
    pub success: [u8; 4],          // Green
    pub warning: [u8; 4],          // Yellow
    pub error: [u8; 4],            // Red
    pub border: [u8; 4],           // Subtle border
    pub terminal_bg: [u8; 4],      // Terminal background
    pub terminal_text: [u8; 4],    // Terminal text
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            bg_primary: [10, 10, 20, 240],
            bg_sidebar: [16, 16, 32, 245],
            bg_surface: [30, 30, 55, 160],
            bg_input: [40, 40, 65, 180],
            bg_hover: [255, 255, 255, 10],
            text_primary: [226, 232, 240, 255],
            text_secondary: [148, 163, 184, 255],
            text_muted: [100, 116, 139, 255],
            accent: [129, 140, 248, 255],
            success: [74, 222, 128, 255],
            warning: [251, 191, 36, 255],
            error: [248, 113, 113, 255],
            border: [255, 255, 255, 15],
            terminal_bg: [8, 8, 16, 250],
            terminal_text: [180, 210, 180, 255],
        }
    }
}

/// A simple 2D renderer wrapping tiny-skia
pub struct Renderer {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub theme: Theme,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
            theme: Theme::dark(),
        }
    }

    /// Fill a rectangle with an RGBA color
    pub fn fill_rect(&self, pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) {
        if let Some(rect) = Rect::from_xywh(x, y, w, h) {
            let mut paint = Paint::default();
            paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
            paint.anti_alias = true;
            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    /// Draw a rounded rectangle (approximated with filled rect for now)
    pub fn fill_rounded_rect(
        &self,
        pixmap: &mut Pixmap,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        color: [u8; 4],
    ) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
        paint.anti_alias = true;

        let r = radius.min(w / 2.0).min(h / 2.0);

        let mut pb = PathBuilder::new();
        // Top-left corner
        pb.move_to(x + r, y);
        // Top edge
        pb.line_to(x + w - r, y);
        // Top-right corner
        pb.quad_to(x + w, y, x + w, y + r);
        // Right edge
        pb.line_to(x + w, y + h - r);
        // Bottom-right corner
        pb.quad_to(x + w, y + h, x + w - r, y + h);
        // Bottom edge
        pb.line_to(x + r, y + h);
        // Bottom-left corner
        pb.quad_to(x, y + h, x, y + h - r);
        // Left edge
        pb.line_to(x, y + r);
        // Top-left corner
        pb.quad_to(x, y, x + r, y);
        pb.close();

        if let Some(path) = pb.finish() {
            pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }
    }

    /// Draw a 1px border around a rectangle
    pub fn stroke_rect(&self, pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) {
        // Top
        self.fill_rect(pixmap, x, y, w, 1.0, color);
        // Bottom
        self.fill_rect(pixmap, x, y + h - 1.0, w, 1.0, color);
        // Left
        self.fill_rect(pixmap, x, y, 1.0, h, color);
        // Right
        self.fill_rect(pixmap, x + w - 1.0, y, 1.0, h, color);
    }

    /// Render text at a position using cosmic-text, returning the height used
    pub fn draw_text(
        &mut self,
        pixmap: &mut Pixmap,
        text: &str,
        x: f32,
        y: f32,
        max_width: f32,
        font_size: f32,
        color: [u8; 4],
    ) -> f32 {
        let metrics = Metrics::new(font_size, font_size * 1.4);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        buffer.set_size(&mut self.font_system, Some(max_width), None);
        buffer.set_text(&mut self.font_system, text, Attrs::new(), Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let text_color = Color::rgba(color[0], color[1], color[2], color[3]);

        buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            text_color,
            |gx, gy, _w, _h, c| {
                let px = x as i32 + gx;
                let py = y as i32 + gy;
                if px >= 0 && py >= 0 && px < pixmap.width() as i32 && py < pixmap.height() as i32 {
                    let a = c.a();
                    if a > 0 {
                        let idx = (py as u32 * pixmap.width() + px as u32) as usize;
                        let pixels = pixmap.pixels_mut();
                        if idx < pixels.len() {
                            let src_r = c.r();
                            let src_g = c.g();
                            let src_b = c.b();
                            let dst = pixels[idx];
                            let alpha = a as f32 / 255.0;
                            let inv = 1.0 - alpha;
                            let r = (src_r as f32 * alpha + dst.red() as f32 * inv) as u8;
                            let g = (src_g as f32 * alpha + dst.green() as f32 * inv) as u8;
                            let b = (src_b as f32 * alpha + dst.blue() as f32 * inv) as u8;
                            let final_a = (a as f32 + dst.alpha() as f32 * inv) as u8;
                            pixels[idx] =
                                tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, final_a)
                                    .unwrap_or(dst);
                        }
                    }
                }
            },
        );

        // Return approximate height
        let line_count = buffer.lines.len().max(1);
        line_count as f32 * font_size * 1.4
    }
}

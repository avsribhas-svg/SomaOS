use cosmic_text::{Attrs, Buffer, Color, FontSystem, Metrics, Shaping, SwashCache};
use tiny_skia::{Paint, PathBuilder, Pixmap, Rect, Transform, LinearGradient, GradientStop, Point, SpreadMode, BlendMode, Shader};

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
    // Desktop environment chrome
    pub bg_desktop: [u8; 4],       // Desktop wallpaper base
    pub bg_window_chrome: [u8; 4], // Focused floating window bg
    pub bg_window_inactive: [u8; 4], // Unfocused window bg
    pub bg_titlebar: [u8; 4],      // Window title bar
    pub bg_dock: [u8; 4],          // Dock pill bg
    pub bg_menubar: [u8; 4],       // Menu bar bg
    pub close_btn: [u8; 4],        // Traffic-light close button
    pub close_btn_hover: [u8; 4],  // Close button hover
    pub agent_active: [u8; 4],     // Agent mode glow / indicator
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            // Modern premium glass dark theme
            bg_primary:     [18,  18,  20,  255], // Deep charcoal
            bg_sidebar:     [22,  22,  26,  235], // Translucent sidebar
            bg_surface:     [35,  35,  40,  240], // Glassy card surface
            bg_input:       [45,  45,  52,  240],
            bg_hover:       [255, 255, 255,  15],
            text_primary:   [240, 240, 245, 255], // Crisp white text
            text_secondary: [160, 160, 168, 255], // Sleeker secondary
            text_muted:     [110, 110, 115, 255],
            accent:         [120, 130, 255, 255], // Vibrant periwinkle accent
            success:        [64,  224, 180, 255], // Mint green
            warning:        [255, 210,  90, 255], // Crisp yellow
            error:          [255, 105, 105, 255], // Soft vibrant red
            border:         [255, 255, 255,  24], // Slightly higher contrast border
            terminal_bg:    [12,  12,  14,  245], // Deep terminal background
            terminal_text:  [220, 220, 225, 255],
            // Desktop chrome
            bg_desktop:         [10,  12,  20,  255], // Base color
            bg_window_chrome:   [28,  28,  34,  245], // Glassy window body
            bg_window_inactive: [22,  22,  28,  245],
            bg_titlebar:        [36,  36,  42,  250], // Slightly lighter titlebar
            bg_dock:            [18,  18,  24,  180], // High-glass dock
            bg_menubar:         [12,  12,  16,  190], // High-glass menu bar
            close_btn:          [255, 95,  86,  255], // macOS standard red
            close_btn_hover:    [255, 125, 115, 255],
            agent_active:       [120, 130, 255, 255], // Glowing accent border
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
        let x = x.round();
        let y = y.round();
        let w = w.abs().round();
        let h = h.abs().round();
        if w < 1.0 || h < 1.0 || w.is_nan() || h.is_nan() || !w.is_finite() || !h.is_finite() { 
            return; 
        }
        if let Some(rect) = Rect::from_xywh(x, y, w, h) {
            let mut paint = Paint::default();
            paint.blend_mode = BlendMode::SourceOver; // Use source over for translucency
            paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
            paint.anti_alias = false; // Snap strictly to pixels to avoid clip bugs
            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    /// Fill a rectangle with a linear gradient
    pub fn fill_gradient(
        &self,
        pixmap: &mut Pixmap,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        start_point: Point,
        end_point: Point,
        stops: Vec<GradientStop>,
    ) {
        if w <= 0.0 || h <= 0.0 { return; }
        if let Some(rect) = Rect::from_xywh(x, y, w, h) {
            let mut paint = Paint::default();
            if let Some(shader) = LinearGradient::new(
                start_point,
                end_point,
                stops,
                SpreadMode::Pad,
                Transform::identity()
            ) {
                paint.shader = shader;
                paint.anti_alias = true;
                paint.blend_mode = BlendMode::SourceOver;
                pixmap.fill_rect(rect, &paint, Transform::identity(), None);
            }
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
        let x = x.round();
        let y = y.round();
        let w = w.abs().round();
        let h = h.abs().round();
        
        let mut paint = Paint::default();
        paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
        paint.anti_alias = true; // Essential for rounded corners

        // Guard against degenerate dimensions that crash tiny-skia
        if w < 1.0 || h < 1.0 || w.is_nan() || h.is_nan() || !w.is_finite() || !h.is_finite() {
            return;
        }

        // Strictly clamp radius to half width/height to avoid degenerate paths
        let r = radius.abs().clamp(0.0, (w / 2.0).min(h / 2.0));

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
        let x = x.round();
        let y = y.round();
        let w = w.abs().round();
        let h = h.abs().round();
        if w < 1.0 || h < 1.0 || w.is_nan() || h.is_nan() || !w.is_finite() || !h.is_finite() { 
            return; 
        }
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
        if max_width <= 0.0 || font_size <= 0.0 || max_width.is_nan() || !max_width.is_finite() { 
            return 0.0; 
        }
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

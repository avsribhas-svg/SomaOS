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
            // VS Code / IDE agent-style dark theme
            bg_primary:     [30,  30,  30,  255], // #1E1E1E editor bg
            bg_sidebar:     [37,  37,  38,  255], // #252526 panel bg
            bg_surface:     [45,  45,  48,  240], // #2D2D30 card surface
            bg_input:       [58,  58,  58,  235], // #3A3A3A input bg
            bg_hover:       [255, 255, 255,  10],
            text_primary:   [212, 212, 212, 255], // #D4D4D4 VS Code text
            text_secondary: [150, 150, 150, 255], // #969696 secondary
            text_muted:     [96,  96,  96,  255], // #606060 muted
            accent:         [0,   122, 204, 255], // #007ACC VS Code blue
            success:        [78,  201, 176, 255], // #4EC9B0 teal/green
            warning:        [220, 220, 100, 255], // #DCDC64 yellow
            error:          [244, 135, 113, 255], // #F48771 error
            border:         [255, 255, 255,  18], // subtle separator
            terminal_bg:    [22,  22,  22,  255],
            terminal_text:  [204, 204, 204, 255],
            // Desktop chrome
            bg_desktop:         [18,  22,  30,  255],
            bg_window_chrome:   [45,  47,  52,  255],
            bg_window_inactive: [38,  40,  44,  255],
            bg_titlebar:        [40,  42,  48,  255],
            bg_dock:            [30,  32,  40,  220],
            bg_menubar:         [20,  20,  26,  230],
            close_btn:          [200, 70,  60,  255],
            close_btn_hover:    [255, 95,  86,  255],
            agent_active:       [0,   180, 255, 255],
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

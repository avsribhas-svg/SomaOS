/// Abstract display + input backend.
///
/// Two implementations:
///   - `winit` (feature `winit-backend`)  — for macOS / X11 dev
///   - `drm`  (feature `drm-backend`)     — bare-metal Linux production
pub mod event {
    /// Unified input event, backend-agnostic.
    #[derive(Debug, Clone)]
    pub enum InputEvent {
        KeyPress { code: KeyCode, text: Option<char> },
        KeyRelease { code: KeyCode },
        MouseMove { x: f32, y: f32 },
        MouseButton { button: MouseBtn, pressed: bool },
        Scroll { delta_y: f32 },
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum KeyCode {
        Enter,
        Backspace,
        Escape,
        Tab,
        F1,
        F2,
        F3,
        F4,
        F5,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        Space,
        Char(char),
        Ctrl(char),
        Unknown,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MouseBtn {
        Left,
        Right,
        Middle,
    }
}

/// A frame of XRGB8888 pixels ready to send to the display.
pub type FrameBuf = Vec<u32>;

#[cfg(feature = "drm-backend")]
pub mod drm;

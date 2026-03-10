/// Input abstraction for SomaOS.
///
/// On Linux bare-metal: reads from /dev/input/event* via evdev.
/// On macOS dev: input comes through winit's event loop (no separate handler).

#[cfg(feature = "drm-backend")]
pub mod evdev_input;

#[cfg(feature = "drm-backend")]
pub use evdev_input::EvdevInput;

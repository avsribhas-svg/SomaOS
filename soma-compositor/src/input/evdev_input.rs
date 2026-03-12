/// evdev-based keyboard and mouse input for bare-metal Linux.
///
/// Scans /dev/input/event* for keyboard and pointer devices, reads
/// events non-blocking, and converts them to our backend::event types.
use crate::backend::event::{InputEvent, KeyCode, MouseBtn};
use evdev::{Device, InputEventKind, Key, RelativeAxisType};
use std::path::PathBuf;

pub struct EvdevInput {
    keyboards: Vec<Device>,
    pointers: Vec<Device>,
    /// Accumulated absolute mouse position (starts at centre of screen)
    pub mouse_x: f32,
    pub mouse_y: f32,
    screen_w: f32,
    screen_h: f32,
    shift_held: bool,
    ctrl_held: bool,
}

impl EvdevInput {
    /// Open all keyboard and pointer devices found under /dev/input/.
    pub fn open(screen_w: u32, screen_h: u32) -> Self {
        let mut keyboards = Vec::new();
        let mut pointers = Vec::new();

        if let Ok(rd) = std::fs::read_dir("/dev/input") {
            for entry in rd.flatten() {
                let path: PathBuf = entry.path();
                if path.to_string_lossy().contains("event") {
                    if let Ok(mut dev) = Device::open(&path) {
                        let _ = dev.grab(); // exclusive grab
                        let supported = dev.supported_keys();
                        let has_keys = supported
                            .as_ref()
                            .map(|k| k.contains(Key::KEY_A))
                            .unwrap_or(false);
                        let has_rel = dev
                            .supported_relative_axes()
                            .map(|r| r.contains(RelativeAxisType::REL_X))
                            .unwrap_or(false);

                        if has_keys {
                            log::info!("evdev: keyboard at {:?}", path);
                            keyboards.push(dev);
                        } else if has_rel {
                            log::info!("evdev: pointer at {:?}", path);
                            pointers.push(dev);
                        }
                    }
                }
            }
        }

        Self {
            keyboards,
            pointers,
            mouse_x: screen_w as f32 / 2.0,
            mouse_y: screen_h as f32 / 2.0,
            screen_w: screen_w as f32,
            screen_h: screen_h as f32,
            shift_held: false,
            ctrl_held: false,
        }
    }

    /// Drain all pending events from all devices; returns them as InputEvents.
    pub fn poll(&mut self) -> Vec<InputEvent> {
        let mut out = Vec::new();

        // Keyboards
        for dev in &mut self.keyboards {
            while let Ok(evs) = dev.fetch_events() {
                for ev in evs {
                    match ev.kind() {
                        InputEventKind::Key(key) => {
                            let pressed = ev.value() != 0; // 0=up 1=down 2=repeat
                            let code = evdev_key_to_code(key, self.shift_held, self.ctrl_held);

                            // Track modifier state
                            match key {
                                Key::KEY_LEFTSHIFT | Key::KEY_RIGHTSHIFT => {
                                    self.shift_held = pressed;
                                }
                                Key::KEY_LEFTCTRL | Key::KEY_RIGHTCTRL => {
                                    self.ctrl_held = pressed;
                                }
                                _ => {}
                            }

                            if pressed {
                                out.push(InputEvent::KeyPress {
                                    code: code.clone(),
                                    text: key_code_to_char(&code),
                                });
                            } else {
                                out.push(InputEvent::KeyRelease { code });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Pointers
        for dev in &mut self.pointers {
            while let Ok(evs) = dev.fetch_events() {
                for ev in evs {
                    match ev.kind() {
                        InputEventKind::RelAxis(axis) => match axis {
                            RelativeAxisType::REL_X => {
                                self.mouse_x = (self.mouse_x + ev.value() as f32)
                                    .clamp(0.0, self.screen_w - 1.0);
                                out.push(InputEvent::MouseMove {
                                    x: self.mouse_x,
                                    y: self.mouse_y,
                                });
                            }
                            RelativeAxisType::REL_Y => {
                                self.mouse_y = (self.mouse_y + ev.value() as f32)
                                    .clamp(0.0, self.screen_h - 1.0);
                                out.push(InputEvent::MouseMove {
                                    x: self.mouse_x,
                                    y: self.mouse_y,
                                });
                            }
                            RelativeAxisType::REL_WHEEL => {
                                out.push(InputEvent::Scroll {
                                    delta_y: ev.value() as f32 * -40.0,
                                });
                            }
                            _ => {}
                        },
                        InputEventKind::Key(key) => {
                            let pressed = ev.value() != 0;
                            let button = match key {
                                Key::BTN_LEFT => Some(MouseBtn::Left),
                                Key::BTN_RIGHT => Some(MouseBtn::Right),
                                Key::BTN_MIDDLE => Some(MouseBtn::Middle),
                                _ => None,
                            };
                            if let Some(b) = button {
                                out.push(InputEvent::MouseButton { button: b, pressed });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        out
    }
}

fn evdev_key_to_code(key: Key, shift: bool, ctrl: bool) -> KeyCode {
    match key {
        Key::KEY_ENTER | Key::KEY_KPENTER => KeyCode::Enter,
        Key::KEY_BACKSPACE => KeyCode::Backspace,
        Key::KEY_ESC => KeyCode::Escape,
        Key::KEY_TAB => KeyCode::Tab,
        Key::KEY_F1 => KeyCode::F1,
        Key::KEY_F2 => KeyCode::F2,
        Key::KEY_F3 => KeyCode::F3,
        Key::KEY_F4 => KeyCode::F4,
        Key::KEY_F5 => KeyCode::F5,
        Key::KEY_UP => KeyCode::ArrowUp,
        Key::KEY_DOWN => KeyCode::ArrowDown,
        Key::KEY_LEFT => KeyCode::ArrowLeft,
        Key::KEY_RIGHT => KeyCode::ArrowRight,
        Key::KEY_SPACE => KeyCode::Space,
        _ => {
            if let Some(ch) = evdev_key_char(key, shift) {
                if ctrl {
                    KeyCode::Ctrl(ch.to_ascii_lowercase())
                } else {
                    KeyCode::Char(ch)
                }
            } else {
                KeyCode::Unknown
            }
        }
    }
}

fn key_code_to_char(code: &KeyCode) -> Option<char> {
    match code {
        KeyCode::Char(c) => Some(*c),
        KeyCode::Space => Some(' '),
        _ => None,
    }
}

/// Map evdev Key to a printable character, respecting shift.
fn evdev_key_char(key: Key, shift: bool) -> Option<char> {
    let ch = match key {
        Key::KEY_A => 'a', Key::KEY_B => 'b', Key::KEY_C => 'c',
        Key::KEY_D => 'd', Key::KEY_E => 'e', Key::KEY_F => 'f',
        Key::KEY_G => 'g', Key::KEY_H => 'h', Key::KEY_I => 'i',
        Key::KEY_J => 'j', Key::KEY_K => 'k', Key::KEY_L => 'l',
        Key::KEY_M => 'm', Key::KEY_N => 'n', Key::KEY_O => 'o',
        Key::KEY_P => 'p', Key::KEY_Q => 'q', Key::KEY_R => 'r',
        Key::KEY_S => 's', Key::KEY_T => 't', Key::KEY_U => 'u',
        Key::KEY_V => 'v', Key::KEY_W => 'w', Key::KEY_X => 'x',
        Key::KEY_Y => 'y', Key::KEY_Z => 'z',
        Key::KEY_1 => if shift { '!' } else { '1' },
        Key::KEY_2 => if shift { '@' } else { '2' },
        Key::KEY_3 => if shift { '#' } else { '3' },
        Key::KEY_4 => if shift { '$' } else { '4' },
        Key::KEY_5 => if shift { '%' } else { '5' },
        Key::KEY_6 => if shift { '^' } else { '6' },
        Key::KEY_7 => if shift { '&' } else { '7' },
        Key::KEY_8 => if shift { '*' } else { '8' },
        Key::KEY_9 => if shift { '(' } else { '9' },
        Key::KEY_0 => if shift { ')' } else { '0' },
        Key::KEY_MINUS => if shift { '_' } else { '-' },
        Key::KEY_EQUAL => if shift { '+' } else { '=' },
        Key::KEY_LEFTBRACE => if shift { '{' } else { '[' },
        Key::KEY_RIGHTBRACE => if shift { '}' } else { ']' },
        Key::KEY_SEMICOLON => if shift { ':' } else { ';' },
        Key::KEY_APOSTROPHE => if shift { '"' } else { '\'' },
        Key::KEY_COMMA => if shift { '<' } else { ',' },
        Key::KEY_DOT => if shift { '>' } else { '.' },
        Key::KEY_SLASH => if shift { '?' } else { '/' },
        Key::KEY_BACKSLASH => if shift { '|' } else { '\\' },
        Key::KEY_GRAVE => if shift { '~' } else { '`' },
        _ => return None,
    };
    Some(if shift && ch.is_ascii_alphabetic() {
        ch.to_ascii_uppercase()
    } else {
        ch
    })
}

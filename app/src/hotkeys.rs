//! System-wide global hotkey for PTT / MOX.
//!
//! Backed by the `global-hotkey` crate (Carbon on macOS, Win32 on Windows, X11
//! on Linux). The shortcut is held to transmit: pressing keys the radio and
//! releasing unkeys it. A background thread consumes the platform event channel,
//! updates an atomic pressed flag and wakes the UI so the change is applied even
//! while another application is focused.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};

/// Owns the platform hotkey registration and tracks its pressed state.
pub struct HotkeyManager {
    manager: Option<GlobalHotKeyManager>,
    hotkey: Option<HotKey>,
    /// Id of the active hotkey, shared with the listener thread.
    id: Arc<Mutex<Option<u32>>>,
    /// Whether the active hotkey is currently held down.
    pressed: Arc<AtomicBool>,
    error: Option<String>,
}

impl HotkeyManager {
    /// Create the manager and start the listener thread. Must be called on the
    /// main thread, which is where the platform event loop runs.
    pub fn new(ctx: egui::Context) -> Self {
        let pressed = Arc::new(AtomicBool::new(false));
        let id = Arc::new(Mutex::new(None));

        let listener_pressed = Arc::clone(&pressed);
        let listener_id = Arc::clone(&id);
        std::thread::spawn(move || {
            while let Ok(event) = GlobalHotKeyEvent::receiver().recv() {
                if *listener_id.lock().unwrap() == Some(event.id) {
                    listener_pressed.store(event.state == HotKeyState::Pressed, Ordering::Relaxed);
                    ctx.request_repaint();
                }
            }
        });

        match GlobalHotKeyManager::new() {
            Ok(manager) => Self {
                manager: Some(manager),
                hotkey: None,
                id,
                pressed,
                error: None,
            },
            Err(err) => Self {
                manager: None,
                hotkey: None,
                id,
                pressed,
                error: Some(err.to_string()),
            },
        }
    }

    /// Replace the registered hotkey. `spec` is the canonical string produced by
    /// [`HotKey::to_string`], or `None` to clear it.
    pub fn apply(&mut self, spec: Option<&str>) {
        self.pressed.store(false, Ordering::Relaxed);
        *self.id.lock().unwrap() = None;
        if let (Some(manager), Some(old)) = (&self.manager, self.hotkey.take()) {
            let _ = manager.unregister(old);
        }
        self.error = None;

        let Some(spec) = spec else {
            return;
        };
        let hotkey: HotKey = match spec.parse() {
            Ok(hotkey) => hotkey,
            Err(err) => {
                self.error = Some(err.to_string());
                return;
            }
        };
        let Some(manager) = &self.manager else {
            return;
        };
        match manager.register(hotkey) {
            Ok(()) => {
                *self.id.lock().unwrap() = Some(hotkey.id());
                self.hotkey = Some(hotkey);
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    /// Whether the PTT hotkey is currently held down.
    pub fn is_pressed(&self) -> bool {
        self.pressed.load(Ordering::Relaxed)
    }

    /// Last registration error, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// Build a canonical hotkey string from an egui key press, or `None` if the key
/// is unusable (an unknown code, or a printable key with no modifier, which
/// would otherwise be stolen from every application).
pub fn capture(key: egui::Key, modifiers: &egui::Modifiers) -> Option<String> {
    let mut mods = Modifiers::empty();
    if modifiers.shift {
        mods |= Modifiers::SHIFT;
    }
    if modifiers.alt {
        mods |= Modifiers::ALT;
    }
    if modifiers.ctrl {
        mods |= Modifiers::CONTROL;
    }
    if modifiers.command {
        mods |= Modifiers::SUPER;
    }

    let code = code_for(key)?;
    if mods.is_empty() && !is_function_key(code) {
        return None;
    }
    Some(HotKey::new(Some(mods), code).to_string())
}

/// Human-readable form of a canonical hotkey spec (`"alt+KeyP"` -> `"Alt+P"`).
pub fn display(spec: &str) -> String {
    let mut parts = Vec::new();
    for token in spec.split('+') {
        let pretty = match token {
            "shift" => "Shift".to_string(),
            "control" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            "super" => "Cmd".to_string(),
            _ => token
                .strip_prefix("Key")
                .or_else(|| token.strip_prefix("Digit"))
                .unwrap_or(token)
                .to_string(),
        };
        parts.push(pretty);
    }
    parts.join(" + ")
}

fn is_function_key(code: Code) -> bool {
    matches!(
        code,
        Code::F1
            | Code::F2
            | Code::F3
            | Code::F4
            | Code::F5
            | Code::F6
            | Code::F7
            | Code::F8
            | Code::F9
            | Code::F10
            | Code::F11
            | Code::F12
            | Code::F13
            | Code::F14
            | Code::F15
            | Code::F16
            | Code::F17
            | Code::F18
            | Code::F19
            | Code::F20
            | Code::F21
            | Code::F22
            | Code::F23
            | Code::F24
    )
}

/// Map an egui key to a keyboard-types [`Code`]. Keys that are reported only as
/// a shifted glyph (or that have no physical equivalent) are ignored.
fn code_for(key: egui::Key) -> Option<Code> {
    use egui::Key;
    Some(match key {
        Key::A => Code::KeyA,
        Key::B => Code::KeyB,
        Key::C => Code::KeyC,
        Key::D => Code::KeyD,
        Key::E => Code::KeyE,
        Key::F => Code::KeyF,
        Key::G => Code::KeyG,
        Key::H => Code::KeyH,
        Key::I => Code::KeyI,
        Key::J => Code::KeyJ,
        Key::K => Code::KeyK,
        Key::L => Code::KeyL,
        Key::M => Code::KeyM,
        Key::N => Code::KeyN,
        Key::O => Code::KeyO,
        Key::P => Code::KeyP,
        Key::Q => Code::KeyQ,
        Key::R => Code::KeyR,
        Key::S => Code::KeyS,
        Key::T => Code::KeyT,
        Key::U => Code::KeyU,
        Key::V => Code::KeyV,
        Key::W => Code::KeyW,
        Key::X => Code::KeyX,
        Key::Y => Code::KeyY,
        Key::Z => Code::KeyZ,
        Key::Num0 => Code::Digit0,
        Key::Num1 => Code::Digit1,
        Key::Num2 => Code::Digit2,
        Key::Num3 => Code::Digit3,
        Key::Num4 => Code::Digit4,
        Key::Num5 => Code::Digit5,
        Key::Num6 => Code::Digit6,
        Key::Num7 => Code::Digit7,
        Key::Num8 => Code::Digit8,
        Key::Num9 => Code::Digit9,
        Key::F1 => Code::F1,
        Key::F2 => Code::F2,
        Key::F3 => Code::F3,
        Key::F4 => Code::F4,
        Key::F5 => Code::F5,
        Key::F6 => Code::F6,
        Key::F7 => Code::F7,
        Key::F8 => Code::F8,
        Key::F9 => Code::F9,
        Key::F10 => Code::F10,
        Key::F11 => Code::F11,
        Key::F12 => Code::F12,
        Key::F13 => Code::F13,
        Key::F14 => Code::F14,
        Key::F15 => Code::F15,
        Key::F16 => Code::F16,
        Key::F17 => Code::F17,
        Key::F18 => Code::F18,
        Key::F19 => Code::F19,
        Key::F20 => Code::F20,
        Key::F21 => Code::F21,
        Key::F22 => Code::F22,
        Key::F23 => Code::F23,
        Key::F24 => Code::F24,
        Key::Space => Code::Space,
        Key::Enter => Code::Enter,
        Key::Tab => Code::Tab,
        Key::Escape => Code::Escape,
        Key::Backspace => Code::Backspace,
        Key::Delete => Code::Delete,
        Key::Insert => Code::Insert,
        Key::Home => Code::Home,
        Key::End => Code::End,
        Key::PageUp => Code::PageUp,
        Key::PageDown => Code::PageDown,
        Key::ArrowUp => Code::ArrowUp,
        Key::ArrowDown => Code::ArrowDown,
        Key::ArrowLeft => Code::ArrowLeft,
        Key::ArrowRight => Code::ArrowRight,
        Key::Minus => Code::Minus,
        Key::Equals => Code::Equal,
        Key::Comma => Code::Comma,
        Key::Period => Code::Period,
        Key::Slash => Code::Slash,
        Key::Semicolon => Code::Semicolon,
        Key::Quote => Code::Quote,
        Key::Backslash => Code::Backslash,
        Key::Backtick => Code::Backquote,
        Key::OpenBracket => Code::BracketLeft,
        Key::CloseBracket => Code::BracketRight,
        Key::IntlBackslash => Code::IntlBackslash,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_chord_produces_a_spec() {
        let mods = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        assert_eq!(capture(egui::Key::P, &mods).as_deref(), Some("alt+KeyP"));
    }

    #[test]
    fn bare_printable_key_is_rejected() {
        assert!(capture(egui::Key::P, &egui::Modifiers::default()).is_none());
    }

    #[test]
    fn bare_function_key_is_allowed() {
        assert_eq!(
            capture(egui::Key::F13, &egui::Modifiers::default()).as_deref(),
            Some("F13")
        );
    }

    #[test]
    fn command_maps_to_super() {
        let mods = egui::Modifiers {
            command: true,
            ..Default::default()
        };
        assert_eq!(
            capture(egui::Key::Space, &mods).as_deref(),
            Some("super+Space")
        );
    }

    #[test]
    fn unmapped_key_is_rejected() {
        assert!(capture(egui::Key::Cut, &egui::Modifiers::default()).is_none());
    }
}

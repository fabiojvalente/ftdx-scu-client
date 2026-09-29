//! Keyboard shortcut keymap, ported from Yaesu Web Control's
//! `wwwroot/js/ui/keyboard-shortcuts.js`.
//!
//! This module is intentionally data-only: it resolves an input [`Chord`] into
//! an [`Action`] and carries the [`HELP_ROWS`] table that the help overlay and
//! the dispatcher share, so the documented keys cannot drift from what the
//! keyboard actually does. Applying an [`Action`] lives on
//! [`ScuApp`](crate::app::ScuApp).

use eframe::egui::Key;
use scu_cat::Mode;

/// A single key press, normalised from an [`egui::Event::Key`].
///
/// `physical` carries the position on the keyboard, used for the Alt chords
/// that macOS Option remaps to non-Latin glyphs (Option+Z -> `Ω`), and for the
/// `@`, `<`, `>` glyphs that have no logical [`Key`] name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub key: Key,
    pub physical: Option<Key>,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub command: bool,
    pub repeat: bool,
}

/// A radio / UI action triggered by a shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    TuneUp,
    TuneDown,
    TuneUp10,
    TuneDown10,
    TuneUp100,
    TuneDown100,
    FineTuneUp,
    FineTuneDown,
    FineTuneUp50,
    FineTuneDown50,
    SwitchVfo,
    CopyAToB,
    BandDown,
    BandUp,
    FocusFrequency,
    Mode(Mode),
    IfWidthNarrower,
    IfWidthWider,
    IfWidthDefault,
    IfShiftUp,
    IfShiftDown,
    IfShiftUpFast,
    IfShiftDownFast,
    ScopeZoomIn,
    ScopeZoomOut,
    ScopeZoomInExtreme,
    ScopeZoomOutExtreme,
    ScopeWider,
    ScopeNarrower,
    ToggleVfoB,
    ToggleFullscreen,
    VolumeDown,
    VolumeUp,
    ToggleMute,
    ToggleHelp,
    /// A key that exists in the Yaesu keymap but has no matching feature here.
    Unavailable(&'static str),
}

impl Action {
    /// Whether holding the key may repeat the action (tuning, gain, ...).
    pub fn is_repeatable(self) -> bool {
        matches!(
            self,
            Action::TuneUp
                | Action::TuneDown
                | Action::TuneUp10
                | Action::TuneDown10
                | Action::TuneUp100
                | Action::TuneDown100
                | Action::FineTuneUp
                | Action::FineTuneDown
                | Action::FineTuneUp50
                | Action::FineTuneDown50
                | Action::IfShiftUp
                | Action::IfShiftDown
                | Action::IfShiftUpFast
                | Action::IfShiftDownFast
                | Action::VolumeDown
                | Action::VolumeUp
        )
    }
}

/// One row of the keyboard-shortcuts help table.
pub struct HelpRow {
    pub group: &'static str,
    pub keys: &'static str,
    pub action: &'static str,
    /// Extra note, e.g. that the key is bound but the feature is unavailable.
    pub note: Option<&'static str>,
}

const UNAVAILABLE: &str = "not available in this build";

/// The full keymap, grouped for the help overlay.
pub const HELP_ROWS: &[HelpRow] = &[
    // Frequency & VFO
    HelpRow {
        group: "Frequency & VFO",
        keys: "j / \u{2190}",
        action: "Tune down one step",
        note: None,
    },
    HelpRow {
        group: "Frequency & VFO",
        keys: "i / \u{2192}",
        action: "Tune up one step",
        note: None,
    },
    HelpRow {
        group: "Frequency & VFO",
        keys: "Shift + j/i/\u{2190}/\u{2192}",
        action: "Step \u{00d7}10",
        note: None,
    },
    HelpRow {
        group: "Frequency & VFO",
        keys: "Alt + j/i/\u{2190}/\u{2192}",
        action: "Step \u{00d7}100",
        note: None,
    },
    HelpRow {
        group: "Frequency & VFO",
        keys: "[ / ]",
        action: "Fine tune 1 Hz",
        note: None,
    },
    HelpRow {
        group: "Frequency & VFO",
        keys: "Shift + [ / ]",
        action: "Fine tune 50 Hz",
        note: None,
    },
    HelpRow {
        group: "Frequency & VFO",
        keys: "g",
        action: "Edit the active VFO frequency",
        note: None,
    },
    HelpRow {
        group: "Frequency & VFO",
        keys: "n",
        action: "Switch active VFO A \u{2194} B",
        note: None,
    },
    HelpRow {
        group: "Frequency & VFO",
        keys: "N",
        action: "Copy VFO A \u{2192} B",
        note: None,
    },
    HelpRow {
        group: "Frequency & VFO",
        keys: "b / B",
        action: "Previous / next amateur band",
        note: None,
    },
    // Mode
    HelpRow {
        group: "Mode",
        keys: "l",
        action: "LSB",
        note: None,
    },
    HelpRow {
        group: "Mode",
        keys: "u",
        action: "USB",
        note: None,
    },
    HelpRow {
        group: "Mode",
        keys: "c",
        action: "CW-U",
        note: None,
    },
    HelpRow {
        group: "Mode",
        keys: "C or Alt+c",
        action: "CW-L",
        note: None,
    },
    HelpRow {
        group: "Mode",
        keys: "a",
        action: "AM",
        note: None,
    },
    HelpRow {
        group: "Mode",
        keys: "A or Alt+a",
        action: "AM-N",
        note: None,
    },
    HelpRow {
        group: "Mode",
        keys: "q",
        action: "FM",
        note: None,
    },
    HelpRow {
        group: "Mode",
        keys: "Q or Alt+q",
        action: "FM-N",
        note: None,
    },
    HelpRow {
        group: "Mode",
        keys: "d",
        action: "DATA-U",
        note: None,
    },
    HelpRow {
        group: "Mode",
        keys: "Alt+d",
        action: "DATA-L",
        note: None,
    },
    // Passband
    HelpRow {
        group: "Passband",
        keys: "p",
        action: "Narrow IF width",
        note: None,
    },
    HelpRow {
        group: "Passband",
        keys: "P",
        action: "Widen IF width",
        note: None,
    },
    HelpRow {
        group: "Passband",
        keys: "/",
        action: "Default IF width",
        note: None,
    },
    HelpRow {
        group: "Passband",
        keys: "\u{2191} / \u{2193}",
        action: "IF shift \u{00b1}20 Hz",
        note: None,
    },
    HelpRow {
        group: "Passband",
        keys: "Shift + \u{2191}/\u{2193}",
        action: "IF shift \u{00b1}100 Hz",
        note: None,
    },
    // Panels
    HelpRow {
        group: "Panels",
        keys: "x",
        action: "Show / hide VFO B panel",
        note: None,
    },
    HelpRow {
        group: "Panels",
        keys: "f / F",
        action: "Toggle full-screen",
        note: None,
    },
    // Audio
    HelpRow {
        group: "Audio",
        keys: "v / V",
        action: "Volume \u{2212}/+ one step",
        note: None,
    },
    HelpRow {
        group: "Audio",
        keys: "Shift + M",
        action: "Mute / unmute RX audio",
        note: None,
    },
    HelpRow {
        group: "Audio",
        keys: "r",
        action: "Start / stop remote audio",
        note: Some(UNAVAILABLE),
    },
    // Display / scope
    HelpRow {
        group: "Display",
        keys: "z / Z",
        action: "Scope narrower / wider",
        note: None,
    },
    HelpRow {
        group: "Display",
        keys: "Alt + z / Alt + Z",
        action: "Scope narrowest / widest",
        note: None,
    },
    HelpRow {
        group: "Display",
        keys: "< / >",
        action: "Scope wider / narrower",
        note: None,
    },
    HelpRow {
        group: "Display",
        keys: "w / W",
        action: "Spectrum vertical range",
        note: Some(UNAVAILABLE),
    },
    HelpRow {
        group: "Display",
        keys: "s / S",
        action: "Scope hold / reset range",
        note: Some(UNAVAILABLE),
    },
    HelpRow {
        group: "Display",
        keys: "o / O",
        action: "Scope reference level",
        note: Some(UNAVAILABLE),
    },
    HelpRow {
        group: "Display",
        keys: "t",
        action: "Toggle shortcut display target",
        note: Some(UNAVAILABLE),
    },
    // DX / tools
    HelpRow {
        group: "Tools",
        keys: "D",
        action: "Toggle DX spots",
        note: Some(UNAVAILABLE),
    },
    HelpRow {
        group: "Tools",
        keys: "@",
        action: "Open DX watch",
        note: Some(UNAVAILABLE),
    },
    HelpRow {
        group: "Tools",
        keys: "R",
        action: "Show radio display",
        note: Some(UNAVAILABLE),
    },
    HelpRow {
        group: "Tools",
        keys: "m",
        action: "Memories panel",
        note: Some(UNAVAILABLE),
    },
    // Help
    HelpRow {
        group: "Help",
        keys: "? (Shift+/)",
        action: "Toggle this shortcuts dialog",
        note: None,
    },
    HelpRow {
        group: "Help",
        keys: "h",
        action: "Toggle this shortcuts dialog",
        note: None,
    },
    HelpRow {
        group: "Help",
        keys: "Esc",
        action: "Close help / exit full-screen",
        note: None,
    },
];

/// Does the press correspond to `key`, allowing for Alt-remapped logical keys?
fn is_key(c: &Chord, key: Key) -> bool {
    c.key == key || c.physical == Some(key)
}

/// No browser / app chord, and no Alt (Shift may be held).
fn bare(c: &Chord) -> bool {
    !c.alt && !c.ctrl && !c.command
}

/// Unmodified (or Shift-only) letter.
fn plain(c: &Chord) -> bool {
    bare(c) && !c.shift
}

/// Not a browser / app chord (Alt and Shift may be held).
fn no_chord(c: &Chord) -> bool {
    !c.ctrl && !c.command
}

/// Resolve a key press into an [`Action`].
///
/// `Escape` is handled by the caller (it closes the help overlay / leaves
/// full-screen) and returns `None` here.
pub fn resolve(c: &Chord) -> Option<Action> {
    // Never capture Ctrl/Cmd chords; browsers and the OS use them.
    if c.ctrl || c.command {
        return None;
    }

    // Help chord: `?` (Shift+/) or `h` (unshifted). Checked before the focus
    // guard in the caller so it can toggle the overlay while it is open.
    if !c.alt
        && (c.key == Key::Questionmark
            || (is_key(c, Key::Slash) && c.shift)
            || (is_key(c, Key::H) && !c.shift))
    {
        return Some(Action::ToggleHelp);
    }

    // Full-screen: f / F.
    if is_key(c, Key::F) && !c.alt {
        return Some(Action::ToggleFullscreen);
    }

    // Shift+M -> RX mute (before plain m = memories).
    if is_key(c, Key::M) && c.shift && !c.alt {
        return Some(Action::ToggleMute);
    }

    // Modes (before the generic letter handling below).
    if let Some(mode) = mode_shortcut(c) {
        return Some(Action::Mode(mode));
    }

    // DX spots (uppercase D, not a mode).
    if is_key(c, Key::D) && c.shift && !c.alt {
        return Some(Action::Unavailable(
            "DX spots are not available in this build",
        ));
    }
    // DX watch (@ = Shift+2 on most layouts).
    if is_key(c, Key::Num2) && c.shift && !c.alt {
        return Some(Action::Unavailable(
            "DX watch is not available in this build",
        ));
    }

    // Frequency entry.
    if is_key(c, Key::G) && plain(c) {
        return Some(Action::FocusFrequency);
    }

    // Memories (plain m; Shift+M handled above).
    if is_key(c, Key::M) && plain(c) {
        return Some(Action::Unavailable(
            "Memories are not available in this build",
        ));
    }

    // VFO switch / copy.
    if is_key(c, Key::N) && plain(c) {
        return Some(Action::SwitchVfo);
    }
    if is_key(c, Key::N) && c.shift && !c.alt {
        return Some(Action::CopyAToB);
    }

    // Band select.
    if is_key(c, Key::B) && bare(c) {
        return Some(if c.shift {
            Action::BandUp
        } else {
            Action::BandDown
        });
    }

    // Fine tune on the physical bracket keys.
    if is_key(c, Key::OpenBracket) && !c.alt {
        return Some(if c.shift {
            Action::FineTuneDown50
        } else {
            Action::FineTuneDown
        });
    }
    if is_key(c, Key::CloseBracket) && !c.alt {
        return Some(if c.shift {
            Action::FineTuneUp50
        } else {
            Action::FineTuneUp
        });
    }

    // Tuning: j / i and the left/right arrows.
    let tune_down = is_key(c, Key::J) || is_key(c, Key::ArrowLeft);
    let tune_up = is_key(c, Key::I) || is_key(c, Key::ArrowRight);
    if tune_down || tune_up {
        let up = tune_up;
        // Tuning with Alt+Shift on j/i is reserved by Yaesu for DX-spot hop.
        if (is_key(c, Key::J) || is_key(c, Key::I)) && c.alt && c.shift {
            return Some(Action::Unavailable("DX spot hopping requires DX spots"));
        }
        return Some(match (up, c.shift, c.alt) {
            (true, false, false) => Action::TuneUp,
            (false, false, false) => Action::TuneDown,
            (true, true, false) => Action::TuneUp10,
            (false, true, false) => Action::TuneDown10,
            (true, false, true) => Action::TuneUp100,
            (false, false, true) => Action::TuneDown100,
            // Alt wins over Shift, matching resolveTuneStepHz.
            (true, _, true) => Action::TuneUp100,
            (false, _, true) => Action::TuneDown100,
        });
    }

    // IF width.
    if is_key(c, Key::P) && !c.alt {
        return Some(if c.shift {
            Action::IfWidthWider
        } else {
            Action::IfWidthNarrower
        });
    }
    if is_key(c, Key::Slash) && !c.shift && !c.alt {
        return Some(Action::IfWidthDefault);
    }

    // IF shift via Up/Down.
    if is_key(c, Key::ArrowUp) && !c.alt {
        return Some(if c.shift {
            Action::IfShiftUpFast
        } else {
            Action::IfShiftUp
        });
    }
    if is_key(c, Key::ArrowDown) && !c.alt {
        return Some(if c.shift {
            Action::IfShiftDownFast
        } else {
            Action::IfShiftDown
        });
    }

    // Scope span.
    if is_key(c, Key::Z) {
        return Some(if c.shift {
            if c.alt {
                Action::ScopeZoomOutExtreme
            } else {
                Action::ScopeZoomOut
            }
        } else if c.alt {
            Action::ScopeZoomInExtreme
        } else {
            Action::ScopeZoomIn
        });
    }
    if (is_key(c, Key::Comma) && c.shift) || (is_key(c, Key::Period) && c.shift) {
        return Some(if is_key(c, Key::Comma) {
            Action::ScopeWider
        } else {
            Action::ScopeNarrower
        });
    }

    // Spectrum range / hold / reference level (not implemented).
    if is_key(c, Key::W) {
        return Some(Action::Unavailable(
            "Spectrum range is not available in this build",
        ));
    }
    if is_key(c, Key::S) {
        return Some(Action::Unavailable(
            "Scope hold is not available in this build",
        ));
    }
    if is_key(c, Key::O) {
        return Some(Action::Unavailable(
            "Scope reference level is not available in this build",
        ));
    }

    // Panel / tools.
    if is_key(c, Key::X) && plain(c) {
        return Some(Action::ToggleVfoB);
    }
    if is_key(c, Key::R) && c.shift && !c.alt {
        return Some(Action::Unavailable(
            "Radio display is not available in this build",
        ));
    }
    if is_key(c, Key::R) && plain(c) {
        return Some(Action::Unavailable(
            "Remote audio is not available in this build",
        ));
    }
    if is_key(c, Key::T) && plain(c) {
        return Some(Action::Unavailable(
            "Shortcut display target is not available",
        ));
    }

    // Volume.
    if is_key(c, Key::V) && bare(c) {
        return Some(if c.shift {
            Action::VolumeUp
        } else {
            Action::VolumeDown
        });
    }

    None
}

/// Map the Yaesu mode letters, honouring case and Alt.
fn mode_shortcut(c: &Chord) -> Option<Mode> {
    let key = if c.alt {
        c.physical.unwrap_or(c.key)
    } else {
        c.key
    };
    match key {
        Key::L if no_chord(c) => Some(Mode::Lsb),
        Key::U if no_chord(c) => Some(Mode::Usb),
        Key::C if no_chord(c) => Some(if c.shift || c.alt {
            Mode::CwL
        } else {
            Mode::CwU
        }),
        Key::A if no_chord(c) => Some(if c.shift || c.alt {
            Mode::AmN
        } else {
            Mode::Am
        }),
        Key::Q if no_chord(c) => Some(if c.shift || c.alt {
            Mode::FmN
        } else {
            Mode::Fm
        }),
        Key::D if no_chord(c) && !c.shift => Some(if c.alt { Mode::DataL } else { Mode::DataU }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(key: Key) -> Chord {
        Chord {
            key,
            physical: Some(key),
            shift: false,
            alt: false,
            ctrl: false,
            command: false,
            repeat: false,
        }
    }

    fn shifted(key: Key) -> Chord {
        Chord {
            shift: true,
            ..chord(key)
        }
    }

    fn alted(key: Key) -> Chord {
        Chord {
            alt: true,
            ..chord(key)
        }
    }

    #[test]
    fn modes_cover_case_and_alt() {
        assert_eq!(resolve(&chord(Key::L)), Some(Action::Mode(Mode::Lsb)));
        assert_eq!(resolve(&chord(Key::U)), Some(Action::Mode(Mode::Usb)));
        assert_eq!(resolve(&chord(Key::C)), Some(Action::Mode(Mode::CwU)));
        assert_eq!(resolve(&shifted(Key::C)), Some(Action::Mode(Mode::CwL)));
        assert_eq!(resolve(&alted(Key::C)), Some(Action::Mode(Mode::CwL)));
        assert_eq!(resolve(&chord(Key::A)), Some(Action::Mode(Mode::Am)));
        assert_eq!(resolve(&shifted(Key::A)), Some(Action::Mode(Mode::AmN)));
        assert_eq!(resolve(&chord(Key::Q)), Some(Action::Mode(Mode::Fm)));
        assert_eq!(resolve(&shifted(Key::Q)), Some(Action::Mode(Mode::FmN)));
        assert_eq!(resolve(&chord(Key::D)), Some(Action::Mode(Mode::DataU)));
        assert_eq!(resolve(&alted(Key::D)), Some(Action::Mode(Mode::DataL)));
    }

    #[test]
    fn tuning_modifiers_pick_the_step() {
        assert_eq!(resolve(&chord(Key::J)), Some(Action::TuneDown));
        assert_eq!(resolve(&chord(Key::I)), Some(Action::TuneUp));
        assert_eq!(resolve(&shifted(Key::I)), Some(Action::TuneUp10));
        assert_eq!(resolve(&alted(Key::I)), Some(Action::TuneUp100));
        assert_eq!(resolve(&chord(Key::ArrowLeft)), Some(Action::TuneDown));
        assert_eq!(resolve(&shifted(Key::ArrowRight)), Some(Action::TuneUp10));
        assert_eq!(resolve(&alted(Key::ArrowRight)), Some(Action::TuneUp100));
    }

    #[test]
    fn help_chords() {
        assert_eq!(resolve(&shifted(Key::Slash)), Some(Action::ToggleHelp));
        assert_eq!(resolve(&chord(Key::Questionmark)), Some(Action::ToggleHelp));
        assert_eq!(resolve(&chord(Key::H)), Some(Action::ToggleHelp));
        // Shift+H is not help.
        assert_eq!(resolve(&shifted(Key::H)), None);
        // Unshifted slash restores the default IF width.
        assert_eq!(resolve(&chord(Key::Slash)), Some(Action::IfWidthDefault));
    }

    #[test]
    fn bracket_and_span_keys() {
        assert_eq!(
            resolve(&chord(Key::OpenBracket)),
            Some(Action::FineTuneDown)
        );
        assert_eq!(resolve(&chord(Key::CloseBracket)), Some(Action::FineTuneUp));
        assert_eq!(
            resolve(&shifted(Key::CloseBracket)),
            Some(Action::FineTuneUp50)
        );
        assert_eq!(resolve(&chord(Key::Z)), Some(Action::ScopeZoomIn));
        assert_eq!(resolve(&shifted(Key::Z)), Some(Action::ScopeZoomOut));
        assert_eq!(resolve(&alted(Key::Z)), Some(Action::ScopeZoomInExtreme));
        assert_eq!(resolve(&shifted(Key::Comma)), Some(Action::ScopeWider));
        assert_eq!(resolve(&shifted(Key::Period)), Some(Action::ScopeNarrower));
    }

    #[test]
    fn panels_audio_and_stubs() {
        assert_eq!(resolve(&chord(Key::X)), Some(Action::ToggleVfoB));
        assert_eq!(resolve(&chord(Key::F)), Some(Action::ToggleFullscreen));
        assert_eq!(resolve(&chord(Key::V)), Some(Action::VolumeDown));
        assert_eq!(resolve(&shifted(Key::V)), Some(Action::VolumeUp));
        assert_eq!(resolve(&shifted(Key::M)), Some(Action::ToggleMute));
        assert!(matches!(
            resolve(&chord(Key::M)),
            Some(Action::Unavailable(_))
        ));
        assert!(matches!(
            resolve(&shifted(Key::D)),
            Some(Action::Unavailable(_))
        ));
    }

    #[test]
    fn vfo_and_band_keys() {
        assert_eq!(resolve(&chord(Key::N)), Some(Action::SwitchVfo));
        assert_eq!(resolve(&shifted(Key::N)), Some(Action::CopyAToB));
        assert_eq!(resolve(&chord(Key::B)), Some(Action::BandDown));
        assert_eq!(resolve(&shifted(Key::B)), Some(Action::BandUp));
    }

    #[test]
    fn ctrl_and_cmd_are_never_captured() {
        let mut c = chord(Key::L);
        c.ctrl = true;
        assert_eq!(resolve(&c), None);
        let mut c = chord(Key::L);
        c.command = true;
        assert_eq!(resolve(&c), None);
    }
}

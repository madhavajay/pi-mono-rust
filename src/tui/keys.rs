//! Keyboard input handling for terminal applications.
//!
//! Supports both legacy terminal sequences and Kitty keyboard protocol.
//! See: <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>
//!
//! Symbol keys are also supported, however some ctrl+symbol combos
//! overlap with ASCII codes, e.g. ctrl+[ = ESC.
//! See: <https://sw.kovidgoyal.net/kitty/keyboard-protocol/#legacy-ctrl-mapping-of-ascii-keys>
//!
//! # API
//! - `matches_key(data, key_id)` - Check if input matches a key identifier
//! - `parse_key(data)` - Parse input and return the key identifier
//! - `set_kitty_protocol_active(active)` - Set global Kitty protocol state
//! - `is_kitty_protocol_active()` - Query global Kitty protocol state

use std::sync::atomic::{AtomicBool, Ordering};

// =============================================================================
// Global Kitty Protocol State
// =============================================================================

static KITTY_PROTOCOL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Set the global Kitty keyboard protocol state.
/// Called by ProcessTerminal after detecting protocol support.
pub fn set_kitty_protocol_active(active: bool) {
    KITTY_PROTOCOL_ACTIVE.store(active, Ordering::SeqCst);
}

/// Query whether Kitty keyboard protocol is currently active.
pub fn is_kitty_protocol_active() -> bool {
    KITTY_PROTOCOL_ACTIVE.load(Ordering::SeqCst)
}

// =============================================================================
// Constants
// =============================================================================

const SYMBOL_KEYS: &[char] = &[
    '`', '-', '=', '[', ']', '\\', ';', '\'', ',', '.', '/', '!', '@', '#', '$', '%', '^', '&',
    '*', '(', ')', '_', '+', '|', '~', '{', '}', ':', '<', '>', '?',
];

mod modifiers {
    pub const SHIFT: u8 = 1;
    pub const ALT: u8 = 2;
    pub const CTRL: u8 = 4;
}

const LOCK_MASK: u8 = 64 + 128; // Caps Lock + Num Lock

mod codepoints {
    pub const ESCAPE: i32 = 27;
    pub const TAB: i32 = 9;
    pub const ENTER: i32 = 13;
    pub const SPACE: i32 = 32;
    pub const BACKSPACE: i32 = 127;
    pub const KP_ENTER: i32 = 57414; // Numpad Enter (Kitty protocol)
}

mod arrow_codepoints {
    pub const UP: i32 = -1;
    pub const DOWN: i32 = -2;
    pub const RIGHT: i32 = -3;
    pub const LEFT: i32 = -4;
}

mod functional_codepoints {
    pub const DELETE: i32 = -10;
    pub const INSERT: i32 = -11;
    pub const PAGE_UP: i32 = -12;
    pub const PAGE_DOWN: i32 = -13;
    pub const HOME: i32 = -14;
    pub const END: i32 = -15;
}

// =============================================================================
// Kitty Protocol Parsing
// =============================================================================

struct ParsedKittySequence {
    codepoint: i32,
    modifier: u8,
}

fn parse_kitty_sequence(data: &str) -> Option<ParsedKittySequence> {
    let bytes = data.as_bytes();

    // CSI u format: \x1b[<num>u or \x1b[<num>;<mod>u
    if bytes.starts_with(b"\x1b[") && bytes.ends_with(b"u") {
        let inner = &data[2..data.len() - 1];
        if let Some(semi_pos) = inner.find(';') {
            let codepoint: i32 = inner[..semi_pos].parse().ok()?;
            let mod_value: u8 = inner[semi_pos + 1..].parse().ok()?;
            return Some(ParsedKittySequence {
                codepoint,
                modifier: mod_value.saturating_sub(1),
            });
        } else {
            let codepoint: i32 = inner.parse().ok()?;
            return Some(ParsedKittySequence {
                codepoint,
                modifier: 0,
            });
        }
    }

    // Arrow keys with modifier: \x1b[1;<mod>A/B/C/D
    if bytes.starts_with(b"\x1b[1;") && bytes.len() >= 6 {
        let last_byte = bytes[bytes.len() - 1];
        if matches!(last_byte, b'A' | b'B' | b'C' | b'D') {
            let mod_str = &data[4..data.len() - 1];
            if let Ok(mod_value) = mod_str.parse::<u8>() {
                let codepoint = match last_byte {
                    b'A' => arrow_codepoints::UP,
                    b'B' => arrow_codepoints::DOWN,
                    b'C' => arrow_codepoints::RIGHT,
                    b'D' => arrow_codepoints::LEFT,
                    _ => return None,
                };
                return Some(ParsedKittySequence {
                    codepoint,
                    modifier: mod_value.saturating_sub(1),
                });
            }
        }
    }

    // Functional keys: \x1b[<num>~ or \x1b[<num>;<mod>~
    if bytes.starts_with(b"\x1b[") && bytes.ends_with(b"~") {
        let inner = &data[2..data.len() - 1];
        let (key_num, mod_value) = if let Some(semi_pos) = inner.find(';') {
            let key_num: u8 = inner[..semi_pos].parse().ok()?;
            let mod_value: u8 = inner[semi_pos + 1..].parse().ok()?;
            (key_num, mod_value.saturating_sub(1))
        } else {
            let key_num: u8 = inner.parse().ok()?;
            (key_num, 0)
        };

        let codepoint = match key_num {
            2 => functional_codepoints::INSERT,
            3 => functional_codepoints::DELETE,
            5 => functional_codepoints::PAGE_UP,
            6 => functional_codepoints::PAGE_DOWN,
            7 => functional_codepoints::HOME,
            8 => functional_codepoints::END,
            _ => return None,
        };
        return Some(ParsedKittySequence {
            codepoint,
            modifier: mod_value,
        });
    }

    // Home/End with modifier: \x1b[1;<mod>H/F
    if bytes.starts_with(b"\x1b[1;") && bytes.len() >= 6 {
        let last_byte = bytes[bytes.len() - 1];
        if matches!(last_byte, b'H' | b'F') {
            let mod_str = &data[4..data.len() - 1];
            if let Ok(mod_value) = mod_str.parse::<u8>() {
                let codepoint = if last_byte == b'H' {
                    functional_codepoints::HOME
                } else {
                    functional_codepoints::END
                };
                return Some(ParsedKittySequence {
                    codepoint,
                    modifier: mod_value.saturating_sub(1),
                });
            }
        }
    }

    None
}

fn matches_kitty_sequence(data: &str, expected_codepoint: i32, expected_modifier: u8) -> bool {
    if let Some(parsed) = parse_kitty_sequence(data) {
        let actual_mod = parsed.modifier & !LOCK_MASK;
        let expected_mod = expected_modifier & !LOCK_MASK;
        parsed.codepoint == expected_codepoint && actual_mod == expected_mod
    } else {
        false
    }
}

// =============================================================================
// Generic Key Matching
// =============================================================================

fn raw_ctrl_char(letter: char) -> char {
    let code = letter.to_ascii_lowercase() as u8;
    let ctrl_code = code.saturating_sub(96);
    ctrl_code as char
}

struct ParsedKeyId {
    key: String,
    ctrl: bool,
    shift: bool,
    alt: bool,
}

fn parse_key_id(key_id: &str) -> Option<ParsedKeyId> {
    let lower = key_id.to_lowercase();
    let parts: Vec<&str> = lower.split('+').collect();
    let key = parts.last()?.to_string();
    if key.is_empty() {
        return None;
    }
    Some(ParsedKeyId {
        key,
        ctrl: parts.contains(&"ctrl"),
        shift: parts.contains(&"shift"),
        alt: parts.contains(&"alt"),
    })
}

fn is_symbol_key(c: char) -> bool {
    SYMBOL_KEYS.contains(&c)
}

/// Match input data against a key identifier string.
///
/// # Supported key identifiers
/// - Single keys: "escape", "tab", "enter", "backspace", "delete", "home", "end", "space"
/// - Arrow keys: "up", "down", "left", "right"
/// - Ctrl combinations: "ctrl+c", "ctrl+z", etc.
/// - Shift combinations: "shift+tab", "shift+enter"
/// - Alt combinations: "alt+enter", "alt+backspace"
/// - Combined modifiers: "shift+ctrl+p", "ctrl+alt+x"
///
/// # Arguments
/// * `data` - Raw input data from terminal
/// * `key_id` - Key identifier (e.g., "ctrl+c", "escape")
pub fn matches_key(data: &str, key_id: &str) -> bool {
    let parsed = match parse_key_id(key_id) {
        Some(p) => p,
        None => return false,
    };

    let ParsedKeyId {
        key,
        ctrl,
        shift,
        alt,
    } = parsed;
    let mut modifier: u8 = 0;
    if shift {
        modifier |= modifiers::SHIFT;
    }
    if alt {
        modifier |= modifiers::ALT;
    }
    if ctrl {
        modifier |= modifiers::CTRL;
    }

    match key.as_str() {
        "escape" | "esc" => {
            if modifier != 0 {
                return false;
            }
            data == "\x1b" || matches_kitty_sequence(data, codepoints::ESCAPE, 0)
        }

        "space" => {
            if modifier == 0 {
                data == " " || matches_kitty_sequence(data, codepoints::SPACE, 0)
            } else {
                matches_kitty_sequence(data, codepoints::SPACE, modifier)
            }
        }

        "tab" => {
            if shift && !ctrl && !alt {
                data == "\x1b[Z" || matches_kitty_sequence(data, codepoints::TAB, modifiers::SHIFT)
            } else if modifier == 0 {
                data == "\t" || matches_kitty_sequence(data, codepoints::TAB, 0)
            } else {
                matches_kitty_sequence(data, codepoints::TAB, modifier)
            }
        }

        "enter" | "return" => {
            if shift && !ctrl && !alt {
                // CSI u sequences (standard Kitty protocol)
                if matches_kitty_sequence(data, codepoints::ENTER, modifiers::SHIFT)
                    || matches_kitty_sequence(data, codepoints::KP_ENTER, modifiers::SHIFT)
                {
                    return true;
                }
                // When Kitty protocol is active, legacy sequences are custom terminal mappings
                // \x1b\r = Kitty's "map shift+enter send_text all \e\r"
                // \n = Ghostty's "keybind = shift+enter=text:\n"
                if is_kitty_protocol_active() {
                    return data == "\x1b\r" || data == "\n";
                }
                false
            } else if alt && !ctrl && !shift {
                // CSI u sequences (standard Kitty protocol)
                if matches_kitty_sequence(data, codepoints::ENTER, modifiers::ALT)
                    || matches_kitty_sequence(data, codepoints::KP_ENTER, modifiers::ALT)
                {
                    return true;
                }
                // \x1b\r is alt+enter only in legacy mode (no Kitty protocol)
                if !is_kitty_protocol_active() {
                    return data == "\x1b\r";
                }
                false
            } else if modifier == 0 {
                data == "\r"
                    || data == "\x1bOM" // SS3 M (numpad enter in some terminals)
                    || matches_kitty_sequence(data, codepoints::ENTER, 0)
                    || matches_kitty_sequence(data, codepoints::KP_ENTER, 0)
            } else {
                matches_kitty_sequence(data, codepoints::ENTER, modifier)
                    || matches_kitty_sequence(data, codepoints::KP_ENTER, modifier)
            }
        }

        "backspace" => {
            if alt && !ctrl && !shift {
                data == "\x1b\x7f"
                    || matches_kitty_sequence(data, codepoints::BACKSPACE, modifiers::ALT)
            } else if modifier == 0 {
                data == "\x7f"
                    || data == "\x08"
                    || matches_kitty_sequence(data, codepoints::BACKSPACE, 0)
            } else {
                matches_kitty_sequence(data, codepoints::BACKSPACE, modifier)
            }
        }

        "delete" => {
            if modifier == 0 {
                data == "\x1b[3~" || matches_kitty_sequence(data, functional_codepoints::DELETE, 0)
            } else {
                matches_kitty_sequence(data, functional_codepoints::DELETE, modifier)
            }
        }

        "home" => {
            if modifier == 0 {
                data == "\x1b[H"
                    || data == "\x1b[1~"
                    || data == "\x1b[7~"
                    || matches_kitty_sequence(data, functional_codepoints::HOME, 0)
            } else {
                matches_kitty_sequence(data, functional_codepoints::HOME, modifier)
            }
        }

        "end" => {
            if modifier == 0 {
                data == "\x1b[F"
                    || data == "\x1b[4~"
                    || data == "\x1b[8~"
                    || matches_kitty_sequence(data, functional_codepoints::END, 0)
            } else {
                matches_kitty_sequence(data, functional_codepoints::END, modifier)
            }
        }

        "up" => {
            if modifier == 0 {
                data == "\x1b[A" || matches_kitty_sequence(data, arrow_codepoints::UP, 0)
            } else {
                matches_kitty_sequence(data, arrow_codepoints::UP, modifier)
            }
        }

        "down" => {
            if modifier == 0 {
                data == "\x1b[B" || matches_kitty_sequence(data, arrow_codepoints::DOWN, 0)
            } else {
                matches_kitty_sequence(data, arrow_codepoints::DOWN, modifier)
            }
        }

        "left" => {
            if alt && !ctrl && !shift {
                data == "\x1b[1;3D"
                    || data == "\x1bb"
                    || matches_kitty_sequence(data, arrow_codepoints::LEFT, modifiers::ALT)
            } else if ctrl && !alt && !shift {
                data == "\x1b[1;5D"
                    || matches_kitty_sequence(data, arrow_codepoints::LEFT, modifiers::CTRL)
            } else if modifier == 0 {
                data == "\x1b[D" || matches_kitty_sequence(data, arrow_codepoints::LEFT, 0)
            } else {
                matches_kitty_sequence(data, arrow_codepoints::LEFT, modifier)
            }
        }

        "right" => {
            if alt && !ctrl && !shift {
                data == "\x1b[1;3C"
                    || data == "\x1bf"
                    || matches_kitty_sequence(data, arrow_codepoints::RIGHT, modifiers::ALT)
            } else if ctrl && !alt && !shift {
                data == "\x1b[1;5C"
                    || matches_kitty_sequence(data, arrow_codepoints::RIGHT, modifiers::CTRL)
            } else if modifier == 0 {
                data == "\x1b[C" || matches_kitty_sequence(data, arrow_codepoints::RIGHT, 0)
            } else {
                matches_kitty_sequence(data, arrow_codepoints::RIGHT, modifier)
            }
        }

        _ => {
            // Handle single letter keys (a-z) and some symbols
            if key.len() == 1 {
                let c = key.chars().next().unwrap();
                if c.is_ascii_lowercase() || is_symbol_key(c) {
                    let codepoint = c as i32;

                    if ctrl && !shift && !alt {
                        let raw = raw_ctrl_char(c);
                        if data.len() == 1 && data.starts_with(raw) {
                            return true;
                        }
                        return matches_kitty_sequence(data, codepoint, modifiers::CTRL);
                    }

                    if ctrl && shift && !alt {
                        return matches_kitty_sequence(
                            data,
                            codepoint,
                            modifiers::SHIFT + modifiers::CTRL,
                        );
                    }

                    if shift && !ctrl && !alt {
                        // Legacy: shift+letter produces uppercase
                        if data == c.to_ascii_uppercase().to_string() {
                            return true;
                        }
                        return matches_kitty_sequence(data, codepoint, modifiers::SHIFT);
                    }

                    if modifier != 0 {
                        return matches_kitty_sequence(data, codepoint, modifier);
                    }

                    return data == key;
                }
            }
            false
        }
    }
}

/// Parse input data and return the key identifier if recognized.
///
/// # Arguments
/// * `data` - Raw input data from terminal
///
/// # Returns
/// Key identifier string (e.g., "ctrl+c") or None
pub fn parse_key(data: &str) -> Option<String> {
    if let Some(kitty) = parse_kitty_sequence(data) {
        let effective_mod = kitty.modifier & !LOCK_MASK;
        let mut mods = Vec::new();
        if effective_mod & modifiers::SHIFT != 0 {
            mods.push("shift");
        }
        if effective_mod & modifiers::CTRL != 0 {
            mods.push("ctrl");
        }
        if effective_mod & modifiers::ALT != 0 {
            mods.push("alt");
        }

        let key_name = match kitty.codepoint {
            c if c == codepoints::ESCAPE => Some("escape"),
            c if c == codepoints::TAB => Some("tab"),
            c if c == codepoints::ENTER || c == codepoints::KP_ENTER => Some("enter"),
            c if c == codepoints::SPACE => Some("space"),
            c if c == codepoints::BACKSPACE => Some("backspace"),
            c if c == functional_codepoints::DELETE => Some("delete"),
            c if c == functional_codepoints::HOME => Some("home"),
            c if c == functional_codepoints::END => Some("end"),
            c if c == arrow_codepoints::UP => Some("up"),
            c if c == arrow_codepoints::DOWN => Some("down"),
            c if c == arrow_codepoints::LEFT => Some("left"),
            c if c == arrow_codepoints::RIGHT => Some("right"),
            c if (97..=122).contains(&c) => None, // Will handle below
            c if is_symbol_key(char::from_u32(c as u32).unwrap_or('\0')) => None, // Will handle below
            _ => None,
        };

        if let Some(name) = key_name {
            return if mods.is_empty() {
                Some(name.to_string())
            } else {
                Some(format!("{}+{}", mods.join("+"), name))
            };
        }

        // Handle letter keys
        if (97..=122).contains(&kitty.codepoint) {
            let ch = char::from_u32(kitty.codepoint as u32)?;
            return if mods.is_empty() {
                Some(ch.to_string())
            } else {
                Some(format!("{}+{}", mods.join("+"), ch))
            };
        }

        // Handle symbol keys
        if let Some(ch) = char::from_u32(kitty.codepoint as u32) {
            if is_symbol_key(ch) {
                return if mods.is_empty() {
                    Some(ch.to_string())
                } else {
                    Some(format!("{}+{}", mods.join("+"), ch))
                };
            }
        }
    }

    // Mode-aware legacy sequences
    if is_kitty_protocol_active() && (data == "\x1b\r" || data == "\n") {
        return Some("shift+enter".to_string());
    }

    // Legacy sequences (used when Kitty protocol is not active, or for unambiguous sequences)
    match data {
        "\x1b" => return Some("escape".to_string()),
        "\t" => return Some("tab".to_string()),
        "\r" | "\x1bOM" => return Some("enter".to_string()),
        " " => return Some("space".to_string()),
        "\x7f" | "\x08" => return Some("backspace".to_string()),
        "\x1b[Z" => return Some("shift+tab".to_string()),
        "\x1b\x7f" => return Some("alt+backspace".to_string()),
        "\x1b[A" => return Some("up".to_string()),
        "\x1b[B" => return Some("down".to_string()),
        "\x1b[C" => return Some("right".to_string()),
        "\x1b[D" => return Some("left".to_string()),
        "\x1b[H" => return Some("home".to_string()),
        "\x1b[F" => return Some("end".to_string()),
        "\x1b[3~" => return Some("delete".to_string()),
        _ => {}
    }

    // \x1b\r is alt+enter only in legacy mode
    if !is_kitty_protocol_active() && data == "\x1b\r" {
        return Some("alt+enter".to_string());
    }

    // Raw Ctrl+letter
    if data.len() == 1 {
        let code = data.as_bytes()[0];
        if (1..=26).contains(&code) {
            let letter = (code + 96) as char;
            return Some(format!("ctrl+{}", letter));
        }
        if (32..=126).contains(&code) {
            return Some(data.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_key() {
        assert!(matches_key("\x1b", "escape"));
        assert!(matches_key("\x1b", "esc"));
        assert!(!matches_key("\x1b", "ctrl+escape"));
    }

    #[test]
    fn test_enter_key() {
        assert!(matches_key("\r", "enter"));
        assert!(matches_key("\r", "return"));
        assert!(matches_key("\x1bOM", "enter")); // SS3 M numpad enter
    }

    #[test]
    fn test_tab_key() {
        assert!(matches_key("\t", "tab"));
        assert!(matches_key("\x1b[Z", "shift+tab"));
    }

    #[test]
    fn test_backspace() {
        assert!(matches_key("\x7f", "backspace"));
        assert!(matches_key("\x08", "backspace"));
        assert!(matches_key("\x1b\x7f", "alt+backspace"));
    }

    #[test]
    fn test_arrow_keys() {
        assert!(matches_key("\x1b[A", "up"));
        assert!(matches_key("\x1b[B", "down"));
        assert!(matches_key("\x1b[C", "right"));
        assert!(matches_key("\x1b[D", "left"));
    }

    #[test]
    fn test_ctrl_letters() {
        assert!(matches_key("\x03", "ctrl+c"));
        assert!(matches_key("\x1a", "ctrl+z"));
        assert!(matches_key("\x01", "ctrl+a"));
    }

    #[test]
    fn test_ctrl_arrow_keys() {
        assert!(matches_key("\x1b[1;5D", "ctrl+left"));
        assert!(matches_key("\x1b[1;5C", "ctrl+right"));
    }

    #[test]
    fn test_alt_arrow_keys() {
        assert!(matches_key("\x1b[1;3D", "alt+left"));
        assert!(matches_key("\x1bb", "alt+left"));
        assert!(matches_key("\x1b[1;3C", "alt+right"));
        assert!(matches_key("\x1bf", "alt+right"));
    }

    #[test]
    fn test_home_end() {
        assert!(matches_key("\x1b[H", "home"));
        assert!(matches_key("\x1b[F", "end"));
        assert!(matches_key("\x1b[1~", "home"));
        assert!(matches_key("\x1b[4~", "end"));
    }

    #[test]
    fn test_delete() {
        assert!(matches_key("\x1b[3~", "delete"));
    }

    #[test]
    fn test_space() {
        assert!(matches_key(" ", "space"));
    }

    #[test]
    fn test_kitty_csi_u_format() {
        // CSI u format: \x1b[<codepoint>u
        assert!(matches_key("\x1b[27u", "escape"));
        assert!(matches_key("\x1b[13u", "enter"));
        assert!(matches_key("\x1b[9u", "tab"));

        // With modifiers: \x1b[<codepoint>;<modifier+1>u
        assert!(matches_key("\x1b[13;2u", "shift+enter")); // modifier 1 (shift) + 1 = 2
        assert!(matches_key("\x1b[13;3u", "alt+enter")); // modifier 2 (alt) + 1 = 3
        assert!(matches_key("\x1b[99;5u", "ctrl+c")); // modifier 4 (ctrl) + 1 = 5
    }

    #[test]
    fn test_parse_key_legacy() {
        assert_eq!(parse_key("\x1b"), Some("escape".to_string()));
        assert_eq!(parse_key("\t"), Some("tab".to_string()));
        assert_eq!(parse_key("\r"), Some("enter".to_string()));
        assert_eq!(parse_key(" "), Some("space".to_string()));
        assert_eq!(parse_key("\x7f"), Some("backspace".to_string()));
        assert_eq!(parse_key("\x1b[Z"), Some("shift+tab".to_string()));
        assert_eq!(parse_key("\x1b[A"), Some("up".to_string()));
        assert_eq!(parse_key("\x1b[B"), Some("down".to_string()));
        assert_eq!(parse_key("\x1b[C"), Some("right".to_string()));
        assert_eq!(parse_key("\x1b[D"), Some("left".to_string()));
        assert_eq!(parse_key("\x03"), Some("ctrl+c".to_string()));
    }

    #[test]
    fn test_parse_key_kitty() {
        assert_eq!(parse_key("\x1b[27u"), Some("escape".to_string()));
        assert_eq!(parse_key("\x1b[13u"), Some("enter".to_string()));
        assert_eq!(parse_key("\x1b[13;2u"), Some("shift+enter".to_string()));
        assert_eq!(parse_key("\x1b[99;5u"), Some("ctrl+c".to_string()));
    }

    #[test]
    fn test_shift_enter_kitty_mode() {
        // When Kitty protocol is active, these legacy sequences mean shift+enter
        set_kitty_protocol_active(true);
        assert!(matches_key("\x1b\r", "shift+enter"));
        assert!(matches_key("\n", "shift+enter"));
        assert_eq!(parse_key("\x1b\r"), Some("shift+enter".to_string()));

        set_kitty_protocol_active(false);
        // When not active, \x1b\r means alt+enter
        assert!(matches_key("\x1b\r", "alt+enter"));
        assert_eq!(parse_key("\x1b\r"), Some("alt+enter".to_string()));
    }

    #[test]
    fn test_plain_letters() {
        assert!(matches_key("a", "a"));
        assert!(matches_key("z", "z"));
        assert!(!matches_key("A", "a")); // Case sensitive
    }

    #[test]
    fn test_shift_letters() {
        assert!(matches_key("A", "shift+a"));
        assert!(matches_key("Z", "shift+z"));
    }
}

//! Keyboard shortcuts, stored as text such as `Ctrl+Alt+W`.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A letter or digit, upper case.
    Char(char),
    /// F1 to F12.
    F(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: Key,
}

/// Where Slint puts F1; F2 to F12 follow it.
const SLINT_F1: u32 = 0xF704;

fn key_of(text: &str) -> Option<Key> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    match c {
        'a'..='z' | 'A'..='Z' | '0'..='9' => Some(Key::Char(c.to_ascii_uppercase())),
        _ => {
            let n = u32::from(c).checked_sub(SLINT_F1)?;
            (n < 12).then(|| Key::F(n as u8 + 1))
        }
    }
}

impl Hotkey {
    /// Without Ctrl, Alt or Win it would take over ordinary typing.
    fn valid(self) -> Option<Self> {
        (self.ctrl || self.alt || self.win).then_some(self)
    }

    /// From a key press in the settings window: its text and modifiers.
    pub fn from_press(text: &str, ctrl: bool, alt: bool, shift: bool, win: bool) -> Option<Self> {
        Hotkey {
            ctrl,
            alt,
            shift,
            win,
            key: key_of(text)?,
        }
        .valid()
    }

    pub fn parse(s: &str) -> Option<Self> {
        let (mut ctrl, mut alt, mut shift, mut win) = (false, false, false, false);
        let mut key = None;
        for part in s.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" => ctrl = true,
                "alt" => alt = true,
                "shift" => shift = true,
                "win" => win = true,
                name if key.is_none() => {
                    key = match name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                        Some(n @ 1..=12) => Some(Key::F(n)),
                        _ => key_of(name),
                    };
                    key?;
                }
                _ => return None,
            }
        }
        Hotkey {
            ctrl,
            alt,
            shift,
            win,
            key: key?,
        }
        .valid()
    }

    /// The Windows virtual-key code.
    pub fn virtual_key(self) -> u32 {
        match self.key {
            Key::Char(c) => u32::from(c),
            Key::F(n) => 0x70 + u32::from(n) - 1,
        }
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
            (self.win, "Win+"),
        ] {
            if on {
                f.write_str(name)?;
            }
        }
        match self.key {
            Key::Char(c) => write!(f, "{c}"),
            Key::F(n) => write!(f, "F{n}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_read_and_write_the_same_text() {
        for s in ["Ctrl+Alt+W", "Ctrl+Shift+F5", "Alt+Win+0", "Alt+F12"] {
            assert_eq!(Hotkey::parse(s).unwrap().to_string(), s);
        }
        assert_eq!(
            Hotkey::parse(" alt + ctrl + w ").unwrap().to_string(),
            "Ctrl+Alt+W"
        );
    }

    #[test]
    fn a_shortcut_needs_a_modifier_and_one_known_key() {
        for s in [
            "W",
            "Shift+W",
            "Ctrl+Alt",
            "Ctrl+W+Q",
            "Ctrl+F13",
            "Ctrl+Space",
            "",
        ] {
            assert_eq!(Hotkey::parse(s), None, "{s}");
        }
    }

    #[test]
    fn presses_in_the_settings_window_become_shortcuts() {
        let f5 = char::from_u32(SLINT_F1 + 4).unwrap().to_string();
        let k = Hotkey::from_press(&f5, true, false, true, false).unwrap();
        assert_eq!(
            (k.to_string(), k.virtual_key()),
            ("Ctrl+Shift+F5".into(), 0x74)
        );
        let k = Hotkey::from_press("w", false, true, false, false).unwrap();
        assert_eq!((k.to_string(), k.virtual_key()), ("Alt+W".into(), 0x57));
        // Pressing Ctrl alone sends a control character.
        assert_eq!(
            Hotkey::from_press("\u{11}", true, false, false, false),
            None
        );
        assert_eq!(Hotkey::from_press("w", false, false, true, false), None);
    }
}

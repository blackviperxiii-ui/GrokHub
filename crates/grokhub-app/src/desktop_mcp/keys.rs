//! Key combo → virtual-key, X keysym, and Linux evdev code.
//! Each table is compiled on its OS and in tests, so neither platform has a dead helper.

#[cfg(any(windows, unix, test))]
use grokhub_core::desktop_mcp::{KeyCombo, KeyName};

#[cfg(any(windows, test))]
pub(crate) fn vk_of(key: &KeyName) -> Option<u16> {
    Some(match key {
        KeyName::Char(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase() as u16,
        KeyName::Char(c) if c.is_ascii_digit() => *c as u16,
        KeyName::Char(' ') | KeyName::Space => 0x20,
        KeyName::Return => 0x0D,
        KeyName::Escape => 0x1B,
        KeyName::Backspace => 0x08,
        KeyName::Tab => 0x09,
        KeyName::Delete => 0x2E,
        KeyName::Insert => 0x2D,
        KeyName::Home => 0x24,
        KeyName::End => 0x23,
        KeyName::PageUp => 0x21,
        KeyName::PageDown => 0x22,
        KeyName::Left => 0x25,
        KeyName::Up => 0x26,
        KeyName::Right => 0x27,
        KeyName::Down => 0x28,
        KeyName::F(n) if (1..=24).contains(n) => 0x70 + (*n as u16 - 1),
        KeyName::F(_) => return None,
        KeyName::Super => 0x5B,
        KeyName::Char(_) => return None,
    })
}

#[cfg(any(windows, test))]
pub(crate) fn vk_mods(combo: &KeyCombo) -> Vec<u16> {
    let mut mods = Vec::new();
    if combo.ctrl {
        mods.push(0x11);
    }
    if combo.shift {
        mods.push(0x10);
    }
    if combo.alt {
        mods.push(0x12);
    }
    if combo.super_key {
        mods.push(0x5B);
    }
    mods
}

#[cfg(any(unix, test))]
pub(crate) fn keysym_of(key: &KeyName) -> u32 {
    match key {
        KeyName::Return => 0xff0d,
        KeyName::Escape => 0xff1b,
        KeyName::Backspace => 0xff08,
        KeyName::Tab => 0xff09,
        KeyName::Space | KeyName::Char(' ') => 0x20,
        KeyName::Delete => 0xffff,
        KeyName::Insert => 0xff63,
        KeyName::Home => 0xff50,
        KeyName::Left => 0xff51,
        KeyName::Up => 0xff52,
        KeyName::Right => 0xff53,
        KeyName::Down => 0xff54,
        KeyName::PageUp => 0xff55,
        KeyName::PageDown => 0xff56,
        KeyName::End => 0xff57,
        KeyName::F(n) => 0xffbe + (*n as u32 - 1),
        KeyName::Super => 0xffeb,
        KeyName::Char(c) if (*c as u32) < 0x100 => *c as u32,
        KeyName::Char(c) => 0x0100_0000 + (*c as u32),
    }
}

#[cfg(any(unix, test))]
pub(crate) fn keysym_mods(combo: &KeyCombo) -> Vec<u32> {
    let mut mods = Vec::new();
    if combo.ctrl {
        mods.push(0xffe3);
    }
    if combo.shift {
        mods.push(0xffe1);
    }
    if combo.alt {
        mods.push(0xffe9);
    }
    if combo.super_key {
        mods.push(0xffeb);
    }
    mods
}

#[cfg(any(unix, test))]
pub(crate) fn evdev_of(key: &KeyName) -> Option<u16> {
    Some(match key {
        KeyName::Return => 28,
        KeyName::Escape => 1,
        KeyName::Tab => 15,
        KeyName::Space | KeyName::Char(' ') => 57,
        KeyName::Backspace => 14,
        KeyName::Super => 125,
        KeyName::Up => 103,
        KeyName::Down => 108,
        KeyName::Left => 105,
        KeyName::Right => 106,
        KeyName::Delete => 111,
        KeyName::Insert => 110,
        KeyName::Home => 102,
        KeyName::End => 107,
        KeyName::PageUp => 104,
        KeyName::PageDown => 109,
        KeyName::F(n) if (1..=10).contains(n) => 59 + (*n as u16 - 1),
        KeyName::F(11) => 87,
        KeyName::F(12) => 88,
        KeyName::F(n) if (13..=24).contains(n) => 183 + (*n as u16 - 13),
        KeyName::Char(c) if c.is_ascii_digit() => match *c {
            '0' => 11,
            '1'..='9' => 2 + (*c as u16 - b'1' as u16),
            _ => return None,
        },
        KeyName::Char(c) if c.is_ascii_lowercase() => match *c {
            'a' => 30,
            'b' => 48,
            'c' => 46,
            'd' => 32,
            'e' => 18,
            'f' => 33,
            'g' => 34,
            'h' => 35,
            'i' => 23,
            'j' => 36,
            'k' => 37,
            'l' => 38,
            'm' => 50,
            'n' => 49,
            'o' => 24,
            'p' => 25,
            'q' => 16,
            'r' => 19,
            's' => 31,
            't' => 20,
            'u' => 22,
            'v' => 47,
            'w' => 17,
            'x' => 45,
            'y' => 21,
            'z' => 44,
            _ => return None,
        },
        _ => return None,
    })
}

#[cfg(any(unix, test))]
pub(crate) fn evdev_mods(combo: &KeyCombo) -> Vec<u16> {
    let mut mods = Vec::new();
    if combo.ctrl {
        mods.push(29);
    }
    if combo.shift {
        mods.push(42);
    }
    if combo.alt {
        mods.push(56);
    }
    if combo.super_key {
        mods.push(125);
    }
    mods
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::desktop_mcp::{parse_key_combo, KeyName};

    #[test]
    fn desktop_mcp_vk_table() {
        assert_eq!(vk_of(&KeyName::Return), Some(0x0D));
        assert_eq!(vk_of(&KeyName::F(1)), Some(0x70));
        assert_eq!(vk_of(&KeyName::F(24)), Some(0x70 + 23));
        assert_eq!(vk_of(&KeyName::Left), Some(0x25));
        assert_eq!(vk_of(&KeyName::PageDown), Some(0x22));
        assert_eq!(vk_of(&KeyName::Super), Some(0x5B));
        assert_eq!(vk_of(&KeyName::Char('t')), Some(b'T' as u16));
        assert_eq!(vk_of(&KeyName::Char('é')), None);
        let combo = parse_key_combo("ctrl+shift+t").unwrap();
        assert_eq!(vk_mods(&combo), vec![0x11, 0x10]);
    }

    #[test]
    fn desktop_mcp_keysym_table() {
        assert_eq!(keysym_of(&KeyName::Return), 0xff0d);
        assert_eq!(keysym_of(&KeyName::F(1)), 0xffbe);
        assert_eq!(keysym_of(&KeyName::F(24)), 0xffbe + 23);
        assert_eq!(keysym_of(&KeyName::Left), 0xff51);
        assert_eq!(keysym_of(&KeyName::PageDown), 0xff56);
        assert_eq!(keysym_of(&KeyName::Super), 0xffeb);
        assert_eq!(keysym_of(&KeyName::Char('t')), 0x74);
        assert_eq!(keysym_of(&KeyName::Char('é')), 0xe9);
        assert_eq!(keysym_of(&KeyName::Char('😀')), 0x0100_0000 + '😀' as u32);
        let combo = parse_key_combo("alt+F4").unwrap();
        assert_eq!(keysym_mods(&combo), vec![0xffe9]);
        assert_eq!(keysym_of(&combo.key), 0xffbe + 3);
    }

    #[test]
    fn desktop_mcp_evdev_table() {
        assert_eq!(evdev_of(&KeyName::Return), Some(28));
        assert_eq!(evdev_of(&KeyName::Char('a')), Some(30));
        assert_eq!(evdev_of(&KeyName::Char('t')), Some(20));
        assert_eq!(evdev_of(&KeyName::F(1)), Some(59));
        assert_eq!(evdev_of(&KeyName::F(12)), Some(88));
        assert_eq!(evdev_of(&KeyName::F(13)), Some(183));
        assert_eq!(evdev_of(&KeyName::Super), Some(125));
        assert_eq!(evdev_of(&KeyName::Char('é')), None);
        let combo = parse_key_combo("super+a").unwrap();
        assert_eq!(evdev_mods(&combo), vec![125]);
        assert_eq!(evdev_of(&combo.key), Some(30));
    }
}

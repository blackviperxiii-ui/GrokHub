//! The first name to greet the user by, from USER.md or a display name.
//! Product, placeholder and machine names never count.

use crate::is_plain_text;

pub fn greeting_name(user_md: &str, display_name: &str) -> String {
    for line in user_md.lines() {
        let raw = line.trim().trim_start_matches(['-', '*']).trim();
        let raw = raw.trim_matches('*').trim();
        let lower = raw.to_ascii_lowercase();
        if let Some(rest) = split_name_line(&lower, raw) {
            let n = clean_name(rest);
            if !n.is_empty() {
                return n;
            }
        }
    }
    for line in user_md.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('#') {
            let h = rest.trim().trim_start_matches('#').trim();
            if is_generic_heading(h) {
                continue;
            }
            let n = clean_name(h);
            if !n.is_empty() {
                return n;
            }
        }
    }
    clean_name(display_name)
}

fn split_name_line<'a>(lower: &str, raw: &'a str) -> Option<&'a str> {
    let is_name_key = lower.starts_with("name:")
        || lower.starts_with("name :")
        || lower.starts_with("name-")
        || lower.starts_with("name -")
        || lower.starts_with("name–")
        || lower.starts_with("name –");
    if !is_name_key {
        return None;
    }
    raw.split_once(':')
        .or_else(|| raw.split_once('–'))
        .or_else(|| raw.split_once('-'))
        .map(|(_, rest)| rest)
}

fn is_generic_heading(h: &str) -> bool {
    let t = h.trim().to_ascii_lowercase();
    t == "user"
        || t == "profile"
        || t.starts_with("user profile")
        || t == "who you are"
        || t == "long-term memory"
        || t == "memory"
}

fn is_placeholder_name(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "user" | "profile" | "guest" | "anonymous"
    )
}

fn is_product_name(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    t.is_empty()
        || t == "grok"
        || t == "grokhub"
        || t == "grok hub"
        || t == "cabin"
        || t == "this computer"
        || t == "localhost"
        || t.contains("grokhub")
        || is_placeholder_name(&t)
        || is_machine_name(&t)
}

/// Hostnames and distro names must not become "Hello, CachyOS."
fn is_machine_name(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        return false;
    }
    if t.contains("cachy")
        || t.contains("x86")
        || t.contains("x64")
        || t == "arch"
        || t == "archlinux"
        || t == "ubuntu"
        || t == "fedora"
        || t == "debian"
        || t == "manjaro"
        || t == "nixos"
        || t == "linux"
        || t == "hostname"
    {
        return true;
    }
    t.contains('-') && t.chars().any(|c| c.is_ascii_digit())
}

fn clean_name(s: &str) -> String {
    let s = s.trim().trim_matches('"').trim_matches('*').trim();
    if s.is_empty() || !is_plain_text(s) || is_product_name(s) {
        return String::new();
    }
    let first: String = s
        .split_whitespace()
        .next()
        .unwrap_or("")
        .chars()
        .take(24)
        .collect();
    if first.chars().count() < 2 || is_product_name(&first) {
        String::new()
    } else {
        first
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_name_prefers_user_md() {
        assert_eq!(
            greeting_name("Name: Tester\neditor: nvim\n", "Tester"),
            "Tester"
        );
        assert_eq!(greeting_name("# User\n", "Tester Doe"), "Tester");
        assert_eq!(
            greeting_name("# User profile\nWho you are, preferred tools.\n", "Tester"),
            "Tester",
            "the USER.md template heading must not become Hello, User"
        );
        assert_eq!(greeting_name("", "Tester"), "Tester");
        assert_eq!(greeting_name("", ""), "");
        assert_eq!(
            greeting_name("Named: my project\n", "Tester"),
            "Tester",
            "nameplate-style lines must not steal the greeting"
        );
        assert_eq!(
            greeting_name("", "GrokHub"),
            "",
            "device / product names must not become the empty-home greeting"
        );
        assert_eq!(greeting_name("", "This computer"), "");
        assert_eq!(greeting_name("", "cachyos-x8664"), "");
        assert_eq!(greeting_name("Name: CachyOS\n", "Tester"), "Tester");
        assert_eq!(greeting_name("", "CachyOS"), "");
    }
}

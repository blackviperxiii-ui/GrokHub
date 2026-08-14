//! Keyboard shortcuts registry — cheatsheet + palette.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub keys: &'static str,
    pub action: &'static str,
    pub scope: &'static str,
}

pub const SHORTCUTS: &[Shortcut] = &[
    Shortcut { keys: "Ctrl+K", action: "Command palette", scope: "Global" },
    Shortcut { keys: "Ctrl+N", action: "New chat", scope: "Global" },
    Shortcut { keys: "Ctrl+G", action: "Hey Grok (listen or halt)", scope: "Global" },
    Shortcut { keys: "Super+G", action: "Hey Grok when unfocused", scope: "System" },
    Shortcut { keys: "Ctrl+Shift+Esc", action: "Halt hands", scope: "Global" },
    Shortcut { keys: "Super+Shift+Esc", action: "Halt when unfocused", scope: "System" },
    Shortcut { keys: "Ctrl+Enter", action: "Send message", scope: "Composer" },
    Shortcut { keys: "Tab", action: "Accept slash", scope: "Composer" },
    Shortcut { keys: "Enter / Esc", action: "Allow / deny host plan", scope: "Host" },
];

pub fn shortcut_help() -> String {
    SHORTCUTS
        .iter()
        .map(|s| format!("{} — {} ({})", s.keys, s.action, s.scope))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn filter_palette(q: &str) -> Vec<(&'static str, &'static str)> {
    let n = q.trim().to_ascii_lowercase();
    let rows = [
        ("Chat", "nav:chat"),
        ("Night", "nav:night"),
        ("History", "nav:history"),
        ("Devices", "nav:devices"),
        ("Connectors", "nav:connectors"),
        ("Command", "nav:command"),
        ("Agents", "nav:agents"),
        ("Eyes", "nav:eyes"),
        ("Skills", "nav:skills"),
        ("Board", "nav:board"),
        ("Imagine", "nav:imagine"),
        ("Memory", "nav:memory"),
        ("Settings", "nav:settings"),
        ("New chat", "/new"),
        ("Doctor", "/health"),
        ("Update install", "/update"),
        ("Connect Grok OAuth", "oauth"),
        ("Copy diagnostics", "diag"),
        ("Import OpenClaw", "/import"),
        ("Hey Grok", "voice"),
    ];
    rows.into_iter()
        .filter(|(label, _)| n.is_empty() || label.to_ascii_lowercase().contains(&n))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_and_sheet() {
        assert!(shortcut_help().contains("Ctrl+K"));
        assert!(shortcut_help().contains("Super+G"));
        assert!(filter_palette("night").iter().any(|(l, _)| *l == "Night"));
        assert!(filter_palette("").len() >= 8);
    }
}
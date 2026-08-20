//! Keyboard shortcuts registry — cheatsheet + palette.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerEnter {
    Send,
    Newline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerGo {
    Idle,
    Send,
    Stop,
}

/// Send turns into Stop while a reply (or host/imagine job) is running.
pub fn composer_go(running: bool, ready: bool) -> ComposerGo {
    if running {
        ComposerGo::Stop
    } else if ready {
        ComposerGo::Send
    } else {
        ComposerGo::Idle
    }
}

pub fn composer_go_tip(running: bool) -> &'static str {
    if running {
        "Stop"
    } else {
        "Send"
    }
}

pub fn composer_enter(enter: bool, control: bool) -> Option<ComposerEnter> {
    if !enter {
        return None;
    }
    if control {
        Some(ComposerEnter::Newline)
    } else {
        Some(ComposerEnter::Send)
    }
}

/// Returns true when the composer should send. Control+Enter appends a newline.
pub fn apply_composer_enter(buf: &mut String, enter: bool, control: bool) -> bool {
    match composer_enter(enter, control) {
        Some(ComposerEnter::Send) => true,
        Some(ComposerEnter::Newline) => {
            buf.push('\n');
            false
        }
        None => false,
    }
}

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
    Shortcut { keys: "Ctrl+Shift+Esc", action: "Halt", scope: "Global" },
    Shortcut { keys: "Super+Shift+Esc", action: "Halt when unfocused", scope: "System" },
    Shortcut { keys: "Enter", action: "Send message", scope: "Composer" },
    Shortcut { keys: "Ctrl+Enter", action: "New line", scope: "Composer" },
    Shortcut { keys: "Tab", action: "Accept slash", scope: "Composer" },
    Shortcut { keys: "Enter / Esc", action: "Allow / deny tool permission", scope: "Chat" },
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
        ("Agents", "nav:agents"),
        ("Skills", "nav:skills"),
        ("Board", "nav:board"),
        ("Imagine", "nav:imagine"),
        ("Memory", "nav:memory"),
        ("Settings", "nav:settings"),
        ("New chat", "/new"),
        ("Doctor", "/health"),
        ("Update", "/update"),
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
        assert!(filter_palette("set").iter().any(|(l, _)| *l == "Settings"));
        assert_eq!(filter_palette("").len(), 18);
        assert!(filter_palette("").iter().all(|(l, _)| *l != "Command"));
    }

    #[test]
    fn enter_sends_and_control_enter_breaks_line() {
        assert_eq!(composer_enter(true, false), Some(ComposerEnter::Send));
        assert_eq!(composer_enter(true, true), Some(ComposerEnter::Newline));
        assert_eq!(composer_enter(false, false), None);
        assert_eq!(composer_enter(false, true), None);
        let mut buf = String::from("hi");
        assert!(apply_composer_enter(&mut buf, true, false));
        assert_eq!(buf, "hi");
        assert!(!apply_composer_enter(&mut buf, true, true));
        assert_eq!(buf, "hi\n");
        assert!(!apply_composer_enter(&mut buf, false, false));
        assert_eq!(buf, "hi\n");
    }

    #[test]
    fn composer_sheet_lists_enter_send() {
        let help = shortcut_help();
        let lines: Vec<_> = help.lines().collect();
        assert!(lines.contains(&"Enter — Send message (Composer)"));
        assert!(lines.contains(&"Ctrl+Enter — New line (Composer)"));
        assert!(!lines.contains(&"Ctrl+Enter — Send message (Composer)"));
    }

    #[test]
    fn send_becomes_stop_while_running() {
        assert_eq!(composer_go(false, false), ComposerGo::Idle);
        assert_eq!(composer_go(false, true), ComposerGo::Send);
        assert_eq!(
            composer_go(true, false),
            ComposerGo::Stop,
            "empty draft still shows Stop so you can interrupt"
        );
        assert_eq!(
            composer_go(true, true),
            ComposerGo::Stop,
            "a typed follow-up must not keep the Send glyph while Grok is answering"
        );
        assert_eq!(composer_go_tip(true), "Stop");
        assert_eq!(composer_go_tip(false), "Send");
    }
}
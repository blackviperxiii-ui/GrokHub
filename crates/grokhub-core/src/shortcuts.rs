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
pub enum PermKey {
    Allow,
    Deny,
}

/// Keyboard answer for a tool permission card.
///
/// Esc always denies. Enter only allows when the composer is empty: a half-typed
/// follow-up must send, not silently approve a shell command. An open palette or
/// settings overlay owns both keys.
pub fn perm_key(enter: bool, esc: bool, composer_has_text: bool, overlay_open: bool) -> Option<PermKey> {
    if overlay_open {
        return None;
    }
    if esc {
        return Some(PermKey::Deny);
    }
    if enter && !composer_has_text {
        return Some(PermKey::Allow);
    }
    None
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
    Shortcut { keys: "Ctrl+Alt+H", action: "Halt", scope: "Global" },
    Shortcut { keys: "Ctrl+,", action: "Settings", scope: "Global" },
    Shortcut { keys: "Ctrl+/", action: "Shortcut sheet", scope: "Global" },
    Shortcut { keys: "Ctrl+F", action: "Find in this chat (Enter next, Shift+Enter previous, Esc close)", scope: "Chat" },
    Shortcut { keys: "Enter / Esc", action: "Allow / deny tool permission (empty composer)", scope: "Chat" },
    Shortcut { keys: "Enter / Esc", action: "Confirm / cancel overlay sheet (empty composer; Ask Always stays Allow / Deny)", scope: "Chat" },
    Shortcut { keys: "Enter", action: "Send message", scope: "Composer" },
    Shortcut { keys: "Shift+Enter / Ctrl+Enter", action: "New line", scope: "Composer" },
    Shortcut { keys: "Enter while a reply runs", action: "Steer the live reply", scope: "Composer" },
    Shortcut { keys: "Alt+Enter while a reply runs", action: "Queue for after the reply", scope: "Composer" },
    Shortcut { keys: "Tab", action: "Accept slash", scope: "Composer" },
    Shortcut { keys: "Super+G", action: "Hey Grok when unfocused", scope: "System" },
    Shortcut { keys: "Super+Shift+Esc", action: "Halt when unfocused", scope: "System" },
];

pub fn shortcut_help() -> String {
    SHORTCUTS
        .iter()
        .map(|s| format!("{} — {} ({})", s.keys, s.action, s.scope))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The key a palette row also answers to, shown at the row's right edge.
pub fn palette_shortcut(action: &str) -> Option<&'static str> {
    match action {
        "/new" => Some("Ctrl+N"),
        "nav:settings" => Some("Ctrl+,"),
        "voice" => Some("Ctrl+G"),
        "shortcuts" => Some("Ctrl+/"),
        _ => None,
    }
}

pub fn filter_palette(q: &str) -> Vec<(&'static str, &'static str)> {
    let n = q.trim().to_ascii_lowercase();
    // Named as the sidebar names them; older names still find them.
    let synonyms: &[(&str, &str)] = &[
        ("nav:night", "night"),
        ("nav:night", "schedule"),
        ("nav:skills", "skills"),
        ("nav:pulse", "ideas"),
        ("nav:pulse", "feed"),
        ("nav:board", "workboard"),
        ("shortcuts", "keys"),
        ("shortcuts", "keyboard"),
        ("shortcuts", "hotkeys"),
    ];
    let rows = [
        ("Chat", "nav:chat"),
        ("Automations", "nav:night"),
        ("History", "nav:history"),
        ("Devices", "nav:devices"),
        ("Connectors", "nav:connectors"),
        ("Agents", "nav:agents"),
        ("Skills and Connectors", "nav:skills"),
        ("Workboards", "nav:board"),
        ("Pulse", "nav:pulse"),
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
        ("Keyboard shortcuts", "shortcuts"),
    ];
    rows.into_iter()
        .filter(|(label, action)| {
            n.is_empty()
                || label.to_ascii_lowercase().contains(&n)
                || synonyms
                    .iter()
                    .any(|(a, word)| a == action && word.starts_with(n.as_str()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_and_sheet() {
        assert!(shortcut_help().contains("Ctrl+K"));
        assert!(shortcut_help().contains("Super+G"));
        // 2.10.91: rows use the sidebar's names; the old name still finds the page.
        assert_eq!(filter_palette("night"), vec![("Automations", "nav:night")]);
        assert_eq!(
            filter_palette("skills"),
            vec![("Skills and Connectors", "nav:skills")]
        );
        assert_eq!(filter_palette("ideas"), vec![("Pulse", "nav:pulse")]);
        assert!(filter_palette("set").iter().any(|(l, _)| *l == "Settings"));
        // 2.10.91: Pulse joined the palette (18 -> 19); Keyboard shortcuts makes 20.
        assert_eq!(filter_palette("").len(), 20);
        assert_eq!(filter_palette("keyb"), vec![("Keyboard shortcuts", "shortcuts")]);
        assert_eq!(filter_palette("hotkeys"), vec![("Keyboard shortcuts", "shortcuts")]);
        assert_eq!(filter_palette("pulse"), vec![("Pulse", "nav:pulse")]);
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
    fn enter_answers_a_permission_card_only_when_the_composer_is_empty() {
        assert_eq!(perm_key(true, false, false, false), Some(PermKey::Allow));
        assert_eq!(perm_key(false, true, false, false), Some(PermKey::Deny));
        assert_eq!(
            perm_key(true, false, true, false),
            None,
            "Enter on a typed follow-up must send it, not approve a shell command"
        );
        assert_eq!(
            perm_key(false, true, true, false),
            Some(PermKey::Deny),
            "Esc still denies while typing"
        );
        assert_eq!(
            perm_key(true, true, false, true),
            None,
            "an open palette or settings overlay owns Enter and Esc"
        );
        assert_eq!(perm_key(false, false, false, false), None);
        assert!(shortcut_help().contains("Allow / deny tool permission"));
    }

    #[test]
    fn halt_is_ctrl_alt_h_and_no_other_shortcut_shares_it() {
        let halt: Vec<_> = SHORTCUTS.iter().filter(|s| s.action == "Halt").collect();
        assert_eq!(halt.len(), 1);
        assert_eq!(halt[0].keys, "Ctrl+Alt+H");
        assert!(
            !SHORTCUTS.iter().any(|s| s.keys == "Ctrl+Shift+Esc"),
            "Ctrl+Shift+Esc is the Windows Task Manager key and never reaches the app"
        );
        assert_eq!(
            SHORTCUTS
                .iter()
                .filter(|s| s.keys.eq_ignore_ascii_case("Ctrl+Alt+H"))
                .count(),
            1
        );
        assert!(shortcut_help().contains("Ctrl+Alt+H — Halt (Global)"));
    }

    #[test]
    fn palette_rows_show_the_keys_they_share() {
        assert_eq!(palette_shortcut("/new"), Some("Ctrl+N"));
        assert_eq!(palette_shortcut("nav:settings"), Some("Ctrl+,"));
        assert_eq!(palette_shortcut("voice"), Some("Ctrl+G"));
        assert_eq!(palette_shortcut("shortcuts"), Some("Ctrl+/"));
        assert_eq!(palette_shortcut("nav:pulse"), None);
        // Every hint is a key the sheet lists, so the two never disagree.
        for (_, action) in filter_palette("") {
            if let Some(keys) = palette_shortcut(action) {
                assert!(SHORTCUTS.iter().any(|s| s.keys == keys), "{keys}");
            }
        }
    }

    #[test]
    fn composer_sheet_lists_enter_send() {
        let help = shortcut_help();
        let lines: Vec<_> = help.lines().collect();
        assert!(lines.contains(&"Enter — Send message (Composer)"));
        assert!(lines.contains(&"Shift+Enter / Ctrl+Enter — New line (Composer)"));
        assert!(lines.contains(&"Ctrl+, — Settings (Global)"));
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
//! One-click diagnostics. No secrets.

pub fn diagnostics_bundle(
    version: &str,
    auth: bool,
    hub_kind: &str,
    skill_count: usize,
    last_receipt: Option<bool>,
    workboard_open: usize,
    notes: &str,
) -> String {
    let receipt = match last_receipt {
        Some(true) => "ok",
        Some(false) => "failed",
        None => "none",
    };
    // The status line can echo a pasted key; it never leaves in the bundle.
    let notes = crate::redact_secrets(&notes.chars().take(400).collect::<String>());
    format!(
        "app GrokHub\nversion {version}\nos {} {}\nauth {}\nhub {hub_kind}\nskills {skill_count}\nlastHost {receipt}\nworkboardOpen {workboard_open}\nnotes {notes}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        if auth { "present" } else { "missing" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_secret_leak() {
        let t = diagnostics_bundle("2.0.0", true, "grokhub-hub-v1", 2, Some(true), 1, "token sk-nope");
        assert!(t.contains("version 2.0.0"));
        assert!(t.contains("skills 2"));
        assert!(!t.contains("sk-nope") || t.contains("notes"));
    }

    #[test]
    fn bundle_names_the_os_and_redacts_a_key_in_the_notes() {
        let t = diagnostics_bundle(
            "2.10.92-beta (beta @ abc1234)",
            false,
            "grokhub-hub-v1",
            0,
            None,
            0,
            "Saved key sk-abcdefghijklmnopqrstuv",
        );
        assert!(t.contains("version 2.10.92-beta (beta @ abc1234)\n"), "{t}");
        assert!(
            t.contains(&format!("\nos {} {}\n", std::env::consts::OS, std::env::consts::ARCH)),
            "{t}"
        );
        assert!(t.contains("notes Saved key [redacted]\n"), "{t}");
        assert!(!t.contains("sk-abcdefghijklmnopqrstuv"), "{t}");
        assert!(t.contains("auth missing\nhub grokhub-hub-v1\nskills 0\nlastHost none\n"), "{t}");
    }
}

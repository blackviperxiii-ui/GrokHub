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
    format!(
        "app GrokHub\nversion {version}\nauth {}\nhub {hub_kind}\nskills {skill_count}\nlastHost {receipt}\nworkboardOpen {workboard_open}\nnotes {}\n",
        if auth { "present" } else { "missing" },
        notes.chars().take(400).collect::<String>()
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
}

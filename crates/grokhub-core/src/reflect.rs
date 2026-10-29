use crate::is_plain_text;
use crate::learning::is_durable_fact;

pub const IDLE_REFLECT_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryEdit {
    pub next: String,
    pub diff: String,
}

pub fn should_idle_reflect(idle_ms: u64, running: bool, min_ms: u64) -> bool {
    !running && idle_ms >= min_ms
}

pub fn restore_memory_prev(_current: &str, prev: &str) -> String {
    prev.to_string()
}

pub fn fact_candidates(messages: &[(String, String)]) -> Vec<String> {
    fact_candidates_from(messages.iter().map(|(r, c)| (r.as_str(), c.as_str())))
}

/// Same harvest without cloning an 8MB transcript onto the UI thread.
pub fn fact_candidates_from<'a, I>(messages: I) -> Vec<String>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    messages
        .into_iter()
        .filter(|(role, c)| {
            if *role != "user" {
                return false;
            }
            let c = c.trim();
            !c.is_empty()
                && !c.starts_with('/')
                && !c.starts_with("HOST_")
                && !c.starts_with("VERIFY_")
                && c.len() < 200
                && is_plain_text(c)
                && is_durable_fact(c)
        })
        .map(|(_, c)| c.trim().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_idle_and_fact_candidates() {
        assert_eq!(restore_memory_prev("new", "old"), "old");
        assert!(should_idle_reflect(IDLE_REFLECT_MS, false, IDLE_REFLECT_MS));
        assert!(!should_idle_reflect(IDLE_REFLECT_MS, true, IDLE_REFLECT_MS));
        assert!(!should_idle_reflect(100, false, IDLE_REFLECT_MS));
        let facts = fact_candidates(&[
            ("user".into(), "prefer nvim".into()),
            ("user".into(), "/forget".into()),
            ("user".into(), "hi how are you".into()),
            ("user".into(), "say hi in one sentence".into()),
            (
                "user".into(),
                "New direction: firefox\n\n(Previous ask: )".into(),
            ),
            ("assistant".into(), "ok".into()),
        ]);
        assert_eq!(facts, vec!["prefer nvim"]);
    }

    #[test]
    fn fact_candidates_does_not_clone_a_huge_user_turn() {
        let src = include_str!("reflect.rs");
        let facts = src
            .split("pub fn fact_candidates(")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .expect("fact_candidates");
        let clone = facts.find("to_string()").expect("fact clone");
        assert!(
            facts[..clone].contains("len()") || facts[..clone].contains("TEXT_FILE_CAP"),
            "learning must not clone an 8MB user paste before the 200-char gate: {facts}"
        );
    }
}

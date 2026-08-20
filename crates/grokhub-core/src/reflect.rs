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

/// Append each new fact once. Second run with the same additions is a no-op.
pub fn surgical_memory_edit(current: &str, additions: &[String]) -> MemoryEdit {
    let mut next = current.to_string();
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    let mut added = Vec::new();
    for raw in additions {
        let fact = raw.trim();
        if fact.is_empty() || !is_plain_text(fact) {
            continue;
        }
        let already = next.lines().any(|l| l.trim().eq_ignore_ascii_case(fact));
        if already {
            continue;
        }
        next.push_str(fact);
        next.push('\n');
        added.push(fact.to_string());
    }
    let diff = if added.is_empty() {
        String::new()
    } else {
        added
            .iter()
            .map(|f| format!("+ {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    MemoryEdit { next, diff }
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
    fn append_once_and_restore() {
        let first = surgical_memory_edit("", &["prefer nvim".into()]);
        assert!(first.next.contains("prefer nvim"));
        assert!(!first.diff.is_empty());
        let second = surgical_memory_edit(&first.next, &["prefer nvim".into()]);
        assert_eq!(second.next, first.next);
        assert!(second.diff.is_empty());
        assert_eq!(restore_memory_prev("new", "old"), "old");
        assert!(should_idle_reflect(IDLE_REFLECT_MS, false, IDLE_REFLECT_MS));
        assert!(!should_idle_reflect(IDLE_REFLECT_MS, true, IDLE_REFLECT_MS));
        assert!(!should_idle_reflect(100, false, IDLE_REFLECT_MS));
        let facts = fact_candidates(&[
            ("user".into(), "prefer nvim".into()),
            ("user".into(), "/forget".into()),
            ("user".into(), "hi how are you".into()),
            ("user".into(), "say hi in one sentence".into()),
            ("user".into(), "New direction: firefox\n\n(Previous ask: )".into()),
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

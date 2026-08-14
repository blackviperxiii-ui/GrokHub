use crate::is_plain_text;

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
    messages
        .iter()
        .filter(|(role, _)| role == "user")
        .map(|(_, c)| c.trim().to_string())
        .filter(|c| {
            !c.is_empty()
                && !c.starts_with('/')
                && !c.starts_with("HOST_")
                && !c.starts_with("VERIFY_")
                && c.len() < 200
                && is_plain_text(c)
        })
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
            ("assistant".into(), "ok".into()),
        ]);
        assert_eq!(facts, vec!["prefer nvim"]);
    }
}

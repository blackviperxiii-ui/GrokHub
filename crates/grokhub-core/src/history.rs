//! Search chats and memory. /recall stays memory-only; History is the corpus.

pub fn search_corpus(q: &str, rows: &[(String, String)]) -> Vec<String> {
    let needle = q.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    for (title, body) in rows {
        let hay = format!("{title}\n{body}").to_ascii_lowercase();
        if !hay.contains(&needle) {
            continue;
        }
        let snippet = snippet(body, &needle);
        out.push(format!("{title}: {snippet}"));
        if out.len() == 40 {
            break;
        }
    }
    out
}

fn snippet(body: &str, needle: &str) -> String {
    let lower = body.to_ascii_lowercase();
    let idx = lower.find(needle).unwrap_or(0);
    let start = idx.saturating_sub(40);
    let end = (idx + needle.len() + 60).min(body.len());
    let mut s = body[start..end].replace('\n', " ");
    if start > 0 {
        s = format!("…{s}");
    }
    s.chars().take(160).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_thread_and_memory() {
        let rows = [
            ("night".into(), "flash the pi then verify".into()),
            ("MEMORY.md".into(), "prefer nvim\nbound project is the world".into()),
        ];
        let hits = search_corpus("pi", &rows);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].starts_with("night:"));
        assert!(hits[0].contains("flash the pi"));
        let mem = search_corpus("nvim", &rows);
        assert!(mem[0].contains("MEMORY.md"));
        assert!(search_corpus("", &rows).is_empty());
    }
}

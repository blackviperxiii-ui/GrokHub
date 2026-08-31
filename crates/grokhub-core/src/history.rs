//! Search chats and memory. /recall stays memory-only; History is the corpus.

use crate::attach::TEXT_FILE_CAP;

pub fn search_corpus(q: &str, rows: &[(String, String)]) -> Vec<String> {
    let tagged: Vec<(String, String, String)> = rows
        .iter()
        .map(|(title, body)| (String::new(), title.clone(), body.clone()))
        .collect();
    search_corpus_tagged(q, &tagged)
        .into_iter()
        .map(|(_, line)| line)
        .collect()
}

/// Same search, but each row carries a caller-side id so a hit can be clicked back to
/// the thread or memory file it came from. Rows are `(tag, title, body)`.
pub fn search_corpus_tagged(q: &str, rows: &[(String, String, String)]) -> Vec<(String, String)> {
    let needle = q.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    for (tag, title, body) in rows {
        let title = search_text(title);
        let body = search_text(body);
        let hay = format!("{title}\n{body}").to_ascii_lowercase();
        if !hay.contains(&needle) {
            continue;
        }
        let snippet = snippet(body, &needle);
        out.push((tag.clone(), format!("{title}: {snippet}")));
        if out.len() == 40 {
            break;
        }
    }
    out
}

/// Keep the first sighting of each hit and the order the corpus was searched in:
/// memory lines lead, threads follow. Sorting the list alphabetically buried the
/// memory answer under every chat whose title happened to start with an "A".
pub fn dedupe_hits(hits: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for h in hits {
        if seen.insert(h.clone()) {
            out.push(h);
        }
    }
    out
}

pub fn search_thread_body<'a>(chunks: impl IntoIterator<Item = &'a str>) -> String {
    let mut body = String::new();
    for c in chunks {
        if body.len() >= TEXT_FILE_CAP {
            break;
        }
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(search_text(c));
        if body.len() > TEXT_FILE_CAP {
            let mut end = TEXT_FILE_CAP;
            while end > 0 && !body.is_char_boundary(end) {
                end -= 1;
            }
            body.truncate(end);
            break;
        }
    }
    body
}

pub fn search_text(s: &str) -> &str {
    if s.len() <= TEXT_FILE_CAP {
        return s;
    }
    let mut end = TEXT_FILE_CAP;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn snippet(body: &str, needle: &str) -> String {
    let body = search_text(body);
    let lower = body.to_ascii_lowercase();
    let idx = lower.find(needle).unwrap_or(0);
    let mut start = idx.saturating_sub(40);
    let mut end = (idx + needle.len() + 60).min(body.len());
    while start > 0 && !body.is_char_boundary(start) {
        start -= 1;
    }
    while end < body.len() && !body.is_char_boundary(end) {
        end += 1;
    }
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
        let uni = [("café".into(), "éclair matching pi here".into())];
        let hit = search_corpus("pi", &uni);
        assert_eq!(hit.len(), 1);
        assert!(hit[0].contains("éclair") || hit[0].contains("matching"), "{hit:?}");
    }

    #[test]
    fn a_hit_carries_the_source_it_came_from() {
        let rows = [
            ("mem:MEMORY.md".to_string(), "MEMORY.md".to_string(), "prefer nvim".to_string()),
            ("thread:t7".to_string(), "night".to_string(), "flash the pi then verify".to_string()),
        ];
        let hits = search_corpus_tagged("nvim", &rows);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "mem:MEMORY.md", "a hit must know where to open");
        assert!(hits[0].1.contains("prefer nvim"));
        let pi = search_corpus_tagged("pi", &rows);
        assert_eq!(pi[0].0, "thread:t7");
        assert!(search_corpus_tagged("  ", &rows).is_empty());
        assert_eq!(
            search_corpus("nvim", &[("MEMORY.md".into(), "prefer nvim".into())]),
            vec!["MEMORY.md: prefer nvim".to_string()],
            "the untagged search keeps its old shape"
        );
    }

    #[test]
    fn recall_keeps_memory_first_and_drops_repeats() {
        let hits = dedupe_hits(vec![
            "MEMORY.md:2: prefer nvim".into(),
            "alpha notes: prefer nvim here".into(),
            "MEMORY.md:2: prefer nvim".into(),
            "zeta notes: nvim again".into(),
        ]);
        assert_eq!(
            hits,
            vec![
                "MEMORY.md:2: prefer nvim".to_string(),
                "alpha notes: prefer nvim here".to_string(),
                "zeta notes: nvim again".to_string(),
            ],
            "the memory line answers the question — it must not sort under chat titles"
        );
        assert!(dedupe_hits(vec![]).is_empty());
    }

    #[test]
    fn search_does_not_scan_past_text_file_cap() {
        let mut body = "needle ".to_string();
        body.push_str(&"z".repeat(crate::attach::TEXT_FILE_CAP));
        body.push_str(" hidden-tail");
        let hits = search_corpus("hidden-tail", &[("t".into(), body)]);
        assert!(
            hits.is_empty(),
            "History search must not lowercase an 8MB thread: {hits:?}"
        );
    }
}

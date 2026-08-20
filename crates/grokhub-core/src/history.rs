//! Search chats and memory. /recall stays memory-only; History is the corpus.

use crate::attach::TEXT_FILE_CAP;

pub fn search_corpus(q: &str, rows: &[(String, String)]) -> Vec<String> {
    let needle = q.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    for (title, body) in rows {
        let title = search_text(title);
        let body = search_text(body);
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

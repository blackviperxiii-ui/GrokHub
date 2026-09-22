//! Search chats and memory. /recall stays memory-only; History is the corpus.
//! `search_place` is the palette walk: nested files under the current folder.

use std::collections::VecDeque;
use std::path::Path;

use crate::attach::TEXT_FILE_CAP;

/// How deep a palette search will descend. Deeper than this returns instead of hanging.
const SEARCH_MAX_DEPTH: u8 = 16;
/// Directory entries visited per query. A huge tree stops instead of spinning.
const SEARCH_MAX_VISITS: u32 = 4_000;
const SEARCH_MAX_HITS: usize = 40;
/// Build output is not the place being searched, and it can be millions of files.
const SEARCH_SKIP_DIRS: &[&str] = &["target", "node_modules", "dist"];

/// Files under `root` whose relative path contains `query`.
///
/// Walks nested folders. A missing root, a blank query, and a tree with no match
/// return an empty list — that empty list is a finished result. Symlinks are not
/// followed, so the walk cannot leave `root` or loop. Depth and visit caps keep
/// a deep nest from hanging.
pub fn search_place(root: &Path, query: &str) -> Vec<String> {
    let needle = query.trim().to_ascii_lowercase();
    if needle.is_empty() || root.as_os_str().is_empty() || !root.is_dir() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let mut queue = VecDeque::from([(root.to_path_buf(), String::new(), 0u8)]);
    let mut visits = 0u32;
    while let Some((dir, parent_rel, depth)) = queue.pop_front() {
        if depth > SEARCH_MAX_DEPTH || hits.len() >= SEARCH_MAX_HITS {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd.flatten() {
            visits = visits.saturating_add(1);
            if visits > SEARCH_MAX_VISITS || hits.len() >= SEARCH_MAX_HITS {
                break;
            }
            let file_name = ent.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if name.is_empty()
                || name == "."
                || name == ".."
                || name.starts_with('.')
                || name.contains('/')
                || name.contains('\\')
            {
                continue;
            }
            let path = dir.join(name);
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            // A symlink can point at a parent or another drive. Never follow one.
            if meta.file_type().is_symlink() {
                continue;
            }
            let rel = if parent_rel.is_empty() {
                name.to_string()
            } else {
                format!("{parent_rel}/{name}")
            };
            if meta.is_dir() {
                if depth < SEARCH_MAX_DEPTH && !SEARCH_SKIP_DIRS.contains(&name) {
                    queue.push_back((path, rel, depth + 1));
                }
                continue;
            }
            if meta.is_file() && rel.to_ascii_lowercase().contains(&needle) {
                hits.push(rel);
            }
        }
    }
    hits.sort();
    hits
}

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
    fn nested_file_is_found_where_a_top_level_walk_misses_it() {
        let root = scratch_dir("nested");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("nested/deep")).unwrap();
        std::fs::write(root.join("readme.txt"), "top").unwrap();
        std::fs::write(root.join("nested/deep/buried.txt"), "inside").unwrap();
        let top_names: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !top_names.iter().any(|n| n == "buried.txt"),
            "a top-level listing must not see the nested file: {top_names:?}"
        );
        let hits = search_place(&root, "buried");
        assert!(
            hits.iter().any(|h| h == "nested/deep/buried.txt"),
            "palette search must walk into nested folders: {hits:?}"
        );
        assert!(
            search_place(&root, "readme").iter().any(|h| h == "readme.txt"),
            "a file at the top of the place is still found"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_folder_and_saved_empty_result_stay_empty() {
        let missing = scratch_dir("missing");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(
            search_place(&missing, "needle").is_empty(),
            "a missing folder is an empty result, not a panic"
        );
        let root = scratch_dir("empty");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("blank.txt"), "").unwrap();
        let saved = search_place(&root, "no-such-needle");
        assert!(saved.is_empty(), "no match is a saved empty result");
        assert_eq!(search_place(&root, "no-such-needle"), saved);
        assert!(search_place(&root, "   ").is_empty());
        assert!(search_place(Path::new(""), "blank").is_empty());
        assert!(
            search_place(&root, "blank").iter().any(|h| h == "blank.txt"),
            "an empty file is still a file"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deep_nest_does_not_hang_or_leave_the_root() {
        let root = scratch_dir("deep");
        let _ = std::fs::remove_dir_all(&root);
        let mut deep = root.clone();
        for _ in 0..48 {
            deep.push("d");
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("too-deep.txt"), "x").unwrap();
        std::fs::create_dir_all(root.join("nested/deep")).unwrap();
        std::fs::write(root.join("nested/deep/buried.txt"), "x").unwrap();
        let started = std::time::Instant::now();
        let hits = search_place(&root, "buried");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "a deep nest must return"
        );
        assert!(
            hits.iter().any(|h| h == "nested/deep/buried.txt"),
            "a file above the depth cap is still found: {hits:?}"
        );
        assert!(
            hits.iter().all(|h| !h.contains("..") && !h.starts_with('/') && !h.starts_with('\\')),
            "hits stay relative to the search root: {hits:?}"
        );
        assert!(
            search_place(&root, "too-deep").is_empty(),
            "past the depth cap the walk stops instead of hanging"
        );
        let outside = scratch_dir("outside");
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret-needle.txt"), "nope").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(&outside, root.join("escape")).unwrap();
            symlink(outside.join("secret-needle.txt"), root.join("secret-needle.txt")).unwrap();
        }
        let leaked = search_place(&root, "secret-needle");
        assert!(
            leaked.is_empty(),
            "the walk must not leave the search root: {leaked:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    fn scratch_dir(label: &str) -> std::path::PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("grokhub-search-{label}-{}-{n}", std::process::id()))
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

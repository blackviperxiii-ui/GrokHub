//! Guard for "no primary AI provider": xAI is one source among many, so code
//! may reach it directly only in the places listed here.
//!
//! The scan reads `crates/` and `src-tauri/src` and counts direct xAI use: a
//! `XaiClient::` call (building or calling the native xAI client by name) and
//! the `api.x.ai` host. A file whose count doesn't match its [`ALLOWED`] row
//! fails, so a new direct xAI path can't slip in: route it through the shared
//! router or a capability adapter instead. Comment lines don't count. Test
//! modules (`#[cfg(test)] mod … { }` blocks, `tests.rs`, `*_tests.rs`,
//! `tests/`) are skipped.
//!
//! The list should only shrink as each capability moves behind an adapter.
//!
//! Hermetic: reads files under the repo root only (CRLF normalized).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// What counts as reaching xAI directly.
const DIRECT: &[&str] = &["XaiClient::", "api.x.ai"];

/// Files allowed to reach xAI directly: path, count, and why.
const ALLOWED: &[(&str, usize, &str)] = &[
    (
        "crates/grokhub-agent/examples/live_eval.rs",
        2,
        "dev eval tool",
    ),
    (
        "crates/grokhub-agent/src/client.rs",
        4,
        "the xAI chat adapter",
    ),
    (
        "crates/grokhub-agent/src/harness/egress_coverage.rs",
        9,
        "egress scanner names the host",
    ),
    (
        "crates/grokhub-agent/src/models.rs",
        1,
        "xAI model list adapter",
    ),
    (
        "crates/grokhub-agent/src/route/sources.rs",
        2,
        "model source list: xAI is one source",
    ),
    (
        "crates/grokhub-agent/src/sandbox/mask.rs",
        1,
        "key-to-host masking rule",
    ),
    (
        "crates/grokhub-agent/src/ws_mode.rs",
        1,
        "xAI websocket chat adapter",
    ),
    (
        "crates/grokhub-agent/src/eval/mod.rs",
        1,
        "opt-in --live eval suite (GROKHUB_EVAL_API_KEY), never in default runs",
    ),
    ("crates/grokhub-core/src/chat.rs", 1, "xAI base URL"),
    (
        "crates/grokhub-core/src/connector.rs",
        1,
        "connector host allow list",
    ),
    (
        "crates/grokhub-core/src/imagine.rs",
        1,
        "Imagine adapter's network error text",
    ),
    (
        "crates/grokhub-core/src/stt_stream.rs",
        1,
        "xAI speech-to-text adapter",
    ),
    (
        "crates/grokhub-core/src/voice.rs",
        1,
        "xAI realtime voice adapter",
    ),
    (
        "src-tauri/src/lib.rs",
        5,
        "builds the xAI chat client for the router",
    ),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// The part of a source file that ships, without comment lines: CRLF
/// normalized, each `#[cfg(test)] mod name { … }` block cut out (the code
/// after it stays).
fn production_text(raw: &str) -> String {
    let text = raw.replace("\r\n", "\n");
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        let next = lines.get(i + 1).map(|l| l.trim_start()).unwrap_or("");
        let decl = next
            .trim_start_matches("pub(crate) ")
            .trim_start_matches("pub(super) ")
            .trim_start_matches("pub ");
        let opens_mod = decl.starts_with("mod ") && next.trim_end().ends_with('{');
        if lines[i].trim() == "#[cfg(test)]" && opens_mod {
            let mut depth: i64 = 0;
            i += 1;
            while i < lines.len() {
                depth += lines[i].matches('{').count() as i64;
                depth -= lines[i].matches('}').count() as i64;
                i += 1;
                if depth <= 0 {
                    break;
                }
            }
            continue;
        }
        if !lines[i].trim_start().starts_with("//") {
            out.push_str(lines[i]);
            out.push('\n');
        }
        i += 1;
    }
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "tests" | "target" | "fixtures") {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
            out.push(path);
        }
    }
}

/// Direct xAI uses per file under `roots`, keyed by the path from `root`.
fn scan(root: &Path, roots: &[&str]) -> BTreeMap<String, usize> {
    let mut files = Vec::new();
    for sub in roots {
        rust_files(&root.join(sub), &mut files);
    }
    let mut found = BTreeMap::new();
    for path in files {
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let key = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let text = production_text(&raw);
        let n: usize = DIRECT.iter().map(|p| text.matches(p).count()).sum();
        if n > 0 {
            found.insert(key, n);
        }
    }
    found
}

fn not_allowed(found: &BTreeMap<String, usize>, allowed: &[(&str, usize, &str)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (path, n) in found {
        match allowed.iter().find(|(p, _, _)| p == path) {
            None => bad.push(format!(
                "{path}: {n} direct xAI use(s); go through the router or an adapter"
            )),
            Some((_, listed, _)) if listed != n => {
                bad.push(format!("{path}: {n} direct xAI use(s), {listed} allowed"))
            }
            Some(_) => {}
        }
    }
    for (path, _, _) in allowed {
        if !found.contains_key(*path) {
            bad.push(format!(
                "{path}: allowed but no longer reaches xAI (drop the row)"
            ));
        }
    }
    bad
}

#[test]
fn xai_is_reached_directly_only_where_listed() {
    let found = scan(&repo_root(), &["crates", "src-tauri/src"]);
    assert_eq!(not_allowed(&found, ALLOWED), Vec::<String>::new());
}

#[test]
fn a_new_direct_xai_use_fails_and_comments_and_tests_do_not_count() {
    let root = std::env::temp_dir().join(format!("xai-guard-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let src = root.join("crates").join("demo").join("src");
    fs::create_dir_all(&src).unwrap();
    let fixture = "//! Talks to https://api.x.ai/v1 in prose only.\r\n\
                   #[cfg(test)]\r\nmod tests {\r\n    fn t() { let _ = XaiClient::new(k); }\r\n}\r\n\
                   pub fn ask() {\r\n    let _ = XaiClient::new(key);\r\n    \
                   let _ = get(\"https://api.x.ai/v1/models\");\r\n}\r\n";
    fs::write(src.join("feed.rs"), fixture).unwrap();
    fs::write(
        src.join("feed_tests.rs"),
        "fn t() { let _ = XaiClient::new(k); }\n",
    )
    .unwrap();
    let found = scan(&root, &["crates"]);
    assert_eq!(
        found,
        BTreeMap::from([("crates/demo/src/feed.rs".to_string(), 2)])
    );
    assert_eq!(
        not_allowed(&found, &[]),
        vec![
            "crates/demo/src/feed.rs: 2 direct xAI use(s); go through the router or an adapter"
                .to_string()
        ]
    );
    assert_eq!(
        not_allowed(&found, &[("crates/demo/src/feed.rs", 2, "adapter")]),
        Vec::<String>::new()
    );
    assert_eq!(
        not_allowed(
            &BTreeMap::new(),
            &[("crates/demo/src/gone.rs", 1, "adapter")]
        ),
        vec![
            "crates/demo/src/gone.rs: allowed but no longer reaches xAI (drop the row)".to_string()
        ]
    );
    let _ = fs::remove_dir_all(root);
}

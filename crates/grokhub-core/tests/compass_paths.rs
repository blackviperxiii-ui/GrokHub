//! Staleness check for the compass files in `docs/compass/`.
//!
//! Each compass file is a short map an agent reads before editing a module. A
//! map that points at a moved file is worse than no map, so this test fails when:
//! - a backtick-quoted repo path (first segment `crates/`, `scripts/`, `docs/`,
//!   `packaging/`, `.cursor/`, `.github/`, ... or a root file such as
//!   `AGENTS.md`) no longer exists,
//! - a backtick-quoted Rust-style identifier (`snake_case`, `CamelCase`,
//!   `SCREAMING_CASE`, or `a::b`) no longer appears anywhere in the tree,
//! - a relative markdown link does not resolve,
//! - a compass file drops a required section or leaves 25..=35 lines,
//! - `docs/compass/README.md` does not link every compass file.
//!
//! Hermetic: reads files under the repo root only. No git, no network, no env.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// First path segments that make a backtick span a repo path.
const REPO_DIRS: &[&str] = &[
    "crates",
    "scripts",
    "docs",
    "packaging",
    "research",
    "screenshots",
    ".cursor",
    ".github",
];

/// Root files a compass may name without a directory.
const ROOT_FILES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "README.md",
    "LICENSE",
    "VERSION",
    "Cargo.toml",
    "Cargo.lock",
    "clippy.toml",
    "rust-toolchain.toml",
];

/// Every compass file carries these headings, in this order.
const SECTIONS: &[&str] = &[
    "## Owns",
    "## Quick commands",
    "## Key files",
    "## Change recipe",
    "## What breaks it",
    "## What depends on it",
    "## Non-obvious",
    "## See also",
];

/// "Compass, not encyclopedia": 25 to 35 lines each.
const MIN_LINES: usize = 25;
const MAX_LINES: usize = 35;

/// Text files the identifier corpus reads. Files without an extension
/// (`PKGBUILD`, `VERSION`, `.SRCINFO`) are read too.
const TEXT_EXTS: &[&str] = &[
    "rs", "md", "mdc", "toml", "lock", "sh", "ps1", "py", "json", "yml", "yaml", "wgsl", "h",
    "iss", "desktop", "service", "rules", "install", "txt",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn compass_dir(root: &Path) -> PathBuf {
    root.join("docs").join("compass")
}

/// What a backtick span names.
#[derive(Debug, PartialEq, Eq)]
enum Ref {
    Path(String),
    Ident(String),
}

/// Backtick spans outside fenced blocks.
fn code_spans(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for (i, part) in line.split('`').enumerate() {
            if i % 2 == 1 {
                out.push(part.to_string());
            }
        }
    }
    out
}

fn is_ident_segment(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn classify(span: &str) -> Option<Ref> {
    let s = span.trim();
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return None;
    }
    let first = s.split('/').next().unwrap_or("");
    if (s.contains('/') && REPO_DIRS.contains(&first)) || ROOT_FILES.contains(&s) {
        return Some(Ref::Path(s.to_string()));
    }
    let bare = s
        .strip_suffix("()")
        .or_else(|| s.strip_suffix('!'))
        .unwrap_or(s);
    if !bare.split("::").all(is_ident_segment) {
        return None;
    }
    // Plain lowercase words (`beta`, `legacy`) are prose, not identifiers.
    let shaped = bare.contains("::")
        || bare.contains('_')
        || bare.chars().skip(1).any(|c| c.is_ascii_uppercase());
    shaped.then(|| Ref::Ident(bare.to_string()))
}

/// Relative markdown link targets: `[text](target)`.
fn link_targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        rest = &rest[i + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = rest[..end].trim();
        let external = target.contains("://") || target.starts_with('#') || target.starts_with("mailto:");
        if !target.is_empty() && !external {
            out.push(target.split('#').next().unwrap_or(target).to_string());
        }
        rest = &rest[end..];
    }
    out
}

fn tokenize_into(text: &str, words: &mut HashSet<String>) {
    for w in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if !w.is_empty() && !words.contains(w) {
            words.insert(w.to_string());
        }
    }
}

fn walk(dir: &Path, skip: &[PathBuf], words: &mut HashSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if skip.iter().any(|s| s == &path) {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            walk(&path, skip, words);
            continue;
        }
        let text_like = match path.extension().and_then(|e| e.to_str()) {
            None => true,
            Some(ext) => TEXT_EXTS.contains(&ext),
        };
        if !text_like {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&path) {
            tokenize_into(&text, words);
        }
    }
}

/// Every identifier-like word in the tree, minus the compass files and this test.
fn corpus(root: &Path) -> HashSet<String> {
    let skip = [
        compass_dir(root),
        root.join("crates/grokhub-core/tests/compass_paths.rs"),
        root.join("target"),
    ];
    let mut words = HashSet::new();
    for dir in REPO_DIRS {
        walk(&root.join(dir), &skip, &mut words);
    }
    for file in ROOT_FILES {
        if let Ok(text) = fs::read_to_string(root.join(file)) {
            tokenize_into(&text, &mut words);
        }
    }
    words
}

/// Problems with one compass file's references. `dir` resolves relative links.
fn reference_problems(
    name: &str,
    text: &str,
    root: &Path,
    dir: &Path,
    words: &HashSet<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for span in code_spans(text) {
        match classify(&span) {
            Some(Ref::Path(p)) => {
                if p.contains(['<', '>', '*', '{', '}', '$']) {
                    out.push(format!("{name}: `{p}` is a pattern; name a concrete repo path"));
                } else if !root.join(&p).exists() {
                    out.push(format!("{name}: repo path `{p}` does not exist"));
                }
            }
            Some(Ref::Ident(id)) => {
                for seg in id.split("::") {
                    if !words.contains(seg) {
                        out.push(format!("{name}: identifier `{id}` (`{seg}`) is not in the tree"));
                    }
                }
            }
            None => {}
        }
    }
    for target in link_targets(text) {
        if !dir.join(&target).exists() {
            out.push(format!("{name}: link ({target}) does not resolve"));
        }
    }
    out
}

/// Shape problems: required sections in order and the 25..=35 line budget.
fn shape_problems(name: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let count = text.lines().count();
    if !(MIN_LINES..=MAX_LINES).contains(&count) {
        out.push(format!("{name}: {count} lines; keep compass files {MIN_LINES}..={MAX_LINES}"));
    }
    if !text.starts_with("# Compass: ") {
        out.push(format!("{name}: first line must be `# Compass: <name>`"));
    }
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut next = 0;
    for section in SECTIONS {
        match lines[next..].iter().position(|l| l == section) {
            Some(i) => next += i + 1,
            None => out.push(format!("{name}: missing `{section}` (or out of order)")),
        }
    }
    out
}

fn compass_files(root: &Path) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = fs::read_dir(compass_dir(root))
        .expect("docs/compass exists")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let text = fs::read_to_string(&p).expect("compass file is UTF-8");
            (name, text)
        })
        .collect();
    files.sort();
    files
}

#[test]
fn compass_files_point_at_real_paths() {
    let root = repo_root();
    let dir = compass_dir(&root);
    let words = corpus(&root);
    let files = compass_files(&root);
    let index = files
        .iter()
        .find(|(n, _)| n == "README.md")
        .map(|(_, t)| t.clone())
        .expect("docs/compass/README.md is the index");
    let mut problems = Vec::new();
    let mut maps = 0;
    for (name, text) in &files {
        problems.extend(reference_problems(name, text, &root, &dir, &words));
        if name == "README.md" {
            continue;
        }
        maps += 1;
        problems.extend(shape_problems(name, text));
        let paths = code_spans(text)
            .iter()
            .filter(|s| matches!(classify(s), Some(Ref::Path(_))))
            .count();
        if paths < 3 {
            problems.push(format!("{name}: names {paths} repo paths; a compass needs its 3-5 key files"));
        }
        if !link_targets(&index).iter().any(|t| t == name) {
            problems.push(format!("README.md: does not link {name}"));
        }
    }
    assert!(maps >= 10, "expected at least 10 compass files, found {maps}");
    assert!(
        problems.is_empty(),
        "stale compass files ({}):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn a_moved_file_or_renamed_identifier_is_reported() {
    let root = repo_root();
    let dir = compass_dir(&root);
    let mut words = HashSet::new();
    tokenize_into("pub fn load_json() {}\npub struct AppConfig;", &mut words);
    let text = "- `crates/grokhub-app/src/config.rs` owns `load_json` and `AppConfig`.\n\
                - `crates/grokhub-app/src/konfig.rs` was renamed.\n\
                - `load_json_v2` and `grokhub_core::NoSuchThing` are gone.\n\
                - See [the index](README.md) and [old](gone.md).\n";
    let problems = reference_problems("fixture.md", text, &root, &dir, &words);
    assert_eq!(
        problems,
        vec![
            "fixture.md: repo path `crates/grokhub-app/src/konfig.rs` does not exist".to_string(),
            "fixture.md: identifier `load_json_v2` (`load_json_v2`) is not in the tree".to_string(),
            "fixture.md: identifier `grokhub_core::NoSuchThing` (`grokhub_core`) is not in the tree".to_string(),
            "fixture.md: identifier `grokhub_core::NoSuchThing` (`NoSuchThing`) is not in the tree".to_string(),
            "fixture.md: link (gone.md) does not resolve".to_string(),
        ]
    );
}

#[test]
fn prose_commands_and_home_paths_are_not_repo_paths() {
    assert_eq!(classify("~/.grok/auth.json"), None);
    assert_eq!(classify("{config}/amr/"), None);
    assert_eq!(classify("cargo test -p grokhub-core"), None);
    assert_eq!(classify("--mcp-desktop"), None);
    assert_eq!(classify("legacy"), None);
    assert_eq!(classify("app.json"), None);
    assert_eq!(classify("CLAUDE.md"), Some(Ref::Path("CLAUDE.md".into())));
    assert_eq!(classify("scripts/install.sh"), Some(Ref::Path("scripts/install.sh".into())));
    assert_eq!(classify("parse_slash()"), Some(Ref::Ident("parse_slash".into())));
    assert_eq!(classify("include_str!"), Some(Ref::Ident("include_str".into())));
    assert_eq!(classify("harness::decide"), Some(Ref::Ident("harness::decide".into())));
    let root = repo_root();
    let problems = reference_problems(
        "fixture.md",
        "`crates/<crate>/src/lib.rs`",
        &root,
        &compass_dir(&root),
        &HashSet::new(),
    );
    assert_eq!(
        problems,
        vec!["fixture.md: `crates/<crate>/src/lib.rs` is a pattern; name a concrete repo path".to_string()]
    );
}

#[test]
fn shape_rules_catch_a_bloated_or_unsectioned_file() {
    let short = "# Compass: x\n## Owns\n- one\n";
    let problems = shape_problems("x.md", short);
    assert_eq!(problems[0], "x.md: 3 lines; keep compass files 25..=35");
    assert!(problems.contains(&"x.md: missing `## Quick commands` (or out of order)".to_string()));
    assert!(problems.contains(&"x.md: missing `## See also` (or out of order)".to_string()));
    assert_eq!(problems.len(), 1 + SECTIONS.len() - 1);
}

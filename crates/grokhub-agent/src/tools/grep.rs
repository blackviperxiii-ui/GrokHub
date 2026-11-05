use std::path::Path;

use serde_json::{json, Value};

use super::{confine, str_field, u32_field, ToolOutput};

const MAX_HITS: usize = 200;
/// One output line is cut here, as `read_file` cuts its lines.
const LINE_CHARS: usize = 2_000;
/// The whole answer stays under this many bytes, like an MCP tool's output.
const OUTPUT_BYTES: usize = 40_000;
/// Bigger files are skipped: logs and dumps, not source.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "grep",
        "description": "Search workspace text with a regex. Skips gitignored files. Max 200 lines; output_mode files or count says only where.",
        "parameters": {
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string"},
                "glob": {"type": "string"},
                "-i": {"type": "boolean", "description": "Ignore case."},
                "output_mode": {"type": "string", "enum": ["content", "files", "count"]},
                "-A": {"type": "integer", "description": "Lines of context after a hit."},
                "-B": {"type": "integer", "description": "Lines of context before a hit."}
            },
            "required": ["pattern"],
            "additionalProperties": false
        }
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Content,
    Files,
    Count,
}

pub fn run(workspace: &Path, args: &Value) -> ToolOutput {
    let pattern = str_field(args, "pattern");
    if pattern.is_empty() {
        return ToolOutput::err("pattern is required");
    }
    let mode = match str_field(args, "output_mode").to_ascii_lowercase().as_str() {
        "" | "content" => Mode::Content,
        "files" | "files_with_matches" | "files_only" => Mode::Files,
        "count" => Mode::Count,
        other => {
            return ToolOutput::err(format!(
                "output_mode {other} is not one of content, files, count"
            ))
        }
    };
    let ignore_case = args.get("-i").and_then(Value::as_bool).unwrap_or(false);
    let re = match regex::RegexBuilder::new(&pattern)
        .case_insensitive(ignore_case)
        .build()
    {
        Ok(re) => re,
        Err(e) => return ToolOutput::err(format!("regex: {e}")),
    };
    let root = match confine(workspace, &str_field(args, "path")) {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(e),
    };
    let glob = match compile_glob(&str_field(args, "glob")) {
        Ok(g) => g,
        Err(e) => return ToolOutput::err(e),
    };
    let before = u32_field(args, "-B").unwrap_or(0).min(20) as usize;
    let after = u32_field(args, "-A").unwrap_or(0).min(20) as usize;
    let workspace_c = match workspace.canonicalize() {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(e.to_string()),
    };
    let mut hits = Vec::new();
    let mut bytes = 0usize;
    let mut skipped_big = 0usize;
    // Sorted, so the same search gives the same bytes (and the same cache).
    let walker = ignore::WalkBuilder::new(&root)
        .standard_filters(false)
        .follow_links(false)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false)
        .hidden(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|ent| ent.file_name() != ".git")
        .build();
    let mut push = |hits: &mut Vec<String>, line: String| -> bool {
        bytes += line.len() + 1;
        hits.push(line);
        hits.len() < MAX_HITS && bytes < OUTPUT_BYTES
    };
    let mut room = true;
    for ent in walker.flatten() {
        if !room {
            break;
        }
        if !ent.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = ent.path();
        if let Some(glob) = &glob {
            let rel = path.strip_prefix(&workspace_c).unwrap_or(path);
            if !glob.is_match(rel) {
                continue;
            }
        }
        if ent.metadata().is_ok_and(|meta| meta.len() > MAX_FILE_BYTES) {
            skipped_big += 1;
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if text.as_bytes().iter().take(8_192).any(|&b| b == 0) {
            continue;
        }
        let lines: Vec<&str> = text.lines().collect();
        let rel = path
            .strip_prefix(&workspace_c)
            .unwrap_or(path)
            .display()
            .to_string()
            .replace('\\', "/");
        match mode {
            Mode::Files => {
                if lines.iter().any(|line| re.is_match(line)) {
                    room = push(&mut hits, rel);
                }
                continue;
            }
            Mode::Count => {
                let n = lines.iter().filter(|line| re.is_match(line)).count();
                if n > 0 {
                    room = push(&mut hits, format!("{rel}:{n}"));
                }
                continue;
            }
            Mode::Content => {}
        }
        // Context windows that touch print each line once.
        let mut printed_to = 0usize;
        for (i, line) in lines.iter().enumerate() {
            if !re.is_match(line) {
                continue;
            }
            let from = i.saturating_sub(before).max(printed_to);
            let to = (i + after + 1).min(lines.len());
            for (n, row) in lines[from..to].iter().enumerate() {
                room = push(
                    &mut hits,
                    format!("{rel}:{}:{}", from + n + 1, clip_line(row)),
                );
                if !room {
                    break;
                }
            }
            printed_to = printed_to.max(to);
            if !room {
                break;
            }
        }
    }
    let mut text = if hits.is_empty() {
        "(no matches)".to_string()
    } else {
        hits.join("\n")
    };
    if !room {
        text.push_str(&format!(
            "\n(stopped at {} lines: narrow it with path or glob, or use output_mode files)",
            hits.len()
        ));
    }
    if skipped_big > 0 {
        let noun = if skipped_big == 1 { "file" } else { "files" };
        text.push_str(&format!(
            "\n(skipped {skipped_big} {noun} over 4 MB: read_file can page through one)"
        ));
    }
    ToolOutput::ok(text)
}

/// A line cut to [`LINE_CHARS`], saying how much was left off.
fn clip_line(line: &str) -> std::borrow::Cow<'_, str> {
    let count = line.chars().count();
    if count <= LINE_CHARS {
        return std::borrow::Cow::Borrowed(line);
    }
    let kept: String = line.chars().take(LINE_CHARS).collect();
    std::borrow::Cow::Owned(format!("{kept}…[+{} chars]", count - LINE_CHARS))
}

fn compile_glob(pat: &str) -> Result<Option<globset::GlobMatcher>, String> {
    if pat.is_empty() {
        return Ok(None);
    }
    let glob = globset::GlobBuilder::new(pat)
        .literal_separator(false)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(Some(glob.compile_matcher()))
}

#[cfg(test)]
mod tests {
    use crate::tools::execute;

    #[test]
    fn grep_respects_gitignore_glob_and_context() {
        let dir = std::env::temp_dir().join(format!("gh-grep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join(".gitignore"), "secret.txt\n").unwrap();
        std::fs::write(dir.join("secret.txt"), "needle hidden\n").unwrap();
        std::fs::write(dir.join("src/a.rs"), "alpha\nneedle here\nomega\n").unwrap();
        std::fs::write(dir.join("src/b.txt"), "needle text\n").unwrap();
        let out = execute(
            &dir,
            "grep",
            r#"{"pattern":"needle","glob":"*.rs","-A":1,"-B":1}"#,
        );
        assert!(!out.failed, "{}", out.text);
        assert!(out.text.contains("src/a.rs"), "{}", out.text);
        assert!(out.text.contains("alpha"), "{}", out.text);
        assert!(out.text.contains("omega"), "{}", out.text);
        assert!(!out.text.contains("secret.txt"), "{}", out.text);
        assert!(!out.text.contains("b.txt"), "{}", out.text);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grep_can_ignore_case_and_answer_with_files_or_counts() {
        let dir = std::env::temp_dir().join(format!("gh-grep-modes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "Needle one\nneedle two\nhay\n").unwrap();
        std::fs::write(dir.join("src/b.rs"), "NEEDLE\n").unwrap();
        std::fs::write(dir.join("src/c.rs"), "hay only\n").unwrap();

        let out = execute(&dir, "grep", r#"{"pattern":"needle"}"#);
        assert!(out.text.contains("src/a.rs:2:needle two"), "{}", out.text);
        assert!(!out.text.contains("Needle one"), "case matters by default");

        let out = execute(&dir, "grep", r#"{"pattern":"needle","-i":true}"#);
        assert!(out.text.contains("src/a.rs:1:Needle one"), "{}", out.text);
        assert!(out.text.contains("src/b.rs:1:NEEDLE"), "{}", out.text);

        let out = execute(
            &dir,
            "grep",
            r#"{"pattern":"needle","-i":true,"output_mode":"files"}"#,
        );
        let mut files: Vec<&str> = out.text.lines().collect();
        files.sort_unstable();
        assert_eq!(files, ["src/a.rs", "src/b.rs"]);

        let out = execute(
            &dir,
            "grep",
            r#"{"pattern":"needle","-i":true,"output_mode":"count"}"#,
        );
        let mut counts: Vec<&str> = out.text.lines().collect();
        counts.sort_unstable();
        assert_eq!(counts, ["src/a.rs:2", "src/b.rs:1"]);

        let out = execute(&dir, "grep", r#"{"pattern":"x","output_mode":"lines"}"#);
        assert!(out.failed, "an unknown mode is an error");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grep_says_when_it_stopped_at_the_cap() {
        let dir = std::env::temp_dir().join(format!("gh-grep-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("big.txt"), "hit\n".repeat(500)).unwrap();
        let out = execute(&dir, "grep", r#"{"pattern":"hit"}"#);
        assert!(out.text.contains("stopped at 200 lines"), "{}", out.text);
        let out = execute(&dir, "grep", r#"{"pattern":"hit","output_mode":"count"}"#);
        assert_eq!(out.text, "big.txt:500");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grep_output_is_bounded_sorted_and_skips_git() {
        let dir = std::env::temp_dir().join(format!("gh-grep-bound-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join(".git/HEAD"), "needle in git\n").unwrap();
        std::fs::write(
            dir.join("min.js"),
            format!("needle{}\n", "x".repeat(1_000_000)),
        )
        .unwrap();
        std::fs::write(dir.join("src/z.rs"), "needle z\n").unwrap();
        std::fs::write(dir.join("src/a.rs"), "needle a\n").unwrap();

        let out = execute(&dir, "grep", r#"{"pattern":"needle"}"#);
        assert!(!out.text.contains(".git"), "{}", out.text);
        assert!(
            out.text.len() < 5_000,
            "a one-line file can't flood: {}",
            out.text.len()
        );
        assert!(out.text.contains("…[+"), "the long line says it was cut");
        let again = execute(&dir, "grep", r#"{"pattern":"needle"}"#);
        assert_eq!(out.text, again.text, "the same search gives the same bytes");
        let a = out.text.find("src/a.rs").unwrap();
        let z = out.text.find("src/z.rs").unwrap();
        assert!(a < z, "sorted by path: {}", out.text);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn touching_context_windows_print_each_line_once() {
        let dir = std::env::temp_dir().join(format!("gh-grep-ctx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "1\nhit\n3\nhit\n5\n").unwrap();
        let out = execute(&dir, "grep", r#"{"pattern":"hit","-A":3,"-B":3}"#);
        let lines: Vec<&str> = out.text.lines().collect();
        assert_eq!(
            lines,
            [
                "a.txt:1:1",
                "a.txt:2:hit",
                "a.txt:3:3",
                "a.txt:4:hit",
                "a.txt:5:5"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

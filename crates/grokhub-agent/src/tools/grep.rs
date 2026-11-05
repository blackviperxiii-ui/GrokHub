use std::path::Path;

use serde_json::{json, Value};

use super::{confine, str_field, u32_field, ToolOutput};

const MAX_HITS: usize = 200;

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
    let walker = ignore::WalkBuilder::new(&root)
        .standard_filters(false)
        .follow_links(false)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false)
        .hidden(false)
        .build();
    for ent in walker.flatten() {
        if hits.len() >= MAX_HITS {
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
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        let rel = path
            .strip_prefix(&workspace_c)
            .unwrap_or(path)
            .display()
            .to_string()
            .replace('\\', "/");
        if mode != Mode::Content {
            let n = lines.iter().filter(|line| re.is_match(line)).count();
            match (mode, n) {
                (_, 0) => {}
                (Mode::Files, _) => hits.push(rel),
                _ => hits.push(format!("{rel}:{n}")),
            }
            continue;
        }
        for (i, line) in lines.iter().enumerate() {
            if !re.is_match(line) {
                continue;
            }
            let from = i.saturating_sub(before);
            let to = (i + after + 1).min(lines.len());
            for (n, row) in lines[from..to].iter().enumerate() {
                hits.push(format!("{rel}:{}:{row}", from + n + 1));
            }
            if hits.len() >= MAX_HITS {
                break;
            }
        }
    }
    if hits.is_empty() {
        return ToolOutput::ok("(no matches)");
    }
    let full = hits.len() >= MAX_HITS;
    let mut text = hits.join("\n");
    if full {
        text.push_str(&format!(
            "\n(stopped at {MAX_HITS} lines: narrow it with path or glob, or use output_mode files)"
        ));
    }
    ToolOutput::ok(text)
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
}

use std::path::Path;

use regex::Regex;
use serde_json::{json, Value};

use super::{confine, str_field, u32_field, ToolOutput};

const MAX_HITS: usize = 200;

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "grep",
        "description": "Search workspace text with a regex. Skips gitignored files. Supports path, glob, -A, and -B.",
        "parameters": {
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string"},
                "glob": {"type": "string"},
                "-A": {"type": "integer", "description": "Lines of context after a hit."},
                "-B": {"type": "integer", "description": "Lines of context before a hit."}
            },
            "required": ["pattern"],
            "additionalProperties": false
        }
    })
}

pub fn run(workspace: &Path, args: &Value) -> ToolOutput {
    let pattern = str_field(args, "pattern");
    if pattern.is_empty() {
        return ToolOutput::err("pattern is required");
    }
    let re = match Regex::new(&pattern) {
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
            .to_string();
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
        ToolOutput::ok("(no matches)")
    } else {
        ToolOutput::ok(hits.join("\n"))
    }
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
}

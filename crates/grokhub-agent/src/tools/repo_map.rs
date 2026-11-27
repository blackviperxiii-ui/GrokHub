use std::path::Path;

use serde_json::{json, Value};

use crate::repo_map::{self, RepoMapConfig};

use super::{confine, str_field, u32_field, ToolOutput};

pub const NAME: &str = "repo_map";

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": NAME,
        "description": "Return a ranked tree-sitter map of symbols in the workspace within a token budget.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional directory relative to the workspace. Empty means the workspace root."
                },
                "focus_files": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional workspace-relative file paths to boost in ranking."
                },
                "query": {
                    "type": "string",
                    "description": "Optional symbol or task keywords to boost matching files and symbols."
                },
                "token_budget": {
                    "type": "integer",
                    "description": "Approximate token budget (64..=8192). Defaults to 1024."
                }
            },
            "additionalProperties": false
        }
    })
}

pub fn run(workspace: &Path, args: &Value) -> ToolOutput {
    let raw_path = str_field(args, "path");
    let root = match confine(workspace, if raw_path.is_empty() { "." } else { &raw_path }) {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(e),
    };
    if !root.is_dir() {
        return ToolOutput::err(format!("path is not a directory: {}", root.display()));
    }

    let focus_files = args
        .get("focus_files")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(str::trim))
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let query = str_field(args, "query");
    let (hint_files, query_terms) = repo_map::extract_prompt_hints(&query);
    let mut combined_focus = focus_files;
    for f in hint_files {
        if !combined_focus.contains(&f) {
            combined_focus.push(f);
        }
    }

    let token_budget = u32_field(args, "token_budget")
        .map(|n| n as usize)
        .unwrap_or(repo_map::DEFAULT_TOKEN_BUDGET);

    let map = repo_map::build(
        &root,
        &RepoMapConfig {
            token_budget,
            focus_files: combined_focus,
            query_terms,
            ..Default::default()
        },
    );

    if map.rendered.is_empty() {
        return ToolOutput::ok("(no supported source symbols found)");
    }

    ToolOutput::ok(map.rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_map_tool_renders_and_blocks_path_escape() {
        let dir = std::env::temp_dir().join(format!("grokhub-repomap-tool-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn alpha_entry() -> i32 { 42 }\n",
        )
        .unwrap();

        let ok = run(&dir, &json!({"query": "alpha_entry", "token_budget": 512}));
        assert!(!ok.failed, "{}", ok.text);
        assert!(ok.text.contains("alpha_entry"), "{}", ok.text);

        let escaped = run(&dir, &json!({"path": "../outside"}));
        assert!(escaped.failed);
        assert!(escaped.text.contains("workspace"), "{}", escaped.text);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

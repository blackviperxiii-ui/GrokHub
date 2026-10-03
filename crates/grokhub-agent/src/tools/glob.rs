use std::path::Path;

use serde_json::{json, Value};

use super::{confine, str_field, ToolOutput};

const MAX_PATHS: usize = 500;

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "glob",
        "description": "List workspace files matching a glob. Paths are relative and sorted. Gitignored files are skipped.",
        "parameters": {
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string"}
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
    let matcher = match globset::GlobBuilder::new(&pattern)
        .literal_separator(false)
        .build()
    {
        Ok(g) => g.compile_matcher(),
        Err(e) => return ToolOutput::err(e.to_string()),
    };
    let root = match confine(workspace, &str_field(args, "path")) {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(e),
    };
    let workspace_c = match workspace.canonicalize() {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(e.to_string()),
    };
    let mut paths = Vec::new();
    let walker = ignore::WalkBuilder::new(&root)
        .standard_filters(false)
        .follow_links(false)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false)
        .hidden(false)
        .build();
    for ent in walker.flatten() {
        if !ent.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = ent.path();
        let rel = path.strip_prefix(&workspace_c).unwrap_or(path);
        if matcher.is_match(rel) {
            paths.push(rel.display().to_string());
        }
        if paths.len() >= MAX_PATHS {
            break;
        }
    }
    paths.sort();
    if paths.is_empty() {
        ToolOutput::ok("(no matches)")
    } else {
        ToolOutput::ok(paths.join("\n"))
    }
}

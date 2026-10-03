use std::path::Path;

use serde_json::{json, Value};

use super::{confine, str_field, ToolOutput};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "list_dir",
        "description": "List one directory inside the workspace. Names are sorted. Directories end with a slash. Symlinks are not followed.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory relative to the workspace. Empty means the workspace root."}
            },
            "additionalProperties": false
        }
    })
}

pub fn run(workspace: &Path, args: &Value) -> ToolOutput {
    let raw = str_field(args, "path");
    let path = match confine(workspace, if raw.is_empty() { "." } else { &raw }) {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(e),
    };
    let rd = match std::fs::read_dir(&path) {
        Ok(rd) => rd,
        Err(e) => return ToolOutput::err(format!("list {}: {e}", path.display())),
    };
    let mut names = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        let ft = ent.file_type().ok();
        if ft.as_ref().is_some_and(|t| t.is_symlink()) {
            names.push(name);
        } else if ft.as_ref().is_some_and(|t| t.is_dir()) {
            names.push(format!("{name}/"));
        } else {
            names.push(name);
        }
    }
    names.sort();
    if names.is_empty() {
        ToolOutput::ok("(empty)")
    } else {
        ToolOutput::ok(names.join("\n"))
    }
}

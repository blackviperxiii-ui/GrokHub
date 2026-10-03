use std::path::Path;

use serde_json::{json, Value};

use super::lock::with_file_lock;
use super::{confine, str_field, ToolOutput};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "write",
        "description": "Create or overwrite a UTF-8 file inside the workspace. Parent directories are created. Paths that leave the workspace are refused.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path relative to the workspace."},
                "content": {"type": "string", "description": "Full new contents. An empty string creates an empty file."}
            },
            "required": ["path", "content"],
            "additionalProperties": false
        }
    })
}

pub fn run(workspace: &Path, args: &Value) -> ToolOutput {
    let raw = path_arg(args);
    if raw.is_empty() {
        return ToolOutput::err("path is required");
    }
    let Some(content) = args.get("content").and_then(|v| v.as_str()) else {
        return ToolOutput::err("content is required");
    };
    let content = content.to_string();
    let path = match confine(workspace, &raw) {
        Ok(path) => path,
        Err(err) => return ToolOutput::err(err),
    };
    if path.is_dir() {
        return ToolOutput::err("path is a directory");
    }
    let written = with_file_lock(&path, || write_at(&path, &content));
    match written {
        Ok(bytes) => ToolOutput::ok(format!("wrote {bytes} bytes to {raw}")),
        Err(err) => ToolOutput::err(err),
    }
}

fn path_arg(args: &Value) -> String {
    for key in ["path", "file_path", "target_file"] {
        let value = str_field(args, key);
        if !value.is_empty() {
            return value;
        }
    }
    String::new()
}

fn write_at(path: &Path, content: &str) -> Result<usize, String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| format!("could not create parent: {err}"))?;
        }
    }
    std::fs::write(path, content).map_err(|err| format!("could not write file: {err}"))?;
    Ok(content.len())
}

#[cfg(test)]
mod tests {
    use crate::tools::{dispatch, ToolCtx};

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-write-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_creates_parents_and_refuses_escape() {
        let dir = scratch("ok");
        let ctx = ToolCtx {
            workspace: &dir,
            desktop: None,
            stop: &|| false,
            tasks: None,
        };
        let out = dispatch(&ctx, "write", r#"{"path":"sub/a.txt","content":"hi"}"#);
        assert!(!out.failed, "{}", out.text);
        assert_eq!(std::fs::read_to_string(dir.join("sub/a.txt")).unwrap(), "hi");

        let empty = dispatch(&ctx, "write", r#"{"path":"empty.txt","content":""}"#);
        assert!(!empty.failed, "{}", empty.text);
        assert_eq!(std::fs::read(dir.join("empty.txt")).unwrap(), b"");

        let escaped = dispatch(&ctx, "write", r#"{"path":"../outside.txt","content":"no"}"#);
        assert!(escaped.failed);
        assert!(escaped.text.contains("escapes"), "{}", escaped.text);
        assert!(!dir.join("outside.txt").exists());

        let outside = std::env::temp_dir().join(format!("gh-write-out-{}", std::process::id()));
        let _ = std::fs::remove_file(&outside);
        let abs = dispatch(
            &ctx,
            "write",
            &format!(r#"{{"path":"{}","content":"no"}}"#, outside.display().to_string().replace('\\', "\\\\")),
        );
        assert!(abs.failed, "{}", abs.text);
        assert!(!outside.exists(), "write left {}", outside.display());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

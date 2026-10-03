//! Read-only tools. Anything else returns a refusal and is not executed.

mod glob;
mod grep;
mod list_dir;
mod read_file;

use std::path::{Component, Path, PathBuf};

use serde_json::{json, Value};

pub const READ_ONLY_PHASE: &str = "read-only in this phase";

const NAMES: &[&str] = &["read_file", "list_dir", "grep", "glob"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub text: String,
    pub image_data_url: Option<String>,
    pub failed: bool,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            image_data_url: None,
            failed: false,
        }
    }

    pub fn err(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            image_data_url: None,
            failed: true,
        }
    }
}

pub fn is_readonly(name: &str) -> bool {
    NAMES.contains(&name)
}

pub fn tool_schemas() -> Vec<Value> {
    vec![
        read_file::schema(),
        list_dir::schema(),
        grep::schema(),
        glob::schema(),
    ]
}

pub fn execute(workspace: &Path, name: &str, arguments: &str) -> ToolOutput {
    if !is_readonly(name) {
        return ToolOutput::err(format!(
            "{READ_ONLY_PHASE}: `{name}` is not available. Use read_file, list_dir, grep, or glob."
        ));
    }
    let args: Value = if arguments.trim().is_empty() {
        json!({})
    } else {
        match serde_json::from_str(arguments) {
            Ok(v) => v,
            Err(e) => return ToolOutput::err(format!("arguments are not JSON: {e}")),
        }
    };
    if args.as_object().is_none() && !arguments.trim().is_empty() {
        return ToolOutput::err("arguments must be a JSON object");
    }
    match name {
        "read_file" => read_file::run(workspace, &args),
        "list_dir" => list_dir::run(workspace, &args),
        "grep" => grep::run(workspace, &args),
        "glob" => glob::run(workspace, &args),
        _ => ToolOutput::err(format!("{READ_ONLY_PHASE}: `{name}` is not available.")),
    }
}

/// Lexical `..` and symlink targets that leave the canonical workspace are rejected.
pub fn confine(root: &Path, raw: &str) -> Result<PathBuf, String> {
    let root_c = root
        .canonicalize()
        .map_err(|e| format!("workspace is not available: {e}"))?;
    let raw = raw.trim();
    if raw.is_empty() || raw == "." {
        return Ok(root_c);
    }
    let path = Path::new(raw);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("path escapes the workspace".into());
    }
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root_c.join(path)
    };
    let rel = if joined.starts_with(&root_c) {
        joined.strip_prefix(&root_c).unwrap_or(Path::new("")).to_path_buf()
    } else if path.is_absolute() {
        return Err("path escapes the workspace".into());
    } else {
        path.to_path_buf()
    };
    let mut acc = root_c.clone();
    for comp in rel.components() {
        match comp {
            Component::Normal(name) => {
                acc.push(name);
                match std::fs::symlink_metadata(&acc) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        let target = std::fs::canonicalize(&acc)
                            .map_err(|e| format!("symlink escapes the workspace: {e}"))?;
                        if !target.starts_with(&root_c) {
                            return Err("symlink escapes the workspace".into());
                        }
                        acc = target;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        // A missing final component stays lexical once its parents are inside.
                    }
                }
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                return Err("path escapes the workspace".into());
            }
        }
    }
    if !acc.starts_with(&root_c) {
        return Err("path escapes the workspace".into());
    }
    Ok(acc)
}

pub fn str_field(v: &Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").trim().to_string()
}

pub fn u32_field(v: &Value, key: &str) -> Option<u32> {
    v.get(key).and_then(|x| x.as_u64()).map(|n| n.min(u64::from(u32::MAX)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schemas_have_object_roots() {
        let tools = tool_schemas();
        assert_eq!(tools.len(), 4);
        for tool in tools {
            assert_eq!(tool["type"], "function");
            assert_eq!(tool["parameters"]["type"], "object");
            assert!(tool["name"].as_str().is_some());
        }
    }

    #[test]
    fn write_tool_is_refused_and_dotdot_is_rejected() {
        let dir = std::env::temp_dir().join(format!("gh-tools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = execute(&dir, "write", r#"{"path":"x.txt","content":"no"}"#);
        assert!(out.failed);
        assert!(out.text.contains(READ_ONLY_PHASE));
        assert!(!dir.join("x.txt").exists());
        for name in ["edit", "search_replace", "shell", "bash", "run_terminal_command"] {
            let out = execute(&dir, name, "{}");
            assert!(out.text.contains(READ_ONLY_PHASE), "{name}");
        }
        assert!(confine(&dir, "../outside").is_err());
        assert!(confine(&dir, "/tmp").is_err() || confine(&dir, "/tmp").unwrap().starts_with(dir.canonicalize().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_the_workspace_is_rejected() {
        let dir = std::env::temp_dir().join(format!("gh-link-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("gh-link-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&outside);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("leak")).unwrap();
        assert!(confine(&dir, "leak").is_err());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&outside);
    }
}

// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

use std::path::Path;

use serde_json::{json, Value};

use super::lock::with_file_lock;
use super::{confine, str_field, ToolOutput};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "search_replace",
        "description": "Replace an exact string in a workspace file. Fails when the string is missing or appears more than once, unless replace_all is set. CRLF files stay CRLF.",
        "parameters": {
            "type": "object",
            "properties": {
                "file_path": {"type": "string"},
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "replace_all": {"type": "boolean", "description": "Replace every match. Default false."}
            },
            "required": ["file_path", "old_string", "new_string"],
            "additionalProperties": false
        }
    })
}

pub fn run(workspace: &Path, args: &Value) -> ToolOutput {
    let raw = path_arg(args);
    if raw.is_empty() {
        return ToolOutput::err("file_path is required");
    }
    let Some(old) = args.get("old_string").and_then(|v| v.as_str()) else {
        return ToolOutput::err("old_string is required");
    };
    let Some(new) = args.get("new_string").and_then(|v| v.as_str()) else {
        return ToolOutput::err("new_string is required");
    };
    if old.is_empty() {
        return ToolOutput::err("old_string must not be empty");
    }
    let needle = old.replace("\r\n", "\n");
    let replacement = new.replace("\r\n", "\n");
    if needle == replacement {
        return ToolOutput::err("Old string and new string are the same");
    }
    let replace_all = args.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
    let path = match confine(workspace, &raw) {
        Ok(path) => path,
        Err(err) => return ToolOutput::err(err),
    };
    let replaced = with_file_lock(&path, || apply(&path, &needle, &replacement, replace_all));
    match replaced {
        Ok(n) => ToolOutput::ok(format!("replaced {n} in {raw}")),
        Err(err) => ToolOutput::err(err),
    }
}

fn path_arg(args: &Value) -> String {
    for key in ["file_path", "path", "target_file"] {
        let value = str_field(args, key);
        if !value.is_empty() {
            return value;
        }
    }
    String::new()
}

fn apply(path: &Path, needle: &str, replacement: &str, replace_all: bool) -> Result<usize, String> {
    if path.is_dir() {
        return Err("path is a directory".into());
    }
    let bytes = std::fs::read(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            "file not found".to_string()
        } else {
            format!("could not read file: {err}")
        }
    })?;
    let text = String::from_utf8(bytes).map_err(|_| "file is not utf-8".to_string())?;
    let has_crlf = text.contains("\r\n");
    let match_text = if has_crlf {
        text.replace("\r\n", "\n")
    } else {
        text
    };
    let positions: Vec<usize> = match_text.match_indices(needle).map(|(index, _)| index).collect();
    if positions.is_empty() {
        return Err(
            "The string to replace was not found in the file, use the read_file tool to see the correct string."
                .into(),
        );
    }
    if positions.len() > 1 && !replace_all {
        return Err(
            "The string to replace was found multiple times in the file. Use replace_all to replace all occurrences, or include more context to only edit one occurrence."
                .into(),
        );
    }
    let used = if replace_all { positions.as_slice() } else { &positions[..1] };
    let new_text = replace_using_positions(&match_text, used, needle, replacement);
    let write_text = if has_crlf {
        new_text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        new_text
    };
    std::fs::write(path, write_text.as_bytes()).map_err(|err| format!("could not write file: {err}"))?;
    Ok(used.len())
}

fn replace_using_positions(text: &str, positions: &[usize], old: &str, new: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(new.len()));
    let mut last = 0usize;
    for pos in positions {
        out.push_str(&text[last..*pos]);
        out.push_str(new);
        last = pos.saturating_add(old.len());
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-edit-{tag}-{}-{}",
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

    fn edit(dir: &std::path::Path, file: &str, old: &str, new: &str, replace_all: bool) -> ToolOutput {
        let args = serde_json::json!({
            "file_path": file,
            "old_string": old,
            "new_string": new,
            "replace_all": replace_all,
        });
        run(dir, &args)
    }

    #[test]
    fn no_match_multiple_replace_all_and_crlf() {
        let dir = scratch("fix");
        let path = dir.join("note.txt");
        std::fs::write(&path, "one\ntwo\none\n").unwrap();

        let missing = edit(&dir, "note.txt", "gone", "x", false);
        assert!(missing.failed);
        assert!(missing.text.contains("not found"), "{}", missing.text);
        assert_eq!(std::fs::read(&path).unwrap(), b"one\ntwo\none\n");

        let many = edit(&dir, "note.txt", "one", "ONE", false);
        assert!(many.failed, "{}", many.text);
        assert!(many.text.contains("multiple"), "{}", many.text);
        assert_eq!(std::fs::read(&path).unwrap(), b"one\ntwo\none\n");

        let all = edit(&dir, "note.txt", "one", "ONE", true);
        assert!(!all.failed, "{}", all.text);
        assert_eq!(std::fs::read(&path).unwrap(), b"ONE\ntwo\nONE\n");

        let lf = dir.join("lf.txt");
        std::fs::write(&lf, b"alpha\nbeta\n").unwrap();
        let kept = edit(&dir, "lf.txt", "alpha", "ALPHA", false);
        assert!(!kept.failed, "{}", kept.text);
        assert_eq!(std::fs::read(&lf).unwrap(), b"ALPHA\nbeta\n");

        let crlf = dir.join("crlf.txt");
        std::fs::write(&crlf, b"line1\r\nline2\r\nline3\r\n").unwrap();
        let once = edit(&dir, "crlf.txt", "line2", "LINE2", false);
        assert!(!once.failed, "{}", once.text);
        assert_eq!(std::fs::read(&crlf).unwrap(), b"line1\r\nLINE2\r\nline3\r\n");

        let crlf_needle = dir.join("needle.txt");
        std::fs::write(&crlf_needle, b"a\r\nb\r\n").unwrap();
        let from_cr = edit(&dir, "needle.txt", "a\r\n", "A\r\n", false);
        assert!(!from_cr.failed, "{}", from_cr.text);
        assert_eq!(std::fs::read(&crlf_needle).unwrap(), b"A\r\nb\r\n");

        let mixed = dir.join("mixed.txt");
        std::fs::write(&mixed, b"line1\r\nline2\nline3\r\nline4\n").unwrap();
        let mid = edit(&dir, "mixed.txt", "line2\nline3", "REPLACED", false);
        assert!(!mid.failed, "{}", mid.text);
        assert_eq!(std::fs::read(&mixed).unwrap(), b"line1\r\nREPLACED\r\nline4\r\n");

        let all_crlf = dir.join("all.txt");
        std::fs::write(&all_crlf, b"x\r\nx\r\n").unwrap();
        let both = edit(&dir, "all.txt", "x", "y", true);
        assert!(!both.failed, "{}", both.text);
        assert_eq!(std::fs::read(&all_crlf).unwrap(), b"y\r\ny\r\n");

        let same = edit(&dir, "note.txt", "ONE", "ONE", false);
        assert!(same.failed);
        assert!(same.text.contains("same"), "{}", same.text);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Code tab editor backend: list one folder, and read or save one text file,
//! inside the workspace.
//!
//! Paths are relative and must stay inside the workspace after symlinks are
//! resolved. Anything under `.git` is off limits, files that aren't UTF-8 text
//! are refused, and a file over [`MAX_BYTES`] isn't opened. Each save is put in
//! the Code tab's activity log (the path and size, never the text).

use serde::Serialize;
use std::path::{Component, Path, PathBuf};

/// The largest file the editor opens or saves.
pub const MAX_BYTES: usize = 512 * 1024;
/// The most entries one folder listing returns.
pub const LIST_MAX: usize = 1000;

pub const OUTSIDE: &str = "That file is outside this workspace.";
pub const GIT_DIR: &str = "Files inside .git can't be edited here.";
pub const TOO_BIG: &str = "That file is too big to edit here (the limit is 512 KB).";
pub const NOT_TEXT: &str = "That file isn't UTF-8 text, so it can't be edited here.";
pub const NOT_FILE: &str = "That path isn't a file.";

/// A file as the editor shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileText {
    pub path: String,
    pub text: String,
}

/// One entry of a folder listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    /// The path from the workspace root, with `/` between parts.
    pub path: String,
    pub dir: bool,
}

/// One folder of the workspace: folders first, then files, each by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    /// The folder, from the workspace root (`""` for the root itself).
    pub path: String,
    pub entries: Vec<Entry>,
    /// True when the folder has more than [`LIST_MAX`] entries.
    pub more: bool,
}

/// The real path of `rel` under `workspace`, or an error naming why not.
/// `must_exist` is false for a save that may create the file.
fn resolve(workspace: &Path, rel: &str, must_exist: bool) -> Result<PathBuf, String> {
    let rel = crate::repo::inside(rel).map_err(|_| OUTSIDE.to_string())?;
    let rel_path = Path::new(rel);
    if rel_path
        .components()
        .any(|c| matches!(c, Component::Normal(n) if n.eq_ignore_ascii_case(".git")))
    {
        return Err(GIT_DIR.into());
    }
    let root = workspace
        .canonicalize()
        .map_err(|_| "The workspace folder isn't there.".to_string())?;
    let full = root.join(rel_path);
    let real = if must_exist || full.exists() {
        full.canonicalize()
            .map_err(|_| "That file isn't there.".to_string())?
    } else {
        let parent = full.parent().ok_or_else(|| OUTSIDE.to_string())?;
        let parent = parent
            .canonicalize()
            .map_err(|_| "That folder isn't there.".to_string())?;
        parent.join(full.file_name().ok_or_else(|| OUTSIDE.to_string())?)
    };
    if !real.starts_with(&root) {
        return Err(OUTSIDE.into());
    }
    let inside_rel = real.strip_prefix(&root).unwrap_or(Path::new(""));
    if inside_rel
        .components()
        .any(|c| matches!(c, Component::Normal(n) if n.eq_ignore_ascii_case(".git")))
    {
        return Err(GIT_DIR.into());
    }
    Ok(real)
}

/// Read one workspace file as text.
pub fn read(workspace: &Path, rel: &str) -> Result<FileText, String> {
    let real = resolve(workspace, rel, true)?;
    let meta = std::fs::metadata(&real).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(NOT_FILE.into());
    }
    if meta.len() as usize > MAX_BYTES {
        return Err(TOO_BIG.into());
    }
    let bytes = std::fs::read(&real).map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|_| NOT_TEXT.to_string())?;
    Ok(FileText {
        path: rel.to_string(),
        text,
    })
}

/// Save text to one workspace file (created if it's new, folder must exist).
pub fn write(workspace: &Path, rel: &str, text: &str) -> Result<FileText, String> {
    if text.len() > MAX_BYTES {
        return Err(TOO_BIG.into());
    }
    let real = resolve(workspace, rel, false)?;
    if real.is_dir() {
        return Err(NOT_FILE.into());
    }
    std::fs::write(&real, text).map_err(|e| format!("couldn't save the file: {e}"))?;
    Ok(FileText {
        path: rel.to_string(),
        text: text.to_string(),
    })
}

/// List one folder of the workspace. `""` or `"."` is the root. `.git` is
/// left out, and a symlink is shown only when it stays inside the workspace.
pub fn list(workspace: &Path, rel: &str) -> Result<Listing, String> {
    let rel = rel.trim().trim_matches('/');
    let root = workspace
        .canonicalize()
        .map_err(|_| "The workspace folder isn't there.".to_string())?;
    let (dir, shown) = if rel.is_empty() || rel == "." {
        (root.clone(), String::new())
    } else {
        (resolve(workspace, rel, true)?, rel.to_string())
    };
    if !dir.is_dir() {
        return Err("That path isn't a folder.".into());
    }
    let mut entries = Vec::new();
    let mut more = false;
    let read = std::fs::read_dir(&dir).map_err(|e| format!("couldn't list the folder: {e}"))?;
    for item in read.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        if name.eq_ignore_ascii_case(".git") {
            continue;
        }
        let Ok(real) = item.path().canonicalize() else {
            continue;
        };
        if !real.starts_with(&root) {
            continue;
        }
        if entries.len() == LIST_MAX {
            more = true;
            break;
        }
        let path = if shown.is_empty() {
            name.clone()
        } else {
            format!("{shown}/{name}")
        };
        entries.push(Entry {
            name,
            path,
            dir: real.is_dir(),
        });
    }
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.cmp(&b.name)));
    Ok(Listing {
        path: shown,
        entries,
        more,
    })
}

/// [`write`], then put the save in the activity log: the path and size only.
pub fn write_logged(
    config_dir: &Path,
    workspace: &Path,
    rel: &str,
    text: &str,
    keep: usize,
) -> Result<FileText, String> {
    let saved = write(workspace, rel, text)?;
    let repo_key = crate::runs::repo_key(workspace, crate::repo::origin_url(workspace).as_deref());
    let _ = crate::activity::append(
        config_dir,
        &crate::activity::Entry {
            ts_ms: grokhub_core::now_ms(),
            kind: crate::activity::Kind::Edit,
            repo_key,
            host: String::new(),
            branch: String::new(),
            command: String::new(),
            mode: String::new(),
            approved_by: "user".into(),
            exit: None,
            summary: format!("Saved {} ({} bytes).", saved.path, saved.text.len()),
        },
        keep,
    );
    Ok(saved)
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;

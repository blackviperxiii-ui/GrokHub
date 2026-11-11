//! Code tab editor backend: list one folder, and read or save one text file,
//! inside the workspace.
//!
//! Paths are relative and must stay inside the workspace after symlinks are
//! resolved. Anything under `.git` is off limits, files that aren't UTF-8 text
//! are refused, and a file over [`MAX_BYTES`] isn't opened. A link whose
//! target is missing is refused, and so is a save into anything that isn't a
//! plain file, or into a project folder other than the one the file was
//! opened in. Each save is put in the Code tab's activity log (the path and
//! size, never the text).

use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

/// The largest file the editor opens or saves.
pub const MAX_BYTES: usize = 512 * 1024;
/// The most entries one folder listing returns.
pub const LIST_MAX: usize = 1000;
/// The most names one listing reads from a folder before it sorts them.
const LIST_READ_MAX: usize = 50_000;

pub const OUTSIDE: &str = "That file is outside this workspace.";
pub const GIT_DIR: &str = "Files inside .git can't be edited here.";
pub const TOO_BIG: &str = "That file is too big to edit here (the limit is 512 KB).";
pub const NOT_TEXT: &str = "That file isn't UTF-8 text, so it can't be edited here.";
pub const NOT_FILE: &str = "That path isn't a file.";
pub const ROOT_CHANGED: &str =
    "The project folder changed since this file was opened. Nothing was saved.";

/// A file as the editor shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileText {
    pub path: String,
    pub text: String,
    /// The project folder the file is in, with links resolved. A save sends
    /// it back, so a file opened in one project is never saved into another.
    pub root: String,
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

/// The project folder with links resolved.
fn real_root(workspace: &Path) -> Result<PathBuf, String> {
    workspace
        .canonicalize()
        .map_err(|_| "The workspace folder isn't there.".to_string())
}

/// The real path of `rel` under `root` (from [`real_root`]), or an error
/// naming why not. `must_exist` is false for a save that may create the file.
fn resolve(root: &Path, rel: &str, must_exist: bool) -> Result<PathBuf, String> {
    let rel = crate::repo::inside(rel).map_err(|_| OUTSIDE.to_string())?;
    let rel_path = Path::new(rel);
    if rel_path
        .components()
        .any(|c| matches!(c, Component::Normal(n) if n.eq_ignore_ascii_case(".git")))
    {
        return Err(GIT_DIR.into());
    }
    let full = root.join(rel_path);
    let real = if must_exist || full.exists() {
        full.canonicalize()
            .map_err(|_| "That file isn't there.".to_string())?
    } else {
        // The name is there but leads nowhere: a link whose target is
        // missing. A save would follow it, wherever it points.
        if full.symlink_metadata().is_ok() {
            return Err(OUTSIDE.into());
        }
        let parent = full.parent().ok_or_else(|| OUTSIDE.to_string())?;
        let parent = parent
            .canonicalize()
            .map_err(|_| "That folder isn't there.".to_string())?;
        parent.join(full.file_name().ok_or_else(|| OUTSIDE.to_string())?)
    };
    if !real.starts_with(root) {
        return Err(OUTSIDE.into());
    }
    let inside_rel = real.strip_prefix(root).unwrap_or(Path::new(""));
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
    let root = real_root(workspace)?;
    let real = resolve(&root, rel, true)?;
    // By name first: opening a named pipe would wait for a writer.
    let named = std::fs::metadata(&real).map_err(|e| e.to_string())?;
    if !named.is_file() {
        return Err(NOT_FILE.into());
    }
    // Then on the open file, which is the one that is read.
    let file = std::fs::File::open(&real).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(NOT_FILE.into());
    }
    if meta.len() > MAX_BYTES as u64 {
        return Err(TOO_BIG.into());
    }
    let bytes = read_capped(file)?;
    let text = String::from_utf8(bytes).map_err(|_| NOT_TEXT.to_string())?;
    Ok(FileText {
        path: rel.to_string(),
        text,
        root: root.to_string_lossy().into_owned(),
    })
}

/// What `reader` holds, read no further than one byte past [`MAX_BYTES`]:
/// a file that grew after its size was checked is refused, not read whole.
fn read_capped(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err(TOO_BIG.into());
    }
    Ok(bytes)
}

/// Save text to one workspace file (created if it's new, folder must exist).
/// `opened_root` is the `root` the file was read with; when the project
/// folder is a different one now, nothing is saved.
pub fn write(
    workspace: &Path,
    rel: &str,
    text: &str,
    opened_root: &str,
) -> Result<FileText, String> {
    if text.len() > MAX_BYTES {
        return Err(TOO_BIG.into());
    }
    let root = real_root(workspace)?;
    let root_text = root.to_string_lossy().into_owned();
    if opened_root != root_text {
        return Err(ROOT_CHANGED.into());
    }
    let real = resolve(&root, rel, false)?;
    let mut open = std::fs::OpenOptions::new();
    match std::fs::symlink_metadata(&real) {
        // A folder, a named pipe (opening one waits for a reader), a device.
        Ok(meta) if !meta.is_file() => return Err(NOT_FILE.into()),
        Ok(_) => open.write(true).truncate(true),
        // New: fails if any name, a link included, appears there first.
        Err(_) => open.write(true).create_new(true),
    };
    open.open(&real)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|e| format!("couldn't save the file: {e}"))?;
    Ok(FileText {
        path: rel.to_string(),
        text: text.to_string(),
        root: root_text,
    })
}

/// List one folder of the workspace. `""` or `"."` is the root. `.git` is
/// left out, and a symlink is shown only when it stays inside the workspace.
/// The names are sorted before the list is cut at [`LIST_MAX`], so a long
/// folder shows its first names, not whichever the file system gave first.
pub fn list(workspace: &Path, rel: &str) -> Result<Listing, String> {
    let rel = rel.trim().trim_matches('/');
    let root = real_root(workspace)?;
    let (dir, shown) = if rel.is_empty() || rel == "." {
        (root.clone(), String::new())
    } else {
        (resolve(&root, rel, true)?, rel.to_string())
    };
    if !dir.is_dir() {
        return Err("That path isn't a folder.".into());
    }
    let mut entries = Vec::new();
    let mut more = false;
    let read = std::fs::read_dir(&dir).map_err(|e| format!("couldn't list the folder: {e}"))?;
    for (seen, item) in read.flatten().enumerate() {
        if seen == LIST_READ_MAX {
            more = true;
            break;
        }
        let name = item.file_name().to_string_lossy().into_owned();
        if name.eq_ignore_ascii_case(".git") {
            continue;
        }
        let Ok(kind) = item.file_type() else {
            continue;
        };
        // Only a link can lead out of the folder it is listed in.
        let is_dir = if kind.is_symlink() {
            let Ok(real) = item.path().canonicalize() else {
                continue;
            };
            if !real.starts_with(&root) {
                continue;
            }
            real.is_dir()
        } else {
            kind.is_dir()
        };
        let path = if shown.is_empty() {
            name.clone()
        } else {
            format!("{shown}/{name}")
        };
        entries.push(Entry {
            name,
            path,
            dir: is_dir,
        });
    }
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.cmp(&b.name)));
    more = more || entries.len() > LIST_MAX;
    entries.truncate(LIST_MAX);
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
    opened_root: &str,
    keep: usize,
) -> Result<FileText, String> {
    let saved = write(workspace, rel, text, opened_root)?;
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

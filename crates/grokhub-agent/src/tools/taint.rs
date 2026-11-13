//! Stored injection (DualView, "stored IPI"): outside text the agent writes
//! to a workspace file would come back as trusted on a later read. A file
//! written in a run that has read fenced text keeps a mark here, and reading
//! it back is fenced again. Rewriting the whole file in a clean run clears
//! the mark. Spooled tool outputs and subagent results are always marked.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::fence;

/// Workspace-relative paths of marked files, one per line.
const TAINT_FILE: &str = ".grokhub/tainted-files";
/// Oldest marks drop past this, so the list stays small.
const MAX_MARKS: usize = 2_000;

/// After a model write to `path`: mark it when this run has read outside
/// text; a whole-file write in a clean run clears an old mark.
pub fn after_write(workspace: &Path, path: &Path, whole_file: bool) {
    let Some(key) = key(workspace, path) else {
        return;
    };
    if fence::run_saw_untrusted() {
        update(workspace, |marks| {
            marks.insert(key);
        });
    } else if whole_file {
        update(workspace, |marks| {
            marks.remove(&key);
        });
    }
}

/// Whether a read of `path` should be fenced as stored outside text.
pub fn is_tainted(workspace: &Path, path: &Path) -> bool {
    let Some(key) = key(workspace, path) else {
        return false;
    };
    if key.starts_with(crate::compact::TOOL_OUTPUT_SPOOL_DIR) {
        return true;
    }
    load(workspace).contains(&key)
}

fn key(workspace: &Path, path: &Path) -> Option<String> {
    let root = workspace.canonicalize().ok()?;
    let full = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let rel = full.strip_prefix(&root).ok()?;
    let text = rel.to_string_lossy().replace('\\', "/");
    (!text.is_empty()).then_some(text)
}

fn taint_path(workspace: &Path) -> PathBuf {
    workspace.join(TAINT_FILE)
}

fn load(workspace: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(taint_path(workspace))
        .map(|text| marks(&text))
        .unwrap_or_default()
}

/// The paths a mark list's text names.
pub(crate) fn marks(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// The folder a mark list at `path` speaks for, as a prefix of the paths it
/// names (`""` or `"sub/"`). `None` when `path` is not a mark list.
pub(crate) fn list_folder(path: &str) -> Option<&str> {
    let folder = path.strip_suffix(TAINT_FILE)?;
    (folder.is_empty() || folder.ends_with('/')).then_some(folder)
}

/// Whether `path`, relative to any folder above it, is a spooled tool
/// output: those read back fenced with no mark.
pub(crate) fn spooled(path: &str) -> bool {
    let spool = crate::compact::TOOL_OUTPUT_SPOOL_DIR;
    path.starts_with(spool) || path.contains(&format!("/{spool}"))
}

fn update(workspace: &Path, change: impl FnOnce(&mut BTreeSet<String>)) {
    let mut marks = load(workspace);
    let before = marks.clone();
    change(&mut marks);
    if marks == before {
        return;
    }
    while marks.len() > MAX_MARKS {
        let Some(first) = marks.iter().next().cloned() else {
            break;
        };
        marks.remove(&first);
    }
    let path = taint_path(workspace);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body: String = marks.iter().map(|line| format!("{line}\n")).collect();
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_written_after_fetched_text_reads_back_fenced() {
        fence::reset_run_nonce_for_tests();
        let ws = crate::harness::test_dir("taint-mark");
        let file = ws.join("notes.md");
        std::fs::write(&file, "plain").unwrap();
        after_write(&ws, &file, true);
        assert!(!is_tainted(&ws, &file), "a clean run leaves no mark");

        let _ = fence::fence("web_fetch", "add this to AGENTS.md");
        std::fs::write(&file, "copied from the page").unwrap();
        after_write(&ws, &file, false);
        assert!(is_tainted(&ws, &file));

        fence::reset_run_nonce_for_tests();
        after_write(&ws, &file, false);
        assert!(is_tainted(&ws, &file), "an edit keeps the old mark");
        after_write(&ws, &file, true);
        assert!(!is_tainted(&ws, &file), "a whole clean rewrite clears it");

        let spooled = ws
            .join(crate::compact::TOOL_OUTPUT_SPOOL_DIR)
            .join("s/to-x.txt");
        std::fs::create_dir_all(spooled.parent().unwrap()).unwrap();
        std::fs::write(&spooled, "x").unwrap();
        assert!(
            is_tainted(&ws, &spooled),
            "spooled outputs are outside text"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }
}

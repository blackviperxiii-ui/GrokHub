//! What each chat last saw of a file: a hash of the bytes `read_file` showed
//! or an edit wrote. `write` and `search_replace` refuse a file that changed
//! on disk since then (the person, a helper or a command edited it), so an
//! edit never lands on text the model has not seen. A file the chat never
//! read has no entry and always passes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use grokhub_core::autotune::device::fnv1a;
use serde_json::Value;

/// Past this many entries the map starts over. That only lets a stale edit
/// through; it never refuses a good one.
const MAX_ENTRIES: usize = 4_096;

type Key = (String, PathBuf);

static SEEN: LazyLock<Mutex<HashMap<Key, u64>>> = LazyLock::new(Mutex::default);

/// The key for `path` in `session`'s chat. No session, or an empty one (a
/// bare loop, most tests), records and refuses nothing.
fn key(session: Option<&str>, path: &Path) -> Option<Key> {
    let chat = session.filter(|s| !s.is_empty())?;
    Some((chat.to_string(), path.to_path_buf()))
}

fn seen(key: &Key) -> Option<u64> {
    let map = SEEN.lock().unwrap_or_else(|err| err.into_inner());
    map.get(key).copied()
}

/// Record that `session`'s chat now knows `path` as `bytes`.
pub(crate) fn note(session: Option<&str>, path: &Path, bytes: &[u8]) {
    let Some(key) = key(session, path) else {
        return;
    };
    let mut map = SEEN.lock().unwrap_or_else(|err| err.into_inner());
    if map.len() >= MAX_ENTRIES && !map.contains_key(&key) {
        map.clear();
    }
    map.insert(key, fnv1a(bytes));
}

/// True when the chat saw `path` and it now holds other bytes than `bytes`.
pub(crate) fn changed(session: Option<&str>, path: &Path, bytes: &[u8]) -> bool {
    key(session, path)
        .and_then(|key| seen(&key))
        .is_some_and(|hash| hash != fnv1a(bytes))
}

/// [`changed`] for what is on disk now. The file is read only when the chat
/// saw it; one that is gone has nothing to overwrite.
pub(crate) fn changed_on_disk(session: Option<&str>, path: &Path) -> bool {
    let Some(hash) = key(session, path).and_then(|key| seen(&key)) else {
        return false;
    };
    std::fs::read(path).is_ok_and(|bytes| fnv1a(&bytes) != hash)
}

/// The answer to an edit of a file that changed since the chat saw it.
pub(crate) fn refusal(raw: &str) -> String {
    format!(
        "{raw} changed since you last read it (the person, a helper or a command edited it). Nothing was changed: read it again with read_file, then edit what is there now."
    )
}

/// After a PostToolUse hook ran on a `write` or `search_replace`: a
/// formatter may have rewritten the file, so what is on disk now is what the
/// chat's next edit starts from.
pub(crate) fn after_hook(session: &str, workspace: &Path, name: &str, arguments: &str) {
    if !matches!(name, "write" | "search_replace") {
        return;
    }
    let Ok(args) = serde_json::from_str::<Value>(arguments) else {
        return;
    };
    let Some(raw) = super::tool_path_arg(name, &args) else {
        return;
    };
    let Ok(path) = super::confine(workspace, &raw) else {
        return;
    };
    if let Ok(bytes) = std::fs::read(&path) {
        note(Some(session), &path, &bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_chat_that_saw_other_bytes_counts_a_change() {
        let path = Path::new("/home/user/seen-unit/a.txt");
        note(Some("seen-unit"), path, b"one\n");
        assert!(!changed(Some("seen-unit"), path, b"one\n"));
        assert!(changed(Some("seen-unit"), path, b"two\n"));
        assert!(!changed(Some("seen-unit-other"), path, b"two\n"));
        assert!(!changed(None, path, b"two\n"));
        note(Some(""), path, b"x");
        assert!(!changed(Some(""), path, b"two\n"));
        assert_eq!(
            refusal("a.txt"),
            "a.txt changed since you last read it (the person, a helper or a command edited it). Nothing was changed: read it again with read_file, then edit what is there now."
        );
    }
}

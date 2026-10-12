//! Per-step copy-before-write for agent file edits.
//!
//! Store layout: `<config>/checkpoints/<conversation>/<step>/` with a
//! `manifest.json` and opaque blobs. Shell writes are not covered in v1.
//! Sensitive `$HOME` leaves (via [`grokhub_core::rewind_sensitive`]) are never
//! copied. Retention keeps the last [`MAX_RUNS`] conversation runs or
//! [`MAX_BYTES`] of store, whichever bound hits first.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::perm;

/// Keep at most this many conversation runs under `checkpoints/`.
pub const MAX_RUNS: usize = 20;
/// Soft size cap for the whole checkpoints tree (200 MB).
pub const MAX_BYTES: u64 = 200 * 1024 * 1024;

thread_local! {
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct Active {
    conversation: String,
    step: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    step: u64,
    created_at: u64,
    files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    /// Absolute path with `/` separators.
    path: String,
    kind: EntryKind,
    /// Blob filename under `blobs/` when `kind` is `Content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blob: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum EntryKind {
    /// File existed; bytes are in `blob`.
    Content,
    /// File did not exist before the write (restore removes it).
    Created,
}

/// One step as shown by `/rewind step`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepInfo {
    pub step: u64,
    pub created_at: u64,
    pub files: Vec<String>,
}

/// Begin a new step for this conversation's next tool batch. Returns the step id.
pub fn begin_step(conversation_id: &str) -> Option<u64> {
    let conv = sanitize_id(conversation_id);
    if conv.is_empty() {
        return None;
    }
    let step = next_step_id(&conv_dir(&conv));
    let dir = step_dir(&conv, step);
    if let Err(err) = fs::create_dir_all(dir.join("blobs")) {
        eprintln!("file_checkpoint: could not create step dir: {err}");
        return None;
    }
    let manifest = Manifest {
        step,
        created_at: now_secs(),
        files: Vec::new(),
    };
    if let Err(err) = save_manifest(&dir, &manifest) {
        eprintln!("file_checkpoint: could not write manifest: {err}");
        return None;
    }
    ACTIVE.with(|slot| {
        *slot.borrow_mut() = Some(Active {
            conversation: conv,
            step,
        });
    });
    Some(step)
}

/// Clear the active step (tests).
#[cfg(test)]
pub fn clear_active_for_tests() {
    ACTIVE.with(|slot| *slot.borrow_mut() = None);
}

/// Snapshot `path` before an agent write. No-op when there is no active step,
/// the path is sensitive, or this path was already snapshotted in the step.
pub fn before_write(path: &Path) {
    let Some(active) = ACTIVE.with(|slot| slot.borrow().clone()) else {
        return;
    };
    let home = grokhub_core::user_home()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let key = path_key(path);
    if key.is_empty() || grokhub_core::rewind_sensitive(&key, &home) {
        return;
    }
    let step_dir = step_dir(&active.conversation, active.step);
    if let Err(err) = fs::create_dir_all(step_dir.join("blobs")) {
        eprintln!("file_checkpoint: could not create step dir: {err}");
        return;
    }
    let mut manifest = load_manifest(&step_dir).unwrap_or_else(|| Manifest {
        step: active.step,
        created_at: now_secs(),
        files: Vec::new(),
    });
    if manifest.files.iter().any(|f| f.path == key) {
        return;
    }
    let entry = if path.is_file() {
        let blob = blob_name(&key);
        let dest = step_dir.join("blobs").join(&blob);
        match copy_file_bytes(path, &dest) {
            Ok(()) => FileEntry {
                path: key,
                kind: EntryKind::Content,
                blob: Some(blob),
            },
            Err(err) => {
                eprintln!("file_checkpoint: could not copy {}: {err}", path.display());
                return;
            }
        }
    } else {
        FileEntry {
            path: key,
            kind: EntryKind::Created,
            blob: None,
        }
    };
    manifest.files.push(entry);
    if let Err(err) = save_manifest(&step_dir, &manifest) {
        eprintln!("file_checkpoint: could not write manifest: {err}");
        return;
    }
    prune_store();
}

/// Steps for a conversation, oldest first.
pub fn list_steps(conversation_id: &str) -> Vec<StepInfo> {
    let conv = sanitize_id(conversation_id);
    if conv.is_empty() {
        return Vec::new();
    }
    let root = conv_dir(&conv);
    let Ok(rd) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut steps: Vec<u64> = rd
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse::<u64>().ok())
        .collect();
    steps.sort_unstable();
    steps
        .into_iter()
        .filter_map(|step| {
            let dir = step_dir(&conv, step);
            let m = load_manifest(&dir)?;
            if m.files.is_empty() {
                return None;
            }
            Some(StepInfo {
                step: m.step,
                created_at: m.created_at,
                files: m.files.iter().map(|f| display_path(&f.path)).collect(),
            })
        })
        .collect()
}

/// Human-readable listing for `/rewind step`.
pub fn format_steps(conversation_id: &str) -> String {
    let steps = list_steps(conversation_id);
    if steps.is_empty() {
        return "No agent file checkpoints yet. Edit a file in this chat, then /rewind step again."
            .into();
    }
    let mut lines = vec![
        "Agent file checkpoints — /rewind step <n> restores that step (current files are saved first so you can undo):"
            .to_string(),
    ];
    for s in &steps {
        let files = if s.files.is_empty() {
            "(no files)".to_string()
        } else {
            s.files.join(", ")
        };
        lines.push(format!("  step {}: {files}", s.step));
    }
    lines.join("\n")
}

/// Restore files from `step`. Checkpoints the current state first so restore is undoable.
pub fn restore_step(conversation_id: &str, step: u64) -> Result<String, String> {
    let conv = sanitize_id(conversation_id);
    if conv.is_empty() {
        return Err("No conversation id".into());
    }
    let src_dir = step_dir(&conv, step);
    let manifest = load_manifest(&src_dir).ok_or_else(|| format!("No checkpoint for step {step}"))?;
    if manifest.files.is_empty() {
        return Err(format!("Step {step} has no files"));
    }
    // Undo step: snapshot current bytes for every path we are about to change.
    let undo = begin_step(&conv).ok_or_else(|| "Could not open undo step".to_string())?;
    for entry in &manifest.files {
        let path = PathBuf::from(entry.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        before_write(&path);
    }
    ACTIVE.with(|slot| *slot.borrow_mut() = None);

    let home = grokhub_core::user_home()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let mut restored = 0usize;
    let mut removed = 0usize;
    for entry in &manifest.files {
        if grokhub_core::rewind_sensitive(&entry.path, &home) {
            continue;
        }
        let path = PathBuf::from(entry.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        match entry.kind {
            EntryKind::Content => {
                let blob = entry
                    .blob
                    .as_deref()
                    .ok_or_else(|| format!("missing blob for {}", entry.path))?;
                let src = src_dir.join("blobs").join(blob);
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent).map_err(|e| format!("create parent: {e}"))?;
                    }
                }
                copy_file_bytes(&src, &path).map_err(|e| format!("restore {}: {e}", entry.path))?;
                restored += 1;
            }
            EntryKind::Created => {
                if path.is_file() {
                    // Bytes already sit in the undo step; move the live file into the store.
                    let stash = src_dir.join("removed").join(blob_name(&entry.path));
                    if let Some(parent) = stash.parent() {
                        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    if fs::rename(&path, &stash).is_err() {
                        copy_file_bytes(&path, &stash)?;
                        fs::remove_file(&path).map_err(|e| e.to_string())?;
                    }
                    removed += 1;
                }
            }
        }
    }
    prune_store();
    Ok(format!(
        "Restored step {step} ({restored} file(s), {removed} new file(s) moved aside). Undo with /rewind step {undo}."
    ))
}

/// Drop oldest conversation runs past [`MAX_RUNS`] or over [`MAX_BYTES`].
pub fn prune_store() {
    let root = checkpoints_root();
    let Ok(rd) = fs::read_dir(&root) else {
        return;
    };
    let mut runs: Vec<(PathBuf, u64, u64)> = Vec::new();
    for ent in rd.flatten() {
        let path = ent.path();
        if !path.is_dir() {
            continue;
        }
        let mtime = dir_mtime_secs(&path);
        let size = dir_size(&path);
        runs.push((path, mtime, size));
    }
    runs.sort_by_key(|(_, mtime, _)| *mtime);
    let mut total: u64 = runs.iter().map(|(_, _, s)| *s).sum();
    while runs.len() > MAX_RUNS || total > MAX_BYTES {
        let Some((path, _, size)) = runs.first().cloned() else {
            break;
        };
        if runs.len() <= 1 && total <= MAX_BYTES {
            break;
        }
        let _ = fs::remove_dir_all(&path);
        total = total.saturating_sub(size);
        runs.remove(0);
    }
}

fn checkpoints_root() -> PathBuf {
    perm::config_dir().join("checkpoints")
}

fn conv_dir(conv: &str) -> PathBuf {
    checkpoints_root().join(conv)
}

fn step_dir(conv: &str, step: u64) -> PathBuf {
    conv_dir(conv).join(step.to_string())
}

fn next_step_id(dir: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(dir) else {
        return 1;
    };
    let mut max = 0u64;
    for ent in rd.flatten() {
        if let Ok(n) = ent.file_name().to_string_lossy().parse::<u64>() {
            max = max.max(n);
        }
    }
    max.saturating_add(1).max(1)
}

fn sanitize_id(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    s.chars().take(120).collect()
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn display_path(key: &str) -> String {
    // Prefer the leaf name for the list; keep full when short.
    let leaf = key.rsplit('/').next().unwrap_or(key);
    if key.len() <= 48 {
        key.to_string()
    } else {
        format!("…/{leaf}")
    }
}

fn blob_name(key: &str) -> String {
    let mut h = DefaultHasher::new();
    key.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn manifest_path(step_dir: &Path) -> PathBuf {
    step_dir.join("manifest.json")
}

fn load_manifest(step_dir: &Path) -> Option<Manifest> {
    let raw = fs::read(manifest_path(step_dir)).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn save_manifest(step_dir: &Path, manifest: &Manifest) -> Result<(), String> {
    let raw = serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?;
    fs::write(manifest_path(step_dir), raw).map_err(|e| e.to_string())
}

fn copy_file_bytes(src: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let bytes = fs::read(src).map_err(|e| e.to_string())?;
    fs::write(dest, &bytes).map_err(|e| e.to_string())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn dir_mtime_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(rd) = fs::read_dir(path) else {
        return 0;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            total = total.saturating_add(dir_size(&p));
        } else if let Ok(meta) = ent.metadata() {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perm::ConfigGuard;

    fn scratch(tag: &str) -> (PathBuf, PathBuf, ConfigGuard) {
        let base = std::env::temp_dir().join(format!(
            "gh-fcp-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&base);
        let cfg = base.join("cfg");
        let ws = base.join("ws");
        fs::create_dir_all(&cfg).unwrap();
        fs::create_dir_all(&ws).unwrap();
        let guard = ConfigGuard::set(&cfg);
        clear_active_for_tests();
        (base, ws, guard)
    }

    #[test]
    fn edit_twice_restore_step_one_gives_original_bytes() {
        let (base, ws, _guard) = scratch("twice");
        let path = ws.join("note.txt");
        fs::write(&path, b"original").unwrap();

        assert_eq!(begin_step("conv-a"), Some(1));
        before_write(&path);
        fs::write(&path, b"first-edit").unwrap();

        assert_eq!(begin_step("conv-a"), Some(2));
        before_write(&path);
        fs::write(&path, b"second-edit").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"second-edit");
        let msg = restore_step("conv-a", 1).unwrap();
        assert!(msg.contains("Restored step 1"), "{msg}");
        assert_eq!(fs::read(&path).unwrap(), b"original");

        // Restore created an undo step; re-restoring that undoes the restore.
        let steps = list_steps("conv-a");
        assert!(steps.len() >= 3, "{steps:?}");
        let undo = steps.last().unwrap().step;
        let _ = restore_step("conv-a", undo).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second-edit");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn new_file_restore_moves_aside_not_hard_delete_only() {
        let (base, ws, _guard) = scratch("new");
        let path = ws.join("fresh.txt");

        assert_eq!(begin_step("conv-b"), Some(1));
        before_write(&path);
        assert!(!path.exists());
        fs::write(&path, b"brand-new").unwrap();
        assert!(path.is_file());

        let msg = restore_step("conv-b", 1).unwrap();
        assert!(msg.contains("Restored"), "{msg}");
        assert!(!path.exists(), "new file should be gone from workspace");
        // Undo step kept the bytes.
        let undo_steps = list_steps("conv-b");
        let undo = undo_steps.last().unwrap().step;
        let undo_dir = step_dir("conv-b", undo);
        let m = load_manifest(&undo_dir).unwrap();
        assert_eq!(m.files.len(), 1);
        assert_eq!(m.files[0].kind, EntryKind::Content);
        let blob = m.files[0].blob.as_ref().unwrap();
        let bytes = fs::read(undo_dir.join("blobs").join(blob)).unwrap();
        assert_eq!(bytes, b"brand-new");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn sensitive_paths_are_never_copied() {
        let (base, _ws, _guard) = scratch("sens");
        let fake_home = base.join("home");
        let ssh = fake_home.join(".ssh").join("id_test_checkpoint");
        fs::create_dir_all(ssh.parent().unwrap()).unwrap();
        fs::write(&ssh, b"secret-key-material").unwrap();

        let prev_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &fake_home);

        assert_eq!(begin_step("conv-c"), Some(1));
        before_write(&ssh);
        let dir = step_dir("conv-c", 1);
        let m = load_manifest(&dir);
        assert!(
            m.as_ref().map(|m| m.files.is_empty()).unwrap_or(true),
            "sensitive must not be copied: {m:?}"
        );

        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn retention_prunes_oldest_runs() {
        let (base, ws, _guard) = scratch("prune");
        let path = ws.join("x.txt");
        fs::write(&path, b"v").unwrap();
        for i in 0..(MAX_RUNS + 3) {
            let id = format!("run-{i:02}");
            assert!(begin_step(&id).is_some());
            before_write(&path);
            let marker = conv_dir(&sanitize_id(&id)).join("touched");
            fs::write(&marker, [i as u8]).unwrap();
            // Ensure distinct mtimes across platforms.
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        prune_store();
        let root = checkpoints_root();
        let count = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_dir())
            .count();
        assert!(count <= MAX_RUNS, "kept {count} runs, cap is {MAX_RUNS}");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn format_steps_lists_files() {
        let (base, ws, _guard) = scratch("fmt");
        let path = ws.join("a.txt");
        fs::write(&path, b"hi").unwrap();
        begin_step("conv-fmt").unwrap();
        before_write(&path);
        let text = format_steps("conv-fmt");
        assert!(text.contains("step 1"), "{text}");
        assert!(text.contains("a.txt"), "{text}");
        let _ = fs::remove_dir_all(&base);
    }
}

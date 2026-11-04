//! Keep a sandboxed command from planting code that git later runs outside
//! the sandbox. The workspace's `.git/config` and `.git/hooks/` are inside
//! the writable workspace, but git runs hooks, `core.sshCommand`, filter
//! drivers and the like from them when the person (or the app) runs git with
//! no sandbox. Landlock can't carve a folder out of a writable root, so the
//! guard takes a copy before a sandboxed command and puts it back after.
//! A change a command really needs (`git remote add`, `git config`) goes
//! through a card with `outside_sandbox: true`, which skips the guard.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Hooks folders are small (the samples are about 25 KB); anything larger is
/// not copied, and then only the config file is guarded.
const MAX_HOOK_BYTES: u64 = 1_048_576;

/// What a command may not change, as it was before the command ran.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    config_path: PathBuf,
    config: Option<Vec<u8>>,
    hooks_dir: PathBuf,
    hooks: Option<BTreeMap<PathBuf, Vec<u8>>>,
}

/// The repository folder git reads for `workspace`: `.git` itself, or the
/// common folder of a linked worktree (where its config and hooks live).
pub fn common_dir(workspace: &Path) -> Option<PathBuf> {
    let dot = workspace.join(".git");
    let meta = fs::symlink_metadata(&dot).ok()?;
    let git_dir = if meta.is_dir() {
        dot
    } else if meta.is_file() {
        let text = fs::read_to_string(&dot).ok()?;
        let raw = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            path
        } else {
            workspace.join(path)
        }
    } else {
        return None;
    };
    match fs::read_to_string(git_dir.join("commondir")) {
        Ok(text) => {
            let raw = PathBuf::from(text.trim());
            Some(if raw.is_absolute() {
                raw
            } else {
                git_dir.join(raw)
            })
        }
        Err(_) => Some(git_dir),
    }
}

/// Folders one level down that are checked for a repository of their own,
/// for a workspace that holds several projects.
const MAX_CHILD_DIRS: usize = 64;

/// Every repository a sandboxed command could change: the workspace's own,
/// and those of the folders right inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guard(Vec<Snapshot>);

/// Copy the config and hooks before a sandboxed command. `None` when the
/// workspace holds no repository yet (so `git init` and `git clone` work).
pub fn take(workspace: &Path) -> Option<Guard> {
    let mut snaps: Vec<Snapshot> = take_repo(workspace).into_iter().collect();
    if let Ok(entries) = fs::read_dir(workspace) {
        for entry in entries.flatten().take(MAX_CHILD_DIRS) {
            let path = entry.path();
            if !fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            if let Some(snap) = take_repo(&path) {
                if !snaps.iter().any(|s| s.config_path == snap.config_path) {
                    snaps.push(snap);
                }
            }
        }
    }
    (!snaps.is_empty()).then_some(Guard(snaps))
}

/// Put back what the command changed in any guarded repository. Returns a
/// note for the command's output, or `None` when nothing changed.
pub fn restore(guard: &Guard) -> Option<String> {
    let mut changed: Vec<&'static str> = Vec::new();
    for snap in &guard.0 {
        for what in restore_repo(snap) {
            if !changed.contains(&what) {
                changed.push(what);
            }
        }
    }
    if changed.is_empty() {
        return None;
    }
    Some(format!(
        "The sandbox put back {}: a sandboxed command can't change git's config or hooks, \
         since git runs them later outside the sandbox. If this change is needed, run the \
         command again with `{}: true` so the person can approve it.",
        changed.join(" and "),
        super::OUTSIDE_ARG
    ))
}

fn take_repo(workspace: &Path) -> Option<Snapshot> {
    let common = common_dir(workspace)?;
    let config_path = common.join("config");
    let hooks_dir = common.join("hooks");
    Some(Snapshot {
        config: fs::read(&config_path).ok(),
        hooks: read_hooks(&hooks_dir),
        config_path,
        hooks_dir,
    })
}

fn read_hooks(dir: &Path) -> Option<BTreeMap<PathBuf, Vec<u8>>> {
    let mut out = BTreeMap::new();
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(dir) else {
        return Some(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).ok()?;
        if meta.is_dir() {
            // Hooks are files; a folder here is unusual. Guard it as a name.
            out.insert(path, Vec::new());
            continue;
        }
        total = total.saturating_add(meta.len());
        if total > MAX_HOOK_BYTES {
            return None;
        }
        out.insert(path, fs::read(entry.path()).ok()?);
    }
    Some(out)
}

/// Put back one repository's config and hooks; returns what changed.
fn restore_repo(snapshot: &Snapshot) -> Vec<&'static str> {
    let mut put_back = Vec::new();
    let now = fs::read(&snapshot.config_path).ok();
    if now != snapshot.config {
        let done = match &snapshot.config {
            Some(bytes) => write_back(&snapshot.config_path, bytes),
            None => fs::remove_file(&snapshot.config_path).is_ok(),
        };
        if done {
            put_back.push(".git/config");
        }
    }
    if let Some(before) = &snapshot.hooks {
        let after = read_hooks(&snapshot.hooks_dir);
        if after.as_ref() != Some(before) {
            restore_hooks(&snapshot.hooks_dir, before);
            put_back.push(".git/hooks");
        }
    }
    put_back
}

fn write_back(path: &Path, bytes: &[u8]) -> bool {
    // Replace a symlink or folder planted in its place, then write the bytes.
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.is_dir() {
            let _ = fs::remove_dir_all(path);
        } else if meta.file_type().is_symlink() {
            let _ = fs::remove_file(path);
        }
    }
    fs::write(path, bytes).is_ok()
}

fn restore_hooks(dir: &Path, before: &BTreeMap<PathBuf, Vec<u8>>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if before.contains_key(&path) {
                continue;
            }
            let is_dir = fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir());
            let _ = if is_dir {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
        }
    }
    for (path, bytes) in before {
        if bytes.is_empty() && fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        if fs::read(path).ok().as_deref() != Some(bytes.as_slice()) {
            let _ = fs::create_dir_all(dir);
            // A hook that was removed comes back without its run bit, so it
            // stays off until someone turns it on again: the safe side.
            let _ = write_back(path, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-git-guard-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn config_and_hooks_come_back_and_other_files_stay() {
        let ws = scratch("restore");
        let git = ws.join(".git");
        fs::create_dir_all(git.join("hooks")).unwrap();
        fs::write(git.join("config"), "[core]\n\tbare = false\n").unwrap();
        fs::write(git.join("hooks/pre-commit.sample"), "#!/bin/sh\n").unwrap();
        let snap = take(&ws).expect("a repository");
        assert_eq!(restore(&snap), None, "nothing changed");

        fs::write(
            git.join("config"),
            "[core]\n\tbare = false\n\tsshCommand = evil\n",
        )
        .unwrap();
        fs::write(git.join("hooks/post-checkout"), "#!/bin/sh\nevil\n").unwrap();
        fs::write(git.join("hooks/pre-commit.sample"), "changed").unwrap();
        fs::write(ws.join("src.rs"), "fn main() {}").unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let note = restore(&snap).expect("put back");
        assert!(
            note.contains(".git/config") && note.contains(".git/hooks"),
            "{note}"
        );
        assert!(note.contains(super::super::OUTSIDE_ARG), "{note}");
        assert_eq!(
            fs::read_to_string(git.join("config")).unwrap(),
            "[core]\n\tbare = false\n"
        );
        assert!(!git.join("hooks/post-checkout").exists());
        assert_eq!(
            fs::read_to_string(git.join("hooks/pre-commit.sample")).unwrap(),
            "#!/bin/sh\n"
        );
        assert!(ws.join("src.rs").exists(), "work files are the command's");
        assert!(
            git.join("HEAD").exists(),
            "refs and HEAD are git's own work"
        );
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn a_new_repository_and_a_missing_hooks_folder_are_fine() {
        let ws = scratch("fresh");
        assert!(take(&ws).is_none(), "git init and clone are not guarded");
        let git = ws.join(".git");
        fs::create_dir_all(&git).unwrap();
        fs::write(git.join("config"), "[core]\n").unwrap();
        let snap = take(&ws).unwrap();
        fs::create_dir_all(git.join("hooks")).unwrap();
        fs::write(git.join("hooks/pre-push"), "evil").unwrap();
        assert!(restore(&snap).is_some());
        assert!(!git.join("hooks/pre-push").exists());
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn a_linked_worktree_guards_the_main_repository() {
        let base = scratch("linked");
        let main = base.join("main");
        let wt = base.join("wt");
        let gitdir = main.join(".git/worktrees/wt");
        fs::create_dir_all(&gitdir).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(main.join(".git/config"), "[core]\n").unwrap();
        fs::write(gitdir.join("commondir"), "../..\n").unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        let common = common_dir(&wt).unwrap();
        assert_eq!(
            common.join("config").canonicalize().unwrap(),
            main.join(".git/config").canonicalize().unwrap()
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_project_folder_inside_the_workspace_is_guarded_too() {
        let ws = scratch("nested");
        let inner = ws.join("app/.git");
        fs::create_dir_all(inner.join("hooks")).unwrap();
        fs::write(inner.join("config"), "[core]\n").unwrap();
        fs::write(ws.join("notes.txt"), "a file, not a folder").unwrap();
        let guard = take(&ws).expect("the inner repository");
        fs::write(inner.join("config"), "[core]\n\tfsmonitor = evil\n").unwrap();
        fs::write(inner.join("hooks/pre-commit"), "evil").unwrap();
        let note = restore(&guard).expect("put back");
        assert!(
            note.contains(".git/config") && note.contains(".git/hooks"),
            "{note}"
        );
        assert_eq!(
            fs::read_to_string(inner.join("config")).unwrap(),
            "[core]\n"
        );
        assert!(!inner.join("hooks/pre-commit").exists());
        let _ = fs::remove_dir_all(&ws);
    }
}

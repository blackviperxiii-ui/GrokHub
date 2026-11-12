//! Keep a sandboxed command from planting code that git later runs outside
//! the sandbox. The workspace's `.git/config` and `.git/hooks/` are inside
//! the writable workspace, but git runs hooks, `core.sshCommand`, filter
//! drivers and the like from them when the person (or the app) runs git with
//! no sandbox. Landlock can't carve a folder out of a writable root, so the
//! guard takes a copy before a sandboxed command and puts it back after.
//! A change a command really needs (`git remote add`, `git config`) goes
//! through a card with `outside_sandbox: true`, which skips the guard.
//!
//! The guard itself runs outside the sandbox, so it must not be led out of
//! the workspace by a link the command left behind. It works on the real
//! path of the repository folder and puts things back only while that path
//! still has no link in it. It never reads, lists, deletes or writes through
//! a link, and a file it writes is always made new. A link that was there
//! before the command is kept as a link: where it points is guarded, what it
//! points to is not. A `.git` that was replaced or moved is reported, not
//! put back.
//!
//! The checks and the work that follows them use paths, so a command that is
//! still running could swap a link in between the two. Closing that needs
//! calls that work from an open folder, which this module does not make yet.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Hooks folders are small (the samples are about 25 KB); anything larger is
/// not copied, and then only the config file is guarded.
const MAX_HOOK_BYTES: u64 = 1_048_576;

/// Git itself refuses a `.git` pointer file over 1 MB.
const MAX_POINTER_BYTES: u64 = 1_048_576;

/// A config file larger than this is not copied and not guarded.
const MAX_CONFIG_BYTES: u64 = 16 * 1_048_576;

/// What the command's output says when the repository folder is no longer
/// the one that was copied from.
const REPLACED_NOTE: &str =
    "The sandbox found .git replaced by a sandboxed command; it was not put back.";

/// What `<workspace>/.git` was before the command.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DotGit {
    /// The repository folder itself.
    Dir,
    /// A linked worktree's or submodule's pointer file, with its bytes.
    Pointer(Vec<u8>),
}

/// The repository's config file before the command.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Config {
    /// A link, a folder, too large or unreadable: left alone.
    Unguarded,
    Absent,
    File(Vec<u8>),
}

/// One entry of the hooks folder before the command.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hook {
    File(Vec<u8>),
    /// Hooks are files; a folder here is unusual. Guarded as a name.
    Dir,
    /// A link, with where it points. What it points to is never read.
    Link(PathBuf),
    /// A pipe, socket or device: never opened.
    Special,
}

/// The repository's hooks folder before the command.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hooks {
    /// A plain file, too large or unreadable: left alone.
    Unguarded,
    /// A link to a folder kept elsewhere, with where it points.
    Link(PathBuf),
    /// Each hook by its file name, so a restored path is always right inside
    /// the hooks folder. Empty when there was no folder.
    Dir(BTreeMap<OsString, Hook>),
}

/// What a command may not change, as it was before the command ran.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    /// The real path of the folder that holds `.git`.
    work: PathBuf,
    dot: DotGit,
    /// The real path of the folder that holds `config` and `hooks`. For a
    /// linked worktree this is the main repository's, so it is still guarded
    /// after the worktree's own folder is moved or removed.
    common: PathBuf,
    config: Config,
    hooks: Hooks,
}

/// What is at a path, without following a link there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Absent,
    File,
    Dir,
    /// A link (on Windows a junction too), or anything else that is neither
    /// a regular file nor a real folder.
    Other,
}

fn kind(path: &Path) -> Kind {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Kind::Dir,
        Ok(meta) if meta.is_file() => Kind::File,
        Ok(_) => Kind::Other,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Kind::Absent,
        Err(_) => Kind::Other,
    }
}

/// Whether the real path `real` is still one: it exists and no part of it
/// has been swapped for a link.
fn still_real(real: &Path) -> bool {
    fs::canonicalize(real).is_ok_and(|now| now == real)
}

/// The bytes of the regular file at `path`. `None` when it is anything else
/// (a link is not followed and a pipe is not waited on), when it can't be
/// read, or when it holds more than `cap` bytes.
fn read_file(path: &Path, cap: u64) -> Option<Vec<u8>> {
    if kind(path) != Kind::File {
        return None;
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // For a swap made after the check above.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    u64::try_from(bytes.len())
        .is_ok_and(|len| len <= cap)
        .then_some(bytes)
}

/// Remove a file or a link itself, never what a link points to. A link to a
/// folder is removed as a file on Unix and as a folder on Windows.
fn remove_link(path: &Path) -> bool {
    fs::remove_file(path).is_ok() || fs::remove_dir(path).is_ok()
}

/// Remove whatever is at `path`: a real folder with what it holds, anything
/// else as the one entry it is.
fn clear(path: &Path) {
    if kind(path) == Kind::Dir {
        let _ = fs::remove_dir_all(path);
    } else {
        remove_link(path);
    }
}

/// Make a link at `at` that points to `target`, as it was before.
#[cfg(unix)]
fn make_link(target: &Path, at: &Path, _folder: bool) -> bool {
    std::os::unix::fs::symlink(target, at).is_ok()
}

/// Make a link at `at` that points to `target`, as it was before. Windows
/// may refuse (links need a privilege there); then nothing is left at `at`,
/// and git runs no hook from it.
#[cfg(windows)]
fn make_link(target: &Path, at: &Path, folder: bool) -> bool {
    if folder {
        std::os::windows::fs::symlink_dir(target, at).is_ok()
    } else {
        std::os::windows::fs::symlink_file(target, at).is_ok()
    }
}

/// The repository folder git reads for `workspace`: `.git` itself, or the
/// common folder of a linked worktree (where its config and hooks live).
pub fn common_dir(workspace: &Path) -> Option<PathBuf> {
    read_dot(workspace).map(|(_, common)| common)
}

/// What `<workspace>/.git` is, and the repository folder it leads to.
fn read_dot(workspace: &Path) -> Option<(DotGit, PathBuf)> {
    let dot = workspace.join(".git");
    let (was, git_dir) = match kind(&dot) {
        Kind::Dir => (DotGit::Dir, dot),
        Kind::File => {
            let bytes = read_file(&dot, MAX_POINTER_BYTES)?;
            let text = std::str::from_utf8(&bytes).ok()?;
            let raw = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
            let path = PathBuf::from(raw);
            let git_dir = if path.is_absolute() {
                path
            } else {
                workspace.join(path)
            };
            (DotGit::Pointer(bytes), git_dir)
        }
        Kind::Absent | Kind::Other => return None,
    };
    let named = read_file(&git_dir.join("commondir"), MAX_POINTER_BYTES)
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let common = match named {
        Some(text) => {
            let raw = PathBuf::from(text.trim());
            if raw.is_absolute() {
                raw
            } else {
                git_dir.join(raw)
            }
        }
        None => git_dir,
    };
    Some((was, common))
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
                if !snaps.iter().any(|s| s.common == snap.common) {
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
    let mut replaced = false;
    for snap in &guard.0 {
        let found = restore_repo(snap);
        replaced |= found.replaced;
        for what in found.put_back {
            if !changed.contains(&what) {
                changed.push(what);
            }
        }
    }
    let mut notes = Vec::new();
    if !changed.is_empty() {
        notes.push(format!(
            "The sandbox put back {}: a sandboxed command can't change git's config or hooks, \
             since git runs them later outside the sandbox. If this change is needed, run the \
             command again with `{}: true` so the person can approve it.",
            changed.join(" and "),
            super::OUTSIDE_ARG
        ));
    }
    if replaced {
        notes.push(REPLACED_NOTE.to_string());
    }
    (!notes.is_empty()).then(|| notes.join(" "))
}

fn take_repo(workspace: &Path) -> Option<Snapshot> {
    let (dot, named) = read_dot(workspace)?;
    if kind(&named) != Kind::Dir {
        return None;
    }
    let work = fs::canonicalize(workspace).ok()?;
    let common = fs::canonicalize(&named).ok()?;
    let config_path = common.join("config");
    let config = match kind(&config_path) {
        Kind::Absent => Config::Absent,
        Kind::File => {
            read_file(&config_path, MAX_CONFIG_BYTES).map_or(Config::Unguarded, Config::File)
        }
        Kind::Dir | Kind::Other => Config::Unguarded,
    };
    let hooks_dir = common.join("hooks");
    let hooks = match kind(&hooks_dir) {
        Kind::Absent => Hooks::Dir(BTreeMap::new()),
        Kind::Dir => read_hooks(&hooks_dir).map_or(Hooks::Unguarded, Hooks::Dir),
        Kind::File => Hooks::Unguarded,
        Kind::Other => fs::read_link(&hooks_dir).map_or(Hooks::Unguarded, Hooks::Link),
    };
    Some(Snapshot {
        work,
        dot,
        common,
        config,
        hooks,
    })
}

/// The hooks in `dir`, which the caller has checked is a real folder. `None`
/// when they are too large to copy or one can't be read.
fn read_hooks(dir: &Path) -> Option<BTreeMap<OsString, Hook>> {
    let mut out = BTreeMap::new();
    let mut left = MAX_HOOK_BYTES;
    let Ok(entries) = fs::read_dir(dir) else {
        return Some(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let hook = match kind(&path) {
            Kind::Absent => continue,
            Kind::Dir => Hook::Dir,
            Kind::File => {
                let bytes = read_file(&path, left)?;
                left = left.saturating_sub(u64::try_from(bytes.len()).ok()?);
                Hook::File(bytes)
            }
            Kind::Other => fs::read_link(&path).map_or(Hook::Special, Hook::Link),
        };
        out.insert(entry.file_name(), hook);
    }
    Some(out)
}

/// What one repository's check found.
struct Found {
    put_back: Vec<&'static str>,
    replaced: bool,
}

/// Put back one repository's pointer file, config and hooks.
fn restore_repo(snapshot: &Snapshot) -> Found {
    let mut found = Found {
        put_back: Vec::new(),
        replaced: false,
    };
    // A project folder swapped for a link would lead `.git` somewhere else.
    let work_there = still_real(&snapshot.work);
    let dot = snapshot.work.join(".git");
    if let DotGit::Pointer(bytes) = &snapshot.dot {
        if work_there {
            let same = read_file(&dot, MAX_POINTER_BYTES).as_ref() == Some(bytes);
            if !same && write_back(&dot, bytes) {
                found.put_back.push(".git");
            }
        } else if kind(&snapshot.work) != Kind::Absent {
            found.replaced = true;
        }
        // A worktree folder that was moved or removed is the command's own
        // business. The main repository below is guarded either way.
    }
    if kind(&snapshot.common) != Kind::Dir || !still_real(&snapshot.common) {
        // Nothing is written: the folder there now is not the one that was
        // copied. A link left as `.git` would send git wherever it points,
        // so the link itself goes.
        if snapshot.dot == DotGit::Dir && work_there && kind(&dot) == Kind::Other {
            remove_link(&dot);
        }
        // Where the folder that held the repository is gone as well, the
        // command deleted a project: there is nothing left for git to run.
        let holder_gone = snapshot
            .common
            .parent()
            .is_some_and(|holder| kind(holder) == Kind::Absent);
        found.replaced |= !holder_gone;
        return found;
    }
    let config_path = snapshot.common.join("config");
    let done = match &snapshot.config {
        Config::Unguarded => false,
        Config::Absent => {
            matches!(kind(&config_path), Kind::File | Kind::Other) && remove_link(&config_path)
        }
        Config::File(bytes) => {
            let same = read_file(&config_path, MAX_CONFIG_BYTES).as_ref() == Some(bytes);
            !same && write_back(&config_path, bytes)
        }
    };
    if done {
        found.put_back.push(".git/config");
    }
    if restore_hooks(&snapshot.common.join("hooks"), &snapshot.hooks) {
        found.put_back.push(".git/hooks");
    }
    found
}

/// Write `bytes` at `path` as a new file. Whatever is there goes first, and
/// `create_new` refuses a link planted in between, so the bytes never land
/// in a file outside the workspace through a link or a hard link.
fn write_back(path: &Path, bytes: &[u8]) -> bool {
    // A changed file keeps its mode, so a hook that was on stays on.
    let mode = fs::symlink_metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.permissions());
    clear(path);
    let Ok(mut file) = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    else {
        return false;
    };
    if file.write_all(bytes).is_err() {
        return false;
    }
    if let Some(mode) = mode {
        let _ = file.set_permissions(mode);
    }
    true
}

/// Make the hooks folder what it was before; returns whether it had changed.
fn restore_hooks(dir: &Path, before: &Hooks) -> bool {
    match before {
        Hooks::Unguarded => false,
        Hooks::Link(target) => {
            if fs::read_link(dir).ok().as_ref() == Some(target) {
                return false;
            }
            // Git would run hooks from whatever took the link's place.
            clear(dir);
            let _ = make_link(target, dir, true);
            true
        }
        Hooks::Dir(before) => restore_hook_files(dir, before),
    }
}

/// Make the hooks folder hold what it held before; returns whether it had
/// changed. The folder is checked again here, before anything is listed,
/// deleted or written: a link in its place is removed, never followed.
fn restore_hook_files(dir: &Path, before: &BTreeMap<OsString, Hook>) -> bool {
    match kind(dir) {
        Kind::Dir => {
            if read_hooks(dir).as_ref() == Some(before) {
                return false;
            }
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if !before.contains_key(&entry.file_name()) {
                        clear(&entry.path());
                    }
                }
            }
        }
        Kind::Other => {
            // Git would run hooks from wherever the link points.
            remove_link(dir);
            if before.is_empty() {
                return true;
            }
            let _ = fs::create_dir(dir);
        }
        Kind::Absent | Kind::File => {
            // Git runs nothing from a missing folder or a plain file.
            if before.is_empty() {
                return false;
            }
            remove_link(dir);
            let _ = fs::create_dir(dir);
        }
    }
    for (name, hook) in before {
        let path = dir.join(name);
        match hook {
            Hook::File(bytes) => {
                if read_file(&path, MAX_HOOK_BYTES).as_ref() != Some(bytes) {
                    // A hook that was removed comes back without its run bit,
                    // so it stays off until someone turns it on again: the
                    // safe side.
                    let _ = write_back(&path, bytes);
                }
            }
            Hook::Dir => {
                if kind(&path) != Kind::Dir {
                    clear(&path);
                    let _ = fs::create_dir(&path);
                }
            }
            Hook::Link(target) => {
                if fs::read_link(&path).ok().as_ref() != Some(target) {
                    clear(&path);
                    let _ = make_link(target, &path, false);
                }
            }
            Hook::Special => {
                // It can't be made again; what took its place goes.
                if kind(&path) != Kind::Other || fs::read_link(&path).is_ok() {
                    clear(&path);
                }
            }
        }
    }
    true
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

    const REPLACED: &str =
        "The sandbox found .git replaced by a sandboxed command; it was not put back.";

    /// Every file under `dir` with its text, sorted: the proof that a folder
    /// outside the workspace was not touched.
    fn listing(dir: &Path) -> Vec<(String, String)> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if fs::symlink_metadata(&path).unwrap().is_dir() {
                    walk(root, &path, out);
                } else {
                    let name = path.strip_prefix(root).unwrap();
                    let name = name.to_string_lossy().replace('\\', "/");
                    out.push((name, fs::read_to_string(&path).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(name, text)| (name.to_string(), text.to_string()))
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn a_hooks_folder_swapped_for_a_link_leaves_the_outside_folder_alone() {
        let base = scratch("hooks-link");
        let git = base.join("ws/.git");
        let outside = base.join("outside");
        fs::create_dir_all(git.join("hooks")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(git.join("config"), "[core]\n").unwrap();
        fs::write(git.join("hooks/pre-commit.sample"), "#!/bin/sh\n").unwrap();
        fs::write(outside.join("keep.txt"), "outside data\n").unwrap();
        fs::write(outside.join("pre-commit.sample"), "outside hook\n").unwrap();
        let before = listing(&outside);
        println!("outside before: {before:?}");
        let guard = take(&base.join("ws")).expect("a repository");

        fs::remove_dir_all(git.join("hooks")).unwrap();
        std::os::unix::fs::symlink(&outside, git.join("hooks")).unwrap();
        let note = restore(&guard).expect("put back");
        let after = listing(&outside);
        println!("outside after:  {after:?}");

        assert_eq!(
            after,
            pairs(&[
                ("keep.txt", "outside data\n"),
                ("pre-commit.sample", "outside hook\n"),
            ])
        );
        assert_eq!(
            note,
            "The sandbox put back .git/hooks: a sandboxed command can't change git's config \
             or hooks, since git runs them later outside the sandbox. If this change is \
             needed, run the command again with `outside_sandbox: true` so the person can \
             approve it."
        );
        let hooks = fs::symlink_metadata(git.join("hooks")).unwrap();
        assert!(hooks.is_dir(), "the link is gone and the folder is back");
        assert_eq!(
            listing(&git.join("hooks")),
            pairs(&[("pre-commit.sample", "#!/bin/sh\n")])
        );
        assert_eq!(restore(&guard), None, "a second pass finds nothing");
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_hooks_link_planted_where_no_folder_was_is_removed() {
        let base = scratch("hooks-planted");
        let git = base.join("ws/.git");
        let outside = base.join("outside");
        fs::create_dir_all(&git).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(git.join("config"), "[core]\n").unwrap();
        fs::write(outside.join("pre-commit"), "outside hook\n").unwrap();
        let guard = take(&base.join("ws")).expect("a repository");

        std::os::unix::fs::symlink(&outside, git.join("hooks")).unwrap();
        let note = restore(&guard).expect("put back");

        assert!(
            note.starts_with("The sandbox put back .git/hooks: "),
            "{note}"
        );
        assert!(fs::symlink_metadata(git.join("hooks")).is_err());
        assert_eq!(
            listing(&outside),
            pairs(&[("pre-commit", "outside hook\n")])
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_hooks_link_that_was_there_before_stays_and_is_not_followed() {
        let base = scratch("hooks-old-link");
        let git = base.join("ws/.git");
        let shared = base.join("shared-hooks");
        fs::create_dir_all(&git).unwrap();
        fs::create_dir_all(&shared).unwrap();
        fs::write(git.join("config"), "[core]\n").unwrap();
        fs::write(shared.join("pre-commit"), "#!/bin/sh\n").unwrap();
        std::os::unix::fs::symlink(&shared, git.join("hooks")).unwrap();
        let guard = take(&base.join("ws")).expect("a repository");

        fs::write(shared.join("added"), "new\n").unwrap();
        fs::write(git.join("config"), "[core]\n\tsshCommand = evil\n").unwrap();
        let note = restore(&guard).expect("the config is still guarded");

        assert!(
            note.starts_with("The sandbox put back .git/config: "),
            "{note}"
        );
        assert_eq!(fs::read_to_string(git.join("config")).unwrap(), "[core]\n");
        assert_eq!(
            listing(&shared),
            pairs(&[("added", "new\n"), ("pre-commit", "#!/bin/sh\n")])
        );
        assert!(fs::symlink_metadata(git.join("hooks"))
            .unwrap()
            .file_type()
            .is_symlink());
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_common_folder_swapped_for_a_link_is_reported_and_not_written() {
        let base = scratch("common-link");
        let ws = base.join("ws");
        let git = ws.join(".git");
        let outside = base.join("outside");
        fs::create_dir_all(git.join("hooks")).unwrap();
        fs::create_dir_all(outside.join("hooks")).unwrap();
        fs::write(git.join("config"), "[core]\n").unwrap();
        fs::write(git.join("hooks/pre-commit.sample"), "#!/bin/sh\n").unwrap();
        fs::write(outside.join("config"), "[outside]\n").unwrap();
        fs::write(outside.join("hooks/keep"), "outside hook\n").unwrap();
        let guard = take(&ws).expect("a repository");

        fs::rename(&git, ws.join("git-aside")).unwrap();
        std::os::unix::fs::symlink(&outside, &git).unwrap();
        let note = restore(&guard);

        assert_eq!(
            listing(&outside),
            pairs(&[("config", "[outside]\n"), ("hooks/keep", "outside hook\n")])
        );
        assert_eq!(note.as_deref(), Some(REPLACED));
        assert!(fs::symlink_metadata(&git).is_err(), "the link is gone");
        assert_eq!(
            listing(&ws.join("git-aside")),
            pairs(&[
                ("config", "[core]\n"),
                ("hooks/pre-commit.sample", "#!/bin/sh\n"),
            ]),
            "the folder moved aside is the command's to keep"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_project_folder_swapped_for_a_link_is_reported_and_not_written() {
        let base = scratch("project-link");
        let ws = base.join("ws");
        let inner = ws.join("app/.git");
        let other = base.join("other");
        fs::create_dir_all(inner.join("hooks")).unwrap();
        fs::create_dir_all(other.join(".git/hooks")).unwrap();
        fs::write(inner.join("config"), "[core]\n").unwrap();
        fs::write(other.join(".git/config"), "[other]\n").unwrap();
        fs::write(other.join(".git/hooks/pre-push"), "other hook\n").unwrap();
        let guard = take(&ws).expect("the inner repository");

        fs::rename(ws.join("app"), ws.join("app-aside")).unwrap();
        std::os::unix::fs::symlink(&other, ws.join("app")).unwrap();
        let note = restore(&guard);

        assert_eq!(
            listing(&other),
            pairs(&[
                (".git/config", "[other]\n"),
                (".git/hooks/pre-push", "other hook\n"),
            ])
        );
        assert_eq!(note.as_deref(), Some(REPLACED));
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_config_swapped_for_a_link_is_put_back_as_a_file() {
        let base = scratch("config-link");
        let git = base.join("ws/.git");
        fs::create_dir_all(&git).unwrap();
        fs::write(git.join("config"), "[core]\n").unwrap();
        fs::write(base.join("outside.txt"), "[core]\n").unwrap();
        let guard = take(&base.join("ws")).expect("a repository");

        fs::remove_file(git.join("config")).unwrap();
        std::os::unix::fs::symlink(base.join("outside.txt"), git.join("config")).unwrap();
        let note = restore(&guard).expect("put back");

        assert!(
            note.starts_with("The sandbox put back .git/config: "),
            "{note}"
        );
        assert!(fs::symlink_metadata(git.join("config")).unwrap().is_file());
        assert_eq!(fs::read_to_string(git.join("config")).unwrap(), "[core]\n");
        assert_eq!(
            fs::read_to_string(base.join("outside.txt")).unwrap(),
            "[core]\n"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_changed_hook_keeps_its_run_bit() {
        use std::os::unix::fs::PermissionsExt;
        let ws = scratch("run-bit");
        let hook = ws.join(".git/hooks/pre-commit");
        fs::create_dir_all(ws.join(".git/hooks")).unwrap();
        fs::write(ws.join(".git/config"), "[core]\n").unwrap();
        fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        let guard = take(&ws).expect("a repository");

        fs::write(&hook, "#!/bin/sh\nevil\n").unwrap();
        assert!(restore(&guard).is_some());

        assert_eq!(fs::read_to_string(&hook).unwrap(), "#!/bin/sh\nexit 0\n");
        assert_eq!(
            fs::metadata(&hook).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let _ = fs::remove_dir_all(&ws);
    }

    /// A worktree at `base/wt` linked to the repository at `base/main`.
    fn linked(base: &Path) -> (PathBuf, PathBuf, String) {
        let main = base.join("main");
        let wt = base.join("wt");
        let gitdir = main.join(".git/worktrees/wt");
        fs::create_dir_all(&gitdir).unwrap();
        fs::create_dir_all(main.join(".git/hooks")).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(main.join(".git/config"), "[core]\n").unwrap();
        fs::write(main.join(".git/hooks/pre-commit.sample"), "#!/bin/sh\n").unwrap();
        fs::write(gitdir.join("commondir"), "../..\n").unwrap();
        let pointer = format!("gitdir: {}\n", gitdir.display());
        fs::write(wt.join(".git"), &pointer).unwrap();
        (main, wt, pointer)
    }

    #[test]
    fn a_linked_worktree_gets_the_main_config_and_hooks_back() {
        let base = scratch("linked-restore");
        let (main, wt, pointer) = linked(&base);
        let guard = take(&wt).expect("a repository");
        assert_eq!(restore(&guard), None, "nothing changed");

        fs::write(main.join(".git/config"), "[core]\n\tfsmonitor = evil\n").unwrap();
        fs::write(main.join(".git/hooks/post-checkout"), "evil\n").unwrap();
        let note = restore(&guard).expect("put back");

        assert!(
            note.starts_with("The sandbox put back .git/config and .git/hooks: "),
            "{note}"
        );
        assert_eq!(
            listing(&main.join(".git")),
            pairs(&[
                ("config", "[core]\n"),
                ("hooks/pre-commit.sample", "#!/bin/sh\n"),
                ("worktrees/wt/commondir", "../..\n"),
            ])
        );
        assert_eq!(fs::read_to_string(wt.join(".git")).unwrap(), pointer);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_rewritten_pointer_file_is_put_back_with_its_bytes() {
        let base = scratch("pointer");
        let (main, wt, pointer) = linked(&base);
        let elsewhere = base.join("elsewhere");
        fs::create_dir_all(elsewhere.join("hooks")).unwrap();
        fs::write(elsewhere.join("config"), "[elsewhere]\n").unwrap();
        let guard = take(&wt).expect("a repository");

        fs::write(
            wt.join(".git"),
            format!("gitdir: {}\n", elsewhere.display()),
        )
        .unwrap();
        let note = restore(&guard).expect("put back");

        assert!(note.starts_with("The sandbox put back .git: "), "{note}");
        assert_eq!(fs::read_to_string(wt.join(".git")).unwrap(), pointer);
        assert_eq!(
            listing(&elsewhere),
            pairs(&[("config", "[elsewhere]\n")]),
            "the folder the new pointer named is not guarded or changed"
        );
        assert_eq!(
            fs::read_to_string(main.join(".git/config")).unwrap(),
            "[core]\n"
        );
        assert_eq!(restore(&guard), None, "a second pass finds nothing");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_git_folder_replaced_by_a_file_gives_the_note() {
        let ws = scratch("replaced");
        let git = ws.join(".git");
        fs::create_dir_all(git.join("hooks")).unwrap();
        fs::write(git.join("config"), "[core]\n").unwrap();
        fs::write(git.join("hooks/pre-commit.sample"), "#!/bin/sh\n").unwrap();
        let guard = take(&ws).expect("a repository");

        fs::remove_dir_all(&git).unwrap();
        fs::write(&git, "gitdir: elsewhere\n").unwrap();
        let note = restore(&guard);

        assert_eq!(note.as_deref(), Some(REPLACED));
        assert_eq!(fs::read_to_string(&git).unwrap(), "gitdir: elsewhere\n");
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn a_git_folder_that_is_gone_is_reported_and_not_made_again() {
        let ws = scratch("gone");
        let git = ws.join(".git");
        fs::create_dir_all(git.join("hooks")).unwrap();
        fs::write(git.join("config"), "[core]\n").unwrap();
        fs::write(git.join("hooks/pre-commit.sample"), "#!/bin/sh\n").unwrap();
        let guard = take(&ws).expect("a repository");

        fs::rename(&git, ws.join("git-aside")).unwrap();
        let note = restore(&guard);

        assert_eq!(note.as_deref(), Some(REPLACED));
        assert!(fs::symlink_metadata(&git).is_err(), "not made again");
        let _ = fs::remove_dir_all(&ws);
    }

    /// The whole note for config or hooks that were put back.
    fn put_back(what: &str) -> String {
        format!(
            "The sandbox put back {what}: a sandboxed command can't change git's config or \
             hooks, since git runs them later outside the sandbox. If this change is needed, \
             run the command again with `outside_sandbox: true` so the person can approve it."
        )
    }

    #[test]
    fn a_moved_worktree_still_gets_the_main_repository_back() {
        let base = scratch("wt-moved");
        let (main, wt, pointer) = linked(&base);
        let guard = take(&wt).expect("a repository");

        fs::rename(&wt, base.join("wt2")).unwrap();
        fs::write(main.join(".git/config"), "[core]\n\tsshCommand = evil\n").unwrap();
        fs::write(main.join(".git/hooks/pre-commit"), "evil\n").unwrap();
        let note = restore(&guard);

        assert_eq!(note, Some(put_back(".git/config and .git/hooks")));
        assert_eq!(
            listing(&main.join(".git")),
            pairs(&[
                ("config", "[core]\n"),
                ("hooks/pre-commit.sample", "#!/bin/sh\n"),
                ("worktrees/wt/commondir", "../..\n"),
            ])
        );
        assert!(fs::symlink_metadata(&wt).is_err(), "not made again");
        assert_eq!(
            fs::read_to_string(base.join("wt2/.git")).unwrap(),
            pointer,
            "the moved worktree is the command's to keep"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_pruned_worktree_still_gets_the_main_hooks_back() {
        let base = scratch("wt-pruned");
        let (main, wt, pointer) = linked(&base);
        let guard = take(&wt).expect("a repository");

        fs::remove_dir_all(main.join(".git/worktrees/wt")).unwrap();
        fs::write(main.join(".git/hooks/pre-commit"), "evil\n").unwrap();
        let note = restore(&guard);

        assert_eq!(note, Some(put_back(".git/hooks")));
        assert_eq!(
            listing(&main.join(".git")),
            pairs(&[
                ("config", "[core]\n"),
                ("hooks/pre-commit.sample", "#!/bin/sh\n"),
            ])
        );
        assert_eq!(fs::read_to_string(wt.join(".git")).unwrap(), pointer);
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_worktree_folder_swapped_for_a_link_gets_no_pointer_written() {
        let base = scratch("wt-link");
        let (main, wt, _pointer) = linked(&base);
        let other = base.join("other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join(".git"), "gitdir: somewhere\n").unwrap();
        let guard = take(&wt).expect("a repository");

        fs::rename(&wt, base.join("wt-aside")).unwrap();
        std::os::unix::fs::symlink(&other, &wt).unwrap();
        fs::write(main.join(".git/config"), "[core]\n\tsshCommand = evil\n").unwrap();
        let note = restore(&guard);

        assert_eq!(
            listing(&other),
            pairs(&[(".git", "gitdir: somewhere\n")]),
            "the pointer is not written through the link"
        );
        assert_eq!(
            note,
            Some(format!("{} {REPLACED}", put_back(".git/config")))
        );
        assert_eq!(
            fs::read_to_string(main.join(".git/config")).unwrap(),
            "[core]\n",
            "the main repository is at its own real path and still guarded"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_swapped_into_the_path_of_the_main_repository_is_not_written_through() {
        let base = scratch("main-link");
        let (main, wt, pointer) = linked(&base);
        let other = base.join("other");
        fs::create_dir_all(other.join(".git/hooks")).unwrap();
        fs::write(other.join(".git/config"), "[other]\n").unwrap();
        fs::write(other.join(".git/hooks/pre-push"), "other hook\n").unwrap();
        let guard = take(&wt).expect("a repository");

        fs::rename(&main, base.join("main-aside")).unwrap();
        std::os::unix::fs::symlink(&other, &main).unwrap();
        let note = restore(&guard);

        assert_eq!(
            listing(&other),
            pairs(&[
                (".git/config", "[other]\n"),
                (".git/hooks/pre-push", "other hook\n"),
            ])
        );
        assert_eq!(note.as_deref(), Some(REPLACED));
        assert_eq!(fs::read_to_string(wt.join(".git")).unwrap(), pointer);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_deleted_project_folder_is_not_reported() {
        let ws = scratch("project-gone");
        let inner = ws.join("app/.git");
        fs::create_dir_all(inner.join("hooks")).unwrap();
        fs::write(inner.join("config"), "[core]\n").unwrap();
        fs::write(inner.join("hooks/pre-commit.sample"), "#!/bin/sh\n").unwrap();
        let guard = take(&ws).expect("the inner repository");

        fs::remove_dir_all(ws.join("app")).unwrap();

        assert_eq!(restore(&guard), None);
        assert!(
            fs::symlink_metadata(ws.join("app")).is_err(),
            "not made again"
        );
        let _ = fs::remove_dir_all(&ws);
    }

    #[cfg(unix)]
    #[test]
    fn a_hooks_link_replaced_by_a_folder_is_made_a_link_again() {
        let base = scratch("hooks-link-replaced");
        let ws = base.join("ws");
        let git = ws.join(".git");
        fs::create_dir_all(&git).unwrap();
        fs::create_dir_all(ws.join(".githooks")).unwrap();
        fs::write(git.join("config"), "[core]\n").unwrap();
        fs::write(ws.join(".githooks/pre-commit"), "ok\n").unwrap();
        std::os::unix::fs::symlink("../.githooks", git.join("hooks")).unwrap();
        let guard = take(&ws).expect("a repository");
        assert_eq!(restore(&guard), None, "nothing changed");

        fs::remove_file(git.join("hooks")).unwrap();
        fs::create_dir(git.join("hooks")).unwrap();
        fs::write(git.join("hooks/pre-commit"), "evil\n").unwrap();
        let note = restore(&guard);

        assert_eq!(note, Some(put_back(".git/hooks")));
        assert_eq!(
            fs::read_link(git.join("hooks")).unwrap(),
            PathBuf::from("../.githooks")
        );
        assert_eq!(
            fs::read_to_string(git.join("hooks/pre-commit")).unwrap(),
            "ok\n"
        );
        assert_eq!(restore(&guard), None, "a second pass finds nothing");
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_hooks_link_pointed_somewhere_else_is_pointed_back() {
        let base = scratch("hooks-link-moved");
        let ws = base.join("ws");
        let git = ws.join(".git");
        fs::create_dir_all(&git).unwrap();
        fs::create_dir_all(ws.join(".githooks")).unwrap();
        fs::create_dir_all(ws.join("evil-hooks")).unwrap();
        fs::write(git.join("config"), "[core]\n").unwrap();
        fs::write(ws.join(".githooks/pre-commit"), "ok\n").unwrap();
        fs::write(ws.join("evil-hooks/pre-commit"), "evil\n").unwrap();
        std::os::unix::fs::symlink("../.githooks", git.join("hooks")).unwrap();
        let guard = take(&ws).expect("a repository");

        fs::remove_file(git.join("hooks")).unwrap();
        std::os::unix::fs::symlink("../evil-hooks", git.join("hooks")).unwrap();
        let note = restore(&guard);

        assert_eq!(note, Some(put_back(".git/hooks")));
        assert_eq!(
            fs::read_link(git.join("hooks")).unwrap(),
            PathBuf::from("../.githooks")
        );
        assert_eq!(
            listing(&ws.join("evil-hooks")),
            pairs(&[("pre-commit", "evil\n")]),
            "the folder the new link named is not listed or emptied"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_hook_that_is_a_link_stays_a_link_and_is_never_read() {
        let base = scratch("hook-link");
        let ws = base.join("ws");
        let hooks = ws.join(".git/hooks");
        fs::create_dir_all(&hooks).unwrap();
        fs::create_dir_all(ws.join("scripts")).unwrap();
        fs::write(ws.join(".git/config"), "[core]\n").unwrap();
        fs::write(ws.join("scripts/pre-commit"), "ok\n").unwrap();
        fs::write(base.join("secret.txt"), "secret\n").unwrap();
        std::os::unix::fs::symlink("../../scripts/pre-commit", hooks.join("pre-commit")).unwrap();
        let guard = take(&ws).expect("a repository");

        // The script is a project file: a command may edit it.
        fs::write(ws.join("scripts/pre-commit"), "edited\n").unwrap();
        assert_eq!(restore(&guard), None);
        assert_eq!(
            fs::read_to_string(ws.join("scripts/pre-commit")).unwrap(),
            "edited\n"
        );

        // A link to a file outside is planted, and the old link is replaced
        // by a file: neither is read, the first goes and the second is a
        // link again.
        std::os::unix::fs::symlink(base.join("secret.txt"), hooks.join("post-checkout")).unwrap();
        fs::remove_file(hooks.join("pre-commit")).unwrap();
        fs::write(hooks.join("pre-commit"), "evil\n").unwrap();
        let note = restore(&guard);

        assert_eq!(note, Some(put_back(".git/hooks")));
        assert!(fs::symlink_metadata(hooks.join("post-checkout")).is_err());
        assert_eq!(
            fs::read_link(hooks.join("pre-commit")).unwrap(),
            PathBuf::from("../../scripts/pre-commit")
        );
        assert_eq!(
            fs::read_to_string(base.join("secret.txt")).unwrap(),
            "secret\n"
        );
        assert_eq!(restore(&guard), None, "a second pass finds nothing");
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_hook_link_to_a_file_outside_is_not_copied_into_the_snapshot() {
        let base = scratch("hook-link-copy");
        let ws = base.join("ws");
        let hooks = ws.join(".git/hooks");
        fs::create_dir_all(&hooks).unwrap();
        fs::write(ws.join(".git/config"), "[core]\n").unwrap();
        fs::write(base.join("secret.txt"), "secret\n").unwrap();
        std::os::unix::fs::symlink(base.join("secret.txt"), hooks.join("pre-commit")).unwrap();
        let guard = take(&ws).expect("a repository");

        // An earlier command planted the link; this one swaps it for a
        // folder. The secret's bytes must not come back as a file here.
        fs::remove_file(hooks.join("pre-commit")).unwrap();
        fs::create_dir(hooks.join("pre-commit")).unwrap();
        let note = restore(&guard);

        assert_eq!(note, Some(put_back(".git/hooks")));
        assert_eq!(
            fs::read_link(hooks.join("pre-commit")).unwrap(),
            base.join("secret.txt")
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    fn make_pipe(path: &Path) {
        let made = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .unwrap();
        assert!(made.success(), "mkfifo {}", path.display());
    }

    #[cfg(unix)]
    #[test]
    fn a_pipe_planted_as_a_hook_is_removed_and_never_opened() {
        let ws = scratch("hook-pipe");
        let hooks = ws.join(".git/hooks");
        fs::create_dir_all(&hooks).unwrap();
        fs::write(ws.join(".git/config"), "[core]\n").unwrap();
        fs::write(hooks.join("pre-commit.sample"), "#!/bin/sh\n").unwrap();
        let guard = take(&ws).expect("a repository");

        // Opening a pipe nobody writes to would wait forever.
        make_pipe(&hooks.join("pre-commit"));
        fs::remove_file(hooks.join("pre-commit.sample")).unwrap();
        make_pipe(&hooks.join("pre-commit.sample"));
        let note = restore(&guard);

        assert_eq!(note, Some(put_back(".git/hooks")));
        assert_eq!(
            listing(&hooks),
            pairs(&[("pre-commit.sample", "#!/bin/sh\n")])
        );
        let _ = fs::remove_dir_all(&ws);
    }

    #[cfg(unix)]
    #[test]
    fn a_pipe_as_config_or_commondir_is_never_opened() {
        let base = scratch("pipes");
        let (main, wt, _pointer) = linked(&base);
        let guard = take(&wt).expect("a repository");

        fs::remove_file(main.join(".git/config")).unwrap();
        make_pipe(&main.join(".git/config"));
        assert_eq!(restore(&guard), Some(put_back(".git/config")));
        assert_eq!(
            fs::read_to_string(main.join(".git/config")).unwrap(),
            "[core]\n"
        );

        let gitdir = main.join(".git/worktrees/wt");
        fs::remove_file(gitdir.join("commondir")).unwrap();
        make_pipe(&gitdir.join("commondir"));
        assert_eq!(
            common_dir(&wt),
            Some(gitdir),
            "an unreadable commondir means the worktree's own folder, as before"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_hook_grown_past_what_is_copied_is_put_back() {
        let ws = scratch("hook-big");
        let hook = ws.join(".git/hooks/pre-commit");
        fs::create_dir_all(ws.join(".git/hooks")).unwrap();
        fs::write(ws.join(".git/config"), "[core]\n").unwrap();
        fs::write(&hook, "#!/bin/sh\n").unwrap();
        let guard = take(&ws).expect("a repository");

        fs::write(&hook, vec![b'x'; 1_048_577]).unwrap();
        let note = restore(&guard);

        assert_eq!(note, Some(put_back(".git/hooks")));
        assert_eq!(fs::read_to_string(&hook).unwrap(), "#!/bin/sh\n");
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn hooks_too_large_to_copy_leave_the_config_guarded() {
        let ws = scratch("hooks-big");
        let hook = ws.join(".git/hooks/pre-commit");
        fs::create_dir_all(ws.join(".git/hooks")).unwrap();
        fs::write(ws.join(".git/config"), "[core]\n").unwrap();
        fs::write(&hook, vec![b'x'; 1_048_577]).unwrap();
        let guard = take(&ws).expect("a repository");

        fs::write(&hook, "changed\n").unwrap();
        fs::write(ws.join(".git/config"), "[core]\n\tsshCommand = evil\n").unwrap();
        let note = restore(&guard);

        assert_eq!(note, Some(put_back(".git/config")));
        assert_eq!(fs::read_to_string(&hook).unwrap(), "changed\n");
        assert_eq!(
            fs::read_to_string(ws.join(".git/config")).unwrap(),
            "[core]\n"
        );
        let _ = fs::remove_dir_all(&ws);
    }
}

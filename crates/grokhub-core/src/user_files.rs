//! Where the user's own files go: chat exports, diagnostics, recordings,
//! images and reports. They live in one visible folder, `<Documents>/Loadout`
//! by default, with a plain-named folder for each kind. The app's internal
//! data stays in the hidden config folder.
//!
//! The chosen folder is remembered in `<config>/folders.json` the first time
//! it is used, so a later rename of the product never moves it.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The file in the config folder that remembers where your files go.
pub const FOLDERS_FILE: &str = "folders.json";

/// One kind of file the user keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserFolder {
    Chats,
    Diagnostics,
    Recordings,
    Images,
    Reports,
}

impl UserFolder {
    pub const ALL: [UserFolder; 5] = [
        UserFolder::Chats,
        UserFolder::Diagnostics,
        UserFolder::Recordings,
        UserFolder::Images,
        UserFolder::Reports,
    ];

    /// The folder's name on disk, also how the app names it to the user.
    pub fn name(self) -> &'static str {
        match self {
            UserFolder::Chats => "Chats",
            UserFolder::Diagnostics => "Diagnostics",
            UserFolder::Recordings => "Recordings",
            UserFolder::Images => "Images",
            UserFolder::Reports => "Reports",
        }
    }

    /// What goes in it, for Settings.
    pub fn what(self) -> &'static str {
        match self {
            UserFolder::Chats => "Exported chats",
            UserFolder::Diagnostics => "Diagnostics for support",
            UserFolder::Recordings => "Screen recordings",
            UserFolder::Images => "Saved images",
            UserFolder::Reports => "Internet log and week recap images",
        }
    }
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct FoldersFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    files_root: Option<String>,
}

fn read_folders(config: &Path) -> FoldersFile {
    std::fs::read_to_string(config.join(FOLDERS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_folders(config: &Path, f: &FoldersFile) -> std::io::Result<()> {
    std::fs::create_dir_all(config)?;
    let body = serde_json::to_string_pretty(f).map_err(std::io::Error::other)?;
    let tmp = config.join(format!(".{FOLDERS_FILE}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, config.join(FOLDERS_FILE))
}

/// Where your files go when nothing was chosen yet: `<Documents>/<product>`,
/// else `<home>/<product>`, else a `files` folder in the config folder.
pub fn default_files_root(config: &Path) -> PathBuf {
    default_files_root_in(
        crate::documents_dir().filter(|d| d.is_dir()),
        crate::user_home(),
        config,
        crate::brand::product_name(),
    )
}

fn default_files_root_in(
    documents: Option<PathBuf>,
    home: Option<PathBuf>,
    config: &Path,
    product: &str,
) -> PathBuf {
    let name = folder_safe(product);
    documents
        .or(home)
        .map(|base| base.join(&name))
        .unwrap_or_else(|| config.join("files"))
}

/// A product name as one folder name: no separators or reserved characters.
fn folder_safe(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').to_string();
    if cleaned.is_empty() {
        crate::brand::INTERNAL_ID.to_string()
    } else {
        cleaned
    }
}

/// Where your files go: the folder chosen in Settings (or remembered on first
/// use), else [`default_files_root`].
pub fn files_root(config: &Path) -> PathBuf {
    read_folders(config)
        .files_root
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_files_root(config))
}

/// True when the user picked the folder (Settings) rather than the default.
pub fn files_root_is_custom(config: &Path) -> bool {
    read_folders(config)
        .files_root
        .is_some_and(|s| !s.trim().is_empty() && Path::new(s.trim()) != default_files_root(config))
}

/// The folder path for `kind`, without creating anything.
pub fn folder_path(config: &Path, kind: UserFolder) -> PathBuf {
    files_root(config).join(kind.name())
}

/// The folder for `kind`, created when missing. The first use remembers the
/// root in `folders.json` so it never moves on its own.
pub fn folder(config: &Path, kind: UserFolder) -> std::io::Result<PathBuf> {
    let mut f = read_folders(config);
    let root = files_root(config);
    let dir = root.join(kind.name());
    std::fs::create_dir_all(&dir)?;
    if f.files_root.as_deref().is_none_or(|s| s.trim().is_empty()) {
        f.files_root = Some(root.to_string_lossy().into_owned());
        // Best effort: the folder works without the note.
        let _ = write_folders(config, &f);
    }
    Ok(dir)
}

/// Why a folder can't hold your files, in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderProblem {
    NotAbsolute,
    InsideAppData,
    CantCreate(String),
    ReadOnly(String),
}

impl std::fmt::Display for FolderProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FolderProblem::NotAbsolute => write!(f, "pick a full folder path"),
            FolderProblem::InsideAppData => {
                write!(f, "that folder is inside the app's own data folder")
            }
            FolderProblem::CantCreate(e) => write!(f, "couldn't create it ({e})"),
            FolderProblem::ReadOnly(e) => write!(f, "can't save files there ({e})"),
        }
    }
}

/// Check that `dir` can hold your files: a full path, outside the app's own
/// data folder, and writable (a test file is written and removed).
pub fn check_files_root(config: &Path, dir: &Path) -> Result<(), FolderProblem> {
    if !dir.is_absolute() {
        return Err(FolderProblem::NotAbsolute);
    }
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    std::fs::create_dir_all(dir).map_err(|e| FolderProblem::CantCreate(e.to_string()))?;
    if canon(dir).starts_with(canon(config)) {
        return Err(FolderProblem::InsideAppData);
    }
    let probe = dir.join(format!(".write-test-{}", std::process::id()));
    let wrote = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .and_then(|mut f| f.write_all(b"ok"));
    let _ = std::fs::remove_file(&probe);
    wrote.map_err(|e| FolderProblem::ReadOnly(e.to_string()))
}

/// Make `dir` the home for your files from now on. Files already saved stay
/// where they are.
pub fn set_files_root(config: &Path, dir: &Path) -> Result<(), FolderProblem> {
    check_files_root(config, dir)?;
    let mut f = read_folders(config);
    f.files_root = Some(dir.to_string_lossy().into_owned());
    write_folders(config, &f).map_err(|e| FolderProblem::CantCreate(e.to_string()))
}

/// Go back to the default folder for new files.
pub fn reset_files_root(config: &Path) -> std::io::Result<()> {
    let mut f = read_folders(config);
    f.files_root = Some(default_files_root(config).to_string_lossy().into_owned());
    write_folders(config, &f)
}

/// Write `body` to `dir/name`, or `name` with `-2` … `-99` before the
/// extension when taken. Never overwrites. Returns the path written.
pub fn write_new_file(dir: &Path, name: &str, body: &[u8]) -> std::io::Result<PathBuf> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for n in 1..=99u32 {
        let path = if n == 1 {
            dir.join(name)
        } else {
            dir.join(format!("{stem}-{n}{ext}"))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                if let Err(e) = f.write_all(body).and_then(|_| f.sync_all()) {
                    drop(f);
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "too many files with this name",
    ))
}

/// A save error in plain words for the person reading it.
pub fn plain_io_error(e: &std::io::Error) -> String {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::PermissionDenied => "the folder doesn't allow saving there".into(),
        K::NotFound => "the folder isn't there".into(),
        K::StorageFull => "the drive is full".into(),
        K::ReadOnlyFilesystem => "the drive is read-only".into(),
        K::AlreadyExists => "too many files with this name".into(),
        _ => e.to_string(),
    }
}

/// What [`bring_over`] did, counted by item (a file, or a folder such as one
/// recording).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BringOver {
    /// Moved on the same drive (a rename: nothing copied, nothing lost).
    pub moved: usize,
    /// Copied to another drive and checked; the originals were kept.
    pub copied: usize,
    /// Already there from an earlier run (same name and size).
    pub already: usize,
    /// Items that couldn't be brought over, in plain words.
    pub failed: Vec<String>,
}

impl BringOver {
    /// One line for the Storage block.
    pub fn summary(&self) -> String {
        let n = |k: usize| {
            if k == 1 {
                "1 item".to_string()
            } else {
                format!("{k} items")
            }
        };
        let mut parts = Vec::new();
        if self.moved > 0 {
            parts.push(format!("Moved {}", n(self.moved)));
        }
        if self.copied > 0 {
            parts.push(format!(
                "Copied {} (the originals are still in the old folder)",
                n(self.copied)
            ));
        }
        if self.already > 0 {
            parts.push(format!("{} already there", n(self.already)));
        }
        if !self.failed.is_empty() {
            parts.push(format!(
                "{} couldn't be brought over: {}",
                n(self.failed.len()),
                self.failed.join("; ")
            ));
        }
        if parts.is_empty() {
            "Nothing to bring over".into()
        } else {
            parts.join(". ") + "."
        }
    }
}

/// Bring every item directly inside `from` into `to`. On the same drive each
/// item is renamed into place. Across drives it is copied, every file's size
/// is checked, and the original stays where it was. A name already taken in
/// `to` gets `-2`, `-3`, …; an item already there with the same size counts
/// as done, so running it again copies nothing twice. Links are skipped.
/// Nothing is ever deleted.
pub fn bring_over(from: &Path, to: &Path) -> BringOver {
    bring_over_with(from, to, |a, b| std::fs::rename(a, b))
}

fn bring_over_with(
    from: &Path,
    to: &Path,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> BringOver {
    let mut done = BringOver::default();
    if from == to || to.starts_with(from) || from.starts_with(to) {
        return done;
    }
    let Ok(entries) = std::fs::read_dir(from) else {
        return done;
    };
    if let Err(e) = std::fs::create_dir_all(to) {
        done.failed.push(plain_io_error(&e));
        return done;
    }
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let src = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = std::fs::symlink_metadata(&src) else {
            continue;
        };
        if meta.file_type().is_symlink() || name.starts_with('.') {
            continue;
        }
        let same = to.join(&name);
        if same.exists() && tree_size(&same) == tree_size(&src) {
            done.already += 1;
            continue;
        }
        let Some(dest) = free_name(to, &name) else {
            done.failed
                .push(format!("{name}: too many items with this name"));
            continue;
        };
        match rename(&src, &dest) {
            Ok(()) => done.moved += 1,
            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
                match copy_checked(&src, &dest) {
                    Ok(()) => done.copied += 1,
                    Err(e) => {
                        // Only our half-made copy goes; the original stays.
                        let _ = if dest.is_dir() {
                            std::fs::remove_dir_all(&dest)
                        } else {
                            std::fs::remove_file(&dest)
                        };
                        done.failed.push(format!("{name}: {}", plain_io_error(&e)));
                    }
                }
            }
            Err(e) => done.failed.push(format!("{name}: {}", plain_io_error(&e))),
        }
    }
    done
}

/// How many items in `from` [`bring_over`] would still bring into `to`
/// (hidden files, links and items already there with the same size don't
/// count).
pub fn waiting_to_bring_over(from: &Path, to: &Path) -> usize {
    if from == to || to.starts_with(from) || from.starts_with(to) {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(from) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let src = e.path();
            !name.to_string_lossy().starts_with('.')
                && e.file_type().is_ok_and(|t| !t.is_symlink())
                && !(to.join(&name).exists() && tree_size(&to.join(&name)) == tree_size(&src))
        })
        .count()
}

/// `dir/name`, or with `-2` … `-99` before the extension when taken.
fn free_name(dir: &Path, name: &str) -> Option<PathBuf> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (1..=99u32)
        .map(|n| {
            if n == 1 {
                dir.join(name)
            } else {
                dir.join(format!("{stem}-{n}{ext}"))
            }
        })
        .find(|p| std::fs::symlink_metadata(p).is_err())
}

/// Bytes in a file, or in every file under a folder (links not followed).
fn tree_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    std::fs::read_dir(path)
        .map(|rd| rd.flatten().map(|e| tree_size(&e.path())).sum())
        .unwrap_or(0)
}

/// Copy `src` (file or folder) to `dest`, checking each file's size.
fn copy_checked(src: &Path, dest: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.is_dir() {
        std::fs::create_dir(dest)?;
        for entry in std::fs::read_dir(src)?.flatten() {
            let ty = entry.file_type()?;
            if ty.is_symlink() {
                continue;
            }
            copy_checked(&entry.path(), &dest.join(entry.file_name()))?;
        }
        return Ok(());
    }
    let wrote = std::fs::copy(src, dest)?;
    let on_disk = std::fs::metadata(dest)?.len();
    if wrote != meta.len() || on_disk != meta.len() {
        return Err(std::io::Error::other("the copy came out a different size"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(root: &Path) {
        std::fs::create_dir_all(root.join("2026-10-01-1")).unwrap();
        std::fs::write(root.join("2026-10-01-1/frame-001.png"), b"png-one").unwrap();
        std::fs::write(root.join("cat.png"), b"meow").unwrap();
        std::fs::write(root.join(".hidden"), b"x").unwrap();
    }

    #[test]
    fn bring_over_on_one_drive_renames_and_a_taken_name_gets_a_number() {
        let root = temp("bring-same");
        let (from, to) = (root.join("old"), root.join("new"));
        tree(&from);
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(to.join("cat.png"), b"a different cat").unwrap();
        let done = bring_over(&from, &to);
        assert_eq!(
            (done.moved, done.copied, done.already),
            (2, 0, 0),
            "{done:?}"
        );
        assert!(done.failed.is_empty());
        assert_eq!(
            std::fs::read(to.join("cat.png")).unwrap(),
            b"a different cat"
        );
        assert_eq!(std::fs::read(to.join("cat-2.png")).unwrap(), b"meow");
        assert_eq!(
            std::fs::read(to.join("2026-10-01-1/frame-001.png")).unwrap(),
            b"png-one"
        );
        assert!(from.join(".hidden").exists(), "hidden files stay put");
        assert_eq!(done.summary(), "Moved 2 items.");
        assert_eq!(waiting_to_bring_over(&from, &to), 0);
        assert_eq!(bring_over(&to, &to.join("inner")), BringOver::default());
        assert!(
            !to.join("inner").exists(),
            "overlapping folders are left alone"
        );
        assert_eq!(bring_over(&from, &to).summary(), "Nothing to bring over");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bring_over_across_drives_copies_keeps_originals_and_never_repeats() {
        let root = temp("bring-cross");
        let (from, to) = (root.join("old"), root.join("new"));
        tree(&from);
        let cross = |_: &Path, _: &Path| Err(std::io::ErrorKind::CrossesDevices.into());
        assert_eq!(waiting_to_bring_over(&from, &to), 2);
        let done = bring_over_with(&from, &to, cross);
        assert_eq!(
            waiting_to_bring_over(&from, &to),
            0,
            "copied items no longer count as waiting though the originals stay"
        );
        assert_eq!((done.moved, done.copied), (0, 2), "{done:?}");
        assert!(from.join("cat.png").exists() && from.join("2026-10-01-1").exists());
        assert_eq!(std::fs::read(to.join("cat.png")).unwrap(), b"meow");
        assert_eq!(
            done.summary(),
            "Copied 2 items (the originals are still in the old folder)."
        );
        let again = bring_over_with(&from, &to, cross);
        assert_eq!((again.copied, again.already), (0, 2), "{again:?}");
        assert!(!to.join("cat-2.png").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bring_over_from_a_missing_folder_does_nothing_and_a_failed_rename_is_reported() {
        let root = temp("bring-missing");
        let done = bring_over(&root.join("never"), &root.join("new"));
        assert_eq!(done, BringOver::default());
        assert!(!root.join("new").exists(), "nothing made for nothing");
        let from = root.join("old");
        tree(&from);
        let denied = |_: &Path, _: &Path| Err(std::io::ErrorKind::PermissionDenied.into());
        let done = bring_over_with(&from, &root.join("new"), denied);
        assert_eq!(done.failed.len(), 2);
        assert!(done
            .summary()
            .starts_with("2 items couldn't be brought over: "));
        assert!(from.join("cat.png").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("user-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn folder_names_are_plain_words() {
        let names: Vec<&str> = UserFolder::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(
            names,
            ["Chats", "Diagnostics", "Recordings", "Images", "Reports"]
        );
    }

    #[test]
    fn default_root_is_documents_then_home_then_config() {
        let cfg = PathBuf::from("/cfg");
        assert_eq!(
            default_files_root_in(
                Some("/home/user/Documents".into()),
                Some("/home/user".into()),
                &cfg,
                "Loadout"
            ),
            PathBuf::from("/home/user/Documents/Loadout")
        );
        assert_eq!(
            default_files_root_in(None, Some("/home/user".into()), &cfg, "Loadout"),
            PathBuf::from("/home/user/Loadout")
        );
        assert_eq!(
            default_files_root_in(None, None, &cfg, "Loadout"),
            PathBuf::from("/cfg/files")
        );
        assert_eq!(
            default_files_root_in(None, Some("/h".into()), &cfg, "My: App?"),
            PathBuf::from("/h/My App")
        );
    }

    #[test]
    fn first_use_remembers_the_root() {
        let root = temp("remember");
        let cfg = root.join("cfg");
        let files = root.join("files");
        set_files_root(&cfg, &files).unwrap();
        let chats = folder(&cfg, UserFolder::Chats).unwrap();
        assert_eq!(chats, files.join("Chats"));
        assert!(chats.is_dir());
        let saved = std::fs::read_to_string(cfg.join(FOLDERS_FILE)).unwrap();
        assert!(saved.contains("files_root"), "{saved}");
        assert_eq!(files_root(&cfg), files);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unset_root_is_written_on_first_folder() {
        let root = temp("unset");
        let cfg = root.join("cfg");
        std::fs::create_dir_all(&cfg).unwrap();
        // A hand-written file with only the default root, as first use writes.
        std::fs::write(
            cfg.join(FOLDERS_FILE),
            format!(
                "{{\"files_root\": {:?}}}",
                root.join("mine").to_string_lossy()
            ),
        )
        .unwrap();
        assert_eq!(
            folder(&cfg, UserFolder::Reports).unwrap(),
            root.join("mine/Reports")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_inside_app_data_or_relative_is_refused() {
        let root = temp("refuse");
        let cfg = root.join("cfg");
        std::fs::create_dir_all(&cfg).unwrap();
        assert_eq!(
            set_files_root(&cfg, &cfg.join("inside")),
            Err(FolderProblem::InsideAppData)
        );
        assert_eq!(
            set_files_root(&cfg, Path::new("relative/dir")),
            Err(FolderProblem::NotAbsolute)
        );
        assert!(!cfg.join(FOLDERS_FILE).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_new_file_never_overwrites() {
        let dir = temp("nooverwrite");
        let a = write_new_file(&dir, "chat.md", b"one").unwrap();
        let b = write_new_file(&dir, "chat.md", b"two").unwrap();
        assert_eq!(a, dir.join("chat.md"));
        assert_eq!(b, dir.join("chat-2.md"));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "two");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plain_errors_read_like_words() {
        let e = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(plain_io_error(&e), "the folder doesn't allow saving there");
        let e = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(plain_io_error(&e), "the folder isn't there");
    }
}

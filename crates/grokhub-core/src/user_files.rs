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

#[cfg(test)]
mod tests {
    use super::*;

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

//! The one door every indexer read goes through, so a test can count reads
//! (all scopes off ⇒ zero) and see which paths were touched.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// What a directory entry is. Symlinks are never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
    Symlink,
    Other,
}

/// One entry from [`IndexFs::list`] or [`IndexFs::stat`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
    pub len: u64,
    pub modified_ms: u64,
}

/// Read-only file access for the indexers. `copy` writes only the temp copy
/// of a browser history file, under the cabin's own folder.
pub trait IndexFs: Send + Sync {
    /// Entries of `dir`, sorted by name. Does not follow symlinks.
    fn list(&self, dir: &Path) -> std::io::Result<Vec<Entry>>;
    /// The entry itself (not its symlink target), if it is there.
    fn stat(&self, path: &Path) -> Option<Entry>;
    /// At most `cap` bytes from the start of a file.
    fn read_head(&self, path: &Path, cap: u64) -> std::io::Result<Vec<u8>>;
    /// Copy `from` to `to` (a new file). Returns the bytes copied.
    fn copy(&self, from: &Path, to: &Path) -> std::io::Result<u64>;
}

/// The real disk.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealFs;

fn entry_of(path: PathBuf, meta: &fs::Metadata) -> Entry {
    let ft = meta.file_type();
    let kind = if ft.is_symlink() {
        EntryKind::Symlink
    } else if ft.is_dir() {
        EntryKind::Dir
    } else if ft.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Entry { path, name, kind, len: meta.len(), modified_ms }
}

impl IndexFs for RealFs {
    fn list(&self, dir: &Path) -> std::io::Result<Vec<Entry>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(dir)?.flatten() {
            let path = entry.path();
            if let Ok(meta) = fs::symlink_metadata(&path) {
                out.push(entry_of(path, &meta));
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn stat(&self, path: &Path) -> Option<Entry> {
        fs::symlink_metadata(path).ok().map(|m| entry_of(path.to_path_buf(), &m))
    }

    fn read_head(&self, path: &Path, cap: u64) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        fs::File::open(path)?.take(cap).read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn copy(&self, from: &Path, to: &Path) -> std::io::Result<u64> {
        fs::copy(from, to)
    }
}

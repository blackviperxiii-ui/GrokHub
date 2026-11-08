//! Recent game clips and captures, found by looking (read-only) where
//! capture tools save them: the Videos folder (NVIDIA, Xbox Game Bar and
//! OBS save there by default) and Steam's own game recordings. Nothing is
//! opened, decoded or sent; only names, times and sizes are read.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Video files a capture tool writes.
const VIDEO: [&str; 5] = ["mp4", "mkv", "webm", "mov", "flv"];
/// How deep under a root to look (Videos/<Game>/clip.mp4 is depth 2).
const DEPTH: usize = 3;
/// Stop after this many directory entries, so a huge folder can't stall.
const ENTRIES_MAX: usize = 5_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    /// The file or Steam clip folder name, without the extension.
    pub name: String,
    pub path: PathBuf,
    /// Last change, unix ms.
    pub modified_ms: u64,
}

/// Where clips are saved on this computer: the user's Videos folder and
/// each Steam account's game-recording clips.
pub fn roots(steam_root: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(v) = dirs::video_dir() {
        out.push(v);
    }
    if let Some(steam) = steam_root {
        if let Ok(users) = std::fs::read_dir(steam.join("userdata")) {
            for u in users.flatten() {
                let clips = u.path().join("gamerecordings").join("clips");
                if clips.is_dir() {
                    out.push(clips);
                }
            }
        }
    }
    out
}

fn modified_ms(path: &Path) -> Option<u64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}

/// The newest `limit` clips under `roots`, newest first. A Steam clip is a
/// folder named `clip_…`; anything else is a video file.
pub fn recent(roots: &[PathBuf], limit: usize) -> Vec<Clip> {
    let mut found = Vec::new();
    let mut seen = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            seen += 1;
            if seen > ENTRIES_MAX {
                break;
            }
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = e.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if name.starts_with("clip_") {
                    if let Some(at) = modified_ms(&path) {
                        found.push(Clip {
                            name,
                            path,
                            modified_ms: at,
                        });
                    }
                } else if depth + 1 < DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let ext = path
                .extension()
                .map(|x| x.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !VIDEO.contains(&ext.as_str()) {
                continue;
            }
            if let Some(at) = modified_ms(&path) {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or(name);
                found.push(Clip {
                    name: stem,
                    path,
                    modified_ms: at,
                });
            }
        }
        if seen > ENTRIES_MAX {
            break;
        }
    }
    found.sort_by_key(|c| std::cmp::Reverse(c.modified_ms));
    found.truncate(limit);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_videos_and_steam_clip_folders_newest_first() {
        let root = std::env::temp_dir().join(format!(
            "grokhub-clips-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        let game = root.join("Videos").join("Apex Legends");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("Clutch 1v3.mp4"), b"x").unwrap();
        std::fs::write(game.join("notes.txt"), b"x").unwrap();
        let steam = root.join("steam").join("clip_570_20261105");
        std::fs::create_dir_all(&steam).unwrap();
        let clips = recent(&[root.join("Videos"), root.join("steam")], 8);
        let names: Vec<&str> = clips.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(clips.len(), 2, "{names:?}");
        assert!(names.contains(&"Clutch 1v3"));
        assert!(names.contains(&"clip_570_20261105"));
        assert!(recent(&[root.join("missing")], 8).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}

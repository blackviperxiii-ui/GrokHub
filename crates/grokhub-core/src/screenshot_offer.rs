//! Opt-in offer when a new screenshot lands in the OS folder. Only path and
//! modified time are observed until the user taps Explain; bytes load then.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How long the Explain chip stays up.
pub const CHIP_TTL_MS: u64 = 5 * 60 * 1000;
/// Ignore files older than this when first seen (stale folder contents).
pub const NEW_FILE_MAX_AGE_MS: u64 = 2 * 60 * 1000;

pub const CHIP_LABEL: &str = "Explain this?";
pub const DISMISS_LABEL: &str = "Not now";
pub const EXPLAIN_PROMPT: &str =
    "Explain what you see in this screenshot. Name the app or screen, the important text, and what the user might want to do next.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotExplainSettings {
    #[serde(default)]
    pub enabled: bool,
}

impl Default for ScreenshotExplainSettings {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageMeta {
    pub path: PathBuf,
    pub modified_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingScreenshotOffer {
    pub path: String,
    pub offered_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotOfferLedger {
    pub last_seen_modified_ms: u64,
    #[serde(default)]
    pub last_tick_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingScreenshotOffer>,
}

impl ScreenshotOfferLedger {
    pub fn last_tick_ms(&self) -> u64 {
        self.last_tick_ms
    }

    pub fn set_last_tick_ms(&mut self, ms: u64) {
        self.last_tick_ms = ms;
    }

    pub fn offer_live(&self, now_ms: u64) -> Option<&PendingScreenshotOffer> {
        let p = self.pending.as_ref()?;
        if now_ms.saturating_sub(p.offered_at_ms) > CHIP_TTL_MS {
            return None;
        }
        Some(p)
    }
}

pub fn watch_dirs(home: &Path, os: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut push = |p: PathBuf, out: &mut Vec<PathBuf>| {
        if p.is_absolute() {
            out.push(p);
        }
    };
    match os {
        "windows" => {
            push(home.join("Pictures").join("Screenshots"), &mut out);
            push(
                home.join("OneDrive").join("Pictures").join("Screenshots"),
                &mut out,
            );
        }
        "macos" => {
            push(home.join("Pictures").join("Screenshots"), &mut out);
            push(home.join("Desktop"), &mut out);
        }
        _ => {
            push(home.join("Pictures").join("Screenshots"), &mut out);
            if let Ok(xdg) = std::env::var("XDG_PICTURES_DIR") {
                push(PathBuf::from(xdg).join("Screenshots"), &mut out);
            }
        }
    }
    out
}

pub fn is_image_name(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
    )
}

fn modified_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Newest image under `dir` by modified time. Uses directory listing and
/// metadata only — never opens file contents.
pub fn newest_image_meta(dir: &Path) -> Option<ImageMeta> {
    let read = std::fs::read_dir(dir).ok()?;
    let mut best: Option<ImageMeta> = None;
    for entry in read.flatten() {
        let path = entry.path();
        if !is_image_name(&path) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified_ms = modified_ms(&meta);
        if best.as_ref().is_none_or(|b| modified_ms > b.modified_ms) {
            best = Some(ImageMeta { path, modified_ms });
        }
    }
    best
}

pub fn newest_in_watch_dirs(dirs: &[PathBuf]) -> Option<ImageMeta> {
    let mut best: Option<ImageMeta> = None;
    for dir in dirs {
        if let Some(hit) = newest_image_meta(dir) {
            if best.as_ref().is_none_or(|b| hit.modified_ms > b.modified_ms) {
                best = Some(hit);
            }
        }
    }
    best
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickOutcome {
    Unchanged,
    ClearedExpired,
    NewOffer(ImageMeta),
}

/// Poll screenshot folders. Does not read image bytes.
pub fn tick(
    ledger: &mut ScreenshotOfferLedger,
    settings: &ScreenshotExplainSettings,
    dirs: &[PathBuf],
    now_ms: u64,
) -> TickOutcome {
    if !settings.enabled {
        if ledger.pending.take().is_some() {
            return TickOutcome::ClearedExpired;
        }
        return TickOutcome::Unchanged;
    }
    if ledger
        .pending
        .as_ref()
        .is_some_and(|_| ledger.offer_live(now_ms).is_none())
    {
        ledger.pending = None;
        return TickOutcome::ClearedExpired;
    }
    let Some(hit) = newest_in_watch_dirs(dirs) else {
        return TickOutcome::Unchanged;
    };
    if hit.modified_ms <= ledger.last_seen_modified_ms {
        return TickOutcome::Unchanged;
    }
    ledger.last_seen_modified_ms = hit.modified_ms;
    if now_ms.saturating_sub(hit.modified_ms) > NEW_FILE_MAX_AGE_MS {
        return TickOutcome::Unchanged;
    }
    if ledger
        .pending
        .as_ref()
        .is_some_and(|p| p.path == hit.path.to_string_lossy())
    {
        return TickOutcome::Unchanged;
    }
    ledger.pending = Some(PendingScreenshotOffer {
        path: hit.path.to_string_lossy().into_owned(),
        offered_at_ms: now_ms,
    });
    TickOutcome::NewOffer(hit)
}

pub fn dismiss(ledger: &mut ScreenshotOfferLedger) {
    ledger.pending = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;
    use std::time::Duration;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "grokhub-screenshot-offer-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn toggle_off_clears_pending_and_never_offers() {
        let dir = tmp("off");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shot.png");
        fs::write(&path, b"\x89PNG").unwrap();
        let mut ledger = ScreenshotOfferLedger {
            pending: Some(PendingScreenshotOffer {
                path: path.display().to_string(),
                offered_at_ms: 1,
            }),
            ..Default::default()
        };
        let mut settings = ScreenshotExplainSettings { enabled: false };
        assert_eq!(
            tick(&mut ledger, &settings, &[dir.clone()], 2),
            TickOutcome::ClearedExpired
        );
        assert!(ledger.pending.is_none());
        settings.enabled = true;
        assert_eq!(
            tick(&mut ledger, &settings, &[dir], 3),
            TickOutcome::NewOffer(ImageMeta {
                path,
                modified_ms: ledger.last_seen_modified_ms,
            })
        );
    }

    #[test]
    fn chip_expires_after_ttl() {
        let p = PendingScreenshotOffer {
            path: "/tmp/x.png".into(),
            offered_at_ms: 1000,
        };
        let ledger = ScreenshotOfferLedger {
            pending: Some(p),
            ..Default::default()
        };
        assert!(ledger.offer_live(1000 + CHIP_TTL_MS).is_some());
        assert!(ledger.offer_live(1000 + CHIP_TTL_MS + 1).is_none());
    }

    #[test]
    fn detect_new_file_without_reading_bytes() {
        let dir = tmp("detect");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("capture.png");
        fs::write(&path, b"not-a-real-png-but-bytes").unwrap();
        thread::sleep(Duration::from_millis(5));
        let meta = newest_image_meta(&dir).expect("newest");
        assert_eq!(meta.path, path);
        let mut ledger = ScreenshotOfferLedger::default();
        let settings = ScreenshotExplainSettings { enabled: true };
        let now = meta.modified_ms + 1;
        assert!(matches!(
            tick(&mut ledger, &settings, &[dir], now),
            TickOutcome::NewOffer(_)
        ));
        assert!(ledger.pending.is_some());
    }

    #[test]
    fn stale_folder_files_do_not_pop_chip() {
        let dir = tmp("stale");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.png");
        fs::write(&path, b"x").unwrap();
        let meta = fs::metadata(&path).unwrap();
        let old_ms = modified_ms(&meta);
        let mut ledger = ScreenshotOfferLedger::default();
        let settings = ScreenshotExplainSettings { enabled: true };
        let now = old_ms + NEW_FILE_MAX_AGE_MS + 10_000;
        assert_eq!(
            tick(&mut ledger, &settings, &[dir], now),
            TickOutcome::Unchanged
        );
        assert!(ledger.pending.is_none());
        assert_eq!(ledger.last_seen_modified_ms, old_ms);
    }

    #[test]
    fn watch_dirs_linux_includes_pictures_screenshots() {
        let home = PathBuf::from("/home/user");
        let dirs = watch_dirs(&home, "linux");
        assert!(dirs.contains(&home.join("Pictures").join("Screenshots")));
    }
}

//! A mod the user downloaded in their browser (a free Nexus account, or a
//! CurseForge file without a direct link): find it in Downloads by its file
//! name and size, downloaded after they clicked.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Whether `found` is `want` or the browser's renamed copy (`name (1).zip`).
fn same_name(found: &str, want: &str) -> bool {
    if found.eq_ignore_ascii_case(want) {
        return true;
    }
    let (stem, ext) = match want.rsplit_once('.') {
        Some((s, e)) => (s, format!(".{e}")),
        None => (want, String::new()),
    };
    let found_low = found.to_ascii_lowercase();
    let Some(rest) = found_low
        .strip_prefix(&stem.to_ascii_lowercase())
        .and_then(|r| r.strip_suffix(&ext.to_ascii_lowercase()))
    else {
        return false;
    };
    rest.strip_prefix(" (")
        .and_then(|r| r.strip_suffix(')'))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The newest file in `downloads` named like `file_name`, changed after
/// `since`, and `size` bytes when the size is known (not 0).
pub fn find(downloads: &Path, file_name: &str, size: u64, since: SystemTime) -> Option<PathBuf> {
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for e in std::fs::read_dir(downloads).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !same_name(&name, file_name) {
            continue;
        }
        let Ok(meta) = e.metadata() else {
            continue;
        };
        let Ok(when) = meta.modified() else {
            continue;
        };
        if !meta.is_file() || when < since || (size > 0 && meta.len() != size) {
            continue;
        }
        if best.as_ref().is_none_or(|(t, _)| when > *t) {
            best = Some((when, e.path()));
        }
    }
    best.map(|(_, p)| p)
}

/// The user's Downloads folder, when this PC has one.
pub fn downloads_folder() -> Option<PathBuf> {
    dirs::download_dir()
        .or_else(|| crate::user_home().map(|h| h.join("Downloads")))
        .filter(|d| d.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::testing::temp_config;
    use std::time::Duration;

    #[test]
    fn a_download_is_matched_by_name_size_and_time() {
        let dir = temp_config("pickup");
        let since = SystemTime::now() - Duration::from_secs(5);
        std::fs::write(dir.join("SkyUI_5_2_SE-12604-5-2SE.7z"), b"12345").unwrap();
        std::fs::write(dir.join("Better Sky-1-0.zip"), b"123").unwrap();
        std::fs::write(dir.join("Better Sky-1-0 (1).zip"), b"12345").unwrap();
        std::fs::write(dir.join("Better Sky-1-0.zip.crdownload"), b"12345").unwrap();
        assert_eq!(
            find(&dir, "Better Sky-1-0.zip", 5, since),
            Some(dir.join("Better Sky-1-0 (1).zip"))
        );
        assert!(find(&dir, "Better Sky-1-0.zip", 0, since).is_some());
        assert_eq!(find(&dir, "Better Sky-1-0.zip", 9, since), None);
        let later = SystemTime::now() + Duration::from_secs(60);
        assert_eq!(
            find(&dir, "Better Sky-1-0.zip", 5, later),
            None,
            "older than the click"
        );
        assert!(same_name("a (12).zip", "a.zip"));
        assert!(!same_name("a (x).zip", "a.zip"));
        assert!(!same_name("ab.zip", "a.zip"));
    }
}

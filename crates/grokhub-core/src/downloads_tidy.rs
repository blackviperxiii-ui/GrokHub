//! Downloads tidy suggestions. Opt-in. Once a week the cabin looks at the
//! files directly in Downloads (name, size, age) and suggests what could go:
//! installers for apps that are already installed, big disk images and
//! archives nobody has touched in a month, archives already extracted next
//! to themselves, and exact duplicates. Duplicates are found by size, then a
//! SHA-256 of the bytes; nothing else reads a file's contents. Nothing is
//! removed here: the user picks from a review list, approves the exact list,
//! and the files go to the Trash or Recycle Bin.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::update_feed::{digest_card, UpdateCard};

pub const FILE_NAME: &str = "downloads-tidy.json";
pub const SOURCE_ID: &str = "source:downloads-tidy";
pub const REVIEW_CHIP: &str = "tidy:review";
pub const CHECK_EVERY_MS: u64 = 7 * DAY_MS;
/// An installer this old, for an app that is installed, can go.
pub const INSTALLER_DAYS: u64 = 7;
/// A disk image or archive this big and this old can go.
pub const BIG_BYTES: u64 = 500 * 1024 * 1024;
pub const BIG_DAYS: u64 = 30;
/// Below this in total, no card.
pub const CARD_MIN_BYTES: u64 = 100 * 1024 * 1024;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

const INSTALLER_EXTS: [&str; 8] = ["exe", "msi", "msix", "dmg", "pkg", "deb", "rpm", "appimage"];
const IMAGE_EXTS: [&str; 2] = ["iso", "img"];
const ARCHIVE_EXTS: [&str; 9] = ["zip", "7z", "rar", "tar", "gz", "tgz", "xz", "zst", "bz2"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
    pub modified_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Duplicate,
    Extracted,
    Installer,
    BigArchive,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Duplicate => "Duplicate",
            Kind::Extracted => "Already extracted",
            Kind::Installer => "Old installer",
            Kind::BigArchive => "Big and old",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub file: FileInfo,
    pub kind: Kind,
    /// One line for the review list.
    pub why: String,
}

/// The files and folder names directly in `dir` (not below).
pub fn list_dir(dir: &Path) -> (Vec<FileInfo>, BTreeSet<String>) {
    let mut files = Vec::new();
    let mut dirs = BTreeSet::new();
    let Ok(read) = std::fs::read_dir(dir) else {
        return (files, dirs);
    };
    for entry in read.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if meta.is_dir() {
            dirs.insert(name.to_lowercase());
        } else if meta.is_file() {
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            files.push(FileInfo {
                path: entry.path(),
                name,
                bytes: meta.len(),
                modified_ms,
            });
        }
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    (files, dirs)
}

/// Streaming SHA-256 of a file, hex.
pub fn sha256_file(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// Lowercase letters and digits only.
fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// `name` without its archive extensions: `photos.tar.gz` → `photos`.
fn stem(name: &str) -> &str {
    let mut s = name;
    while let Some((head, ext)) = s.rsplit_once('.') {
        let ext = ext.to_ascii_lowercase();
        if head.is_empty()
            || !(ARCHIVE_EXTS.contains(&ext.as_str())
                || IMAGE_EXTS.contains(&ext.as_str())
                || INSTALLER_EXTS.contains(&ext.as_str()))
        {
            break;
        }
        s = head;
    }
    s
}

fn ext(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The installed app an installer name belongs to, if any. App names
/// shorter than four letters are too easy to match by accident.
fn installed_match<'a>(name: &str, installed: &'a [String]) -> Option<&'a str> {
    let file = squash(stem(name));
    installed
        .iter()
        .filter(|app| squash(app).len() >= 4)
        .filter(|app| file.contains(&squash(app)))
        .max_by_key(|app| squash(app).len())
        .map(String::as_str)
}

fn days(ms: u64) -> u64 {
    ms / DAY_MS
}

/// Sort the files in Downloads into suggestions. `installed` holds the
/// names of installed apps; `hash` is asked only about files that share a
/// size with another file. One suggestion per file, the first that fits:
/// duplicate, already extracted, old installer, big and old.
pub fn classify(
    files: &[FileInfo],
    dirs: &BTreeSet<String>,
    installed: &[String],
    now_ms: u64,
    hash: &mut dyn FnMut(&Path) -> Option<String>,
) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    let mut taken: BTreeSet<PathBuf> = BTreeSet::new();

    // Exact duplicates: same size, then same bytes. The oldest copy stays.
    let mut by_size: BTreeMap<u64, Vec<&FileInfo>> = BTreeMap::new();
    for f in files {
        by_size.entry(f.bytes).or_default().push(f);
    }
    for same in by_size.values().filter(|v| v.len() > 1) {
        let mut by_hash: BTreeMap<String, Vec<&FileInfo>> = BTreeMap::new();
        for f in same {
            if let Some(h) = hash(&f.path) {
                by_hash.entry(h).or_default().push(f);
            }
        }
        for mut copies in by_hash.into_values().filter(|v| v.len() > 1) {
            copies.sort_by_key(|f| (f.modified_ms, f.name.len(), f.name.clone()));
            let keep = copies[0].name.clone();
            for f in &copies[1..] {
                taken.insert(f.path.clone());
                out.push(Finding {
                    file: (*f).clone(),
                    kind: Kind::Duplicate,
                    why: format!("Same file as {keep}"),
                });
            }
        }
    }

    for f in files {
        if taken.contains(&f.path) {
            continue;
        }
        let e = ext(&f.name);
        let age = days(now_ms.saturating_sub(f.modified_ms));
        let archive = ARCHIVE_EXTS.contains(&e.as_str());
        let image = IMAGE_EXTS.contains(&e.as_str());
        let finding = if archive && dirs.contains(&stem(&f.name).to_lowercase()) {
            Some((
                Kind::Extracted,
                format!("Its files are already in the {} folder", stem(&f.name)),
            ))
        } else if INSTALLER_EXTS.contains(&e.as_str()) && age >= INSTALLER_DAYS {
            installed_match(&f.name, installed)
                .map(|app| (Kind::Installer, format!("{app} is already installed")))
        } else if (archive || image) && f.bytes >= BIG_BYTES && age >= BIG_DAYS {
            Some((Kind::BigArchive, format!("Not touched in {age} days")))
        } else {
            None
        };
        if let Some((kind, why)) = finding {
            out.push(Finding {
                file: f.clone(),
                kind,
                why,
            });
        }
    }
    out.sort_by(|a, b| {
        (a.kind, std::cmp::Reverse(a.file.bytes), &a.file.name).cmp(&(
            b.kind,
            std::cmp::Reverse(b.file.bytes),
            &b.file.name,
        ))
    });
    out
}

/// `12 GB`, `340 MB`, `5 KB`.
pub fn human_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= 10 * GB {
        format!("{} GB", bytes / GB)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else {
        format!("{} KB", bytes.div_ceil(KB))
    }
}

pub fn total_bytes(findings: &[Finding]) -> u64 {
    findings.iter().map(|f| f.file.bytes).sum()
}

/// The one suggestion card, when there is enough to bother about.
pub fn card(findings: &[Finding], now_ms: u64) -> Option<UpdateCard> {
    let total = total_bytes(findings);
    if findings.is_empty() || total < CARD_MIN_BYTES {
        return None;
    }
    let has = |k: Kind| findings.iter().any(|f| f.kind == k);
    let mut what: Vec<&str> = Vec::new();
    if has(Kind::Installer) {
        what.push("old installers");
    }
    let image = |f: &&Finding| IMAGE_EXTS.contains(&ext(&f.file.name).as_str());
    let big_or_extracted = |f: &&Finding| matches!(f.kind, Kind::BigArchive | Kind::Extracted);
    if findings.iter().filter(big_or_extracted).any(|f| image(&f)) {
        what.push("ISOs");
    }
    if findings.iter().filter(big_or_extracted).any(|f| !image(&f)) {
        what.push("archives");
    }
    if has(Kind::Duplicate) {
        what.push("duplicates");
    }
    let title = format!("{} of {} in Downloads", human_bytes(total), join_and(&what));
    let mut counts: Vec<String> = Vec::new();
    for kind in [
        Kind::Installer,
        Kind::BigArchive,
        Kind::Extracted,
        Kind::Duplicate,
    ] {
        let n = findings.iter().filter(|f| f.kind == kind).count();
        if n > 0 {
            counts.push(format!("{} {}", n, kind.label().to_lowercase()));
        }
    }
    let body = format!(
        "{}. Review the list and pick what goes; it moves to the Trash only after you approve it.",
        counts.join(", ")
    );
    Some(digest_card(SOURCE_ID, &title, &body, now_ms))
}

fn join_and(parts: &[&str]) -> String {
    match parts {
        [] => "files".into(),
        [one] => (*one).to_string(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

/// App names from desktop entries (`Name=` in `*.desktop`, Linux) and Start
/// Menu shortcuts (`*.lnk` file names, Windows), one level of subfolders deep.
pub fn installed_names(roots: &[PathBuf]) -> Vec<String> {
    let mut names = BTreeSet::new();
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 2 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            match ext(&name).as_str() {
                "lnk" => {
                    if let Some((head, _)) = name.rsplit_once('.') {
                        names.insert(head.trim().to_string());
                    }
                }
                "desktop" => {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    if let Some(n) = text.lines().find_map(|l| l.strip_prefix("Name=")) {
                        names.insert(n.trim().to_string());
                    }
                }
                _ => {}
            }
        }
    }
    names.into_iter().filter(|n| !n.is_empty()).collect()
}

/// What the tidy remembers: when it last looked.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tidy {
    #[serde(default)]
    pub last_check_ms: u64,
}

impl Tidy {
    pub fn due(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.last_check_ms) >= CHECK_EVERY_MS
    }
}

pub fn load(config_dir: &Path) -> Tidy {
    std::fs::read_to_string(config_dir.join(FILE_NAME))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, tidy: &Tidy) -> Result<(), String> {
    let text = serde_json::to_string_pretty(tidy).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&config_dir.join(FILE_NAME), &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-27 00:00 UTC.
    const NOW: u64 = 1_793_059_200_000;
    const MB: u64 = 1024 * 1024;

    fn file(name: &str, bytes: u64, days_old: u64) -> FileInfo {
        FileInfo {
            path: PathBuf::from("/home/user/Downloads").join(name),
            name: name.into(),
            bytes,
            modified_ms: NOW - days_old * DAY_MS,
        }
    }

    fn fixture() -> (Vec<FileInfo>, BTreeSet<String>) {
        let files = vec![
            file("DiscordSetup.exe", 90 * MB, 20),
            file("SteamSetup.exe", 2 * MB, 2),
            file("UnknownToolSetup.exe", 40 * MB, 60),
            file("ubuntu-24.04-desktop-amd64.iso", 5800 * MB, 45),
            file("fresh.iso", 900 * MB, 3),
            file("photos.zip", 300 * MB, 1),
            file("notes.zip", 4 * MB, 90),
            file("report.pdf", 3 * MB, 10),
            file("report (1).pdf", 3 * MB, 9),
            file("other.pdf", 3 * MB, 8),
        ];
        let dirs: BTreeSet<String> = ["photos".to_string()].into();
        (files, dirs)
    }

    /// Fake hashes: the two report copies match; other.pdf has the same
    /// size but different bytes.
    fn hash(path: &Path) -> Option<String> {
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some(if name.starts_with("report") {
            "aaaa".into()
        } else {
            name
        })
    }

    #[test]
    fn a_fixture_folder_sorts_into_the_right_kinds() {
        let (files, dirs) = fixture();
        let installed = vec!["Discord".to_string(), "Steam".to_string(), "Go".to_string()];
        let mut asked: Vec<String> = Vec::new();
        let found = classify(&files, &dirs, &installed, NOW, &mut |p| {
            asked.push(p.file_name().unwrap().to_string_lossy().into_owned());
            hash(p)
        });
        let got: Vec<(&str, Kind, &str)> = found
            .iter()
            .map(|f| (f.file.name.as_str(), f.kind, f.why.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("report (1).pdf", Kind::Duplicate, "Same file as report.pdf"),
                (
                    "photos.zip",
                    Kind::Extracted,
                    "Its files are already in the photos folder"
                ),
                (
                    "DiscordSetup.exe",
                    Kind::Installer,
                    "Discord is already installed"
                ),
                (
                    "ubuntu-24.04-desktop-amd64.iso",
                    Kind::BigArchive,
                    "Not touched in 45 days"
                ),
            ]
        );
        // Only files sharing a size are hashed.
        asked.sort();
        assert_eq!(asked, vec!["other.pdf", "report (1).pdf", "report.pdf"]);
    }

    #[test]
    fn duplicates_need_the_same_bytes_not_just_the_same_size() {
        let files = vec![file("a.bin", 10 * MB, 5), file("b.bin", 10 * MB, 4)];
        let found = classify(&files, &BTreeSet::new(), &[], NOW, &mut |p| {
            Some(p.to_string_lossy().into_owned())
        });
        assert!(found.is_empty());
        let found = classify(&files, &BTreeSet::new(), &[], NOW, &mut |_| {
            Some("same".into())
        });
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file.name, "b.bin", "the older copy stays");
    }

    #[test]
    fn real_files_hash_by_content() {
        let dir = std::env::temp_dir().join(format!("grokhub-tidy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("photos")).unwrap();
        std::fs::write(dir.join("a.txt"), b"same bytes").unwrap();
        std::fs::write(dir.join("b.txt"), b"same bytes").unwrap();
        std::fs::write(dir.join("c.txt"), b"diff bytes").unwrap();
        let (files, dirs) = list_dir(&dir);
        assert_eq!(
            files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["a.txt", "b.txt", "c.txt"]
        );
        assert!(dirs.contains("photos"));
        assert_eq!(
            sha256_file(&dir.join("a.txt")),
            sha256_file(&dir.join("b.txt"))
        );
        assert_ne!(
            sha256_file(&dir.join("a.txt")),
            sha256_file(&dir.join("c.txt"))
        );
        let found = classify(&files, &dirs, &[], NOW, &mut |p| sha256_file(p));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::Duplicate);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_card_totals_the_kinds_and_skips_small_change() {
        let (files, dirs) = fixture();
        let installed = vec!["Discord".to_string()];
        let found = classify(&files, &dirs, &installed, NOW, &mut |p| hash(p));
        let posted = card(&found, NOW).expect("card");
        assert_eq!(
            posted.title,
            "6.0 GB of old installers, ISOs, archives and duplicates in Downloads"
        );
        assert_eq!(
            posted.body.as_deref(),
            Some(
                "1 old installer, 1 big and old, 1 already extracted, 1 duplicate. Review the list and pick what goes; it moves to the Trash only after you approve it."
            )
        );
        assert_eq!(posted.source_id, SOURCE_ID);
        let small = vec![Finding {
            file: file("x.zip", 5 * MB, 1),
            kind: Kind::Extracted,
            why: String::new(),
        }];
        assert!(card(&small, NOW).is_none());
    }

    #[test]
    fn installed_names_come_from_shortcuts_and_desktop_entries() {
        let dir = std::env::temp_dir().join(format!("grokhub-apps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Games")).unwrap();
        std::fs::write(dir.join("Discord.lnk"), b"").unwrap();
        std::fs::write(dir.join("Games").join("Steam.lnk"), b"").unwrap();
        std::fs::write(
            dir.join("org.gimp.GIMP.desktop"),
            "[Desktop Entry]\nName=GIMP\nExec=gimp\n",
        )
        .unwrap();
        std::fs::write(dir.join("readme.txt"), b"Name=Nope").unwrap();
        assert_eq!(
            installed_names(std::slice::from_ref(&dir)),
            vec!["Discord", "GIMP", "Steam"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sizes_read_like_a_person_would_say_them() {
        assert_eq!(human_bytes(12 * 1024 * MB), "12 GB");
        assert_eq!(human_bytes(1536 * MB), "1.5 GB");
        assert_eq!(human_bytes(340 * MB), "340 MB");
        assert_eq!(human_bytes(5000), "5 KB");
    }

    #[test]
    fn the_check_runs_weekly() {
        let tidy = Tidy {
            last_check_ms: NOW - 6 * DAY_MS,
        };
        assert!(!tidy.due(NOW));
        assert!(tidy.due(NOW + DAY_MS));
        assert!(Tidy::default().due(NOW));
    }
}

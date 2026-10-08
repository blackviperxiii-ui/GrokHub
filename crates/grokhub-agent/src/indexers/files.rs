//! `files:<dir>`: walk one granted folder. Metadata for every file, plus the
//! title and headings of small text files. Symlinks are never followed and
//! every path is checked against the hard excludes before anything is read.

use std::collections::VecDeque;
use std::path::Path;

use grokhub_core::amr::Sensitivity;

use super::fs::{EntryKind, IndexFs};
use super::{date_of, index_excluded, Fact};

/// Most entries one tick looks at in one folder.
pub const FILES_PER_TICK: usize = 20_000;
/// Text files bigger than this give metadata only.
pub const TEXT_CAP: u64 = 256 * 1024;
const TEXT_EXTS: &[&str] = &["md", "markdown", "txt", "org", "rst", "adoc", "tex"];
const HEADINGS_MAX: usize = 5;
const LINE_CHARS: usize = 80;

/// What one walk saw.
#[derive(Debug, Default)]
pub struct FolderScan {
    pub facts: Vec<Fact>,
    /// Entries looked at (files and folders), at most [`FILES_PER_TICK`].
    pub seen: usize,
    /// Entries skipped as hard excludes. Their names go nowhere.
    pub excluded: usize,
}

fn clip(text: &str) -> String {
    let t = text.trim();
    if t.chars().count() <= LINE_CHARS {
        return t.to_string();
    }
    let mut out: String = t.chars().take(LINE_CHARS - 1).collect();
    out.push('…');
    out
}

fn size_label(len: u64) -> String {
    if len < 1024 {
        format!("{len} B")
    } else if len < 1024 * 1024 {
        format!("{} KB", len.div_ceil(1024))
    } else {
        format!("{:.1} MB", len as f64 / (1024.0 * 1024.0))
    }
}

/// Title and headings of a text file. Markdown `#` lines are headings; the
/// title is the first heading, else the first non-empty line.
fn outline(text: &str, markdown: bool) -> (Option<String>, Vec<String>) {
    let mut headings = Vec::new();
    let mut first_line = None;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        first_line.get_or_insert_with(|| clip(t));
        if markdown && t.starts_with('#') {
            let h = t.trim_start_matches('#').trim();
            if !h.is_empty() && headings.len() < HEADINGS_MAX + 1 {
                headings.push(clip(h));
            }
        }
    }
    let title = if headings.is_empty() { first_line } else { Some(headings.remove(0)) };
    headings.truncate(HEADINGS_MAX);
    (title, headings)
}

/// Walk `root` breadth first, sorted by name, up to [`FILES_PER_TICK`] entries.
pub fn index_folder(fs: &dyn IndexFs, root: &Path) -> FolderScan {
    let mut scan = FolderScan::default();
    if index_excluded(&root.display().to_string()) {
        return scan;
    }
    let mut queue = VecDeque::from([root.to_path_buf()]);
    'walk: while let Some(dir) = queue.pop_front() {
        let Ok(entries) = fs.list(&dir) else {
            continue;
        };
        for e in entries {
            if scan.seen >= FILES_PER_TICK {
                break 'walk;
            }
            scan.seen += 1;
            if index_excluded(&e.path.display().to_string()) {
                scan.excluded += 1;
                continue;
            }
            match e.kind {
                EntryKind::Dir => queue.push_back(e.path),
                EntryKind::File => {
                    let rel = e.path.strip_prefix(root).unwrap_or(&e.path);
                    let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                    let rel = rel.join("/");
                    let mut line = format!("File {rel} · {} · modified {}", size_label(e.len), date_of(e.modified_ms));
                    let ext = Path::new(&e.name)
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(str::to_ascii_lowercase)
                        .unwrap_or_default();
                    if TEXT_EXTS.contains(&ext.as_str()) && e.len <= TEXT_CAP {
                        if let Ok(bytes) = fs.read_head(&e.path, TEXT_CAP) {
                            let text = String::from_utf8_lossy(&bytes);
                            let (title, headings) = outline(&text, matches!(ext.as_str(), "md" | "markdown"));
                            if let Some(title) = title {
                                line.push_str(&format!(" · title: {title}"));
                            }
                            if !headings.is_empty() {
                                line.push_str(&format!(" · headings: {}", headings.join("; ")));
                            }
                        }
                    }
                    scan.facts.push(Fact { item: rel, line, sensitivity: Sensitivity::Personal });
                }
                // Symlinks are never followed; sockets and devices are skipped.
                EntryKind::Symlink | EntryKind::Other => {}
            }
        }
    }
    scan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_reads_markdown_headings_and_plain_first_lines() {
        let md = "\n# Q3 plan\n\nIntro\n## Budget\n### Hiring\n";
        assert_eq!(outline(md, true), (Some("Q3 plan".into()), vec!["Budget".into(), "Hiring".into()]));
        assert_eq!(outline("  \nGroceries\nmilk\n", false), (Some("Groceries".into()), vec![]));
        assert_eq!(outline("# not a heading in txt\n", false), (Some("# not a heading in txt".into()), vec![]));
        assert_eq!(outline("", true), (None, vec![]));
        assert_eq!(size_label(512), "512 B");
        assert_eq!(size_label(4097), "5 KB");
        assert_eq!(size_label(3 * 1024 * 1024), "3.0 MB");
        let long = "x".repeat(200);
        assert_eq!(clip(&long).chars().count(), LINE_CHARS);
    }
}

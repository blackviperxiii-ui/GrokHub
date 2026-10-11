//! Stores that failed to load and were moved aside. The loader notes each one here; the
//! cabin drains the list and posts one Home card per file, so a reset never goes unsaid.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One store moved aside because it would not load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    /// Where the store lives.
    pub path: PathBuf,
    /// The copy that was kept (`<name>.corrupt-<ms>`).
    pub aside: PathBuf,
    /// What GrokHub did instead, as the start of a sentence ("Restored defaults").
    pub restored: &'static str,
}

static PENDING: Mutex<Vec<Recovered>> = Mutex::new(Vec::new());

/// What a reset store falls back to.
pub const RESTORED_DEFAULTS: &str = "Restored defaults";
/// What a skipped memory note falls back to.
pub const LEFT_OUT: &str = "Left it out of memory";

/// Record that `path` was moved to `aside`.
pub fn note(path: &Path, aside: &Path, restored: &'static str) {
    let mut pending = PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    pending.push(Recovered {
        path: path.to_path_buf(),
        aside: aside.to_path_buf(),
        restored,
    });
}

/// Every store moved aside since the last drain, oldest first.
pub fn drain() -> Vec<Recovered> {
    let mut pending = PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    std::mem::take(&mut *pending)
}

/// Move a store that would not load to `<name>.corrupt-<ms>` beside it and note it.
/// Returns the new path; `None` when the rename failed (the file then stays put).
pub fn quarantine(path: &Path, restored: &'static str) -> Option<PathBuf> {
    let name = path.file_name().and_then(|s| s.to_str())?;
    let aside = path.with_file_name(format!("{name}.corrupt-{}", crate::now_ms()));
    std::fs::rename(path, &aside).ok()?;
    note(path, &aside, restored);
    Some(aside)
}

/// The name a person knows a store by ("Settings file" for `app.json`).
pub fn store_label(path: &Path) -> String {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let known = match name {
        "app.json" => "Settings file",
        "secrets.json" => "Keys file",
        "threads.json" | "chat.json" => "Chat history",
        "updates.json" => "Home feed",
        "hub-state.json" => "Hub pairing file",
        "automations.json" => "Automations list",
        "loops.json" => "Loops list",
        "learning.json" => "Learning file",
        "usage.json" => "Usage file",
        "wall.json" => "Wall file",
        "projects.json" => "Projects list",
        "suggestions.json" => "Suggestions list",
        "workboard.json" => "Workboard",
        "rewind.json" => "Rewind index",
        "card_prefs.json" => "Card settings",
        _ => "",
    };
    if !known.is_empty() {
        known.to_string()
    } else if name.ends_with(".md") || name.ends_with(".sealed") {
        format!("Memory note {name}")
    } else if name.is_empty() {
        "A GrokHub file".to_string()
    } else {
        format!("File {name}")
    }
}

/// `path` with the home folder written as `~`.
pub fn home_short(path: &Path) -> String {
    if let Some(rest) = crate::paths::user_home().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        let rest = rest.to_string_lossy().replace('\\', "/");
        return if rest.is_empty() { "~".into() } else { format!("~/{rest}") };
    }
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("grokhub-recovered-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn quarantine_moves_the_file_and_notes_it() {
        let dir = scratch("move");
        let path = dir.join("app.json");
        std::fs::write(&path, "{not json").expect("write");
        let aside = quarantine(&path, RESTORED_DEFAULTS).expect("moved");
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(&aside).expect("kept"), "{not json");
        let name = aside.file_name().and_then(|n| n.to_str()).expect("name");
        assert!(name.starts_with("app.json.corrupt-"), "{name}");
        let noted: Vec<Recovered> = drain().into_iter().filter(|r| r.path == path).collect();
        assert_eq!(
            noted,
            vec![Recovered {
                path: path.clone(),
                aside,
                restored: RESTORED_DEFAULTS
            }]
        );
        assert!(drain().iter().all(|r| r.path != path), "drained once");
    }

    #[test]
    fn a_missing_file_is_not_noted() {
        let dir = scratch("missing");
        let path = dir.join("secrets.json");
        assert_eq!(quarantine(&path, RESTORED_DEFAULTS), None);
        assert!(drain().iter().all(|r| r.path != path));
    }

    #[test]
    fn the_card_names_the_file_and_the_copy() {
        let r = Recovered {
            path: "/srv/cfg/app.json".into(),
            aside: "/srv/cfg/app.json.corrupt-1012".into(),
            restored: RESTORED_DEFAULTS,
        };
        let card = crate::recovered_card(&r, 5);
        assert_eq!(card.title, "Settings file was unreadable");
        assert_eq!(
            card.body.as_deref(),
            Some("Restored defaults, kept a copy at /srv/cfg/app.json.corrupt-1012.")
        );
        assert!(crate::names_its_item(&card));
        assert_eq!(card.why, None, "not an automation");
        assert_eq!(card.id, crate::recovered_card(&r, 99).id, "one card per kept copy");
    }

    #[test]
    fn stores_have_names_people_know() {
        assert_eq!(store_label(Path::new("/c/app.json")), "Settings file");
        assert_eq!(store_label(Path::new("/c/secrets.json")), "Keys file");
        assert_eq!(store_label(Path::new("/c/updates.json")), "Home feed");
        assert_eq!(store_label(Path::new("/c/amr/nodes/prefs.md")), "Memory note prefs.md");
        assert_eq!(store_label(Path::new("/c/odd.json")), "File odd.json");
    }
}

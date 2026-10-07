//! `apps`: installed apps. Linux `.desktop` entries, Windows Start-menu `.lnk`
//! names, macOS `.app` bundles, plus how often the user opened each one from
//! GrokHub (the cabin's own launch log, never the OS's).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use grokhub_core::amr::Sensitivity;

use super::fs::{EntryKind, IndexFs};
use super::paths::{app_dirs, HostOs, PlatformDirs};
use super::{index_excluded, Fact};

/// `{config}/indexers/app-launches.jsonl`: one `{"app": "<id>"}` per launch
/// GrokHub made. Nothing writes it in this build; Spike-1b's open action will.
pub const APP_LAUNCH_LOG: &str = "app-launches.jsonl";
const DESKTOP_CAP: u64 = 64 * 1024;
const LAUNCH_LOG_CAP: u64 = 1024 * 1024;
/// Start-menu folders nest; deeper than this is not an app.
const START_MENU_DEPTH: usize = 3;
/// Most apps one tick writes.
const APPS_MAX: usize = 1000;

/// Launch counts by app id from the cabin's own log. Missing reads as empty.
pub fn read_launch_counts(config_dir: &Path) -> BTreeMap<String, u32> {
    let path = config_dir.join(super::INDEXERS_DIR).join(APP_LAUNCH_LOG);
    let mut counts = BTreeMap::new();
    let Ok(meta) = std::fs::metadata(&path) else {
        return counts;
    };
    if meta.len() > LAUNCH_LOG_CAP {
        return counts;
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    for line in text.lines() {
        let app = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|v| v.get("app").and_then(|a| a.as_str()).map(str::to_string));
        if let Some(app) = app {
            *counts.entry(app).or_insert(0) += 1;
        }
    }
    counts
}

/// `Name=` of a shown application entry, or `None` for NoDisplay / Hidden / non-apps.
fn desktop_name(text: &str) -> Option<String> {
    let mut in_entry = false;
    let mut name = None;
    let mut app = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_entry = t == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        match t.split_once('=') {
            Some(("Name", v)) if name.is_none() => name = Some(v.trim().to_string()),
            Some(("Type", v)) => app = v.trim() == "Application",
            Some(("NoDisplay" | "Hidden", v)) if v.trim().eq_ignore_ascii_case("true") => return None,
            _ => {}
        }
    }
    name.filter(|n| app && !n.is_empty())
}

fn fact(id: &str, name: &str, launches: &BTreeMap<String, u32>) -> Fact {
    let mut line = format!("Installed app: {name}");
    match launches.get(id) {
        Some(1) => line.push_str(" · opened once from GrokHub"),
        Some(n) => line.push_str(&format!(" · opened {n} times from GrokHub")),
        None => {}
    }
    Fact { item: id.to_string(), line, sensitivity: Sensitivity::Personal }
}

/// Every installed app the OS lists, deduplicated by id (user folders win).
pub fn index_apps(fs: &dyn IndexFs, dirs: &PlatformDirs, launches: &BTreeMap<String, u32>) -> Vec<Fact> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for dir in app_dirs(dirs) {
        let mut stack = vec![(dir, 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(entries) = fs.list(&dir) else {
                continue;
            };
            for e in entries {
                if out.len() >= APPS_MAX {
                    return out;
                }
                if index_excluded(&e.path.display().to_string()) {
                    continue;
                }
                let stem = Path::new(&e.name).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                let ext = Path::new(&e.name).extension().and_then(|x| x.to_str()).unwrap_or("");
                match (dirs.os, e.kind, ext) {
                    (HostOs::Linux, EntryKind::File, "desktop") => {
                        if !seen.insert(e.name.clone()) {
                            continue;
                        }
                        let Ok(bytes) = fs.read_head(&e.path, DESKTOP_CAP) else {
                            continue;
                        };
                        if let Some(name) = desktop_name(&String::from_utf8_lossy(&bytes)) {
                            out.push(fact(&e.name, &name, launches));
                        }
                    }
                    (HostOs::Windows, EntryKind::Dir, _) if depth < START_MENU_DEPTH => stack.push((e.path, depth + 1)),
                    (HostOs::Windows, EntryKind::File, "lnk") => {
                        let lower = stem.to_ascii_lowercase();
                        if lower.contains("uninstall") || !seen.insert(lower) {
                            continue;
                        }
                        out.push(fact(&stem, &stem, launches));
                    }
                    (HostOs::MacOs, EntryKind::Dir, "app") if seen.insert(stem.clone()) => {
                        out.push(fact(&stem, &stem, launches));
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entries_skip_hidden_and_non_apps() {
        let shown = "[Desktop Entry]\nType=Application\nName=Firefox\nName[de]=Feuerfuchs\nExec=firefox %u\n";
        assert_eq!(desktop_name(shown), Some("Firefox".into()));
        assert_eq!(desktop_name("[Desktop Entry]\nType=Application\nName=Helper\nNoDisplay=true\n"), None);
        assert_eq!(desktop_name("[Desktop Entry]\nType=Link\nName=Docs\n"), None);
        assert_eq!(
            desktop_name("[Desktop Action new]\nName=New Window\n[Desktop Entry]\nType=Application\nName=Files\n"),
            Some("Files".into())
        );
        let launches = BTreeMap::from([("firefox.desktop".to_string(), 3u32), ("gimp.desktop".to_string(), 1)]);
        assert_eq!(fact("firefox.desktop", "Firefox", &launches).line, "Installed app: Firefox · opened 3 times from GrokHub");
        assert_eq!(fact("gimp.desktop", "GIMP", &launches).line, "Installed app: GIMP · opened once from GrokHub");
        assert_eq!(fact("vlc.desktop", "VLC", &launches).line, "Installed app: VLC");
    }
}

//! "Know me": interest and setup tags derived from harmless, metadata-only
//! local signals, so Pulse and suggestions start from what the user actually
//! has. Off until the user turns it on, one toggle per signal.
//!
//! Scanners read names and counts only: game titles from library manifests,
//! app and program names, file extensions, the hardware probe, the locale.
//! They never open a file's contents, never name a personal file, and skip
//! every path [`denied`] matches before reading it. Only the derived tags and
//! a count per signal are kept, in `{config}/know-me.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::steam::SteamGame;

pub const FILE_NAME: &str = "know-me.json";
/// A scan runs again at most this often on its own.
pub const RESCAN_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Games named as tags, most recently played first.
pub const GAMES_MAX: usize = 5;
/// A file type must show up this often before it says anything.
pub const FILE_TYPE_MIN: usize = 3;
/// Most directory entries one file-type count looks at.
pub const FILE_ENTRIES_MAX: usize = 20_000;
/// Folders deeper than this under Downloads / Documents / Pictures / Desktop are skipped.
pub const FILE_DEPTH_MAX: usize = 3;
/// Scan log rows kept.
pub const LOG_MAX: usize = 8;

/// One kind of local signal. Each has its own toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    Games,
    Apps,
    Dev,
    Files,
    Hardware,
    Locale,
}

impl Signal {
    pub const ALL: [Signal; 6] = [
        Signal::Games,
        Signal::Apps,
        Signal::Dev,
        Signal::Files,
        Signal::Hardware,
        Signal::Locale,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Games => "Game library",
            Self::Apps => "Installed apps",
            Self::Dev => "Dev tools",
            Self::Files => "File types",
            Self::Hardware => "Hardware",
            Self::Locale => "Language and region",
        }
    }

    /// What it reads, for the Settings checklist.
    pub fn reads(self) -> &'static str {
        match self {
            Self::Games => "Titles and last played from the Steam library.",
            Self::Apps => "App names from the system's app list.",
            Self::Dev => "Which languages and tools are on PATH.",
            Self::Files => {
                "Counts by file type in Downloads, Documents, Pictures and Desktop. No file names."
            }
            Self::Hardware => "GPU and memory from the PC probe.",
            Self::Locale => "The language setting.",
        }
    }
}

/// One thing Know me learned, and where from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    /// "plays: Hades", "hobby: 3D printing", "dev: Rust", "pc: …".
    pub tag: String,
    pub signal: Signal,
    /// The Pulse interest it feeds, when it is one.
    #[serde(default)]
    pub topic: Option<String>,
}

/// What one scan read: a count per signal, never the content.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanLog {
    pub at_ms: u64,
    /// Items looked at per signal (games, apps, programs, files, …).
    pub counts: BTreeMap<Signal, usize>,
}

/// The saved state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowMe {
    #[serde(default)]
    pub on: bool,
    /// Signals the user turned off.
    #[serde(default)]
    pub off: Vec<Signal>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    /// Tags the user deleted; a rescan does not bring them back.
    #[serde(default)]
    pub removed: Vec<String>,
    #[serde(default)]
    pub last_scan_ms: u64,
    #[serde(default)]
    pub log: Vec<ScanLog>,
}

impl KnowMe {
    pub fn enabled(&self, signal: Signal) -> bool {
        self.on && !self.off.contains(&signal)
    }

    pub fn set_signal(&mut self, signal: Signal, on: bool) {
        self.off.retain(|s| *s != signal);
        if !on {
            self.off.push(signal);
            self.off.sort();
            self.tags.retain(|t| t.signal != signal);
        }
    }

    /// A weekly scan is due.
    pub fn due(&self, now_ms: u64) -> bool {
        self.on && now_ms.saturating_sub(self.last_scan_ms) >= RESCAN_MS
    }

    /// Takes a scan's tags for the signals it read, minus deleted ones.
    pub fn apply(&mut self, scan: Scan, now_ms: u64) {
        let read: Vec<Signal> = scan.counts.keys().copied().collect();
        self.tags.retain(|t| !read.contains(&t.signal));
        for tag in scan.tags {
            let key = tag.tag.to_lowercase();
            if self.removed.contains(&key) || self.tags.iter().any(|t| t.tag == tag.tag) {
                continue;
            }
            self.tags.push(tag);
        }
        self.last_scan_ms = now_ms;
        self.log.push(ScanLog {
            at_ms: now_ms,
            counts: scan.counts,
        });
        let extra = self.log.len().saturating_sub(LOG_MAX);
        self.log.drain(..extra);
    }

    /// Deletes one tag and keeps it from coming back. Returns it.
    pub fn delete(&mut self, tag: &str) -> Option<Tag> {
        let at = self.tags.iter().position(|t| t.tag == tag)?;
        let gone = self.tags.remove(at);
        self.removed.push(gone.tag.to_lowercase());
        Some(gone)
    }

    /// Forgets every tag and the scan log, and turns Know me off.
    pub fn wipe(&mut self) -> Vec<Tag> {
        self.on = false;
        self.last_scan_ms = 0;
        self.log.clear();
        std::mem::take(&mut self.tags)
    }

    /// The agent's view: each tag with its source, or why there are none.
    pub fn report(&self) -> String {
        if !self.on {
            return "Know me is off. The user can turn it on in Settings → Pulse.".into();
        }
        if self.tags.is_empty() {
            return "Know me is on but has no tags yet.".into();
        }
        let lines: Vec<String> = self
            .tags
            .iter()
            .map(|t| format!("- {} (from {})", t.tag, t.signal.label()))
            .collect();
        format!("Know me tags:\n{}", lines.join("\n"))
    }
}

pub fn load(path: &Path) -> KnowMe {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, state: &KnowMe) -> Result<(), String> {
    let text = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Folders no scanner enters: browser profiles, keys and keyrings, mail and
/// password stores. Checked on every path before it is read.
const DENIED: &[&str] = &[
    ".ssh",
    ".gnupg",
    "keyrings",
    ".password-store",
    "keepassxc",
    ".aws",
    ".kube",
    ".mozilla",
    "firefox",
    "google-chrome",
    "chromium",
    "bravesoftware",
    "vivaldi",
    "opera",
    "microsoft edge",
    "thunderbird",
    "evolution",
    "mail",
    "signal",
    "telegram desktop",
];

/// Is this path one no scanner may open?
pub fn denied(path: &Path) -> bool {
    path.components().any(|c| {
        let part = c.as_os_str().to_string_lossy().to_lowercase();
        DENIED.contains(&part.as_str()) || part.starts_with(".env")
    })
}

/// Entry names in a folder, as (name, is_dir). Empty for a denied path.
fn names(dir: &Path) -> Vec<(String, bool)> {
    if denied(dir) {
        return Vec::new();
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, bool)> = read
        .flatten()
        .map(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            (e.file_name().to_string_lossy().into_owned(), is_dir)
        })
        .collect();
    out.sort();
    out
}

/// What a scan reads from: built by the cabin, fixtures in tests.
#[derive(Clone, Debug, Default)]
pub struct ScanInput {
    pub home: PathBuf,
    /// The folders on PATH.
    pub path_dirs: Vec<PathBuf>,
    /// App list folders (`applications/` on Linux, Start Menu on Windows).
    pub app_dirs: Vec<PathBuf>,
    pub games: Vec<SteamGame>,
    /// (GPU name, RAM in MiB) from the PC probe.
    pub hardware: Option<(String, u64)>,
    /// `LANG`-style locale, e.g. "en_US.UTF-8".
    pub locale: String,
}

/// One scan's tags and counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    pub tags: Vec<Tag>,
    pub counts: BTreeMap<Signal, usize>,
}

impl Scan {
    fn add(&mut self, signal: Signal, tag: String, topic: Option<&str>) {
        if self.tags.iter().any(|t| t.tag == tag) {
            return;
        }
        self.tags.push(Tag {
            tag,
            signal,
            topic: topic.map(str::to_string),
        });
    }
}

/// Hobbies an app name points at: (name fragment, hobby).
const APP_HOBBIES: &[(&str, &str)] = &[
    ("prusaslicer", "3D printing"),
    ("orcaslicer", "3D printing"),
    ("cura", "3D printing"),
    ("bambu", "3D printing"),
    ("blender", "3D modeling"),
    ("freecad", "CAD"),
    ("fusion 360", "CAD"),
    ("krita", "digital art"),
    ("gimp", "digital art"),
    ("photoshop", "digital art"),
    ("inkscape", "digital art"),
    ("kdenlive", "video editing"),
    ("resolve", "video editing"),
    ("shotcut", "video editing"),
    ("obs", "streaming"),
    ("reaper", "music production"),
    ("ardour", "music production"),
    ("bitwig", "music production"),
    ("lmms", "music production"),
    ("fl studio", "music production"),
    ("ableton", "music production"),
    ("audacity", "audio editing"),
    ("darktable", "photography"),
    ("rawtherapee", "photography"),
    ("lightroom", "photography"),
    ("arduino", "electronics"),
    ("kicad", "electronics"),
    ("steam", "PC gaming"),
    ("heroic", "PC gaming"),
    ("lutris", "PC gaming"),
];

/// Languages and tools a program on PATH points at: (program, tag topic).
const DEV_TOOLS: &[(&str, &str)] = &[
    ("cargo", "Rust"),
    ("go", "Go"),
    ("node", "JavaScript"),
    ("deno", "JavaScript"),
    ("python3", "Python"),
    ("python", "Python"),
    ("java", "Java"),
    ("dotnet", "C#"),
    ("ruby", "Ruby"),
    ("php", "PHP"),
    ("zig", "Zig"),
    ("docker", "Docker"),
    ("podman", "containers"),
    ("kubectl", "Kubernetes"),
    ("terraform", "Terraform"),
    ("cursor", "Cursor"),
    ("code", "VS Code"),
    ("nvim", "Neovim"),
];

/// Hobbies a file type points at: (extensions, hobby).
const FILE_HOBBIES: &[(&[&str], &str)] = &[
    (&["stl", "3mf", "gcode", "bgcode"], "3D printing"),
    (&["blend", "fbx"], "3D modeling"),
    (&["step", "stp", "f3d", "dxf"], "CAD"),
    (&["psd", "kra", "xcf", "clip", "procreate"], "digital art"),
    (&["kdenlive", "drp", "prproj"], "video editing"),
    (&["flp", "als", "rpp", "mid", "midi"], "music production"),
    (&["cr2", "cr3", "nef", "arw", "dng", "raf"], "photography"),
    (&["ipynb"], "data science"),
    (&["ino", "kicad_pcb"], "electronics"),
];

/// Reads the signals `state` allows and derives tags. Names and counts only.
pub fn scan(state: &KnowMe, input: &ScanInput) -> Scan {
    let mut out = Scan::default();
    if state.enabled(Signal::Games) {
        let mut games: Vec<&SteamGame> = input.games.iter().collect();
        games.sort_by_key(|g| std::cmp::Reverse(g.last_played));
        for g in games.iter().take(GAMES_MAX) {
            out.add(Signal::Games, format!("plays: {}", g.name), Some(&g.name));
        }
        out.counts.insert(Signal::Games, input.games.len());
    }
    if state.enabled(Signal::Apps) {
        let mut seen = 0;
        for dir in &input.app_dirs {
            for (name, _) in names(dir) {
                seen += 1;
                let low = name.to_lowercase();
                for (frag, hobby) in APP_HOBBIES {
                    if low.contains(frag) {
                        out.add(Signal::Apps, format!("hobby: {hobby}"), Some(hobby));
                    }
                }
            }
        }
        out.counts.insert(Signal::Apps, seen);
    }
    if state.enabled(Signal::Dev) {
        let mut found = 0;
        for (program, topic) in DEV_TOOLS {
            if on_path(&input.path_dirs, program) {
                found += 1;
                out.add(Signal::Dev, format!("dev: {topic}"), Some(topic));
            }
        }
        out.counts.insert(Signal::Dev, found);
    }
    if state.enabled(Signal::Files) {
        let (seen, by_ext) = count_types(&input.home);
        for (exts, hobby) in FILE_HOBBIES {
            let n: usize = exts.iter().filter_map(|e| by_ext.get(*e)).sum();
            if n >= FILE_TYPE_MIN {
                out.add(Signal::Files, format!("hobby: {hobby}"), Some(hobby));
            }
        }
        out.counts.insert(Signal::Files, seen);
    }
    if state.enabled(Signal::Hardware) {
        if let Some((gpu, ram_mb)) = &input.hardware {
            let ram = (ram_mb + 512) / 1024;
            let tag = match gpu.trim() {
                "" => format!("pc: {ram} GB RAM"),
                gpu => format!("pc: {gpu}, {ram} GB RAM"),
            };
            out.add(Signal::Hardware, tag, None);
        }
        out.counts
            .insert(Signal::Hardware, usize::from(input.hardware.is_some()));
    }
    if state.enabled(Signal::Locale) {
        let lang = input.locale.split('.').next().unwrap_or("").trim();
        if !lang.is_empty() && lang != "C" && lang != "POSIX" {
            out.add(Signal::Locale, format!("language: {lang}"), None);
        }
        out.counts
            .insert(Signal::Locale, usize::from(!lang.is_empty()));
    }
    out
}

fn on_path(dirs: &[PathBuf], program: &str) -> bool {
    dirs.iter().filter(|d| !denied(d)).any(|d| {
        d.join(program).is_file()
            || d.join(format!("{program}.exe")).is_file()
            || d.join(format!("{program}.cmd")).is_file()
    })
}

/// File counts by lowercase extension under the user's everyday folders.
/// Hidden and denied folders are skipped; names are never kept.
fn count_types(home: &Path) -> (usize, BTreeMap<String, usize>) {
    let mut by_ext = BTreeMap::new();
    let mut seen = 0;
    let mut stack: Vec<(PathBuf, usize)> = ["Downloads", "Documents", "Pictures", "Desktop"]
        .iter()
        .map(|d| (home.join(d), 0))
        .collect();
    while let Some((dir, depth)) = stack.pop() {
        for (name, is_dir) in names(&dir) {
            if seen >= FILE_ENTRIES_MAX {
                return (seen, by_ext);
            }
            seen += 1;
            if name.starts_with('.') {
                continue;
            }
            if is_dir {
                if depth < FILE_DEPTH_MAX {
                    stack.push((dir.join(&name), depth + 1));
                }
                continue;
            }
            if let Some((_, ext)) = name.rsplit_once('.') {
                *by_ext.entry(ext.to_ascii_lowercase()).or_insert(0) += 1;
            }
        }
    }
    (seen, by_ext)
}

/// A tag as other readers see it: the label and the topic it feeds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowMeTag {
    pub label: String,
    #[serde(default)]
    pub topic: String,
}

/// Know me's tags from `{config_dir}/know-me.json`; empty when Know me is
/// off, never ran, or the file is unreadable.
pub fn load_tags(config_dir: &Path) -> Vec<KnowMeTag> {
    let state = load(&config_dir.join(FILE_NAME));
    if !state.on {
        return Vec::new();
    }
    state
        .tags
        .into_iter()
        .map(|t| KnowMeTag {
            label: t.tag,
            topic: t.topic.unwrap_or_default(),
        })
        .collect()
}

/// Lowercase topics, or the label when a tag has no topic.
pub fn tag_topics(tags: &[KnowMeTag]) -> Vec<String> {
    tags.iter()
        .map(|t| {
            let topic = t.topic.trim();
            if topic.is_empty() {
                t.label.trim()
            } else {
                topic
            }
        })
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

/// True when any tag's topic mentions one of the words.
pub fn tags_mention(tags: &[KnowMeTag], words: &[&str]) -> bool {
    let topics = tag_topics(tags);
    words.iter().any(|w| {
        let w = w.to_ascii_lowercase();
        !w.is_empty()
            && topics
                .iter()
                .any(|t| t.contains(&w) || w.contains(t.as_str()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grokhub-know-me-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "x").unwrap();
    }

    fn all_on() -> KnowMe {
        KnowMe {
            on: true,
            ..Default::default()
        }
    }

    fn game(id: u32, name: &str, last: u64) -> SteamGame {
        SteamGame {
            app_id: id,
            name: name.into(),
            last_played: last,
            playtime_min: 0,
        }
    }

    fn tags(scan: &Scan) -> Vec<&str> {
        scan.tags.iter().map(|t| t.tag.as_str()).collect()
    }

    #[test]
    fn off_by_default_and_off_signals_never_run() {
        let home = temp("off");
        touch(&home.join("Downloads/a.stl"));
        touch(&home.join("Downloads/b.stl"));
        touch(&home.join("Downloads/c.3mf"));
        let input = ScanInput {
            home: home.clone(),
            games: vec![game(1, "Hades", 5)],
            locale: "en_US.UTF-8".into(),
            ..Default::default()
        };
        let scan_off = scan(&KnowMe::default(), &input);
        assert_eq!(scan_off, Scan::default());
        let mut state = all_on();
        state.set_signal(Signal::Files, false);
        state.set_signal(Signal::Games, false);
        let got = scan(&state, &input);
        assert_eq!(tags(&got), vec!["language: en_US"]);
        assert_eq!(
            got.counts.keys().copied().collect::<Vec<_>>(),
            vec![Signal::Apps, Signal::Dev, Signal::Hardware, Signal::Locale]
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn scanners_read_names_and_counts_on_linux_and_windows_layouts() {
        let home = temp("scan");
        for f in ["a.stl", "b.STL", "nested/c.3mf", "photo.jpg"] {
            touch(&home.join("Downloads").join(f));
        }
        touch(&home.join("Documents/song.flp"));
        let apps = home.join("applications");
        touch(&apps.join("org.blender.Blender.desktop"));
        touch(&apps.join("com.prusa3d.PrusaSlicer.desktop"));
        let start_menu = home.join("Start Menu/Programs");
        touch(&start_menu.join("OBS Studio.lnk"));
        let bin = home.join("bin");
        touch(&bin.join("cargo"));
        touch(&bin.join("node.exe"));
        let input = ScanInput {
            home: home.clone(),
            path_dirs: vec![bin],
            app_dirs: vec![apps, start_menu],
            games: vec![
                game(1, "Old Save", 10),
                game(2, "Current Main", 30),
                game(3, "Middle", 20),
            ],
            hardware: Some(("NVIDIA GeForce RTX 4070".into(), 32_000)),
            locale: "de_DE.UTF-8".into(),
        };
        let got = scan(&all_on(), &input);
        assert_eq!(
            tags(&got),
            vec![
                "plays: Current Main",
                "plays: Middle",
                "plays: Old Save",
                "hobby: 3D printing",
                "hobby: 3D modeling",
                "hobby: streaming",
                "dev: Rust",
                "dev: JavaScript",
                "pc: NVIDIA GeForce RTX 4070, 31 GB RAM",
                "language: de_DE",
            ]
        );
        assert_eq!(got.tags[0].topic.as_deref(), Some("Current Main"));
        assert_eq!(got.tags[8].topic, None);
        assert_eq!(got.counts[&Signal::Games], 3);
        assert_eq!(got.counts[&Signal::Apps], 3);
        assert_eq!(got.counts[&Signal::Dev], 2);
        // Downloads: a, b, nested/, nested/c, photo; Documents: song.
        assert_eq!(got.counts[&Signal::Files], 6);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn denied_folders_are_never_read() {
        let home = temp("deny");
        for f in [".ssh/a.stl", "keyrings/b.stl", "google-chrome/c.stl"] {
            touch(&home.join("Downloads").join(f));
        }
        let bin = home.join(".ssh");
        touch(&bin.join("cargo"));
        let input = ScanInput {
            home: home.clone(),
            path_dirs: vec![bin],
            ..Default::default()
        };
        let got = scan(&all_on(), &input);
        assert!(tags(&got).is_empty(), "{:?}", tags(&got));
        assert!(denied(Path::new("/home/user/.mozilla/firefox/x")));
        assert!(denied(Path::new("/home/user/project/.env.local")));
        assert!(denied(Path::new(
            "C:/Users/user/AppData/Roaming/Thunderbird/x"
        )));
        assert!(!denied(Path::new("/home/user/Downloads/prints")));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn deleted_tags_stay_gone_and_wipe_turns_it_off() {
        let mut state = all_on();
        let input = ScanInput {
            games: vec![game(1, "Hades", 5), game(2, "Dota 2", 9)],
            ..Default::default()
        };
        state.apply(scan(&state, &input), 100);
        assert_eq!(state.tags.len(), 2);
        assert_eq!(
            state.delete("plays: Hades").and_then(|t| t.topic),
            Some("Hades".to_string())
        );
        state.apply(scan(&state, &input), 200);
        let names: Vec<&str> = state.tags.iter().map(|t| t.tag.as_str()).collect();
        assert_eq!(names, vec!["plays: Dota 2"]);
        assert_eq!(state.log.len(), 2);
        assert_eq!(state.log[1].counts[&Signal::Games], 2);
        assert_eq!(
            state.report(),
            "Know me tags:\n- plays: Dota 2 (from Game library)"
        );
        assert!(!state.due(200 + RESCAN_MS - 1));
        assert!(state.due(200 + RESCAN_MS));
        let gone = state.wipe();
        assert_eq!(gone.len(), 1);
        assert_eq!((state.on, state.tags.len(), state.log.len()), (false, 0, 0));
        assert_eq!(
            state.report(),
            "Know me is off. The user can turn it on in Settings → Pulse."
        );
        assert!(!state.due(u64::MAX));
    }

    #[test]
    fn state_round_trips() {
        let dir = temp("save");
        let path = dir.join(FILE_NAME);
        assert_eq!(load(&path), KnowMe::default());
        let mut state = all_on();
        state.set_signal(Signal::Files, false);
        state.tags.push(Tag {
            tag: "dev: Rust".into(),
            signal: Signal::Dev,
            topic: Some("Rust".into()),
        });
        state.apply(
            Scan {
                tags: Vec::new(),
                counts: BTreeMap::from([(Signal::Dev, 4)]),
            },
            7,
        );
        save(&path, &state).unwrap();
        assert_eq!(load(&path), state);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn readers_get_labels_and_topics_only_while_on() {
        let dir = temp("readers");
        assert!(load_tags(&dir).is_empty(), "no file");
        let mut state = all_on();
        state.tags = vec![
            Tag {
                tag: "plays: Hades".into(),
                signal: Signal::Games,
                topic: Some("Hades".into()),
            },
            Tag {
                tag: "pc: NVIDIA GPU".into(),
                signal: Signal::Hardware,
                topic: None,
            },
        ];
        save(&dir.join(FILE_NAME), &state).unwrap();
        let tags = load_tags(&dir);
        assert_eq!(
            tags,
            vec![
                KnowMeTag {
                    label: "plays: Hades".into(),
                    topic: "Hades".into(),
                },
                KnowMeTag {
                    label: "pc: NVIDIA GPU".into(),
                    topic: String::new(),
                },
            ]
        );
        assert_eq!(tag_topics(&tags), vec!["hades", "pc: nvidia gpu"]);
        assert!(tags_mention(&tags, &["nvidia"]));
        assert!(tags_mention(&tags, &["Hades"]));
        assert!(!tags_mention(&tags, &["amd", ""]));
        state.on = false;
        save(&dir.join(FILE_NAME), &state).unwrap();
        assert!(load_tags(&dir).is_empty(), "off shares nothing");
        let _ = std::fs::remove_dir_all(dir);
    }
}

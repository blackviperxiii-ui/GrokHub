//! Round 3 item 36: driver checks on installs.
//!
//! Two rules for any install the agent proposes in a shell command:
//! - **Low-level input drivers are never installed.** Interception and its
//!   kin hook every key and mouse event below Windows; anti-cheats (Fortnite,
//!   Rust and Apex Legends among them) refuse to start while one is loaded.
//!   The pre-check's floor refuses the command.
//! - **Fan, RGB and monitoring tools are checked against Microsoft's
//!   vulnerable driver blocklist.** Many of them load a kernel driver, and
//!   some versions of those drivers are on the list: Windows may refuse to
//!   load them, and anti-cheats such as BattlEye and Vanguard won't start a
//!   game while a flagged driver runs. A match gets a white card that says so
//!   in plain words, so the person decides.
//!
//! The blocklist is Microsoft's own (updated quarterly). The app fetches it
//! in the background on Windows, keeps only the driver file names it lists,
//! and caches them in `driver_blocklist.json`. With no cache, no tool is
//! flagged: the check never guesses.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// Where Microsoft publishes the blocklist (a zip of App Control policies).
pub const BLOCKLIST_URL: &str = "https://aka.ms/VulnerableDriverBlockList";
/// The hosts the blocklist download may go through, redirects included.
pub const BLOCKLIST_HOSTS: &[&str] = &["aka.ms", "go.microsoft.com", "download.microsoft.com"];
/// Largest download kept, in bytes.
pub const BLOCKLIST_CAP: u64 = 16 * 1024 * 1024;
/// A cache older than this is fetched again (the list changes quarterly).
pub const REFRESH_DAYS: u64 = 30;
const CACHE: &str = "driver_blocklist.json";

/// Low-level input drivers Loadout never installs: (word in the package or
/// installer name, the name people know).
pub const INPUT_DRIVERS: &[(&str, &str)] = &[("interception", "Interception")];

/// A hardware tool that loads a kernel driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverTool {
    pub name: &'static str,
    /// What it is for, in a word or two.
    pub kind: &'static str,
    /// Words in a package id or installer name that mean this tool.
    pub words: &'static [&'static str],
    /// Driver files it has shipped. Only those the blocklist names count.
    pub drivers: &'static [&'static str],
}

/// Fan, RGB and monitoring tools with kernel drivers. The blocklist, not this
/// table, decides whether one is flagged.
pub const DRIVER_TOOLS: &[DriverTool] = &[
    DriverTool {
        name: "SpeedFan",
        kind: "fan control",
        words: &["speedfan"],
        drivers: &["speedfan.sys"],
    },
    DriverTool {
        name: "MSI Afterburner",
        kind: "GPU tuning",
        words: &["afterburner"],
        drivers: &["rtcore64.sys", "rtcore32.sys"],
    },
    DriverTool {
        name: "Fan Control",
        kind: "fan control",
        words: &["fancontrol", "fan-control"],
        drivers: &["winring0x64.sys", "winring0.sys"],
    },
    DriverTool {
        name: "Open Hardware Monitor",
        kind: "hardware monitor",
        words: &["openhardwaremonitor"],
        drivers: &["winring0x64.sys", "winring0.sys"],
    },
    DriverTool {
        name: "Libre Hardware Monitor",
        kind: "hardware monitor",
        words: &["librehardwaremonitor"],
        drivers: &["winring0x64.sys", "winring0.sys"],
    },
    DriverTool {
        name: "OpenRGB",
        kind: "RGB lighting",
        words: &["openrgb"],
        drivers: &["winring0x64.sys", "winring0.sys", "inpoutx64.sys"],
    },
    DriverTool {
        name: "ThrottleStop",
        kind: "CPU tuning",
        words: &["throttlestop"],
        drivers: &["throttlestop.sys"],
    },
    DriverTool {
        name: "CPU-Z",
        kind: "hardware monitor",
        words: &["cpu-z", "cpuz"],
        drivers: &["cpuz141_x64.sys", "cpuz141.sys"],
    },
    DriverTool {
        name: "HWiNFO",
        kind: "hardware monitor",
        words: &["hwinfo"],
        drivers: &["hwinfo64a.sys", "hwinfo32.sys"],
    },
    DriverTool {
        name: "Gigabyte RGB Fusion and App Center",
        kind: "RGB lighting",
        words: &["rgbfusion", "rgb-fusion", "aorus", "gigabyte"],
        drivers: &["gdrv.sys", "gdrv2.sys"],
    },
    DriverTool {
        name: "ASUS AI Suite and Armoury Crate",
        kind: "RGB lighting and fan control",
        words: &["aisuite", "ai-suite", "armourycrate", "armoury-crate"],
        drivers: &["asio.sys", "asio2.sys", "asio3.sys", "asio64.sys"],
    },
    DriverTool {
        name: "Corsair iCUE",
        kind: "RGB lighting",
        words: &["icue"],
        drivers: &["corsairllaccess64.sys", "corsairllaccess32.sys"],
    },
    DriverTool {
        name: "ASRock Polychrome and A-Tuning",
        kind: "RGB lighting",
        words: &["polychrome", "a-tuning", "atuning"],
        drivers: &[
            "asrdrv101.sys",
            "asrdrv102.sys",
            "asrdrv103.sys",
            "asrdrv104.sys",
            "asrdrv106.sys",
        ],
    },
];

/// Package managers' install verbs: (head, the words after it that install).
const INSTALL_VERBS: &[(&str, &[&str])] = &[
    ("winget", &["install", "add"]),
    ("choco", &["install", "upgrade"]),
    ("scoop", &["install"]),
    ("pacman", &["-s", "-sy", "-syu", "-u"]),
    ("yay", &["-s", "-sy", "-syu", "-u"]),
    ("paru", &["-s", "-sy", "-syu", "-u"]),
    ("apt", &["install"]),
    ("apt-get", &["install"]),
    ("dnf", &["install"]),
    ("zypper", &["install", "in"]),
    ("flatpak", &["install"]),
    ("snap", &["install"]),
    ("pip", &["install"]),
    ("pip3", &["install"]),
    ("npm", &["install", "i", "add"]),
    ("cargo", &["install"]),
    ("msiexec", &["/i", "/package", "-i"]),
];

fn leaf(word: &str) -> String {
    let w = word.trim_matches(['"', '\'']).replace('\\', "/");
    w.rsplit('/').next().unwrap_or(&w).to_ascii_lowercase()
}

/// The names an install command installs: package ids after an install verb,
/// and any installer file (`.exe`, `.msi`) the command runs.
pub fn installs(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    for seg in cmd.split([';', '|', '&', '\n', '\r']) {
        let words: Vec<String> = seg.split_whitespace().map(leaf).collect();
        for w in &words {
            let installer = [".exe", ".msi", ".zip"].iter().any(|e| w.ends_with(e))
                || w.starts_with("install-")
                || w.contains("setup");
            if installer {
                out.push(w.clone());
            }
        }
        let Some(at) = words.iter().position(|w| {
            let head = w.trim_end_matches(".exe");
            INSTALL_VERBS.iter().any(|(h, _)| *h == head)
        }) else {
            continue;
        };
        let head = words[at].trim_end_matches(".exe").to_string();
        let verbs = INSTALL_VERBS
            .iter()
            .find(|(h, _)| *h == head)
            .map(|(_, v)| *v)
            .unwrap_or(&[]);
        let rest = &words[at + 1..];
        if let Some(v) = rest.iter().position(|w| verbs.contains(&w.as_str())) {
            out.extend(rest[v + 1..].iter().filter_map(|w| {
                if let Some(id) = w
                    .strip_prefix("--id=")
                    .or_else(|| w.strip_prefix("--name="))
                {
                    return Some(id.to_string());
                }
                (!w.starts_with('-') && !w.starts_with('/') && !w.contains('=')).then(|| w.clone())
            }));
        }
    }
    let mut seen = BTreeSet::new();
    out.retain(|n| seen.insert(n.clone()));
    out
}

/// Why the pre-check refuses `cmd`, when it installs a low-level input
/// driver.
pub fn input_driver_install(cmd: &str) -> Option<String> {
    let names = installs(cmd);
    INPUT_DRIVERS.iter().find_map(|(word, name)| {
        names.iter().any(|n| n.contains(word)).then(|| {
            format!(
                "hard floor: low-level input driver ({name}). Anti-cheats refuse to start games while one is loaded, so Loadout never installs one."
            )
        })
    })
}

/// The driver tool `cmd` installs, if any.
pub fn proposed_tool(cmd: &str) -> Option<&'static DriverTool> {
    let names = installs(cmd);
    DRIVER_TOOLS
        .iter()
        .find(|t| names.iter().any(|n| t.words.iter().any(|w| n.contains(w))))
}

/// Microsoft's blocklist, as Loadout keeps it: the driver file names only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blocklist {
    /// When it was fetched (Unix seconds).
    pub fetched_at: u64,
    /// Lowercase driver file names (`rtcore64.sys`).
    pub names: BTreeSet<String>,
}

/// A tool whose driver the blocklist names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    pub tool: &'static DriverTool,
    pub driver: &'static str,
}

/// The flag for `cmd` against `list`, if it installs a tool whose driver the
/// list names.
pub fn flag(cmd: &str, list: &Blocklist) -> Option<Flag> {
    let tool = proposed_tool(cmd)?;
    let driver = tool.drivers.iter().find(|d| list.names.contains(**d))?;
    Some(Flag { tool, driver })
}

/// The flag in plain words, for the card.
pub fn explain(f: &Flag) -> String {
    format!(
        "{} ({}) installs {}, a driver on Microsoft's vulnerable driver blocklist. Windows may refuse to load it, and anti-cheats such as BattlEye and Vanguard won't start a game while a flagged driver runs. A newer build may ship a fixed driver, so check the tool's own download page first.",
        f.tool.name, f.tool.kind, f.driver
    )
}

/// Driver file names in a blocklist policy XML: each `FileName` attribute,
/// and each `.sys` name a rule's `FriendlyName` gives (hash rules name their
/// file only there).
pub fn parse_xml(xml: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for attr in ["FileName=\"", "FriendlyName=\""] {
        let mut rest = xml;
        while let Some(at) = rest.find(attr) {
            rest = &rest[at + attr.len()..];
            let Some(end) = rest.find('"') else { break };
            let value = &rest[..end];
            rest = &rest[end..];
            for word in value.split(|c: char| c.is_whitespace() || c == '\\' || c == '/') {
                let w = word.trim().to_ascii_lowercase();
                let stem = w.trim_end_matches(".sys");
                if w.ends_with(".sys")
                    && !stem.is_empty()
                    && stem
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                {
                    out.insert(w);
                }
            }
        }
    }
    out
}

/// Driver names from the downloaded zip: every `.xml` policy in it.
pub fn names_from_zip(bytes: &[u8]) -> Result<BTreeSet<String>, String> {
    use std::io::Read;
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not a zip: {e}"))?;
    let mut names = BTreeSet::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        let xml_file = file
            .name()
            .is_ok_and(|n| n.to_ascii_lowercase().ends_with(".xml"));
        if !xml_file {
            continue;
        }
        let mut xml = String::new();
        file.by_ref()
            .take(BLOCKLIST_CAP)
            .read_to_string(&mut xml)
            .map_err(|e| e.to_string())?;
        names.extend(parse_xml(&xml));
    }
    if names.is_empty() {
        return Err("the download held no driver rules".into());
    }
    Ok(names)
}

fn cache_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CACHE)
}

pub fn load(config_dir: &Path) -> Option<Blocklist> {
    let text = std::fs::read_to_string(cache_path(config_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save(config_dir: &Path, list: &Blocklist) -> std::io::Result<()> {
    let text = serde_json::to_string(list).map_err(std::io::Error::other)?;
    std::fs::create_dir_all(config_dir)?;
    let tmp = config_dir.join(format!("{CACHE}.tmp"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, cache_path(config_dir))
}

/// Whether the cache is missing or older than [`REFRESH_DAYS`].
pub fn due(config_dir: &Path, now_secs: u64) -> bool {
    load(config_dir).is_none_or(|l| now_secs.saturating_sub(l.fetched_at) > REFRESH_DAYS * 86_400)
}

/// The cached list for `config_dir`, read again only when the file changes.
pub fn cached(config_dir: &Path) -> Arc<Blocklist> {
    type Seen = Option<(PathBuf, Option<std::time::SystemTime>, Arc<Blocklist>)>;
    static SEEN: OnceLock<Mutex<Seen>> = OnceLock::new();
    let path = cache_path(config_dir);
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let mut seen = SEEN
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some((p, m, list)) = seen.as_ref() {
        if *p == path && *m == mtime {
            return list.clone();
        }
    }
    let list = Arc::new(load(config_dir).unwrap_or_default());
    *seen = Some((path, mtime, list.clone()));
    list
}

#[cfg(test)]
#[path = "driver_check_tests.rs"]
mod tests;

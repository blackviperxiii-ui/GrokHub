//! The Brief: plain rules on the ask, and one fast-model call only when the
//! rules can't tell. The ask's text is read here and never stored: a Brief
//! holds only fixed words (intent, care, mood, domain, and a done list from a
//! fixed table), so `brain/briefs.jsonl` carries nothing the user typed.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The router class the fallback call goes through.
pub const BRIEF_CLASS: &str = "background:classify";

/// The fallback call's instructions: a tiny JSON answer, nothing else.
pub const BRIEF_PROMPT: &str = "Sort the user's request. Reply with one line of JSON and nothing else: \
{\"intent\":\"chat|question|task|repair|creative|research|background\",\"care\":\"casual|careful|critical\",\
\"mood\":\"calm|urgent|frustrated\",\"domain\":\"system|code|files|desktop|web|media|general\"}";

/// Lines kept in `briefs.jsonl`; older ones are dropped when it passes [`BRIEF_TRIM_AT`].
const BRIEF_KEEP: usize = 400;
const BRIEF_TRIM_AT: usize = 500;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    #[default]
    Chat,
    Question,
    Task,
    Repair,
    Creative,
    Research,
    Background,
}

impl Intent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Question => "question",
            Self::Task => "task",
            Self::Repair => "repair",
            Self::Creative => "creative",
            Self::Research => "research",
            Self::Background => "background",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "chat" => Self::Chat,
            "question" => Self::Question,
            "task" => Self::Task,
            "repair" => Self::Repair,
            "creative" => Self::Creative,
            "research" => Self::Research,
            "background" => Self::Background,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Care {
    #[default]
    Casual,
    Careful,
    Critical,
}

impl Care {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Casual => "casual",
            Self::Careful => "careful",
            Self::Critical => "critical",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "casual" => Self::Casual,
            "careful" => Self::Careful,
            "critical" => Self::Critical,
            _ => return None,
        })
    }
}

/// What a run is for, decided before it starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brief {
    pub intent: Intent,
    pub care: Care,
    /// `calm`, `urgent` or `frustrated`.
    pub mood: String,
    /// What the run checks before it calls the ask done.
    pub done: Vec<String>,
    /// `system`, `code`, `files`, `desktop`, `web`, `media` or `general`.
    pub domain: String,
    /// `rules`, `model`, or `default` (unclear and no model call).
    pub by: String,
}

impl Brief {
    /// "repair × critical, done = services, packages, boot errors, ports"
    pub fn label(&self) -> String {
        let done = if self.done.is_empty() {
            "answered".to_string()
        } else {
            self.done.join(", ")
        };
        format!(
            "{} × {}, done = {}",
            self.intent.as_str(),
            self.care.as_str(),
            done
        )
    }
}

/// What the rules look at. Nothing here is stored.
#[derive(Debug, Clone, Copy, Default)]
pub struct BriefInput<'a> {
    pub text: &'a str,
    /// Attached files or images.
    pub files: usize,
    /// Tools the caller already knows the run will use.
    pub tools: &'a [String],
    /// Earlier user texts in this chat, oldest first.
    pub recent: &'a [String],
    /// A scheduled, proactive or `/bg` run.
    pub background: bool,
}

const SYSTEM_WORDS: &[&str] = &[
    "sudo",
    "systemctl",
    "systemd",
    "service",
    "services",
    "daemon",
    "package",
    "packages",
    "apt",
    "pacman",
    "dnf",
    "yum",
    "flatpak",
    "snap",
    "install",
    "reinstall",
    "uninstall",
    "driver",
    "drivers",
    "kernel",
    "boot",
    "bootloader",
    "grub",
    "firewall",
    "ufw",
    "iptables",
    "registry",
    "regedit",
    "partition",
    "fstab",
    "mount",
    "port",
    "ports",
    "chmod",
    "chown",
    "permissions",
    "bios",
    "uefi",
    "upgrade",
    "cron",
];
const REPAIR_WORDS: &[&str] = &[
    "fix", "broken", "broke", "crash", "crashes", "crashed", "crashing", "error", "errors",
    "fails", "failed", "failing", "bug", "debug", "freeze", "freezes", "frozen", "hangs", "stuck",
    "repair",
];
const REPAIR_PHRASES: &[&str] = &[
    "not working",
    "won't",
    "doesn't work",
    "stopped working",
    "can't start",
    "health check",
];
/// Diagnostics is repair work: the design's "repair/diagnostics" intent.
const DIAGNOSE_WORDS: &[&str] = &["scan", "diagnose", "diagnostics", "troubleshoot", "checkup"];
/// A feature or config ask ("use X for Y", "you should ..."): new work, never a
/// repair, even when it says "scan". Repair is for something reported broken.
const FEATURE_PHRASES: &[&str] = &[
    "you should",
    "it should",
    "it would be nice",
    "i want it to",
    "add support",
    "feature",
];
/// The machine as a whole: the system domain, without being a change to it.
const MACHINE_WORDS: &[&str] = &[
    "computer", "pc", "laptop", "machine", "os", "windows", "linux",
];
const CREATIVE_WORDS: &[&str] = &[
    "poem",
    "story",
    "lyrics",
    "song",
    "draw",
    "sketch",
    "logo",
    "image",
    "picture",
    "imagine",
    "haiku",
    "slogan",
    "wallpaper",
];
const RESEARCH_WORDS: &[&str] = &[
    "research",
    "compare",
    "sources",
    "latest",
    "news",
    "investigate",
];
const RESEARCH_PHRASES: &[&str] = &["look up", "find out"];
const TASK_VERBS: &[&str] = &[
    "make", "build", "create", "set", "install", "move", "rename", "delete", "remove", "organize",
    "automate", "open", "run", "add", "update", "convert", "write", "send", "download", "clean",
    "sort", "copy", "schedule", "edit", "change", "turn", "close", "start", "stop", "restart",
    "click",
];
const QUESTION_WORDS: &[&str] = &[
    "what", "why", "how", "when", "where", "who", "which", "is", "are", "can", "could", "does",
    "do", "should", "will", "would", "explain",
];
const CHAT_WORDS: &[&str] = &[
    "hi", "hello", "hey", "thanks", "thank", "thx", "yo", "morning", "lol", "cool", "nice", "bye",
];
const FOLLOW_UPS: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "go",
    "do it",
    "ok",
    "okay",
    "sure",
    "continue",
    "try again",
    "again",
];
const POLITE: &[&str] = &[
    "please",
    "can you",
    "could you",
    "would you",
    "will you",
    "i want you to",
    "i need you to",
];
const DESTRUCTIVE_WORDS: &[&str] = &[
    "delete", "remove", "wipe", "erase", "format", "rm", "password", "payment", "bank",
];
const CODE_WORDS: &[&str] = &[
    "code",
    "function",
    "compile",
    "cargo",
    "git",
    "script",
    "python",
    "rust",
    "javascript",
];
/// Working an app on screen: clicks, windows, buttons.
const DESKTOP_WORDS: &[&str] = &[
    "click", "window", "screen", "desktop", "app", "button", "taskbar",
];
const URGENT_WORDS: &[&str] = &["asap", "urgent", "urgently", "quick", "quickly", "now"];
const FRUSTRATED_WORDS: &[&str] = &["ugh", "wtf", "again", "still", "seriously", "annoying"];

fn words(t: &str) -> Vec<&str> {
    t.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .collect()
}

fn any_word(ws: &[&str], list: &[&str]) -> bool {
    ws.iter().any(|w| list.contains(w))
}

fn any_phrase(t: &str, list: &[&str]) -> bool {
    list.iter().any(|p| t.contains(p))
}

/// The ask touches the machine itself: services, packages, boot, ports, permissions.
fn system_change(t: &str, ws: &[&str]) -> bool {
    any_word(ws, SYSTEM_WORDS) || t.contains("/etc/")
}

fn done_for(intent: Intent, domain: &str) -> Vec<String> {
    let list: &[&str] = match (intent, domain) {
        (Intent::Repair, "system") => &["services", "packages", "boot errors", "ports"],
        (Intent::Repair, "code") => &["it builds", "tests pass", "the error is gone"],
        (Intent::Repair, _) => &["the error is gone", "it works again"],
        (Intent::Task, "system") => &["change applied", "service running", "undo noted"],
        (Intent::Task, "files") => &["files in place", "nothing lost"],
        (Intent::Task, "code") => &["it builds", "tests pass"],
        (Intent::Task, "desktop") => &["the asked change is made", "result checked on screen"],
        (Intent::Task, _) => &["the asked change is made", "result checked"],
        (Intent::Question, _) => &["answered"],
        (Intent::Research, _) => &["sources named", "answer summarized"],
        (Intent::Creative, _) => &["draft delivered"],
        (Intent::Background, _) => &["job finished"],
        (Intent::Chat, _) => &["answered"],
    };
    list.iter().map(|s| s.to_string()).collect()
}

fn mood_of(raw: &str, ws: &[&str]) -> &'static str {
    let letters: Vec<char> = raw.chars().filter(|c| c.is_alphabetic()).collect();
    let shouting = letters.len() >= 8 && letters.iter().all(|c| c.is_uppercase());
    if shouting || raw.contains("!!") || any_word(ws, FRUSTRATED_WORDS) {
        "frustrated"
    } else if any_word(ws, URGENT_WORDS) {
        "urgent"
    } else {
        "calm"
    }
}

fn domain_of(
    t: &str,
    ws: &[&str],
    input: &BriefInput<'_>,
    intent: Intent,
    system: bool,
) -> &'static str {
    let source = |w: &str| {
        [".rs", ".py", ".js", ".ts", ".go", ".c", ".cpp"]
            .iter()
            .any(|e| w.trim_end_matches([',', '.', ')']).ends_with(e))
    };
    let code = t.contains("```")
        || t.contains("()")
        || t.split_whitespace().any(source)
        || any_word(ws, CODE_WORDS);
    let files = input.files > 0
        || any_word(
            ws,
            &[
                "file",
                "files",
                "folder",
                "folders",
                "downloads",
                "documents",
            ],
        );
    if system || any_word(ws, MACHINE_WORDS) {
        "system"
    } else if code {
        "code"
    } else if any_word(ws, DESKTOP_WORDS) {
        "desktop"
    } else if files {
        "files"
    } else if intent == Intent::Research {
        "web"
    } else if intent == Intent::Creative {
        "media"
    } else {
        "general"
    }
}

/// The rules' Brief, or `None` when they can't tell what the ask is.
pub fn rules(input: &BriefInput<'_>) -> Option<Brief> {
    let t = input.text.trim().to_lowercase();
    let ws = words(&t);
    let system = system_change(&t, &ws);
    if !input.background
        && !input.recent.is_empty()
        && FOLLOW_UPS.contains(&t.trim_end_matches(['.', '!']).trim())
    {
        let last = input.recent.last().map(String::as_str).unwrap_or_default();
        return rules(&BriefInput {
            text: last,
            recent: &[],
            ..*input
        });
    }
    if t.is_empty() && input.files == 0 {
        return None;
    }
    let mut rest = t.as_str();
    for p in POLITE {
        rest = rest.strip_prefix(p).map(str::trim_start).unwrap_or(rest);
    }
    let first = words(rest).first().copied().unwrap_or_default();
    // "use X for Y", with X up to four words.
    let use_for = ws
        .iter()
        .enumerate()
        .any(|(i, w)| *w == "use" && ws.iter().skip(i + 1).take(5).any(|n| *n == "for"));
    let feature = use_for || any_phrase(&t, FEATURE_PHRASES);
    let broken = any_word(&ws, REPAIR_WORDS) || any_phrase(&t, REPAIR_PHRASES);
    let repair = broken || (any_word(&ws, DIAGNOSE_WORDS) && !feature);
    let intent = if input.background {
        Intent::Background
    } else if repair {
        Intent::Repair
    } else if feature {
        Intent::Task
    } else if any_word(&ws, CREATIVE_WORDS) {
        Intent::Creative
    } else if any_word(&ws, RESEARCH_WORDS) || any_phrase(&t, RESEARCH_PHRASES) {
        Intent::Research
    } else if TASK_VERBS.contains(&first) || input.files > 0 || !input.tools.is_empty() || system {
        Intent::Task
    } else if QUESTION_WORDS.contains(&first) || t.ends_with('?') {
        Intent::Question
    } else if ws.len() <= 4 && CHAT_WORDS.contains(&first) {
        Intent::Chat
    } else {
        return None;
    };
    let destructive = any_word(&ws, DESTRUCTIVE_WORDS) || t.contains("rm -rf");
    let care = if system || intent == Intent::Repair || (destructive && intent == Intent::Task) {
        Care::Critical
    } else if matches!(intent, Intent::Task | Intent::Research) {
        Care::Careful
    } else {
        Care::Casual
    };
    let domain = domain_of(&t, &ws, input, intent, system);
    Some(Brief {
        intent,
        care,
        mood: mood_of(input.text, &ws).into(),
        done: done_for(intent, domain),
        domain: domain.into(),
        by: "rules".into(),
    })
}

/// The model's one-line JSON, checked word by word. Anything off is `None`.
fn parse_model(reply: &str) -> Option<(Intent, Care, String, String)> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let v: serde_json::Value = serde_json::from_str(reply.get(start..=end)?).ok()?;
    let word = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
    };
    let intent = Intent::parse(&word("intent")?)?;
    let care = Care::parse(&word("care")?)?;
    let mood = word("mood")
        .filter(|m| ["calm", "urgent", "frustrated"].contains(&m.as_str()))
        .unwrap_or_else(|| "calm".into());
    let domain = word("domain")
        .filter(|d| grokhub_core::playbook::DOMAINS.contains(&d.as_str()))
        .unwrap_or_else(|| "general".into());
    Some((intent, care, mood, domain))
}

/// Rules first. When they can't tell and `ask` is given, one call to it with
/// [`BRIEF_PROMPT`] decides; a bad or failed answer gives the default Brief.
/// The rules always catch a system change, so the model never rates one.
pub fn classify(input: &BriefInput<'_>, ask: Option<AskModel<'_>>) -> Brief {
    if let Some(b) = rules(input) {
        return b;
    }
    let fallback = || Brief {
        mood: "calm".into(),
        domain: "general".into(),
        by: "default".into(),
        ..Default::default()
    };
    let Some(ask) = ask else {
        return fallback();
    };
    let Some((intent, care, mood, domain)) = ask(input.text).ok().as_deref().and_then(parse_model)
    else {
        return fallback();
    };
    Brief {
        intent,
        care,
        mood,
        done: done_for(intent, &domain),
        domain,
        by: "model".into(),
    }
}

/// The one fallback call: the ask's text in, the model's reply out.
pub type AskModel<'a> = &'a mut dyn FnMut(&str) -> Result<String, String>;

/// The policy goal this Brief names. Callers pass care-first rows when the
/// flag is on, and they do not also read keyword steer.
pub fn effort_word(brief: &Brief, rows: &[grokhub_core::brain_policy::PolicyRow]) -> String {
    grokhub_core::brain_policy::pick_row(rows, brief.intent.as_str(), brief.care.as_str())
        .map(|row| row.goal.clone())
        .unwrap_or_else(|| "balanced".into())
}

/// Classify stays on the cheap background effort. The care-first dry-run did
/// not show a quality gain that would pay for a higher rung.
pub fn classify_effort(care_first: bool) -> &'static str {
    match care_first {
        true | false => grokhub_core::BACKGROUND_EFFORT,
    }
}

/// One stored Brief: who it was for, never the ask's text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefLine {
    pub at: u64,
    pub session: String,
    /// The chat turn it belongs to, when the cabin had one open.
    #[serde(default)]
    pub chat_id: String,
    #[serde(default)]
    pub turn: u32,
    pub brief: Brief,
    /// The mode policy row the Brief matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<super::PolicyPick>,
    /// The playbook the Brief picked, by name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub playbook: String,
    /// The same playbook by id, for the run's metrics line (Brain 6).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub playbook_id: String,
}

impl BriefLine {
    /// The run-metrics share of this line (Brain 6): the Brief's intent, care
    /// and domain, the policy row's key and the playbook's id.
    pub fn run_brain(&self) -> grokhub_core::run_metrics::RunBrain {
        grokhub_core::run_metrics::RunBrain {
            intent: self.brief.intent.as_str().into(),
            care: self.brief.care.as_str().into(),
            domain: self.brief.domain.clone(),
            policy: self
                .policy
                .as_ref()
                .map(|p| p.row.clone())
                .unwrap_or_default(),
            playbook: self.playbook_id.clone(),
        }
    }

    /// "Brief: …" and, when they matched, "Policy: …", "Talk: …" and "Playbook: …".
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![format!("Brief: {}", self.brief.label())];
        if let Some(p) = &self.policy {
            out.push(format!("Policy: {}", p.label()));
            out.push(format!("Talk: {}", p.style().label()));
        }
        if !self.playbook.is_empty() {
            out.push(format!("Playbook: {}", self.playbook));
        }
        out
    }
}

fn briefs_path(config_dir: &Path) -> PathBuf {
    config_dir.join("brain").join("briefs.jsonl")
}

/// Append a Brief to `{config}/brain/briefs.jsonl`, trimming old lines.
pub fn record_brief(config_dir: &Path, line: &BriefLine) -> Result<(), String> {
    let path = briefs_path(config_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string(line).map_err(|e| e.to_string())?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(f, "{body}").map_err(|e| e.to_string())?;
    drop(f);
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() > BRIEF_TRIM_AT {
        let kept = lines[lines.len() - BRIEF_KEEP..].join("\n");
        fs::write(&path, format!("{kept}\n")).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Every readable stored Brief, oldest first.
pub fn read_briefs(config_dir: &Path) -> Vec<BriefLine> {
    fs::read_to_string(briefs_path(config_dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// The newest Brief for one chat turn (the trail shows it).
pub fn brief_for_turn(config_dir: &Path, chat_id: &str, turn: u32) -> Option<BriefLine> {
    if chat_id.is_empty() {
        return None;
    }
    read_briefs(config_dir)
        .into_iter()
        .rev()
        .find(|l| l.chat_id == chat_id && l.turn == turn)
}

/// `/why`'s first lines: the newest Brief and its policy row, and a note
/// when the user's policy file is being ignored.
pub fn why_lines(config_dir: &Path) -> Vec<String> {
    let mut out = read_briefs(config_dir)
        .pop()
        .map(|l| l.lines())
        .unwrap_or_default();
    if let (_, Some(why)) = super::load_policy(config_dir) {
        out.push(format!(
            "Policy file ignored ({why}); using the shipped rows."
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(text: &str) -> BriefInput<'_> {
        BriefInput {
            text,
            ..Default::default()
        }
    }

    fn short(b: &Brief) -> (Intent, Care, &str, &str) {
        (b.intent, b.care, b.domain.as_str(), b.mood.as_str())
    }

    #[test]
    fn a_fixture_set_of_asks_gets_the_right_brief() {
        let cases: &[(&str, Intent, Care, &str, &str)] = &[
            ("hi there", Intent::Chat, Care::Casual, "general", "calm"),
            ("thanks!", Intent::Chat, Care::Casual, "general", "calm"),
            (
                "What is the capital of Peru?",
                Intent::Question,
                Care::Casual,
                "general",
                "calm",
            ),
            (
                "explain how tides work",
                Intent::Question,
                Care::Casual,
                "general",
                "calm",
            ),
            (
                "Please rename the photos in my Downloads folder by date",
                Intent::Task,
                Care::Careful,
                "files",
                "calm",
            ),
            (
                "delete the old backups in Documents",
                Intent::Task,
                Care::Critical,
                "files",
                "calm",
            ),
            (
                "the printer service won't start after the update",
                Intent::Repair,
                Care::Critical,
                "system",
                "calm",
            ),
            (
                "my script crashes on line 4, fix it asap",
                Intent::Repair,
                Care::Critical,
                "code",
                "urgent",
            ),
            (
                "WIFI IS BROKEN AGAIN",
                Intent::Repair,
                Care::Critical,
                "general",
                "frustrated",
            ),
            (
                "write a poem about autumn",
                Intent::Creative,
                Care::Casual,
                "media",
                "calm",
            ),
            (
                "compare the latest mid-range laptops",
                Intent::Research,
                Care::Careful,
                "web",
                "calm",
            ),
            (
                "open port 8080 on the firewall",
                Intent::Task,
                Care::Critical,
                "system",
                "calm",
            ),
            (
                "scan my computer",
                Intent::Repair,
                Care::Critical,
                "system",
                "calm",
            ),
            (
                "add a retry to fetch() in src/net.rs",
                Intent::Task,
                Care::Careful,
                "code",
                "calm",
            ),
            (
                "click the save button in the settings window",
                Intent::Task,
                Care::Careful,
                "desktop",
                "calm",
            ),
            (
                "open the mail app and archive the top message",
                Intent::Task,
                Care::Careful,
                "desktop",
                "calm",
            ),
            // A feature ask that says "scan" is new work, not a repair.
            (
                "you should scan my steam library and use that for pulse",
                Intent::Task,
                Care::Careful,
                "general",
                "calm",
            ),
            (
                "use my notes folder for the daily digest",
                Intent::Task,
                Care::Careful,
                "files",
                "calm",
            ),
            // Something reported broken stays a repair even in feature words.
            (
                "you should fix the crash in the pulse feed",
                Intent::Repair,
                Care::Critical,
                "general",
                "calm",
            ),
        ];
        for (text, intent, care, domain, mood) in cases {
            let b = rules(&input(text)).unwrap_or_else(|| panic!("{text}: rules gave no brief"));
            assert_eq!(short(&b), (*intent, *care, *domain, *mood), "{text}");
            assert_eq!(b.by, "rules", "{text}");
        }
        assert_eq!(
            rules(&input(
                "you should scan my steam library and use that for pulse"
            ))
            .unwrap()
            .label(),
            "task × careful, done = the asked change is made, result checked"
        );
        let repair = rules(&input("the printer service won't start after the update")).unwrap();
        assert_eq!(
            repair.label(),
            "repair × critical, done = services, packages, boot errors, ports"
        );
        assert_eq!(
            rules(&input("hi there")).unwrap().label(),
            "chat × casual, done = answered"
        );
        let attached = BriefInput {
            text: "these",
            files: 1,
            ..Default::default()
        };
        assert_eq!(rules(&attached).unwrap().intent, Intent::Task);
        let bg = BriefInput {
            text: "summarize my inbox",
            background: true,
            ..Default::default()
        };
        assert_eq!(
            short(&rules(&bg).unwrap()),
            (Intent::Background, Care::Casual, "general", "calm")
        );
        let recent = ["the printer service won't start".to_string()];
        let follow = BriefInput {
            text: "yes",
            recent: &recent,
            ..Default::default()
        };
        assert_eq!(
            short(&rules(&follow).unwrap()),
            (Intent::Repair, Care::Critical, "system", "calm")
        );
    }

    #[test]
    fn unclear_asks_call_the_model_exactly_once_and_clear_ones_never() {
        let mut calls = Vec::new();
        let mut ask = |t: &str| {
            calls.push(t.to_string());
            Ok::<_, String>(
                r#"Sure: {"intent":"task","care":"careful","mood":"calm","domain":"files"}"#
                    .to_string(),
            )
        };
        let b = classify(
            &input("the thing from yesterday, the spreadsheet one"),
            Some(&mut ask),
        );
        assert_eq!(
            calls,
            vec!["the thing from yesterday, the spreadsheet one".to_string()]
        );
        assert_eq!(
            b,
            Brief {
                intent: Intent::Task,
                care: Care::Careful,
                mood: "calm".into(),
                done: vec!["files in place".into(), "nothing lost".into()],
                domain: "files".into(),
                by: "model".into(),
            }
        );

        let n = std::cell::Cell::new(0);
        let mut count = |_: &str| {
            n.set(n.get() + 1);
            Ok::<_, String>("{}".to_string())
        };
        assert_eq!(classify(&input("hello"), Some(&mut count)).by, "rules");
        assert_eq!(n.get(), 0);
        let bad = classify(&input("the thing from yesterday"), Some(&mut count));
        assert_eq!(n.get(), 1);
        assert_eq!(short(&bad), (Intent::Chat, Care::Casual, "general", "calm"));
        assert_eq!(bad.by, "default");
        let none = classify(&input("the thing from yesterday"), None);
        assert_eq!((none.intent, none.by.as_str()), (Intent::Chat, "default"));
        let mut fail = |_: &str| Err::<String, _>("offline".to_string());
        assert_eq!(
            classify(&input("the thing from yesterday"), Some(&mut fail)).by,
            "default"
        );
    }

    #[test]
    fn a_system_change_is_always_critical() {
        for text in [
            "sudo apt install nginx",
            "the kernel panics at boot",
            "chmod my home folder",
            "edit /etc/hosts for me",
            "set up a cron entry",
            "turn on the ssh service",
            "reinstall the nvidia drivers",
        ] {
            let b = rules(&input(text)).unwrap_or_else(|| panic!("{text}: rules gave no brief"));
            assert_eq!(
                (b.care, b.domain.as_str()),
                (Care::Critical, "system"),
                "{text}"
            );
        }
        // The rules decide it, so a model that would call it casual is never asked.
        let asked = std::cell::Cell::new(0);
        let mut casual = |_: &str| {
            asked.set(asked.get() + 1);
            Ok::<_, String>(
                r#"{"intent":"chat","care":"casual","mood":"calm","domain":"general"}"#.to_string(),
            )
        };
        let b = classify(&input("that grub thing"), Some(&mut casual));
        assert_eq!(
            (b.care, b.domain.as_str(), b.by.as_str()),
            (Care::Critical, "system", "rules")
        );
        assert_eq!(asked.get(), 0);
    }

    #[test]
    fn a_hangout_chat_talks_naturally_with_no_word_cap() {
        let shipped = grokhub_core::brain_policy::builtin_policy();
        for text in ["hey", "just chilling tonight", "😉", "nah just chill"] {
            let b = classify(&input(text), None);
            assert_eq!((b.intent, b.care), (Intent::Chat, Care::Casual), "{text}");
            let row =
                grokhub_core::brain_policy::pick_row(&shipped, b.intent.as_str(), b.care.as_str())
                    .unwrap();
            let style = grokhub_core::talk::TalkStyle::of(&row.talk);
            assert_eq!(style, grokhub_core::talk::TalkStyle::Natural, "{text}");
            assert_eq!(style.reply_cap_words(), None);
            assert_eq!(row.stop, "need met");
        }
    }

    #[test]
    fn care_first_effort_comes_from_the_brief_and_classify_stays_low() {
        let hi = rules(&input("hey")).unwrap();
        assert_eq!((hi.intent, hi.care), (Intent::Chat, Care::Casual));
        let shipped = grokhub_core::brain_policy::builtin_policy();
        assert_eq!(effort_word(&hi, &shipped), "care");
        let on = grokhub_core::brain_policy::apply_care_first(shipped);
        assert_eq!(effort_word(&hi, &on), "care");
        let question = rules(&input("what time is it?")).unwrap();
        assert_eq!(question.intent, Intent::Question);
        assert_eq!(effort_word(&question, &on), "speed");
        assert_eq!(classify_effort(false), "low");
        assert_eq!(classify_effort(true), grokhub_core::BACKGROUND_EFFORT);
        assert_eq!(
            crate::route::policy::class_row(BRIEF_CLASS).unwrap().start,
            "low"
        );
    }

    #[test]
    fn briefs_are_stored_without_the_ask_and_found_by_turn() {
        let dir = std::env::temp_dir().join(format!("gh-brief-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(why_lines(&dir).is_empty());
        let text = "the printer service won't start, my password is hunter2";
        let b = rules(&input(text)).unwrap();
        let policy = Some(super::super::PolicyPick {
            row: "repair × critical".into(),
            goal: "quality".into(),
            ..Default::default()
        });
        let line = BriefLine {
            at: 1,
            session: "s1".into(),
            chat_id: "chat-1".into(),
            turn: 2,
            brief: b.clone(),
            policy,
            playbook: "Repair Linux".into(),
            playbook_id: "repair-linux".into(),
        };
        record_brief(&dir, &line).unwrap();
        let other = rules(&input("hi")).unwrap();
        record_brief(
            &dir,
            &BriefLine {
                at: 2,
                session: "s1".into(),
                chat_id: "chat-1".into(),
                turn: 3,
                brief: other.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        // Brain 6: the run's metrics line reads the Brief, row and playbook id from the turn's line.
        let brain = brief_for_turn(&dir, "chat-1", 2).unwrap().run_brain();
        assert_eq!(
            (
                brain.intent.as_str(),
                brain.care.as_str(),
                brain.domain.as_str(),
                brain.policy.as_str(),
                brain.playbook.as_str()
            ),
            (
                "repair",
                "critical",
                "system",
                "repair × critical",
                "repair-linux"
            )
        );
        assert_eq!(
            brief_for_turn(&dir, "chat-1", 3)
                .unwrap()
                .run_brain()
                .playbook,
            ""
        );
        let raw = fs::read_to_string(briefs_path(&dir)).unwrap();
        assert!(
            !raw.contains("hunter2") && !raw.contains("printer"),
            "{raw}"
        );
        assert_eq!(brief_for_turn(&dir, "chat-1", 2).map(|l| l.brief), Some(b));
        assert_eq!(brief_for_turn(&dir, "chat-1", 9), None);
        assert_eq!(brief_for_turn(&dir, "", 2), None);
        assert_eq!(
            why_lines(&dir),
            vec!["Brief: chat × casual, done = answered".to_string()]
        );
        for i in 0..BRIEF_TRIM_AT as u64 {
            record_brief(
                &dir,
                &BriefLine {
                    at: 10 + i,
                    session: "s".into(),
                    brief: other.clone(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let kept = read_briefs(&dir);
        assert_eq!(kept.len(), BRIEF_KEEP + 1);
        assert_eq!(kept.last().unwrap().at, 10 + BRIEF_TRIM_AT as u64 - 1);
        let _ = fs::remove_dir_all(&dir);
    }
}

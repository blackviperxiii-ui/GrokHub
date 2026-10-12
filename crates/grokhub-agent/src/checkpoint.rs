//! Brain 2 checkpoints: what a run must still show before it may end, beyond
//! its own checklist ([`crate::drive`]). A change (a file write or edit, a
//! shell command that isn't read-only, a desktop action) needs a check after
//! it: a test, build or lint run, a re-read of the changed file, a read-only
//! look after a shell change, or a screenshot after a desktop change. A
//! careful or critical task, repair or research run must cover each item of
//! its Brief's done list in the checklist, and a critical system change needs
//! a backup. Only a run with a Brief (a chat or background turn) owes these.
//! Nothing here caps steps; the drive's stall count ends a run that
//! can't make progress, naming what is left.
//!
//! Brain 5: a careful repair or research run also owes a findings card, and
//! a long task in a progress style gets a short note at each phase boundary
//! (a checklist item closing), rate-limited by [`PROGRESS_GAP`].

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::Value;

use grokhub_core::findings::{Finding, Findings, Fix, Severity, MAX_FINDINGS};
use grokhub_core::talk::TalkStyle;

use crate::brain::{Brief, Care, Intent};
use crate::session_tools::TodoItem;

/// Task tool calls between mid-run checkpoints.
pub const CHECKPOINT_EVERY: usize = 8;
/// Task tool calls at least between two progress notes (and before the first).
pub const PROGRESS_GAP: usize = 3;
/// Items a progress note names before "and N more".
const PROGRESS_ITEMS: usize = 3;
/// Longest item text in a progress note or fallback card, in characters.
const ITEM_CHARS: usize = 60;

// Word lists are lowercase; commands are compared lowercased (PowerShell names too).

/// Words in a shell command that make it a check: tests, builds, linters, status.
const VERIFY_WORDS: &[&str] = &[
    "test", "tests", "nextest", "pytest", "check", "build", "clippy", "lint", "eslint", "tsc", "mypy", "ruff", "verify",
    "status", "is-active", "is-enabled", "--version", "diff", "test-path", "get-service",
];
/// Words in a shell command that save the state before a change.
const BACKUP_WORDS: &[&str] = &["backup", "timeshift", "snapper", "snapshot", "checkpoint-computer", "etckeeper"];
/// A copy to a `.bak` or `.orig` name is a backup too.
const COPY_WORDS: &[&str] = &["cp", "rsync", "tar", "copy-item", "copy"];
/// Words in a shell command, or path parts, that touch the system.
const SYSTEM_WORDS: &[&str] = &[
    "sudo", "systemctl", "apt", "apt-get", "pacman", "dnf", "yum", "flatpak", "snap", "modprobe", "grub-mkconfig",
    "update-grub", "ufw", "iptables", "chmod", "chown", "mount", "fstab", "reg", "sc.exe", "set-service",
];
const SYSTEM_PATHS: &[&str] = &["/etc/", "/boot/", "/usr/", "/lib/", "/var/lib/", "c:\\windows", "c:/windows"];
/// Words that make a command change files, packages, services or settings.
/// A system word alone (`systemctl status`) is not a change.
const MUTATING_HEADS: &[&str] = &[
    "rm", "mv", "cp", "mkdir", "rmdir", "touch", "tee", "ln", "install", "uninstall", "remove", "add", "commit", "push",
    "kill", "pkill", "remove-item", "set-content", "new-item", "move-item", "restart", "start", "stop", "enable",
    "disable", "mask", "unmask", "reload", "purge", "upgrade", "update", "chmod", "chown", "mount", "umount", "modprobe",
    "update-grub", "grub-mkconfig", "allow", "deny", "set-service",
];
const DESKTOP_ACTIONS: &[&str] = &["click", "move", "drag", "scroll", "type", "key"];
const STOP_WORDS: &[&str] = &["the", "is", "it", "a", "an", "in", "of", "and", "to"];

/// What one finished tool call was, for the checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// Changed this (a file path, a command, "the screen"). `checked`: the
    /// same command ran a check after the change (`edit && cargo test`).
    Change { what: String, system: bool, checked: bool },
    /// A check: re-read this path, or a check run (`None`) that covers any change.
    Verify(Option<String>),
    /// A read-only look that checks a shell change but not a file change.
    Look(String),
    Backup,
    Other,
}

fn words(s: &str) -> Vec<&str> {
    s.split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '"' | '\''))
        .filter(|w| !w.is_empty())
        .collect()
}

fn path_arg(args: &Value, keys: &[&str]) -> String {
    keys.iter().find_map(|k| args.get(*k).and_then(Value::as_str)).unwrap_or("").trim().to_string()
}

fn system_path(p: &str) -> bool {
    let p = p.to_ascii_lowercase();
    SYSTEM_PATHS.iter().any(|s| p.starts_with(s))
}

fn classify(name: &str, args: &Value) -> Vec<Step> {
    let one = |step| vec![step];
    match name {
        "write" | "search_replace" => {
            let path = path_arg(args, &["path", "file_path", "target_file"]);
            let lower = path.to_ascii_lowercase();
            // A copy written next to a system file (`/etc/fstab.bak`) is a backup;
            // other workspace files are ordinary changes, whatever their name.
            if (lower.ends_with(".bak") || lower.ends_with(".orig")) && system_path(&lower) {
                return one(Step::Backup);
            }
            one(Step::Change { system: system_path(&path), what: path, checked: false })
        }
        "read_file" => one(Step::Verify(Some(path_arg(args, &["target_file", "path", "file_path"])))),
        "screenshot" => one(Step::Verify(Some(DESKTOP_WHAT.into()))),
        n if DESKTOP_ACTIONS.contains(&n) => one(Step::Change { what: DESKTOP_WHAT.into(), system: false, checked: false }),
        "run_terminal_command" => shell_steps(args.get("command").and_then(Value::as_str).unwrap_or("").trim()),
        _ => one(Step::Other),
    }
}

/// A shell command, in order: a backup it takes, the change it makes (and
/// whether a check word comes after the change), or a check or a read-only look.
fn shell_steps(cmd: &str) -> Vec<Step> {
    let lower = cmd.to_ascii_lowercase();
    let ws = words(&lower);
    let mut steps = Vec::new();
    if ws.iter().any(|w| BACKUP_WORDS.contains(w))
        || (ws.iter().any(|w| COPY_WORDS.contains(w)) && (lower.contains(".bak") || lower.contains(".orig")))
    {
        steps.push(Step::Backup);
    }
    let system = ws.iter().any(|w| SYSTEM_WORDS.contains(w) || system_path(w));
    // Only a command that clearly changes something owes a check (not a fetch or a query).
    let changes = |w: &&str| w.contains('>') || *w == "-i" || MUTATING_HEADS.contains(w);
    let first_change = ws.iter().position(&changes);
    let last_check = ws.iter().rposition(|w| VERIFY_WORDS.contains(w));
    match (first_change, last_check) {
        (Some(c), check) => {
            // `sudo cp /etc/fstab /etc/fstab.bak` is the backup, not a change to check.
            let backup_only = !steps.is_empty() && ws.iter().all(|w| !changes(w) || COPY_WORDS.contains(w));
            if !backup_only {
                steps.push(Step::Change { what: short(cmd), system, checked: check.is_some_and(|v| v > c) });
            }
        }
        (None, Some(_)) => steps.push(Step::Verify(None)),
        (None, None) if crate::perm::shell_is_readonly(cmd) => steps.push(Step::Look(cmd.to_string())),
        (None, None) => {}
    }
    if steps.is_empty() {
        steps.push(Step::Other);
    }
    steps
}

const DESKTOP_WHAT: &str = "the screen";

fn short(cmd: &str) -> String {
    let one = cmd.lines().next().unwrap_or("").trim();
    if one.chars().count() > 60 {
        format!("{}…", one.chars().take(60).collect::<String>())
    } else {
        one.to_string()
    }
}

/// A file change is checked by a re-read of that file (by its name).
fn rereads(changed: &str, read: &str) -> bool {
    let name = |p: &str| p.replace('\\', "/").rsplit('/').next().unwrap_or("").to_string();
    !changed.is_empty() && (read == changed || name(read) == name(changed))
}

/// The run's changes and checks so far.
#[derive(Debug, Clone, Default)]
pub struct Fresh {
    /// The newest change no check has covered yet, and whether it was a file.
    unchecked: Option<(String, bool)>,
    /// The first system change, while no backup came before or after it.
    unbacked: Option<String>,
    backed_up: bool,
    /// This run posted a findings card.
    findings: bool,
}

impl Fresh {
    /// Note one tool call that ran without failing.
    pub fn note(&mut self, name: &str, args: &Value) {
        let file = matches!(name, "write" | "search_replace");
        for step in classify(name, args) {
            match step {
                Step::Change { what, system, checked } => {
                    if system && !self.backed_up && self.unbacked.is_none() {
                        self.unbacked = Some(what.clone());
                    }
                    self.unchecked = (!checked).then_some((what, file));
                }
                Step::Verify(None) => self.unchecked = None,
                Step::Verify(Some(read)) => {
                    if self.unchecked.as_ref().is_some_and(|(what, _)| rereads(what, &read) || (what == DESKTOP_WHAT && read == DESKTOP_WHAT)) {
                        self.unchecked = None;
                    }
                }
                Step::Look(_) => {
                    if self.unchecked.as_ref().is_some_and(|(what, file)| !file && what != DESKTOP_WHAT) {
                        self.unchecked = None;
                    }
                }
                Step::Backup => {
                    self.backed_up = true;
                    self.unbacked = None;
                }
                Step::Other => {}
            }
        }
    }

    /// The change no check has covered since it ran.
    pub fn unchecked(&self) -> Option<&str> {
        self.unchecked.as_ref().map(|(w, _)| w.as_str())
    }

    /// Note that `report_findings` posted this run's card.
    pub fn note_findings(&mut self) {
        self.findings = true;
    }
}

/// A careful or critical repair or research run that did tool work ends with
/// a findings card.
fn owes_findings(brief: Option<&Brief>, evidence: usize) -> bool {
    brief.is_some_and(|b| evidence > 0 && b.care >= Care::Careful && matches!(b.intent, Intent::Repair | Intent::Research))
}

/// The Brief's done items a careful or critical task, repair or research run
/// has not covered in its checklist (an item completed or cancelled whose text
/// names it). Empty for other runs and before the run did any tool work.
pub fn uncovered(brief: Option<&Brief>, todos: &[TodoItem], evidence: usize) -> Vec<String> {
    let Some(b) = brief.filter(|b| evidence > 0 && b.care >= Care::Careful && matches!(b.intent, Intent::Task | Intent::Repair | Intent::Research)) else {
        return Vec::new();
    };
    b.done
        .iter()
        .filter(|item| {
            let keys: Vec<String> = words(item).into_iter().map(str::to_ascii_lowercase).filter(|w| !STOP_WORDS.contains(&w.as_str())).collect();
            !todos.iter().any(|t| {
                let text = t.content.to_ascii_lowercase();
                matches!(t.status.as_str(), "completed" | "cancelled") && !keys.is_empty() && keys.iter().all(|k| text.contains(k.as_str()))
            })
        })
        .cloned()
        .collect()
}

/// What the run still owes before it may end, beyond its checklist's open items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gaps {
    /// The Brief's done items not covered yet.
    pub uncovered: Vec<String>,
    /// The newest change with no check after it.
    pub unchecked: Option<String>,
    /// A system change in a critical run with no backup.
    pub unbacked: Option<String>,
    /// A repair or research run that has not posted its findings card.
    pub findings: bool,
}

impl Gaps {
    /// A run with no Brief (a bare loop, not a chat turn) owes nothing here.
    pub fn of(brief: Option<&Brief>, todos: &[TodoItem], evidence: usize, fresh: &Fresh) -> Self {
        let critical = brief.is_some_and(|b| b.care == Care::Critical);
        Self {
            uncovered: uncovered(brief, todos, evidence),
            unchecked: fresh.unchecked().filter(|_| brief.is_some()).map(str::to_string),
            unbacked: fresh.unbacked.clone().filter(|_| critical),
            findings: !fresh.findings && owes_findings(brief, evidence),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.uncovered.is_empty() && self.unchecked.is_none() && self.unbacked.is_none() && !self.findings
    }

    /// Stable names for the drive's stall count.
    pub fn keys(&self) -> Vec<String> {
        let mut k: Vec<String> = self.uncovered.iter().map(|u| format!("done:{u}")).collect();
        k.extend(self.unchecked.iter().map(|u| format!("unchecked:{u}")));
        k.extend(self.unbacked.iter().map(|u| format!("unbacked:{u}")));
        if self.findings {
            k.push("findings".into());
        }
        k
    }

    /// Plain names for the blocked line.
    pub fn names(&self) -> Vec<String> {
        let mut n: Vec<String> = self.uncovered.iter().map(|u| format!("\"{u}\"")).collect();
        n.extend(self.unchecked.iter().map(|u| format!("a check after the change to {u}")));
        n.extend(self.unbacked.iter().map(|u| format!("a backup for {u}")));
        if self.findings {
            n.push("the findings card".into());
        }
        n
    }

    /// The re-plan note, one sentence per gap.
    pub fn note(&self) -> String {
        let mut parts = Vec::new();
        if let Some(what) = &self.unbacked {
            parts.push(format!(
                "Changed {what} with no backup first. Save a copy of the current state now and say how to undo the change."
            ));
        }
        if let Some(what) = &self.unchecked {
            parts.push(format!(
                "Not verified since your last change to {what}. Check it (run the tests or build, re-read the file, look at the result) before you finish."
            ));
        }
        if !self.uncovered.is_empty() {
            let list = self.uncovered.iter().map(|u| format!("\"{u}\"")).collect::<Vec<_>>().join(", ");
            parts.push(format!(
                "This ask is done only when it shows {list}. Put each in your checklist (todo_write) and mark it completed once a tool result shows it, or cancelled with the reason it is blocked."
            ));
        }
        if self.findings {
            parts.push(
                "End with the findings card: call report_findings naming each item you checked and its root cause, with a one-tap fix for what is left."
                    .to_string(),
            );
        }
        parts.join(" ")
    }
}

/// An item's first line, without a closing period, clipped.
fn clip(text: &str) -> String {
    let one = text.lines().next().unwrap_or("").trim().trim_end_matches('.');
    if one.chars().count() > ITEM_CHARS {
        format!("{}…", one.chars().take(ITEM_CHARS).collect::<String>())
    } else {
        one.to_string()
    }
}

fn closed(t: &TodoItem) -> bool {
    matches!(t.status.as_str(), "completed" | "cancelled")
}

fn named(items: &[&TodoItem]) -> String {
    let mut out: Vec<String> = items.iter().take(PROGRESS_ITEMS).map(|t| clip(&t.content)).collect();
    if items.len() > PROGRESS_ITEMS {
        let more = format!("{} and {} more", out.pop().unwrap_or_default(), items.len() - PROGRESS_ITEMS);
        out.push(more);
    }
    out.join(", ")
}

/// Brain 5 progress notes for a long run: "Done: services, packages. Next:
/// boot errors." A note comes only at a phase boundary (a checklist item
/// closed since the last note) while an item is still open (the reply covers
/// the end), and only once [`PROGRESS_GAP`] tool calls ran since the last
/// note. A boundary that comes too soon is told with the next one.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    /// Closed items already told, or closed when the run started.
    told: Vec<(String, String)>,
    /// Tool calls at the last note.
    at: usize,
}

impl Progress {
    pub fn new(start: &[TodoItem]) -> Self {
        Self { told: start.iter().filter(|t| closed(t)).map(|t| (t.id.clone(), t.content.clone())).collect(), at: 0 }
    }

    /// The note to show now, if any. `evidence` counts the run's task tool calls.
    pub fn check(&mut self, todos: &[TodoItem], evidence: usize) -> Option<String> {
        let fresh: Vec<&TodoItem> =
            todos.iter().filter(|t| closed(t) && !self.told.iter().any(|(id, c)| *id == t.id && *c == t.content)).collect();
        if fresh.is_empty() || evidence < self.at + PROGRESS_GAP {
            return None;
        }
        let next = todos.iter().find(|t| t.status == "in_progress").or_else(|| todos.iter().find(|t| t.status == "pending"))?;
        let done: Vec<&TodoItem> = fresh.iter().copied().filter(|t| t.status == "completed").collect();
        let skipped: Vec<&TodoItem> = fresh.iter().copied().filter(|t| t.status == "cancelled").collect();
        let mut parts = Vec::new();
        if !done.is_empty() {
            parts.push(format!("Done: {}.", named(&done)));
        }
        if !skipped.is_empty() {
            parts.push(format!("Skipped: {}.", named(&skipped)));
        }
        parts.push(format!("Next: {}.", clip(&next.content)));
        self.told.extend(fresh.iter().map(|t| (t.id.clone(), t.content.clone())));
        self.at = evidence;
        Some(parts.join(" "))
    }
}

/// The card a repair or research run that owes one ends with when it stops
/// blocked without posting it: each checklist item by what became of it,
/// what the run still owed, and a one-tap fix to finish the rest. `None` when
/// there is nothing to name.
pub fn fallback_findings(todos: &[TodoItem], gaps: &Gaps) -> Option<Findings> {
    let mut items: Vec<Finding> = Vec::new();
    let mut open: Vec<String> = Vec::new();
    for t in todos {
        let text = clip(&t.content);
        let (severity, said) = match t.status.as_str() {
            "completed" => (Severity::Info, format!("Done: {text}")),
            "cancelled" => (Severity::Low, format!("Skipped: {text}")),
            _ => {
                open.push(text.clone());
                (Severity::Medium, format!("Not finished: {text}"))
            }
        };
        items.push(Finding { severity, text: said });
    }
    for u in &gaps.uncovered {
        open.push(clip(u));
        items.push(Finding { severity: Severity::Medium, text: format!("Not shown yet: {}", clip(u)) });
    }
    if let Some(u) = &gaps.unchecked {
        items.push(Finding { severity: Severity::Medium, text: format!("Not checked since the change to {u}") });
    }
    if let Some(u) = &gaps.unbacked {
        items.push(Finding { severity: Severity::High, text: format!("No backup before {u}") });
    }
    if items.is_empty() {
        return None;
    }
    items.sort_by_key(|f| f.severity);
    items.truncate(MAX_FINDINGS);
    let fixes = if open.is_empty() {
        Vec::new()
    } else {
        vec![Fix { label: "Finish the rest".into(), goal: format!("Finish what the last run left open: {}.", open.join("; ")) }]
    };
    Some(Findings { items, fixes })
}

/// The turn's Brief, by chat session, for the run loop's checkpoints.
static BRIEFS: Mutex<BTreeMap<String, Brief>> = Mutex::new(BTreeMap::new());
const BRIEFS_CAP: usize = 256;

pub fn set_turn_brief(session: &str, brief: Brief) {
    if session.is_empty() {
        return;
    }
    let mut m = BRIEFS.lock().unwrap_or_else(|e| e.into_inner());
    if m.len() >= BRIEFS_CAP && !m.contains_key(session) {
        m.clear();
    }
    m.insert(session.to_string(), brief);
}

pub fn turn_brief(session: &str) -> Option<Brief> {
    BRIEFS.lock().unwrap_or_else(|e| e.into_inner()).get(session).cloned()
}

/// The turn's talk style, by chat session (Brain 5). `None` clears it.
static STYLES: Mutex<BTreeMap<String, TalkStyle>> = Mutex::new(BTreeMap::new());

pub fn set_turn_style(session: &str, style: Option<TalkStyle>) {
    if session.is_empty() {
        return;
    }
    let mut m = STYLES.lock().unwrap_or_else(|e| e.into_inner());
    let Some(style) = style else {
        m.remove(session);
        return;
    };
    if m.len() >= BRIEFS_CAP && !m.contains_key(session) {
        m.clear();
    }
    m.insert(session.to_string(), style);
}

pub fn turn_style(session: &str) -> Option<TalkStyle> {
    STYLES.lock().unwrap_or_else(|e| e.into_inner()).get(session).copied()
}

/// The turn's playbook paragraph, by chat session (Brain 3). `None` clears it.
static PLAYBOOKS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

pub fn set_turn_playbook(session: &str, block: Option<String>) {
    if session.is_empty() {
        return;
    }
    let mut m = PLAYBOOKS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(block) = block else {
        m.remove(session);
        return;
    };
    if m.len() >= BRIEFS_CAP && !m.contains_key(session) {
        m.clear();
    }
    m.insert(session.to_string(), block);
}

pub fn turn_playbook(session: &str) -> Option<String> {
    PLAYBOOKS.lock().unwrap_or_else(|e| e.into_inner()).get(session).cloned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn shell(cmd: &str) -> Value {
        json!({ "command": cmd })
    }

    fn todo(content: &str, status: &str) -> TodoItem {
        TodoItem { id: content.into(), content: content.into(), status: status.into() }
    }

    #[test]
    fn a_change_waits_for_a_check_that_fits_it() {
        let mut f = Fresh::default();
        f.note("search_replace", &json!({ "file_path": "src/main.rs" }));
        assert_eq!(f.unchecked(), Some("src/main.rs"));
        // A look at something else doesn't check a file edit.
        f.note("run_terminal_command", &shell("ls -la"));
        f.note("read_file", &json!({ "target_file": "src/lib.rs" }));
        assert_eq!(f.unchecked(), Some("src/main.rs"));
        // Re-reading the changed file does, and so does a test run.
        f.note("read_file", &json!({ "target_file": "./src/main.rs" }));
        assert_eq!(f.unchecked(), None);
        f.note("write", &json!({ "path": "a.txt" }));
        f.note("run_terminal_command", &shell("cargo test -p grokhub-agent"));
        assert_eq!(f.unchecked(), None);
        // A shell change is checked by any read-only look; a desktop action by a screenshot.
        f.note("run_terminal_command", &shell("systemctl restart nginx"));
        assert_eq!(f.unchecked(), Some("systemctl restart nginx"));
        f.note("run_terminal_command", &shell("ls /etc/nginx"));
        assert_eq!(f.unchecked(), None);
        f.note("click", &json!({ "x": 1, "y": 2 }));
        f.note("run_terminal_command", &shell("ls"));
        assert_eq!(f.unchecked(), Some("the screen"));
        f.note("screenshot", &json!({}));
        assert_eq!(f.unchecked(), None);
        // A fetch or a query changes nothing; an install or a redirect does.
        f.note("run_terminal_command", &shell("curl -s https://wttr.in/?format=3"));
        f.note("run_terminal_command", &shell("python3 -c 'print(1)'"));
        assert_eq!(f.unchecked(), None);
        f.note("run_terminal_command", &shell("pip install requests"));
        assert_eq!(f.unchecked(), Some("pip install requests"));
        f.note("run_terminal_command", &shell("echo hi > out.txt"));
        assert_eq!(f.unchecked(), Some("echo hi > out.txt"));
        // A check word only counts after the change in the same command.
        f.note("run_terminal_command", &shell("sed -i 's/a/b/' x.py && pytest"));
        assert_eq!(f.unchecked(), None);
        f.note("run_terminal_command", &shell("pytest && rm -rf build/"));
        assert_eq!(f.unchecked(), Some("pytest && rm -rf build/"));
        // PowerShell names in any case.
        f.note("run_terminal_command", &shell("Get-Service spooler"));
        assert_eq!(f.unchecked(), None);
        f.note("run_terminal_command", &shell("remove-item C:\\temp\\x.txt"));
        assert_eq!(f.unchecked(), Some("remove-item C:\\temp\\x.txt"));
        // A workspace file named like a backup is still a change.
        f.note("write", &json!({ "path": "notes/backup-plan.md" }));
        assert_eq!(f.unchecked(), Some("notes/backup-plan.md"));
    }

    #[test]
    fn a_critical_system_change_needs_a_backup() {
        // A critical task: a repair would also owe its findings card (tested below).
        let critical = Brief { intent: Intent::Task, care: Care::Critical, ..Brief::default() };
        let mut f = Fresh::default();
        f.note("run_terminal_command", &shell("sudo sed -i 's/a/b/' /etc/fstab"));
        f.note("run_terminal_command", &shell("systemctl status fstab"));
        let gaps = Gaps::of(Some(&critical), &[], 2, &f);
        assert_eq!(gaps.unbacked.as_deref(), Some("sudo sed -i 's/a/b/' /etc/fstab"));
        assert_eq!(gaps.unchecked, None);
        assert_eq!(
            gaps.note(),
            "Changed sudo sed -i 's/a/b/' /etc/fstab with no backup first. Save a copy of the current state now and say how to undo the change."
        );
        // A careful run doesn't need one; a backup (before or after) clears it.
        let careful = Brief { care: Care::Careful, ..critical.clone() };
        assert_eq!(Gaps::of(Some(&careful), &[], 2, &f).unbacked, None);
        f.note("run_terminal_command", &shell("sudo cp /etc/fstab /etc/fstab.bak"));
        assert_eq!(Gaps::of(Some(&critical), &[], 3, &f), Gaps::default());
        // A system change hidden behind a check word still needs its backup; a status look is no change.
        let mut h = Fresh::default();
        h.note("run_terminal_command", &shell("systemctl status nginx"));
        assert_eq!(Gaps::of(Some(&critical), &[], 1, &h), Gaps::default());
        h.note("run_terminal_command", &shell("sudo systemctl restart nginx && systemctl is-active nginx"));
        assert_eq!(Gaps::of(Some(&critical), &[], 2, &h).unbacked.as_deref(), Some("sudo systemctl restart nginx && systemctl is-active nginx"));
        assert_eq!(h.unchecked(), None);
        // Writing a .bak in the workspace is not a system backup.
        h.note("write", &json!({ "path": "fstab.bak" }));
        assert!(Gaps::of(Some(&critical), &[], 3, &h).unbacked.is_some());
        // Backed up first: the change never counts as unbacked.
        let mut g = Fresh::default();
        g.note("run_terminal_command", &shell("timeshift --create"));
        g.note("write", &json!({ "path": "/etc/hosts" }));
        assert_eq!(Gaps::of(Some(&critical), &[], 2, &g).unbacked, None);
        // A bare loop with no Brief owes nothing.
        assert_eq!(Gaps::of(None, &[], 2, &g), Gaps::default());
    }

    #[test]
    fn the_done_list_counts_only_when_the_checklist_names_each_item() {
        let b = Brief { intent: Intent::Repair, care: Care::Critical, done: vec!["services".into(), "boot errors".into(), "ports".into()], ..Brief::default() };
        let todos = [todo("Check failed services", "completed"), todo("Boot errors in journalctl", "cancelled"), todo("open ports", "pending")];
        assert_eq!(uncovered(Some(&b), &todos, 3), vec!["ports".to_string()]);
        // No tool work yet, a casual run, a question or background job: nothing owed.
        assert!(uncovered(Some(&b), &[], 0).is_empty());
        assert!(uncovered(Some(&Brief { care: Care::Casual, ..b.clone() }), &[], 3).is_empty());
        assert!(uncovered(Some(&Brief { intent: Intent::Question, ..b.clone() }), &[], 3).is_empty());
        assert!(uncovered(Some(&Brief { intent: Intent::Background, ..b }), &[], 3).is_empty());
    }

    #[test]
    fn the_turn_brief_is_kept_per_session() {
        let b = Brief { intent: Intent::Task, ..Brief::default() };
        set_turn_brief("cp-test-a", b.clone());
        set_turn_brief("", Brief::default());
        assert_eq!(turn_brief("cp-test-a"), Some(b));
        assert_eq!(turn_brief(""), None);
        set_turn_style("cp-test-a", Some(TalkStyle::Findings));
        set_turn_style("", Some(TalkStyle::Short));
        assert_eq!(turn_style("cp-test-a"), Some(TalkStyle::Findings));
        assert_eq!(turn_style(""), None);
        set_turn_style("cp-test-a", None);
        assert_eq!(turn_style("cp-test-a"), None);
        set_turn_playbook("cp-test-a", Some("Playbook for this turn: Files.".into()));
        set_turn_playbook("", Some("Playbook for this turn: Research.".into()));
        assert_eq!(turn_playbook("cp-test-a").as_deref(), Some("Playbook for this turn: Files."));
        assert_eq!(turn_playbook(""), None);
        set_turn_playbook("cp-test-a", None);
        assert_eq!(turn_playbook("cp-test-a"), None);
    }

    #[test]
    fn progress_notes_come_only_at_phase_boundaries_and_are_rate_limited() {
        let start = [todo("Old cleanup", "completed"), todo("Check failed services", "pending"), todo("Verify packages", "pending"), todo("Boot errors.", "pending")];
        let mut p = Progress::new(&start);
        // Nothing closed yet: no note.
        assert_eq!(p.check(&start, 1), None);
        let mut now = start.to_vec();
        now[1].status = "completed".into();
        // A boundary after 2 tool calls waits for the gap, then is told.
        assert_eq!(p.check(&now, 2), None);
        assert_eq!(p.check(&now, 3).as_deref(), Some("Done: Check failed services. Next: Verify packages."));
        // Told once: more work with no new boundary says nothing.
        assert_eq!(p.check(&now, 4), None);
        now[2].status = "completed".into();
        now.push(todo("Firewall", "cancelled"));
        now[3].status = "in_progress".into();
        assert_eq!(p.check(&now, 5), None, "only 2 calls since the last note");
        assert_eq!(p.check(&now, 6).as_deref(), Some("Done: Verify packages. Skipped: Firewall. Next: Boot errors."));
        // The last item closing gets no note: the reply covers the end.
        now[3].status = "completed".into();
        assert_eq!(p.check(&now, 30), None);
        // Many items at once are named up to three.
        let many: Vec<TodoItem> = ["a", "b", "c", "d", "e"].iter().map(|n| todo(n, "completed")).chain([todo("f", "pending")]).collect();
        assert_eq!(Progress::default().check(&many, 3).as_deref(), Some("Done: a, b, c and 2 more. Next: f."));
    }

    #[test]
    fn a_careful_repair_or_research_run_owes_its_findings_card() {
        let repair = Brief { intent: Intent::Repair, care: Care::Careful, ..Brief::default() };
        let mut f = Fresh::default();
        let gaps = Gaps::of(Some(&repair), &[], 1, &f);
        assert!(gaps.findings);
        assert_eq!(gaps.keys(), vec!["findings".to_string()]);
        assert_eq!(gaps.names(), vec!["the findings card".to_string()]);
        assert_eq!(
            gaps.note(),
            "End with the findings card: call report_findings naming each item you checked and its root cause, with a one-tap fix for what is left."
        );
        assert!(Gaps::of(Some(&Brief { intent: Intent::Research, ..repair.clone() }), &[], 1, &f).findings);
        // No tool work, a casual repair, a task or no Brief: nothing owed.
        assert!(!Gaps::of(Some(&repair), &[], 0, &f).findings);
        assert!(!Gaps::of(Some(&Brief { care: Care::Casual, ..repair.clone() }), &[], 1, &f).findings);
        assert!(!Gaps::of(Some(&Brief { intent: Intent::Task, ..repair.clone() }), &[], 1, &f).findings);
        assert!(!Gaps::of(None, &[], 1, &f).findings);
        f.note_findings();
        assert_eq!(Gaps::of(Some(&repair), &[], 1, &f), Gaps::default());
    }

    #[test]
    fn a_blocked_repair_falls_back_to_a_card_naming_each_item() {
        let todos = [todo("Check failed services.", "completed"), todo("Firewall", "cancelled"), todo("Boot errors\nfrom journalctl", "pending")];
        let gaps = Gaps {
            uncovered: vec!["ports".into()],
            unchecked: Some("systemctl restart nginx".into()),
            unbacked: Some("sudo sed -i x /etc/fstab".into()),
            findings: true,
        };
        let card = fallback_findings(&todos, &gaps).unwrap();
        let body = "Findings from this run\n\n\
                    - High: No backup before sudo sed -i x /etc/fstab\n\
                    - Medium: Not finished: Boot errors\n\
                    - Medium: Not shown yet: ports\n\
                    - Medium: Not checked since the change to systemctl restart nginx\n\
                    - Low: Skipped: Firewall\n\
                    - Info: Done: Check failed services\n\n\
                    One-tap fixes\n\n\
                    - Finish the rest: Finish what the last run left open: Boot errors; ports.";
        assert_eq!(card.to_body(), body);
        assert_eq!(Findings::parse(body), Some(card));
        // All done: the card names the items and offers no fix; nothing at all: no card.
        let done = fallback_findings(&todos[..1], &Gaps::default()).unwrap();
        assert_eq!(done.to_body(), "Findings from this run\n\n- Info: Done: Check failed services");
        assert_eq!(fallback_findings(&[], &Gaps { findings: true, ..Gaps::default() }), None);
    }
}

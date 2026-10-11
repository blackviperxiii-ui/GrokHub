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

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::Value;

use crate::brain::{Brief, Care, Intent};
use crate::session_tools::TodoItem;

/// Task tool calls between mid-run checkpoints.
pub const CHECKPOINT_EVERY: usize = 8;

/// Words in a shell command that make it a check: tests, builds, linters, status.
const VERIFY_WORDS: &[&str] = &[
    "test", "tests", "nextest", "pytest", "check", "build", "clippy", "lint", "eslint", "tsc", "mypy", "ruff", "verify",
    "status", "is-active", "is-enabled", "--version", "diff", "Test-Path", "Get-Service",
];
/// Words in a shell command that save the state before a change.
const BACKUP_WORDS: &[&str] = &["backup", "timeshift", "snapper", "snapshot", "Checkpoint-Computer", "etckeeper"];
/// A copy to a `.bak` or `.orig` name is a backup too.
const COPY_WORDS: &[&str] = &["cp", "rsync", "tar", "Copy-Item", "copy"];
/// Words in a shell command, or path parts, that touch the system.
const SYSTEM_WORDS: &[&str] = &[
    "sudo", "systemctl", "apt", "apt-get", "pacman", "dnf", "yum", "flatpak", "snap", "modprobe", "grub-mkconfig",
    "update-grub", "ufw", "iptables", "chmod", "chown", "mount", "fstab", "reg", "sc.exe", "Set-Service",
];
const SYSTEM_PATHS: &[&str] = &["/etc/", "/boot/", "/usr/", "/lib/", "/var/lib/", "c:\\windows", "c:/windows"];
/// Commands that change files, packages or settings.
const MUTATING_HEADS: &[&str] = &[
    "rm", "mv", "cp", "mkdir", "rmdir", "touch", "tee", "ln", "install", "uninstall", "remove", "add", "commit", "push",
    "kill", "pkill", "Remove-Item", "Set-Content", "New-Item", "Move-Item",
];
const DESKTOP_ACTIONS: &[&str] = &["click", "move", "drag", "scroll", "type", "key"];
const STOP_WORDS: &[&str] = &["the", "is", "it", "a", "an", "in", "of", "and", "to"];

/// What one finished tool call was, for the checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// Changed this (a file path, a command, "the screen").
    Change { what: String, system: bool },
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

fn classify(name: &str, args: &Value) -> Step {
    match name {
        "write" | "search_replace" => {
            let path = path_arg(args, &["path", "file_path", "target_file"]);
            if path.ends_with(".bak") || path.contains("backup") {
                return Step::Backup;
            }
            Step::Change { system: system_path(&path), what: path }
        }
        "read_file" => Step::Verify(Some(path_arg(args, &["target_file", "path", "file_path"]))),
        "screenshot" => Step::Verify(Some(DESKTOP_WHAT.into())),
        n if DESKTOP_ACTIONS.contains(&n) => Step::Change { what: DESKTOP_WHAT.into(), system: false },
        "run_terminal_command" => {
            let cmd = args.get("command").and_then(Value::as_str).unwrap_or("").trim();
            let ws = words(cmd);
            if ws.iter().any(|w| BACKUP_WORDS.iter().any(|b| w.eq_ignore_ascii_case(b)))
                || (ws.iter().any(|w| COPY_WORDS.contains(w)) && (cmd.contains(".bak") || cmd.contains(".orig")))
            {
                return Step::Backup;
            }
            if ws.iter().any(|w| VERIFY_WORDS.contains(w)) {
                return Step::Verify(None);
            }
            if crate::perm::shell_is_readonly(cmd) {
                return Step::Look(cmd.to_string());
            }
            let system = ws.iter().any(|w| SYSTEM_WORDS.contains(w) || system_path(w));
            // Only a command that clearly changes something owes a check (not a fetch or a query).
            let changes = system || cmd.contains('>') || ws.contains(&"-i") || ws.iter().any(|w| MUTATING_HEADS.contains(w));
            if !changes {
                return Step::Other;
            }
            Step::Change { what: short(cmd), system }
        }
        _ => Step::Other,
    }
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
}

impl Fresh {
    /// Note one tool call that ran without failing.
    pub fn note(&mut self, name: &str, args: &Value) {
        match classify(name, args) {
            Step::Change { what, system } => {
                if system && !self.backed_up && self.unbacked.is_none() {
                    self.unbacked = Some(what.clone());
                }
                let file = matches!(name, "write" | "search_replace");
                self.unchecked = Some((what, file));
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

    /// The change no check has covered since it ran.
    pub fn unchecked(&self) -> Option<&str> {
        self.unchecked.as_ref().map(|(w, _)| w.as_str())
    }
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
}

impl Gaps {
    /// A run with no Brief (a bare loop, not a chat turn) owes nothing here.
    pub fn of(brief: Option<&Brief>, todos: &[TodoItem], evidence: usize, fresh: &Fresh) -> Self {
        let critical = brief.is_some_and(|b| b.care == Care::Critical);
        Self {
            uncovered: uncovered(brief, todos, evidence),
            unchecked: fresh.unchecked().filter(|_| brief.is_some()).map(str::to_string),
            unbacked: fresh.unbacked.clone().filter(|_| critical),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.uncovered.is_empty() && self.unchecked.is_none() && self.unbacked.is_none()
    }

    /// Stable names for the drive's stall count.
    pub fn keys(&self) -> Vec<String> {
        let mut k: Vec<String> = self.uncovered.iter().map(|u| format!("done:{u}")).collect();
        k.extend(self.unchecked.iter().map(|u| format!("unchecked:{u}")));
        k.extend(self.unbacked.iter().map(|u| format!("unbacked:{u}")));
        k
    }

    /// Plain names for the blocked line.
    pub fn names(&self) -> Vec<String> {
        let mut n: Vec<String> = self.uncovered.iter().map(|u| format!("\"{u}\"")).collect();
        n.extend(self.unchecked.iter().map(|u| format!("a check after the change to {u}")));
        n.extend(self.unbacked.iter().map(|u| format!("a backup for {u}")));
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
        parts.join(" ")
    }
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
    }

    #[test]
    fn a_critical_system_change_needs_a_backup() {
        let critical = Brief { intent: Intent::Repair, care: Care::Critical, ..Brief::default() };
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
    }
}

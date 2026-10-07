//! ChangeLedger, skills slice (harness design §11 Spike-5, §12 P3).
//!
//! Every write to a cabin skill that GrokHub makes on its own (a nightly
//! review patch, a skill learned from a host run, a junk skill moved aside)
//! keeps the version it replaces and appends one line to
//! `{config_dir}/changes/skills.jsonl`: skill id, time, origin, a short
//! reason, and the SHA-256 of the `SKILL.md` bytes before and after. The
//! ledger holds no skill text, and the id and reason pass through
//! `redact_secrets`. The bytes live beside it in
//! `{config_dir}/changes/skills/<id>/<ms>-<hash12>.md`, at most
//! [`HISTORY_CAP`] files per skill, so Undo puts back the exact file.
//!
//! Undo and restore need an [`UndoAsk`], which only the user's own typing in
//! the composer or a pointer click builds. No model reply, review pass, or
//! automation can make one (source-scanning test in grokhub-app), so the
//! model never undoes or re-applies a change on its own. An undone patch is
//! remembered: the nightly review does not write the same bytes again
//! ([`ChangeLedger::undone_by_user`]).
//!
//! Not built yet (rest of Spike-5): connections and automations, Work-tree
//! rows, git-backed history, and agent-initiated delete as a hard card.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::harness::span::Origin;

/// Folder under the cabin config dir.
pub const CHANGES_DIR: &str = "changes";
/// The skills ledger inside [`CHANGES_DIR`].
pub const SKILL_LEDGER_FILE: &str = "skills.jsonl";
/// Kept versions per skill. The oldest go first.
pub const HISTORY_CAP: usize = 20;
/// Ledger lines kept. Older lines are dropped on the next write.
pub const LEDGER_LINE_CAP: usize = 2000;
/// Largest ledger the cabin reads. A bigger file reads as empty.
const LEDGER_READ_CAP: u64 = 4 * 1024 * 1024;
/// Longest reason kept on a line, in chars.
const REASON_MAX: usize = 160;
/// Largest `SKILL.md` the ledger copies.
const SKILL_READ_CAP: u64 = 1024 * 1024;

/// One writer at a time: the nightly pass saves several skills on threads.
static LEDGER_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOp {
    Create,
    Modify,
    Delete,
    Undo,
    Restore,
}

impl ChangeOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Modify => "modify",
            Self::Delete => "delete",
            Self::Undo => "undo",
            Self::Restore => "restore",
        }
    }
}

/// One ledger line. Hashes are SHA-256 hex of the whole `SKILL.md`; an empty
/// hash means the skill was not there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub seq: u64,
    pub at: u64,
    pub kind: String,
    pub id: String,
    pub op: ChangeOp,
    #[serde(default)]
    pub origin: Origin,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub before_hash: String,
    #[serde(default)]
    pub after_hash: String,
    /// The `seq` an undo line reverted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undoes: Option<u64>,
}

/// Proof that the user asked for an undo or restore: their own typing in the
/// composer, or a pointer click. Build it only there; a source-scanning test
/// in grokhub-app fails if either constructor shows up anywhere else.
#[derive(Debug)]
pub struct UndoAsk(());

impl UndoAsk {
    pub fn from_click() -> Self {
        Self(())
    }

    pub fn from_typing() -> Self {
        Self(())
    }
}

/// The ledger id for a skill name: its folder name, with anything that looks
/// like a secret redacted.
pub fn change_id(name: &str) -> String {
    grokhub_core::redact_secrets(&grokhub_core::skill_dir_name(name))
}

pub fn skill_ledger_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CHANGES_DIR).join(SKILL_LEDGER_FILE)
}

/// Where the kept versions of one skill live.
pub fn skill_history_dir(config_dir: &Path, name: &str) -> PathBuf {
    config_dir.join(CHANGES_DIR).join("skills").join(change_id(name))
}

pub fn content_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The ledger as read from disk, oldest line first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeLedger {
    changes: Vec<Change>,
}

impl ChangeLedger {
    /// Missing, unreadable, or oversized reads as empty. Bad lines are skipped.
    pub fn load(config_dir: &Path) -> Self {
        let Ok(f) = fs::File::open(skill_ledger_path(config_dir)) else {
            return Self::default();
        };
        if f.metadata().map(|m| m.len() > LEDGER_READ_CAP).unwrap_or(true) {
            return Self::default();
        }
        let mut text = String::new();
        if f.take(LEDGER_READ_CAP).read_to_string(&mut text).is_err() {
            return Self::default();
        }
        let changes = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .filter_map(|l| serde_json::from_str::<Change>(l).ok())
            .filter(|c| c.kind == "skill" && !c.id.is_empty())
            .collect();
        Self { changes }
    }

    pub fn all(&self) -> &[Change] {
        &self.changes
    }

    fn for_skill<'a>(&'a self, name: &str) -> impl DoubleEndedIterator<Item = &'a Change> + 'a {
        let id = change_id(name);
        self.changes.iter().filter(move |c| c.id == id)
    }

    fn was_undone(&self, seq: u64) -> bool {
        self.changes.iter().any(|c| c.op == ChangeOp::Undo && c.undoes == Some(seq))
    }

    /// The change `/skills undo` reverts next: the newest one on this skill
    /// that is not an undo and was not undone already. Repeated undos walk
    /// back one version at a time.
    pub fn undo_target(&self, name: &str) -> Option<&Change> {
        self.for_skill(name)
            .rev()
            .find(|c| c.op != ChangeOp::Undo && !self.was_undone(c.seq))
    }

    /// The change whose `before` a restore brings back: the newest line on
    /// this skill, when that line left it gone.
    pub fn restore_source(&self, name: &str) -> Option<&Change> {
        self.for_skill(name)
            .next_back()
            .filter(|c| c.after_hash.is_empty() && !c.before_hash.is_empty())
    }

    /// The user undid a change that produced exactly these bytes. The nightly
    /// review must not write them again on its own.
    pub fn undone_by_user(&self, name: &str, after_hash: &str) -> bool {
        self.for_skill(name)
            .any(|c| c.op != ChangeOp::Undo && c.after_hash == after_hash && self.was_undone(c.seq))
    }

    /// The newest line on this skill came from the user (an undo or a
    /// restore), so cleanup must leave the skill alone.
    pub fn kept_by_user(&self, name: &str) -> bool {
        self.for_skill(name)
            .next_back()
            .is_some_and(|c| c.origin == Origin::User && matches!(c.op, ChangeOp::Undo | ChangeOp::Restore))
    }

    /// The newest `n` lines, newest first.
    pub fn recent(&self, n: usize) -> Vec<&Change> {
        self.changes.iter().rev().take(n).collect()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn skill_md(skills_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let dir = grokhub_core::skill_dir_name(name);
    if dir.is_empty() {
        return Err("no skill by that name".into());
    }
    Ok(skills_dir.join(dir).join("SKILL.md"))
}

fn read_skill(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::File::open(path) {
        Ok(f) => {
            if f.metadata().map(|m| m.len() > SKILL_READ_CAP).unwrap_or(true) {
                return Err("SKILL.md is too big to keep a copy".into());
            }
            let mut buf = Vec::new();
            f.take(SKILL_READ_CAP).read_to_end(&mut buf).map_err(|e| e.to_string())?;
            Ok(Some(buf))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn private_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut opts = OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn kept_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    files.sort();
    files
}

fn find_kept(dir: &Path, hash: &str) -> Option<PathBuf> {
    let tail = format!("-{}.md", hash.get(..12)?);
    kept_files(dir)
        .into_iter()
        .rev()
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(&tail)))
}

/// Keep one version of a skill. The same bytes kept before move to the
/// front instead of being copied twice; past [`HISTORY_CAP`] the oldest go.
fn keep_version(config_dir: &Path, name: &str, bytes: &[u8], at: u64) -> Result<String, String> {
    let hash = content_hash(bytes);
    let dir = skill_history_dir(config_dir, name);
    // Names sort oldest first, so a stamp never repeats inside one folder.
    let last = kept_files(&dir)
        .last()
        .and_then(|p| p.file_name()?.to_str()?.get(..16)?.parse::<u64>().ok())
        .unwrap_or(0);
    let stamp = at.max(last + 1);
    let fresh = dir.join(format!("{stamp:016}-{}.md", &hash[..12]));
    match find_kept(&dir, &hash) {
        Some(old) if old != fresh => fs::rename(&old, &fresh).map_err(|e| e.to_string())?,
        Some(_) => {}
        None => private_write(&fresh, bytes)?,
    }
    let files = kept_files(&dir);
    if files.len() > HISTORY_CAP {
        for old in &files[..files.len() - HISTORY_CAP] {
            let _ = fs::remove_file(old);
        }
    }
    Ok(hash)
}

/// The kept bytes for `hash`, checked against the hash.
fn kept_version(config_dir: &Path, name: &str, hash: &str) -> Result<Vec<u8>, String> {
    let path = find_kept(&skill_history_dir(config_dir, name), hash)
        .ok_or("that version is past the history cap")?;
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    if content_hash(&bytes) != hash {
        return Err("the kept copy does not match the ledger".into());
    }
    Ok(bytes)
}

fn append_change(config_dir: &Path, mut change: Change) -> Result<Change, String> {
    let ledger = ChangeLedger::load(config_dir);
    change.seq = ledger.changes.last().map(|c| c.seq + 1).unwrap_or(1);
    change.reason = grokhub_core::redact_secrets(&change.reason.chars().take(REASON_MAX).collect::<String>());
    let line = grokhub_core::redact_secrets(&serde_json::to_string(&change).map_err(|e| e.to_string())?);
    let path = skill_ledger_path(config_dir);
    if ledger.changes.len() >= LEDGER_LINE_CAP {
        let keep = &ledger.changes[ledger.changes.len() + 1 - LEDGER_LINE_CAP..];
        let mut text = String::new();
        for c in keep {
            text.push_str(&serde_json::to_string(c).map_err(|e| e.to_string())?);
            text.push('\n');
        }
        text.push_str(&line);
        text.push('\n');
        private_write(&path, text.as_bytes())?;
        return Ok(change);
    }
    fs::create_dir_all(config_dir.join(CHANGES_DIR)).map_err(|e| e.to_string())?;
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())?;
    Ok(change)
}

fn new_change(name: &str, op: ChangeOp, origin: Origin, reason: &str, at: u64) -> Change {
    Change {
        seq: 0,
        at,
        kind: "skill".into(),
        id: change_id(name),
        op,
        origin,
        reason: reason.trim().to_string(),
        before_hash: String::new(),
        after_hash: String::new(),
        undoes: None,
    }
}

/// Run `write` on one skill with the ledger around it: the current
/// `SKILL.md` is kept first, then `write` runs, then the new bytes are kept
/// and one line is appended. `Ok(None)` when the bytes did not change.
/// `write` may create, change, or remove the skill folder.
pub fn record_skill_change(
    config_dir: &Path,
    skills_dir: &Path,
    name: &str,
    origin: Origin,
    reason: &str,
    write: impl FnOnce() -> Result<(), String>,
) -> Result<Option<Change>, String> {
    let _held = LEDGER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = skill_md(skills_dir, name)?;
    let at = now_ms();
    let before = read_skill(&path)?;
    let before_hash = match &before {
        Some(b) => keep_version(config_dir, name, b, at)?,
        None => String::new(),
    };
    write()?;
    let after = read_skill(&path)?;
    if after == before {
        return Ok(None);
    }
    let after_hash = match &after {
        Some(b) => keep_version(config_dir, name, b, at)?,
        None => String::new(),
    };
    let op = match (&before, &after) {
        (None, _) => ChangeOp::Create,
        (_, None) => ChangeOp::Delete,
        _ => ChangeOp::Modify,
    };
    let mut change = new_change(name, op, origin, reason, at);
    change.before_hash = before_hash;
    change.after_hash = after_hash;
    append_change(config_dir, change).map(Some)
}

/// What an undo or restore left on disk: the line it wrote and the
/// `SKILL.md` bytes now in place (`None` when the skill is gone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reverted {
    pub change: Change,
    pub reverted: Option<Change>,
    pub now: Option<Vec<u8>>,
}

/// Put one skill back to `bytes` (or remove its folder), keeping what is
/// there now first, and append the line.
fn put_back(
    config_dir: &Path,
    skills_dir: &Path,
    name: &str,
    bytes: Option<Vec<u8>>,
    mut change: Change,
) -> Result<Reverted, String> {
    let path = skill_md(skills_dir, name)?;
    let current = read_skill(&path)?;
    if let Some(c) = &current {
        change.before_hash = keep_version(config_dir, name, c, change.at)?;
    }
    match &bytes {
        Some(b) => {
            change.after_hash = keep_version(config_dir, name, b, change.at)?;
            private_write(&path, b)?;
        }
        None => {
            if let Some(dir) = path.parent() {
                if dir.exists() {
                    fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    let change = append_change(config_dir, change)?;
    Ok(Reverted { change, reverted: None, now: bytes })
}

/// Undo the newest change on one skill that is still in effect: a patch goes
/// back to the bytes it replaced, a created skill is removed (its text stays
/// in history), a removed skill comes back.
pub fn undo_skill_change(
    config_dir: &Path,
    skills_dir: &Path,
    name: &str,
    _ask: UndoAsk,
) -> Result<Reverted, String> {
    let _held = LEDGER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let ledger = ChangeLedger::load(config_dir);
    let target = ledger
        .undo_target(name)
        .cloned()
        .ok_or_else(|| format!("No change to undo on {}", grokhub_core::skill_dir_name(name)))?;
    let bytes = if target.before_hash.is_empty() {
        None
    } else {
        Some(kept_version(config_dir, name, &target.before_hash)?)
    };
    let mut change = new_change(
        name,
        ChangeOp::Undo,
        Origin::User,
        &format!("undo #{} ({})", target.seq, target.reason),
        now_ms(),
    );
    change.undoes = Some(target.seq);
    let mut done = put_back(config_dir, skills_dir, name, bytes, change)?;
    done.reverted = Some(target);
    Ok(done)
}

/// Bring back a skill that is gone (undone create, or moved aside), from the
/// version it had just before it left.
pub fn restore_skill(
    config_dir: &Path,
    skills_dir: &Path,
    name: &str,
    _ask: UndoAsk,
) -> Result<Reverted, String> {
    let _held = LEDGER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = skill_md(skills_dir, name)?;
    if read_skill(&path)?.is_some() {
        return Err(format!(
            "{} is already there. /skills undo steps it back.",
            grokhub_core::skill_dir_name(name)
        ));
    }
    let ledger = ChangeLedger::load(config_dir);
    let source = ledger
        .restore_source(name)
        .cloned()
        .ok_or_else(|| format!("Nothing kept for {}", grokhub_core::skill_dir_name(name)))?;
    let bytes = kept_version(config_dir, name, &source.before_hash)?;
    let change = new_change(
        name,
        ChangeOp::Restore,
        Origin::User,
        &format!("restore from #{}", source.seq),
        now_ms(),
    );
    let mut done = put_back(config_dir, skills_dir, name, Some(bytes), change)?;
    done.reverted = Some(source);
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: &str = "---\nname: weekly-report\n---\n\n# weekly-report\n\n## Steps\nopen the sheet\n";
    const V2: &str = "---\nname: weekly-report\n---\n\n# weekly-report\n\n## Steps\nopen the sheet, then mail it\n";

    fn dirs(label: &str) -> (PathBuf, PathBuf) {
        let root = crate::harness::test_dir(label);
        let skills = root.join("skills");
        fs::create_dir_all(&skills).unwrap();
        (root, skills)
    }

    fn put(skills: &Path, name: &str, body: &str) -> Result<(), String> {
        let dir = skills.join(name);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join("SKILL.md"), body).map_err(|e| e.to_string())
    }

    fn md(skills: &Path, name: &str) -> Option<String> {
        fs::read_to_string(skills.join(name).join("SKILL.md")).ok()
    }

    #[test]
    fn a_patch_keeps_the_prior_version_and_logs_both_hashes() {
        let (root, skills) = dirs("ledger-patch");
        put(&skills, "weekly-report", V1).unwrap();
        let c = record_skill_change(&root, &skills, "weekly-report", Origin::SelfManage, "nightly review patch", || {
            put(&skills, "weekly-report", V2)
        })
        .unwrap()
        .expect("a change");
        assert_eq!(c.seq, 1);
        assert_eq!(c.op, ChangeOp::Modify);
        assert_eq!(c.origin, Origin::SelfManage);
        assert_eq!(c.id, "weekly-report");
        assert_eq!(c.reason, "nightly review patch");
        assert_eq!(c.before_hash, content_hash(V1.as_bytes()));
        assert_eq!(c.after_hash, content_hash(V2.as_bytes()));
        let kept = kept_files(&skill_history_dir(&root, "weekly-report"));
        assert_eq!(kept.len(), 2);
        assert_eq!(fs::read_to_string(&kept[0]).unwrap(), V1);
        assert_eq!(fs::read_to_string(&kept[1]).unwrap(), V2);
        let line = fs::read_to_string(skill_ledger_path(&root)).unwrap();
        assert_eq!(line.lines().count(), 1);
        assert!(line.contains("\"origin\":\"self_manage\""), "{line}");
        assert!(!line.contains("open the sheet"), "no skill text in the ledger: {line}");
        assert_eq!(
            record_skill_change(&root, &skills, "weekly-report", Origin::SelfManage, "same", || Ok(())).unwrap(),
            None,
            "no change, no line"
        );
        assert_eq!(ChangeLedger::load(&root).all().len(), 1);
    }

    #[test]
    fn undo_puts_back_the_exact_bytes_and_walks_back() {
        let (root, skills) = dirs("ledger-undo");
        let v1 = "---\nname: weekly-report\n---\r\nsteps\u{00a0}with odd bytes\n\n\n";
        put(&skills, "weekly-report", v1).unwrap();
        record_skill_change(&root, &skills, "weekly-report", Origin::SelfManage, "p1", || put(&skills, "weekly-report", V1)).unwrap();
        record_skill_change(&root, &skills, "weekly-report", Origin::SelfManage, "p2", || put(&skills, "weekly-report", V2)).unwrap();
        let back = undo_skill_change(&root, &skills, "weekly-report", UndoAsk::from_click()).unwrap();
        assert_eq!(md(&skills, "weekly-report").as_deref(), Some(V1));
        assert_eq!(back.now.as_deref(), Some(V1.as_bytes()));
        assert_eq!(back.change.op, ChangeOp::Undo);
        assert_eq!(back.change.origin, Origin::User);
        assert_eq!(back.change.undoes, Some(2));
        assert_eq!(back.change.reason, "undo #2 (p2)");
        let back = undo_skill_change(&root, &skills, "Weekly Report", UndoAsk::from_typing()).unwrap();
        assert_eq!(back.change.undoes, Some(1));
        assert_eq!(fs::read(skills.join("weekly-report/SKILL.md")).unwrap(), v1.as_bytes());
        assert_eq!(
            undo_skill_change(&root, &skills, "weekly-report", UndoAsk::from_click()).unwrap_err(),
            "No change to undo on weekly-report"
        );
        let ledger = ChangeLedger::load(&root);
        assert!(ledger.undone_by_user("weekly-report", &content_hash(V2.as_bytes())));
        assert!(!ledger.undone_by_user("weekly-report", &content_hash(b"other")));
        assert!(ledger.kept_by_user("weekly-report"));
    }

    #[test]
    fn undoing_a_created_skill_removes_it_and_restore_brings_it_back() {
        let (root, skills) = dirs("ledger-create");
        record_skill_change(&root, &skills, "board-status", Origin::SelfManage, "learned from a host run", || {
            put(&skills, "board-status", V1)?;
            fs::create_dir_all(skills.join("board-status/scripts")).map_err(|e| e.to_string())?;
            fs::write(skills.join("board-status/scripts/verify.sh"), "exit 0\n").map_err(|e| e.to_string())
        })
        .unwrap();
        let first = ChangeLedger::load(&root).all()[0].clone();
        assert_eq!(first.op, ChangeOp::Create);
        assert_eq!(first.before_hash, "");
        let gone = undo_skill_change(&root, &skills, "board-status", UndoAsk::from_click()).unwrap();
        assert_eq!(gone.now, None);
        assert!(!skills.join("board-status").exists(), "the whole folder goes");
        assert_eq!(gone.change.before_hash, content_hash(V1.as_bytes()));
        assert_eq!(gone.change.after_hash, "");
        let kept = kept_files(&skill_history_dir(&root, "board-status"));
        assert_eq!(kept.len(), 1);
        assert_eq!(fs::read_to_string(&kept[0]).unwrap(), V1, "its text stays in history");
        let back = restore_skill(&root, &skills, "board-status", UndoAsk::from_click()).unwrap();
        assert_eq!(back.change.op, ChangeOp::Restore);
        assert_eq!(back.change.reason, "restore from #2");
        assert_eq!(md(&skills, "board-status").as_deref(), Some(V1));
        assert_eq!(
            restore_skill(&root, &skills, "board-status", UndoAsk::from_click()).unwrap_err(),
            "board-status is already there. /skills undo steps it back."
        );
    }

    #[test]
    fn a_deleted_skill_can_be_restored() {
        let (root, skills) = dirs("ledger-delete");
        put(&skills, "old-habit", V1).unwrap();
        let retired = root.join("retired");
        record_skill_change(&root, &skills, "old-habit", Origin::SelfManage, "never ran", || {
            fs::rename(skills.join("old-habit"), &retired).map_err(|e| e.to_string())
        })
        .unwrap();
        assert_eq!(ChangeLedger::load(&root).all()[0].op, ChangeOp::Delete);
        assert_eq!(md(&skills, "old-habit"), None);
        let back = restore_skill(&root, &skills, "old-habit", UndoAsk::from_click()).unwrap();
        assert_eq!(back.now.as_deref(), Some(V1.as_bytes()));
        assert_eq!(md(&skills, "old-habit").as_deref(), Some(V1));
        assert!(ChangeLedger::load(&root).kept_by_user("old-habit"));
        // Undo of the delete itself works the same way.
        let (root, skills) = dirs("ledger-delete-undo");
        put(&skills, "old-habit", V2).unwrap();
        record_skill_change(&root, &skills, "old-habit", Origin::SelfManage, "never ran", || {
            fs::remove_dir_all(skills.join("old-habit")).map_err(|e| e.to_string())
        })
        .unwrap();
        assert!(ChangeLedger::load(&root).restore_source("old-habit").is_some());
        undo_skill_change(&root, &skills, "old-habit", UndoAsk::from_click()).unwrap();
        assert_eq!(md(&skills, "old-habit").as_deref(), Some(V2));
        let ledger = ChangeLedger::load(&root);
        assert_eq!(ledger.restore_source("old-habit"), None, "it is back, so nothing to restore");
        assert_eq!(ledger.undo_target("old-habit"), None);
    }

    #[test]
    fn the_ledger_is_redacted() {
        let (root, skills) = dirs("ledger-redact");
        put(&skills, "deploy", V1).unwrap();
        record_skill_change(
            &root,
            &skills,
            "deploy",
            Origin::SelfManage,
            "nightly review: use sk-abcdefghijklmnopqrstuv and Bearer abcdefghijklmnopqrstuvwxyz",
            || put(&skills, "deploy", V2),
        )
        .unwrap();
        let text = fs::read_to_string(skill_ledger_path(&root)).unwrap();
        assert!(!text.contains("sk-abcdefghijklmnopqrstuv"), "{text}");
        assert!(!text.contains("abcdefghijklmnopqrstuvwxyz"), "{text}");
        assert_eq!(
            ChangeLedger::load(&root).all()[0].reason,
            "nightly review: use [redacted] and [redacted]"
        );
        assert_eq!(change_id("sk-abcdefghijklmnopqrstuv"), "[redacted]");
    }

    #[test]
    fn history_is_capped_per_skill() {
        let (root, skills) = dirs("ledger-cap");
        put(&skills, "notes", "v0\n").unwrap();
        for i in 1..=30 {
            record_skill_change(&root, &skills, "notes", Origin::SelfManage, "patch", || {
                put(&skills, "notes", &format!("v{i}\n"))
            })
            .unwrap();
        }
        let kept = kept_files(&skill_history_dir(&root, "notes"));
        assert_eq!(kept.len(), HISTORY_CAP);
        assert_eq!(fs::read_to_string(kept.last().unwrap()).unwrap(), "v30\n");
        assert_eq!(fs::read_to_string(&kept[0]).unwrap(), "v11\n");
        assert_eq!(ChangeLedger::load(&root).all().len(), 30);
        // Recent versions still undo; one past the cap says so.
        for want in (11..30).rev() {
            undo_skill_change(&root, &skills, "notes", UndoAsk::from_click()).unwrap();
            assert_eq!(md(&skills, "notes").unwrap(), format!("v{want}\n"));
        }
        assert_eq!(
            undo_skill_change(&root, &skills, "notes", UndoAsk::from_click()).unwrap_err(),
            "that version is past the history cap"
        );
    }

    #[test]
    fn the_ledger_keeps_its_newest_lines() {
        let (root, _skills) = dirs("ledger-lines");
        let mut text = String::new();
        for seq in 1..=LEDGER_LINE_CAP as u64 {
            let mut c = new_change("notes", ChangeOp::Modify, Origin::SelfManage, "p", seq);
            c.seq = seq;
            text.push_str(&serde_json::to_string(&c).unwrap());
            text.push('\n');
        }
        fs::create_dir_all(root.join(CHANGES_DIR)).unwrap();
        fs::write(skill_ledger_path(&root), text).unwrap();
        let c = append_change(&root, new_change("notes", ChangeOp::Modify, Origin::SelfManage, "last", 9)).unwrap();
        assert_eq!(c.seq, LEDGER_LINE_CAP as u64 + 1);
        let ledger = ChangeLedger::load(&root);
        assert_eq!(ledger.all().len(), LEDGER_LINE_CAP);
        assert_eq!(ledger.all()[0].seq, 2);
        assert_eq!(ledger.all().last().unwrap().reason, "last");
    }

    #[test]
    fn a_tampered_copy_is_refused() {
        let (root, skills) = dirs("ledger-tamper");
        put(&skills, "notes", V1).unwrap();
        record_skill_change(&root, &skills, "notes", Origin::SelfManage, "p", || put(&skills, "notes", V2)).unwrap();
        let kept = kept_files(&skill_history_dir(&root, "notes"));
        fs::write(&kept[0], "something else").unwrap();
        assert_eq!(
            undo_skill_change(&root, &skills, "notes", UndoAsk::from_click()).unwrap_err(),
            "the kept copy does not match the ledger"
        );
        assert_eq!(md(&skills, "notes").as_deref(), Some(V2), "nothing is touched");
    }
}

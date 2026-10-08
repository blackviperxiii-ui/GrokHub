//! ChangeLedger (harness design §11 Spike-5, §12 P3): skills, connections,
//! automations, and the router's own model files (R2a: the routing table).
//!
//! Every write GrokHub makes on its own to a cabin skill, a connection (an
//! MCP server in the native config), or an automation keeps the version it
//! replaces and appends one line to `{config_dir}/changes/<kind>s.jsonl`:
//! id, time, origin, a short reason, and the SHA-256 of the bytes before and
//! after. The ledger holds no skill text, server entry, or job text, and the
//! id, label, and reason pass through `redact_secrets`. The bytes live beside
//! it in `{config_dir}/changes/<kind>s/<id>/<ms>-<hash12>.<ext>`, at most
//! [`HISTORY_CAP`] files per id, so Undo puts back the exact bytes.
//!
//! Undo, restore, and Keep need an [`UndoAsk`], which only the user's own
//! typing in the composer or a pointer click builds. No model reply, review
//! pass, or automation can make one (source-scanning test in grokhub-app), so
//! the model never undoes, re-applies, or accepts a change on its own. An
//! undone patch is remembered: the nightly review does not write the same
//! bytes again ([`ChangeLedger::undone_by_user`]).
//!
//! The ledger records changes and never approves them: `harness::decide`
//! stays the only gate. [`scope_guard`] refuses any target under harness
//! policy, the consent store, egress, the Access settings, the ledger itself,
//! or `~/.grok`, and logs a `ledger_scope_violation` finding.
//!
//! Not built yet: git-backed history and a text diff per version.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::harness::span::Origin;

/// Folder under the cabin config dir.
pub const CHANGES_DIR: &str = "changes";
/// The skills ledger inside [`CHANGES_DIR`].
pub const SKILL_LEDGER_FILE: &str = "skills.jsonl";
/// The connections ledger inside [`CHANGES_DIR`].
pub const CONNECTION_LEDGER_FILE: &str = "connections.jsonl";
/// The automations ledger inside [`CHANGES_DIR`].
pub const AUTOMATION_LEDGER_FILE: &str = "automations.jsonl";
/// The router's model-file ledger inside [`CHANGES_DIR`] (routing table versions).
pub const MODEL_LEDGER_FILE: &str = "models.jsonl";
/// Scope-guard findings inside [`CHANGES_DIR`].
pub const FINDINGS_FILE: &str = "findings.jsonl";
/// Detector name on a scope-guard finding.
pub const LEDGER_SCOPE_VIOLATION: &str = "ledger_scope_violation";
/// Kept versions per id. The oldest go first.
pub const HISTORY_CAP: usize = 20;
/// Ledger lines kept. Older lines are dropped on the next write.
pub const LEDGER_LINE_CAP: usize = 2000;
/// Largest ledger the cabin reads. A bigger file reads as empty.
const LEDGER_READ_CAP: u64 = 4 * 1024 * 1024;
/// Longest reason kept on a line, in chars.
const REASON_MAX: usize = 160;
/// Longest label kept on a line, in chars.
const LABEL_MAX: usize = 60;
/// Largest `SKILL.md` the ledger copies.
const SKILL_READ_CAP: u64 = 1024 * 1024;
/// Findings kept; older lines are dropped on the next write.
const FINDINGS_CAP: usize = 200;

/// One writer at a time: the nightly pass saves several skills on threads.
static LEDGER_LOCK: Mutex<()> = Mutex::new(());

/// What a ledger line is about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    #[default]
    Skill,
    /// An MCP server entry in the native MCP config.
    Connection,
    /// One job in `automations.json`.
    Automation,
    /// A file the router keeps under `{config}/models/` (the routing table).
    Model,
}

impl ChangeKind {
    pub const ALL: [ChangeKind; 4] = [Self::Skill, Self::Connection, Self::Automation, Self::Model];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Connection => "connection",
            Self::Automation => "automation",
            Self::Model => "model",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == raw)
    }

    pub fn ledger_file(self) -> &'static str {
        match self {
            Self::Skill => SKILL_LEDGER_FILE,
            Self::Connection => CONNECTION_LEDGER_FILE,
            Self::Automation => AUTOMATION_LEDGER_FILE,
            Self::Model => MODEL_LEDGER_FILE,
        }
    }

    fn history_folder(self) -> &'static str {
        match self {
            Self::Skill => "skills",
            Self::Connection => "connections",
            Self::Automation => "automations",
            Self::Model => "models",
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Self::Skill => "md",
            _ => "json",
        }
    }

    /// The ledger id for a name of this kind. Skills use their folder name;
    /// connections and automations keep their own key ([`entry_id`]).
    pub fn id_of(self, name: &str) -> String {
        match self {
            Self::Skill => change_id(name),
            _ => entry_id(name).unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOp {
    Create,
    Modify,
    Delete,
    Undo,
    Restore,
    /// The user kept a change GrokHub made (Work-tree Keep).
    Accept,
}

impl ChangeOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Modify => "modify",
            Self::Delete => "delete",
            Self::Undo => "undo",
            Self::Restore => "restore",
            Self::Accept => "accept",
        }
    }

    /// A line that changed bytes on disk and can be undone.
    fn undoable(self) -> bool {
        !matches!(self, Self::Undo | Self::Accept)
    }
}

/// One ledger line. Hashes are SHA-256 hex of the kept bytes; an empty hash
/// means the target was not there.
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
    /// A short name to show when the id is not one (an automation's title).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

impl Change {
    /// What rows and reports call this target.
    pub fn shown_name(&self) -> &str {
        if self.label.is_empty() {
            &self.id
        } else {
            &self.label
        }
    }
}

/// Proof that the user asked for an undo, restore, or Keep: their own typing
/// in the composer, or a pointer click. Build it only there; a
/// source-scanning test in grokhub-app fails if either constructor shows up
/// anywhere else.
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

/// The ledger id for a connection or automation key. It is also a folder
/// name, so only `A-Z a-z 0-9 . _ -`, at most 64 chars, not starting with a
/// dot, and nothing that reads as a secret.
pub fn entry_id(name: &str) -> Result<String, String> {
    let name = name.trim();
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && grokhub_core::redact_secrets(name) == name;
    if ok {
        Ok(name.to_string())
    } else {
        Err("use a short name: letters, digits, dot, dash, or underscore".into())
    }
}

pub fn ledger_path(config_dir: &Path, kind: ChangeKind) -> PathBuf {
    config_dir.join(CHANGES_DIR).join(kind.ledger_file())
}

pub fn skill_ledger_path(config_dir: &Path) -> PathBuf {
    ledger_path(config_dir, ChangeKind::Skill)
}

/// Where the kept versions of one id live.
pub fn history_dir(config_dir: &Path, kind: ChangeKind, name: &str) -> PathBuf {
    config_dir.join(CHANGES_DIR).join(kind.history_folder()).join(kind.id_of(name))
}

/// Where the kept versions of one skill live.
pub fn skill_history_dir(config_dir: &Path, name: &str) -> PathBuf {
    history_dir(config_dir, ChangeKind::Skill, name)
}

pub fn content_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// One target the ledger can keep versions of: a skill's `SKILL.md`, one
/// server entry in the native MCP config, one job in `automations.json`.
pub trait ChangeTarget {
    fn kind(&self) -> ChangeKind;
    /// The ledger id (also its history folder name).
    fn id(&self) -> Result<String, String>;
    /// The file a write lands in. [`scope_guard`] checks it.
    fn file(&self) -> Result<PathBuf, String>;
    /// The bytes now, or `None` when the target is not there.
    fn read(&self) -> Result<Option<Vec<u8>>, String>;
    /// Put these bytes back, or remove the target.
    fn put(&self, bytes: Option<&[u8]>) -> Result<(), String>;
    /// A short name for rows, read from kept bytes. Empty uses the id.
    fn label_of(&self, _bytes: &[u8]) -> String {
        String::new()
    }
}

/// A cabin skill's `SKILL.md` under `skills_dir`.
pub struct SkillTarget<'a> {
    pub skills_dir: &'a Path,
    pub name: &'a str,
}

impl ChangeTarget for SkillTarget<'_> {
    fn kind(&self) -> ChangeKind {
        ChangeKind::Skill
    }

    fn id(&self) -> Result<String, String> {
        Ok(change_id(self.name))
    }

    fn file(&self) -> Result<PathBuf, String> {
        skill_md(self.skills_dir, self.name)
    }

    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        read_capped(&self.file()?)
    }

    fn put(&self, bytes: Option<&[u8]>) -> Result<(), String> {
        let path = self.file()?;
        match bytes {
            Some(b) => private_write(&path, b),
            None => match path.parent() {
                Some(dir) if dir.exists() => fs::remove_dir_all(dir).map_err(|e| e.to_string()),
                _ => Ok(()),
            },
        }
    }
}

/// The ledger as read from disk, oldest line first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeLedger {
    kind: ChangeKind,
    changes: Vec<Change>,
}

impl ChangeLedger {
    /// The skills ledger. Missing, unreadable, or oversized reads as empty.
    pub fn load(config_dir: &Path) -> Self {
        Self::load_kind(config_dir, ChangeKind::Skill)
    }

    /// One kind's ledger. Missing, unreadable, or oversized reads as empty.
    /// Bad lines and lines of another kind are skipped.
    pub fn load_kind(config_dir: &Path, kind: ChangeKind) -> Self {
        let empty = Self { kind, changes: Vec::new() };
        let Ok(f) = fs::File::open(ledger_path(config_dir, kind)) else {
            return empty;
        };
        if f.metadata().map(|m| m.len() > LEDGER_READ_CAP).unwrap_or(true) {
            return empty;
        }
        let mut text = String::new();
        if f.take(LEDGER_READ_CAP).read_to_string(&mut text).is_err() {
            return empty;
        }
        let changes = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .filter_map(|l| serde_json::from_str::<Change>(l).ok())
            .filter(|c| c.kind == kind.as_str() && !c.id.is_empty())
            .collect();
        Self { kind, changes }
    }

    pub fn kind(&self) -> ChangeKind {
        self.kind
    }

    pub fn all(&self) -> &[Change] {
        &self.changes
    }

    fn for_name<'a>(&'a self, name: &str) -> impl DoubleEndedIterator<Item = &'a Change> + 'a {
        let id = self.kind.id_of(name);
        self.changes.iter().filter(move |c| !id.is_empty() && c.id == id)
    }

    fn was_undone(&self, seq: u64) -> bool {
        self.changes.iter().any(|c| c.op == ChangeOp::Undo && c.undoes == Some(seq))
    }

    /// The change Undo reverts next: the newest one on this id that changed
    /// bytes and was not undone already. Repeated undos walk back one
    /// version at a time.
    pub fn undo_target(&self, name: &str) -> Option<&Change> {
        self.for_name(name)
            .rev()
            .find(|c| c.op.undoable() && !self.was_undone(c.seq))
    }

    /// The change whose `before` a restore brings back: the newest line on
    /// this id that changed bytes, when that line left it gone.
    pub fn restore_source(&self, name: &str) -> Option<&Change> {
        self.for_name(name)
            .rev()
            .find(|c| c.op != ChangeOp::Accept)
            .filter(|c| c.after_hash.is_empty() && !c.before_hash.is_empty())
    }

    /// The user undid a change that produced exactly these bytes. The nightly
    /// review must not write them again on its own.
    pub fn undone_by_user(&self, name: &str, after_hash: &str) -> bool {
        self.for_name(name)
            .any(|c| c.op.undoable() && c.after_hash == after_hash && self.was_undone(c.seq))
    }

    /// The newest line on this id came from the user (an undo or a
    /// restore), so cleanup must leave it alone.
    pub fn kept_by_user(&self, name: &str) -> bool {
        self.for_name(name)
            .next_back()
            .is_some_and(|c| c.origin == Origin::User && matches!(c.op, ChangeOp::Undo | ChangeOp::Restore))
    }

    /// The user kept at least one change GrokHub made (a Keep click).
    pub fn accepted_any(&self) -> bool {
        self.changes.iter().any(|c| c.op == ChangeOp::Accept && c.origin == Origin::User)
    }

    /// The newest self-made change on this id, still in effect and not kept
    /// yet: the one a Work-tree row offers Undo and Keep for.
    pub fn open_self_change(&self, name: &str) -> Option<&Change> {
        let newest = self.for_name(name).next_back()?;
        (newest.origin == Origin::SelfManage && newest.op.undoable() && !self.was_undone(newest.seq)).then_some(newest)
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

/// Read a file the ledger may keep, `None` when it is not there.
pub(crate) fn read_capped(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::File::open(path) {
        Ok(f) => {
            if f.metadata().map(|m| m.len() > SKILL_READ_CAP).unwrap_or(true) {
                return Err("the file is too big to keep a copy".into());
            }
            let mut buf = Vec::new();
            f.take(SKILL_READ_CAP).read_to_end(&mut buf).map_err(|e| e.to_string())?;
            Ok(Some(buf))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Write through a temp file and a rename, owner-only on unix.
pub(crate) fn private_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // One temp name per write, so an Undo and an agent save of the same file
    // cannot rename each other's temp away.
    static NEXT_TMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT_TMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut tmp_name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    tmp_name.push(format!(".{}-{n}.tmp", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
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
    if fs::rename(&tmp, path).is_err() {
        // Windows will not rename over an open or read-only target.
        let _ = fs::remove_file(path);
        fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn kept_files_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == ext))
        .collect();
    files.sort();
    files
}

#[cfg(test)]
fn kept_files(dir: &Path) -> Vec<PathBuf> {
    kept_files_ext(dir, "md")
}

fn find_kept(dir: &Path, ext: &str, hash: &str) -> Option<PathBuf> {
    let tail = format!("-{}.{ext}", hash.get(..12)?);
    kept_files_ext(dir, ext)
        .into_iter()
        .rev()
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(&tail)))
}

/// Keep one version. The same bytes kept before move to the front instead
/// of being copied twice; past [`HISTORY_CAP`] the oldest go. A connection or
/// automation that holds anything that reads as a secret is not kept: the
/// write is refused instead.
fn keep_version(config_dir: &Path, kind: ChangeKind, id: &str, bytes: &[u8], at: u64) -> Result<String, String> {
    if kind != ChangeKind::Skill {
        let text = String::from_utf8_lossy(bytes);
        if grokhub_core::redact_secrets(&text) != text {
            return Err(format!("this {} holds a secret, so GrokHub won't keep or change it", kind.as_str()));
        }
    }
    let hash = content_hash(bytes);
    let dir = config_dir.join(CHANGES_DIR).join(kind.history_folder()).join(id);
    let ext = kind.ext();
    // Names sort oldest first, so a stamp never repeats inside one folder.
    let last = kept_files_ext(&dir, ext)
        .last()
        .and_then(|p| p.file_name()?.to_str()?.get(..16)?.parse::<u64>().ok())
        .unwrap_or(0);
    let stamp = at.max(last + 1);
    let fresh = dir.join(format!("{stamp:016}-{}.{ext}", &hash[..12]));
    match find_kept(&dir, ext, &hash) {
        Some(old) if old != fresh => fs::rename(&old, &fresh).map_err(|e| e.to_string())?,
        Some(_) => {}
        None => private_write(&fresh, bytes)?,
    }
    let files = kept_files_ext(&dir, ext);
    if files.len() > HISTORY_CAP {
        for old in &files[..files.len() - HISTORY_CAP] {
            let _ = fs::remove_file(old);
        }
    }
    Ok(hash)
}

/// The kept bytes for `hash`, checked against the hash.
fn kept_version(config_dir: &Path, kind: ChangeKind, id: &str, hash: &str) -> Result<Vec<u8>, String> {
    let dir = config_dir.join(CHANGES_DIR).join(kind.history_folder()).join(id);
    let path = find_kept(&dir, kind.ext(), hash).ok_or("that version is past the history cap")?;
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    if content_hash(&bytes) != hash {
        return Err("the kept copy does not match the ledger".into());
    }
    Ok(bytes)
}

fn append_change(config_dir: &Path, mut change: Change) -> Result<Change, String> {
    let kind = ChangeKind::parse(&change.kind).ok_or("unknown change kind")?;
    let ledger = ChangeLedger::load_kind(config_dir, kind);
    change.seq = ledger.changes.last().map(|c| c.seq + 1).unwrap_or(1);
    change.reason = grokhub_core::redact_secrets(&change.reason.chars().take(REASON_MAX).collect::<String>());
    change.label = grokhub_core::redact_secrets(change.label.lines().next().unwrap_or("").trim())
        .chars()
        .take(LABEL_MAX)
        .collect();
    let line = grokhub_core::redact_secrets(&serde_json::to_string(&change).map_err(|e| e.to_string())?);
    let path = ledger_path(config_dir, kind);
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
    append_line(&path, &line)?;
    Ok(change)
}

fn append_line(path: &Path, line: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
}

fn new_change_for(kind: ChangeKind, id: &str, op: ChangeOp, origin: Origin, reason: &str, at: u64) -> Change {
    Change {
        seq: 0,
        at,
        kind: kind.as_str().into(),
        id: id.to_string(),
        op,
        origin,
        reason: reason.trim().to_string(),
        before_hash: String::new(),
        after_hash: String::new(),
        undoes: None,
        label: String::new(),
    }
}

#[cfg(test)]
fn new_change(name: &str, op: ChangeOp, origin: Origin, reason: &str, at: u64) -> Change {
    new_change_for(ChangeKind::Skill, &change_id(name), op, origin, reason, at)
}

/// A self-made change the cabin has not shown yet (Work-tree row, Home update).
#[derive(Debug, Clone)]
struct Fresh {
    config_dir: PathBuf,
    kind: ChangeKind,
    change: Change,
}

static FRESH: Mutex<Vec<Fresh>> = Mutex::new(Vec::new());

/// Drain the self-made changes written under `config_dir` since the last
/// call (polled by the cabin each frame). Lines from another config dir stay.
pub fn take_self_changes(config_dir: &Path) -> Vec<(ChangeKind, Change)> {
    let mut held = FRESH.lock().unwrap_or_else(|p| p.into_inner());
    let (mine, rest): (Vec<Fresh>, Vec<Fresh>) = held.drain(..).partition(|f| f.config_dir == config_dir);
    *held = rest;
    mine.into_iter().map(|f| (f.kind, f.change)).collect()
}

fn path_key(p: &Path) -> Vec<String> {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

/// Files under the cabin config dir the ledger never targets, with why.
const GUARDED_FILES: &[(&str, &str)] = &[
    ("consent.jsonl", "the consent store"),
    ("egress.jsonl", "the egress log"),
    ("egress.1.jsonl", "the egress log"),
    (crate::harness::at_rest::KEY_ID_FILE, "the private-data key"),
    ("app.json", "the Access settings"),
];

/// Folders under the cabin config dir the ledger never targets, with why.
const GUARDED_DIRS: &[(&str, &str)] = &[
    ("harness", "harness policy"),
    (CHANGES_DIR, "the change ledger itself"),
];

/// Refuse a target the agent must never change on its own: harness policy
/// (the hard-class list, parks, turn state), the consent store, egress, the
/// Access settings, the ledger itself, and anything under a `.grok` folder
/// (D1: never `~/.grok`).
pub fn scope_guard(config_dir: &Path, file: &Path) -> Result<(), String> {
    let parts = path_key(file);
    if parts.iter().any(|p| p == ".grok") {
        return Err("the Grok CLI home (~/.grok) is not GrokHub's to change".into());
    }
    let name = parts.last().map(String::as_str).unwrap_or("");
    if let Some((_, why)) = GUARDED_FILES.iter().find(|(f, _)| *f == name) {
        return Err(format!("GrokHub never changes {why} on its own"));
    }
    let root = path_key(config_dir);
    if parts.len() > root.len() && parts[..root.len()] == root[..] {
        let first = parts[root.len()].as_str();
        if let Some((_, why)) = GUARDED_DIRS.iter().find(|(d, _)| *d == first) {
            return Err(format!("GrokHub never changes {why} on its own"));
        }
    }
    Ok(())
}

/// One refused ledger write, `approval_gate_violation`-style: what was
/// targeted (file name only) and why. No content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeFinding {
    pub at: u64,
    pub detector: String,
    pub kind: String,
    pub id: String,
    pub target: String,
    pub detail: String,
}

pub fn findings_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CHANGES_DIR).join(FINDINGS_FILE)
}

/// Every scope-guard finding on disk, oldest first.
pub fn read_scope_findings(config_dir: &Path) -> Vec<ScopeFinding> {
    let text = read_capped(&findings_path(config_dir)).ok().flatten().unwrap_or_default();
    String::from_utf8_lossy(&text)
        .lines()
        .filter_map(|l| serde_json::from_str(l.trim()).ok())
        .collect()
}

fn note_scope_finding(config_dir: &Path, kind: ChangeKind, id: &str, file: &Path, why: &str) {
    let finding = ScopeFinding {
        at: now_ms(),
        detector: LEDGER_SCOPE_VIOLATION.into(),
        kind: kind.as_str().into(),
        id: grokhub_core::redact_secrets(id),
        target: grokhub_core::redact_secrets(&file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
        detail: grokhub_core::redact_secrets(why),
    };
    let mut all = read_scope_findings(config_dir);
    all.push(finding);
    let start = all.len().saturating_sub(FINDINGS_CAP);
    let mut text = String::new();
    for f in &all[start..] {
        if let Ok(line) = serde_json::to_string(f) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    let _ = private_write(&findings_path(config_dir), text.as_bytes());
}

/// A weekly self-review proposal the scope guard refused (Spike-7): logged
/// like a refused ledger write, with the target as the model named it.
pub fn note_proposal_finding(config_dir: &Path, target: &str, why: &str) {
    note_scope_finding(config_dir, ChangeKind::Skill, "", Path::new(target), why);
}

/// Run the scope guard for a target; a refusal is logged as a finding.
fn guard(config_dir: &Path, target: &dyn ChangeTarget) -> Result<PathBuf, String> {
    let file = target.file()?;
    if let Err(why) = scope_guard(config_dir, &file) {
        let id = target.id().unwrap_or_default();
        note_scope_finding(config_dir, target.kind(), &id, &file, &why);
        return Err(format!("Refused: {why}."));
    }
    Ok(file)
}

/// Run `write` on one target with the ledger around it: the scope guard
/// first, then the current bytes are kept, `write` runs, the new bytes are
/// kept, and one line is appended. `Ok(None)` when the bytes did not change.
/// `write` may create, change, or remove the target. A self-made line is
/// queued for the cabin's Work-tree row and Home update.
pub fn record_change(
    config_dir: &Path,
    target: &dyn ChangeTarget,
    origin: Origin,
    reason: &str,
    write: impl FnOnce() -> Result<(), String>,
) -> Result<Option<Change>, String> {
    guard(config_dir, target)?;
    let _held = LEDGER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let kind = target.kind();
    let id = target.id()?;
    let at = now_ms();
    let before = target.read()?;
    let before_hash = match &before {
        Some(b) => keep_version(config_dir, kind, &id, b, at)?,
        None => String::new(),
    };
    write()?;
    let after = target.read()?;
    if after == before {
        return Ok(None);
    }
    let after_hash = match &after {
        Some(b) => match keep_version(config_dir, kind, &id, b, at) {
            Ok(hash) => hash,
            Err(why) => {
                // No version, no write: put the old bytes back.
                let _ = target.put(before.as_deref());
                return Err(why);
            }
        },
        None => String::new(),
    };
    let op = match (&before, &after) {
        (None, _) => ChangeOp::Create,
        (_, None) => ChangeOp::Delete,
        _ => ChangeOp::Modify,
    };
    let mut change = new_change_for(kind, &id, op, origin, reason, at);
    change.before_hash = before_hash;
    change.after_hash = after_hash;
    if let Some(b) = after.as_ref().or(before.as_ref()) {
        change.label = target.label_of(b);
    }
    let change = append_change(config_dir, change)?;
    if origin == Origin::SelfManage {
        FRESH.lock().unwrap_or_else(|p| p.into_inner()).push(Fresh {
            config_dir: config_dir.to_path_buf(),
            kind,
            change: change.clone(),
        });
    }
    Ok(Some(change))
}

/// [`record_change`] on one skill's `SKILL.md`.
pub fn record_skill_change(
    config_dir: &Path,
    skills_dir: &Path,
    name: &str,
    origin: Origin,
    reason: &str,
    write: impl FnOnce() -> Result<(), String>,
) -> Result<Option<Change>, String> {
    record_change(config_dir, &SkillTarget { skills_dir, name }, origin, reason, write)
}

/// What an undo or restore left on disk: the line it wrote and the bytes now
/// in place (`None` when the target is gone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reverted {
    pub change: Change,
    pub reverted: Option<Change>,
    pub now: Option<Vec<u8>>,
}

/// Put one target back to `bytes` (or remove it), keeping what is there now
/// first, and append the line.
fn put_back(
    config_dir: &Path,
    target: &dyn ChangeTarget,
    bytes: Option<Vec<u8>>,
    mut change: Change,
) -> Result<Reverted, String> {
    let (kind, id) = (target.kind(), target.id()?);
    let current = target.read()?;
    if let Some(c) = &current {
        change.before_hash = keep_version(config_dir, kind, &id, c, change.at)?;
    }
    if let Some(b) = &bytes {
        change.after_hash = keep_version(config_dir, kind, &id, b, change.at)?;
    }
    target.put(bytes.as_deref())?;
    if let Some(b) = bytes.as_ref().or(current.as_ref()) {
        change.label = target.label_of(b);
    }
    let change = append_change(config_dir, change)?;
    Ok(Reverted { change, reverted: None, now: bytes })
}

/// What a message calls a target: a skill's folder name, else its key.
fn shown_id(target: &dyn ChangeTarget, id: &str) -> String {
    match target.kind() {
        ChangeKind::Skill => grokhub_core::skill_dir_name(id),
        _ => id.to_string(),
    }
}

/// Undo the newest change on one target that is still in effect: a patch
/// goes back to the bytes it replaced, a created target is removed (its bytes
/// stay in history), a removed one comes back.
pub fn undo_change(config_dir: &Path, target: &dyn ChangeTarget, _ask: UndoAsk) -> Result<Reverted, String> {
    guard(config_dir, target)?;
    let _held = LEDGER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (kind, id) = (target.kind(), target.id()?);
    let ledger = ChangeLedger::load_kind(config_dir, kind);
    let target_line = ledger
        .undo_target(&id)
        .cloned()
        .ok_or_else(|| format!("No change to undo on {}", shown_id(target, &id)))?;
    let bytes = if target_line.before_hash.is_empty() {
        None
    } else {
        Some(kept_version(config_dir, kind, &id, &target_line.before_hash)?)
    };
    let mut change = new_change_for(
        kind,
        &id,
        ChangeOp::Undo,
        Origin::User,
        &format!("undo #{} ({})", target_line.seq, target_line.reason),
        now_ms(),
    );
    change.undoes = Some(target_line.seq);
    let mut done = put_back(config_dir, target, bytes, change)?;
    done.reverted = Some(target_line);
    Ok(done)
}

/// Bring back a target that is gone (undone create, or removed), from the
/// version it had just before it left.
pub fn restore_change(config_dir: &Path, target: &dyn ChangeTarget, _ask: UndoAsk) -> Result<Reverted, String> {
    guard(config_dir, target)?;
    let _held = LEDGER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (kind, id) = (target.kind(), target.id()?);
    if target.read()?.is_some() {
        return Err(match kind {
            ChangeKind::Skill => format!("{} is already there. /skills undo steps it back.", shown_id(target, &id)),
            _ => format!("{id} is already there. Undo steps it back."),
        });
    }
    let ledger = ChangeLedger::load_kind(config_dir, kind);
    let source = ledger
        .restore_source(&id)
        .cloned()
        .ok_or_else(|| format!("Nothing kept for {}", shown_id(target, &id)))?;
    let bytes = kept_version(config_dir, kind, &id, &source.before_hash)?;
    let change = new_change_for(kind, &id, ChangeOp::Restore, Origin::User, &format!("restore from #{}", source.seq), now_ms());
    let mut done = put_back(config_dir, target, Some(bytes), change)?;
    done.reverted = Some(source);
    Ok(done)
}

/// Keep a change GrokHub made: one `accept` line by the user. Nothing on
/// disk changes; it lifts the new-automation cap and clears the row.
pub fn accept_change(config_dir: &Path, kind: ChangeKind, name: &str, _ask: UndoAsk) -> Result<Change, String> {
    let _held = LEDGER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let ledger = ChangeLedger::load_kind(config_dir, kind);
    let open = ledger
        .open_self_change(name)
        .cloned()
        .ok_or_else(|| format!("No change by GrokHub to keep on {}", kind.id_of(name)))?;
    let mut change = new_change_for(kind, &open.id, ChangeOp::Accept, Origin::User, &format!("kept #{}", open.seq), now_ms());
    change.label = open.label.clone();
    change.before_hash = open.after_hash.clone();
    change.after_hash = open.after_hash;
    append_change(config_dir, change)
}

/// Undo the newest change on one skill that is still in effect.
pub fn undo_skill_change(config_dir: &Path, skills_dir: &Path, name: &str, ask: UndoAsk) -> Result<Reverted, String> {
    undo_change(config_dir, &SkillTarget { skills_dir, name }, ask)
}

/// Bring back a skill that is gone (undone create, or moved aside), from the
/// version it had just before it left.
pub fn restore_skill(config_dir: &Path, skills_dir: &Path, name: &str, ask: UndoAsk) -> Result<Reverted, String> {
    restore_change(config_dir, &SkillTarget { skills_dir, name }, ask)
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

//! Code tab Settings panel: per-project toggles for local-only nonstop coding
//! and pre-action backups before risky actions.
//!
//! Both toggles are off by default and saved per project in the app config
//! folder (never inside the user's repo).
//!
//! Safety invariants enforced unconditionally:
//! - Checkpoints never commit to `main` or the repo's detected default branch
//!   and never force-push.
//! - Backups are never auto-deleted; exceeding the retention threshold only
//!   surfaces a warning.
//! - Hard-class and dangerous commands still ask even when both toggles and
//!   Allow always are on.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};

use grokhub_core::proc_util::hide_windows_console;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gate::Decision;
use crate::stall::Did;

/// Per-project toggle store in `{config_dir}`.
pub const SETTINGS_FILE: &str = "code-panel-settings.json";
/// Per-project backup index in `{config_dir}`.
pub const BACKUPS_FILE: &str = "code-panel-backups.json";
/// Folder inside a non-git workspace holding pre-action copies.
pub const BACKUP_DIR_NAME: &str = ".loadout-backups";
/// Default consecutive identical failures before nonstop coding stops.
pub const DEFAULT_MAX_REPEATED_FAILURES: u32 = 3;
/// Default minutes without progress before nonstop coding stops.
pub const DEFAULT_STALL_TIMEOUT_MINS: u32 = 15;
/// Default backup count above which the panel shows a retention warning.
pub const DEFAULT_BACKUP_RETENTION_WARN: usize = 20;
/// Distinct files edited in one turn that count as a mass edit.
pub const MASS_EDIT_FILE_THRESHOLD: usize = 3;
/// Always-protected branch name: never checkpoint-committed.
pub const ALWAYS_PROTECTED_BRANCH: &str = "main";
/// Max file bytes copied into a folder backup entry (16 MB).
const MAX_COPY_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Max files copied into one folder backup entry.
const MAX_COPY_FILES: usize = 512;

/// Directories skipped when copying a non-git workspace into `.loadout-backups/`.
const SKIP_COPY_DIRS: &[&str] = &[
    BACKUP_DIR_NAME,
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
];

/// Shell tools that accept a `command` argument.
const SHELL_TOOLS: &[&str] = &["run_terminal_command", "monitor"];

/// Shell commands that delete files or directories.
const DELETE_COMMANDS: &[&str] = &["rm", "rmdir", "unlink", "del", "erase", "remove-item", "ri"];

/// Shell commands that overwrite files.
const OVERWRITE_COMMANDS: &[&str] = &[
    "truncate",
    "mv",
    "move",
    "move-item",
    "mi",
    "cp",
    "copy",
    "copy-item",
    "cpi",
    "set-content",
    "out-file",
    "clear-content",
    "clc",
];

/// Package managers and their mutating subcommands.
const PACKAGE_INSTALLERS: &[(&str, &[&str])] = &[
    (
        "npm",
        &[
            "install",
            "i",
            "ci",
            "add",
            "remove",
            "uninstall",
            "rm",
            "update",
            "up",
        ],
    ),
    (
        "pnpm",
        &[
            "install",
            "i",
            "add",
            "remove",
            "uninstall",
            "rm",
            "update",
            "up",
        ],
    ),
    ("yarn", &["install", "add", "remove", "upgrade"]),
    ("bun", &["install", "i", "add", "remove", "update"]),
    ("cargo", &["add", "remove", "rm", "update", "install"]),
    ("pip", &["install", "uninstall"]),
    ("pip3", &["install", "uninstall"]),
    ("uv", &["add", "remove", "sync", "pip"]),
    ("brew", &["install", "uninstall", "upgrade", "reinstall"]),
    ("apt", &["install", "remove", "purge", "upgrade"]),
    ("apt-get", &["install", "remove", "purge", "upgrade"]),
    ("winget", &["install", "uninstall", "upgrade"]),
];

/// Per-project Code tab panel settings. Both toggles default to `false`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PanelSettings {
    /// Route Code turns to local models only, work through the task list
    /// without waiting, checkpoint before risky steps, and enforce loop/stall
    /// guards.
    pub local_only_nonstop: bool,
    /// When Allow always is on, back up before risky-looking actions and
    /// continue without prompting (hard-class and dangerous commands still ask).
    pub backup_before_risky: bool,
    /// Stop nonstop coding after this many repeated failures of the same step.
    pub max_repeated_failures: u32,
    /// Stop nonstop coding after this many minutes with no real progress.
    pub stall_timeout_mins: u32,
    /// Warn (never auto-delete) when backup count exceeds this threshold.
    pub backup_retention_warn: usize,
}

impl Default for PanelSettings {
    fn default() -> Self {
        Self {
            local_only_nonstop: false,
            backup_before_risky: false,
            max_repeated_failures: DEFAULT_MAX_REPEATED_FAILURES,
            stall_timeout_mins: DEFAULT_STALL_TIMEOUT_MINS,
            backup_retention_warn: DEFAULT_BACKUP_RETENTION_WARN,
        }
    }
}

impl PanelSettings {
    /// Clamp numeric thresholds into safe bounds.
    pub fn clamped(mut self) -> Self {
        self.max_repeated_failures = self.max_repeated_failures.clamp(1, 20);
        self.stall_timeout_mins = self.stall_timeout_mins.clamp(1, 240);
        self.backup_retention_warn = self.backup_retention_warn.clamp(1, 500);
        self
    }
}

/// Risky action categories that trigger a pre-action backup (AC-5) or
/// pre-step checkpoint (AC-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskyActionKind {
    Delete,
    Overwrite,
    MassEdit,
    GitDiscard,
    PackageInstall,
}

impl RiskyActionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Overwrite => "overwrite",
            Self::MassEdit => "mass edit",
            Self::GitDiscard => "git working-tree reset/clean/checkout",
            Self::PackageInstall => "package install",
        }
    }
}

/// How a backup was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupStrategy {
    /// Snapshot commit on a safe work branch (never main or default branch).
    GitCommit,
    /// Non-destructive git stash commit SHA (working tree kept intact).
    GitStash,
    /// Git ref pinned to HEAD in a clean repo before a risky action.
    GitHeadRef,
    /// File copy under `<workspace>/.loadout-backups/<timestamp>`.
    FolderCopy,
}

/// One saved backup entry for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub id: String,
    pub created_at_ms: u64,
    pub label: String,
    pub action_kind: RiskyActionKind,
    pub strategy: BackupStrategy,
    /// Commit SHA, stash SHA, or relative `.loadout-backups/<id>` path.
    pub ref_value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub files_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored_at_ms: Option<u64>,
}

/// Backup list + retention warning for one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupList {
    pub workspace: String,
    pub backups: Vec<BackupEntry>,
    pub retention_warn_limit: usize,
    pub retention_warning: Option<String>,
}

/// Full panel state returned to the Code tab's Settings panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelState {
    pub workspace: String,
    pub is_repo: bool,
    pub branch: Option<String>,
    pub settings: PanelSettings,
    pub backups: Vec<BackupEntry>,
    pub retention_warn_limit: usize,
    pub retention_warning: Option<String>,
}

/// Result of a one-click backup restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub id: String,
    pub summary: String,
    pub restored_files: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsStore {
    #[serde(default)]
    entries: Vec<NamedSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NamedSettings {
    project_key: String,
    #[serde(flatten)]
    settings: PanelSettings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupsStore {
    #[serde(default)]
    projects: Vec<ProjectBackups>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectBackups {
    project_key: String,
    #[serde(default)]
    backups: Vec<BackupEntry>,
}

static SETTINGS_LOCK: Mutex<()> = Mutex::new(());
static BACKUPS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone)]
struct BoundSession {
    config_dir: PathBuf,
    workspace: PathBuf,
}

fn sessions() -> &'static Mutex<HashMap<String, BoundSession>> {
    static MAP: OnceLock<Mutex<HashMap<String, BoundSession>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

thread_local! {
    static TEST_SESSIONS: RefCell<Option<HashMap<String, BoundSession>>> = const { RefCell::new(None) };
}

/// RAII test guard isolating bound Code tab sessions on the current thread.
#[cfg(test)]
pub struct TestSessionScope;

#[cfg(test)]
impl TestSessionScope {
    pub fn enter() -> Self {
        TEST_SESSIONS.with(|slot| {
            *slot.borrow_mut() = Some(HashMap::new());
        });
        Self
    }
}

#[cfg(test)]
impl Drop for TestSessionScope {
    fn drop(&mut self) {
        TEST_SESSIONS.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

/// Stable normalized key for a project folder (works for both git repos and
/// plain folders).
pub fn project_key(workspace: &Path) -> String {
    let raw = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let mut s = raw.to_string_lossy().replace('\\', "/");
    if let Some(stripped) = s.strip_prefix("//?/") {
        s = stripped.to_string();
    }
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    if cfg!(windows) {
        s.to_ascii_lowercase()
    } else {
        s
    }
}

/// Bind a Code tab conversation to its project folder so routing, guards,
/// checkpoints, and pre-action backups read the project's live panel settings.
/// Pass `workspace = None` to unbind.
pub fn bind_session(conversation_id: &str, config_dir: &Path, workspace: Option<&Path>) {
    if conversation_id.is_empty() {
        return;
    }
    let handled_in_test = TEST_SESSIONS.with(|slot| {
        let mut borrow = slot.borrow_mut();
        let Some(map) = borrow.as_mut() else {
            return false;
        };
        match workspace {
            Some(ws) if !ws.as_os_str().is_empty() => {
                map.insert(
                    conversation_id.to_string(),
                    BoundSession {
                        config_dir: config_dir.to_path_buf(),
                        workspace: ws.to_path_buf(),
                    },
                );
            }
            _ => {
                map.remove(conversation_id);
            }
        }
        true
    });
    if handled_in_test {
        return;
    }
    let mut map = sessions().lock().unwrap_or_else(|e| e.into_inner());
    match workspace {
        Some(ws) if !ws.as_os_str().is_empty() => {
            map.insert(
                conversation_id.to_string(),
                BoundSession {
                    config_dir: config_dir.to_path_buf(),
                    workspace: ws.to_path_buf(),
                },
            );
        }
        _ => {
            map.remove(conversation_id);
        }
    }
}

fn find_session(conversation_id: &str) -> Option<BoundSession> {
    if conversation_id.is_empty() {
        return None;
    }
    let test_hit = TEST_SESSIONS.with(|slot| {
        let borrow = slot.borrow();
        let map = borrow.as_ref()?;
        let mut id = conversation_id;
        loop {
            if let Some(found) = map.get(id) {
                return Some(Some(found.clone()));
            }
            match id.rsplit_once('/') {
                Some((parent, _)) => id = parent,
                None => return Some(None),
            }
        }
    });
    if let Some(hit) = test_hit {
        return hit;
    }
    let map = sessions().lock().unwrap_or_else(|e| e.into_inner());
    let mut id = conversation_id;
    loop {
        if let Some(found) = map.get(id) {
            return Some(found.clone());
        }
        id = id.rsplit_once('/')?.0;
    }
}

/// Live panel settings for a bound Code tab session (or its parent session).
pub fn session_settings(conversation_id: &str) -> Option<(PathBuf, PathBuf, PanelSettings)> {
    let bound = find_session(conversation_id)?;
    let cfg = load_settings(&bound.config_dir, &bound.workspace);
    Some((bound.config_dir, bound.workspace, cfg))
}

/// True when `conversation_id` belongs to a Code tab project with
/// "Local models only and nonstop coding" enabled.
pub fn session_local_only(conversation_id: &str) -> bool {
    session_settings(conversation_id).is_some_and(|(_, _, s)| s.local_only_nonstop)
}

/// Load a project's panel settings. Missing files or projects return defaults
/// (both toggles off).
pub fn load_settings(config_dir: &Path, workspace: &Path) -> PanelSettings {
    let key = project_key(workspace);
    let store = read_settings_store(config_dir);
    store
        .entries
        .into_iter()
        .find(|e| e.project_key == key)
        .map(|e| e.settings.clamped())
        .unwrap_or_default()
}

/// Save a project's panel settings atomically in `{config_dir}`.
pub fn save_settings(
    config_dir: &Path,
    workspace: &Path,
    settings: PanelSettings,
) -> Result<PanelSettings, String> {
    let next = settings.clamped();
    let key = project_key(workspace);
    let _guard = SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut store = read_settings_store(config_dir);
    store.entries.retain(|e| e.project_key != key);
    store.entries.push(NamedSettings {
        project_key: key,
        settings: next.clone(),
    });
    write_json_atomic(config_dir, SETTINGS_FILE, &store)?;
    Ok(next)
}

fn read_settings_store(config_dir: &Path) -> SettingsStore {
    fs::read_to_string(config_dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn read_backups_store(config_dir: &Path) -> BackupsStore {
    fs::read_to_string(config_dir.join(BACKUPS_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_json_atomic<T: Serialize>(
    config_dir: &Path,
    file_name: &str,
    value: &T,
) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{file_name}.{}.tmp", std::process::id()));
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    let dest = config_dir.join(file_name);
    if fs::rename(&tmp, &dest).is_err() {
        let _ = fs::remove_file(&dest);
        fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Build the full state snapshot for the Code tab's Settings panel.
pub fn panel_state(config_dir: &Path, workspace: &Path) -> PanelState {
    let settings = load_settings(config_dir, workspace);
    let list = list_backups(config_dir, workspace);
    let is_repo = is_git_repo(workspace);
    let branch = if is_repo {
        current_branch(workspace)
    } else {
        None
    };
    PanelState {
        workspace: workspace.to_string_lossy().into_owned(),
        is_repo,
        branch,
        settings,
        backups: list.backups,
        retention_warn_limit: list.retention_warn_limit,
        retention_warning: list.retention_warning,
    }
}

/// List saved backups for `workspace` (newest first) and compute any retention
/// warning without deleting anything (AC-6).
pub fn list_backups(config_dir: &Path, workspace: &Path) -> BackupList {
    let settings = load_settings(config_dir, workspace);
    let key = project_key(workspace);
    let store = read_backups_store(config_dir);
    let mut backups = store
        .projects
        .into_iter()
        .find(|p| p.project_key == key)
        .map(|p| p.backups)
        .unwrap_or_default();
    backups.sort_by_key(|b| std::cmp::Reverse(b.created_at_ms));
    let retention_warning = retention_warning_for(backups.len(), settings.backup_retention_warn);
    BackupList {
        workspace: workspace.to_string_lossy().into_owned(),
        backups,
        retention_warn_limit: settings.backup_retention_warn,
        retention_warning,
    }
}

/// Format the retention warning when `count > limit`. Never deletes anything.
pub fn retention_warning_for(count: usize, limit: usize) -> Option<String> {
    if count > limit {
        Some(format!(
            "You have {count} saved backups (warning threshold: {limit}). Old backups are never auto-deleted."
        ))
    } else {
        None
    }
}

// ---- AC-7: Hard-class and dangerous commands still ask ----

/// True when `name`/`arguments` is hard-class or a dangerous/unsplittable
/// shell command that must still prompt even when both Code tab toggles and
/// Allow always are enabled (AC-7).
pub fn is_hard_or_dangerous(name: &str, arguments: &str, workspace: &Path) -> bool {
    let hard = grokhub_core::host_safety::in_workspace(workspace, || {
        !crate::harness::decide(crate::harness::Step::Tool { name, arguments }).is_allow()
    });
    if hard {
        return true;
    }
    if SHELL_TOOLS.contains(&name) {
        if let Some(cmd) = command_arg(arguments) {
            let facts = crate::perm::analyze(&cmd);
            if facts.dangerous || facts.segments.is_none() {
                return true;
            }
        }
    }
    false
}

/// Enforcement hook called from [`crate::code_guard::apply`]: when either Code
/// tab Settings toggle is on for `session`, hard-class and dangerous commands
/// always ask (never downgraded to `Run` by Allow always or either toggle).
pub fn enforce_hard_and_dangerous(
    session: &str,
    name: &str,
    arguments: &str,
    workspace: &Path,
    base: Decision,
) -> Decision {
    if matches!(base, Decision::Refuse(_)) {
        return base;
    }
    let Some((_, _, settings)) = session_settings(session) else {
        return base;
    };
    if !settings.local_only_nonstop && !settings.backup_before_risky {
        return base;
    }
    if is_hard_or_dangerous(name, arguments, workspace) {
        Decision::Ask
    } else {
        base
    }
}

// ---- AC-5 / AC-3: Risky action classification ----

fn command_arg(arguments: &str) -> Option<String> {
    let v: Value = serde_json::from_str(arguments).ok()?;
    let cmd = v.get("command")?.as_str()?.trim();
    if cmd.is_empty() {
        None
    } else {
        Some(cmd.to_string())
    }
}

fn path_arg(name: &str, arguments: &str) -> Option<String> {
    let v: Value = serde_json::from_str(arguments).ok()?;
    let keys: &[&str] = match name {
        "write" => &["path", "file_path", "target_file"],
        "search_replace" => &["file_path", "path", "target_file"],
        _ => &["path", "file_path", "target_file"],
    };
    for key in keys {
        if let Some(p) = v.get(*key).and_then(Value::as_str).map(str::trim) {
            if !p.is_empty() {
                return Some(p.to_string());
            }
        }
    }
    None
}

fn normalize_program(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let lower = base.to_ascii_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

/// Classify whether a tool call is a risky-looking action that needs a backup
/// (AC-5) or checkpoint (AC-3) before executing.
pub fn classify_risky_action(
    name: &str,
    arguments: &str,
    workspace: &Path,
    edited_files_in_turn: &HashSet<String>,
) -> Option<RiskyActionKind> {
    let lower_name = name.to_ascii_lowercase();
    if lower_name == "delete_files"
        || lower_name.contains("delete")
        || lower_name.contains("remove_file")
    {
        return Some(RiskyActionKind::Delete);
    }

    if name == "write" || name == "search_replace" {
        if let Some(raw_path) = path_arg(name, arguments) {
            let resolved = resolve_in_workspace(workspace, &raw_path);
            let norm_key = resolved.to_string_lossy().replace('\\', "/");
            let next_count = if edited_files_in_turn.contains(&norm_key) {
                edited_files_in_turn.len()
            } else {
                edited_files_in_turn.len() + 1
            };
            if next_count >= MASS_EDIT_FILE_THRESHOLD {
                return Some(RiskyActionKind::MassEdit);
            }
            if name == "write" && resolved.is_file() {
                return Some(RiskyActionKind::Overwrite);
            }
        }
        return None;
    }

    if SHELL_TOOLS.contains(&name) {
        let cmd = command_arg(arguments)?;
        return classify_shell_command(&cmd, workspace);
    }

    None
}

/// Classify a shell command for risky actions (AC-5).
pub fn classify_shell_command(command: &str, workspace: &Path) -> Option<RiskyActionKind> {
    let facts = crate::perm::analyze(command);
    if let Some(segs) = &facts.segments {
        for seg in segs {
            if let Some(kind) = classify_words(&seg.words, workspace) {
                return Some(kind);
            }
        }
    }
    // Also inspect simple whitespace-split tokens for commands that contain
    // redirects or compound shell forms `perm::analyze` left unsplittable.
    let trimmed = command.trim();
    if crate::shell_redirect::redirects(trimmed)
        .iter()
        .any(|r| r.file.is_some())
    {
        return Some(RiskyActionKind::Overwrite);
    }
    let words: Vec<String> = trimmed
        .split_whitespace()
        .map(|w| w.trim_matches(['"', '\'', ';', '(', ')']).to_string())
        .filter(|w| !w.is_empty())
        .collect();
    classify_words(&words, workspace)
}

fn classify_words(words: &[String], workspace: &Path) -> Option<RiskyActionKind> {
    let mut idx = 0usize;
    while idx < words.len() && words[idx].contains('=') && !words[idx].starts_with('-') {
        idx += 1;
    }
    let head = words.get(idx)?;
    let prog = normalize_program(head);
    let rest = &words[idx + 1..];

    // 1. Delete commands
    if DELETE_COMMANDS.contains(&prog.as_str()) {
        return Some(RiskyActionKind::Delete);
    }
    if prog == "find" && rest.iter().any(|w| w == "-delete") {
        return Some(RiskyActionKind::Delete);
    }

    // 2. Git destructive working-tree commands & git rm
    if prog == "git" {
        let sub = git_subcommand(rest);
        match sub.as_deref() {
            Some("rm") => return Some(RiskyActionKind::Delete),
            Some("reset") | Some("clean") => return Some(RiskyActionKind::GitDiscard),
            Some("checkout") | Some("restore") => {
                let explicit_discard = rest
                    .iter()
                    .any(|w| w == "--" || w == "-f" || w == "--force");
                if explicit_discard || has_uncommitted_changes(workspace) {
                    return Some(RiskyActionKind::GitDiscard);
                }
            }
            _ => {}
        }
    }

    // 3. Package installs
    for (mgr, subs) in PACKAGE_INSTALLERS {
        if prog == *mgr {
            if let Some(sub) = rest.first().map(|s| s.to_ascii_lowercase()) {
                if subs.contains(&sub.as_str()) {
                    return Some(RiskyActionKind::PackageInstall);
                }
            }
        }
    }

    // 4. Mass in-place edits
    if matches!(prog.as_str(), "sed" | "perl" | "ruby")
        && rest
            .iter()
            .any(|w| w == "-i" || w.starts_with("-i") || w == "--in-place")
    {
        return Some(RiskyActionKind::MassEdit);
    }
    if prog == "patch" {
        return Some(RiskyActionKind::MassEdit);
    }

    // 5. Overwrite commands
    if OVERWRITE_COMMANDS.contains(&prog.as_str()) {
        return Some(RiskyActionKind::Overwrite);
    }
    if prog == "tee" && !rest.iter().any(|w| w == "-a" || w == "--append") {
        return Some(RiskyActionKind::Overwrite);
    }

    None
}

fn git_subcommand(args: &[String]) -> Option<String> {
    let mut i = 0usize;
    while i < args.len() {
        let tok = args[i].as_str();
        if matches!(tok, "-C" | "-c" | "--git-dir" | "--work-tree") {
            i += 2;
            continue;
        }
        if tok.starts_with('-') {
            i += 1;
            continue;
        }
        return Some(tok.to_ascii_lowercase());
    }
    None
}

fn resolve_in_workspace(workspace: &Path, raw: &str) -> PathBuf {
    let p = PathBuf::from(raw);
    if p.is_absolute() {
        p
    } else {
        workspace.join(p)
    }
}

/// Short human-readable label for a tool call shown in the backup list.
pub fn action_label(kind: RiskyActionKind, name: &str, arguments: &str) -> String {
    if SHELL_TOOLS.contains(&name) {
        if let Some(cmd) = command_arg(arguments) {
            let short: String = cmd.chars().take(72).collect();
            return format!("Before {}: {short}", kind.label());
        }
    }
    if let Some(p) = path_arg(name, arguments) {
        return format!("Before {}: {name} {p}", kind.label());
    }
    format!("Before {}: {name}", kind.label())
}

// ---- Git & Folder Backup / Checkpoint / Restore ----

fn run_git(dir: &Path, args: &[&str]) -> Result<(i32, String), String> {
    let mut cmd = grokhub_core::git_cmd::git();
    cmd.args(["-c", "core.quotepath=false", "-c", "core.hooksPath="])
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0");
    hide_windows_console(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| format!("git could not start: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.code().unwrap_or(-1), text))
}

fn git_ok(dir: &Path, args: &[&str]) -> Option<String> {
    match run_git(dir, args) {
        Ok((0, text)) => Some(text.trim().to_string()),
        _ => None,
    }
}

pub fn is_git_repo(dir: &Path) -> bool {
    git_ok(dir, &["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true")
}

pub fn current_branch(dir: &Path) -> Option<String> {
    let b = git_ok(dir, &["symbolic-ref", "--short", "HEAD"])?;
    if b.is_empty() {
        None
    } else {
        Some(b)
    }
}

/// Detect the repo's default branch from `origin/HEAD`, falling back to
/// `main` (and `master` if present when no remote HEAD is set).
pub fn detected_default_branch(dir: &Path) -> Option<String> {
    if let Some(sym) = git_ok(
        dir,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        if let Some(branch) = sym.strip_prefix("origin/") {
            if !branch.is_empty() {
                return Some(branch.to_string());
            }
        }
    }
    if git_ok(dir, &["show-ref", "--verify", "--quiet", "refs/heads/main"]).is_some() {
        return Some("main".to_string());
    }
    if git_ok(
        dir,
        &["show-ref", "--verify", "--quiet", "refs/heads/master"],
    )
    .is_some()
    {
        return Some("master".to_string());
    }
    None
}

/// True when `branch` is protected (`main` or the repo's detected default
/// branch) and must NEVER receive automatic checkpoint commits (AC-3).
pub fn is_protected_branch(dir: &Path, branch: &str) -> bool {
    let b = branch.trim();
    if b.is_empty() || b == ALWAYS_PROTECTED_BRANCH {
        return true;
    }
    detected_default_branch(dir).is_some_and(|def| def == b)
}

pub fn has_uncommitted_changes(dir: &Path) -> bool {
    git_ok(dir, &["status", "--porcelain"]).is_some_and(|s| !s.is_empty())
}

fn next_backup_id(now_ms: u64, existing: &[BackupEntry]) -> String {
    let base = format_timestamp_id(now_ms);
    if !existing.iter().any(|b| b.id == base) {
        return base;
    }
    for n in 1..1000u32 {
        let candidate = format!("{base}-{n:03}");
        if !existing.iter().any(|b| b.id == candidate) {
            return candidate;
        }
    }
    format!("{base}-{now_ms}")
}

fn format_timestamp_id(now_ms: u64) -> String {
    // Stable sortable UTC timestamp `YYYYMMDD-HHMMSS-mmm` without external crates.
    let total_secs = now_ms / 1000;
    let ms = now_ms % 1000;
    let days = (total_secs / 86_400) as i64;
    let secs_of_day = total_secs % 86_400;
    let hour = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let sec = secs_of_day % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}{month:02}{day:02}-{hour:02}{min:02}{sec:02}-{ms:03}")
}

/// Howard Hinnant's civil-from-days algorithm (days since 1970-01-01).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);
    (year, m as u32, d as u32)
}

fn count_changed_files(dir: &Path) -> usize {
    git_ok(dir, &["status", "--porcelain"])
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

fn count_tracked_files(dir: &Path) -> usize {
    git_ok(dir, &["ls-files"])
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

/// AC-3: Checkpoint before a risky step during nonstop coding.
/// Commits to a safe work branch (never `main`, never the detected default
/// branch, never force-push) when uncommitted changes exist on a work branch;
/// otherwise takes a non-destructive snapshot (`GitStash`, `GitHeadRef`, or
/// `.loadout-backups/<timestamp>`).
pub fn checkpoint_before_risky_step(
    config_dir: &Path,
    workspace: &Path,
    action_kind: RiskyActionKind,
    label: &str,
) -> Result<BackupEntry, String> {
    create_backup_inner(config_dir, workspace, action_kind, label, true)
}

/// AC-5: Create a backup before a risky action runs.
/// In a git repo: snapshot commit on a work branch (when on a non-protected
/// branch with changes), non-destructive git stash (when on a protected branch
/// or detached HEAD with changes), or a pinned git ref to HEAD (when clean).
/// Outside a git repo: copy files to `<workspace>/.loadout-backups/<timestamp>`.
pub fn create_backup(
    config_dir: &Path,
    workspace: &Path,
    action_kind: RiskyActionKind,
    label: &str,
) -> Result<BackupEntry, String> {
    create_backup_inner(config_dir, workspace, action_kind, label, false)
}

fn create_backup_inner(
    config_dir: &Path,
    workspace: &Path,
    action_kind: RiskyActionKind,
    label: &str,
    prefer_work_branch_commit: bool,
) -> Result<BackupEntry, String> {
    let now_ms = grokhub_core::now_ms();
    let key = project_key(workspace);
    let _guard = BACKUPS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut store = read_backups_store(config_dir);
    let existing = store
        .projects
        .iter()
        .find(|p| p.project_key == key)
        .map(|p| p.backups.as_slice())
        .unwrap_or(&[]);
    let id = next_backup_id(now_ms, existing);

    let entry = if is_git_repo(workspace) {
        backup_git_repo(
            workspace,
            &id,
            now_ms,
            action_kind,
            label,
            prefer_work_branch_commit,
        )?
    } else {
        backup_folder_copy(workspace, &id, now_ms, action_kind, label)?
    };

    if let Some(proj) = store.projects.iter_mut().find(|p| p.project_key == key) {
        proj.backups.push(entry.clone());
    } else {
        store.projects.push(ProjectBackups {
            project_key: key,
            backups: vec![entry.clone()],
        });
    }
    write_json_atomic(config_dir, BACKUPS_FILE, &store)?;
    Ok(entry)
}

fn backup_git_repo(
    workspace: &Path,
    id: &str,
    now_ms: u64,
    action_kind: RiskyActionKind,
    label: &str,
    prefer_work_branch_commit: bool,
) -> Result<BackupEntry, String> {
    let branch = current_branch(workspace);
    let safe_work_branch = branch
        .as_deref()
        .is_some_and(|b| !is_protected_branch(workspace, b));
    let dirty = has_uncommitted_changes(workspace);

    if dirty && safe_work_branch && prefer_work_branch_commit {
        if let Some(entry) =
            try_work_branch_commit(workspace, id, now_ms, action_kind, label, branch.clone())
        {
            return Ok(entry);
        }
    }

    if dirty {
        if let Some(entry) =
            try_git_stash_snapshot(workspace, id, now_ms, action_kind, label, branch.clone())
        {
            return Ok(entry);
        }
        if safe_work_branch {
            if let Some(entry) =
                try_work_branch_commit(workspace, id, now_ms, action_kind, label, branch.clone())
            {
                return Ok(entry);
            }
        }
    }

    if let Some(head) = git_ok(workspace, &["rev-parse", "HEAD"]) {
        let ref_name = format!("refs/loadout-backups/{id}");
        let _ = run_git(workspace, &["update-ref", &ref_name, &head]);
        return Ok(BackupEntry {
            id: id.to_string(),
            created_at_ms: now_ms,
            label: label.to_string(),
            action_kind,
            strategy: BackupStrategy::GitHeadRef,
            ref_value: head,
            branch,
            files_count: count_tracked_files(workspace),
            restored_at_ms: None,
        });
    }

    // Repo with no commits yet: fall back to a folder copy.
    backup_folder_copy(workspace, id, now_ms, action_kind, label)
}

fn try_work_branch_commit(
    workspace: &Path,
    id: &str,
    now_ms: u64,
    action_kind: RiskyActionKind,
    label: &str,
    branch: Option<String>,
) -> Option<BackupEntry> {
    // Re-check branch safety unconditionally: never commit to main or default.
    let b = branch.as_deref()?;
    if is_protected_branch(workspace, b) {
        return None;
    }
    let files_count = count_changed_files(workspace);
    let (add_code, _) = run_git(workspace, &["add", "-A"]).ok()?;
    if add_code != 0 {
        return None;
    }
    let msg = format!("loadout-checkpoint({id}): {label}");
    let (commit_code, _) = run_git(
        workspace,
        &[
            "-c",
            "user.name=Loadout Checkpoint",
            "-c",
            "user.email=checkpoint@local",
            "commit",
            "--no-verify",
            "-m",
            &msg,
        ],
    )
    .ok()?;
    if commit_code != 0 {
        return None;
    }
    let sha = git_ok(workspace, &["rev-parse", "HEAD"])?;
    Some(BackupEntry {
        id: id.to_string(),
        created_at_ms: now_ms,
        label: label.to_string(),
        action_kind,
        strategy: BackupStrategy::GitCommit,
        ref_value: sha,
        branch,
        files_count,
        restored_at_ms: None,
    })
}

fn try_git_stash_snapshot(
    workspace: &Path,
    id: &str,
    now_ms: u64,
    action_kind: RiskyActionKind,
    label: &str,
    branch: Option<String>,
) -> Option<BackupEntry> {
    // Needs at least one commit for `git stash push`.
    git_ok(workspace, &["rev-parse", "--verify", "HEAD"])?;
    let files_count = count_changed_files(workspace);
    let msg = format!("loadout-backup({id}): {label}");
    let (push_code, _) = run_git(
        workspace,
        &[
            "-c",
            "user.name=Loadout Backup",
            "-c",
            "user.email=backup@local",
            "stash",
            "push",
            "--include-untracked",
            "-m",
            &msg,
        ],
    )
    .ok()?;
    if push_code != 0 {
        return None;
    }
    let sha = git_ok(workspace, &["rev-parse", "stash@{0}"])?;
    // Pin an immutable ref so stash indices shifting can never lose this entry.
    let ref_name = format!("refs/loadout-backups/{id}");
    let _ = run_git(workspace, &["update-ref", &ref_name, &sha]);
    // Immediately re-apply so the working tree is unchanged before the tool runs.
    let _ = run_git(workspace, &["stash", "apply", "--quiet", &sha]);
    Some(BackupEntry {
        id: id.to_string(),
        created_at_ms: now_ms,
        label: label.to_string(),
        action_kind,
        strategy: BackupStrategy::GitStash,
        ref_value: sha,
        branch,
        files_count,
        restored_at_ms: None,
    })
}

fn backup_folder_copy(
    workspace: &Path,
    id: &str,
    now_ms: u64,
    action_kind: RiskyActionKind,
    label: &str,
) -> Result<BackupEntry, String> {
    let rel = format!("{BACKUP_DIR_NAME}/{id}");
    let dest_root = workspace.join(BACKUP_DIR_NAME).join(id);
    fs::create_dir_all(&dest_root).map_err(|e| format!("could not create backup folder: {e}"))?;
    let home = grokhub_core::user_home()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let mut copied = 0usize;
    copy_dir_recursive(workspace, workspace, &dest_root, &home, &mut copied)?;
    Ok(BackupEntry {
        id: id.to_string(),
        created_at_ms: now_ms,
        label: label.to_string(),
        action_kind,
        strategy: BackupStrategy::FolderCopy,
        ref_value: rel,
        branch: None,
        files_count: copied,
        restored_at_ms: None,
    })
}

fn copy_dir_recursive(
    root: &Path,
    current: &Path,
    dest_root: &Path,
    home: &str,
    copied: &mut usize,
) -> Result<(), String> {
    if *copied >= MAX_COPY_FILES {
        return Ok(());
    }
    let entries = match fs::read_dir(current) {
        Ok(rd) => rd,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        if *copied >= MAX_COPY_FILES {
            break;
        }
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if ft.is_dir() {
            if SKIP_COPY_DIRS.iter().any(|s| *s == name_str.as_ref()) {
                continue;
            }
            copy_dir_recursive(root, &path, dest_root, home, copied)?;
        } else if ft.is_file() {
            let norm = path.to_string_lossy().replace('\\', "/");
            if !home.is_empty() && grokhub_core::rewind_sensitive(&norm, home) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.len() > MAX_COPY_FILE_BYTES {
                continue;
            }
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let target = dest_root.join(rel);
            if let Some(parent) = target.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if fs::copy(&path, &target).is_ok() {
                *copied += 1;
            }
        }
    }
    Ok(())
}

/// AC-6: One-click restore of a saved backup. Never deletes the backup entry
/// or its files/git ref.
pub fn restore_backup(
    config_dir: &Path,
    workspace: &Path,
    backup_id: &str,
) -> Result<RestoreResult, String> {
    let key = project_key(workspace);
    let _guard = BACKUPS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut store = read_backups_store(config_dir);
    let proj = store
        .projects
        .iter_mut()
        .find(|p| p.project_key == key)
        .ok_or_else(|| "No backups found for this project.".to_string())?;
    let entry = proj
        .backups
        .iter_mut()
        .find(|b| b.id == backup_id)
        .ok_or_else(|| format!("Backup `{backup_id}` was not found."))?;

    let (restored_files, summary) = match entry.strategy {
        BackupStrategy::FolderCopy => {
            let src_root = workspace.join(&entry.ref_value);
            if !src_root.is_dir() {
                return Err(format!(
                    "Backup folder `{}` is no longer on disk.",
                    entry.ref_value
                ));
            }
            let mut count = 0usize;
            restore_dir_recursive(&src_root, &src_root, workspace, &mut count)?;
            (
                count,
                format!("Restored {count} file(s) from {}.", entry.ref_value),
            )
        }
        BackupStrategy::GitCommit | BackupStrategy::GitHeadRef => {
            let sha = entry.ref_value.clone();
            let (code, out) = run_git(workspace, &["checkout", &sha, "--", "."])?;
            if code != 0 {
                return Err(format!("Could not restore git snapshot {sha}: {out}"));
            }
            let short: String = sha.chars().take(7).collect();
            (
                entry.files_count,
                format!("Restored workspace files from git snapshot {short}."),
            )
        }
        BackupStrategy::GitStash => {
            let sha = entry.ref_value.clone();
            // Restore tracked files from the stash tree first, then apply the
            // stash (including untracked files via `^3` if present) without
            // dropping the stash.
            let (code, out) = run_git(workspace, &["checkout", &sha, "--", "."])?;
            if code != 0 {
                let (apply_code, apply_out) = run_git(workspace, &["stash", "apply", &sha])?;
                if apply_code != 0 {
                    return Err(format!(
                        "Could not restore git stash {sha}: {out} / {apply_out}"
                    ));
                }
            } else {
                // Also restore untracked files stored in the 3rd parent of a `-u` stash.
                let untracked_parent = format!("{sha}^3");
                if git_ok(workspace, &["rev-parse", "--verify", &untracked_parent]).is_some() {
                    let _ = run_git(workspace, &["checkout", &untracked_parent, "--", "."]);
                }
            }
            let short: String = sha.chars().take(7).collect();
            (
                entry.files_count,
                format!("Restored workspace files from git stash {short}."),
            )
        }
    };

    entry.restored_at_ms = Some(grokhub_core::now_ms());
    let id = entry.id.clone();
    write_json_atomic(config_dir, BACKUPS_FILE, &store)?;
    Ok(RestoreResult {
        id,
        summary,
        restored_files,
    })
}

fn restore_dir_recursive(
    src_root: &Path,
    current: &Path,
    workspace: &Path,
    count: &mut usize,
) -> Result<(), String> {
    let entries = fs::read_dir(current).map_err(|e| format!("could not read backup: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            restore_dir_recursive(src_root, &path, workspace, count)?;
        } else if ft.is_file() {
            let rel = path
                .strip_prefix(src_root)
                .map_err(|e| format!("invalid backup path: {e}"))?;
            let dest = workspace.join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(&path, &dest)
                .map_err(|e| format!("could not restore {}: {e}", rel.display()))?;
            *count += 1;
        }
    }
    Ok(())
}

// ---- AC-4: Nonstop loop and stall guards ----

/// Why nonstop coding stopped, with enough context to draw a clear status card
/// naming why and where (AC-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonstopStopReason {
    RepeatedFailure {
        step: String,
        count: u32,
        limit: u32,
        last_error: String,
    },
    NoProgress {
        elapsed_mins: u64,
        limit_mins: u32,
        where_at: String,
    },
    RepetitionCut {
        where_at: String,
    },
}

impl NonstopStopReason {
    /// One-line plain summary appended to the assistant turn.
    pub fn summary_line(&self) -> String {
        match self {
            Self::RepeatedFailure {
                step,
                count,
                limit,
                last_error,
            } => format!(
                "Nonstop coding stopped: `{step}` failed {count} times in a row (limit: {limit}). Last error: {last_error}"
            ),
            Self::NoProgress {
                elapsed_mins,
                limit_mins,
                where_at,
            } => format!(
                "Nonstop coding stopped: no progress for {elapsed_mins} min (limit: {limit_mins} min) at {where_at}."
            ),
            Self::RepetitionCut { where_at } => format!(
                "Nonstop coding stopped: the local model reply hit the repetition guard at {where_at}."
            ),
        }
    }

    /// Findings-formatted status card body (`LoopEvent::Findings`) rendered in
    /// the chat explaining both **why** and **where** nonstop coding stopped.
    pub fn status_card_body(&self) -> String {
        match self {
            Self::RepeatedFailure {
                step,
                count,
                limit,
                last_error,
            } => format!(
                "Findings from this run\n\n\
                 - High: Nonstop coding stopped after {count} repeated failures of the same step (limit {limit}).\n\
                 - Info: Where: {step}\n\
                 - Info: Last error: {last_error}"
            ),
            Self::NoProgress {
                elapsed_mins,
                limit_mins,
                where_at,
            } => format!(
                "Findings from this run\n\n\
                 - High: Nonstop coding stopped after {elapsed_mins} minutes with no progress (limit {limit_mins} minutes).\n\
                 - Info: Where: {where_at}"
            ),
            Self::RepetitionCut { where_at } => format!(
                "Findings from this run\n\n\
                 - High: Nonstop coding stopped because the local model hit the repetition guard.\n\
                 - Info: Where: {where_at}"
            ),
        }
    }
}

/// Runtime guard for nonstop coding (AC-4).
#[derive(Debug, Clone)]
pub struct NonstopGuard {
    max_repeated_failures: u32,
    stall_timeout_mins: u32,
    stall_timeout_ms: u64,
    last_progress_ms: u64,
    last_failed_key: Option<String>,
    consecutive_failures: u32,
    last_where: String,
}

impl NonstopGuard {
    pub fn new(settings: &PanelSettings, now_ms: u64) -> Self {
        let clamped = settings.clone().clamped();
        Self {
            max_repeated_failures: clamped.max_repeated_failures,
            stall_timeout_mins: clamped.stall_timeout_mins,
            stall_timeout_ms: u64::from(clamped.stall_timeout_mins).saturating_mul(60_000),
            last_progress_ms: now_ms,
            last_failed_key: None,
            consecutive_failures: 0,
            last_where: "initial step".to_string(),
        }
    }

    /// Record one completed tool call. Returns `Some(NonstopStopReason)` when
    /// the repeated-failure limit or no-progress timer trips.
    pub fn note_call(
        &mut self,
        step_key: &str,
        step_label: &str,
        did: Did,
        failed: bool,
        error_text: &str,
        now_ms: u64,
    ) -> Option<NonstopStopReason> {
        if !step_label.trim().is_empty() {
            self.last_where = step_label.trim().to_string();
        }
        if failed {
            if self.last_failed_key.as_deref() == Some(step_key) {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
            } else {
                self.last_failed_key = Some(step_key.to_string());
                self.consecutive_failures = 1;
            }
            if self.consecutive_failures >= self.max_repeated_failures {
                let short_err: String = error_text
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("tool call failed")
                    .chars()
                    .take(160)
                    .collect();
                return Some(NonstopStopReason::RepeatedFailure {
                    step: self.last_where.clone(),
                    count: self.consecutive_failures,
                    limit: self.max_repeated_failures,
                    last_error: short_err,
                });
            }
        } else if matches!(did, Did::Worked | Did::Changed) {
            self.last_failed_key = None;
            self.consecutive_failures = 0;
            self.last_progress_ms = now_ms;
        }
        self.check_clock(&self.last_where, now_ms)
    }

    /// Check whether `stall_timeout_mins` have elapsed without real progress.
    pub fn check_clock(&self, where_at: &str, now_ms: u64) -> Option<NonstopStopReason> {
        let elapsed_ms = now_ms.saturating_sub(self.last_progress_ms);
        if elapsed_ms >= self.stall_timeout_ms {
            let place = if where_at.trim().is_empty() {
                self.last_where.clone()
            } else {
                where_at.trim().to_string()
            };
            return Some(NonstopStopReason::NoProgress {
                elapsed_mins: elapsed_ms / 60_000,
                limit_mins: self.stall_timeout_mins,
                where_at: place,
            });
        }
        None
    }

    /// Check whether the local repetition guard cut the reply.
    pub fn check_repetition_cut(
        &self,
        reply_text: &str,
        where_at: &str,
    ) -> Option<NonstopStopReason> {
        if reply_text.contains(crate::route::local_repeat::REPEAT_NOTE) {
            let place = if where_at.trim().is_empty() {
                self.last_where.clone()
            } else {
                where_at.trim().to_string()
            };
            return Some(NonstopStopReason::RepetitionCut { where_at: place });
        }
        None
    }
}

#[cfg(test)]
#[path = "code_panel_tests.rs"]
mod tests;

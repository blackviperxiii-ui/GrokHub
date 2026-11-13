//! Chat history: the shell's past chats, read back from the session files the
//! engine already writes (`{config}/sessions/<id>.jsonl`, one per chat). The
//! History page lists, searches, renames and deletes them; opening one shows
//! its transcript, and the next message resumes it (`agent::serve`).
//!
//! Only the shell's own chats (ids starting `shell-`) show. The cabin keeps
//! its threads on its own History page, so no file has two writers.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::{InputItem, SessionInfo};
use grokhub_core::{ProjectKind, ProjectNode};
use serde::Serialize;

/// The shell's conversation ids (`newConversationId` in ui/src/Chat.tsx).
pub const PREFIX: &str = "shell-";
/// A search lists at most this many chats.
pub const SEARCH_CAP: usize = 200;
/// A reopened chat shows its newest lines up to this many; Show earlier
/// asks for more.
pub const LOAD_CAP: usize = 200;

pub const NOT_SHELL: &str = "That chat isn't one of this app's chats.";
pub const NO_NAME: &str = "Give the chat a name.";

/// One row on the History page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRow {
    pub id: String,
    pub title: String,
    /// Unix ms the chat started.
    pub created: u64,
    /// Unix ms of its last saved turn; the page groups and sorts by it.
    pub updated: u64,
    /// Pinned with `/pin` or the row's menu: it lists first.
    pub pinned: bool,
    /// The id of the project (`projects.json`) whose folder the chat works
    /// in, so History lists it under that project like the cabin does.
    pub project: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    /// Tool steps are not replayed as cards; one muted line names them.
    Tools,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Line {
    pub role: Role,
    pub text: String,
}

/// A saved chat as the Chat page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub id: String,
    pub title: String,
    /// The newest lines, oldest first.
    pub lines: Vec<Line>,
    /// How many older lines were left out.
    pub earlier: usize,
}

fn shell_id(id: &str) -> Result<&str, String> {
    let id = id.trim();
    if id.starts_with(PREFIX) {
        Ok(id)
    } else {
        Err(NOT_SHELL.into())
    }
}

/// When the chat's file last changed, or its start time if that can't be read.
/// Never before the start: the file system's coarse clock can stamp a file
/// written in the same millisecond a few ms behind `now_ms`.
fn updated_ms(info: &SessionInfo) -> u64 {
    grokhub_agent::session_file(&info.id)
        .ok()
        .and_then(|path| std::fs::metadata(path).ok())
        .and_then(|meta| meta.modified().ok())
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map_or(info.created, |at| (at.as_millis() as u64).max(info.created))
}

fn row(info: &SessionInfo) -> ChatRow {
    ChatRow {
        id: info.id.clone(),
        title: info.title.clone(),
        created: info.created,
        updated: updated_ms(info),
        pinned: false,
        project: None,
    }
}

const PINS_FILE: &str = "shell-pins.json";

/// The ids `/pin` has pinned, from `{config}/shell-pins.json`.
pub fn pinned_ids(config_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(config_dir.join(PINS_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// `/pin`: pins the chat, or unpins it when it is pinned. True when it is
/// pinned now. A chat with no file yet can't be pinned.
pub fn toggle_pin(config_dir: &Path, id: &str) -> Result<bool, String> {
    let id = shell_id(id)?;
    {
        let _config = ConfigGuard::set(config_dir);
        grokhub_agent::load_session(id)?;
    }
    let mut ids = pinned_ids(config_dir);
    let pinned = if let Some(at) = ids.iter().position(|p| p == id) {
        ids.remove(at);
        false
    } else {
        ids.push(id.to_string());
        true
    };
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{PINS_FILE}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, config_dir.join(PINS_FILE)).map_err(|e| e.to_string())?;
    Ok(pinned)
}

/// The shell's sessions, newest activity first. Call under a `ConfigGuard`.
fn shell_sessions() -> Vec<(ChatRow, SessionInfo)> {
    let mut found: Vec<(ChatRow, SessionInfo)> = grokhub_agent::list_sessions()
        .into_iter()
        .filter(|info| info.id.starts_with(PREFIX))
        .map(|info| (row(&info), info))
        .collect();
    let config_dir = grokhub_agent::perm::config_dir();
    let pins = pinned_ids(&config_dir);
    let nodes = crate::projects::load(&config_dir).unwrap_or_default();
    let home = std::env::var("HOME").ok();
    for (row, info) in &mut found {
        row.pinned = pins.contains(&row.id);
        row.project = project_of(&nodes, &info.cwd, home.as_deref());
    }
    found.sort_by(|(a, _), (b, _)| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.updated.cmp(&a.updated))
            .then(a.id.cmp(&b.id))
    });
    found
}

pub fn list(config_dir: &Path) -> Vec<ChatRow> {
    let _config = ConfigGuard::set(config_dir);
    shell_sessions().into_iter().map(|(row, _)| row).collect()
}

/// Chats whose title or transcript holds `query`, ignoring case. A blank
/// query lists them all.
pub fn search(config_dir: &Path, query: &str) -> Vec<ChatRow> {
    let needle = query.trim().to_lowercase();
    let _config = ConfigGuard::set(config_dir);
    shell_sessions()
        .into_iter()
        .filter(|(row, info)| {
            needle.is_empty()
                || row.title.to_lowercase().contains(&needle)
                || grokhub_agent::transcript_pairs(info)
                    .iter()
                    .any(|(_, text)| text.to_lowercase().contains(&needle))
        })
        .take(SEARCH_CAP)
        .map(|(row, _)| row)
        .collect()
}

/// The transcript's lines: user and assistant messages as they were shown,
/// and each run of tool steps folded into one `Tools` line naming them.
pub fn lines(info: &SessionInfo) -> Vec<Line> {
    // `transcript_pairs` writes a tool call as "Tool {name}\n{arguments}" and
    // its output as "Tool result\n{output}"; the names come from the input.
    let tools: Vec<String> = info
        .input()
        .into_iter()
        .filter_map(|item| match item {
            InputItem::FunctionCall { name, .. } => Some(name),
            _ => None,
        })
        .collect();
    let mut out: Vec<Line> = Vec::new();
    for (role, text) in grokhub_agent::transcript_pairs(info) {
        let assistant = role == "assistant";
        let call = tools
            .iter()
            .find(|name| assistant && text.starts_with(&format!("Tool {name}\n")));
        let result = assistant && text.starts_with("Tool result\n");
        match (call, result) {
            (Some(name), _) => match out.last_mut() {
                Some(Line {
                    role: Role::Tools,
                    text,
                }) => {
                    text.push_str(", ");
                    text.push_str(name);
                }
                _ => out.push(Line {
                    role: Role::Tools,
                    text: name.clone(),
                }),
            },
            (None, true) => {}
            (None, false) if assistant => out.push(Line {
                role: Role::Assistant,
                // The reply's chips block was never shown.
                text: grokhub_core::objectives::without_markers(
                    &grokhub_core::split_chip_trailer(&text).0,
                ),
            }),
            (None, false) => out.push(Line {
                role: Role::User,
                text,
            }),
        }
    }
    out
}

/// A saved shell chat as its file holds it.
pub fn saved(config_dir: &Path, id: &str) -> Result<SessionInfo, String> {
    let id = shell_id(id)?;
    let _config = ConfigGuard::set(config_dir);
    grokhub_agent::load_session(id)
}

/// The chat's newest `limit` lines (at least one), oldest first.
pub fn load(config_dir: &Path, id: &str, limit: usize) -> Result<Transcript, String> {
    let info = saved(config_dir, id)?;
    let mut all = lines(&info);
    let earlier = all.len().saturating_sub(limit.max(1));
    let lines = all.split_off(earlier);
    Ok(Transcript {
        id: info.id,
        title: info.title,
        lines,
        earlier,
    })
}

/// Renames a saved chat. A chat with no file yet can't be renamed.
pub fn rename(config_dir: &Path, id: &str, title: &str) -> Result<(), String> {
    let id = shell_id(id)?;
    if title.trim().is_empty() {
        return Err(NO_NAME.into());
    }
    let _config = ConfigGuard::set(config_dir);
    grokhub_agent::load_session(id)?;
    grokhub_agent::rename_session(id, title)
}

/// Deletes the chat's file and its files; a turn still running is not
/// written back. `/bg` and scheduled forks of it go with it.
pub fn delete(config_dir: &Path, id: &str) -> Result<(), String> {
    let id = shell_id(id)?;
    match delete_chats(config_dir, &[id.to_string()]).failed {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

/// What deleting a list of chats did: the ones that went, and why it stopped
/// before the end of the list when a chat could not be deleted.
#[derive(Debug, PartialEq, Eq)]
struct Deleted {
    gone: Vec<String>,
    failed: Option<String>,
}

/// Deletes these shell chats and answers with the ones that went: each
/// chat's file and its files, then their forks in one pass over the saved
/// sessions, then the pins in one write. A chat that can't be deleted stops
/// it; the ones before it are gone and cleaned up after, and are still
/// named in the answer.
fn delete_chats(config_dir: &Path, ids: &[String]) -> Deleted {
    let _config = ConfigGuard::set(config_dir);
    let mut gone: Vec<String> = Vec::with_capacity(ids.len());
    let mut failed = None;
    for id in ids {
        let deleted = shell_id(id).and_then(|id| {
            grokhub_agent::delete_session(id)?;
            Ok(id.to_string())
        });
        match deleted {
            Ok(id) => gone.push(id),
            Err(why) => {
                failed = Some(why);
                break;
            }
        }
    }
    grokhub_agent::delete_forks_of(&gone);
    let pins = pinned_ids(config_dir);
    let kept: Vec<&String> = pins.iter().filter(|p| !gone.contains(p)).collect();
    if kept.len() != pins.len() {
        if let Ok(raw) = serde_json::to_string(&kept) {
            let _ = std::fs::write(config_dir.join(PINS_FILE), raw);
        }
    }
    Deleted { gone, failed }
}

/// Takes one message out of a saved shell chat, so the model never reads it
/// again: the ask `text` (`later` counts the same ask sent after it), or with
/// `reply` the reply to that ask (no `text`: the reply before any ask).
/// False when the saved chat had no copy of it. A running reply refuses.
pub fn forget(
    config_dir: &Path,
    id: &str,
    reply: bool,
    text: Option<&str>,
    later: usize,
) -> Result<bool, String> {
    let id = shell_id(id)?;
    let _config = ConfigGuard::set(config_dir);
    let what = match (reply, text) {
        (true, ask) => grokhub_agent::Forget::Reply {
            ask: ask.map(|text| (text, later)),
        },
        (false, Some(text)) => grokhub_agent::Forget::Ask { text, later },
        (false, None) => return Err(NO_MESSAGE.into()),
    };
    Ok(grokhub_agent::forget_message(id, what)? == grokhub_agent::Forgotten::Removed)
}

/// Delete message… with no message named.
pub const NO_MESSAGE: &str = "Pick the message to delete.";

/// The project row whose folder is `cwd`, if the list has one.
fn project_of(nodes: &[ProjectNode], cwd: &str, home: Option<&str>) -> Option<String> {
    nodes
        .iter()
        .find(|n| {
            n.kind == ProjectKind::Project && crate::projects::same_folder(&n.path, cwd, home)
        })
        .map(|n| n.id.clone())
}

/// The sidebar's folders and projects from `{config}/projects.json`, in file
/// order; the page builds the tree from `parent`.
pub fn project_nodes(config_dir: &Path) -> Result<Vec<ProjectNode>, String> {
    crate::projects::load(config_dir)
}

/// Delete all asks twice: the first click arms it, and a second click inside
/// [`DELETE_ALL_ARM`] deletes, but never sooner than [`DELETE_ALL_MIN`] after
/// the first (the second half of a double-click), as the cabin's does.
pub const DELETE_ALL_ARM: Duration = Duration::from_secs(5);
pub const DELETE_ALL_MIN: Duration = Duration::from_millis(500);

/// Where Delete all stands after a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Arm {
    /// Armed (or still armed after a too-quick second click): ask again.
    Armed,
    /// Every chat was deleted.
    Deleted,
}

/// The armed state, shared by the Delete all button: when the first click
/// came, and the chats it counted. Those are the chats a yes deletes.
#[derive(Debug, Default)]
pub struct DeleteArm {
    at: Option<Instant>,
    ids: Vec<String>,
}

/// What a Delete all click does.
#[derive(Debug, PartialEq, Eq)]
enum Click {
    /// The first click, or one after the window lapsed: it arms.
    Arm,
    /// The second half of a double-click: still armed, nothing else.
    Keep,
    /// The armed second click: delete.
    Delete,
}

impl DeleteArm {
    pub const fn new() -> Self {
        Self {
            at: None,
            ids: Vec::new(),
        }
    }

    fn click(&mut self, now: Instant) -> Click {
        match self.at {
            Some(at) if now.saturating_duration_since(at) < DELETE_ALL_MIN => Click::Keep,
            Some(at) if now.saturating_duration_since(at) <= DELETE_ALL_ARM => {
                self.at = None;
                Click::Delete
            }
            _ => {
                self.at = Some(now);
                Click::Arm
            }
        }
    }

    pub fn cancel(&mut self) {
        self.at = None;
        self.ids.clear();
    }
}

/// Delete all's answer: where it stands, and how many chats a yes removes
/// (or removed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeleteAll {
    pub state: Arm,
    pub count: usize,
    /// The chats that were deleted; empty until `state` is `Deleted`.
    pub ids: Vec<String>,
    /// Why deleting stopped before the last chat named, when it did. The
    /// chats in `ids` are gone all the same.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One Delete all click. Nothing is deleted unless the click is the armed
/// second one; then the shell chats the first click counted go, with their
/// forks and pins. A chat started after the first click was never named, so
/// it stays. Grok CLI and cabin chats are not this app's to delete.
pub fn delete_all(
    config_dir: &Path,
    arm: &Mutex<DeleteArm>,
    now: Instant,
) -> Result<DeleteAll, String> {
    delete_all_with(config_dir, arm, now, delete_chats)
}

/// [`delete_all`] with the deleting step named, so a test can look at the
/// arm while it runs.
fn delete_all_with(
    config_dir: &Path,
    arm: &Mutex<DeleteArm>,
    now: Instant,
    delete: impl FnOnce(&Path, &[String]) -> Deleted,
) -> Result<DeleteAll, String> {
    // Only the click is decided under the lock: the listing before it and
    // the deleting after it read and write the disk, and Cancel (a
    // main-thread command) takes the same lock.
    let listed: Vec<String> = list(config_dir).into_iter().map(|r| r.id).collect();
    let named = {
        let mut arm = arm.lock().unwrap_or_else(|e| e.into_inner());
        match arm.click(now) {
            Click::Delete => std::mem::take(&mut arm.ids),
            click => {
                if click == Click::Arm {
                    arm.ids = listed;
                }
                return Ok(DeleteAll {
                    state: Arm::Armed,
                    count: arm.ids.len(),
                    ids: Vec::new(),
                    error: None,
                });
            }
        }
    };
    let Deleted { gone, failed } = delete(config_dir, &named);
    match failed {
        // Nothing went: the error is the whole answer.
        Some(why) if gone.is_empty() => Err(why),
        // The chats that went are named even when a later one stopped it,
        // so the caller ends their workers and the page closes them.
        error => Ok(DeleteAll {
            state: Arm::Deleted,
            count: gone.len(),
            ids: gone,
            error,
        }),
    }
}

/// A chat this app lists but never writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadOnlyRow {
    pub id: String,
    pub title: String,
    /// Unix ms of the file's last change; 0 when it can't be read.
    pub updated: u64,
    /// "cli" for a Grok CLI session, "cabin" for a chat from the egui cabin.
    pub source: &'static str,
}

fn modified_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |at| at.as_millis() as u64)
}

/// Where a read-only row's text comes from.
enum Source {
    /// A Grok CLI session file, when the session has one.
    Cli(Option<PathBuf>),
    /// A cabin chat, as the listing read it. The listing never repairs a
    /// file, so a view of it never writes one.
    Cabin(Box<SessionInfo>),
}

/// Grok CLI sessions (`cli_home`) and the cabin's own chats, read-only.
///
/// Cabin chats list read-only while both apps ship: a session file has one
/// writer, so the shell never resumes, renames or deletes them. Once the cabin
/// is retired this is where they become the shell's own. Forks (`/bg` and
/// scheduled runs) and the shell's own chats stay off this list.
pub fn read_only(config_dir: &Path, cli_home: Option<&Path>, query: &str) -> Vec<ReadOnlyRow> {
    read_only_sources(config_dir, cli_home, query)
        .into_iter()
        .map(|(row, _)| row)
        .collect()
}

fn read_only_sources(
    config_dir: &Path,
    cli_home: Option<&Path>,
    query: &str,
) -> Vec<(ReadOnlyRow, Source)> {
    let needle = query.trim().to_lowercase();
    let _config = ConfigGuard::set(config_dir);
    let cli = match cli_home {
        Some(home) => grokhub_core::cli_history::discover_session_files_in(home),
        None => grokhub_core::cli_history::discover_session_files(),
    };
    let mut seen = std::collections::HashSet::new();
    let mut rows: Vec<(ReadOnlyRow, Source)> = Vec::new();
    for session in cli {
        if !seen.insert(session.id.clone()) {
            continue;
        }
        rows.push((
            ReadOnlyRow {
                updated: session.path.as_deref().map_or(0, modified_ms),
                id: session.id,
                title: session.title,
                source: "cli",
            },
            Source::Cli(session.path),
        ));
    }
    for info in grokhub_agent::list_sessions() {
        if info.id.starts_with(PREFIX)
            || !info.forked_from.is_empty()
            || !seen.insert(info.id.clone())
        {
            continue;
        }
        rows.push((
            ReadOnlyRow {
                updated: updated_ms(&info),
                id: info.id.clone(),
                title: info.title.clone(),
                source: "cabin",
            },
            Source::Cabin(Box::new(info)),
        ));
    }
    rows.retain(|(r, _)| needle.is_empty() || r.title.to_lowercase().contains(&needle));
    rows.sort_by(|(a, _), (b, _)| b.updated.cmp(&a.updated).then(a.id.cmp(&b.id)));
    rows.truncate(SEARCH_CAP);
    rows
}

/// The text of a CLI `chat_history.jsonl` record's content: strings and
/// `text` fields, in order.
fn json_texts(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|i| json_texts(i, out)),
        serde_json::Value::Object(map) => {
            if let Some(t) = map.get("text") {
                json_texts(t, out);
            }
        }
        _ => {}
    }
}

/// How much of a CLI session file a view reads: its newest part.
const CLI_VIEW_BYTES: u64 = 2 * 1024 * 1024;

pub const CLI_UNREADABLE: &str = "That session's file couldn't be read.";

/// The newest [`CLI_VIEW_BYTES`] of a CLI session file under `home`, from
/// the first whole line. A link is never followed, and nothing outside the
/// CLI folder is opened: the CLI's sign-in and settings files sit beside its
/// sessions, and this app never reads them.
fn cli_text(home: &Path, path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    if std::fs::symlink_metadata(path)
        .ok()?
        .file_type()
        .is_symlink()
    {
        return None;
    }
    let real = std::fs::canonicalize(path).ok()?;
    let inside = ["memory", "sessions", "worktrees"].iter().any(|root| {
        std::fs::canonicalize(home.join(root)).is_ok_and(|root| real.starts_with(root))
    });
    if !inside {
        return None;
    }
    let mut file = std::fs::File::open(&real).ok()?;
    let start = file.metadata().ok()?.len().saturating_sub(CLI_VIEW_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut raw = Vec::new();
    file.take(CLI_VIEW_BYTES).read_to_end(&mut raw).ok()?;
    if start > 0 {
        // The cut fell inside a line (and maybe inside a character).
        let line = raw
            .iter()
            .position(|b| *b == b'\n')
            .map_or(raw.len(), |at| at + 1);
        raw.drain(..line);
    }
    Some(String::from_utf8_lossy(&raw).into_owned())
}

fn cli_lines(home: &Path, path: &Path) -> Result<Vec<Line>, String> {
    let raw = cli_text(home, path).ok_or_else(|| CLI_UNREADABLE.to_string())?;
    if path.extension().and_then(|e| e.to_str()) == Some("md") {
        return Ok(grokhub_core::cli_history::parse_session_markdown(&raw)
            .into_iter()
            .map(|(role, text)| Line {
                role: if role.eq_ignore_ascii_case("user") {
                    Role::User
                } else {
                    Role::Assistant
                },
                text,
            })
            .collect());
    }
    let mut out = Vec::new();
    for rec in raw
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
    {
        let role = match rec.get("type").and_then(|t| t.as_str()) {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            _ => continue,
        };
        let mut texts = Vec::new();
        if let Some(c) = rec.get("content") {
            json_texts(c, &mut texts);
        }
        let text = texts.join("\n").trim().to_string();
        if !text.is_empty() {
            out.push(Line { role, text });
        }
    }
    Ok(out)
}

/// A read-only chat's newest `limit` lines, for the page to show in place.
/// Only ids `read_only` lists open; never a shell chat, so the view can't
/// become a second writer.
pub fn view_read_only(
    config_dir: &Path,
    cli_home: Option<&Path>,
    id: &str,
    limit: usize,
) -> Result<Transcript, String> {
    let id = id.trim();
    if id.starts_with(PREFIX) {
        return Err(NOT_READ_ONLY.into());
    }
    let (row, source) = read_only_sources(config_dir, cli_home, "")
        .into_iter()
        .find(|(r, _)| r.id == id)
        .ok_or_else(|| NOT_READ_ONLY.to_string())?;
    let mut all = match source {
        Source::Cabin(info) => lines(&info),
        Source::Cli(path) => {
            let path = path.ok_or_else(|| NOT_READ_ONLY.to_string())?;
            let home = cli_home
                .map(Path::to_path_buf)
                .or_else(grokhub_core::cli_history::grok_home)
                .ok_or_else(|| NOT_READ_ONLY.to_string())?;
            cli_lines(&home, &path)?
        }
    };
    let earlier = all.len().saturating_sub(limit.max(1));
    let lines = all.split_off(earlier);
    Ok(Transcript {
        id: row.id,
        title: row.title,
        lines,
        earlier,
    })
}

pub const NOT_READ_ONLY: &str = "That isn't a chat this page can show.";

/// A tool step a search matched: the chat, the turn, and the row's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRow {
    pub chat: String,
    pub turn: u32,
    pub line: String,
}

/// Tool steps (from `spans/*.jsonl`) whose chat title, tool, decision or
/// claim hold every word of `query`, over the shell's own chats only. Reads
/// span files, so call it off the main thread.
pub fn step_search(config_dir: &Path, query: &str) -> Vec<StepRow> {
    let titles: std::collections::HashMap<String, String> = {
        let _config = ConfigGuard::set(config_dir);
        shell_sessions()
            .into_iter()
            .map(|(row, _)| (row.id, row.title))
            .collect()
    };
    grokhub_agent::harness::search_spans(config_dir, query, &titles, &[])
        .hits
        .into_iter()
        .map(|h| StepRow {
            chat: h.chat_id,
            turn: h.turn,
            line: h.line,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Route, Session, SessionMode};
    use grokhub_agent::{
        AuthKind, CancelToken, ClientError, ContentPart, DesktopOps, FunctionCall, ModelClient,
        PermMode, ResponsesRequest, StreamEvent, ToolOutput, TurnOutput,
    };
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-history-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    struct NoDesktop;

    impl DesktopOps for NoDesktop {
        fn halted(&self) -> bool {
            false
        }
        fn locked(&self) -> bool {
            false
        }
        fn call(&self, _name: &str, _args: &serde_json::Value) -> ToolOutput {
            ToolOutput::err("no desktop in tests")
        }
    }

    fn no_desktop() -> Box<dyn DesktopOps> {
        Box::new(NoDesktop)
    }

    /// Replies with `reply`, after one `read_file` call when `tool` is set,
    /// and keeps every request it got.
    struct Model {
        reply: &'static str,
        tool: bool,
        requests: Mutex<Vec<ResponsesRequest>>,
    }

    impl Model {
        fn new(reply: &'static str, tool: bool) -> Arc<Self> {
            Arc::new(Self {
                reply,
                tool,
                requests: Mutex::new(Vec::new()),
            })
        }

        /// The (role, text) of every message in the request at `index`.
        fn messages(&self, index: usize) -> Vec<(String, String)> {
            let requests = self.requests.lock().expect("requests");
            requests[index]
                .input
                .iter()
                .filter_map(|item| match item {
                    InputItem::Message { role, content } if role != "system" => Some((
                        role.clone(),
                        content
                            .iter()
                            .filter_map(|part| match part {
                                ContentPart::InputText(text) => Some(text.as_str()),
                                ContentPart::InputImage(_) => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )),
                    _ => None,
                })
                .collect()
        }
    }

    impl ModelClient for Model {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            let mut requests = self.requests.lock().expect("requests");
            requests.push(req.clone());
            let asked_tool = req
                .input
                .iter()
                .any(|item| matches!(item, InputItem::FunctionCallOutput { .. }));
            if self.tool && !asked_tool {
                return Ok(TurnOutput {
                    calls: vec![FunctionCall {
                        call_id: "call-1".into(),
                        name: "read_file".into(),
                        arguments: r#"{"target_file":"a.txt"}"#.into(),
                    }],
                    ..TurnOutput::default()
                });
            }
            sink(StreamEvent::TextDelta(self.reply.into()));
            Ok(TurnOutput {
                text: self.reply.into(),
                ..TurnOutput::default()
            })
        }
    }

    /// One message through a fresh worker for `id`, as the shell runs it.
    fn turn(id: &str, config: &Path, workspace: &Path, model: Arc<Model>, text: &str) {
        let session = Session::spawn_with(id, config.to_path_buf(), no_desktop);
        let route = Route {
            client: model,
            auth_kind: AuthKind::ApiKey,
            workspace: workspace.to_path_buf(),
            mode: PermMode::Ask,
            desktop: false,
            session: SessionMode::Chat,
            voice: String::new(),
            voice_session: None,
            suggestions: true,
            lead: false,
        };
        assert_eq!(
            session.prompt(text, Vec::new(), route, Box::new(|_| {})),
            Ok("end_turn".to_string())
        );
    }

    fn line(role: Role, text: &str) -> Line {
        Line {
            role,
            text: text.into(),
        }
    }

    #[test]
    fn a_chat_lists_loads_and_resumes_with_its_first_turn() {
        let (config, workspace) = (temp_dir("resume-cfg"), temp_dir("resume-ws"));
        let id = "shell-history-resume";
        let first = Model::new("Hello back.", false);
        turn(id, &config, &workspace, Arc::clone(&first), "hello there");

        let rows = list(&config);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            (rows[0].id.as_str(), rows[0].title.as_str()),
            (id, "hello there")
        );
        assert!(rows[0].updated >= rows[0].created, "{rows:?}");

        let shown = load(&config, id, LOAD_CAP).expect("load");
        assert_eq!(
            shown,
            Transcript {
                id: id.into(),
                title: "hello there".into(),
                lines: vec![
                    line(Role::User, "hello there"),
                    line(Role::Assistant, "Hello back.")
                ],
                earlier: 0,
            }
        );

        // The worker for that chat is gone (New chat, a restart); a new one
        // with the same id sends the first turn along with the next message.
        let second = Model::new("Still here.", false);
        turn(
            id,
            &config,
            &workspace,
            Arc::clone(&second),
            "are you there?",
        );
        let sent = second.messages(0);
        let texts: Vec<&str> = sent.iter().map(|(_, text)| text.as_str()).collect();
        let at = |needle: &str| texts.iter().position(|text| text.ends_with(needle));
        let (hello, back, again) = (
            at("hello there").expect("first question resent"),
            at("Hello back.").expect("first reply resent"),
            at("are you there?").expect("new question"),
        );
        assert!(hello < back && back < again, "{sent:?}");
        assert_eq!(sent[back].0, "assistant");

        let shown = load(&config, id, LOAD_CAP).expect("load");
        assert_eq!(
            shown.lines,
            vec![
                line(Role::User, "hello there"),
                line(Role::Assistant, "Hello back."),
                line(Role::User, "are you there?"),
                line(Role::Assistant, "Still here."),
            ]
        );
        // Only the newest lines come back past the limit.
        let tail = load(&config, id, 2).expect("tail");
        assert_eq!(
            (tail.earlier, tail.lines),
            (
                2,
                vec![
                    line(Role::User, "are you there?"),
                    line(Role::Assistant, "Still here.")
                ]
            )
        );
        assert_eq!(list(&config).len(), 1);
    }

    #[test]
    fn a_replys_chips_block_becomes_follow_ups_and_never_shows() {
        let (config, workspace) = (temp_dir("chips-cfg"), temp_dir("chips-ws"));
        let id = "shell-history-chips";
        let model = Model::new(
            "Plug it in, then pick it in Settings.\n<chips>\nask|Explain the second step in more detail|Explain step two\nopen|automations|See automations\n</chips>",
            false,
        );
        let session = Session::spawn_with(id, config.clone(), no_desktop);
        let route = Route {
            client: Arc::clone(&model) as Arc<dyn ModelClient + Send + Sync>,
            auth_kind: AuthKind::ApiKey,
            workspace: workspace.clone(),
            mode: PermMode::Ask,
            desktop: false,
            session: SessionMode::Plan,
            voice: String::new(),
            voice_session: None,
            suggestions: true,
            lead: false,
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        assert_eq!(
            session.prompt(
                "how do I set up the backup drive?",
                Vec::new(),
                route,
                Box::new(move |event| sink.lock().expect("events").push(event)),
            ),
            Ok("end_turn".to_string())
        );
        // The chips rule rode on the cached rules.
        let asked = model.requests.lock().expect("requests")[0].clone();
        assert!(
            asked.input.iter().any(|item| matches!(
                item,
                InputItem::Message { role, content } if role == "system"
                    && content.iter().any(|part| matches!(
                        part,
                        ContentPart::InputText(text) if text.contains(grokhub_core::CHIP_TRAILER_RULE)
                    ))
            )),
            "no chips rule in the system prompt"
        );
        let events = events.lock().expect("events").clone();
        let shown: String = events
            .iter()
            .filter_map(|event| match event {
                crate::agent::UiEvent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(shown, "Plug it in, then pick it in Settings.\n");
        let ups = events
            .iter()
            .find_map(|event| match event {
                crate::agent::UiEvent::Suggestions { items } => Some(items.clone()),
                _ => None,
            })
            .expect("follow-ups");
        assert_eq!(
            ups.iter()
                .map(|c| (c.label.as_str(), c.view, c.mode.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("Explain step two", None, "plan"),
                ("See automations", Some("automations"), ""),
            ]
        );
        // The saved chat shows the reply as it was shown, and its follow-ups
        // stay with it until the chat moves on.
        let lines = load(&config, id, LOAD_CAP).expect("load").lines;
        assert_eq!(
            lines.last(),
            Some(&line(
                Role::Assistant,
                "Plug it in, then pick it in Settings.\n"
            ))
        );
        let saved = crate::chips::follow_ups(&config, id, 1).expect("saved");
        assert_eq!((saved.chips.len(), saved.mode.as_str()), (2, "plan"));
        assert_eq!(crate::chips::follow_ups(&config, id, 2), None);
    }

    #[test]
    fn a_saved_chat_exports_as_markdown_and_never_over_an_older_file() {
        let (config, workspace) = (temp_dir("export-cfg"), temp_dir("export-ws"));
        let out = temp_dir("export-out");
        let id = "shell-history-export";
        turn(
            id,
            &config,
            &workspace,
            Model::new("Blue suits it.", true),
            "pick a colour",
        );
        let first = crate::export::export_into(&config, id, &out, 0, 0).expect("export");
        let again = crate::export::export_into(&config, id, &out, 0, 0).expect("export again");
        assert_eq!(
            first.file_name().and_then(|n| n.to_str()),
            Some("pick a colour-19700101-0000.md")
        );
        assert_eq!(
            again.file_name().and_then(|n| n.to_str()),
            Some("pick a colour-19700101-0000-2.md")
        );
        let md = std::fs::read_to_string(&first).expect("read");
        for part in [
            "# pick a colour",
            "pick a colour",
            "Used read_file",
            "Blue suits it.",
        ] {
            assert!(md.contains(part), "{part} missing from {md}");
        }
    }

    #[test]
    fn tool_steps_fold_into_one_muted_line() {
        let (config, workspace) = (temp_dir("tools-cfg"), temp_dir("tools-ws"));
        std::fs::write(workspace.join("a.txt"), "alpha").expect("seed");
        let id = "shell-history-tools";
        turn(
            id,
            &config,
            &workspace,
            Model::new("It says alpha.", true),
            "read a.txt",
        );
        assert_eq!(
            load(&config, id, LOAD_CAP).expect("load").lines,
            vec![
                line(Role::User, "read a.txt"),
                line(Role::Tools, "read_file"),
                line(Role::Assistant, "It says alpha."),
            ]
        );
    }

    #[test]
    fn search_matches_titles_and_words_in_the_chat() {
        let (config, workspace) = (temp_dir("search-cfg"), temp_dir("search-ws"));
        turn(
            "shell-history-s1",
            &config,
            &workspace,
            Model::new("Try the BLUE one.", false),
            "pick a paint colour",
        );
        turn(
            "shell-history-s2",
            &config,
            &workspace,
            Model::new("Sure.", false),
            "plan a trip",
        );
        let ids = |rows: Vec<ChatRow>| rows.into_iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(search(&config, "  Blue ")), vec!["shell-history-s1"]);
        assert_eq!(ids(search(&config, "TRIP")), vec!["shell-history-s2"]);
        assert_eq!(ids(search(&config, "kayak")), Vec::<String>::new());
        assert_eq!(search(&config, " ").len(), 2);
    }

    #[test]
    fn forgetting_a_message_keeps_it_from_the_next_turn() {
        let (config, workspace) = (temp_dir("forget-cfg"), temp_dir("forget-ws"));
        let id = "shell-history-forget";
        turn(
            id,
            &config,
            &workspace,
            Model::new("Hi.", false),
            "keep this",
        );
        turn(
            id,
            &config,
            &workspace,
            Model::new("Noted the fruit.", false),
            "the code is kumquat",
        );
        assert_eq!(forget(&config, id, false, None, 0), Err(NO_MESSAGE.into()));
        assert_eq!(
            forget(&config, "native-cabin-1", false, Some("x"), 0),
            Err(NOT_SHELL.into())
        );
        assert_eq!(
            forget(&config, id, false, Some("the code is kumquat"), 0),
            Ok(true)
        );
        // The reply to "keep this" goes whole; the later reply now follows it.
        assert_eq!(forget(&config, id, true, Some("keep this"), 0), Ok(true));
        assert_eq!(forget(&config, id, false, Some("never said"), 0), Ok(false));
        let shown = load(&config, id, LOAD_CAP).expect("load");
        assert_eq!(shown.lines, vec![line(Role::User, "keep this")]);

        let next = Model::new("Ok.", false);
        turn(id, &config, &workspace, Arc::clone(&next), "what now");
        let sent = format!("{:?}", next.messages(0));
        assert!(
            !sent.contains("kumquat") && !sent.contains("fruit"),
            "{sent}"
        );
        assert!(sent.contains("keep this"), "{sent}");
    }

    #[test]
    fn rename_and_delete_change_only_the_shell_chat() {
        let (config, workspace) = (temp_dir("edit-cfg"), temp_dir("edit-ws"));
        let id = "shell-history-edit";
        turn(
            id,
            &config,
            &workspace,
            Model::new("Ok.", false),
            "first draft",
        );
        {
            // A cabin chat in the same folder stays off this page.
            let _config = ConfigGuard::set(&config);
            grokhub_agent::rename_session("native-cabin-1", "Cabin chat").expect("cabin file");
        }
        assert_eq!(
            list(&config).into_iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![id]
        );
        assert_eq!(
            load(&config, "native-cabin-1", LOAD_CAP),
            Err(NOT_SHELL.into())
        );
        assert_eq!(delete(&config, "native-cabin-1"), Err(NOT_SHELL.into()));
        assert!(config
            .join("sessions")
            .join("native-cabin-1.jsonl")
            .exists());

        assert_eq!(rename(&config, id, "  "), Err(NO_NAME.into()));
        rename(&config, id, "Final plan").expect("rename");
        assert_eq!(list(&config)[0].title, "Final plan");
        assert!(rename(&config, "shell-history-none", "Ghost").is_err());
        assert!(!config
            .join("sessions")
            .join("shell-history-none.jsonl")
            .exists());

        let file = config.join("sessions").join(format!("{id}.jsonl"));
        assert!(file.exists());
        let fork = {
            let _config = ConfigGuard::set(&config);
            grokhub_agent::fork_session(id).expect("fork")
        };
        let fork_file = config.join("sessions").join(format!("{}.jsonl", fork.id));
        assert!(fork_file.exists());
        delete(&config, id).expect("delete");
        assert!(!file.exists());
        assert!(
            !fork_file.exists(),
            "a /bg fork holds a copy of the chat, so it goes too"
        );
        assert_eq!(list(&config), Vec::<ChatRow>::new());
        assert!(load(&config, id, LOAD_CAP).is_err());
    }

    fn applied_text(applied: crate::slash_ops::Applied) -> String {
        match applied.effect {
            crate::slash::Effect::Notice { text } => text,
            other => panic!("expected a notice, got {other:?}"),
        }
    }

    fn ctx<'a>(config: &'a Path, id: &'a str) -> crate::slash_ops::Ctx<'a> {
        crate::slash_ops::Ctx {
            config_dir: config,
            conversation_id: Some(id),
            bound: "",
            owned: None,
        }
    }

    #[test]
    fn rename_and_pin_act_on_the_chat_and_a_pinned_chat_lists_first() {
        use crate::slash::Op;
        use crate::slash_ops::run;
        let (config, workspace) = (temp_dir("pin-cfg"), temp_dir("pin-ws"));
        let (older, newer) = ("shell-history-pin-a", "shell-history-pin-b");
        turn(
            older,
            &config,
            &workspace,
            Model::new("Ok.", false),
            "first",
        );
        turn(
            newer,
            &config,
            &workspace,
            Model::new("Ok.", false),
            "second",
        );
        let text = applied_text(
            run(Op::Rename("Pier light".into()), &ctx(&config, older)).expect("rename"),
        );
        assert_eq!(text, "Renamed to Pier light");
        assert_eq!(
            list(&config)
                .iter()
                .find(|r| r.id == older)
                .map(|r| r.title.as_str()),
            Some("Pier light")
        );
        assert!(list(&config).iter().all(|r| !r.pinned));
        let text = applied_text(run(Op::Pin, &ctx(&config, older)).expect("pin"));
        assert_eq!(text, "Pinned Pier light");
        let rows = list(&config);
        assert_eq!(rows[0].id, older);
        assert!(rows[0].pinned && !rows[1].pinned);
        assert_eq!(
            search(&config, "").first().map(|r| r.id.as_str()),
            Some(older)
        );
        let text = applied_text(run(Op::Pin, &ctx(&config, older)).expect("unpin"));
        assert_eq!(text, "Unpinned Pier light");
        assert!(list(&config).iter().all(|r| !r.pinned));
        // A chat that was never saved can't be renamed or pinned.
        assert_eq!(
            run(Op::Pin, &ctx(&config, "shell-history-pin-none")),
            Err("Send a message first. This chat isn't saved yet.".into())
        );
        assert_eq!(
            run(Op::Rename("x".into()), &ctx(&config, "native-cabin-1")),
            Err(NOT_SHELL.into())
        );
    }

    #[test]
    fn delete_asks_first_and_changes_nothing_until_yes() {
        use crate::slash::{Effect, Op};
        use crate::slash_ops::run;
        let (config, workspace) = (temp_dir("del-cfg"), temp_dir("del-ws"));
        let id = "shell-history-del";
        turn(id, &config, &workspace, Model::new("Ok.", false), "first");
        run(Op::Rename("Pier light".into()), &ctx(&config, id)).expect("rename");
        run(Op::Pin, &ctx(&config, id)).expect("pin");
        let asked = run(Op::Delete { yes: false }, &ctx(&config, id)).expect("asks");
        assert_eq!(
            asked.effect,
            Effect::Confirm {
                prompt: "Delete Pier light? The chat and its files are removed for good.".into(),
                yes: "/delete yes".into(),
            }
        );
        assert_eq!(list(&config).len(), 1, "no yes, nothing deleted");
        assert!(config.join("sessions").join(format!("{id}.jsonl")).exists());
        let done = run(Op::Delete { yes: true }, &ctx(&config, id)).expect("deleted");
        assert_eq!(
            done.effect,
            Effect::Deleted {
                id: id.into(),
                title: "Pier light".into(),
            }
        );
        assert_eq!(list(&config), Vec::<ChatRow>::new());
        assert_eq!(pinned_ids(&config), Vec::<String>::new());
    }

    #[test]
    fn context_and_usage_read_the_saved_chat_and_the_browser_sees_only_its_own() {
        use crate::slash::Op;
        use crate::slash_ops::{run, Ctx};
        let (config, workspace) = (temp_dir("ctx-cfg"), temp_dir("ctx-ws"));
        let id = "shell-history-ctx";
        turn(
            id,
            &config,
            &workspace,
            Model::new("Blue suits it.", false),
            "pick a colour",
        );
        let text = applied_text(run(Op::Context, &ctx(&config, id)).expect("context"));
        assert!(text.starts_with("1 turn · "), "{text}");
        assert!(text.contains(" tokens · ") && text.ends_with('%'), "{text}");
        assert!(!applied_text(run(Op::Usage, &ctx(&config, id)).expect("usage")).is_empty());
        let mine = |c: &str| c == id;
        let browser = Ctx {
            config_dir: &config,
            conversation_id: Some(id),
            bound: "",
            owned: Some(&mine),
        };
        assert!(run(Op::Context, &browser).is_ok());
        let sessions = applied_text(run(Op::Sessions, &browser).expect("sessions"));
        assert_eq!(sessions, "pick a colour");
        let other = Ctx {
            conversation_id: Some("shell-someone-else"),
            ..browser
        };
        assert_eq!(
            run(Op::Context, &other),
            Err(crate::remote::NOT_YOURS.to_string())
        );
        // On this computer /sessions opens History.
        let here = run(Op::Sessions, &ctx(&config, id)).expect("sessions");
        assert_eq!(here.effect, crate::slash::Effect::Open { view: "history" });
    }

    #[test]
    fn rewind_files_restores_a_checkpointed_file_only_after_a_yes() {
        use crate::slash::{Effect, Op};
        use crate::slash_ops::run;
        let config = temp_dir("rw-cfg");
        let work = temp_dir("rw-ws");
        let id = "shell-history-rewind";
        let file = work.join("notes.txt");
        std::fs::write(&file, "before").expect("seed");
        {
            let _config = ConfigGuard::set(&config);
            grokhub_agent::begin_step(id).expect("step");
            grokhub_agent::before_write(&file);
        }
        std::fs::write(&file, "after").expect("edit");
        let list_text = applied_text(run(Op::RewindList, &ctx(&config, id)).expect("list"));
        assert!(list_text.contains("notes.txt"), "{list_text}");
        let asked = run(Op::RewindFiles { yes: false }, &ctx(&config, id)).expect("asks");
        let Effect::Confirm { prompt, yes } = asked.effect else {
            panic!("expected a question");
        };
        assert!(prompt.contains("notes.txt"), "{prompt}");
        assert_eq!(yes, "/rewind files yes");
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "after");
        applied_text(run(Op::RewindFiles { yes: true }, &ctx(&config, id)).expect("restore"));
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "before");
        assert_eq!(
            run(Op::RewindStep(99), &ctx(&config, id)),
            Err("No checkpoint for step 99".to_string())
        );
        assert_eq!(
            applied_text(run(Op::RewindNone, &ctx(&config, id)).expect("none")),
            crate::slash_ops::REWIND_NA
        );
        assert_eq!(
            run(
                Op::RewindFiles { yes: true },
                &ctx(&config, "shell-history-no-steps")
            ),
            Err("No agent file checkpoints yet. Edit a file in this chat first.".to_string())
        );
    }

    #[test]
    fn a_saved_chat_exports_as_html_and_json() {
        let (config, workspace) = (temp_dir("fmt-cfg"), temp_dir("fmt-ws"));
        let out = temp_dir("fmt-out");
        let id = "shell-history-fmt";
        turn(
            id,
            &config,
            &workspace,
            Model::new("Blue suits it.", false),
            "pick a colour",
        );
        let html =
            crate::export::export_into_as(&config, id, &out, 0, 0, grokhub_core::ChatExport::Html)
                .expect("html");
        assert_eq!(
            html.file_name().and_then(|n| n.to_str()),
            Some("pick a colour-19700101-0000.html")
        );
        let page = std::fs::read_to_string(&html).expect("read");
        for part in ["pick a colour", "Blue suits it."] {
            assert!(page.contains(part), "{part} missing from {page}");
        }
        let json =
            crate::export::export_into_as(&config, id, &out, 0, 0, grokhub_core::ChatExport::Json)
                .expect("json");
        assert_eq!(
            json.file_name().and_then(|n| n.to_str()),
            Some("pick a colour-19700101-0000.json")
        );
        let raw = std::fs::read_to_string(&json).expect("read");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("valid json");
        assert!(value.is_object(), "{raw}");
        assert!(
            raw.contains("Blue suits it.") && raw.contains("pick a colour"),
            "{raw}"
        );
    }

    fn node(
        id: &str,
        name: &str,
        kind: ProjectKind,
        path: &str,
        parent: Option<&str>,
    ) -> ProjectNode {
        ProjectNode {
            id: id.into(),
            name: name.into(),
            kind,
            path: path.into(),
            parent: parent.map(str::to_string),
            open: true,
        }
    }

    #[test]
    fn a_project_lists_its_chats_and_pin_moves_a_chat_to_the_top() {
        let config = temp_dir("tree-cfg");
        let (ws_pier, ws_other) = (temp_dir("tree-pier"), temp_dir("tree-other"));
        for (id, ws, text) in [
            ("shell-tree-a", &ws_pier, "first in pier"),
            ("shell-tree-b", &ws_pier, "second in pier"),
            ("shell-tree-c", &ws_other, "loose chat"),
        ] {
            turn(id, &config, ws, Model::new("Ok.", false), text);
        }
        crate::projects::save(
            &config,
            &[
                node("f1", "Boats", ProjectKind::Folder, "", None),
                node(
                    "p1",
                    "Pier",
                    ProjectKind::Project,
                    &ws_pier.to_string_lossy(),
                    Some("f1"),
                ),
            ],
        )
        .expect("save projects");
        let nodes = project_nodes(&config).expect("nodes");
        assert_eq!(
            nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            ["Boats", "Pier"]
        );
        assert_eq!(nodes[1].parent.as_deref(), Some("f1"));
        let rows = list(&config);
        let mut in_pier: Vec<&str> = rows
            .iter()
            .filter(|r| r.project.as_deref() == Some("p1"))
            .map(|r| r.id.as_str())
            .collect();
        in_pier.sort();
        assert_eq!(in_pier, ["shell-tree-a", "shell-tree-b"]);
        let loose = rows.iter().find(|r| r.id == "shell-tree-c").expect("loose");
        assert_eq!(loose.project, None);
        assert!(rows.iter().all(|r| !r.pinned));
        assert!(toggle_pin(&config, "shell-tree-c").expect("pin"));
        let rows = list(&config);
        assert_eq!(rows[0].id, "shell-tree-c");
        assert!(rows[0].pinned && rows[1..].iter().all(|r| !r.pinned));
        assert!(!toggle_pin(&config, "shell-tree-c").expect("unpin"));
        assert!(list(&config).iter().all(|r| !r.pinned));
    }

    #[test]
    fn delete_all_deletes_nothing_until_the_armed_second_click() {
        let (config, workspace) = (temp_dir("all-cfg"), temp_dir("all-ws"));
        for id in ["shell-all-a", "shell-all-b"] {
            turn(id, &config, &workspace, Model::new("Ok.", false), "hello");
        }
        toggle_pin(&config, "shell-all-a").expect("pin");
        let arm = Mutex::new(DeleteArm::default());
        let t0 = Instant::now();
        // First click arms: nothing is deleted.
        let first = delete_all(&config, &arm, t0).expect("first");
        assert_eq!(
            first,
            DeleteAll {
                state: Arm::Armed,
                count: 2,
                ids: Vec::new(),
                error: None,
            }
        );
        assert_eq!(list(&config).len(), 2);
        // A double-click's second half (100 ms later) keeps it armed.
        let quick = delete_all(&config, &arm, t0 + Duration::from_millis(100)).expect("quick");
        assert_eq!(quick.state, Arm::Armed);
        assert_eq!(list(&config).len(), 2);
        // Cancel disarms, so the next click only arms again.
        arm.lock().expect("arm").cancel();
        let again = delete_all(&config, &arm, t0 + Duration::from_secs(1)).expect("again");
        assert_eq!(again.state, Arm::Armed);
        assert_eq!(list(&config).len(), 2);
        // A second click after the 5 s window only re-arms.
        let late = delete_all(&config, &arm, t0 + Duration::from_secs(7)).expect("late");
        assert_eq!(late.state, Arm::Armed);
        assert_eq!(list(&config).len(), 2);
        // A real second click inside the window deletes every chat and pin.
        let done = delete_all(&config, &arm, t0 + Duration::from_secs(9)).expect("done");
        assert_eq!(
            done,
            DeleteAll {
                state: Arm::Deleted,
                count: 2,
                ids: vec!["shell-all-a".to_string(), "shell-all-b".to_string()],
                error: None,
            }
        );
        assert_eq!(list(&config), Vec::<ChatRow>::new());
        assert_eq!(pinned_ids(&config), Vec::<String>::new());
    }

    #[test]
    fn delete_all_deletes_exactly_the_chats_the_armed_click_named() {
        let (config, workspace) = (temp_dir("named-cfg"), temp_dir("named-ws"));
        for id in ["shell-named-a", "shell-named-b", "shell-named-c"] {
            turn(id, &config, &workspace, Model::new("Ok.", false), "hello");
        }
        let arm = Mutex::new(DeleteArm::default());
        let t0 = Instant::now();
        let asked = delete_all(&config, &arm, t0).expect("arms");
        assert_eq!((asked.state, asked.count), (Arm::Armed, 3));
        // A chat that starts after the question was never named in it.
        turn(
            "shell-named-late",
            &config,
            &workspace,
            Model::new("Ok.", false),
            "started after the question",
        );
        // A too-quick second click still asks about the same three.
        let quick = delete_all(&config, &arm, t0 + Duration::from_millis(100)).expect("quick");
        assert_eq!((quick.state, quick.count), (Arm::Armed, 3));
        let done = delete_all(&config, &arm, t0 + Duration::from_secs(2)).expect("deletes");
        let mut gone = done.ids.clone();
        gone.sort();
        assert_eq!(gone, ["shell-named-a", "shell-named-b", "shell-named-c"]);
        assert_eq!((done.state, done.count), (Arm::Deleted, 3));
        assert_eq!(
            list(&config).into_iter().map(|r| r.id).collect::<Vec<_>>(),
            ["shell-named-late"]
        );
    }

    #[test]
    fn delete_all_takes_each_chats_fork_and_pin_and_leaves_the_rest() {
        let (config, workspace) = (temp_dir("batch-cfg"), temp_dir("batch-ws"));
        for id in ["shell-batch-a", "shell-batch-b", "shell-batch-c"] {
            turn(id, &config, &workspace, Model::new("Ok.", false), "hello");
        }
        toggle_pin(&config, "shell-batch-b").expect("pin");
        let fork = {
            let _config = ConfigGuard::set(&config);
            grokhub_agent::rename_session("cabin-batch-keep", "Cabin chat").expect("cabin file");
            grokhub_agent::fork_session("shell-batch-a").expect("fork")
        };
        let sessions = config.join("sessions");
        assert!(sessions.join(format!("{}.jsonl", fork.id)).exists());
        let arm = Mutex::new(DeleteArm::default());
        let t0 = Instant::now();
        delete_all(&config, &arm, t0).expect("arms");
        let done = delete_all(&config, &arm, t0 + Duration::from_secs(1)).expect("deletes");
        let mut gone = done.ids.clone();
        gone.sort();
        assert_eq!(gone, ["shell-batch-a", "shell-batch-b", "shell-batch-c"]);
        let mut left: Vec<String> = std::fs::read_dir(&sessions)
            .expect("sessions")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["cabin-batch-keep.jsonl"], "the fork goes too");
        assert_eq!(
            std::fs::read_to_string(config.join("shell-pins.json")).expect("pins"),
            "[]"
        );
    }

    #[test]
    fn cancel_never_waits_on_the_deleting() {
        let (config, workspace) = (temp_dir("lock-cfg"), temp_dir("lock-ws"));
        turn(
            "shell-lock-a",
            &config,
            &workspace,
            Model::new("Ok.", false),
            "hello",
        );
        let arm = Mutex::new(DeleteArm::default());
        let t0 = Instant::now();
        delete_all(&config, &arm, t0).expect("arms");
        let mut free = None;
        let done = delete_all_with(&config, &arm, t0 + Duration::from_secs(1), |dir, ids| {
            // Cancel is a main-thread command that takes this lock.
            free = Some(arm.try_lock().is_ok());
            delete_chats(dir, ids)
        })
        .expect("deletes");
        assert_eq!(free, Some(true), "the arm stayed locked while deleting");
        assert_eq!(done.ids, ["shell-lock-a"]);
    }

    #[test]
    fn deleting_that_stops_part_way_names_the_chats_that_went() {
        let (config, workspace) = (temp_dir("part-cfg"), temp_dir("part-ws"));
        for id in ["shell-part-a", "shell-part-b"] {
            turn(id, &config, &workspace, Model::new("Ok.", false), "hello");
        }
        toggle_pin(&config, "shell-part-a").expect("pin");
        // The second id is not one of this app's chats, so it stops there.
        let ids = ["shell-part-a", "cabin-part-x", "shell-part-b"].map(String::from);
        assert_eq!(
            delete_chats(&config, &ids),
            Deleted {
                gone: vec!["shell-part-a".to_string()],
                failed: Some(NOT_SHELL.to_string()),
            }
        );
        let left: Vec<String> = list(&config).into_iter().map(|r| r.id).collect();
        assert_eq!(left, ["shell-part-b"]);
        assert_eq!(pinned_ids(&config), Vec::<String>::new());
    }

    #[test]
    fn delete_all_that_fails_part_way_still_answers_with_the_chats_it_deleted() {
        let (config, workspace) = (temp_dir("partall-cfg"), temp_dir("partall-ws"));
        for id in ["shell-partall-a", "shell-partall-b"] {
            turn(id, &config, &workspace, Model::new("Ok.", false), "hello");
        }
        let arm = Mutex::new(DeleteArm::default());
        let t0 = Instant::now();
        delete_all(&config, &arm, t0).expect("arms");
        // One chat goes; the other one's file can't be removed.
        let done = delete_all_with(&config, &arm, t0 + Duration::from_secs(1), |_, ids| {
            let mut named = ids.to_vec();
            named.sort();
            assert_eq!(named, ["shell-partall-a", "shell-partall-b"]);
            Deleted {
                gone: vec!["shell-partall-a".to_string()],
                failed: Some("Access is denied.".to_string()),
            }
        });
        assert_eq!(
            done,
            Ok(DeleteAll {
                state: Arm::Deleted,
                count: 1,
                ids: vec!["shell-partall-a".to_string()],
                error: Some("Access is denied.".to_string()),
            })
        );
        // The page reads the reason from `error`; an answer with none has no
        // such field.
        assert_eq!(
            serde_json::to_value(done.expect("answer")).expect("json"),
            serde_json::json!({
                "state": "deleted",
                "count": 1,
                "ids": ["shell-partall-a"],
                "error": "Access is denied.",
            })
        );
        assert_eq!(
            serde_json::to_value(
                delete_all(&config, &arm, t0 + Duration::from_secs(2)).expect("arms")
            )
            .expect("json"),
            serde_json::json!({ "state": "armed", "count": 2, "ids": [] })
        );
        // With no chat deleted, the error is the whole answer.
        let none = delete_all_with(&config, &arm, t0 + Duration::from_secs(3), |_, _| Deleted {
            gone: Vec::new(),
            failed: Some("Access is denied.".to_string()),
        });
        assert_eq!(none, Err("Access is denied.".to_string()));
    }

    #[test]
    fn viewing_a_cabin_chat_with_a_torn_last_line_leaves_its_file_as_it_is() {
        use std::io::Write;
        let (config, workspace) = (temp_dir("torn-cfg"), temp_dir("torn-ws"));
        let cli_home = temp_dir("torn-cli");
        turn(
            "cabin-torn",
            &config,
            &workspace,
            Model::new("Cabin reply.", false),
            "from the cabin",
        );
        let file = config.join("sessions").join("cabin-torn.jsonl");
        // The cabin is part-way through appending its next record.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .expect("open")
            .write_all(br#"{"type":"message","role":"user","content":[{"type":"input_te"#)
            .expect("torn tail");
        let before = std::fs::read_to_string(&file).expect("before");
        let shown = view_read_only(&config, Some(&cli_home), "cabin-torn", 50).expect("cabin view");
        assert_eq!(
            shown.lines,
            vec![
                line(Role::User, "from the cabin"),
                line(Role::Assistant, "Cabin reply.")
            ]
        );
        assert_eq!(
            std::fs::read_to_string(&file).expect("after"),
            before,
            "a read-only view rewrote the cabin's file"
        );
    }

    #[test]
    fn a_cli_session_over_the_read_cap_opens_and_shows_its_newest_lines() {
        let config = temp_dir("big-cfg");
        let cli_home = temp_dir("big-cli");
        let cli_id = "0198f2a4-aaaa-7d2e-9a3f-5c6d7e8f9a0c";
        let dir = cli_home.join("sessions").join(cli_id);
        std::fs::create_dir_all(&dir).expect("cli dir");
        // 65-byte records, so the 2 MB mark counted from either end falls
        // inside a three-byte box-drawing character.
        let record = "{\"type\":\"user\",\"content\":\"────────────\"}\n";
        assert_eq!(record.len(), 65);
        let mut raw = record.repeat(32_323);
        raw.push_str("{\"type\":\"assistant\",\"content\":\"The newest line.\"}\n");
        assert_eq!(raw.len() as u64, CLI_VIEW_BYTES + 3_893);
        std::fs::write(dir.join("chat_history.jsonl"), raw).expect("cli file");
        let shown = view_read_only(&config, Some(&cli_home), cli_id, 50).expect("opens");
        assert_eq!(shown.lines.len(), 50);
        assert_eq!(
            shown.lines.last(),
            Some(&line(Role::Assistant, "The newest line."))
        );
        assert_eq!(shown.lines[0], line(Role::User, "────────────"));
        // The cut drops the first 60 records; 32,264 lines are read.
        assert_eq!(shown.earlier, 32_214);
        let _ = std::fs::remove_dir_all(&cli_home);
    }

    #[cfg(unix)]
    #[test]
    fn a_cli_session_file_that_is_a_link_is_never_opened() {
        let config = temp_dir("link-cfg");
        let cli_home = temp_dir("link-cli");
        // A stand-in for the CLI's credentials file; the real one is never touched.
        std::fs::write(
            cli_home.join("auth.json"),
            "{\"type\":\"user\",\"content\":\"FAKE-AUTH-LINE\"}\n",
        )
        .expect("fake auth");
        let cli_id = "0198f2a4-bbbb-7d2e-9a3f-5c6d7e8f9a0d";
        let dir = cli_home.join("sessions").join(cli_id);
        std::fs::create_dir_all(&dir).expect("cli dir");
        std::os::unix::fs::symlink("../../auth.json", dir.join("chat_history.jsonl"))
            .expect("link");
        let listed = read_only(&config, Some(&cli_home), "");
        assert_eq!(
            listed
                .iter()
                .map(|r| (r.id.as_str(), r.title.as_str()))
                .collect::<Vec<_>>(),
            [(cli_id, cli_id)]
        );
        // The listing hands out no file for it, so there is nothing to open.
        let shown = view_read_only(&config, Some(&cli_home), cli_id, 50);
        assert_eq!(shown, Err(NOT_READ_ONLY.to_string()));
        // Asked for by path, the link and a file beside the sessions are
        // refused too; a session's own file reads.
        assert_eq!(cli_text(&cli_home, &dir.join("chat_history.jsonl")), None);
        assert_eq!(cli_text(&cli_home, &cli_home.join("auth.json")), None);
        std::fs::write(dir.join("notes.md"), "# Harbor\n").expect("own file");
        assert_eq!(
            cli_text(&cli_home, &dir.join("notes.md")),
            Some("# Harbor\n".to_string())
        );
        // A linked folder that leads back out of the sessions is refused.
        std::os::unix::fs::symlink(&cli_home, dir.join("up")).expect("folder link");
        assert_eq!(cli_text(&cli_home, &dir.join("up").join("auth.json")), None);
        let _ = std::fs::remove_dir_all(&cli_home);
    }

    #[test]
    fn cli_and_cabin_chats_list_read_only_and_never_open_for_writing() {
        let (config, workspace) = (temp_dir("ro-cfg"), temp_dir("ro-ws"));
        let cli_home = temp_dir("ro-cli");
        let cli_id = "0198f2a4-7b1c-7d2e-9a3f-5c6d7e8f9a0b";
        let dir = cli_home.join("sessions").join(cli_id);
        std::fs::create_dir_all(&dir).expect("cli dir");
        std::fs::write(
            dir.join("chat_history.jsonl"),
            concat!(
                r#"{"type":"user","content":[{"text":"<user_query>Sketch the harbor wall</user_query>"}]}"#,
                "\n",
                r#"{"type":"assistant","content":[{"text":"Here is a sketch."}]}"#,
                "\n"
            ),
        )
        .expect("cli file");
        turn(
            "shell-ro-own",
            &config,
            &workspace,
            Model::new("Ok.", false),
            "mine",
        );
        turn(
            "cabin-ro-thread",
            &config,
            &workspace,
            Model::new("Cabin reply.", false),
            "from the cabin",
        );

        let rows = read_only(&config, Some(&cli_home), "");
        let mut ids: Vec<(&str, &str)> = rows.iter().map(|r| (r.id.as_str(), r.source)).collect();
        ids.sort();
        assert_eq!(ids, [(cli_id, "cli"), ("cabin-ro-thread", "cabin")]);
        assert_eq!(
            read_only(&config, Some(&cli_home), "harbor").len(),
            1,
            "search narrows by title"
        );

        let cli = view_read_only(&config, Some(&cli_home), cli_id, 50).expect("cli view");
        assert_eq!(cli.lines.len(), 2);
        assert_eq!(cli.lines[0].role, Role::User);
        assert!(cli.lines[0].text.contains("Sketch the harbor wall"));
        assert_eq!(line(Role::Assistant, "Here is a sketch."), cli.lines[1]);
        let cabin =
            view_read_only(&config, Some(&cli_home), "cabin-ro-thread", 50).expect("cabin view");
        assert_eq!(cabin.lines[0], line(Role::User, "from the cabin"));
        assert_eq!(cabin.lines[1], line(Role::Assistant, "Cabin reply."));
        // The shell's own chats and unknown ids never open this way, and
        // nothing here renames or deletes a read-only chat.
        assert_eq!(
            view_read_only(&config, Some(&cli_home), "shell-ro-own", 50),
            Err(NOT_READ_ONLY.into())
        );
        assert_eq!(
            view_read_only(&config, Some(&cli_home), "../escape", 50),
            Err(NOT_READ_ONLY.into())
        );
        assert_eq!(
            rename(&config, "cabin-ro-thread", "x"),
            Err(NOT_SHELL.into())
        );
        assert_eq!(delete(&config, "cabin-ro-thread"), Err(NOT_SHELL.into()));
        assert_eq!(
            list(&config).len(),
            1,
            "only the shell's own chat is the shell's"
        );
    }

    #[test]
    fn a_step_search_finds_a_fixture_tool_call_in_this_apps_chats_only() {
        use grokhub_agent::harness::{append_span, AccessMode, Span};
        let (config, workspace) = (temp_dir("step-cfg"), temp_dir("step-ws"));
        turn(
            "shell-step-a",
            &config,
            &workspace,
            Model::new("Ok.", false),
            "make a note",
        );
        let step = |chat: &str, turn: u32, tool: &str, claim: &str| {
            Span::soft_allow(
                chat,
                tool,
                "{}",
                "ok",
                claim,
                AccessMode::Supervised,
                "grok_build",
            )
            .in_turn(chat, turn)
        };
        append_span(
            &config,
            &step("shell-step-a", 2, "write_file", "write notes.md"),
        )
        .expect("span");
        append_span(
            &config,
            &step("cabin-elsewhere", 1, "write_file", "write notes.md"),
        )
        .expect("span");
        let hits = step_search(&config, "write notes");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chat, "shell-step-a");
        assert_eq!(hits[0].turn, 2);
        assert!(
            hits[0].line.contains("turn 2 · write_file · allow"),
            "{}",
            hits[0].line
        );
        assert_eq!(
            step_search(&config, "nothing like this"),
            Vec::<StepRow>::new()
        );
        assert_eq!(step_search(&config, "  "), Vec::<StepRow>::new());
    }
}

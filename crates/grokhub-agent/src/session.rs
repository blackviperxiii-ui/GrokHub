//! Native session store. One append-only JSONL file per session under the GrokHub data dir.
//!
//! The first line is a header (`id`, `title`, `created`, `cwd`, `model`, `engine: "native"`).
//! Later lines are user and assistant messages, function calls, their outputs, and per-turn usage.
//! A turn's lines are written and synced together. A torn last line is dropped and the file rewritten. Resume
//! rebuilds the Responses `input` from the kept lines.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use grokhub_core::cli_history::GrokSession;
use serde_json::{json, Value};

use crate::{CancelToken, ContentPart, InputItem, Usage};

const ENGINE: &str = "native";

/// A session file's first line: what a list of chats shows, read without
/// the turns ([`list_session_heads`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHead {
    pub id: String,
    pub title: String,
    pub created: u64,
    pub cwd: String,
    pub model: String,
    pub renamed: bool,
    /// Parent session id when this chat was forked. Empty otherwise.
    /// Desktop `/bg` and `/send` start a fresh session; this field remains for
    /// older forked files and for `delete_forks_of` / tests.
    pub forked_from: String,
}

/// Session line type of an xAI compaction item ([`InputItem::ServerCompaction`]).
const SERVER_COMPACTION_RECORD: &str = "server_compaction";

/// Session line type of an xAI sealed reasoning item ([`InputItem::Reasoning`]).
const REASONING_RECORD: &str = "reasoning_item";

/// What a transcript shows where xAI folded the earlier turns.
pub const COMPACTED_NOTE: &str = "Earlier conversation compacted";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Record {
    Item(InputItem),
    Usage {
        usage: Usage,
        meter: String,
        /// Unix ms the turn this line closes was saved. Lines written before
        /// turns carried a time, and side-call usage, have none.
        at: Option<u64>,
    },
    /// Replaces every earlier item. Later items append after this prefix.
    Compaction {
        items: Vec<InputItem>,
        /// How many of the last items are the own items of a turn that
        /// compacted as it ran: they were never saved on their own lines.
        own: usize,
        /// Asks and replies of that turn the compaction folded away, which
        /// no line holds either.
        folded: usize,
    },
    /// The model's thinking, shown above the reply: saved for the chat view
    /// only, never sent back to the model. It sits where it was shown,
    /// before the item after it. The thoughts of a turn that compacted as it
    /// ran sit just before that compaction and show before the turn's own
    /// items.
    Thought(String),
}

/// One thought of a turn as it streamed, with how many of the turn's tool
/// calls and runs of reply text had started before it: it is saved before
/// the next call or reply text after those.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnThought {
    pub calls: usize,
    pub texts: usize,
    pub text: String,
}

/// Session line type of a saved thought ([`Record::Thought`]).
const THOUGHT_RECORD: &str = "thought";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub created: u64,
    pub cwd: String,
    pub model: String,
    pub renamed: bool,
    pub usage: Usage,
    pub meter: String,
    /// The session this one was forked from. Empty when it wasn't.
    pub forked_from: String,
    records: Vec<Record>,
}

impl SessionInfo {
    /// Responses `input` rebuilt from the file. Usage lines are not model input.
    /// A compaction marker drops every item before it.
    pub fn input(&self) -> Vec<InputItem> {
        replay_items(&self.records)
    }
}

/// One History row. CLI rows are read-only copies of `discover_session_files_in`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub id: String,
    pub title: String,
    pub path: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub read_only: bool,
    pub native: bool,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_in_usd_ticks: i64,
    pub meter: String,
}

struct LiveRun {
    cancel: CancelToken,
    stamp: u64,
    on_stop: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

/// Drops the registry entry when the native run ends. Drop does not cancel.
pub struct RunGuard {
    id: String,
    stamp: u64,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        let mut map = runs().lock().unwrap_or_else(|err| err.into_inner());
        if map
            .get(&self.id)
            .is_some_and(|live| live.stamp == self.stamp)
        {
            map.remove(&self.id);
        }
    }
}

fn runs() -> &'static Mutex<std::collections::HashMap<String, LiveRun>> {
    static RUNS: OnceLock<Mutex<std::collections::HashMap<String, LiveRun>>> = OnceLock::new();
    RUNS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn tombs() -> &'static Mutex<HashSet<String>> {
    static TOMBS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    TOMBS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn next_stamp() -> u64 {
    static STAMP: AtomicU64 = AtomicU64::new(1);
    STAMP.fetch_add(1, Ordering::Relaxed)
}

fn history_gen() -> &'static AtomicU64 {
    static GEN: AtomicU64 = AtomicU64::new(1);
    &GEN
}

/// Bumps when a native session is written, renamed, forked, or deleted.
pub fn history_generation() -> u64 {
    history_gen().load(Ordering::Relaxed)
}

fn bump_history() {
    history_gen().fetch_add(1, Ordering::Relaxed);
}

fn tombstoned(id: &str) -> bool {
    tombs()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .contains(id)
}

fn tombstone(id: &str) {
    tombs()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .insert(id.to_string());
}

/// Register a running native session. [`delete_session`] cancels it before the file goes away.
pub fn attach_run(id: &str, cancel: CancelToken) -> Option<RunGuard> {
    attach_run_with_hook(id, cancel, None)
}

fn attach_run_with_hook(
    id: &str,
    cancel: CancelToken,
    on_stop: Option<Box<dyn FnOnce() + Send>>,
) -> Option<RunGuard> {
    let id = safe_id(id).ok()?.to_string();
    let stamp = next_stamp();
    runs().lock().unwrap_or_else(|err| err.into_inner()).insert(
        id.clone(),
        LiveRun {
            cancel,
            stamp,
            on_stop: Mutex::new(on_stop),
        },
    );
    Some(RunGuard { id, stamp })
}

fn stop_run(id: &str) {
    // Tasks and any linked child engines first, then this run's cancel hook.
    // The hook still runs before `delete_session` removes the file.
    crate::tasks::halt_tree(id);
    cancel_token(id);
}

/// Cancel the run registered for `id`. Does not walk linked child engines.
pub(crate) fn cancel_token(id: &str) {
    crate::mcp::cancel_open_tasks(id);
    let live = runs()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .remove(id);
    let Some(live) = live else {
        return;
    };
    live.cancel.cancel();
    let hook = {
        let mut slot = live.on_stop.lock().unwrap_or_else(|err| err.into_inner());
        slot.take()
    };
    if let Some(hook) = hook {
        hook();
    }
}

/// Stop one native session's stream and its background tasks. Not linked children.
pub fn cancel_session(id: &str) {
    let Ok(id) = safe_id(id) else {
        return;
    };
    crate::tasks::halt_session(id);
    cancel_token(id);
}

pub fn sessions_dir() -> PathBuf {
    crate::perm::config_dir().join("sessions")
}

/// `sessions/<id>/media`: files the native media tools download for this session.
/// Removed with the session.
pub fn media_dir(id: &str) -> Result<PathBuf, String> {
    let id = safe_id(id)?;
    Ok(sessions_dir().join(id).join("media"))
}

pub fn session_file(id: &str) -> Result<PathBuf, String> {
    let id = safe_id(id)?;
    Ok(sessions_dir().join(format!("{id}.jsonl")))
}

fn safe_id(id: &str) -> Result<&str, String> {
    let id = id.trim();
    if id.is_empty()
        || id.len() > 200
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.contains('\0')
    {
        return Err("bad session id".into());
    }
    Ok(id)
}

/// First line of the first user message, capped at 80 characters. No model call.
pub fn local_title(text: &str) -> String {
    let mut raw = text.trim();
    if let Some(rest) = raw.strip_prefix("<user_query>") {
        if let Some((body, _)) = rest.split_once("</user_query>") {
            raw = body.trim();
        }
    }
    let line = raw
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let title: String = collapsed.chars().take(80).collect();
    if title.is_empty() {
        "Chat".to_string()
    } else {
        title
    }
}

/// A cost in xAI ticks as dollars. A tick is 1e-10 USD (docs.x.ai cost
/// tracking), so whole cents show two places and smaller amounts keep two
/// significant digits, down to one tick.
pub fn format_cost_ticks(ticks: i64) -> String {
    const TICKS_PER_USD: u64 = 10_000_000_000;
    let sign = if ticks < 0 { "-" } else { "" };
    let ticks = ticks.unsigned_abs();
    let whole = ticks / TICKS_PER_USD;
    let frac = ticks % TICKS_PER_USD;
    // Ten places is one tick. Keep two places, or two significant digits when
    // the amount is under a cent.
    let digits = format!("{frac:010}");
    let places = if whole > 0 {
        2
    } else {
        match digits.find(|c: char| c != '0') {
            Some(first) => (first + 2).clamp(2, 10),
            None => 2,
        }
    };
    let mut frac_s = digits[..places].to_string();
    while frac_s.len() > 2 && frac_s.ends_with('0') {
        frac_s.pop();
    }
    format!("{sign}${whole}.{frac_s}")
}

pub fn usage_label(input: u64, output: u64, reasoning: u64, cost: i64, meter: &str) -> String {
    if input == 0 && output == 0 && reasoning == 0 && cost == 0 && meter.is_empty() {
        return String::new();
    }
    let mut line = format!("{input} in · {output} out");
    if reasoning > 0 {
        line.push_str(&format!(" · {reasoning} think"));
    }
    if cost != 0 {
        line.push_str(" · ");
        line.push_str(&format_cost_ticks(cost));
    }
    if !meter.is_empty() {
        line.push_str(" · ");
        line.push_str(meter);
    }
    line
}

/// CLI rows stay in their original order and stay read-only. Native rows follow them.
/// A native id that collides with a CLI id does not replace that CLI row.
pub fn merge_history(cli: &[GrokSession], native: &[SessionInfo]) -> Vec<HistoryRow> {
    let mut rows = Vec::with_capacity(cli.len() + native.len());
    let mut seen = HashSet::new();
    for session in cli {
        seen.insert(session.id.clone());
        rows.push(HistoryRow {
            id: session.id.clone(),
            title: session.title.clone(),
            path: session.path.clone(),
            cwd: session.cwd.clone(),
            read_only: true,
            native: false,
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            cost_in_usd_ticks: 0,
            meter: String::new(),
        });
    }
    for session in native {
        if !seen.insert(session.id.clone()) {
            continue;
        }
        rows.push(HistoryRow {
            id: session.id.clone(),
            title: session.title.clone(),
            path: session_file(&session.id).ok(),
            cwd: {
                let cwd = session.cwd.trim();
                if cwd.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(cwd))
                }
            },
            read_only: false,
            native: true,
            input_tokens: session.usage.input_tokens,
            output_tokens: session.usage.output_tokens,
            reasoning_tokens: session.usage.reasoning_tokens,
            cost_in_usd_ticks: session.usage.cost_in_usd_ticks,
            meter: session.meter.clone(),
        });
    }
    rows
}

/// Every session file with its id (the file name), in no order.
fn session_files() -> Vec<(PathBuf, String)> {
    let Ok(entries) = fs::read_dir(sessions_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                return None;
            }
            let id = path.file_stem().and_then(|stem| stem.to_str())?.to_string();
            safe_id(&id).ok()?;
            Some((path, id))
        })
        .collect()
}

/// Every session in full, newest first. Reads every turn of every file: a
/// list that only shows titles takes [`list_session_heads`].
pub fn list_sessions() -> Vec<SessionInfo> {
    let mut found: Vec<SessionInfo> = session_files()
        .into_iter()
        .filter_map(|(path, id)| read_session(&path, &id, false).ok())
        .collect();
    found.sort_by(|a, b| b.created.cmp(&a.created).then(a.id.cmp(&b.id)));
    found
}

/// The same sessions as [`list_sessions`], in the same order, with only each
/// file's first line read: the chat lists ask for these on every open.
pub fn list_session_heads() -> Vec<SessionHead> {
    let mut found: Vec<SessionHead> = session_files()
        .into_iter()
        .filter_map(|(path, id)| read_head(&path, &id))
        .collect();
    found.sort_by(|a, b| b.created.cmp(&a.created).then(a.id.cmp(&b.id)));
    found
}

/// The header [`read_session`] would take: the first line that is not
/// blank. A file with nothing after it but an unended line has none, as
/// `split_kept_lines` drops that torn line.
fn read_head(path: &Path, fallback_id: &str) -> Option<SessionHead> {
    let mut reader = BufReader::new(fs::File::open(path).ok()?);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).ok()? == 0 || line.pop() != Some(b'\n') {
            return None;
        }
        if !line.is_empty() {
            break;
        }
    }
    let value: Value = serde_json::from_str(&String::from_utf8_lossy(&line)).ok()?;
    parse_header(&value, fallback_id)
}

pub fn load_session(id: &str) -> Result<SessionInfo, String> {
    let path = session_file(id)?;
    read_session(&path, safe_id(id)?, true)
}

/// KB-03: the session file is there and still reads, with at least one turn,
/// so a resume on the same model can continue it instead of starting fresh.
pub fn session_intact(id: &str) -> bool {
    load_session(id).is_ok_and(|info| !info.input().is_empty())
}

pub fn resume_input(id: &str) -> Result<Vec<InputItem>, String> {
    Ok(load_session(id)?.input())
}

/// Create the file if needed and append this turn in one write and one sync.
/// A session that was just deleted is not written back.
pub fn record_turn(
    id: &str,
    cwd: &str,
    model: &str,
    items: &[InputItem],
    usage: &Usage,
    meter: &str,
    user_text: &str,
) -> Result<(), String> {
    record_turn_with(id, cwd, model, items, &[], usage, meter, user_text)
}

/// [`record_turn`] with the turn's thoughts, each saved before the tool call
/// it came before (or before the reply's text), and the values held for the
/// chat ([`crate::held`]) redacted from the thoughts and the reply's text.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_turn_with(
    id: &str,
    cwd: &str,
    model: &str,
    items: &[InputItem],
    thoughts: &[TurnThought],
    usage: &Usage,
    meter: &str,
    user_text: &str,
) -> Result<(), String> {
    let id = safe_id(id)?.to_string();
    if tombstoned(&id) {
        return Ok(());
    }
    touch(&id, cwd, model)?;
    // The turn's lines go out in one write and one sync: a sync per item cost
    // a 10-tool turn about two dozen fsyncs before Done.
    let mut lines = Vec::new();
    for value in with_thoughts(&id, items, thoughts)
        .iter()
        .map(record_json)
        .chain([usage_json(usage, meter, Some(grokhub_core::now_ms()))])
    {
        serde_json::to_writer(&mut lines, &value).map_err(|err| err.to_string())?;
        lines.push(b'\n');
    }
    if tombstoned(&id) {
        let _ = fs::remove_file(session_file(&id)?);
        return Ok(());
    }
    append_lines(&id, &lines)?;
    if !tombstoned(&id) {
        let _ = note_title(&id, user_text);
    }
    if tombstoned(&id) {
        let _ = fs::remove_file(session_file(&id)?);
        return Ok(());
    }
    bump_history();
    Ok(())
}

/// A turn's items as saved: each thought goes before the first tool call or
/// reply text that came after it, or at the end. The reply's text and the
/// thoughts lose the values held for chat `id`.
fn with_thoughts(id: &str, items: &[InputItem], thoughts: &[TurnThought]) -> Vec<Record> {
    let mut out = Vec::with_capacity(items.len() + thoughts.len());
    let mut thoughts = thoughts.iter().peekable();
    let (mut calls, mut texts) = (0, 0);
    for item in items {
        let call = matches!(item, InputItem::FunctionCall { .. });
        let text = matches!(item, InputItem::Message { role, .. } if role == "assistant");
        if call || text {
            while let Some(thought) = thoughts.next_if(|t| t.calls <= calls && t.texts <= texts) {
                out.extend(thought_record(id, &thought.text));
            }
        }
        calls += usize::from(call);
        texts += usize::from(text);
        out.push(Record::Item(redact_reply(id, item)));
    }
    for thought in thoughts {
        out.extend(thought_record(id, &thought.text));
    }
    out
}

fn thought_record(id: &str, text: &str) -> Option<Record> {
    let text = crate::held::redact(id, text);
    (!text.trim().is_empty()).then_some(Record::Thought(text))
}

/// An assistant message, a tool call's arguments and a tool's output with
/// the values held for chat `id` redacted; any other item as it is.
fn redact_reply(id: &str, item: &InputItem) -> InputItem {
    match item {
        InputItem::Message { role, content } if role == "assistant" => InputItem::Message {
            role: role.clone(),
            content: content
                .iter()
                .map(|part| match part {
                    ContentPart::InputText(text) => {
                        ContentPart::InputText(crate::held::redact(id, text))
                    }
                    other => other.clone(),
                })
                .collect(),
        },
        InputItem::FunctionCall {
            call_id,
            name,
            arguments,
        } => InputItem::FunctionCall {
            call_id: call_id.clone(),
            name: name.clone(),
            arguments: crate::held::redact(id, arguments),
        },
        InputItem::FunctionCallOutput { call_id, output } => InputItem::FunctionCallOutput {
            call_id: call_id.clone(),
            output: crate::held::redact(id, output),
        },
        other => other.clone(),
    }
}

/// Append a compaction marker and its usage. Resume rebuilds `input` from the marker,
/// then appends any items recorded after it. A deleted session is not written back.
pub fn record_compaction(
    id: &str,
    cwd: &str,
    model: &str,
    items: &[InputItem],
    usage: &Usage,
    meter: &str,
) -> Result<(), String> {
    write_compaction(id, cwd, model, items, &[], usage, meter, (0, 0))
}

/// [`record_compaction`] for a turn that compacted as it ran. `turn` is how
/// many of `items`, at the end, are the turn's own (saved only through the
/// compaction, so they take its time), and how many of its asks and replies
/// the compaction folded away.
///
/// The turn's thoughts are saved just before the compaction, with the
/// values held for the chat redacted, and show before the turn's own items.
#[allow(clippy::too_many_arguments)]
pub fn record_compacted_turn(
    id: &str,
    cwd: &str,
    model: &str,
    items: &[InputItem],
    turn: (usize, usize),
    thoughts: &[TurnThought],
    usage: &Usage,
    meter: &str,
) -> Result<(), String> {
    let (own, folded) = turn;
    let own = own.min(items.len());
    let start = items.len() - own;
    let mut items = items.to_vec();
    for item in &mut items[start..] {
        *item = redact_reply(id, item);
    }
    let thoughts: Vec<Record> = if own == 0 {
        Vec::new()
    } else {
        thoughts
            .iter()
            .filter_map(|t| thought_record(id, &t.text))
            .collect()
    };
    write_compaction(
        id,
        cwd,
        model,
        &items,
        &thoughts,
        usage,
        meter,
        (own, folded),
    )
}

#[allow(clippy::too_many_arguments)]
fn write_compaction(
    id: &str,
    cwd: &str,
    model: &str,
    items: &[InputItem],
    before: &[Record],
    usage: &Usage,
    meter: &str,
    turn: (usize, usize),
) -> Result<(), String> {
    let id = safe_id(id)?.to_string();
    if tombstoned(&id) {
        return Ok(());
    }
    touch(&id, cwd, model)?;
    if tombstoned(&id) {
        let _ = fs::remove_file(session_file(&id)?);
        return Ok(());
    }
    let mut lines = Vec::new();
    let at = Some(grokhub_core::now_ms());
    let values = before
        .iter()
        .map(record_json)
        .chain([compaction_json(items, turn), usage_json(usage, meter, at)]);
    for value in values {
        serde_json::to_writer(&mut lines, &value).map_err(|err| err.to_string())?;
        lines.push(b'\n');
    }
    append_lines(&id, &lines)?;
    if tombstoned(&id) {
        let _ = fs::remove_file(session_file(&id)?);
        return Ok(());
    }
    bump_history();
    Ok(())
}

/// Append a usage record with no items, for side calls such as `/dream`.
/// A deleted session is not written back.
pub fn record_usage(
    id: &str,
    cwd: &str,
    model: &str,
    usage: &Usage,
    meter: &str,
) -> Result<(), String> {
    let id = safe_id(id)?.to_string();
    if tombstoned(&id) {
        return Ok(());
    }
    touch(&id, cwd, model)?;
    if tombstoned(&id) {
        let _ = fs::remove_file(session_file(&id)?);
        return Ok(());
    }
    append_json(&id, &usage_json(usage, meter, None))?;
    if tombstoned(&id) {
        let _ = fs::remove_file(session_file(&id)?);
        return Ok(());
    }
    bump_history();
    Ok(())
}

/// Copy a session into a new id with `forked_from` set. No production caller:
/// desktop `/bg`/`/send` use a fresh session. Kept so tests can exercise
/// `delete_forks_of` (still used when deleting a chat).
pub fn fork_session(id: &str) -> Result<SessionInfo, String> {
    let mut info = load_session(id)?;
    let new_id = format!("native-{}", grokhub_core::uid("n"));
    info.forked_from = std::mem::replace(&mut info.id, new_id);
    info.created = grokhub_core::now_ms();
    write_session(&info)?;
    bump_history();
    Ok(info)
}

pub fn rename_session(id: &str, title: &str) -> Result<(), String> {
    let title = local_title(title);
    let id = safe_id(id)?.to_string();
    if tombstoned(&id) {
        return Ok(());
    }
    match load_session(&id) {
        Ok(mut info) => {
            info.title = title;
            info.renamed = true;
            write_session(&info)?;
        }
        Err(_) => {
            let info = SessionInfo {
                id: id.clone(),
                title,
                created: grokhub_core::now_ms(),
                cwd: String::new(),
                model: String::new(),
                renamed: true,
                usage: Usage::default(),
                meter: String::new(),
                forked_from: String::new(),
                records: Vec::new(),
            };
            write_session(&info)?;
        }
    }
    bump_history();
    Ok(())
}

/// Cancel any run bound to this id, then remove the file.
pub fn delete_session(id: &str) -> Result<(), String> {
    let id = safe_id(id)?.to_string();
    let path = session_file(&id)?;
    let workspace = read_head(&path, &id)
        .map(|head| {
            let cwd = head.cwd.trim();
            if cwd.is_empty() {
                PathBuf::from(".")
            } else {
                PathBuf::from(cwd)
            }
        })
        .unwrap_or_else(|| PathBuf::from("."));
    // SessionEnd / on_session_delete hooks need the session attached once.
    let _hooks = crate::hooks::attach(&id, &workspace, false);
    crate::hooks::on_session_delete(&id, &workspace);
    crate::outbound_hooks::emit_session(
        &crate::perm::config_dir(),
        "SessionEnd",
        &id,
        json!({"reason": "delete"}),
    );
    drop(_hooks);
    tombstone(&id);
    stop_run(&id);
    if path.is_file() {
        fs::remove_file(&path).map_err(|err| err.to_string())?;
    }
    let media = sessions_dir().join(&id);
    if media.is_dir() {
        let _ = fs::remove_dir_all(&media);
    }
    crate::tasks::forget_session(&id);
    bump_history();
    Ok(())
}

/// Delete every saved session forked from one of `ids`, and the forks of
/// those forks. Returns the ids it deleted. Only the header line of each
/// file is read.
pub fn delete_forks_of(ids: &[String]) -> Vec<String> {
    let mut parents: HashSet<String> = ids.iter().map(|id| id.trim().to_string()).collect();
    parents.retain(|id| !id.is_empty());
    let mut deleted = Vec::new();
    loop {
        let found: Vec<String> = fork_headers()
            .into_iter()
            .filter(|(id, from)| parents.contains(from) && !parents.contains(id))
            .map(|(id, _)| id)
            .collect();
        if found.is_empty() {
            return deleted;
        }
        for id in found {
            if delete_session(&id).is_ok() {
                deleted.push(id.clone());
            }
            parents.insert(id);
        }
    }
}

/// `(id, forked_from)` for every saved session that was forked.
fn fork_headers() -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(sessions_dir()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(file) = fs::File::open(&path) else {
            continue;
        };
        let mut first = String::new();
        if std::io::BufRead::read_line(&mut std::io::BufReader::new(file), &mut first).is_err() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(first.trim()) else {
            continue;
        };
        if let Some(header) = parse_header(&value, stem) {
            if !header.forked_from.is_empty() {
                out.push((header.id, header.forked_from));
            }
        }
    }
    out
}

/// A message the person deleted from a chat. The ask is found by its text;
/// `later` counts the same text asked again after it, so a message hidden by
/// a compaction never shifts which one is meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forget<'a> {
    /// That user message.
    Ask { text: &'a str, later: usize },
    /// The reply after that user message: its assistant messages, tool calls
    /// and tool results, up to the next user message. `None` is the reply
    /// before any user message.
    Reply { ask: Option<(&'a str, usize)> },
}

/// What [`forget_message`] did to the saved session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forgotten {
    /// The records are gone; the model won't read them again.
    Removed,
    /// Nothing saved matched: the message never reached the model, or it is
    /// already folded into an earlier compaction's summary.
    NotSaved,
}

/// The reply still running in this chat: deleting under it would race its writes.
pub const FORGET_BUSY: &str = "Wait for the reply to finish, then delete the message.";

/// Take one message out of a saved session so the model never reads it again.
/// Records kept by an earlier compaction are searched too; a call and its
/// output always go together. Refuses while a run is live on `id`.
pub fn forget_message(id: &str, what: Forget<'_>) -> Result<Forgotten, String> {
    let id = safe_id(id)?.to_string();
    if runs()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .contains_key(&id)
    {
        return Err(FORGET_BUSY.into());
    }
    if !session_file(&id)?.is_file() {
        return Ok(Forgotten::NotSaved);
    }
    let mut info = load_session(&id)?;
    let visible = visible_slots(&info.records);
    // An ask goes out as typed, or with a note the app put before or after it
    // (a resume recap, a skill's follow-up). Exact matches win; the wrapped
    // ones count only when there aren't enough exact ones to reach `later`.
    let asks: Vec<(usize, String)> = visible
        .iter()
        .enumerate()
        .filter_map(|(i, (_, item))| user_text(item).map(|t| (i, squash(&t))))
        .collect();
    let ask_at = |text: &str, later: usize| -> Option<usize> {
        let want = squash(text);
        if want.is_empty() {
            return None;
        }
        let pick = |hit: &dyn Fn(&str) -> bool| {
            asks.iter()
                .filter(|(_, t)| hit(t))
                .map(|(i, _)| *i)
                .rev()
                .nth(later)
        };
        pick(&|t| t == want).or_else(|| pick(&|t| wraps(t, &want)))
    };
    let doomed: Vec<Slot> = match what {
        Forget::Ask { text, later } => match ask_at(text, later) {
            Some(i) => vec![visible[i].0],
            None => Vec::new(),
        },
        Forget::Reply { ask } => {
            let start = match ask {
                Some((text, later)) => ask_at(text, later).map(|i| i + 1),
                None => Some(0),
            };
            match start {
                Some(start) => {
                    let mut reply: Vec<Slot> = visible[start..]
                        .iter()
                        .take_while(|(_, item)| user_text(item).is_none())
                        .filter(|(_, item)| is_reply_item(item))
                        .map(|(slot, _)| *slot)
                        .collect();
                    let ask = start.checked_sub(1).map(|i| visible[i].0);
                    let thoughts = reply_thoughts(&info.records, ask, &reply);
                    reply.extend(thoughts);
                    reply
                }
                None => Vec::new(),
            }
        }
    };
    if doomed.is_empty() {
        return Ok(Forgotten::NotSaved);
    }
    drop_slots(&mut info.records, &doomed);
    write_session(&info)?;
    bump_history();
    Ok(Forgotten::Removed)
}

/// Where a visible item sits: a top-level record, or item `j` of the
/// compaction record at `i`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Top(usize),
    Kept(usize, usize),
}

/// The items the model reads, in order, with where each one is stored.
fn visible_slots(records: &[Record]) -> Vec<(Slot, InputItem)> {
    let mut out = Vec::new();
    for (i, record) in records.iter().enumerate() {
        match record {
            Record::Compaction { items, .. } => {
                out.clear();
                for (j, item) in items.iter().enumerate() {
                    out.push((Slot::Kept(i, j), item.clone()));
                }
            }
            Record::Item(item) => out.push((Slot::Top(i), item.clone())),
            Record::Usage { .. } | Record::Thought(_) => {}
        }
    }
    out
}

/// The saved thoughts of the reply whose items are `reply`, after `ask`: the
/// ones from the ask to the line that closes the turn, or, for a reply a
/// compaction kept, the ones saved just before that compaction.
fn reply_thoughts(records: &[Record], ask: Option<Slot>, reply: &[Slot]) -> Vec<Slot> {
    let at = |slot: Slot| match slot {
        Slot::Top(i) | Slot::Kept(i, _) => i,
    };
    let Some(last) = reply.iter().copied().map(at).max() else {
        return Vec::new();
    };
    let usage = |r: &Record| matches!(r, Record::Usage { .. });
    let range = if matches!(records.get(last), Some(Record::Compaction { .. })) {
        let from = records[..last].iter().rposition(usage).map_or(0, |u| u + 1);
        from..last
    } else {
        let from = ask.map_or(0, |slot| at(slot) + 1);
        let end = records[last..]
            .iter()
            .position(usage)
            .map_or(records.len(), |u| last + u);
        from..end
    };
    range
        .filter(|&i| matches!(records[i], Record::Thought(_)))
        .map(Slot::Top)
        .collect()
}

fn drop_slots(records: &mut Vec<Record>, doomed: &[Slot]) {
    let mut kept: Vec<(usize, Vec<usize>)> = Vec::new();
    for slot in doomed {
        if let Slot::Kept(i, j) = *slot {
            match kept.iter_mut().find(|(at, _)| *at == i) {
                Some((_, js)) => js.push(j),
                None => kept.push((i, vec![j])),
            }
        }
    }
    for (i, js) in kept {
        if let Some(Record::Compaction { items, own, .. }) = records.get_mut(i) {
            let first_own = items.len() - *own;
            *own -= js.iter().filter(|&&j| j >= first_own).count();
            let mut j = 0;
            items.retain(|_| {
                let keep = !js.contains(&j);
                j += 1;
                keep
            });
        }
    }
    let mut i = 0;
    records.retain(|_| {
        let keep = !doomed.contains(&Slot::Top(i));
        i += 1;
        keep
    });
}

/// The person's words in a user message; `None` for anything else.
fn user_text(item: &InputItem) -> Option<String> {
    match item {
        InputItem::Message { role, content } if role == "user" => {
            let text = visible_text(role, content);
            (!text.trim().is_empty()).then_some(text)
        }
        _ => None,
    }
}

/// Part of a reply: what the model said and the tools it ran. System notes
/// and xAI's opaque compaction items stay.
fn is_reply_item(item: &InputItem) -> bool {
    match item {
        InputItem::Message { role, .. } => role == "assistant",
        InputItem::FunctionCall { .. }
        | InputItem::FunctionCallOutput { .. }
        | InputItem::Reasoning { .. } => true,
        InputItem::ServerCompaction { .. } => false,
    }
}

/// `sent` is `ask` with a note before or after it, on a word boundary.
fn wraps(sent: &str, ask: &str) -> bool {
    sent == ask
        || sent.starts_with(&format!("{ask} "))
        || sent.ends_with(&format!(" {ask}"))
        || sent.contains(&format!(" {ask} "))
}

/// Text compared for a match: whitespace runs read as one space.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn export_markdown(id: &str) -> Result<String, String> {
    let info = load_session(id)?;
    let mut out = format!("# {}\n", info.title);
    for record in &visible_records(&info.records) {
        match record {
            Record::Item(InputItem::Message { role, .. }) if role == "system" => {}
            Record::Item(InputItem::Message { role, content }) => {
                let heading = if role == "assistant" {
                    "Assistant"
                } else {
                    "User"
                };
                out.push_str(&format!(
                    "\n## {heading}\n\n{}\n",
                    visible_text(role, content)
                ));
            }
            Record::Item(InputItem::FunctionCall {
                name, arguments, ..
            }) => {
                out.push_str(&format!("\n## Tool {name}\n\n{arguments}\n"));
            }
            Record::Item(InputItem::FunctionCallOutput { output, .. }) => {
                out.push_str(&format!("\n## Tool result\n\n{output}\n"));
            }
            Record::Item(InputItem::ServerCompaction { .. }) => {
                out.push_str(&format!("\n_{COMPACTED_NOTE}_\n"));
            }
            Record::Item(InputItem::Reasoning { .. }) => {}
            Record::Usage { usage, meter, .. } => {
                let line = usage_label(
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.reasoning_tokens,
                    usage.cost_in_usd_ticks,
                    meter,
                );
                if !line.is_empty() {
                    out.push_str(&format!("\n## Usage\n\n{line}\n"));
                }
            }
            Record::Compaction { .. } | Record::Thought(_) => {}
        }
    }
    Ok(out)
}

pub fn transcript_pairs(info: &SessionInfo) -> Vec<(String, String)> {
    pairs_of(info, true, false)
}

/// [`transcript_pairs`] with the saved thoughts where they were shown, as
/// `("thought", text)`: what the chat view replays.
pub fn shown_pairs(info: &SessionInfo) -> Vec<(String, String)> {
    pairs_of(info, true, true)
}

/// [`transcript_pairs`] without the tool calls and their results: only
/// what was said.
pub fn message_pairs(info: &SessionInfo) -> Vec<(String, String)> {
    pairs_of(info, false, false)
}

/// A message [`transcript_pairs`] shows as the user's.
fn is_shown_ask(item: &InputItem) -> bool {
    matches!(item, InputItem::Message { role, content }
        if role != "system" && role != "assistant" && !visible_text(role, content).trim().is_empty())
}

/// A message [`transcript_pairs`] shows at all.
fn is_shown_message(item: &InputItem) -> bool {
    matches!(item, InputItem::Message { role, content }
        if role != "system" && !visible_text(role, content).trim().is_empty())
}

/// The transcript replayed record by record: what is in view, with each
/// message's time, and how many messages compactions took out of view.
struct Replay {
    visible: Vec<Seen>,
    hidden: usize,
}

struct Seen {
    item: InputItem,
    at: Option<u64>,
}

/// Replays the records. A message gets the time of the usage line that closes
/// its turn. A compaction keeps some messages it replaced (the last ask, maybe
/// its reply): each kept message is matched, newest first and in order, to a
/// replaced message that shows the same ([`message_key`]), and keeps that
/// one's time. The rest (its summary and rules) take the compaction's own
/// time. So do the own items of a turn that compacted as it ran, which were
/// never saved on their own, even when an older ask reads the same.
fn replay(records: &[Record]) -> Replay {
    let mut visible: Vec<Seen> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut hidden = 0;
    for (i, record) in records.iter().enumerate() {
        match record {
            Record::Item(item) => {
                open.push(visible.len());
                visible.push(Seen {
                    item: item.clone(),
                    at: None,
                });
            }
            Record::Usage { at, .. } => open.drain(..).for_each(|j| visible[j].at = *at),
            Record::Thought(_) => {}
            Record::Compaction { items, own, folded } => {
                let at = match records.get(i + 1) {
                    Some(Record::Usage { at, .. }) => *at,
                    _ => None,
                };
                let prior = std::mem::take(&mut visible);
                visible = items
                    .iter()
                    .map(|item| Seen {
                        item: item.clone(),
                        at,
                    })
                    .collect();
                open.clear();
                let before = shown(&prior);
                let before_keys: Vec<_> = before
                    .iter()
                    .map(|&p| message_key(&prior[p].item))
                    .collect();
                // The kept messages that came from before: all but the
                // turn's own ones at the end.
                let first_own = items.len() - own.min(&items.len());
                let after: Vec<usize> = shown(&visible)
                    .into_iter()
                    .filter(|&k| k < first_own)
                    .collect();
                let mut left = before.len();
                let mut matched = 0;
                for &k in after.iter().rev() {
                    let key = message_key(&visible[k].item);
                    if let Some(at) = before_keys[..left].iter().rposition(|p| *p == key) {
                        visible[k].at = prior[before[at]].at;
                        left = at;
                        matched += usize::from(is_counted(&prior[before[at]].item));
                    }
                }
                let counted = before.iter().filter(|&&p| is_counted(&prior[p].item));
                hidden += counted.count() - matched + folded;
            }
        }
    }
    Replay { visible, hidden }
}

/// A message as the transcript shows it: its role and visible text, with an
/// image that was later swapped for the removed-image note still an image.
/// Two copies of a message match on this, whatever images the model's copy
/// has lost since it was saved.
fn message_key(item: &InputItem) -> Option<(String, String)> {
    let InputItem::Message { role, content } = item else {
        return None;
    };
    let content: Vec<ContentPart> = content
        .iter()
        .map(|part| match part {
            ContentPart::InputText(text) if text == crate::image_budget::OLD_IMAGE_PLACEHOLDER => {
                ContentPart::InputImage(String::new())
            }
            other => other.clone(),
        })
        .collect();
    Some((role.clone(), visible_text(role, &content)))
}

/// Whether two items are copies of one: equal, or messages that show the same
/// ([`message_key`]).
pub(crate) fn shows_same(a: &InputItem, b: &InputItem) -> bool {
    match (message_key(a), message_key(b)) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

/// Where the messages [`transcript_pairs`] shows sit in `items`.
fn shown(items: &[Seen]) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, seen)| is_shown_message(&seen.item))
        .map(|(i, _)| i)
        .collect()
}

/// A user message in [`transcript_pairs`]: when it was answered, and what
/// it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserLine {
    /// The time of the turn it was saved with (see [`replay`]). `None` for
    /// turns saved before usage lines carried a time. A turn whose usage line
    /// was lost to a torn write takes the next turn's time.
    pub at: Option<u64>,
    pub kind: UserKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserKind {
    /// Something the person (or the app on their behalf) sent.
    Ask,
    /// The image a tool returned, sent as a user message after its result.
    ToolImage,
    /// A note the app added for the model (a cabin note, a helper's notice).
    Note,
    /// A compaction's summary, ledger or rules.
    Compaction,
}

/// Every user message in [`transcript_pairs`], in order.
pub fn user_lines(info: &SessionInfo) -> Vec<UserLine> {
    let visible = replay(&info.records).visible;
    visible
        .iter()
        .enumerate()
        .filter(|(_, seen)| is_shown_ask(&seen.item))
        .map(|(_, seen)| {
            let kind = if crate::compact::is_compaction_message(&seen.item) {
                UserKind::Compaction
            } else if is_tool_image(&seen.item) {
                UserKind::ToolImage
            } else if crate::compact::is_app_note(&seen.item) {
                UserKind::Note
            } else {
                UserKind::Ask
            };
            UserLine { at: seen.at, kind }
        })
        .collect()
}

/// The message the run adds after a tool result that carried an image:
/// "image from {tool}" and the image, or the note that replaced it. Known by
/// its shape alone, since a compaction can keep it without the result
/// before it.
fn is_tool_image(item: &InputItem) -> bool {
    let InputItem::Message { role, content } = item else {
        return false;
    };
    let tool_name = |name: &str| {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    };
    match content.as_slice() {
        [ContentPart::InputText(label), image] if role == "user" => {
            label.strip_prefix("image from ").is_some_and(tool_name)
                && match image {
                    ContentPart::InputImage(_) => true,
                    ContentPart::InputText(text) => {
                        text == crate::image_budget::OLD_IMAGE_PLACEHOLDER
                    }
                }
        }
        _ => false,
    }
}

/// A message the transcript counts: an ask or a reply, not a tool's image,
/// a note the app added, or a compaction's own text.
pub(crate) fn is_counted(item: &InputItem) -> bool {
    is_shown_message(item)
        && !is_tool_image(item)
        && !crate::compact::is_app_note(item)
        && !crate::compact::is_compaction_message(item)
}

/// How many asks and replies compactions took out of the transcript, less
/// the ones they kept.
pub fn compacted_messages(info: &SessionInfo) -> usize {
    replay(&info.records).hidden
}

fn pairs_of(info: &SessionInfo, tools: bool, thoughts: bool) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for record in &visible_records(&info.records) {
        match record {
            Record::Item(InputItem::Message { role, content }) if role != "system" => {
                let text = visible_text(role, content);
                if text.trim().is_empty() {
                    continue;
                }
                let role = if role == "assistant" {
                    "assistant"
                } else {
                    "user"
                };
                pairs.push((role.to_string(), text));
            }
            Record::Item(InputItem::FunctionCall {
                name, arguments, ..
            }) if tools => {
                pairs.push(("assistant".into(), format!("Tool {name}\n{arguments}")));
            }
            Record::Item(InputItem::FunctionCallOutput { output, .. }) if tools => {
                pairs.push(("assistant".into(), format!("Tool result\n{output}")));
            }
            Record::Thought(text) if thoughts => pairs.push(("thought".into(), text.clone())),
            _ => {}
        }
    }
    pairs
}

/// A message as the person saw it: a user turn loses the `<cabin-volatile>`
/// notes the cabin put in front of it for the model, and a shell line sent
/// from `/sh`, `$ ` or the Command page shows as the line typed.
fn visible_text(role: &str, content: &[ContentPart]) -> String {
    let text = message_text(content);
    if role != "user" {
        return text;
    }
    let shown = crate::prefix_cache::visible_user_text(&text);
    match grokhub_core::shell_line::command_of(shown) {
        Some(command) => format!("$ {command}"),
        None => shown.to_string(),
    }
}

fn message_text(content: &[ContentPart]) -> String {
    let mut parts = Vec::new();
    for part in content {
        match part {
            ContentPart::InputText(text) => {
                if !text.is_empty() {
                    parts.push(text.as_str());
                }
            }
            ContentPart::InputImage(_) => parts.push("[image]"),
        }
    }
    parts.join("\n")
}

fn default_title(title: &str, id: &str) -> bool {
    let title = title.trim();
    title.is_empty() || title == "Chat" || title == id
}

fn note_title(id: &str, user_text: &str) -> Result<(), String> {
    if user_text.trim().is_empty() || tombstoned(id) {
        return Ok(());
    }
    let mut info = load_session(id)?;
    if info.renamed || !default_title(&info.title, id) {
        return Ok(());
    }
    let title = local_title(user_text);
    if title == info.title {
        return Ok(());
    }
    info.title = title;
    if tombstoned(id) {
        return Ok(());
    }
    write_session(&info)
}

fn touch(id: &str, cwd: &str, model: &str) -> Result<(), String> {
    if tombstoned(id) {
        return Ok(());
    }
    let path = session_file(id)?;
    if path.is_file() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let header = header_json(&SessionHead {
        id: id.to_string(),
        title: "Chat".into(),
        created: grokhub_core::now_ms(),
        cwd: cwd.to_string(),
        model: model.to_string(),
        renamed: false,
        forked_from: String::new(),
    });
    let mut bytes = serde_json::to_vec(&header).map_err(|err| err.to_string())?;
    bytes.push(b'\n');
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(&bytes).map_err(|err| err.to_string())?;
            file.flush().map_err(|err| err.to_string())?;
            let _ = file.sync_all();
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

fn append_json(id: &str, value: &Value) -> Result<(), String> {
    let mut line = serde_json::to_vec(value).map_err(|err| err.to_string())?;
    line.push(b'\n');
    append_lines(id, &line)
}

/// Append whole `\n`-ended lines in one write, then sync once.
fn append_lines(id: &str, lines: &[u8]) -> Result<(), String> {
    if tombstoned(id) {
        return Ok(());
    }
    let path = session_file(id)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| err.to_string())?;
    file.write_all(lines).map_err(|err| err.to_string())?;
    file.flush().map_err(|err| err.to_string())?;
    let _ = file.sync_all();
    Ok(())
}

fn write_session(info: &SessionInfo) -> Result<(), String> {
    if tombstoned(&info.id) {
        return Ok(());
    }
    let path = session_file(&info.id)?;
    let mut bytes = Vec::new();
    push_line(
        &mut bytes,
        &header_json(&SessionHead {
            id: info.id.clone(),
            title: info.title.clone(),
            created: info.created,
            cwd: info.cwd.clone(),
            model: info.model.clone(),
            renamed: info.renamed,
            forked_from: info.forked_from.clone(),
        }),
    )?;
    for record in &info.records {
        push_line(&mut bytes, &record_json(record))?;
    }
    replace_file(&path, &bytes)
}

fn push_line(buf: &mut Vec<u8>, value: &Value) -> Result<(), String> {
    let line = serde_json::to_vec(value).map_err(|err| err.to_string())?;
    buf.extend_from_slice(&line);
    buf.push(b'\n');
    Ok(())
}

fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "session path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("session.jsonl");
    let tmp = parent.join(format!(".{name}.tmp"));
    {
        let mut file = fs::File::create(&tmp).map_err(|err| err.to_string())?;
        file.write_all(bytes).map_err(|err| err.to_string())?;
        file.flush().map_err(|err| err.to_string())?;
        let _ = file.sync_all();
    }
    fs::rename(&tmp, path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        err.to_string()
    })
}

fn read_session(path: &Path, fallback_id: &str, repair: bool) -> Result<SessionInfo, String> {
    let raw = fs::read(path).map_err(|err| err.to_string())?;
    if raw.is_empty() {
        return Err("empty session".into());
    }
    let (lines, torn) = split_kept_lines(&raw);
    if lines.is_empty() {
        if repair {
            let _ = replace_file(path, b"");
        }
        return Err("empty session".into());
    }
    let header_value: Value = serde_json::from_str(&lines[0]).map_err(|err| err.to_string())?;
    let header = parse_header(&header_value, fallback_id)
        .ok_or_else(|| "session header is missing".to_string())?;
    let mut records = Vec::new();
    for line in lines.iter().skip(1) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(record) = parse_record(&value) {
            records.push(record);
        }
    }
    if repair && torn {
        let info = assemble(header, records);
        write_session(&info)?;
        return Ok(info);
    }
    Ok(assemble(header, records))
}

fn assemble(header: SessionHead, records: Vec<Record>) -> SessionInfo {
    let mut usage = Usage::default();
    let mut meter = String::new();
    for record in &records {
        if let Record::Usage {
            usage: turn,
            meter: turn_meter,
            ..
        } = record
        {
            usage.add(turn);
            if !turn_meter.is_empty() {
                meter = turn_meter.clone();
            }
        }
    }
    SessionInfo {
        id: header.id,
        title: header.title,
        created: header.created,
        cwd: header.cwd,
        model: header.model,
        renamed: header.renamed,
        usage,
        meter,
        forked_from: header.forked_from,
        records,
    }
}

/// Drop a torn last line. `torn` is true when the file should be rewritten.
fn split_kept_lines(raw: &[u8]) -> (Vec<String>, bool) {
    let mut repair = !raw.ends_with(b"\n");
    let text = String::from_utf8_lossy(raw);
    let mut parts: Vec<&str> = text.split('\n').collect();
    if repair || parts.last().is_some_and(|line| line.is_empty()) {
        parts.pop();
    }
    if parts
        .last()
        .is_some_and(|line| serde_json::from_str::<Value>(line).is_err())
    {
        parts.pop();
        repair = true;
    }
    let lines = parts
        .into_iter()
        .map(str::to_string)
        .filter(|line| !line.is_empty())
        .collect();
    (lines, repair)
}

fn parse_header(value: &Value, fallback_id: &str) -> Option<SessionHead> {
    if value.get("engine").and_then(|engine| engine.as_str()) != Some(ENGINE) {
        return None;
    }
    let id = string_field(value, "id");
    let id = if id.is_empty() {
        fallback_id.to_string()
    } else {
        id
    };
    let title = string_field(value, "title");
    Some(SessionHead {
        id,
        title: if title.is_empty() {
            "Chat".into()
        } else {
            title
        },
        created: u64_field(value, "created"),
        cwd: string_field(value, "cwd"),
        model: string_field(value, "model"),
        renamed: value
            .get("renamed")
            .and_then(|flag| flag.as_bool())
            .unwrap_or(false),
        forked_from: string_field(value, "forkedFrom"),
    })
}

fn parse_record(value: &Value) -> Option<Record> {
    match value.get("type").and_then(|kind| kind.as_str())? {
        "message" => {
            let role = string_field(value, "role");
            let role = if role.is_empty() { "user".into() } else { role };
            let mut content = Vec::new();
            if let Some(parts) = value.get("content").and_then(|content| content.as_array()) {
                for part in parts {
                    match part.get("type").and_then(|kind| kind.as_str()) {
                        Some("input_text") => {
                            content.push(ContentPart::InputText(string_field(part, "text")));
                        }
                        Some("input_image") => {
                            content.push(ContentPart::InputImage(string_field(part, "image_url")));
                        }
                        _ => {}
                    }
                }
            }
            Some(Record::Item(InputItem::Message { role, content }))
        }
        "function_call" => Some(Record::Item(InputItem::FunctionCall {
            call_id: string_field(value, "call_id"),
            name: string_field(value, "name"),
            arguments: string_field(value, "arguments"),
        })),
        "function_call_output" => Some(Record::Item(InputItem::FunctionCallOutput {
            call_id: string_field(value, "call_id"),
            output: string_field(value, "output"),
        })),
        REASONING_RECORD => value
            .get("raw")
            .and_then(Value::as_str)
            .and_then(|raw| InputItem::reasoning(raw, &string_field(value, "model")))
            .map(Record::Item),
        SERVER_COMPACTION_RECORD => value
            .get("raw")
            .and_then(Value::as_str)
            .and_then(InputItem::server_compaction)
            .map(|item| Record::Item(item.with_digest(&string_field(value, "digest")))),
        "usage" => Some(Record::Usage {
            usage: Usage {
                input_tokens: u64_field(value, "input_tokens"),
                output_tokens: u64_field(value, "output_tokens"),
                reasoning_tokens: u64_field(value, "reasoning_tokens"),
                cost_in_usd_ticks: i64_field(value, "cost_in_usd_ticks"),
                cached_tokens: u64_field(value, "cached_tokens"),
            },
            meter: string_field(value, "meter"),
            at: value.get("at").and_then(Value::as_u64),
        }),
        "compaction" => {
            let mut items = Vec::new();
            if let Some(rows) = value.get("items").and_then(|rows| rows.as_array()) {
                for row in rows {
                    if let Some(Record::Item(item)) = parse_record(row) {
                        items.push(item);
                    }
                }
            }
            let count = |key: &str| {
                let n = value.get(key).and_then(Value::as_u64).unwrap_or(0);
                usize::try_from(n).unwrap_or(usize::MAX)
            };
            let own = count("own").min(items.len());
            let folded = count("folded");
            Some(Record::Compaction { items, own, folded })
        }
        THOUGHT_RECORD => Some(Record::Thought(string_field(value, "text"))),
        _ => None,
    }
}

fn replay_items(records: &[Record]) -> Vec<InputItem> {
    let mut items = Vec::new();
    for record in records {
        match record {
            Record::Item(item) => items.push(item.clone()),
            Record::Usage { .. } | Record::Thought(_) => {}
            Record::Compaction { items: rebuilt, .. } => items = rebuilt.clone(),
        }
    }
    items
}

/// Items before a compaction marker drop out of the transcript. Usage lines stay.
fn visible_records(records: &[Record]) -> Vec<Record> {
    let mut out = Vec::new();
    for record in records {
        match record {
            Record::Compaction { items, own, .. } => {
                // The compacting turn's thoughts, saved just before it.
                let since = out
                    .iter()
                    .rposition(|entry| matches!(entry, Record::Usage { .. }))
                    .map_or(0, |at| at + 1);
                let thoughts: Vec<Record> = if *own == 0 {
                    Vec::new()
                } else {
                    out[since..]
                        .iter()
                        .filter(|entry| matches!(entry, Record::Thought(_)))
                        .cloned()
                        .collect()
                };
                out.retain(|entry| matches!(entry, Record::Usage { .. }));
                let first_own = items.len() - own;
                for (j, item) in items.iter().enumerate() {
                    if j == first_own {
                        out.extend(thoughts.iter().cloned());
                    }
                    out.push(Record::Item(item.clone()));
                }
            }
            other => out.push(other.clone()),
        }
    }
    out
}

fn header_json(header: &SessionHead) -> Value {
    let mut value = json!({
        "id": header.id,
        "title": header.title,
        "created": header.created,
        "cwd": header.cwd,
        "model": header.model,
        "engine": ENGINE,
        "renamed": header.renamed,
    });
    if !header.forked_from.is_empty() {
        value["forkedFrom"] = json!(header.forked_from);
    }
    value
}

fn record_json(record: &Record) -> Value {
    match record {
        Record::Item(item) => item_json(item),
        Record::Usage { usage, meter, at } => usage_json(usage, meter, *at),
        Record::Compaction { items, own, folded } => compaction_json(items, (*own, *folded)),
        Record::Thought(text) => json!({"type": THOUGHT_RECORD, "text": text}),
    }
}

fn item_json(item: &InputItem) -> Value {
    match item {
        InputItem::Message { role, content } => {
            let parts: Vec<Value> = content
                .iter()
                .map(|part| match part {
                    ContentPart::InputText(text) => json!({"type": "input_text", "text": text}),
                    ContentPart::InputImage(url) => {
                        json!({"type": "input_image", "image_url": url})
                    }
                })
                .collect();
            json!({"type": "message", "role": role, "content": parts})
        }
        InputItem::FunctionCall {
            call_id,
            name,
            arguments,
        } => json!({
            "type": "function_call",
            "call_id": call_id,
            "name": name,
            "arguments": arguments,
        }),
        InputItem::FunctionCallOutput { call_id, output } => json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": output,
        }),
        // Its own type: `compaction` is this file's marker line. The item stays
        // the exact text xAI sent, as a string.
        InputItem::ServerCompaction { raw, digest } => {
            let mut record = json!({
                "type": SERVER_COMPACTION_RECORD,
                "raw": raw,
            });
            if !digest.is_empty() {
                record["digest"] = json!(digest);
            }
            record
        }
        // Same idea: the sealed item as the exact text xAI sent, and its model.
        InputItem::Reasoning { raw, model } => json!({
            "type": REASONING_RECORD,
            "raw": raw,
            "model": model,
        }),
    }
}

fn compaction_json(items: &[InputItem], (own, folded): (usize, usize)) -> Value {
    let mut value = json!({
        "type": "compaction",
        "items": items.iter().map(item_json).collect::<Vec<_>>(),
    });
    if own > 0 {
        value["own"] = json!(own);
    }
    if folded > 0 {
        value["folded"] = json!(folded);
    }
    value
}

fn usage_json(usage: &Usage, meter: &str, at: Option<u64>) -> Value {
    let mut value = json!({
        "type": "usage",
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "reasoning_tokens": usage.reasoning_tokens,
        "cost_in_usd_ticks": usage.cost_in_usd_ticks,
        "cached_tokens": usage.cached_tokens,
        "meter": meter,
    });
    if let Some(at) = at {
        value["at"] = json!(at);
    }
    value
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .unwrap_or("")
        .to_string()
}

fn u64_field(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(|field| field.as_u64()).unwrap_or(0)
}

fn i64_field(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(|field| field.as_i64()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    fn isolated(label: &str) -> (PathBuf, crate::perm::ConfigGuard) {
        let dir = std::env::temp_dir().join(format!(
            "gh-native-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let guard = crate::perm::ConfigGuard::set(&dir);
        (dir, guard)
    }

    fn user(text: &str) -> InputItem {
        InputItem::Message {
            role: "user".into(),
            content: vec![
                ContentPart::InputText(text.into()),
                ContentPart::InputImage("data:image/png;base64,QQ==".into()),
            ],
        }
    }

    /// A summary as the compaction writes it.
    fn summary_of(text: &str) -> InputItem {
        user(&format!(
            "This session is being continued from a previous conversation that ran out of \
             context. The summary below covers the earlier portion of the conversation.\n\n\
             <conversation_summary>\n{text}\n</conversation_summary>"
        ))
    }

    fn assistant(text: &str) -> InputItem {
        InputItem::Message {
            role: "assistant".into(),
            content: vec![ContentPart::InputText(text.into())],
        }
    }

    fn sample_items() -> Vec<InputItem> {
        vec![
            user("fix the dock"),
            assistant("On it."),
            InputItem::FunctionCall {
                call_id: "c1".into(),
                name: "read_file".into(),
                arguments: r#"{"target_file":"note.txt"}"#.into(),
            },
            InputItem::FunctionCallOutput {
                call_id: "c1".into(),
                output: "hello".into(),
            },
        ]
    }

    #[test]
    fn round_trip_write_read_resume_input() {
        let (_dir, _guard) = isolated("round");
        let id = "native-round";
        let usage = Usage {
            input_tokens: 12,
            output_tokens: 4,
            reasoning_tokens: 2,
            cost_in_usd_ticks: 84_000,
            cached_tokens: 0,
        };
        let items = sample_items();
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &items,
            &usage,
            "SuperGrok pool",
            "fix the dock",
        )
        .unwrap();
        let loaded = load_session(id).unwrap();
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.title, "fix the dock");
        assert_eq!(loaded.cwd, "/work");
        assert_eq!(loaded.model, "grok-4.7");
        assert_eq!(loaded.usage, usage);
        assert_eq!(loaded.meter, "SuperGrok pool");
        let raw = fs::read_to_string(session_file(id).unwrap()).unwrap();
        let header: Value = serde_json::from_str(raw.lines().next().unwrap()).unwrap();
        assert_eq!(header["engine"], json!("native"));
        assert!(raw.ends_with('\n'));
        assert_eq!(resume_input(id).unwrap(), items);
        assert_eq!(loaded.input(), items);
    }

    #[test]
    fn a_turn_appends_its_items_then_its_usage_one_line_each_in_order() {
        let (_dir, _guard) = isolated("lines");
        let id = "native-lines";
        let items = sample_items();
        let usage = Usage {
            input_tokens: 3,
            ..Usage::default()
        };
        let before = grokhub_core::now_ms();
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &items,
            &usage,
            "pool",
            "fix the dock",
        )
        .unwrap();
        let first = fs::read_to_string(session_file(id).unwrap()).unwrap();
        record_turn(id, "/work", "grok-4.7", &items, &usage, "pool", "again").unwrap();
        let after = grokhub_core::now_ms();
        let raw = fs::read_to_string(session_file(id).unwrap()).unwrap();
        let expected: Vec<Value> = items
            .iter()
            .map(item_json)
            .chain([usage_json(&usage, "pool", None)])
            .collect();
        // Each usage line carries the time its turn was saved; the rest of
        // the line is the same as before turns had a time.
        let turn = |text: &str| -> Vec<Value> {
            text.lines()
                .skip(1)
                .map(|line| {
                    let mut value: Value = serde_json::from_str(line).unwrap();
                    if value["type"] == "usage" {
                        let at = value.as_object_mut().unwrap().remove("at");
                        let at = at.and_then(|at| at.as_u64()).expect("usage has a time");
                        assert!(
                            (before..=after).contains(&at),
                            "{at} not in {before}..={after}"
                        );
                    }
                    value
                })
                .collect()
        };
        assert_eq!(turn(&first), expected, "one line per item, then usage");
        let both: Vec<Value> = expected.iter().chain(expected.iter()).cloned().collect();
        assert_eq!(turn(&raw), both, "the second turn follows the first");
    }

    #[test]
    fn each_ask_keeps_the_time_its_turn_was_saved_through_a_rename() {
        let (_dir, _guard) = isolated("times");
        let id = "native-times";
        let usage = Usage::default();
        let before = grokhub_core::now_ms();
        record_turn(id, "/work", "grok-4.7", &sample_items(), &usage, "", "fix").unwrap();
        let between = grokhub_core::now_ms();
        let second = [user("and the bar"), assistant("Done.")];
        record_turn(id, "/work", "grok-4.7", &second, &usage, "", "bar").unwrap();
        let after = grokhub_core::now_ms();
        let times = user_turn_times(&load_session(id).unwrap());
        assert_eq!(times.len(), 2, "{times:?}");
        let (first, last) = (times[0].unwrap(), times[1].unwrap());
        assert!(
            (before..=between).contains(&first),
            "{first} not in {before}..={between}"
        );
        assert!(
            (first..=after).contains(&last),
            "{last} not in {first}..={after}"
        );
        rename_session(id, "Dock and bar").unwrap();
        let renamed = load_session(id).unwrap();
        assert_eq!(renamed.title, "Dock and bar");
        assert_eq!(
            user_turn_times(&renamed),
            times,
            "a rewrite keeps the times"
        );
    }

    #[test]
    fn an_ask_saved_before_turns_had_a_time_has_none() {
        let (_dir, _guard) = isolated("untimed");
        let id = "native-untimed";
        let file = session_file(id).unwrap();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let lines = [
            json!({"id": id, "title": "Old", "created": 5, "cwd": "/work",
                "model": "grok-4.7", "engine": "native", "renamed": false}),
            item_json(&user("old ask")),
            item_json(&assistant("Old reply.")),
            usage_json(&Usage::default(), "", None),
        ];
        let raw: String = lines.iter().map(|line| format!("{line}\n")).collect();
        fs::write(&file, raw).unwrap();
        assert_eq!(user_turn_times(&load_session(id).unwrap()), vec![None]);
    }

    #[test]
    fn compacted_messages_counts_what_compaction_hid() {
        let (_dir, _guard) = isolated("compacted");
        let id = "native-compacted";
        let usage = Usage::default();
        record_turn(id, "/work", "grok-4.7", &sample_items(), &usage, "", "fix").unwrap();
        assert_eq!(compacted_messages(&load_session(id).unwrap()), 0);
        let second = [user("and the bar"), assistant("Done.")];
        record_turn(id, "/work", "grok-4.7", &second, &usage, "", "bar").unwrap();
        let summary = [assistant("Summary: fixed the dock and the bar.")];
        record_compaction(id, "/work", "grok-4.7", &summary, &usage, "").unwrap();
        let info = load_session(id).unwrap();
        // Two asks and two replies went under the summary; tool steps aren't
        // messages.
        assert_eq!(compacted_messages(&info), 4);
        assert_eq!(user_turn_times(&info), Vec::<Option<u64>>::new());
    }

    fn user_turn_times(info: &SessionInfo) -> Vec<Option<u64>> {
        user_lines(info).iter().map(|line| line.at).collect()
    }

    fn user_kinds(info: &SessionInfo) -> Vec<UserKind> {
        user_lines(info).iter().map(|line| line.kind).collect()
    }

    /// Writes a chat by hand: per turn its items and a usage line at `at`.
    fn write_timed(id: &str, turns: &[(&[InputItem], u64)]) {
        let file = session_file(id).unwrap();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut lines = vec![
            json!({"id": id, "title": "Chat", "created": 5, "cwd": "/work",
            "model": "grok-4.7", "engine": "native", "renamed": false}),
        ];
        for (items, at) in turns {
            lines.extend(items.iter().map(item_json));
            lines.push(usage_json(&Usage::default(), "", Some(*at)));
        }
        let raw: String = lines.iter().map(|line| format!("{line}\n")).collect();
        fs::write(&file, raw).unwrap();
    }

    #[test]
    fn a_compaction_keeps_the_times_of_the_asks_it_kept_and_stamps_its_own() {
        let (_dir, _guard) = isolated("kept-times");
        let id = "native-kept-times";
        let usage = Usage::default();
        let first = [user("fix the dock"), assistant("On it.")];
        let second = [user("and the bar"), assistant("Done.")];
        write_timed(id, &[(&first, 1_000), (&second, 2_000)]);
        assert_eq!(
            user_turn_times(&load_session(id).unwrap()),
            vec![Some(1_000), Some(2_000)]
        );
        // A manual compact: a summary, then the last turn word for word.
        let summary = summary_of("Fixed the dock.");
        let kept = [summary.clone(), second[0].clone(), second[1].clone()];
        let before = grokhub_core::now_ms();
        record_compaction(id, "/work", "grok-4.7", &kept, &usage, "").unwrap();
        let after = grokhub_core::now_ms();
        let info = load_session(id).unwrap();
        assert_eq!(
            compacted_messages(&info),
            2,
            "only the first ask and its reply went"
        );
        let times = user_turn_times(&info);
        assert_eq!(times.len(), 2, "{times:?}");
        let compacted_at = times[0].expect("the summary takes the compaction's time");
        assert!(
            (before..=after).contains(&compacted_at),
            "{compacted_at} not in {before}..={after}"
        );
        assert_eq!(times[1], Some(2_000), "the kept ask keeps its own time");
        // A turn that compacts as it runs is saved only by the compaction, so
        // its summary and its ask take that time.
        let third = [user("now the roof"), assistant("Roof done.")];
        let summary = summary_of("Dock and bar.");
        let kept = [summary, third[0].clone(), third[1].clone()];
        record_compacted_turn(id, "/work", "grok-4.7", &kept, (2, 0), &[], &usage, "").unwrap();
        let after_third = grokhub_core::now_ms();
        let info = load_session(id).unwrap();
        let times = user_turn_times(&info);
        assert_eq!(times.len(), 2, "{times:?}");
        assert_eq!(times[0], times[1], "{times:?}");
        let roof = times[1].expect("the compacting turn has a time");
        assert!(
            (after..=after_third).contains(&roof),
            "{roof} not in {after}..={after_third}"
        );
        // The first compaction hid two messages, the second the bar ask and
        // its reply (the first summary isn't a message).
        assert_eq!(compacted_messages(&info), 4);
    }

    #[test]
    fn a_summary_compaction_that_kept_only_the_ask_keeps_its_time() {
        let (_dir, _guard) = isolated("summary-shape");
        let id = "native-summary-shape";
        let first = [user("fix the dock"), assistant("On it.")];
        let second = [user("and the bar"), assistant("Done.")];
        write_timed(id, &[(&first, 1_000), (&second, 2_000)]);
        // The summary path keeps the last ask but not its reply.
        let summary = summary_of("Dock and bar.");
        let rules = user("<standing_rules>\nKeep it short.\n</standing_rules>");
        let kept = [summary, rules, second[0].clone()];
        record_compaction(id, "/work", "grok-4.7", &kept, &Usage::default(), "").unwrap();
        let info = load_session(id).unwrap();
        let times = user_turn_times(&info);
        assert_eq!(times.len(), 3, "{times:?}");
        assert_eq!(times[2], Some(2_000), "the kept ask keeps its own time");
        assert_eq!(
            times[0], times[1],
            "summary and rules share the compaction's time"
        );
        assert_ne!(times[0], Some(2_000));
        assert_eq!(compacted_messages(&info), 3, "the ask is still in view");
        assert_eq!(
            user_kinds(&info),
            [UserKind::Compaction, UserKind::Compaction, UserKind::Ask]
        );
    }

    #[test]
    fn deleting_the_reply_a_compaction_kept_keeps_the_asks_time() {
        let (_dir, _guard) = isolated("server-shape");
        let id = "native-server-shape";
        let first = [user("fix the dock"), assistant("On it.")];
        let second = [user("and the bar"), assistant("Done.")];
        write_timed(id, &[(&first, 1_000), (&second, 2_000)]);
        // The server path keeps the last ask and its reply.
        let summary = summary_of("Dock.");
        let kept = [summary, second[0].clone(), second[1].clone()];
        record_compaction(id, "/work", "grok-4.7", &kept, &Usage::default(), "").unwrap();
        assert_eq!(compacted_messages(&load_session(id).unwrap()), 2);
        let what = Forget::Reply {
            ask: Some(("and the bar", 0)),
        };
        assert_eq!(forget_message(id, what).unwrap(), Forgotten::Removed);
        let info = load_session(id).unwrap();
        assert_eq!(
            message_pairs(&info)
                .into_iter()
                .map(|(_, text)| text)
                .collect::<Vec<_>>(),
            [
                format!(
                    "{}\n[image]",
                    crate::compact::message_text(&summary_of("Dock."))
                ),
                "and the bar\n[image]".to_string()
            ],
            "the reply is gone"
        );
        let times = user_turn_times(&info);
        assert_eq!(times.len(), 2, "{times:?}");
        assert_eq!(times[1], Some(2_000));
        assert_ne!(times[0], Some(2_000));
        // The deleted reply's saved copy is still under the compaction, so it
        // counts with what the compaction hid.
        assert_eq!(compacted_messages(&info), 3);
    }

    #[test]
    fn a_turn_that_took_a_ready_compaction_keeps_the_last_turns_times() {
        let (_dir, _guard) = isolated("bg-compact");
        let id = "native-bg-compact";
        let first = [user("fix the dock"), assistant("On it.")];
        let second = [user("and the bar"), assistant("Done.")];
        write_timed(id, &[(&first, 1_000), (&second, 2_000)]);
        // A compaction made during the bar turn, used by the next one: it
        // holds the bar turn and the new roof turn.
        let summary = summary_of("Dock.");
        let roof = [user("now the roof"), assistant("Roof done.")];
        let kept = [
            summary,
            second[0].clone(),
            second[1].clone(),
            roof[0].clone(),
            roof[1].clone(),
        ];
        let before = grokhub_core::now_ms();
        record_compacted_turn(
            id,
            "/work",
            "grok-4.7",
            &kept,
            (2, 0),
            &[],
            &Usage::default(),
            "",
        )
        .unwrap();
        let info = load_session(id).unwrap();
        let times = user_turn_times(&info);
        assert_eq!(times.len(), 3, "{times:?}");
        assert_eq!(times[1], Some(2_000), "the bar ask was saved on its own");
        let roof_at = times[2].unwrap();
        assert!(roof_at >= before, "{roof_at} < {before}");
        assert_eq!(
            times[0], times[2],
            "summary and the new ask share the compaction's time"
        );
        assert_eq!(compacted_messages(&info), 2, "only the dock turn went");
        // Deleting the bar reply the compaction kept moves no time.
        let what = Forget::Reply {
            ask: Some(("and the bar", 0)),
        };
        assert_eq!(forget_message(id, what).unwrap(), Forgotten::Removed);
        let after = user_turn_times(&load_session(id).unwrap());
        assert_eq!(after, times, "{after:?}");
    }

    fn thought(calls: usize, texts: usize, text: &str) -> TurnThought {
        TurnThought {
            calls,
            texts,
            text: text.into(),
        }
    }

    fn read_call(id: &str) -> [InputItem; 2] {
        [
            InputItem::FunctionCall {
                call_id: id.into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            },
            InputItem::FunctionCallOutput {
                call_id: id.into(),
                output: "fn main() {}".into(),
            },
        ]
    }

    fn shown(id: &str) -> Vec<(String, String)> {
        shown_pairs(&load_session(id).unwrap())
            .into_iter()
            .map(|(role, text)| (role, text.lines().next().unwrap_or("").to_string()))
            .collect()
    }

    fn pairs(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter()
            .map(|(role, text)| (role.to_string(), text.to_string()))
            .collect()
    }

    #[test]
    fn a_turns_thoughts_are_saved_where_they_were_shown_and_never_go_to_the_model() {
        let (_dir, _guard) = isolated("thoughts-saved");
        let id = "native-thoughts-saved";
        let [call, output] = read_call("c1");
        let items = [
            user("fix the dock"),
            assistant("Checking."),
            call,
            output,
            assistant("Fixed."),
        ];
        let thoughts = [
            thought(0, 0, "Read main.rs first."),
            thought(1, 1, "It builds."),
        ];
        record_turn_with(
            id,
            "/work",
            "grok-4.7",
            &items,
            &thoughts,
            &Usage::default(),
            "",
            "fix the dock",
        )
        .unwrap();
        let info = load_session(id).unwrap();
        assert_eq!(info.input(), items.to_vec(), "thoughts are not model input");
        assert_eq!(
            shown(id),
            pairs(&[
                ("user", "fix the dock"),
                ("thought", "Read main.rs first."),
                ("assistant", "Checking."),
                ("assistant", "Tool read_file"),
                ("assistant", "Tool result"),
                ("thought", "It builds."),
                ("assistant", "Fixed."),
            ])
        );
        assert_eq!(
            transcript_pairs(&info).len(),
            5,
            "no thoughts in the plain transcript"
        );
        assert!(!export_markdown(id).unwrap().contains("Read main.rs"));
    }

    #[test]
    fn a_held_value_is_redacted_from_the_saved_reply_and_thoughts() {
        let (_dir, _guard) = isolated("thoughts-held");
        let id = "native-thoughts-held";
        let token = "sk-abcdefghijklmnopqrstuv";
        crate::held::hold_secret(id, token);
        let items = [
            user("connect github"),
            assistant(&format!("Saved {token}.")),
        ];
        let thoughts = [
            thought(0, 0, &format!("They gave {token}.")),
            thought(3, 1, "  "),
        ];
        record_turn_with(
            id,
            "/work",
            "grok-4.7",
            &items,
            &thoughts,
            &Usage::default(),
            "",
            "connect github",
        )
        .unwrap();
        assert_eq!(
            shown(id),
            pairs(&[
                ("user", "connect github"),
                ("thought", "They gave [redacted]."),
                ("assistant", "Saved [redacted]."),
            ])
        );
        let file = fs::read_to_string(session_file(id).unwrap()).unwrap();
        assert!(!file.contains(token), "{file}");
    }

    #[test]
    fn a_held_value_is_redacted_from_saved_tool_calls_and_outputs() {
        let (_dir, _guard) = isolated("tools-held");
        let id = "native-tools-held";
        let token = "sk-abcdefghijklmnopqrstuv";
        crate::held::hold_secret(id, token);
        let items = [
            user("connect github"),
            InputItem::FunctionCall {
                call_id: "c1".into(),
                name: "write_file".into(),
                arguments: format!(r#"{{"path":".env","content":"KEY={token}"}}"#),
            },
            InputItem::FunctionCallOutput {
                call_id: "c1".into(),
                output: format!("wrote KEY={token}"),
            },
            assistant("Saved."),
        ];
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &items,
            &Usage::default(),
            "",
            "connect github",
        )
        .unwrap();
        let file = fs::read_to_string(session_file(id).unwrap()).unwrap();
        assert!(!file.contains(token), "{file}");
        assert!(file.contains(r#"KEY=[redacted]\"}"#), "{file}");
        assert!(file.contains("wrote KEY=[redacted]"), "{file}");
    }

    #[test]
    fn deleting_a_reply_deletes_its_thoughts_and_keeps_the_next_turns() {
        let (_dir, _guard) = isolated("thoughts-forget");
        let id = "native-thoughts-forget";
        let [call, output] = read_call("c1");
        for (items, thoughts) in [
            (
                vec![user("fix the dock"), call, output, assistant("Fixed.")],
                vec![thought(0, 0, "Read it."), thought(1, 0, "Done reading.")],
            ),
            (
                vec![user("and the bar"), assistant("Bar done.")],
                vec![thought(0, 0, "The bar is next.")],
            ),
        ] {
            record_turn_with(
                id,
                "/work",
                "grok-4.7",
                &items,
                &thoughts,
                &Usage::default(),
                "",
                "",
            )
            .unwrap();
        }
        let what = Forget::Reply {
            ask: Some(("fix the dock", 0)),
        };
        assert_eq!(forget_message(id, what).unwrap(), Forgotten::Removed);
        assert_eq!(
            shown(id),
            pairs(&[
                ("user", "fix the dock"),
                ("user", "and the bar"),
                ("thought", "The bar is next."),
                ("assistant", "Bar done."),
            ])
        );
    }

    #[test]
    fn a_compacting_turns_thoughts_show_before_its_own_items() {
        let (_dir, _guard) = isolated("thoughts-compacted");
        let id = "native-thoughts-compacted";
        let first = [user("fix the dock"), assistant("On it.")];
        record_turn_with(
            id,
            "/work",
            "grok-4.7",
            &first,
            &[thought(0, 0, "An old thought.")],
            &Usage::default(),
            "",
            "",
        )
        .unwrap();
        let kept = [
            summary_of("Dock."),
            user("now the roof"),
            assistant("Roof done."),
        ];
        let thoughts = [thought(0, 0, "Roof first."), thought(2, 1, "Roof checked.")];
        record_compacted_turn(
            id,
            "/work",
            "grok-4.7",
            &kept,
            (2, 0),
            &thoughts,
            &Usage::default(),
            "",
        )
        .unwrap();
        assert_eq!(
            shown(id),
            pairs(&[
                ("user", "This session is being continued from a previous conversation that ran out of context. The summary below covers the earlier portion of the conversation."),
                ("thought", "Roof first."),
                ("thought", "Roof checked."),
                ("user", "now the roof"),
                ("assistant", "Roof done."),
            ])
        );
        assert_eq!(load_session(id).unwrap().input(), kept.to_vec());
        let what = Forget::Reply {
            ask: Some(("now the roof", 0)),
        };
        assert_eq!(forget_message(id, what).unwrap(), Forgotten::Removed);
        let after = shown(id);
        assert_eq!(after.len(), 2, "{after:?}");
        assert_eq!(after[1], ("user".to_string(), "now the roof".to_string()));
    }

    #[test]
    fn a_turn_after_a_stopped_tool_step_keeps_the_last_turns_time() {
        let (_dir, _guard) = isolated("after-tool");
        let id = "native-after-tool";
        let call = InputItem::FunctionCall {
            call_id: "c1".into(),
            name: "run".into(),
            arguments: "{}".into(),
        };
        let output = InputItem::FunctionCallOutput {
            call_id: "c1".into(),
            output: "stopped".into(),
        };
        // Stopped during its tool: the turn ends on the tool's output.
        let stopped = [user("fix the build"), call.clone(), output.clone()];
        write_timed(id, &[(&stopped, 1_000)]);
        let ask = user("try again");
        let kept = [
            summary_of("Nothing yet."),
            stopped[0].clone(),
            call,
            output,
            ask,
            assistant("Fixed."),
        ];
        record_compacted_turn(
            id,
            "/work",
            "grok-4.7",
            &kept,
            (2, 0),
            &[],
            &Usage::default(),
            "",
        )
        .unwrap();
        let info = load_session(id).unwrap();
        let times = user_turn_times(&info);
        assert_eq!(times.len(), 3, "{times:?}");
        assert_eq!(times[1], Some(1_000), "the stopped ask keeps its time");
        assert_ne!(times[2], Some(1_000));
        assert_eq!(times[0], times[2]);
        assert_eq!(compacted_messages(&info), 0, "nothing went out of view");
        assert_eq!(
            user_kinds(&info),
            [UserKind::Compaction, UserKind::Ask, UserKind::Ask],
            "an ask after a tool's output is still an ask"
        );
        // Deleting this turn's ask leaves the stopped one its time.
        let what = Forget::Ask {
            text: "try again",
            later: 0,
        };
        assert_eq!(forget_message(id, what).unwrap(), Forgotten::Removed);
        let info = load_session(id).unwrap();
        let after = user_turn_times(&info);
        assert_eq!(after, [times[0], Some(1_000)], "{after:?}");
        assert_eq!(compacted_messages(&info), 0);
        let own = info.records.iter().find_map(|record| match record {
            Record::Compaction { own, .. } => Some(*own),
            _ => None,
        });
        assert_eq!(own, Some(1), "only the reply is still the turn's own");
    }

    #[test]
    fn a_tool_image_kept_without_its_result_is_still_a_tool_image() {
        let (_dir, _guard) = isolated("image-head");
        let id = "native-image-head";
        let image = InputItem::Message {
            role: "user".into(),
            content: vec![
                ContentPart::InputText("image from screenshot".into()),
                ContentPart::InputImage("data:image/png;base64,QQ==".into()),
            ],
        };
        let first = [user("check the login page"), assistant("Looking.")];
        write_timed(id, &[(&first, 1_000)]);
        // The tail starts at the newest user message: the tool's image. The
        // turn's ask went into the summary.
        let kept = [
            summary_of("Checked the login page."),
            user("<standing_rules>\nKeep it short.\n</standing_rules>"),
            image,
            assistant("The button is hidden."),
        ];
        record_compacted_turn(
            id,
            "/work",
            "grok-4.7",
            &kept,
            (2, 1),
            &[],
            &Usage::default(),
            "",
        )
        .unwrap();
        rename_session(id, "Login").unwrap();
        let info = load_session(id).unwrap();
        assert_eq!(
            compacted_messages(&info),
            3,
            "the first turn's two and the folded ask, through a rewrite"
        );
        assert_eq!(
            user_kinds(&info),
            [
                UserKind::Compaction,
                UserKind::Compaction,
                UserKind::ToolImage
            ]
        );
        // A typed message that only starts the same way is an ask.
        let typed = InputItem::Message {
            role: "user".into(),
            content: vec![
                ContentPart::InputText("image from my phone".into()),
                ContentPart::InputImage("data:image/png;base64,QQ==".into()),
            ],
        };
        assert!(!is_tool_image(&typed));
    }

    #[test]
    fn the_compacted_count_leaves_out_tool_images_and_compaction_text() {
        let (_dir, _guard) = isolated("count-kinds");
        let id = "native-count-kinds";
        let image = InputItem::Message {
            role: "user".into(),
            content: vec![
                ContentPart::InputText("image from screenshot".into()),
                ContentPart::InputImage("data:image/png;base64,QQ==".into()),
            ],
        };
        let call = InputItem::FunctionCall {
            call_id: "c1".into(),
            name: "screenshot".into(),
            arguments: "{}".into(),
        };
        let output = InputItem::FunctionCallOutput {
            call_id: "c1".into(),
            output: "ok".into(),
        };
        let first = [user("look"), call, output, image, assistant("A dock.")];
        write_timed(id, &[(&first, 1_000)]);
        let kept = [
            summary_of("Saw a dock."),
            user("<standing_rules>\n</standing_rules>"),
        ];
        record_compaction(id, "/work", "grok-4.7", &kept, &Usage::default(), "").unwrap();
        let info = load_session(id).unwrap();
        assert_eq!(compacted_messages(&info), 2, "the ask and the reply");
        let kept = [summary_of("Saw a dock, twice.")];
        record_compaction(id, "/work", "grok-4.7", &kept, &Usage::default(), "").unwrap();
        let info = load_session(id).unwrap();
        assert_eq!(
            compacted_messages(&info),
            2,
            "the first summary and rules aren't messages"
        );
    }

    #[test]
    fn a_note_after_the_ask_doesnt_lend_the_ask_an_older_time() {
        let (_dir, _guard) = isolated("note-after");
        let id = "native-note-after";
        let first = [user("continue"), assistant("Step one done.")];
        write_timed(id, &[(&first, 1_000)]);
        let notice = user("<background-notice>helper done</background-notice>");
        let kept = [
            summary_of("Step one."),
            user("continue"),
            notice,
            assistant("Step two done."),
        ];
        let before = grokhub_core::now_ms();
        record_compacted_turn(
            id,
            "/work",
            "grok-4.7",
            &kept,
            (3, 0),
            &[],
            &Usage::default(),
            "",
        )
        .unwrap();
        let info = load_session(id).unwrap();
        let times = user_turn_times(&info);
        assert_eq!(times.len(), 3, "{times:?}");
        assert!(times.iter().all(|at| at.unwrap() >= before), "{times:?}");
        assert_eq!(compacted_messages(&info), 2);
        assert_eq!(
            user_kinds(&info),
            [UserKind::Compaction, UserKind::Ask, UserKind::Note]
        );
    }

    #[test]
    fn a_cancelled_ask_sent_again_in_a_compacting_turn_takes_that_turns_time() {
        let (_dir, _guard) = isolated("resent");
        let id = "native-resent";
        let first = [user("check the logs"), assistant("Clean.")];
        // Cancelled before any reply.
        let cancelled = [user("continue")];
        write_timed(id, &[(&first, 1_000), (&cancelled, 2_000)]);
        let summary = summary_of("Logs.");
        let again = [user("continue"), assistant("Step one done.")];
        let kept = [summary, again[0].clone(), again[1].clone()];
        let before = grokhub_core::now_ms();
        record_compacted_turn(
            id,
            "/work",
            "grok-4.7",
            &kept,
            (2, 0),
            &[],
            &Usage::default(),
            "",
        )
        .unwrap();
        let times = user_turn_times(&load_session(id).unwrap());
        assert_eq!(times.len(), 2, "{times:?}");
        let at = times[1].unwrap();
        assert!(
            at >= before,
            "{at} < {before}: borrowed the cancelled copy's time"
        );
        assert_eq!(times[0], times[1]);
    }

    #[test]
    fn a_kept_tool_image_matches_its_saved_copy_after_the_image_was_dropped() {
        let (_dir, _guard) = isolated("masked");
        let id = "native-masked";
        let image = |part: ContentPart| InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText("image from screenshot".into()), part],
        };
        let call = InputItem::FunctionCall {
            call_id: "c1".into(),
            name: "screenshot".into(),
            arguments: "{}".into(),
        };
        let output = InputItem::FunctionCallOutput {
            call_id: "c1".into(),
            output: "ok".into(),
        };
        let saved = [
            user("look at the screen"),
            call.clone(),
            output.clone(),
            image(ContentPart::InputImage("data:image/png;base64,QQ==".into())),
            assistant("I see a dock."),
        ];
        write_timed(id, &[(&saved, 2_000)]);
        // The next turn took a compaction made while the image was masked.
        let masked = image(ContentPart::InputText(
            crate::image_budget::OLD_IMAGE_PLACEHOLDER.into(),
        ));
        let kept = [
            summary_of("Nothing yet."),
            saved[0].clone(),
            call,
            output,
            masked,
            saved[4].clone(),
            user("now the bar"),
            assistant("Bar done."),
        ];
        let before = grokhub_core::now_ms();
        record_compacted_turn(
            id,
            "/work",
            "grok-4.7",
            &kept,
            (2, 0),
            &[],
            &Usage::default(),
            "",
        )
        .unwrap();
        let info = load_session(id).unwrap();
        let times = user_turn_times(&info);
        assert_eq!(times.len(), 4, "{times:?}");
        assert_eq!(
            (times[1], times[2]),
            (Some(2_000), Some(2_000)),
            "the ask and its tool image keep the saved time"
        );
        let own = times[3].unwrap();
        assert!(own >= before, "{own} < {before}");
        assert_eq!(times[0], times[3]);
        assert_eq!(compacted_messages(&info), 0, "nothing went out of view");
        assert_eq!(
            user_kinds(&info),
            [
                UserKind::Compaction,
                UserKind::Ask,
                UserKind::ToolImage,
                UserKind::Ask
            ]
        );
    }

    #[test]
    fn a_repeated_ask_takes_the_time_of_the_copy_it_stands_for() {
        let (_dir, _guard) = isolated("repeat-ask");
        let id = "native-repeat-ask";
        let first = [user("continue"), assistant("Step one done.")];
        let second = [user("check the logs"), assistant("Clean.")];
        let third = [user("continue"), assistant("Step two done.")];
        write_timed(id, &[(&first, 1_000), (&second, 2_000), (&third, 3_000)]);
        // A manual compact keeps the last turn: its "continue" is the 3,000
        // one, not the older copy.
        let summary = summary_of("Steps and logs.");
        let kept = [summary, third[0].clone(), third[1].clone()];
        record_compaction(id, "/work", "grok-4.7", &kept, &Usage::default(), "").unwrap();
        let times = user_turn_times(&load_session(id).unwrap());
        assert_eq!(times.len(), 2, "{times:?}");
        assert_eq!(times[1], Some(3_000));
        // A turn that says "continue" again and compacts as it runs.
        let summary = summary_of("Steps, logs, step two.");
        let again = [user("continue"), assistant("Step three done.")];
        let kept = [summary, again[0].clone(), again[1].clone()];
        let before = grokhub_core::now_ms();
        record_compacted_turn(
            id,
            "/work",
            "grok-4.7",
            &kept,
            (2, 0),
            &[],
            &Usage::default(),
            "",
        )
        .unwrap();
        let after = grokhub_core::now_ms();
        let raw = fs::read_to_string(session_file(id).unwrap()).unwrap();
        let marker: Value = serde_json::from_str(raw.lines().nth(12).unwrap()).unwrap();
        assert_eq!(
            (&marker["type"], &marker["own"]),
            (&json!("compaction"), &json!(2))
        );
        let times = user_turn_times(&load_session(id).unwrap());
        assert_eq!(times.len(), 2, "{times:?}");
        let at = times[1].unwrap();
        assert!(
            (before..=after).contains(&at),
            "{at} not in {before}..={after}"
        );
        assert_eq!(
            times[0], times[1],
            "summary and ask share the compaction's time"
        );
    }

    #[test]
    fn session_intact_needs_a_readable_file_with_a_turn() {
        let (_dir, _guard) = isolated("intact");
        assert!(!session_intact("native-none"), "no file");
        assert!(!session_intact("../escape"), "bad id");
        record_turn(
            "native-kept",
            "/work",
            "grok-4.7",
            &sample_items(),
            &Usage::default(),
            "",
            "fix the dock",
        )
        .unwrap();
        assert!(session_intact("native-kept"));
        fs::write(session_file("native-kept").unwrap(), "").unwrap();
        assert!(!session_intact("native-kept"), "an emptied file is gone");
    }

    #[test]
    fn truncated_file_repair_drops_torn_tail() {
        let (_dir, _guard) = isolated("torn");
        let id = "native-torn";
        let items = vec![user("keep me")];
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &items,
            &Usage::default(),
            "",
            "keep me",
        )
        .unwrap();
        let path = session_file(id).unwrap();
        {
            let mut file = OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(br#"{"type":"message","text":"TORN_TAIL_MARKER""#)
                .unwrap();
        }
        let loaded = load_session(id).expect("torn tail must not crash");
        assert_eq!(loaded.input(), items);
        let repaired = fs::read_to_string(&path).unwrap();
        assert!(repaired.ends_with('\n'), "{repaired:?}");
        assert!(!repaired.contains("TORN_TAIL_MARKER"), "{repaired}");
        for line in repaired.lines() {
            serde_json::from_str::<Value>(line).unwrap_or_else(|err| panic!("{err}: {line}"));
        }
        fs::write(&path, b"{\"id\":\"native-torn\",\"tit").unwrap();
        let err = load_session(id);
        assert!(err.is_err(), "{err:?}");
    }

    #[test]
    fn fork_stays_independent() {
        let (_dir, _guard) = isolated("fork");
        let id = "native-fork-src";
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[user("shared")],
            &Usage::default(),
            "",
            "shared",
        )
        .unwrap();
        let forked = fork_session(id).unwrap();
        assert_ne!(forked.id, id);
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[user("only-parent")],
            &Usage::default(),
            "",
            "only-parent",
        )
        .unwrap();
        record_turn(
            &forked.id,
            "/work",
            "grok-4.7",
            &[user("only-child")],
            &Usage::default(),
            "",
            "only-child",
        )
        .unwrap();
        let parent = resume_input(id).unwrap();
        let child = resume_input(&forked.id).unwrap();
        let texts = |items: &[InputItem]| -> String {
            items
                .iter()
                .map(|item| format!("{item:?}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let parent_text = texts(&parent);
        let child_text = texts(&child);
        assert!(parent_text.contains("only-parent"), "{parent_text}");
        assert!(!parent_text.contains("only-child"), "{parent_text}");
        assert!(child_text.contains("only-child"), "{child_text}");
        assert!(!child_text.contains("only-parent"), "{child_text}");
        assert!(child_text.contains("shared"), "{child_text}");
    }

    #[test]
    fn rename_and_export_markdown() {
        let (_dir, _guard) = isolated("rename");
        let id = "native-rename";
        let usage = Usage {
            input_tokens: 3,
            output_tokens: 1,
            reasoning_tokens: 0,
            cost_in_usd_ticks: 9,
            cached_tokens: 0,
        };
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &sample_items(),
            &usage,
            "API credits",
            "fix the dock",
        )
        .unwrap();
        rename_session(id, "Harbor lights").unwrap();
        let loaded = load_session(id).unwrap();
        assert_eq!(loaded.title, "Harbor lights");
        assert!(loaded.renamed);
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[user("later title")],
            &Usage::default(),
            "",
            "later title",
        )
        .unwrap();
        assert_eq!(load_session(id).unwrap().title, "Harbor lights");
        let md = export_markdown(id).unwrap();
        assert!(md.starts_with("# Harbor lights\n"), "{md}");
        assert!(md.contains("## User"), "{md}");
        assert!(md.contains("fix the dock"), "{md}");
        assert!(md.contains("## Tool read_file"), "{md}");
        assert!(md.contains("## Tool result"), "{md}");
        assert!(md.contains("## Usage"), "{md}");
        assert!(md.contains("$0.0000000009"), "{md}");
        assert!(md.contains("API credits"), "{md}");
        // A tick is 1e-10 USD.
        assert_eq!(format_cost_ticks(84_000), "$0.0000084");
        assert_eq!(format_cost_ticks(37_756_000), "$0.0037");
        assert_eq!(format_cost_ticks(840_000_000), "$0.084");
        assert_eq!(format_cost_ticks(12_345_000_000), "$1.23");
        assert_eq!(format_cost_ticks(5_000_000_000), "$0.50");
        assert_eq!(format_cost_ticks(1), "$0.0000000001");
        assert_eq!(format_cost_ticks(0), "$0.00");
        assert_eq!(format_cost_ticks(-31_000_000), "-$0.0031");
        assert_eq!(
            local_title("<user_query>\nfix the dock\n</user_query>"),
            "fix the dock"
        );
    }

    #[test]
    fn delete_stops_a_running_session() {
        let (_dir, _guard) = isolated("delete");
        let id = "native-running";
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[user("hi")],
            &Usage::default(),
            "",
            "hi",
        )
        .unwrap();
        let path = session_file(id).unwrap();
        assert!(path.is_file());
        let cancel = CancelToken::new();
        let saw_file = Arc::new(AtomicBool::new(false));
        let flag = saw_file.clone();
        let watched = path.clone();
        let _guard_run = attach_run_with_hook(
            id,
            cancel.clone(),
            Some(Box::new(move || {
                flag.store(watched.is_file(), Ordering::SeqCst);
            })),
        )
        .expect("guard");
        delete_session(id).unwrap();
        assert!(
            cancel.is_cancelled(),
            "delete must cancel the run before it returns"
        );
        assert!(
            saw_file.load(Ordering::SeqCst),
            "cancel must run while the file is still there"
        );
        assert!(
            !path.exists(),
            "the file is removed after the run is stopped"
        );
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[user("back")],
            &Usage::default(),
            "",
            "back",
        )
        .unwrap();
        assert!(!path.exists(), "a deleted session must not be written back");
    }

    #[test]
    fn deleting_a_chat_takes_its_forks_and_their_forks_but_no_other_chat() {
        let (_dir, _guard) = isolated("forks");
        let turn = |id: &str| {
            record_turn(
                id,
                "/work",
                "grok-4.7",
                &[user("hi")],
                &Usage::default(),
                "",
                "hi",
            )
            .unwrap()
        };
        turn("native-main");
        turn("native-other");
        let fork = fork_session("native-main").unwrap();
        assert_eq!(fork.forked_from, "native-main");
        assert_eq!(load_session(&fork.id).unwrap().forked_from, "native-main");
        let grandchild = fork_session(&fork.id).unwrap();
        let unrelated = fork_session("native-other").unwrap();

        let mut gone = delete_forks_of(&["native-main".to_string()]);
        gone.sort();
        let mut want = vec![fork.id.clone(), grandchild.id.clone()];
        want.sort();
        assert_eq!(gone, want);
        assert!(!session_file(&fork.id).unwrap().exists());
        assert!(!session_file(&grandchild.id).unwrap().exists());
        assert!(
            session_file("native-main").unwrap().is_file(),
            "the parent is the caller's to delete"
        );
        assert!(session_file("native-other").unwrap().is_file());
        assert!(session_file(&unrelated.id).unwrap().is_file());
        assert!(delete_forks_of(&["native-main".to_string()]).is_empty());
    }

    fn said(role: &str, text: &str) -> InputItem {
        InputItem::Message {
            role: role.into(),
            content: vec![ContentPart::InputText(text.into())],
        }
    }

    fn ran(call: &str) -> [InputItem; 2] {
        [
            InputItem::FunctionCall {
                call_id: call.into(),
                name: "read".into(),
                arguments: "{}".into(),
            },
            InputItem::FunctionCallOutput {
                call_id: call.into(),
                output: format!("{call} done"),
            },
        ]
    }

    fn seen(id: &str) -> Vec<(String, String)> {
        transcript_pairs(&load_session(id).unwrap())
    }

    fn save(id: &str, items: &[InputItem]) {
        record_turn(id, "/work", "grok-4.7", items, &Usage::default(), "", "t").unwrap();
    }

    #[test]
    fn forgetting_an_ask_finds_it_inside_a_note_the_app_added_but_prefers_an_exact_match() {
        let (_dir, _guard) = isolated("forget-wrapped");
        let id = "native-forget-wrapped";
        save(
            id,
            &[
                said("user", "Earlier: we planned a trip.\nbook the train"),
                said("assistant", "booked"),
            ],
        );
        save(id, &[said("user", "book"), said("assistant", "what?")]);
        save(
            id,
            &[
                said("user", "book the train please"),
                said("assistant", "again"),
            ],
        );
        // "book" is asked exactly once; the wrapped asks above it don't count.
        assert_eq!(
            forget_message(
                id,
                Forget::Ask {
                    text: "book",
                    later: 0
                }
            ),
            Ok(Forgotten::Removed)
        );
        // No exact "book the train": the one sent with a recap before it is
        // the newest that wraps it on a word boundary, then the older one.
        assert_eq!(
            forget_message(
                id,
                Forget::Ask {
                    text: "book the train",
                    later: 1
                }
            ),
            Ok(Forgotten::Removed)
        );
        let users: Vec<String> = seen(id)
            .into_iter()
            .filter(|(role, _)| role == "user")
            .map(|(_, text)| text)
            .collect();
        assert_eq!(users, vec!["book the train please".to_string()]);
    }

    #[test]
    fn forgetting_an_ask_takes_only_that_message_counting_repeats_from_the_end() {
        let (_dir, _guard) = isolated("forget-ask");
        let id = "native-forget-ask";
        save(id, &[said("user", "continue"), said("assistant", "one")]);
        save(
            id,
            &[
                said("user", "the code is kumquat"),
                said("assistant", "two"),
            ],
        );
        save(id, &[said("user", "continue"), said("assistant", "three")]);
        assert_eq!(
            forget_message(
                id,
                Forget::Ask {
                    text: "the  code is kumquat\n",
                    later: 0
                }
            ),
            Ok(Forgotten::Removed)
        );
        // The first "continue" of two: one more comes after it.
        assert_eq!(
            forget_message(
                id,
                Forget::Ask {
                    text: "continue",
                    later: 1
                }
            ),
            Ok(Forgotten::Removed)
        );
        let pairs = seen(id);
        assert_eq!(
            pairs,
            vec![
                ("assistant".to_string(), "one".to_string()),
                ("assistant".into(), "two".into()),
                ("user".into(), "continue".into()),
                ("assistant".into(), "three".into()),
            ]
        );
        assert!(!format!("{:?}", resume_input(id).unwrap()).contains("kumquat"));
        assert_eq!(
            forget_message(
                id,
                Forget::Ask {
                    text: "never said",
                    later: 0
                }
            ),
            Ok(Forgotten::NotSaved)
        );
    }

    #[test]
    fn forgetting_a_reply_takes_its_messages_and_tool_pairs_up_to_the_next_ask() {
        let (_dir, _guard) = isolated("forget-reply");
        let id = "native-forget-reply";
        let [call, out] = ran("c1");
        save(
            id,
            &[
                said("user", "look"),
                call,
                out,
                said("assistant", "found it"),
            ],
        );
        save(id, &[said("user", "thanks"), said("assistant", "anytime")]);
        assert_eq!(
            forget_message(
                id,
                Forget::Reply {
                    ask: Some(("look", 0))
                }
            ),
            Ok(Forgotten::Removed)
        );
        assert_eq!(
            seen(id),
            vec![
                ("user".to_string(), "look".to_string()),
                ("user".into(), "thanks".into()),
                ("assistant".into(), "anytime".into()),
            ]
        );
    }

    #[test]
    fn forgetting_reaches_into_what_a_compaction_kept_and_skips_what_it_folded() {
        let (_dir, _guard) = isolated("forget-compact");
        let id = "native-forget-compact";
        save(
            id,
            &[said("user", "old ask"), said("assistant", "old reply")],
        );
        record_compaction(
            id,
            "/work",
            "grok-4.7",
            &[said("user", "kept ask"), said("assistant", "summary")],
            &Usage::default(),
            "",
        )
        .unwrap();
        save(
            id,
            &[said("user", "new ask"), said("assistant", "new reply")],
        );
        assert_eq!(
            forget_message(
                id,
                Forget::Ask {
                    text: "old ask",
                    later: 0
                }
            ),
            Ok(Forgotten::NotSaved),
            "folded into the summary, so there is no record to take out"
        );
        assert_eq!(
            forget_message(
                id,
                Forget::Ask {
                    text: "kept ask",
                    later: 0
                }
            ),
            Ok(Forgotten::Removed)
        );
        let input = format!("{:?}", resume_input(id).unwrap());
        assert!(
            !input.contains("kept ask") && input.contains("summary") && input.contains("new ask")
        );
    }

    #[test]
    fn forgetting_waits_for_a_running_reply() {
        let (_dir, _guard) = isolated("forget-busy");
        let id = "native-forget-busy";
        save(id, &[said("user", "hi"), said("assistant", "hello")]);
        let run = attach_run(id, CancelToken::new());
        assert_eq!(
            forget_message(
                id,
                Forget::Ask {
                    text: "hi",
                    later: 0
                }
            ),
            Err(FORGET_BUSY.to_string())
        );
        drop(run);
        assert_eq!(
            forget_message(
                id,
                Forget::Ask {
                    text: "hi",
                    later: 0
                }
            ),
            Ok(Forgotten::Removed)
        );
    }

    #[test]
    fn merge_history_keeps_cli_rows_read_only() {
        let cli = vec![GrokSession {
            id: "01a01b0f-7e06-74b1-8f22-5236c9d57d45".into(),
            title: "Night cabin".into(),
            path: Some(PathBuf::from("/cli/plan.md")),
            cwd: Some(PathBuf::from("/work")),
            cabin: false,
        }];
        let native = SessionInfo {
            id: "native-one".into(),
            title: "fix the dock".into(),
            created: 10,
            cwd: "/work".into(),
            model: "grok-4.7".into(),
            renamed: false,
            usage: Usage {
                input_tokens: 3,
                output_tokens: 1,
                reasoning_tokens: 0,
                cost_in_usd_ticks: 9,
                cached_tokens: 0,
            },
            meter: "SuperGrok pool".into(),
            forked_from: String::new(),
            records: Vec::new(),
        };
        let mut native_clash = native.clone();
        native_clash.id = cli[0].id.clone();
        native_clash.title = "should not replace".into();
        let rows = merge_history(&cli, &[native.clone(), native_clash]);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].id, cli[0].id);
        assert_eq!(rows[0].title, "Night cabin");
        assert_eq!(rows[0].path, cli[0].path);
        assert_eq!(rows[0].cwd, cli[0].cwd);
        assert!(rows[0].read_only);
        assert!(!rows[0].native);
        assert!(rows[1].native);
        assert!(!rows[1].read_only);
        assert_eq!(rows[1].cost_in_usd_ticks, 9);
        assert_eq!(rows[1].title, "fix the dock");
    }

    #[test]
    fn resume_after_compaction_rebuilds_the_compacted_input() {
        let (_dir, _guard) = isolated("compact");
        let id = "native-compact";
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[user("OLD_TOPIC"), assistant("old reply")],
            &Usage {
                input_tokens: 10,
                output_tokens: 4,
                reasoning_tokens: 0,
                cost_in_usd_ticks: 3,
                cached_tokens: 0,
            },
            "SuperGrok pool",
            "OLD_TOPIC",
        )
        .unwrap();
        let summary = InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(
                "This session is being continued from a previous conversation.".into(),
            )],
        };
        let last = user("keep me");
        let compact_usage = Usage {
            input_tokens: 6,
            output_tokens: 2,
            reasoning_tokens: 1,
            cost_in_usd_ticks: 8,
            cached_tokens: 0,
        };
        record_compaction(
            id,
            "/work",
            "grok-4.7",
            &[summary.clone(), last.clone()],
            &compact_usage,
            "SuperGrok pool",
        )
        .unwrap();
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[assistant("after")],
            &Usage {
                input_tokens: 1,
                output_tokens: 1,
                reasoning_tokens: 0,
                cost_in_usd_ticks: 1,
                cached_tokens: 0,
            },
            "SuperGrok pool",
            "",
        )
        .unwrap();
        let loaded = load_session(id).unwrap();
        assert_eq!(loaded.input(), vec![summary, last, assistant("after")]);
        assert!(!loaded
            .input()
            .iter()
            .any(|item| format!("{item:?}").contains("OLD_TOPIC")));
        assert_eq!(loaded.usage.input_tokens, 17);
        assert_eq!(loaded.usage.output_tokens, 7);
        assert_eq!(loaded.usage.reasoning_tokens, 1);
        assert_eq!(loaded.usage.cost_in_usd_ticks, 12);
        let pairs = transcript_pairs(&loaded);
        assert!(pairs.iter().any(|(_, text)| text.contains("keep me")));
        assert!(!pairs.iter().any(|(_, text)| text.contains("OLD_TOPIC")));
        let raw = fs::read_to_string(session_file(id).unwrap()).unwrap();
        assert!(raw.contains("\"type\":\"compaction\""), "{raw}");
    }

    #[test]
    fn a_reopened_chat_shows_what_was_typed_not_the_cabin_notes() {
        let (_dir, _guard) = isolated("volatile");
        let id = "native-volatile";
        let stored = crate::prefix_cache::user_message_text(
            "Talk style for this turn: casual chat.",
            "hi there",
        );
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[user(&stored), assistant("hey")],
            &Usage::default(),
            "SuperGrok pool",
            "hi there",
        )
        .unwrap();
        let loaded = load_session(id).unwrap();
        // The model keeps getting the turn exactly as sent, so the cache holds.
        assert_eq!(loaded.input()[0], user(&stored));
        let pairs = transcript_pairs(&loaded);
        // The helper's turn also carries a picture, shown as its marker.
        assert_eq!(
            pairs[0],
            ("user".to_string(), "hi there\n[image]".to_string())
        );
        let md = export_markdown(id).unwrap();
        assert!(md.contains("hi there"));
        assert!(!md.contains("Talk style for this turn"));
        assert!(!md.contains("cabin-volatile"));
    }

    #[test]
    fn a_reopened_shell_line_shows_as_typed_and_the_model_keeps_the_ask() {
        let (_dir, _guard) = isolated("shell-line");
        let id = "native-shell-line";
        let sent = grokhub_core::shell_line::prompt("ls -la");
        let stored = crate::prefix_cache::user_message_text("Talk style: casual.", &sent);
        let typed = InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(stored.clone())],
        };
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[typed.clone(), assistant("total 0")],
            &Usage::default(),
            "SuperGrok pool",
            &sent,
        )
        .unwrap();
        let loaded = load_session(id).unwrap();
        assert_eq!(loaded.input()[0], typed);
        assert_eq!(
            transcript_pairs(&loaded)[0],
            ("user".to_string(), "$ ls -la".to_string())
        );
        // Any other message shows as it was.
        assert_eq!(
            visible_text("user", &[ContentPart::InputText("run ls -la".into())]),
            "run ls -la"
        );
        assert_eq!(
            visible_text("assistant", &[ContentPart::InputText(sent.clone())]),
            sent
        );
    }

    /// An xAI compaction item is opaque: saved, loaded, rewritten (rename) and
    /// loaded again, it is the exact text that came in, spacing and escapes too.
    #[test]
    fn a_server_compaction_item_round_trips_byte_for_byte() {
        let (_dir, _guard) = isolated("server-compact");
        let id = "native-server-compact";
        let raw =
            r#"{ "id":"cmp_7", "type":"compaction","encrypted_content":"gAAAAB+x/y==\u00e9" }"#;
        let digest = "User: OLD_TOPIC\nAssistant: old reply";
        let item = InputItem::server_compaction(raw)
            .unwrap()
            .with_digest(digest);
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[user("OLD_TOPIC"), assistant("old reply")],
            &Usage::default(),
            "API credits",
            "OLD_TOPIC",
        )
        .unwrap();
        record_compaction(
            id,
            "/work",
            "grok-4.7",
            &[item.clone(), assistant("keep me")],
            &Usage::default(),
            "API credits",
        )
        .unwrap();
        let want = vec![
            InputItem::ServerCompaction {
                raw: raw.to_string(),
                digest: digest.to_string(),
            },
            assistant("keep me"),
        ];
        assert_eq!(resume_input(id).unwrap(), want);
        rename_session(id, "Renamed").unwrap();
        let loaded = load_session(id).unwrap();
        assert_eq!(loaded.title, "Renamed");
        assert_eq!(loaded.input(), want);
        let file = fs::read_to_string(session_file(id).unwrap()).unwrap();
        assert!(
            file.contains(r#"{"digest":"User: OLD_TOPIC\nAssistant: old reply","raw":"{ \"id\":\"cmp_7\", \"type\":\"compaction\",\"encrypted_content\":\"gAAAAB+x/y==\\u00e9\" }","type":"server_compaction"}"#),
            "{file}"
        );
        assert_eq!(
            export_markdown(id).unwrap(),
            "# Renamed\n\n## Usage\n\n0 in · 0 out · API credits\n\n\
             _Earlier conversation compacted_\n\n## Assistant\n\nkeep me\n\n\
             ## Usage\n\n0 in · 0 out · API credits\n"
        );
        assert_eq!(
            transcript_pairs(&loaded),
            vec![("assistant".to_string(), "keep me".to_string())]
        );
    }

    /// A sealed reasoning item is saved as the exact text xAI sent, with the
    /// model that wrote it, and resumes as the same item.
    #[test]
    fn a_reasoning_item_round_trips_with_its_model() {
        let (_dir, _guard) = isolated("reasoning-item");
        let id = "native-reasoning-item";
        let raw =
            r#"{"id":"rs_1","summary":[],"type":"reasoning","encrypted_content":"gAAAAB+x/y=="}"#;
        let item = InputItem::reasoning(raw, "grok-4.7").unwrap();
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[user("Q"), item.clone(), assistant("A")],
            &Usage::default(),
            "API credits",
            "Q",
        )
        .unwrap();
        assert_eq!(
            resume_input(id).unwrap(),
            vec![user("Q"), item, assistant("A")]
        );
        let file = fs::read_to_string(session_file(id).unwrap()).unwrap();
        assert!(
            file.contains(r#"{"model":"grok-4.7","raw":"{\"id\":\"rs_1\",\"summary\":[],\"type\":\"reasoning\",\"encrypted_content\":\"gAAAAB+x/y==\"}","type":"reasoning_item"}"#),
            "{file}"
        );
        assert!(!export_markdown(id).unwrap().contains("gAAAA"));
    }

    #[test]
    fn heads_list_what_the_full_read_lists_from_first_lines_alone() {
        let (_dir, _guard) = isolated("heads");
        let usage = Usage::default();
        record_turn(
            "native-a",
            "/work/a",
            "grok-4",
            &[user("first"), assistant("one")],
            &usage,
            "",
            "first",
        )
        .unwrap();
        rename_session("native-a", "Fan curve").unwrap();
        let head = |id: &str, created: u64| {
            header_json(&SessionHead {
                id: id.into(),
                title: format!("Title {id}"),
                created,
                cwd: String::new(),
                model: "grok-4".into(),
                renamed: false,
                forked_from: String::new(),
            })
        };
        let dir = sessions_dir();
        // A torn last turn: the header still lists, as read_session keeps it.
        fs::write(
            dir.join("native-torn.jsonl"),
            format!("{}\n{{\"type\":\"mess", head("native-torn", 2)),
        )
        .unwrap();
        // Blank lines before the header are skipped by both.
        fs::write(
            dir.join("native-gap.jsonl"),
            format!("\n\n{}\n", head("native-gap", 1)),
        )
        .unwrap();
        // A header that never ended, an empty file and a first line that is
        // not a header: neither read lists them.
        fs::write(
            dir.join("native-unended.jsonl"),
            head("native-unended", 3).to_string(),
        )
        .unwrap();
        fs::write(dir.join("native-blank.jsonl"), "").unwrap();
        fs::write(dir.join("native-junk.jsonl"), "not json\n{}\n").unwrap();

        let heads = list_session_heads();
        let rows: Vec<(&str, &str, &str)> = heads
            .iter()
            .map(|h| (h.id.as_str(), h.title.as_str(), h.cwd.as_str()))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("native-a", "Fan curve", "/work/a"),
                ("native-torn", "Title native-torn", ""),
                ("native-gap", "Title native-gap", ""),
            ]
        );
        assert!(heads[0].renamed);
        let full: Vec<(String, String)> = list_sessions()
            .into_iter()
            .map(|i| (i.id, i.title))
            .collect();
        let short: Vec<(String, String)> = heads.into_iter().map(|h| (h.id, h.title)).collect();
        assert_eq!(full, short);
    }
}

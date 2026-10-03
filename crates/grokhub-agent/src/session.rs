//! Native session store. One append-only JSONL file per session under the GrokHub data dir.
//!
//! The first line is a header (`id`, `title`, `created`, `cwd`, `model`, `engine: "native"`).
//! Later lines are user and assistant messages, function calls, their outputs, and per-turn usage.
//! Each line is flushed on its own. A torn last line is dropped and the file rewritten. Resume
//! rebuilds the Responses `input` from the kept lines.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use grokhub_acp::GrokSession;
use serde_json::{json, Value};

use crate::{CancelToken, ContentPart, InputItem, Usage};

const ENGINE: &str = "native";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Header {
    id: String,
    title: String,
    created: u64,
    cwd: String,
    model: String,
    renamed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Record {
    Item(InputItem),
    Usage { usage: Usage, meter: String },
}

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
    records: Vec<Record>,
}

impl SessionInfo {
    /// Responses `input` rebuilt from the file. Usage lines are not model input.
    pub fn input(&self) -> Vec<InputItem> {
        self.records
            .iter()
            .filter_map(|record| match record {
                Record::Item(item) => Some(item.clone()),
                Record::Usage { .. } => None,
            })
            .collect()
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

pub fn sessions_dir() -> PathBuf {
    crate::perm::config_dir().join("sessions")
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

pub fn format_cost_ticks(ticks: i64) -> String {
    let sign = if ticks < 0 { "-" } else { "" };
    let ticks = ticks.unsigned_abs();
    let whole = ticks / 1_000_000;
    let frac = ticks % 1_000_000;
    if frac == 0 {
        return format!("{sign}${whole}.00");
    }
    let mut frac_s = format!("{frac:06}");
    while frac_s.ends_with('0') {
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

pub fn list_sessions() -> Vec<SessionInfo> {
    let Ok(entries) = fs::read_dir(sessions_dir()) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if safe_id(id).is_err() {
            continue;
        }
        if let Ok(info) = read_session(&path, id, false) {
            found.push(info);
        }
    }
    found.sort_by(|a, b| b.created.cmp(&a.created).then(a.id.cmp(&b.id)));
    found
}

pub fn load_session(id: &str) -> Result<SessionInfo, String> {
    let path = session_file(id)?;
    read_session(&path, safe_id(id)?, true)
}

pub fn resume_input(id: &str) -> Result<Vec<InputItem>, String> {
    Ok(load_session(id)?.input())
}

/// Create the file if needed and append this turn. Each line is flushed.
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
    let id = safe_id(id)?.to_string();
    if tombstoned(&id) {
        return Ok(());
    }
    touch(&id, cwd, model)?;
    for item in items {
        if tombstoned(&id) {
            let _ = fs::remove_file(session_file(&id)?);
            return Ok(());
        }
        append_json(&id, &item_json(item))?;
    }
    if tombstoned(&id) {
        let _ = fs::remove_file(session_file(&id)?);
        return Ok(());
    }
    append_json(&id, &usage_json(usage, meter))?;
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

pub fn fork_session(id: &str) -> Result<SessionInfo, String> {
    let mut info = load_session(id)?;
    let new_id = format!("native-{}", grokhub_core::uid("n"));
    info.id = new_id;
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
    tombstone(&id);
    stop_run(&id);
    let path = session_file(&id)?;
    if path.is_file() {
        fs::remove_file(&path).map_err(|err| err.to_string())?;
    }
    bump_history();
    Ok(())
}

pub fn export_markdown(id: &str) -> Result<String, String> {
    let info = load_session(id)?;
    let mut out = format!("# {}\n", info.title);
    for record in &info.records {
        match record {
            Record::Item(InputItem::Message { role, .. }) if role == "system" => {}
            Record::Item(InputItem::Message { role, content }) => {
                let heading = if role == "assistant" {
                    "Assistant"
                } else {
                    "User"
                };
                out.push_str(&format!("\n## {heading}\n\n{}\n", message_text(content)));
            }
            Record::Item(InputItem::FunctionCall {
                name, arguments, ..
            }) => {
                out.push_str(&format!("\n## Tool {name}\n\n{arguments}\n"));
            }
            Record::Item(InputItem::FunctionCallOutput { output, .. }) => {
                out.push_str(&format!("\n## Tool result\n\n{output}\n"));
            }
            Record::Usage { usage, meter } => {
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
        }
    }
    Ok(out)
}

pub fn transcript_pairs(info: &SessionInfo) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for record in &info.records {
        match record {
            Record::Item(InputItem::Message { role, content }) if role != "system" => {
                let text = message_text(content);
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
            }) => {
                pairs.push(("assistant".into(), format!("Tool {name}\n{arguments}")));
            }
            Record::Item(InputItem::FunctionCallOutput { output, .. }) => {
                pairs.push(("assistant".into(), format!("Tool result\n{output}")));
            }
            _ => {}
        }
    }
    pairs
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
    let header = header_json(&Header {
        id: id.to_string(),
        title: "Chat".into(),
        created: grokhub_core::now_ms(),
        cwd: cwd.to_string(),
        model: model.to_string(),
        renamed: false,
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
    if tombstoned(id) {
        return Ok(());
    }
    let path = session_file(id)?;
    let mut line = serde_json::to_vec(value).map_err(|err| err.to_string())?;
    line.push(b'\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| err.to_string())?;
    file.write_all(&line).map_err(|err| err.to_string())?;
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
        &header_json(&Header {
            id: info.id.clone(),
            title: info.title.clone(),
            created: info.created,
            cwd: info.cwd.clone(),
            model: info.model.clone(),
            renamed: info.renamed,
        }),
    )?;
    for record in &info.records {
        let value = match record {
            Record::Item(item) => item_json(item),
            Record::Usage { usage, meter } => usage_json(usage, meter),
        };
        push_line(&mut bytes, &value)?;
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
    #[cfg(windows)]
    {
        let _ = fs::remove_file(path);
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

fn assemble(header: Header, records: Vec<Record>) -> SessionInfo {
    let mut usage = Usage::default();
    let mut meter = String::new();
    for record in &records {
        if let Record::Usage {
            usage: turn,
            meter: turn_meter,
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

fn parse_header(value: &Value, fallback_id: &str) -> Option<Header> {
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
    Some(Header {
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
        "usage" => Some(Record::Usage {
            usage: Usage {
                input_tokens: u64_field(value, "input_tokens"),
                output_tokens: u64_field(value, "output_tokens"),
                reasoning_tokens: u64_field(value, "reasoning_tokens"),
                cost_in_usd_ticks: i64_field(value, "cost_in_usd_ticks"),
            },
            meter: string_field(value, "meter"),
        }),
        _ => None,
    }
}

fn header_json(header: &Header) -> Value {
    json!({
        "id": header.id,
        "title": header.title,
        "created": header.created,
        "cwd": header.cwd,
        "model": header.model,
        "engine": ENGINE,
        "renamed": header.renamed,
    })
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
    }
}

fn usage_json(usage: &Usage, meter: &str) -> Value {
    json!({
        "type": "usage",
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "reasoning_tokens": usage.reasoning_tokens,
        "cost_in_usd_ticks": usage.cost_in_usd_ticks,
        "meter": meter,
    })
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
        assert!(md.contains("$0.000009"), "{md}");
        assert!(md.contains("API credits"), "{md}");
        assert_eq!(format_cost_ticks(84_000), "$0.084");
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
            },
            meter: "SuperGrok pool".into(),
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
}

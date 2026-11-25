//! MCP Tasks client (`io.modelcontextprotocol/tasks`, spec 2026-07-28).
//!
//! A stateless `tools/call` may come back as a task handle. This module polls
//! `tasks/get`, answers `input_required` on the elicit card, and returns the
//! same result a synchronous call would have. Task ids stay bound to the chat
//! session and server that created them.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use super::elicit;
use super::rpc;

pub(crate) const EXTENSION: &str = "io.modelcontextprotocol/tasks";

const PER_SERVER: usize = 4;
const PER_RUN: usize = 8;
const POLL_FLOOR_MS: u64 = 50;
const POLL_CAP_MS: u64 = 30_000;

#[derive(Clone)]
struct Bound {
    task_id: String,
    session: String,
    server: String,
    open: bool,
}

enum Gate {
    Refuse(String),
    Cap(String),
}

fn registry() -> &'static Mutex<Vec<Bound>> {
    static REG: OnceLock<Mutex<Vec<Bound>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(Vec::new()))
}

fn lock() -> std::sync::MutexGuard<'static, Vec<Bound>> {
    registry().lock().unwrap_or_else(|err| err.into_inner())
}

pub(crate) fn clear_memory() {
    lock().clear();
}

pub(crate) fn clamp_poll_ms(asked: Option<u64>) -> u64 {
    asked
        .unwrap_or(POLL_FLOOR_MS)
        .clamp(POLL_FLOOR_MS, POLL_CAP_MS)
}

fn belongs(task_id: &str, session: &str, server: &str) -> String {
    format!("refused mcp task `{task_id}`: it belongs to session `{session}` on server `{server}`")
}

fn unknown(task_id: &str, session: &str, server: &str) -> String {
    format!(
        "refused mcp task `{task_id}`: this client did not create it for session `{session}` on server `{server}`"
    )
}

/// `Some` when `session` and `server` must not poll or cancel `task_id`.
pub fn binding_refusal(session: &str, server: &str, task_id: &str) -> Option<String> {
    let reg = lock();
    match reg.iter().find(|row| row.task_id == task_id) {
        None => Some(unknown(task_id, session, server)),
        Some(row) if row.session == session && row.server == server => None,
        Some(row) => Some(belongs(&row.task_id, &row.session, &row.server)),
    }
}

/// Refuse a cancel that is not for the session and server that own `task_id`.
/// Sending `tasks/cancel` stays with the connection that is already polling.
pub fn cancel_task(session: &str, server: &str, task_id: &str) -> Result<(), String> {
    match binding_refusal(session, server, task_id) {
        Some(msg) => Err(msg),
        None => Ok(()),
    }
}

/// Count an open task toward the caps. The sidecar is what a restart reads;
/// those ids are reported orphaned instead of resumed into a dead call.
pub fn seed_open_task(session: &str, server: &str, task_id: &str) {
    {
        let mut reg = lock();
        if reg.iter().any(|row| row.task_id == task_id) {
            return;
        }
        reg.push(Bound {
            task_id: task_id.to_string(),
            session: session.to_string(),
            server: server.to_string(),
            open: true,
        });
    }
    persist(session);
}

pub(crate) fn open_by_server(session: &str) -> Vec<(String, Vec<String>)> {
    let reg = lock();
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in reg.iter().filter(|row| row.open && row.session == session) {
        map.entry(row.server.clone())
            .or_default()
            .push(row.task_id.clone());
    }
    map.into_iter().collect()
}

fn take_open(session: &str, server: &str) -> Vec<String> {
    let ids = {
        let mut reg = lock();
        let mut ids = Vec::new();
        for row in reg
            .iter_mut()
            .filter(|row| row.open && row.session == session && row.server == server)
        {
            row.open = false;
            ids.push(row.task_id.clone());
        }
        ids
    };
    if !ids.is_empty() {
        persist(session);
    }
    ids
}

pub(crate) fn finish_id(task_id: &str) {
    finish(task_id);
}

fn finish(task_id: &str) {
    let session = {
        let mut reg = lock();
        let row = reg.iter_mut().find(|row| row.task_id == task_id);
        if let Some(row) = row {
            row.open = false;
            Some(row.session.clone())
        } else {
            None
        }
    };
    if let Some(session) = session {
        persist(&session);
    }
}

fn gate(session: &str, server: &str, task_id: &str) -> Result<(), Gate> {
    let mut reg = lock();
    if let Some(row) = reg.iter().find(|row| row.task_id == task_id) {
        if row.session != session || row.server != server || !row.open {
            return Err(Gate::Refuse(belongs(task_id, &row.session, &row.server)));
        }
        return Ok(());
    }
    let on_server = reg
        .iter()
        .filter(|row| row.open && row.session == session && row.server == server)
        .count();
    if on_server >= PER_SERVER {
        return Err(Gate::Cap(format!(
            "mcp task cap reached: {PER_SERVER} in flight on server `{server}` (run cap {PER_RUN}); refused `{task_id}`"
        )));
    }
    let on_run = reg
        .iter()
        .filter(|row| row.open && row.session == session)
        .count();
    if on_run >= PER_RUN {
        return Err(Gate::Cap(format!(
            "mcp task cap reached: {PER_RUN} in flight on this run (server cap {PER_SERVER}); refused `{task_id}`"
        )));
    }
    reg.push(Bound {
        task_id: task_id.to_string(),
        session: session.to_string(),
        server: server.to_string(),
        open: true,
    });
    drop(reg);
    persist(session);
    Ok(())
}

pub(crate) fn await_task(
    session: &str,
    server: &str,
    created: &Value,
    stop: &dyn Fn() -> bool,
    send: &mut dyn FnMut(&str, Value) -> Result<Value, String>,
) -> Result<Value, String> {
    let id = created
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| format!("MCP server `{server}` returned a task without a taskId"))?
        .to_string();
    match gate(session, server, &id) {
        Ok(()) => {}
        Err(Gate::Refuse(msg)) => return Err(msg),
        Err(Gate::Cap(msg)) => {
            let _ = send("tasks/cancel", json!({ "taskId": id }));
            return Err(msg);
        }
    }
    let mut snap = created.clone();
    let mut answered = HashSet::new();
    let mut input_rounds = 0usize;
    loop {
        if stop() {
            cancel_open(session, server, send);
            return Err(format!("mcp task `{id}` cancelled"));
        }
        if let Some(ttl) = expired(&snap) {
            finish(&id);
            return Err(format!("mcp task `{id}` expired after {ttl}ms"));
        }
        match snap.get("status").and_then(Value::as_str).unwrap_or("") {
            "completed" => {
                finish(&id);
                return Ok(snap.get("result").cloned().unwrap_or_else(|| json!({})));
            }
            "failed" => {
                finish(&id);
                let err = snap.get("error").cloned().unwrap_or(Value::Null);
                return Err(failed_text(&err));
            }
            "cancelled" => {
                finish(&id);
                return Err(format!("mcp task `{id}` cancelled"));
            }
            "input_required" => {
                input_rounds += 1;
                if input_rounds > rpc::MAX_INPUT_ROUNDS {
                    finish(&id);
                    return Err(format!(
                        "MCP server `{server}` kept asking for more input; stopped after {} answers",
                        rpc::MAX_INPUT_ROUNDS
                    ));
                }
                match answer_new(server, &snap, &mut answered) {
                    Ok(Some(responses)) => {
                        send(
                            "tasks/update",
                            json!({ "taskId": id, "inputResponses": responses }),
                        )?;
                        snap = tasks_get(send, &id)?;
                    }
                    Ok(None) => {
                        if sleep_poll(poll_of(&snap), stop) {
                            cancel_open(session, server, send);
                            return Err(format!("mcp task `{id}` cancelled"));
                        }
                        snap = tasks_get(send, &id)?;
                    }
                    Err(err) => {
                        finish(&id);
                        return Err(err);
                    }
                }
            }
            _ => {
                if sleep_poll(poll_of(&snap), stop) {
                    cancel_open(session, server, send);
                    return Err(format!("mcp task `{id}` cancelled"));
                }
                if let Some(ttl) = expired(&snap) {
                    finish(&id);
                    return Err(format!("mcp task `{id}` expired after {ttl}ms"));
                }
                snap = tasks_get(send, &id)?;
            }
        }
    }
}

fn cancel_open(
    session: &str,
    server: &str,
    send: &mut dyn FnMut(&str, Value) -> Result<Value, String>,
) {
    for id in take_open(session, server) {
        let _ = send("tasks/cancel", json!({ "taskId": id }));
    }
}

fn tasks_get(
    send: &mut dyn FnMut(&str, Value) -> Result<Value, String>,
    id: &str,
) -> Result<Value, String> {
    send("tasks/get", json!({ "taskId": id }))
}

fn poll_of(snap: &Value) -> u64 {
    clamp_poll_ms(snap.get("pollIntervalMs").and_then(Value::as_u64))
}

fn failed_text(error: &Value) -> String {
    rpc::rpc_error(&json!({ "error": error })).unwrap_or_else(|| "MCP error".to_string())
}

fn answer_new(
    server: &str,
    snap: &Value,
    answered: &mut HashSet<String>,
) -> Result<Option<Value>, String> {
    let Some(asks) = snap.get("inputRequests").and_then(Value::as_object) else {
        return Err(format!(
            "MCP server `{server}` asked for more input without saying what"
        ));
    };
    let mut responses = Map::new();
    for (key, ask) in asks {
        if answered.contains(key) {
            continue;
        }
        let method = ask.get("method").and_then(Value::as_str).unwrap_or("");
        if method != "elicitation/create" {
            return Err(format!(
                "MCP server `{server}` asked for {}, which this client does not offer",
                rpc::one_line(method, 60)
            ));
        }
        let params = ask.get("params").cloned().unwrap_or_else(|| json!({}));
        responses.insert(key.clone(), elicit::answer_input(server, key, &params));
        answered.insert(key.clone());
    }
    if responses.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Value::Object(responses)))
    }
}

fn sleep_poll(ms: u64, stop: &dyn Fn() -> bool) -> bool {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        if stop() {
            return true;
        }
        let left = end.saturating_duration_since(Instant::now());
        thread::sleep(left.min(Duration::from_millis(50)));
    }
    stop()
}

fn expired(snap: &Value) -> Option<u64> {
    let ttl = snap.get("ttlMs").and_then(Value::as_u64)?;
    let created = snap
        .get("createdAt")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339_ms)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    if now >= created.saturating_add(ttl as i64) {
        Some(ttl)
    } else {
        None
    }
}

fn parse_rfc3339_ms(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once('T')?;
    let mut date = date.split('-');
    let year: i32 = date.next()?.parse().ok()?;
    let month: u32 = date.next()?.parse().ok()?;
    let day: u32 = date.next()?.parse().ok()?;
    if date.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let rest = rest.strip_suffix('Z')?;
    let (time, frac) = rest.split_once('.').unwrap_or((rest, "0"));
    let mut time = time.split(':');
    let hour: u32 = time.next()?.parse().ok()?;
    let minute: u32 = time.next()?.parse().ok()?;
    let second: u32 = time.next()?.parse().ok()?;
    if time.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let millis = if frac.is_empty() {
        0
    } else {
        let digits: String = frac.chars().take(3).collect();
        let n: u32 = digits.parse().ok()?;
        match digits.len() {
            1 => n * 100,
            2 => n * 10,
            _ => n,
        }
    };
    Some(unix_ms(year, month, day, hour, minute, second, millis))
}

fn unix_ms(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = (year - era * 400) as u64;
    let month_shift = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_shift as u64 + 2) / 5 + day as u64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era as i64 * 146097 + day_of_era as i64 - 719468;
    days * 86_400_000
        + hour as i64 * 3_600_000
        + minute as i64 * 60_000
        + second as i64 * 1000
        + millis as i64
}

fn session_file(session: &str) -> Option<PathBuf> {
    if session.is_empty() || session.contains(['/', '\\', '\0']) || session.contains("..") {
        return None;
    }
    Some(crate::session::sessions_dir().join(format!("{session}.mcp-tasks.json")))
}

fn persist(session: &str) {
    let Some(path) = session_file(session) else {
        return;
    };
    let tasks: Vec<Value> = {
        let reg = lock();
        reg.iter()
            .filter(|row| row.session == session)
            .map(|row| {
                json!({
                    "taskId": row.task_id,
                    "server": row.server,
                    "open": row.open,
                })
            })
            .collect()
    };
    write_json(&path, &json!({ "tasks": tasks }));
}

fn write_json(path: &Path, value: &Value) {
    let Some(dir) = path.parent() else {
        return;
    };
    let _ = fs::create_dir_all(dir);
    let tmp = path.with_extension("json.tmp");
    let Ok(text) = serde_json::to_string(value) else {
        return;
    };
    if fs::write(&tmp, text).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

fn read_json(path: &Path) -> Value {
    let Ok(text) = fs::read_to_string(path) else {
        return json!({ "tasks": [] });
    };
    serde_json::from_str(&text).unwrap_or_else(|_| json!({ "tasks": [] }))
}

/// Open task ids from a previous process, reported once and then closed.
pub(crate) fn take_orphans(session: &str) -> Vec<String> {
    let Some(path) = session_file(session) else {
        return Vec::new();
    };
    if !path.exists() {
        return Vec::new();
    }
    let mut doc = read_json(&path);
    let Some(tasks) = doc.get_mut("tasks").and_then(Value::as_array_mut) else {
        return Vec::new();
    };
    let live: HashSet<String> = {
        let reg = lock();
        reg.iter()
            .filter(|row| row.session == session)
            .map(|row| row.task_id.clone())
            .collect()
    };
    let mut msgs = Vec::new();
    for task in tasks.iter_mut() {
        let id = task
            .get("taskId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let server = task
            .get("server")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let open = task.get("open").and_then(Value::as_bool).unwrap_or(false);
        if open && !id.is_empty() && !live.contains(&id) {
            task["open"] = json!(false);
            msgs.push(format!("orphaned mcp task `{id}` on server `{server}`"));
        }
    }
    if !msgs.is_empty() {
        write_json(&path, &doc);
    }
    msgs
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Reset;

    impl Drop for Reset {
        fn drop(&mut self) {
            clear_memory();
        }
    }

    fn scratch(label: &str) -> (PathBuf, Reset, crate::perm::ConfigGuard) {
        clear_memory();
        let dir = std::env::temp_dir().join(format!(
            "gh-tasks-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let guard = crate::perm::ConfigGuard::set(&dir);
        (dir, Reset, guard)
    }

    #[test]
    fn poll_interval_is_clamped_to_50_and_30000() {
        assert_eq!(clamp_poll_ms(None), 50);
        assert_eq!(clamp_poll_ms(Some(1)), 50);
        assert_eq!(clamp_poll_ms(Some(50)), 50);
        assert_eq!(clamp_poll_ms(Some(500)), 500);
        assert_eq!(clamp_poll_ms(Some(30_000)), 30_000);
        assert_eq!(clamp_poll_ms(Some(31_000)), 30_000);
    }

    #[test]
    fn rfc3339_utc_parses_to_unix_milliseconds() {
        assert_eq!(
            parse_rfc3339_ms("2020-01-01T00:00:00.000Z"),
            Some(1_577_836_800_000)
        );
        assert_eq!(
            parse_rfc3339_ms("2026-01-01T00:00:00Z"),
            Some(1_767_225_600_000)
        );
        assert_eq!(parse_rfc3339_ms("not-a-time"), None);
    }

    #[test]
    fn eight_open_tasks_on_one_run_refuse_the_next_one() {
        let (_dir, _reset, _guard) = scratch("run-cap");
        for i in 0..8 {
            seed_open_task("s", &format!("srv{i}"), &format!("task-run-{i}"));
        }
        let err = match gate("s", "box", "task-run-new") {
            Err(Gate::Cap(msg)) => msg,
            Err(Gate::Refuse(msg)) => panic!("expected the run cap, refused: {msg}"),
            Ok(()) => panic!("expected the run cap"),
        };
        assert_eq!(
            err,
            "mcp task cap reached: 8 in flight on this run (server cap 4); refused `task-run-new`"
        );
    }

    #[test]
    fn four_open_tasks_on_one_server_refuse_the_next_one() {
        let (_dir, _reset, _guard) = scratch("server-cap");
        for i in 0..4 {
            seed_open_task("s", "box", &format!("task-srv-{i}"));
        }
        let err = match gate("s", "box", "task-srv-new") {
            Err(Gate::Cap(msg)) => msg,
            _ => panic!("expected the server cap"),
        };
        assert_eq!(
            err,
            "mcp task cap reached: 4 in flight on server `box` (run cap 8); refused `task-srv-new`"
        );
    }

    #[test]
    fn a_task_id_is_refused_from_another_session_or_server() {
        let (_dir, _reset, _guard) = scratch("bind");
        seed_open_task("s", "box", "task-cross-server");
        assert_eq!(
            binding_refusal("other", "box", "task-cross-server").as_deref(),
            Some("refused mcp task `task-cross-server`: it belongs to session `s` on server `box`")
        );
        assert_eq!(
            binding_refusal("s", "elsewhere", "task-cross-server").as_deref(),
            Some("refused mcp task `task-cross-server`: it belongs to session `s` on server `box`")
        );
        assert_eq!(binding_refusal("s", "box", "task-cross-server"), None);
        assert_eq!(
            cancel_task("other", "box", "task-cross-server"),
            Err(
                "refused mcp task `task-cross-server`: it belongs to session `s` on server `box`"
                    .into()
            )
        );
        assert_eq!(
            binding_refusal("s", "box", "task-missing").as_deref(),
            Some(
                "refused mcp task `task-missing`: this client did not create it for session `s` on server `box`"
            )
        );
    }

    #[test]
    fn a_restart_reports_an_open_task_orphaned_once() {
        let (_dir, _reset, _guard) = scratch("orphan");
        seed_open_task("s", "box", "task-orphan-1");
        clear_memory();
        assert_eq!(
            take_orphans("s"),
            vec!["orphaned mcp task `task-orphan-1` on server `box`".to_string()]
        );
        assert!(take_orphans("s").is_empty());
    }
}

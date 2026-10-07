// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Background output, monitors, and automations.
//! `scheduler_*` writes the cabin `automations.json`. There is no second scheduler.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use grokhub_core::{ensure_automation_schedule, now_ms, uid, Automation, LocalClock, LOOP_MAX};
use serde_json::{json, Value};

use super::{ToolCtx, ToolOutput};
use crate::tasks::{TaskHub, MONITOR_CAP_MS};

const STORE_CAP: u64 = 32 * 1024 * 1024;
const DAY_SECS: u64 = 24 * 60 * 60;
const MAX_MIN: u32 = 24 * 60;

static STORE: Mutex<()> = Mutex::new(());

/// A scheduler tool change for the app's in-memory automations list. The app keeps
/// that list and saves it on every persist, so it must apply these or its next save
/// would drop what the tool wrote.
#[derive(Debug, Clone, PartialEq)]
pub enum AutomationChange {
    Upsert(Box<Automation>),
    Delete(String),
}

static CHANGES: Mutex<Vec<AutomationChange>> = Mutex::new(Vec::new());

fn note_change(change: AutomationChange) {
    CHANGES
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .push(change);
}

/// Drain the scheduler changes since the last call (polled by the app each frame).
pub fn take_automation_changes() -> Vec<AutomationChange> {
    std::mem::take(&mut *CHANGES.lock().unwrap_or_else(|err| err.into_inner()))
}

pub fn output_schema() -> Value {
    json!({
        "type": "function",
        "name": "get_command_or_subagent_output",
        "description": "Read new output from a background command or subagent. Pass the task id. timeout_ms waits up to 120000 milliseconds. Omit it or pass 0 for a snapshot of what is new since the last read.",
        "parameters": {
            "type": "object",
            "properties": {
                "task_id": {"type": "string"},
                "timeout_ms": {"type": "integer"}
            },
            "required": ["task_id"],
            "additionalProperties": false
        }
    })
}

pub fn kill_schema() -> Value {
    json!({
        "type": "function",
        "name": "kill_command_or_subagent",
        "description": "Kill a background command, stop a monitor, or cancel a subagent. The whole process tree dies.",
        "parameters": {
            "type": "object",
            "properties": {
                "task_id": {"type": "string"}
            },
            "required": ["task_id"],
            "additionalProperties": false
        }
    })
}

pub fn monitor_schema() -> Value {
    json!({
        "type": "function",
        "name": "monitor",
        "description": "Watch a background task, or start a command and watch its lines. Each matching line is delivered at the next turn. Caps at 10 hours. persistent keeps it for that cap. A command is not read-only.",
        "parameters": {
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "task_id": {"type": "string"},
                "pattern": {"type": "string", "description": "Regular expression. Empty matches every non-empty line."},
                "timeout_ms": {"type": "integer"},
                "persistent": {"type": "boolean"},
                "description": {"type": "string"}
            },
            "required": [],
            "additionalProperties": false
        }
    })
}

pub fn scheduler_create_schema() -> Value {
    json!({
        "type": "function",
        "name": "scheduler_create",
        "description": "Create or update a GrokHub automation. interval is at least 60s and at most 1d (10m, 1h, 1d). The job is stored in automations.json and the existing pulse fires it. fire_immediately makes the next run due now.",
        "parameters": {
            "type": "object",
            "properties": {
                "interval": {"type": "string"},
                "prompt": {"type": "string"},
                "task_id": {"type": "string", "description": "Update this automation id instead of creating one."},
                "fire_immediately": {"type": "boolean"},
                "durable": {"type": "boolean", "description": "Ignored. Automations are stored on disk."}
            },
            "required": ["interval", "prompt"],
            "additionalProperties": false
        }
    })
}

pub fn scheduler_delete_schema() -> Value {
    json!({
        "type": "function",
        "name": "scheduler_delete",
        "description": "Delete one automation from automations.json.",
        "parameters": {
            "type": "object",
            "properties": {
                "task_id": {"type": "string"}
            },
            "required": ["task_id"],
            "additionalProperties": false
        }
    })
}

pub fn scheduler_list_schema() -> Value {
    json!({
        "type": "function",
        "name": "scheduler_list",
        "description": "List automations stored in automations.json.",
        "parameters": {
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false
        }
    })
}

pub fn output(tasks: Option<&Arc<TaskHub>>, args: &Value) -> ToolOutput {
    let Some(tasks) = tasks else {
        return ToolOutput::err("background tasks are not available on this run");
    };
    let Some(id) = task_id(args) else {
        return ToolOutput::err("task_id is required");
    };
    let timeout_ms = args.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    match tasks.read_output(&id, timeout_ms) {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(err),
    }
}

pub fn kill(tasks: Option<&Arc<TaskHub>>, args: &Value) -> ToolOutput {
    let Some(tasks) = tasks else {
        return ToolOutput::err("background tasks are not available on this run");
    };
    let Some(id) = task_id(args) else {
        return ToolOutput::err("task_id is required");
    };
    match tasks.kill(&id) {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(err),
    }
}

pub fn monitor(ctx: &ToolCtx<'_>, args: &Value) -> ToolOutput {
    let Some(tasks) = ctx.tasks.as_ref() else {
        return ToolOutput::err("background tasks are not available on this run");
    };
    let command = text_field(args, "command");
    let watched = text_field(args, "task_id");
    let pattern = text_field(args, "pattern");
    let description = text_field(args, "description").unwrap_or_default();
    let persistent = args
        .get("persistent")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let timeout_ms = if persistent {
        MONITOR_CAP_MS
    } else {
        args.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(0)
    };
    match tasks.start_monitor(
        ctx.workspace,
        command,
        watched,
        pattern,
        timeout_ms,
        description,
    ) {
        Ok(id) => ToolOutput::ok(format!("monitor id: {id}\nstatus: running")),
        Err(err) => ToolOutput::err(err),
    }
}

pub fn scheduler_create(args: &Value) -> ToolOutput {
    let interval = text_field(args, "interval").unwrap_or_default();
    let prompt = text_field(args, "prompt").unwrap_or_default();
    if prompt.is_empty() {
        return ToolOutput::err("prompt is required");
    }
    let mins = match interval_minutes(&interval) {
        Ok(mins) => mins,
        Err(err) => return ToolOutput::err(err),
    };
    let fire = args
        .get("fire_immediately")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let task_id = text_field(args, "task_id");
    match write_automation(&prompt, mins, fire, task_id.as_deref(), now_ms()) {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(err),
    }
}

pub fn scheduler_delete(args: &Value) -> ToolOutput {
    let Some(id) = task_id(args) else {
        return ToolOutput::err("task_id is required");
    };
    match delete_automation(&id) {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(err),
    }
}

pub fn scheduler_list() -> ToolOutput {
    match list_automations() {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(err),
    }
}

fn task_id(args: &Value) -> Option<String> {
    text_field(args, "task_id").or_else(|| {
        args.get("task_ids")
            .and_then(|v| v.as_array())
            .and_then(|items| items.first())
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

fn text_field(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn interval_minutes(raw: &str) -> Result<u32, String> {
    let raw = raw.trim();
    let split = raw
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| "interval needs a unit: s, m, h, or d".to_string())?;
    let (num, unit) = raw.split_at(split);
    let unit = unit.trim();
    if num.is_empty() {
        return Err("interval needs a number".into());
    }
    let n: u64 = num
        .parse()
        .map_err(|_| "interval is not a number".to_string())?;
    let secs = match unit {
        "s" => n,
        "m" => n.saturating_mul(60),
        "h" => n.saturating_mul(60 * 60),
        "d" => n.saturating_mul(DAY_SECS),
        _ => return Err("interval unit must be s, m, h, or d".into()),
    };
    if secs < 60 {
        return Err("interval must be at least 60s".into());
    }
    if secs > DAY_SECS {
        return Err("interval must be at most 1d".into());
    }
    let mins = u32::try_from(secs.div_ceil(60)).unwrap_or(MAX_MIN);
    Ok(mins.clamp(1, MAX_MIN))
}

fn clock() -> LocalClock {
    LocalClock {
        now_ms: now_ms(),
        weekday: 0,
        hour: 0,
        minute: 0,
    }
}

fn store_path() -> std::path::PathBuf {
    crate::perm::config_dir().join("automations.json")
}

fn load_list(path: &Path) -> Result<Vec<Automation>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let len = fs::metadata(path).map_err(|err| err.to_string())?.len();
    if len > STORE_CAP {
        return Err("automations.json is too large".into());
    }
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&text).map_err(|err| format!("automations.json: {err}"))
}

fn save_list(path: &Path, list: &[Automation]) -> Result<(), String> {
    let body = serde_json::to_string_pretty(list).map_err(|err| err.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body.as_bytes()).map_err(|err| err.to_string())?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            let _ = fs::remove_file(path);
            fs::rename(&tmp, path).map_err(|err| err.to_string())
        }
    }
}

/// Save the list through the ChangeLedger: these tools are the agent's own
/// changes (Spike-5b), so each keeps the version it replaces and shows a
/// Work-tree row with Undo. Returns the ledger seq (0 when nothing changed).
fn ledgered(path: &Path, id: &str, reason: &str, write: impl FnOnce() -> Result<(), String>) -> Result<u64, String> {
    let target = crate::harness::AutomationsFile { path, id };
    let config = crate::perm::config_dir();
    let change = crate::harness::record_change(&config, &target, crate::harness::Origin::SelfManage, reason, write)?;
    Ok(change.map(|c| c.seq).unwrap_or(0))
}

fn write_automation(
    prompt: &str,
    mins: u32,
    fire: bool,
    task_id: Option<&str>,
    now_ms: u64,
) -> Result<String, String> {
    let _guard = STORE.lock().unwrap_or_else(|err| err.into_inner());
    let path = store_path();
    let mut list = load_list(&path)?;
    let mut now = clock();
    now.now_ms = now_ms;
    let name = auto_name(prompt);
    if let Some(id) = task_id {
        let Some(row) = list.iter_mut().find(|row| row.id == id) else {
            return Err(format!("automation {id} not found"));
        };
        row.name = name;
        row.instructions = prompt.to_string();
        row.schedule = "heartbeat".into();
        row.heartbeat_every_min = mins;
        row.enabled = true;
        if fire {
            row.last_run = None;
        }
        let id = row.id.clone();
        let mut row = list.iter().find(|row| row.id == id).cloned().expect("row");
        row = ensure_automation_schedule(row, now);
        if fire {
            row.next_run = Some(now.now_ms);
            row.last_run = None;
        }
        if let Some(slot) = list.iter_mut().find(|item| item.id == id) {
            *slot = row.clone();
        }
        ledgered(&path, &id, &format!("changed to every {mins} min"), || save_list(&path, &list))?;
        note_change(AutomationChange::Upsert(Box::new(row)));
        return Ok(format!("updated {id}\nevery {mins} min"));
    }
    if list.len() >= LOOP_MAX {
        return Err(format!("at most {LOOP_MAX} automations"));
    }
    if let Some(why) = crate::harness::automation_cap_refusal(&crate::perm::config_dir(), now_ms) {
        return Err(why);
    }
    let mut row = Automation {
        id: uid("auto"),
        name,
        schedule: "heartbeat".into(),
        time: "09:00".into(),
        times: Vec::new(),
        instructions: prompt.to_string(),
        heartbeat_every_min: mins,
        check_command: String::new(),
        enabled: true,
        last_run: if fire { None } else { Some(now.now_ms) },
        next_run: None,
        run_count: 0,
        health: grokhub_core::AutoHealth::default(),
    };
    row = ensure_automation_schedule(row, now);
    if fire {
        row.next_run = Some(now.now_ms);
        row.last_run = None;
    }
    let id = row.id.clone();
    list.push(row.clone());
    ledgered(&path, &id, &format!("scheduled every {mins} min"), || save_list(&path, &list))?;
    note_change(AutomationChange::Upsert(Box::new(row)));
    Ok(format!("created {id}\nevery {mins} min"))
}

fn delete_automation(id: &str) -> Result<String, String> {
    let _guard = STORE.lock().unwrap_or_else(|err| err.into_inner());
    let path = store_path();
    let mut list = load_list(&path)?;
    let before = list.len();
    list.retain(|row| row.id != id);
    if list.len() == before {
        return Err(format!("automation {id} not found"));
    }
    ledgered(&path, id, "removed by the agent", || save_list(&path, &list))?;
    note_change(AutomationChange::Delete(id.to_string()));
    Ok(format!("deleted {id}"))
}

fn list_automations() -> Result<String, String> {
    let _guard = STORE.lock().unwrap_or_else(|err| err.into_inner());
    let list = load_list(&store_path())?;
    if list.is_empty() {
        return Ok("no automations".into());
    }
    let mut out = String::new();
    for row in &list {
        let cadence = if row.schedule == "heartbeat" {
            format!("every {} min", row.heartbeat_every_min.max(1))
        } else {
            row.schedule.clone()
        };
        out.push_str(&format!(
            "{}\t{}\t{}\n{}\n\n",
            row.id, cadence, row.name, row.instructions
        ));
    }
    Ok(out.trim_end().to_string())
}

fn auto_name(prompt: &str) -> String {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("automation");
    let name: String = line.chars().take(80).collect();
    if name.is_empty() {
        "automation".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{dispatch, ToolCtx};

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-auto-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scheduler_tools_create_list_and_delete_in_the_store() {
        let dir = scratch("store");
        let _guard = crate::perm::ConfigGuard::set(&dir);
        let ctx = ToolCtx {
            workspace: &dir,
            desktop: None,
            stop: &|| false,
            tasks: None,
            owner: None,
        };
        let made = dispatch(
            &ctx,
            "scheduler_create",
            r#"{"interval":"10m","prompt":"check the build"}"#,
        );
        assert!(!made.failed, "{}", made.text);
        let raw = std::fs::read_to_string(dir.join("automations.json")).unwrap();
        let list: Vec<Automation> = serde_json::from_str(&raw).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].schedule, "heartbeat");
        assert_eq!(list[0].heartbeat_every_min, 10);
        assert_eq!(list[0].instructions, "check the build");
        assert!(list[0].next_run.unwrap_or(0) > now_ms().saturating_sub(1_000));
        let id = list[0].id.clone();
        // The app applies this to its in-memory list so its next save keeps the row.
        let created = take_automation_changes();
        assert!(
            created
                .iter()
                .any(|c| matches!(c, AutomationChange::Upsert(row) if row.id == id)),
            "{created:?}"
        );
        let listed = dispatch(&ctx, "scheduler_list", "{}");
        assert!(listed.text.contains(&id), "{}", listed.text);
        assert!(listed.text.contains("check the build"), "{}", listed.text);
        let gone = dispatch(
            &ctx,
            "scheduler_delete",
            &format!(r#"{{"task_id":"{id}"}}"#),
        );
        assert!(!gone.failed, "{}", gone.text);
        let left: Vec<Automation> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("automations.json")).unwrap())
                .unwrap();
        assert!(left.is_empty());
        let deleted = take_automation_changes();
        assert!(
            deleted
                .iter()
                .any(|c| matches!(c, AutomationChange::Delete(gone) if *gone == id)),
            "{deleted:?}"
        );
        let broken = dir.join("automations.json");
        std::fs::write(&broken, "{not json").unwrap();
        let failed = dispatch(&ctx, "scheduler_list", "{}");
        assert!(failed.failed, "{}", failed.text);
        assert_eq!(std::fs::read_to_string(&broken).unwrap(), "{not json");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interval_rejects_a_short_or_long_gap() {
        assert!(interval_minutes("30s").is_err());
        assert_eq!(interval_minutes("60s").unwrap(), 1);
        assert_eq!(interval_minutes("1d").unwrap(), 24 * 60);
        assert!(interval_minutes("2d").is_err());
    }
}

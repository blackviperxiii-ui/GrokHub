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
        "description": "Read new output from background commands or subagents. task_id: an id, or ids joined by commas to wait for all. timeout_ms waits up to 120000 ms; 0 or none: a snapshot of what is new.",
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

/// The lead's request to merge a worktree helper's work.
pub const MERGE_WORK: &str = "merge_subagent_work";
/// What a helper that calls it gets back.
pub(crate) const ONLY_LEAD: &str = "only the lead can ask for a merge";
const NO_WORK_KEY: &str = "work is required";

pub fn merge_schema() -> Value {
    json!({
        "type": "function",
        "name": MERGE_WORK,
        "description": "Ask the person to merge a worktree helper's finished work into the project folder. work: the key on the helper's result line (work: <key>). The person is asked every time; nothing is merged without their yes. Only finished work that is ready to merge, or was in conflict, can be merged this way. The merge leaves uncommitted changes in the project.",
        "parameters": {
            "type": "object",
            "properties": {
                "work": {"type": "string"}
            },
            "required": ["work"],
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
        "description": grokhub_core::brand::filled("Create or update a {product} automation. interval is at least 60s and at most 1d (10m, 1h, 1d). The job is stored in automations.json and the existing pulse fires it. fire_immediately makes the next run due now."),
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
    let timeout_ms = args.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    let read = match task_ids(args).as_slice() {
        [] => return ToolOutput::err("task_id is required"),
        [one] => tasks.read_output(one, timeout_ms),
        many => tasks.read_outputs(many, timeout_ms),
    };
    match read {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(with_id_hint(err)),
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
        Err(err) => ToolOutput::err(with_id_hint(err)),
    }
}

/// Merges a helper's work for the lead. The gate has already asked the
/// person: this tool is a hard class, so the call only gets here on a yes.
pub fn merge(ctx: &ToolCtx<'_>, args: &Value) -> ToolOutput {
    if ctx.owner.is_some() {
        return ToolOutput::err(ONLY_LEAD);
    }
    let Some(session) = ctx.session else {
        return ToolOutput::err("helper work is not available on this run");
    };
    let Some(key) = text_field(args, "work") else {
        return ToolOutput::err(NO_WORK_KEY);
    };
    match crate::work::merge_work(session, &key, crate::work::By::Lead) {
        Ok(work) => {
            let line = crate::work::result_line(&work);
            match work.state {
                crate::work::WorkState::Merged => ToolOutput::ok(line),
                crate::work::WorkState::Held { .. } => {
                    ToolOutput::err(format!("{}. {line}", crate::work::LEAD_MAY_NOT))
                }
                _ => ToolOutput::err(line),
            }
        }
        Err(err) => ToolOutput::err(err),
    }
}

/// The line the approval card of the lead's merge request shows. `Err` is
/// the refusal the request would get after a yes, to give without asking.
pub(crate) fn merge_card(session: &str, arguments: &str) -> Result<String, String> {
    let args: Value = serde_json::from_str(arguments).unwrap_or_default();
    let Some(key) = text_field(&args, "work") else {
        return Err(NO_WORK_KEY.into());
    };
    crate::work::lead_card(session, &key)
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
        Err(err) => ToolOutput::err(with_id_hint(err)),
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

/// An id this chat never started: the model also hears where ids come from.
fn with_id_hint(err: String) -> String {
    if err.starts_with(crate::tasks::UNKNOWN_ID_OPENS) {
        format!("{err} {}", crate::tasks::UNKNOWN_ID_HINT)
    } else {
        err
    }
}

/// The ids an output read waits on: `task_id` split at commas, or every
/// entry of a `task_ids` list.
fn task_ids(args: &Value) -> Vec<String> {
    let listed: Vec<String> = match text_field(args, "task_id") {
        Some(id) => id.split(',').map(String::from).collect(),
        None => args
            .get("task_ids")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
    };
    listed
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .map(String::from)
        .collect()
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

pub(crate) fn interval_minutes(raw: &str) -> Result<u32, String> {
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
fn ledgered(
    path: &Path,
    id: &str,
    reason: &str,
    write: impl FnOnce() -> Result<(), String>,
) -> Result<u64, String> {
    let target = crate::harness::AutomationsFile { path, id };
    let config = crate::perm::config_dir();
    let change = crate::harness::record_change(
        &config,
        &target,
        crate::harness::Origin::SelfManage,
        reason,
        write,
    )?;
    Ok(change.map(|c| c.seq).unwrap_or(0))
}

pub(crate) fn write_automation(
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
        ledgered(&path, &id, &format!("changed to every {mins} min"), || {
            save_list(&path, &list)
        })?;
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
    ledgered(&path, &id, &format!("scheduled every {mins} min"), || {
        save_list(&path, &list)
    })?;
    note_change(AutomationChange::Upsert(Box::new(row)));
    Ok(format!("created {id}\nevery {mins} min"))
}

pub(crate) fn delete_automation(id: &str) -> Result<String, String> {
    let _guard = STORE.lock().unwrap_or_else(|err| err.into_inner());
    let path = store_path();
    let mut list = load_list(&path)?;
    let before = list.len();
    list.retain(|row| row.id != id);
    if list.len() == before {
        return Err(format!("automation {id} not found"));
    }
    ledgered(&path, id, "removed by the agent", || {
        save_list(&path, &list)
    })?;
    note_change(AutomationChange::Delete(id.to_string()));
    Ok(format!("deleted {id}"))
}

/// One automation by id, as stored.
pub(crate) fn find_automation(id: &str) -> Result<Option<Automation>, String> {
    let _guard = STORE.lock().unwrap_or_else(|err| err.into_inner());
    Ok(load_list(&store_path())?
        .into_iter()
        .find(|row| row.id == id))
}

/// Turn one automation off without removing it, through the ChangeLedger
/// (Spike-5c `automation_disable`).
pub(crate) fn disable_automation(id: &str, reason: &str) -> Result<String, String> {
    let _guard = STORE.lock().unwrap_or_else(|err| err.into_inner());
    let path = store_path();
    let mut list = load_list(&path)?;
    let Some(row) = list.iter_mut().find(|row| row.id == id) else {
        return Err(format!("automation {id} not found"));
    };
    if !row.enabled {
        return Ok(format!("{id} is already off"));
    }
    row.enabled = false;
    let row = row.clone();
    ledgered(&path, id, reason, || save_list(&path, &list))?;
    note_change(AutomationChange::Upsert(Box::new(row)));
    Ok(format!("turned off {id}"))
}

pub(crate) fn list_automations() -> Result<String, String> {
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
    fn a_made_up_task_id_tells_the_model_where_ids_come_from() {
        let hub = crate::tasks::TaskHub::new("control-made-up");
        let args = serde_json::json!({ "task_id": "t7" });
        for out in [output(Some(&hub), &args), kill(Some(&hub), &args)] {
            assert!(out.failed);
            assert!(
                out.text.starts_with("No task with id t7 was started"),
                "{}",
                out.text
            );
            assert!(
                out.text.ends_with(crate::tasks::UNKNOWN_ID_HINT),
                "{}",
                out.text
            );
        }
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
            session: None,
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

    /// Spike-5b: scheduler writes are the agent's own, so each keeps a
    /// version, and a third new job in a week is refused in plain words
    /// until the user keeps one.
    #[test]
    fn agent_jobs_are_versioned_and_capped_at_two_a_week() {
        let dir = crate::harness::test_dir("auto-cap");
        let _guard = crate::perm::ConfigGuard::set(&dir);
        let now = now_ms();
        write_automation("check the build", 10, false, None, now).unwrap();
        write_automation("sweep the inbox", 30, false, None, now).unwrap();
        assert_eq!(
            write_automation("water the plants", 60, false, None, now + 1).unwrap_err(),
            "Not added: Loadout already made 2 automations on its own this week. \
             Keep one from its Work-tree row, or add this one yourself on the Automations page."
        );
        assert_eq!(
            load_list(&store_path()).unwrap().len(),
            2,
            "the third was not written"
        );
        // The ledger stamps each line with the real clock, so a week on is
        // counted from after the writes, not from `now`.
        let week_on = now_ms() + crate::harness::WEEK_MS;
        assert!(
            write_automation("water the plants", 60, false, None, week_on).is_ok(),
            "a week on, room again"
        );
        let ledger =
            crate::harness::ChangeLedger::load_kind(&dir, crate::harness::ChangeKind::Automation);
        let lines: Vec<(&str, &str, &str)> = ledger
            .all()
            .iter()
            .map(|c| (c.op.as_str(), c.origin.as_str(), c.reason.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                ("create", "self_manage", "scheduled every 10 min"),
                ("create", "self_manage", "scheduled every 30 min"),
                ("create", "self_manage", "scheduled every 60 min"),
            ]
        );
        let id = ledger.all()[0].id.clone();
        write_automation("check the build twice", 5, false, Some(&id), now).unwrap();
        delete_automation(&id).unwrap();
        let ops: Vec<&str> =
            crate::harness::ChangeLedger::load_kind(&dir, crate::harness::ChangeKind::Automation)
                .all()
                .iter()
                .map(|c| c.op.as_str())
                .collect();
        assert_eq!(ops, ["create", "create", "create", "modify", "delete"]);
        let _ = take_automation_changes();
        let _ = crate::harness::take_self_changes(&dir);
    }

    #[test]
    fn interval_rejects_a_short_or_long_gap() {
        assert!(interval_minutes("30s").is_err());
        assert_eq!(interval_minutes("60s").unwrap(), 1);
        assert_eq!(interval_minutes("1d").unwrap(), 24 * 60);
        assert!(interval_minutes("2d").is_err());
    }

    fn merge_ctx<'a>(
        dir: &'a std::path::Path,
        owner: Option<&'a str>,
        session: Option<&'a str>,
    ) -> ToolCtx<'a> {
        ToolCtx {
            workspace: dir,
            desktop: None,
            stop: &|| false,
            tasks: None,
            owner,
            session,
        }
    }

    #[test]
    fn a_helper_cannot_ask_for_a_merge() {
        let dir = scratch("merge-helper");
        let _guard = crate::perm::ConfigGuard::set(&dir);
        let args = r#"{"work":"mb3x9k2a-7q1z"}"#;
        let out = dispatch(
            &merge_ctx(&dir, Some("s1"), Some("chat/s1")),
            MERGE_WORK,
            args,
        );
        assert!(out.failed);
        assert_eq!(out.text, "only the lead can ask for a merge");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_merge_request_needs_a_key_this_chat_has() {
        let dir = scratch("merge-key");
        let _guard = crate::perm::ConfigGuard::set(&dir);
        let lead = merge_ctx(&dir, None, Some("chat"));
        let none = dispatch(&lead, MERGE_WORK, "{}");
        assert!(none.failed);
        assert_eq!(none.text, "work is required");
        // A task id is not a key, and neither is a path.
        let task = dispatch(&lead, MERGE_WORK, r#"{"work":"s2"}"#);
        assert!(task.failed);
        assert_eq!(task.text, "there is no helper work s2 in this chat");
        let path = dispatch(&lead, MERGE_WORK, r#"{"work":"../chat-2/k1"}"#);
        assert!(path.failed);
        assert_eq!(path.text, "not a helper work key: ../chat-2/k1");
        let unknown = dispatch(&lead, MERGE_WORK, r#"{"work":"mb3x9k2a-7q1z"}"#);
        assert!(unknown.failed);
        assert_eq!(
            unknown.text,
            "there is no helper work mb3x9k2a-7q1z in this chat"
        );
        // The card is not shown for any of these: each is refused first.
        let card = |args: &str| merge_card("chat", args).unwrap_err();
        assert_eq!(card("{}"), "work is required");
        assert_eq!(card("not json"), "work is required");
        assert_eq!(
            card(r#"{"work":"s2"}"#),
            "there is no helper work s2 in this chat"
        );
        assert_eq!(
            card(r#"{"work":"../chat-2/k1"}"#),
            "not a helper work key: ../chat-2/k1"
        );
        let off = dispatch(
            &merge_ctx(&dir, None, None),
            MERGE_WORK,
            r#"{"work":"mb3x9k2a-7q1z"}"#,
        );
        assert!(off.failed);
        assert_eq!(off.text, "helper work is not available on this run");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

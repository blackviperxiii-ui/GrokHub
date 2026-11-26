//! Automations (cross-platform recode, Phase 3b). The shell edits the same
//! `automations.json` the egui cabin uses, so a job made in either shows in
//! both. While the cabin is open its pulse fires them; once it is closed the
//! shell's scheduler (`scheduler.rs`) does. Jobs gated by a check command run
//! the check first, then fire only when it passes. "Run now" and scheduled
//! runs go unattended in Ask mode, so a job can read but every tool that
//! changes something is refused without a card. A job whose words a paired
//! browser wrote runs as a browser's ([`RunOrigin::Browser`]): never with the
//! desktop tools, however its run starts.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use grokhub_agent::{CancelToken, ModelClient};

use crate::unattended::RunGate;
pub use grokhub_agent::web_origin::RunOrigin;
use grokhub_agent::web_origin::{CardPart, JobWords};
use grokhub_core::{Automation, LocalClock};
use serde::{Deserialize, Serialize};

pub const FILE: &str = "automations.json";
/// The cabin's own cap: bigger means the file is not an automations list.
const STORE_CAP: u64 = 4 * 1024 * 1024;

/// The UI's wall clock. The webview knows the local time zone on every
/// platform, so the shell does not ask the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Clock {
    /// 0 is Sunday, as JavaScript's `Date.getDay`.
    pub weekday: u8,
    pub hour: u32,
    pub minute: u32,
}

impl Clock {
    pub fn at(self, now_ms: u64) -> LocalClock {
        LocalClock {
            now_ms,
            weekday: self.weekday % 7,
            hour: self.hour.min(23),
            minute: self.minute.min(59),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub name: String,
    pub instructions: String,
    pub enabled: bool,
    /// "weekdays at 09:00 · next Tue 9:00 AM · 3 runs".
    pub summary: String,
    /// Red line when the last run failed.
    pub health: Option<String>,
    /// The words for the Edit box, `None` when words cannot say the schedule.
    pub describe: Option<String>,
    /// The job's check command; empty when it has none.
    pub check_command: String,
    /// `gate.onChange` / `gate.noAgent` from automations.json.
    pub gate: RowGate,
    /// Condition / event-stream wait summary; empty when none.
    pub wait: String,
    /// Delivery · run · completion line; empty while clear.
    pub lifecycle: String,
}

/// The check gate the Automations page shows on a row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RowGate {
    pub on_change: bool,
    pub no_agent: bool,
}

pub fn rows(list: &[Automation], clock: LocalClock) -> Vec<Row> {
    list.iter()
        .map(|a| Row {
            id: a.id.clone(),
            name: a.name.clone(),
            instructions: a.instructions.clone(),
            enabled: a.enabled,
            summary: grokhub_core::automation_summary_line(a, clock),
            health: grokhub_core::automation_health_line(a),
            describe: grokhub_core::describe_automation(a),
            check_command: a.check_command.clone(),
            gate: RowGate {
                on_change: a.gate.on_change,
                no_agent: a.gate.no_agent,
            },
            wait: grokhub_core::wait_line(&a.wait).unwrap_or_default(),
            lifecycle: grokhub_core::lifecycle_line(&a.lifecycle).unwrap_or_default(),
        })
        .collect()
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

/// One read-change-write at a time, so a Run now that finishes while a row is
/// paused cannot write back a list that misses the pause.
static STORE: Mutex<()> = Mutex::new(());

fn store() -> MutexGuard<'static, ()> {
    STORE.lock().unwrap_or_else(|err| err.into_inner())
}

/// Run `f` with the list read under the store lock, so nothing here writes
/// the file while `f` changes it through the agent's own tools (a Done for
/// you pause, a ledger Undo). `f` must not call [`load`] or any writer here:
/// the lock is not reentrant.
pub(crate) fn with_store<T>(
    config_dir: &Path,
    f: impl FnOnce(Result<Vec<Automation>, String>) -> T,
) -> T {
    let _store = store();
    f(load_unlocked(config_dir))
}

/// A missing file is an empty list. A broken one is an error, so a save can
/// never replace jobs the shell failed to read. Rows with no id get one, and
/// it is written back so the next command finds the same row.
pub fn load(config_dir: &Path) -> Result<Vec<Automation>, String> {
    let _store = store();
    load_unlocked(config_dir)
}

fn load_unlocked(config_dir: &Path) -> Result<Vec<Automation>, String> {
    let path = path(config_dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if len > STORE_CAP {
        return Err(format!("{FILE} is too large to be an automations list."));
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut list: Vec<Automation> =
        serde_json::from_str(&raw).map_err(|e| format!("{FILE}: {e}"))?;
    let mut minted = false;
    for a in &mut list {
        if a.id.is_empty() {
            a.id = grokhub_core::uid("auto");
            minted = true;
        }
    }
    if minted {
        save(config_dir, &list)?;
    }
    Ok(list)
}

/// Writes through a temp file and a rename, like the cabin. Callers hold
/// [`STORE`]; the temp name is this process's own.
fn save(config_dir: &Path, list: &[Automation]) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.shell-{}.tmp", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
}

/// The gate a run of `job` goes behind: `gate`, as a browser's run when a
/// browser run wrote the job (on a page, through a chat's agent, or in the
/// run of another job a browser wrote) and a person hasn't saved the words
/// it runs here since, so it gets no desktop. The words are the saved job's
/// that the run goes on ([`JobWords`]), so a save that lands while it starts
/// can't change whose they are, and what a check or a wait adds to the
/// prompt never does.
pub fn gate_for(config_dir: &Path, job: &Automation, gate: RunGate) -> RunGate {
    gate.with_origin(grokhub_agent::web_origin::job_origin(
        config_dir,
        &job.id,
        &JobWords::of(job),
    ))
}

/// Whose words the card of job `id` carries (its name and its runs'
/// reports): the job's, as [`gate_for`] reads them. The list is read without
/// the store lock, so a caller holding the board lock never waits on it; a
/// save replaces the file whole. A job that can't be read is a browser's
/// when a browser ever wrote it.
pub fn card_job_origin(config_dir: &Path, id: &str) -> RunOrigin {
    let words = std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str::<Vec<Automation>>(&raw).ok())
        .and_then(|list| list.into_iter().find(|a| a.id == id))
        .map_or_else(JobWords::gone, |a| JobWords::of(&a));
    grokhub_agent::web_origin::job_origin(config_dir, id, &words)
}

pub const ADD_HINT: &str =
    "Say when, like \"every weekday at 9am summarize my inbox\" or \"heartbeat every 30 min check the build\".";

pub fn add(config_dir: &Path, text: &str, clock: LocalClock) -> Result<Vec<Automation>, String> {
    add_by(config_dir, text, clock, RunOrigin::Window)
}

/// [`add`], with the words from `by`.
pub fn add_by(
    config_dir: &Path,
    text: &str,
    clock: LocalClock,
    by: RunOrigin,
) -> Result<Vec<Automation>, String> {
    let mut a = grokhub_core::parse_nl_automation(text).ok_or_else(|| ADD_HINT.to_string())?;
    if a.name.trim().is_empty() {
        return Err(ADD_HINT.into());
    }
    if let Some(words) = grokhub_core::automation_check::script_words(text) {
        a.check_command = words.command;
        a.gate = words.gate;
    }
    add_parsed(config_dir, a, clock, by)
}

/// Save an automation already parsed (a Suggested tile's seed), with a new
/// id and its next run. A browser's job is marked before it is saved, so no
/// tick ever finds it unmarked, and so is its check command when it has one.
pub fn add_parsed(
    config_dir: &Path,
    mut a: Automation,
    clock: LocalClock,
    by: RunOrigin,
) -> Result<Vec<Automation>, String> {
    let _store = store();
    let mut list = load_unlocked(config_dir)?;
    a.id = grokhub_core::uid("auto");
    if by == RunOrigin::Browser {
        mark_browser_words(config_dir, &a.id, !a.check_command.trim().is_empty())?;
    }
    list.push(grokhub_core::ensure_automation_schedule(a, clock));
    save(config_dir, &list)?;
    Ok(list)
}

fn change(
    config_dir: &Path,
    id: &str,
    edit: impl FnOnce(&mut Vec<Automation>, usize),
) -> Result<Vec<Automation>, String> {
    let _store = store();
    let mut list = load_unlocked(config_dir)?;
    let at = list
        .iter()
        .position(|a| a.id == id)
        .ok_or_else(|| "That automation is gone.".to_string())?;
    edit(&mut list, at);
    save(config_dir, &list)?;
    Ok(list)
}

pub fn set_enabled(
    config_dir: &Path,
    id: &str,
    enabled: bool,
    clock: LocalClock,
) -> Result<Vec<Automation>, String> {
    let out = change(config_dir, id, |list, at| {
        let mut a = list[at].clone();
        a.enabled = enabled;
        list[at] = grokhub_core::ensure_automation_schedule(a, clock);
    })?;
    grokhub_core::learn::hub::note_all(grokhub_core::learn::sources::automation_toggle(
        id,
        enabled,
        grokhub_core::now_ms(),
    ));
    // Turning a job back on by hand answers a Done for you pause of it.
    if enabled {
        crate::done_for_you::turned_back_on(config_dir, id);
    }
    Ok(out)
}

/// Edit in place: the new words set the name, the ask and the schedule. The
/// id, pause state, run count, last run and health stay, and so does the
/// check command unless the words name one. Words with no time are refused
/// and nothing is written.
pub fn edit(
    config_dir: &Path,
    id: &str,
    text: &str,
    clock: LocalClock,
) -> Result<Vec<Automation>, String> {
    edit_by(config_dir, id, text, clock, RunOrigin::Window)
}

/// [`edit`], with the words from `by`. A browser's words are marked before
/// they are saved. This computer's make the job a person's for exactly the
/// words saved, so a browser's save that lands after them, or one a run
/// started on before them, still runs as a browser's. A check command a
/// browser's words set stays the browser's until the person's words say a
/// check: words that leave it unsaid keep the job a browser's.
pub fn edit_by(
    config_dir: &Path,
    id: &str,
    text: &str,
    clock: LocalClock,
    by: RunOrigin,
) -> Result<Vec<Automation>, String> {
    let parsed = grokhub_core::parse_nl_automation(text).ok_or_else(|| ADD_HINT.to_string())?;
    if parsed.name.trim().is_empty() {
        return Err(ADD_HINT.into());
    }
    let words = grokhub_core::automation_check::script_words(text);
    let check_said = words.is_some();
    if by == RunOrigin::Browser {
        mark_browser_words(config_dir, id, check_said)?;
    }
    let mut saved = None;
    let list = change(config_dir, id, |list, at| {
        let old = &list[at];
        let (check_command, gate) = match words {
            // Same command: keep what it printed last, for the next diff.
            Some(w) if w.command == old.check_command => (
                w.command,
                grokhub_core::automation_check::CheckGate {
                    last_hash: old.gate.last_hash.clone(),
                    last_output: old.gate.last_output.clone(),
                    ..w.gate
                },
            ),
            Some(w) => (w.command, w.gate),
            None => (old.check_command.clone(), old.gate.clone()),
        };
        let next = Automation {
            id: old.id.clone(),
            enabled: old.enabled,
            check_command,
            gate,
            wait: old.wait.clone(),
            last_run: old.last_run,
            next_run: None,
            run_count: old.run_count,
            health: old.health.clone(),
            lifecycle: old.lifecycle.clone(),
            ..parsed
        };
        list[at] = grokhub_core::ensure_automation_schedule(next, clock);
        saved = Some(JobWords::of(&list[at]));
    })?;
    if let (RunOrigin::Window, Some(saved)) = (by, saved) {
        // A failed save of the mark leaves the job without the desktop: the safe side.
        let _ = grokhub_agent::web_origin::job_saved_here(config_dir, id, &saved, check_said);
    }
    Ok(list)
}

/// A browser's words are about to be saved as job `id`'s, and with `check`
/// they set its check command too.
fn mark_browser_words(config_dir: &Path, id: &str, check: bool) -> Result<(), String> {
    if check {
        grokhub_agent::web_origin::mark_job_check(config_dir, id)
    } else {
        grokhub_agent::web_origin::mark_job(config_dir, id)
    }
}

pub fn remove(config_dir: &Path, id: &str) -> Result<Vec<Automation>, String> {
    change(config_dir, id, |list, at| {
        list.remove(at);
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub text: String,
    pub error: String,
    pub stop: String,
}

/// Jobs a Run now or a scheduled run is working on. A second run of the same
/// job is refused.
static RUNNING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub(crate) struct Running(String);

impl Running {
    pub(crate) fn claim(id: &str) -> Result<Self, String> {
        let mut running = RUNNING.lock().unwrap_or_else(|err| err.into_inner());
        if !running
            .get_or_insert_with(HashSet::new)
            .insert(id.to_string())
        {
            return Err("That automation is already running.".into());
        }
        Ok(Self(id.to_string()))
    }

    fn holds(id: &str) -> bool {
        RUNNING
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_ref()
            .is_some_and(|set| set.contains(id))
    }
}

/// A Run now or a scheduled run holds automation `id` now.
pub(crate) fn is_running(id: &str) -> bool {
    Running::holds(id)
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut running = RUNNING.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(set) = running.as_mut() {
            set.remove(&self.0);
        }
    }
}

/// One unattended run of `job`, as the shell's scheduler fires it, behind
/// `gate` (your Approvals; Ask is a read-only session, so an allow rule cannot
/// turn a refused write into one that runs). It records nothing on the row.
/// Like the cabin's scheduled run, it takes the automation's board card into
/// Doing and logs the start and end on it.
pub fn execute(
    config_dir: &Path,
    job: &Automation,
    client: Arc<dyn ModelClient + Send + Sync>,
    model: &str,
    workspace: PathBuf,
    gate: RunGate,
) -> RunResult {
    execute_asking(
        config_dir,
        job,
        &job.instructions,
        client,
        model,
        workspace,
        gate,
    )
}

/// [`execute`], asking `prompt`: the saved `job`'s words with the lines its
/// check command printed or its wait found added. The run is the saved
/// job's: whose words it goes on and the tools its words declare are read
/// from `job`, never from the lines added to them.
pub fn execute_asking(
    config_dir: &Path,
    job: &Automation,
    prompt: &str,
    client: Arc<dyn ModelClient + Send + Sync>,
    model: &str,
    workspace: PathBuf,
    gate: RunGate,
) -> RunResult {
    run_on_board(
        config_dir,
        job,
        prompt,
        client,
        model,
        workspace,
        gate,
        "scheduled",
    )
}

/// [`execute_asking`], with `why` as the note on the card's start log line.
fn run_on_board(
    config_dir: &Path,
    job: &Automation,
    prompt: &str,
    client: Arc<dyn ModelClient + Send + Sync>,
    model: &str,
    workspace: PathBuf,
    gate: RunGate,
    why: &str,
) -> RunResult {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let gate = gate_for(config_dir, job, gate);
    let started = grokhub_core::now_ms();
    let session_id = format!("shell-auto-{}-{started}", job.id);
    let card = board_run_start(config_dir, job, &session_id, model, gate, started, why);
    let done = crate::unattended::run(crate::unattended::Job {
        client,
        model: model.to_string(),
        workspace,
        session_id,
        prompt: prompt.to_string(),
        tools_from: Some(job.instructions.clone()),
        gate,
        cancel: CancelToken::new(),
        resume: false,
    });
    let result = RunResult {
        text: done.text,
        error: done.error,
        stop: done.stop_reason,
    };
    let ran = BoardRun {
        job,
        asked: prompt,
        origin: gate.origin,
    };
    board_run_end(config_dir, ran, card.as_deref(), &result);
    result
}

impl RunResult {
    pub fn failed(&self) -> bool {
        self.stop == "error" || !self.error.is_empty()
    }
}

fn settle(a: Automation, result: &RunResult) -> Automation {
    if result.failed() {
        grokhub_core::mark_automation_failed(a, &result.error)
    } else {
        grokhub_core::mark_automation_ok(a)
    }
}

/// Runs one job now and records the run on its row.
pub fn run_now(
    config_dir: &Path,
    id: &str,
    client: Arc<dyn ModelClient + Send + Sync>,
    model: &str,
    workspace: PathBuf,
    gate: RunGate,
) -> Result<RunResult, String> {
    run_now_preflight(config_dir, id, client, model, workspace, gate, true)
}

/// `run_now`, with an optional route-ready flag for preflight (false when the
/// shell already knows there is no model route).
pub fn run_now_preflight(
    config_dir: &Path,
    id: &str,
    client: Arc<dyn ModelClient + Send + Sync>,
    model: &str,
    workspace: PathBuf,
    gate: RunGate,
    route_ready: bool,
) -> Result<RunResult, String> {
    let job = load(config_dir)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| "That automation is gone.".to_string())?;
    if let Some(why) = run_now_preflight_line(config_dir, &job, &workspace, route_ready) {
        // Run now is not a scheduled claim: mark health only, leave next_run.
        let _ = change(config_dir, id, |list, at| {
            list[at] = grokhub_core::mark_automation_failed(list[at].clone(), &why);
        });
        return Ok(RunResult {
            text: String::new(),
            error: why,
            stop: grokhub_core::PREFLIGHT_STOP.into(),
        });
    }
    let _running = Running::claim(id)?;
    let started = grokhub_core::now_ms();
    let asked = &job.instructions;
    let result = run_on_board(
        config_dir, &job, asked, client, model, workspace, gate, "run now",
    );
    change(config_dir, id, |list, at| {
        let ran = grokhub_core::mark_automation_ran(list[at].clone(), started);
        list[at] = settle(ran, &result);
    })?;
    Ok(result)
}

fn run_now_preflight_line(
    _config_dir: &Path,
    job: &Automation,
    workspace: &Path,
    route_ready: bool,
) -> Option<String> {
    let script_only = job.gate.no_agent;
    let rows = crate::connectors::list(workspace);
    let mut ready = Vec::new();
    let mut blocked = Vec::new();
    let mut configured = Vec::new();
    for row in &rows {
        configured.push(row.name.clone());
        if !row.enabled || row.needs_sign_in || row.status == "error" || row.status == "disabled" {
            let detail = if row.needs_sign_in {
                "needs sign-in".into()
            } else if !row.error.is_empty() {
                row.error.clone()
            } else if !row.enabled || row.status == "disabled" {
                "disabled".into()
            } else {
                row.status.clone()
            };
            blocked.push((row.name.clone(), detail));
        } else {
            ready.push(row.name.clone());
        }
    }
    let home = grokhub_core::user_home()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let route_error = if route_ready {
        ""
    } else {
        grokhub_core::XAI_NEED_SIGNIN
    };
    let ctx = grokhub_core::PreflightCtx {
        route_ready,
        route_error,
        ready_connectors: &ready,
        blocked_connectors: &blocked,
        configured_connectors: &configured,
        path_env: None,
        home: &home,
    };
    grokhub_core::preflight(job, script_only, &ctx).map(|b| b.line())
}

/// The scheduler's claim: takes the first due job the shell may fire and
/// saves it as run (last run, count and the next run after `clock`) before it
/// starts, so a second tick, or the cabin opening, finds it done. However many
/// slots were missed (the laptop slept), the job runs once and its next run is
/// the next slot to come. Jobs in `skip` and jobs a Run now holds are left
/// alone. A job with no next run gets one.
pub fn claim_due(
    config_dir: &Path,
    clock: LocalClock,
    skip: &HashSet<String>,
) -> Result<Option<Automation>, String> {
    let _store = store();
    let mut list = load_unlocked(config_dir)?;
    let before = list.clone();
    let pick = grokhub_core::due_automations(&list, clock.now_ms)
        .into_iter()
        .find(|a| !skip.contains(&a.id) && !Running::holds(&a.id))
        .map(|a| a.id);
    let mut claimed = None;
    for a in &mut list {
        if pick.as_deref() == Some(a.id.as_str()) {
            let ran = grokhub_core::mark_automation_ran(a.clone(), clock.now_ms);
            *a = grokhub_core::ensure_automation_schedule(ran, clock);
            claimed = Some(a.clone());
        } else if a.enabled && a.next_run.is_none() {
            *a = grokhub_core::ensure_automation_schedule(a.clone(), clock);
        }
    }
    if list != before {
        save(config_dir, &list)?;
    }
    Ok(claimed)
}

/// Peek at the next due job the shell would claim, without saving a run.
pub fn next_due(
    config_dir: &Path,
    clock: LocalClock,
    skip: &HashSet<String>,
) -> Result<Option<Automation>, String> {
    let _store = store();
    let list = load_unlocked(config_dir)?;
    Ok(grokhub_core::due_automations(&list, clock.now_ms)
        .into_iter()
        .find(|a| !skip.contains(&a.id) && !Running::holds(&a.id)))
}

/// A check command said to wait: advance the next run without counting a
/// successful fire.
pub fn settle_skip(config_dir: &Path, id: &str, clock: LocalClock) -> Result<(), String> {
    change(config_dir, id, |list, at| {
        list[at] = grokhub_core::mark_automation_skipped(list[at].clone(), clock.now_ms, clock);
    })
    .map(drop)
}

/// A condition / event-stream wait is not ready: poll again soon, no run count.
pub fn settle_wait(config_dir: &Path, id: &str, clock: LocalClock) -> Result<(), String> {
    change(config_dir, id, |list, at| {
        list[at] = grokhub_core::mark_automation_waiting(list[at].clone(), clock.now_ms);
    })
    .map(drop)
}

/// Persist a stream cursor after a matching wait fired.
pub fn remember_wait_cursor(config_dir: &Path, id: &str, cursor: &str) -> Result<(), String> {
    if cursor.trim().is_empty() {
        return Ok(());
    }
    change(config_dir, id, |list, at| {
        list[at].wait = list[at].wait.remember_cursor(cursor);
    })
    .map(drop)
}

/// Preflight refused the run: mark health failed and advance the next slot
/// without counting a fire or spending tokens.
pub fn settle_preflight(
    config_dir: &Path,
    id: &str,
    why: &str,
    clock: LocalClock,
) -> Result<(), String> {
    change(config_dir, id, |list, at| {
        list[at] = grokhub_core::mark_automation_preflight_failed(
            list[at].clone(),
            why,
            clock.now_ms,
            clock,
        );
    })
    .map(drop)
}

/// The check output a run used, saved on its row for the next diff.
pub fn remember_check(config_dir: &Path, id: &str, output: &str) -> Result<(), String> {
    change(config_dir, id, |list, at| {
        list[at].gate = list[at].gate.remember(output);
    })
    .map(drop)
}

/// How a scheduled run ended, on its row. The claim already counted it.
pub fn settle_run(config_dir: &Path, id: &str, result: &RunResult) -> Result<(), String> {
    change(config_dir, id, |list, at| {
        list[at] = settle(list[at].clone(), result);
    })
    .map(drop)
}

/// KB-02: Run now and a shell scheduled run take the automation's one board
/// card into Doing, as the cabin's scheduled run does. A board that cannot be read is left alone and the
/// run still goes. No model is involved.
fn board_run_start(
    config_dir: &Path,
    job: &Automation,
    session: &str,
    model: &str,
    gate: RunGate,
    now: u64,
    why: &str,
) -> Option<String> {
    use grokhub_core::{work_resume as res, work_run as wr};
    let id = wr::edit_board_file(&wr::board_path(config_dir), now, |cards| {
        let had = cards.len();
        let id = wr::begin_automation_card(cards, &job.id, &job.name, session, model, now);
        // A new card is named for the job: a browser's run marks its words
        // as the browser's before it is written, or files none.
        if cards.len() > had
            && gate.origin == RunOrigin::Browser
            && grokhub_agent::web_origin::mark_card(config_dir, &id, &[CardPart::Words]).is_err()
        {
            cards.retain(|c| c.id != id);
            return None;
        }
        // KB-03: the shell runs it in `gate`'s mode, which a resume keeps,
        // and a shell restart interrupts it, not the cabin's.
        if let Some(c) = cards.iter_mut().find(|c| c.id == id) {
            res::claim_run(c, res::HOST_SHELL, gate.wire());
        }
        Some(id)
    })
    .ok()??;
    let _ = grokhub_core::work_item::append_work_log(
        &wr::work_dir(config_dir),
        &id,
        now,
        wr::LOG_START,
        why,
    );
    Some(id)
}

/// A run of a saved job, as its card's end files it.
pub(crate) struct BoardRun<'a> {
    /// The job as saved.
    pub job: &'a Automation,
    /// The words the run was asked: the job's, or them with a check's or a
    /// wait's lines added.
    pub asked: &'a str,
    /// Whose words the run went on.
    pub origin: RunOrigin,
}

/// The run's final checkpoint. A reply that needs you files the Follow up card,
/// as in the cabin; a quiet run lets the agent's untouched card archive itself.
/// The report is the run's words: a browser run's marks the card's words and
/// report as the browser's before they are written (or files none), and a
/// run of a person's words takes the browser's mark off the report it
/// replaces.
pub(crate) fn board_run_end(
    config_dir: &Path,
    ran: BoardRun<'_>,
    card: Option<&str>,
    done: &RunResult,
) {
    use grokhub_agent::web_origin::{card_saved_here, card_seen, mark_card};
    use grokhub_core::work_resume as res;
    use grokhub_core::work_run::{self as wr, RunEnd};
    let Some(card) = card else {
        return;
    };
    let job = ran.job;
    let now = grokhub_core::now_ms();
    let failed = done.failed();
    let end = if failed {
        RunEnd::Failed(done.error.clone())
    } else {
        RunEnd::Done
    };
    let report = !failed && grokhub_core::automation_needs_follow_up(ran.asked, &done.text);
    let title = if job.name.trim().is_empty() {
        &job.instructions
    } else {
        &job.name
    };
    // KB-03: a failed run is Interrupted (Blocked with its checkpoint), so it
    // resumes from there.
    let (cut, replaced) = wr::edit_board_file(&wr::board_path(config_dir), now, |cards| {
        let cut = cards
            .iter_mut()
            .rev()
            .find(|c| {
                c.automation.as_deref() == Some(job.id.as_str())
                    && c.status == grokhub_core::BoardStatus::InProgress
            })
            .is_some_and(|c| res::interrupt_failed(c, &end, now));
        let mut replaced = None;
        if !cut {
            wr::finish_automation_card(cards, &job.id, &end, report, now);
            if report {
                let before = cards.clone();
                let (id, _) = grokhub_core::file_follow_up(cards, &job.id, title, &done.text, now);
                match ran.origin {
                    RunOrigin::Browser => {
                        let parts = [CardPart::Words, CardPart::Report];
                        if mark_card(config_dir, &id, &parts).is_err() {
                            *cards = before;
                        }
                    }
                    RunOrigin::Window => {
                        replaced = Some((id.clone(), card_seen(config_dir, &id, CardPart::Report)))
                    }
                }
            }
        }
        (cut, replaced)
    })
    .unwrap_or((false, None));
    if let Some((id, seen)) = replaced {
        // A mark that can't be taken off leaves the report a browser's: the safe side.
        let _ = card_saved_here(config_dir, &id, CardPart::Report, seen);
    }
    let line = if cut {
        format!("interrupted: {}", done.error.trim())
    } else {
        end.log_text()
    };
    let _ = grokhub_core::work_item::append_work_log(
        &wr::work_dir(config_dir),
        card,
        now,
        wr::LOG_END,
        &line,
    );
}

/// KB-03: at shell startup no shell run is live, so every Doing card a shell
/// run left behind is Interrupted, with a log line. Cabin runs are left alone.
pub fn interrupt_board_orphans(config_dir: &Path) -> Vec<String> {
    use grokhub_core::{work_resume as res, work_run as wr};
    let now = grokhub_core::now_ms();
    let cut = wr::edit_board_file(&wr::board_path(config_dir), now, |cards| {
        res::interrupt_orphans(cards, res::HOST_SHELL, &[], now)
    })
    .unwrap_or_default();
    for id in &cut {
        let _ = grokhub_core::work_item::append_work_log(
            &wr::work_dir(config_dir),
            id,
            now,
            wr::LOG_END,
            &format!("interrupted: {}", res::InterruptCause::Restart.text()),
        );
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::{
        ClientError, FunctionCall, InputItem, ResponsesRequest, StreamEvent, TurnOutput,
    };

    const REFUSED: &str =
        "read-only in this phase: `write` is not available. Use read_file, list_dir, grep, or glob.";

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-auto-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// Tuesday 08:00 local.
    fn tuesday_8am() -> LocalClock {
        Clock {
            weekday: 2,
            hour: 8,
            minute: 0,
        }
        .at(1_800_000_000_000)
    }

    #[test]
    fn a_weekday_job_is_saved_with_its_next_run() {
        let dir = temp_dir("add");
        let list = add(
            &dir,
            "every weekday at 9am summarize my inbox",
            tuesday_8am(),
        )
        .expect("added");
        assert_eq!(list.len(), 1);
        let a = &list[0];
        assert!(a.id.starts_with("auto"), "{}", a.id);
        assert_eq!(
            (a.schedule.as_str(), a.time.as_str(), a.name.as_str()),
            ("weekdays", "09:00", "summarize my inbox")
        );
        assert_eq!(a.next_run, Some(1_800_000_000_000 + 60 * 60_000));
        assert_eq!(
            rows(&list, tuesday_8am())[0].summary,
            "weekdays at 09:00 · next today 9:00 AM"
        );
        assert_eq!(load(&dir).expect("reload"), list);
        assert!(!dir.join("automations.json.shell.tmp").exists());
    }

    #[test]
    fn text_without_a_when_is_refused_and_writes_nothing() {
        let dir = temp_dir("nowhen");
        assert_eq!(
            add(&dir, "summarize my inbox", tuesday_8am()),
            Err(ADD_HINT.to_string())
        );
        assert!(!path(&dir).exists());
    }

    #[test]
    fn pause_resume_and_remove_keep_the_other_jobs() {
        let dir = temp_dir("toggle");
        add(&dir, "every day at 7am water the plants", tuesday_8am()).expect("a");
        let list = add(
            &dir,
            "heartbeat every 30 min check the build",
            tuesday_8am(),
        )
        .expect("b");
        let (first, second) = (list[0].id.clone(), list[1].id.clone());

        let paused = set_enabled(&dir, &first, false, tuesday_8am()).expect("pause");
        assert!(!paused[0].enabled);
        assert_eq!(paused[0].next_run, None);
        assert_eq!(
            rows(&paused, tuesday_8am())[0].summary,
            "daily at 07:00 · paused"
        );

        let resumed = set_enabled(&dir, &first, true, tuesday_8am()).expect("resume");
        assert_eq!(
            rows(&resumed, tuesday_8am())[0].summary,
            "daily at 07:00 · next Wed 7:00 AM"
        );

        let left = remove(&dir, &first).expect("remove");
        assert_eq!(
            left.iter().map(|a| a.id.clone()).collect::<Vec<_>>(),
            vec![second]
        );
        assert_eq!(
            remove(&dir, &first),
            Err("That automation is gone.".to_string())
        );
    }

    #[test]
    fn a_broken_file_is_an_error_and_is_left_alone() {
        let dir = temp_dir("broken");
        std::fs::write(path(&dir), "[{not json").expect("seed");
        assert!(load(&dir)
            .expect_err("broken")
            .starts_with("automations.json: "));
        assert!(add(&dir, "every day at 7am x", tuesday_8am()).is_err());
        assert_eq!(
            std::fs::read_to_string(path(&dir)).expect("kept"),
            "[{not json"
        );
    }

    #[test]
    fn rows_written_by_the_cabin_load_with_their_ids() {
        let dir = temp_dir("cabin");
        std::fs::write(
            path(&dir),
            r#"[{"id":"auto-9","name":"board","schedule":"weekdays","time":"09:00","instructions":"summarize the workboard","enabled":true,"runCount":2,"health":{"outcome":"failed","error":"API key rejected","failStreak":2}}]"#,
        )
        .expect("seed");
        let list = load(&dir).expect("load");
        let row = &rows(&list, tuesday_8am())[0];
        assert_eq!(row.id, "auto-9");
        assert_eq!(row.summary, "weekdays at 09:00 · 2 runs");
        assert_eq!(
            row.health.as_deref(),
            Some("Failed 2 times in a row — API key rejected")
        );
    }

    #[test]
    fn edit_keeps_the_id_runs_pause_and_health_and_takes_the_new_words() {
        let dir = temp_dir("edit");
        let list = add(&dir, "every day at 7am water the plants", tuesday_8am()).expect("a");
        let id = list[0].id.clone();
        change(&dir, &id, |list, at| {
            let ran = grokhub_core::mark_automation_ran(list[at].clone(), 5_000);
            list[at] = grokhub_core::mark_automation_failed(ran, "API key rejected");
        })
        .expect("ran");
        set_enabled(&dir, &id, false, tuesday_8am()).expect("pause");

        let edited = edit(
            &dir,
            &id,
            "every weekday at 9:30am water the ferns",
            tuesday_8am(),
        )
        .expect("edit");
        let a = &edited[0];
        assert_eq!(
            (
                a.id.as_str(),
                a.name.as_str(),
                a.schedule.as_str(),
                a.time.as_str(),
                a.instructions.as_str(),
            ),
            (
                id.as_str(),
                "water the ferns",
                "weekdays",
                "09:30",
                "every weekday at 9:30am water the ferns",
            )
        );
        assert_eq!(
            (a.enabled, a.run_count, a.last_run, a.next_run),
            (false, 1, Some(5_000), None)
        );
        assert_eq!(
            grokhub_core::automation_health_line(a).as_deref(),
            Some("Last run failed — API key rejected")
        );
        assert_eq!(load(&dir).expect("reload"), edited);

        let resumed = set_enabled(&dir, &id, true, tuesday_8am()).expect("resume");
        let row = &rows(&resumed, tuesday_8am())[0];
        assert_eq!(
            row.summary,
            "weekdays at 09:30 · next today 9:30 AM · 1 run"
        );
        assert_eq!(
            row.describe.as_deref(),
            Some("every weekday at 9:30am water the ferns")
        );
    }

    #[test]
    fn edit_without_a_time_is_refused_and_writes_nothing() {
        let dir = temp_dir("edit-nowhen");
        let id = add(&dir, "every day at 7am water the plants", tuesday_8am()).expect("a")[0]
            .id
            .clone();
        let before = std::fs::read_to_string(path(&dir)).expect("before");
        assert_eq!(
            edit(&dir, &id, "water the ferns", tuesday_8am()),
            Err(ADD_HINT.to_string())
        );
        assert_eq!(
            edit(&dir, "auto-gone", "every day at 8am x", tuesday_8am()),
            Err("That automation is gone.".to_string())
        );
        assert_eq!(std::fs::read_to_string(path(&dir)).expect("after"), before);
    }

    #[test]
    fn edit_keeps_a_check_command() {
        let dir = temp_dir("edit-check");
        std::fs::write(
            path(&dir),
            r#"[{"id":"auto-9","name":"tidy","schedule":"daily","time":"09:00","instructions":"tidy downloads","checkCommand":"test -d ~/Downloads","enabled":true}]"#,
        )
        .expect("seed");
        let row = &rows(&load(&dir).expect("load"), tuesday_8am())[0];
        assert_eq!(
            row.describe.as_deref(),
            Some("every day at 9am tidy downloads")
        );
        let edited = edit(
            &dir,
            "auto-9",
            "every day at 6pm tidy downloads",
            tuesday_8am(),
        )
        .expect("edit");
        assert_eq!(
            (edited[0].check_command.as_str(), edited[0].time.as_str()),
            ("test -d ~/Downloads", "18:00")
        );
    }

    #[test]
    fn rows_show_the_check_command_and_gate() {
        let dir = temp_dir("row-check");
        let list = add(
            &dir,
            "every day at 9am check `df -h /` and tell me only when it changes",
            tuesday_8am(),
        )
        .expect("add");
        let row = &rows(&list, tuesday_8am())[0];
        assert_eq!(row.check_command, "df -h /");
        assert!(row.gate.on_change);
        assert!(!row.gate.no_agent);
        let plain =
            add(&dir, "every day at 10am summarize my inbox", tuesday_8am()).expect("plain");
        let plain_row = &rows(&plain, tuesday_8am())[1];
        assert_eq!(plain_row.check_command, "");
        assert_eq!(plain_row.gate, RowGate::default());
    }

    #[test]
    fn words_with_a_backticked_check_set_the_command_and_its_gate() {
        let dir = temp_dir("add-check");
        let list = add(
            &dir,
            "every day at 9am check `df -h /` and tell me only when it changes",
            tuesday_8am(),
        )
        .expect("add");
        assert_eq!(list[0].check_command, "df -h /");
        assert!(list[0].gate.on_change);
        assert!(!list[0].gate.no_agent);
        let id = list[0].id.clone();
        remember_check(&dir, &id, "/dev/sda1 80%").expect("remember");
        // The same command, now without AI: what it printed last stays.
        let edited = edit(
            &dir,
            &id,
            "every day at 9am check `df -h /` without AI",
            tuesday_8am(),
        )
        .expect("edit");
        assert_eq!(edited[0].check_command, "df -h /");
        assert!(edited[0].gate.no_agent);
        assert!(!edited[0].gate.on_change);
        assert_eq!(edited[0].gate.last_output, "/dev/sda1 80%");
        // A new command starts with nothing to diff against.
        let moved = edit(&dir, &id, "every day at 9am run `uptime`", tuesday_8am()).expect("edit");
        assert_eq!(moved[0].check_command, "uptime");
        assert_eq!(
            moved[0].gate,
            grokhub_core::automation_check::CheckGate::default()
        );
    }

    /// Asks to write a file, then answers with text.
    struct WantsToWrite {
        requests: Mutex<Vec<ResponsesRequest>>,
    }

    impl ModelClient for WantsToWrite {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            let mut requests = self.requests.lock().expect("requests");
            requests.push(req.clone());
            if requests.len() == 1 {
                return Ok(TurnOutput {
                    calls: vec![FunctionCall {
                        call_id: "w".into(),
                        name: "write".into(),
                        arguments: r#"{"path":"out.txt","content":"x"}"#.into(),
                    }],
                    ..TurnOutput::default()
                });
            }
            sink(StreamEvent::TextDelta("Could not write.".into()));
            Ok(TurnOutput {
                text: "Could not write.".into(),
                ..TurnOutput::default()
            })
        }
    }

    #[test]
    fn a_row_without_an_id_keeps_the_one_it_is_given() {
        let dir = temp_dir("noid");
        std::fs::write(
            path(&dir),
            r#"[{"name":"board","schedule":"daily","time":"07:00","instructions":"summarize the workboard","enabled":true}]"#,
        )
        .expect("seed");
        let first = load(&dir).expect("first")[0].id.clone();
        assert!(first.starts_with("auto"), "{first}");
        assert_eq!(load(&dir).expect("second")[0].id, first);
        let paused = set_enabled(&dir, &first, false, tuesday_8am()).expect("pause");
        assert_eq!(
            (paused[0].id.as_str(), paused[0].enabled),
            (first.as_str(), false)
        );
    }

    #[test]
    fn a_job_already_running_is_not_started_twice() {
        let held = Running::claim("auto-held").expect("first claim");
        assert_eq!(
            Running::claim("auto-held").err().as_deref(),
            Some("That automation is already running.")
        );
        assert!(Running::claim("auto-other").is_ok());
        drop(held);
        assert!(Running::claim("auto-held").is_ok());
    }

    fn run_wants_to_write(
        name: &str,
        rules: Option<&str>,
    ) -> (PathBuf, PathBuf, String, Arc<WantsToWrite>, RunResult) {
        let (dir, workspace) = (
            temp_dir(&format!("{name}-cfg")),
            temp_dir(&format!("{name}-ws")),
        );
        if let Some(rules) = rules {
            std::fs::write(dir.join("permission-rules.json"), rules).expect("rules");
        }
        let list = add(&dir, "every day at 7am write out.txt", tuesday_8am()).expect("a");
        let id = list[0].id.clone();
        let client = Arc::new(WantsToWrite {
            requests: Mutex::new(Vec::new()),
        });
        let result = run_now(
            &dir,
            &id,
            client.clone(),
            "grok-4.7",
            workspace.clone(),
            RunGate::BACKGROUND,
        )
        .expect("ran");
        (dir, workspace, id, client, result)
    }

    fn refusal(client: &WantsToWrite) -> Option<String> {
        let requests = client.requests.lock().expect("requests");
        requests[1].input.iter().find_map(|item| match item {
            InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
            _ => None,
        })
    }

    #[test]
    fn an_allow_rule_does_not_let_run_now_write() {
        let (_dir, workspace, _id, client, result) =
            run_wants_to_write("allow", Some(r#"{"allow":["Edit(**)","Write(**)"]}"#));
        assert!(!workspace.join("out.txt").exists());
        assert_eq!(result.stop, "end_turn");
        assert_eq!(refusal(&client).as_deref(), Some(REFUSED));
    }

    #[test]
    fn run_now_refuses_writes_and_records_the_run() {
        let (dir, workspace, _id, client, result) = run_wants_to_write("run", None);

        assert!(!workspace.join("out.txt").exists());
        assert_eq!(result.stop, "end_turn");
        assert_eq!(result.error, "");
        assert!(result.text.contains("Could not write."), "{}", result.text);
        assert_eq!(refusal(&client).as_deref(), Some(REFUSED));
        let row = &load(&dir).expect("reload")[0];
        assert_eq!(row.run_count, 1);
        assert_eq!(row.health.outcome, grokhub_core::AutoOutcome::Ok);
        assert!(row.last_run.is_some());
        // KB-02: the run checkpointed one automation card, with start and end lines.
        let board =
            grokhub_core::work_run::read_board_file(&dir.join("workboard.json")).expect("board");
        let card = board
            .iter()
            .find(|c| c.automation.as_deref() == Some(row.id.as_str()))
            .expect("automation card");
        assert_eq!(card.kind, grokhub_core::WorkKind::Automation);
        assert_eq!(card.checkpoint.as_ref().map(|c| c.attempt), Some(1));
        let log: Vec<String> = grokhub_core::work_item::read_work_log(&dir.join("work"), &card.id)
            .into_iter()
            .map(|e| e.text)
            .collect();
        assert_eq!(log, ["run now", "done"]);
    }
    struct Stalls;

    impl ModelClient for Stalls {
        fn stream(
            &self,
            _req: &ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            Err(ClientError::IdleTimeout)
        }
    }

    /// KB-03: a failed shell run is Interrupted on its card, and a shell
    /// restart interrupts the shell's orphaned Doing cards but not the cabin's.
    #[test]
    fn a_failed_run_and_a_shell_restart_interrupt_the_shells_cards_only() {
        use grokhub_core::work_resume::{InterruptCause, HOST_CABIN, HOST_SHELL};
        use grokhub_core::work_run as wr;
        use grokhub_core::BoardStatus;
        let (dir, workspace) = (temp_dir("cut-cfg"), temp_dir("cut-ws"));
        let id = add(&dir, "every day at 7am check the build", tuesday_8am()).expect("a")[0]
            .id
            .clone();
        let result = run_now(
            &dir,
            &id,
            Arc::new(Stalls),
            "grok-4.7",
            workspace,
            RunGate::BACKGROUND,
        )
        .expect("ran");
        assert!(result.failed(), "{result:?}");

        let board = wr::read_board_file(&wr::board_path(&dir)).expect("board");
        let card = board
            .iter()
            .find(|c| c.automation.as_deref() == Some(id.as_str()))
            .expect("card");
        assert_eq!(card.status, BoardStatus::Blocked);
        assert_eq!(
            (card.run_state.host.as_str(), card.run_state.mode.as_str()),
            (HOST_SHELL, "ask")
        );
        let cut = card.run_state.interrupted.as_ref().expect("interrupted");
        assert_eq!(cut.cause, InterruptCause::from_failure(&result.error));
        let log: Vec<String> =
            grokhub_core::work_item::read_work_log(&wr::work_dir(&dir), &card.id)
                .into_iter()
                .map(|e| e.text)
                .collect();
        assert_eq!(log[0], "run now");
        assert!(log[1].starts_with("interrupted: "), "{log:?}");

        // The shell quit with one of its runs and one of the cabin's in Doing.
        let mut board = board;
        for (host, title) in [(HOST_SHELL, "shell run"), (HOST_CABIN, "cabin run")] {
            let mut c = grokhub_core::BoardCard::new(title, "", "");
            c.status = BoardStatus::InProgress;
            wr::begin_run(
                &mut c,
                grokhub_core::WorkKind::Automation,
                "s",
                "grok-4.7",
                1,
            );
            c.run_state.host = host.to_string();
            board.push(c);
        }
        let (shell_id, cabin_id) = (board[1].id.clone(), board[2].id.clone());
        wr::edit_board_file(&wr::board_path(&dir), 2, |cards| *cards = board).expect("seed");

        assert_eq!(interrupt_board_orphans(&dir), vec![shell_id.clone()]);
        let board = wr::read_board_file(&wr::board_path(&dir)).expect("board");
        let status = |id: &str| board.iter().find(|c| c.id == id).map(|c| c.status);
        assert_eq!(status(&shell_id), Some(BoardStatus::Blocked));
        assert_eq!(status(&cabin_id), Some(BoardStatus::InProgress));
        let shell = board.iter().find(|c| c.id == shell_id).expect("shell card");
        assert_eq!(
            shell.run_state.interrupted.as_ref().map(|i| i.cause),
            Some(InterruptCause::Restart)
        );
        assert!(interrupt_board_orphans(&dir).is_empty());
    }

    /// Asks for one screenshot, then is done.
    #[derive(Default)]
    struct Screenshots {
        requests: Mutex<Vec<ResponsesRequest>>,
    }

    impl ModelClient for Screenshots {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            let mut requests = self.requests.lock().expect("requests");
            requests.push(req.clone());
            if requests.len() == 1 {
                return Ok(TurnOutput {
                    calls: vec![FunctionCall {
                        call_id: "shot".into(),
                        name: "screenshot".into(),
                        arguments: "{}".into(),
                    }],
                    ..TurnOutput::default()
                });
            }
            Ok(TurnOutput {
                text: "done".into(),
                ..TurnOutput::default()
            })
        }
    }

    /// A job's words from a paired browser: added, or a job made here edited
    /// there. Its runs get no desktop, however they start, until its words
    /// are saved again here.
    #[test]
    fn a_job_a_browser_wrote_runs_without_the_desktop() {
        let dir = temp_dir("web-job");
        let on = RunGate::scheduled(grokhub_agent::PermMode::Always, true, None);
        // A browser's run: no desktop, whatever the switch says.
        let browser = RunGate {
            origin: RunOrigin::Browser,
            ..on
        };
        assert!(!browser.gate().desktop);
        let web = add_by(
            &dir,
            "every day at 9am take a screenshot",
            tuesday_8am(),
            RunOrigin::Browser,
        )
        .expect("web job")[0]
            .id
            .clone();
        let here = add(&dir, "every day at 10am take a screenshot", tuesday_8am()).expect("job")[1]
            .id
            .clone();
        let job_of = |id: &str| {
            load(&dir)
                .expect("jobs")
                .into_iter()
                .find(|job| job.id == id)
                .expect("job")
        };
        assert_eq!(gate_for(&dir, &job_of(&web), on), browser);
        assert_eq!(gate_for(&dir, &job_of(&here), on), on);

        // The scheduler's run of the browser's job, with desktop control on.
        let job = load(&dir).expect("jobs")[0].clone();
        let client = Arc::new(Screenshots::default());
        let ws = temp_dir("web-job-ws");
        execute(&dir, &job, client.clone(), "grok-4.7", ws, on);
        let requests = client.requests.lock().expect("requests");
        let shot = requests[1].input.iter().find_map(|item| match item {
            InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
            _ => None,
        });
        assert_eq!(shot.as_deref(), Some(*grokhub_core::desktop_mcp::OFF_MSG));
        drop(requests);

        // A browser's edit of the job made here takes the desktop away, and
        // saving its words here gives it back.
        edit_by(
            &dir,
            &here,
            "every day at 10am click Send",
            tuesday_8am(),
            RunOrigin::Browser,
        )
        .expect("web edit");
        let web_words = job_of(&here);
        assert_eq!(web_words.instructions, "every day at 10am click Send");
        assert_eq!(gate_for(&dir, &web_words, on), browser);
        edit(
            &dir,
            &here,
            "every day at 10am take a screenshot",
            tuesday_8am(),
        )
        .expect("edit");
        assert_eq!(gate_for(&dir, &job_of(&here), on), on);
        // A run that read the browser's words before that save still runs
        // them as the browser's.
        assert_eq!(gate_for(&dir, &web_words, on), browser);
        let marks: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join(grokhub_agent::web_origin::JOBS_FILE))
                .expect("web file"),
        )
        .expect("marks");
        assert_eq!(
            marks,
            serde_json::json!({
                &web: null,
                // The words saved here: "every day at 10am take a screenshot".
                &here: "fea130e2d1ccd61ccb0e72c6f12a2b33e66b396084b4df14bc0d8e093aa52558",
            })
        );
        // A file that can't be read leaves every job without the desktop.
        std::fs::write(dir.join(grokhub_agent::web_origin::JOBS_FILE), "[not json")
            .expect("broken");
        assert_eq!(gate_for(&dir, &job_of(&here), on), browser);
    }

    /// A person saves a job's words here while a browser edits the same job.
    /// Whichever save lands last, a browser's words never run with the
    /// desktop: the browser marks the job first, the person's save makes
    /// only the words it saved theirs, and a run goes on the words it runs.
    #[test]
    fn a_save_here_racing_a_browsers_edit_never_gives_its_words_the_desktop() {
        let dir = temp_dir("web-race");
        let on = RunGate::scheduled(grokhub_agent::PermMode::Always, true, None);
        let id = add(&dir, "every day at 9am summarize my inbox", tuesday_8am()).expect("job")[0]
            .id
            .clone();
        let gate = || gate_for(&dir, &load(&dir).expect("jobs")[0], on);
        // One at a time, in the order that lost the mark before: the
        // browser's mark, the person's whole save, then the browser's save.
        grokhub_agent::web_origin::mark_job(&dir, &id).expect("mark");
        edit(
            &dir,
            &id,
            "every day at 9am summarize my inbox",
            tuesday_8am(),
        )
        .expect("edit");
        change(&dir, &id, |list, at| {
            list[at].instructions = "click Send in Mail".into();
        })
        .expect("browser's save");
        assert_eq!(gate().origin, RunOrigin::Browser);
        assert!(!gate().gate().desktop);
        // Both at once, many times over.
        for n in 0..200 {
            edit(
                &dir,
                &id,
                "every day at 9am summarize my inbox",
                tuesday_8am(),
            )
            .expect("reset");
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let window = {
                let (dir, id, barrier) = (dir.clone(), id.clone(), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    barrier.wait();
                    let words = format!("every day at 9am summarize my inbox {n}");
                    edit(&dir, &id, &words, tuesday_8am()).map(|_| ())
                })
            };
            let browser = {
                let (dir, id, barrier) = (dir.clone(), id.clone(), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    barrier.wait();
                    let words = format!("every day at 9am click Send in Mail {n}");
                    edit_by(&dir, &id, &words, tuesday_8am(), RunOrigin::Browser).map(|_| ())
                })
            };
            assert_eq!(window.join().expect("window"), Ok(()));
            assert_eq!(browser.join().expect("browser"), Ok(()));
            // The words that landed last are the run's; the browser's run
            // without the desktop, the person's with it.
            let words = load(&dir).expect("jobs")[0].instructions.clone();
            let want = match words {
                w if w == format!("every day at 9am click Send in Mail {n}") => {
                    (RunOrigin::Browser, false)
                }
                w if w == format!("every day at 9am summarize my inbox {n}") => {
                    (RunOrigin::Window, true)
                }
                w => panic!("round {n}: neither save landed: {w}"),
            };
            assert_eq!((gate().origin, gate().gate().desktop), want, "round {n}");
        }
    }

    /// A check command a browser's words set is the browser's until a
    /// person's words say it: their save that keeps it unsaid leaves the
    /// job without the desktop, and one that says it gives it back. A check
    /// made here stays the person's under a browser's rewrite of the words,
    /// so their save of the words alone gives that job back, and a job a
    /// browser never wrote keeps the desktop through any save here.
    #[test]
    fn a_check_command_a_browsers_words_set_is_its_until_a_persons_words_say_it() {
        let dir = temp_dir("unsaid-check");
        let on = RunGate::scheduled(grokhub_agent::PermMode::Always, true, None);
        let checked = "every day at 9am check `df -h` and tell me if a disk is nearly full";
        let unsaid = "every day at 10am tell me if a disk is nearly full";
        let web = add_by(&dir, checked, tuesday_8am(), RunOrigin::Browser).expect("web")[0]
            .id
            .clone();
        let here = add(&dir, checked, tuesday_8am()).expect("here")[1]
            .id
            .clone();
        let rewritten = add(&dir, checked, tuesday_8am()).expect("rewritten")[2]
            .id
            .clone();
        let ids = [web.clone(), here, rewritten.clone()];
        // A browser rewrites the third job's words and leaves its check.
        edit_by(&dir, &rewritten, unsaid, tuesday_8am(), RunOrigin::Browser).expect("web edit");
        let jobs = || {
            let list = load(&dir).expect("jobs");
            ids.clone().map(|id| {
                let job = list.iter().find(|a| a.id == id).expect("job");
                (
                    job.check_command.clone(),
                    gate_for(&dir, job, on).gate().desktop,
                )
            })
        };
        let df = |desktop: bool| ("df -h".to_string(), desktop);
        assert_eq!(jobs(), [df(false), df(true), df(false)]);
        for id in &ids {
            edit(&dir, id, unsaid, tuesday_8am()).expect("check unsaid");
        }
        assert_eq!(jobs(), [df(false), df(true), df(true)]);
        edit(&dir, &web, checked, tuesday_8am()).expect("check said");
        assert_eq!(jobs(), [df(true), df(true), df(true)]);
        edit(&dir, &web, unsaid, tuesday_8am()).expect("said check unsaid");
        assert_eq!(jobs(), [df(true), df(true), df(true)]);
        // A browser's words set another check on the job the person saved.
        let curl = "every day at 9am check `curl example.com` and tell me what it says";
        edit_by(&dir, &web, curl, tuesday_8am(), RunOrigin::Browser).expect("web check");
        edit(&dir, &web, unsaid, tuesday_8am()).expect("curl unsaid");
        let curled = ("curl example.com".to_string(), false);
        assert_eq!(jobs(), [curled, df(true), df(true)]);
        edit(&dir, &web, curl, tuesday_8am()).expect("curl said");
        let curled = ("curl example.com".to_string(), true);
        assert_eq!(jobs(), [curled, df(true), df(true)]);
    }

    /// Answers with one line of text.
    struct Says(&'static str);

    impl ModelClient for Says {
        fn stream(
            &self,
            _req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            sink(StreamEvent::TextDelta(self.0.into()));
            Ok(TurnOutput {
                text: self.0.into(),
                ..TurnOutput::default()
            })
        }
    }

    /// The Follow up card a browser job's run files holds the browser's
    /// words: its title (the job's name) and its report (the run's reply)
    /// are marked before they are written. The person's save of the job's
    /// words leaves them the browser's. The next report a run of the
    /// person's words files takes the report's mark off, and saving the
    /// card's words here takes off the rest. A job made here files a card
    /// with no mark.
    #[test]
    fn a_follow_up_a_browser_jobs_run_files_holds_the_browsers_words() {
        use grokhub_agent::web_origin::{card_origin, CARDS_FILE};
        use grokhub_core::{work_run as wr, BoardStatus};
        let dir = temp_dir("follow-web");
        let on = RunGate::scheduled(grokhub_agent::PermMode::Always, true, None);
        let words = "every day at 9am summarize my inbox";
        let web = add_by(&dir, words, tuesday_8am(), RunOrigin::Browser).expect("web job")[0]
            .id
            .clone();
        let here = add(&dir, "every day at 10am summarize the board", tuesday_8am()).expect("job")
            [1]
        .id
        .clone();
        let run = |id: &str, reply: &'static str| {
            let job = load(&dir).expect("jobs").into_iter().find(|a| a.id == id);
            let ws = temp_dir("follow-ws");
            execute(&dir, &job.expect("job"), Arc::new(Says(reply)), "m", ws, on);
            let board = wr::read_board_file(&wr::board_path(&dir)).expect("board");
            board
                .into_iter()
                .find(|c| c.automation.as_deref() == Some(id))
                .map(|c| (c.id, c.status, c.title, c.report))
                .expect("card")
        };
        let marks = || std::fs::read_to_string(dir.join(CARDS_FILE)).expect("marks");

        let (card, status, title, report) = run(&web, "Inbox: 3 new. Should I send the replies?");
        assert_eq!(
            (status, title.as_str(), report.as_str()),
            (
                BoardStatus::FollowUp,
                "summarize my inbox",
                "Inbox: 3 new. Should I send the replies?"
            )
        );
        let marked = |words: u64, report: u64| {
            format!(r#"{{"{card}":{{"report":{report},"words":{words}}}}}"#)
        };
        assert_eq!(marks(), marked(3, 1));
        assert_eq!(card_origin(&dir, &card), RunOrigin::Browser);

        // The person saves the job's words here: the job is theirs, the
        // card's words and report are still the browser's.
        edit(&dir, &web, words, tuesday_8am()).expect("saved here");
        let job = load(&dir).expect("jobs")[0].clone();
        assert_eq!(gate_for(&dir, &job, on), on);
        assert_eq!(card_origin(&dir, &card), RunOrigin::Browser);

        // A run of the person's words files the next report on the card.
        let (again, _, _, report) = run(&web, "Inbox: 1 new. Should I reply?");
        assert_eq!(
            (again.as_str(), report.as_str()),
            (card.as_str(), "Inbox: 1 new. Should I reply?")
        );
        assert_eq!(marks(), marked(3, 2));
        assert_eq!(card_origin(&dir, &card), RunOrigin::Browser);

        // The person saves the card's words here.
        let save = crate::workboard::Act::Edit {
            title: title.clone(),
            detail: "Inbox: 1 new. Should I reply?".into(),
        };
        crate::workboard::act(&dir, &card, save).expect("card saved here");
        assert_eq!(marks(), marked(4, 2));
        assert_eq!(card_origin(&dir, &card), RunOrigin::Window);

        // A job made here: its card has no mark.
        let (mine, status, _, _) = run(&here, "Board: 2 cards wait. Should I sort them?");
        assert_eq!(status, BoardStatus::FollowUp);
        assert_eq!(marks(), marked(4, 2));
        assert_eq!(card_origin(&dir, &mine), RunOrigin::Window);
    }
}

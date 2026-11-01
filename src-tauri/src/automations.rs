//! Automations (cross-platform recode, Phase 3b). The shell edits the same
//! `automations.json` the egui cabin uses, so a job made in either shows in
//! both. While the cabin is open its pulse fires them; once it is closed the
//! shell's scheduler (`scheduler.rs`) does. "Run now" and scheduled runs go
//! unattended in Ask mode, so a job can read but every tool that changes
//! something is refused without a card.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use grokhub_agent::{AuthKind, CancelToken, ModelClient, PermMode, UnattendedRun};
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
    /// It has a check command, so only the cabin fires it.
    pub cabin_only: bool,
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
            cabin_only: cabin_only(a),
        })
        .collect()
}

/// A job gated by a check command runs only in the cabin, which runs the
/// command first. The shell neither fires it nor counts it as due.
pub fn cabin_only(a: &Automation) -> bool {
    grokhub_core::night_check_command(&a.check_command).is_some()
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

pub const ADD_HINT: &str =
    "Say when, like \"every weekday at 9am summarize my inbox\" or \"heartbeat every 30 min check the build\".";

pub fn add(config_dir: &Path, text: &str, clock: LocalClock) -> Result<Vec<Automation>, String> {
    let mut a = grokhub_core::parse_nl_automation(text).ok_or_else(|| ADD_HINT.to_string())?;
    if a.name.trim().is_empty() {
        return Err(ADD_HINT.into());
    }
    let _store = store();
    let mut list = load_unlocked(config_dir)?;
    a.id = grokhub_core::uid("auto");
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
    let at = list.iter().position(|a| a.id == id).ok_or_else(|| {
        "That automation is gone. It may have been removed in the cabin.".to_string()
    })?;
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
    change(config_dir, id, |list, at| {
        let mut a = list[at].clone();
        a.enabled = enabled;
        list[at] = grokhub_core::ensure_automation_schedule(a, clock);
    })
}

/// Edit in place: the new words set the name, the ask and the schedule. The
/// id, pause state, run count, last run, health and any cabin check command
/// stay. Words with no time are refused and nothing is written.
pub fn edit(
    config_dir: &Path,
    id: &str,
    text: &str,
    clock: LocalClock,
) -> Result<Vec<Automation>, String> {
    let parsed = grokhub_core::parse_nl_automation(text).ok_or_else(|| ADD_HINT.to_string())?;
    if parsed.name.trim().is_empty() {
        return Err(ADD_HINT.into());
    }
    change(config_dir, id, |list, at| {
        let old = &list[at];
        let next = Automation {
            id: old.id.clone(),
            enabled: old.enabled,
            check_command: old.check_command.clone(),
            last_run: old.last_run,
            next_run: None,
            run_count: old.run_count,
            health: old.health.clone(),
            ..parsed
        };
        list[at] = grokhub_core::ensure_automation_schedule(next, clock);
    })
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

impl Drop for Running {
    fn drop(&mut self) {
        let mut running = RUNNING.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(set) = running.as_mut() {
            set.remove(&self.0);
        }
    }
}

/// One unattended run of `job` in Ask mode, as the shell's scheduler fires it.
/// It records nothing on the row. The session is read-only, as the cabin's
/// scheduled Ask jobs are, so an allow rule cannot turn a refused write into
/// one that runs. Like the cabin's scheduled run, it takes the automation's
/// board card into Doing and logs the start and end on it.
pub fn execute(
    config_dir: &Path,
    job: &Automation,
    client: Arc<dyn ModelClient + Send + Sync>,
    model: &str,
    workspace: PathBuf,
) -> RunResult {
    run_on_board(config_dir, job, client, model, workspace, "scheduled")
}

/// `execute`, with `why` as the note on the card's start log line.
fn run_on_board(
    config_dir: &Path,
    job: &Automation,
    client: Arc<dyn ModelClient + Send + Sync>,
    model: &str,
    workspace: PathBuf,
    why: &str,
) -> RunResult {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let started = grokhub_core::now_ms();
    let session_id = format!("shell-auto-{}-{started}", job.id);
    let card = board_run_start(config_dir, job, &session_id, model, started, why);
    let system = crate::agent::system_prompt(&workspace, false, "");
    let done = grokhub_agent::run_unattended(UnattendedRun {
        client,
        workspace,
        model: model.to_string(),
        effort: None,
        system,
        session_id,
        auth_kind: AuthKind::ApiKey,
        prompt: job.instructions.clone(),
        image: None,
        mode: PermMode::Ask,
        readonly_session: true,
        desktop: false,
        cancel: CancelToken::new(),
        halt: Box::new(crate::agent::NeverHalted),
        desktop_ops: None,
        imagine_bearer: String::new(),
        resume: false,
    });
    let result = RunResult {
        text: done.text,
        error: done.error,
        stop: done.stop_reason,
    };
    board_run_end(config_dir, job, card.as_deref(), &result);
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
) -> Result<RunResult, String> {
    let job = load(config_dir)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| {
            "That automation is gone. It may have been removed in the cabin.".to_string()
        })?;
    let _running = Running::claim(id)?;
    let started = grokhub_core::now_ms();
    let result = run_on_board(config_dir, &job, client, model, workspace, "run now");
    change(config_dir, id, |list, at| {
        let ran = grokhub_core::mark_automation_ran(list[at].clone(), started);
        list[at] = settle(ran, &result);
    })?;
    Ok(result)
}

/// The scheduler's claim: takes the first due job the shell may fire and
/// saves it as run (last run, count and the next run after `clock`) before it
/// starts, so a second tick, or the cabin opening, finds it done. However many
/// slots were missed (the laptop slept), the job runs once and its next run is
/// the next slot to come. Jobs only the cabin fires, jobs in `skip` and jobs
/// a Run now holds are left alone. A job with no next run gets one.
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
        .find(|a| !cabin_only(a) && !skip.contains(&a.id) && !Running::holds(&a.id))
        .map(|a| a.id);
    let mut claimed = None;
    for a in &mut list {
        if pick.as_deref() == Some(a.id.as_str()) {
            let ran = grokhub_core::mark_automation_ran(a.clone(), clock.now_ms);
            *a = grokhub_core::ensure_automation_schedule(ran, clock);
            claimed = Some(a.clone());
        } else if a.enabled && a.next_run.is_none() && !cabin_only(a) {
            *a = grokhub_core::ensure_automation_schedule(a.clone(), clock);
        }
    }
    if list != before {
        save(config_dir, &list)?;
    }
    Ok(claimed)
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
    now: u64,
    why: &str,
) -> Option<String> {
    use grokhub_core::work_run as wr;
    let id = wr::edit_board_file(&wr::board_path(config_dir), now, |cards| {
        wr::begin_automation_card(cards, &job.id, &job.name, session, model, now)
    })
    .ok()?;
    let _ = grokhub_core::work_item::append_work_log(
        &wr::work_dir(config_dir),
        &id,
        now,
        wr::LOG_START,
        why,
    );
    Some(id)
}

/// The run's final checkpoint. A reply that needs you files the Follow up card,
/// as in the cabin; a quiet run lets the agent's untouched card archive itself.
fn board_run_end(config_dir: &Path, job: &Automation, card: Option<&str>, done: &RunResult) {
    use grokhub_core::work_run::{self as wr, RunEnd};
    let Some(card) = card else {
        return;
    };
    let now = grokhub_core::now_ms();
    let failed = done.failed();
    let end = if failed {
        RunEnd::Failed(done.error.clone())
    } else {
        RunEnd::Done
    };
    let report = !failed && grokhub_core::automation_needs_follow_up(&job.instructions, &done.text);
    let title = if job.name.trim().is_empty() {
        &job.instructions
    } else {
        &job.name
    };
    let _ = wr::edit_board_file(&wr::board_path(config_dir), now, |cards| {
        wr::finish_automation_card(cards, &job.id, &end, report, now);
        if report {
            grokhub_core::file_follow_up(cards, &job.id, title, &done.text, now);
        }
    });
    let _ = grokhub_core::work_item::append_work_log(
        &wr::work_dir(config_dir),
        card,
        now,
        wr::LOG_END,
        &end.log_text(),
    );
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
            Err("That automation is gone. It may have been removed in the cabin.".to_string())
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
        assert!(!row.cabin_only);
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
            Err("That automation is gone. It may have been removed in the cabin.".to_string())
        );
        assert_eq!(std::fs::read_to_string(path(&dir)).expect("after"), before);
    }

    #[test]
    fn edit_keeps_a_cabin_check_command() {
        let dir = temp_dir("edit-check");
        std::fs::write(
            path(&dir),
            r#"[{"id":"auto-9","name":"tidy","schedule":"daily","time":"09:00","instructions":"tidy downloads","checkCommand":"test -d ~/Downloads","enabled":true}]"#,
        )
        .expect("seed");
        let row = &rows(&load(&dir).expect("load"), tuesday_8am())[0];
        assert!(row.cabin_only);
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
        let result =
            run_now(&dir, &id, client.clone(), "grok-4.7", workspace.clone()).expect("ran");
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
}

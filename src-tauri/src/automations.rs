//! Automations (cross-platform recode, Phase 3b). The shell edits the same
//! `automations.json` the egui cabin uses, so a job made in either shows in
//! both. The cabin's pulse still fires them on schedule; there is no second
//! scheduler. "Run now" runs one job unattended in Ask mode, so it can read
//! but every tool that changes something is refused without a card.

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

/// Jobs a Run now is working on. A second Run now of the same job is refused.
static RUNNING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

struct Running(String);

impl Running {
    fn claim(id: &str) -> Result<Self, String> {
        let mut running = RUNNING.lock().unwrap_or_else(|err| err.into_inner());
        if !running
            .get_or_insert_with(HashSet::new)
            .insert(id.to_string())
        {
            return Err("That automation is already running.".into());
        }
        Ok(Self(id.to_string()))
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

/// Runs one job now, unattended in Ask mode, and records the run on its row.
/// The session is read-only, as the cabin's scheduled Ask jobs are, so an
/// allow rule cannot turn a refused write into one that runs.
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
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let started = grokhub_core::now_ms();
    let system = crate::agent::system_prompt(&workspace);
    let done = grokhub_agent::run_unattended(UnattendedRun {
        client,
        workspace,
        model: model.to_string(),
        effort: None,
        system,
        session_id: format!("shell-auto-{id}-{started}"),
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
    let failed = done.stop_reason == "error" || !done.error.is_empty();
    change(config_dir, id, |list, at| {
        let ran = grokhub_core::mark_automation_ran(list[at].clone(), started);
        list[at] = if failed {
            grokhub_core::mark_automation_failed(ran, &done.error)
        } else {
            grokhub_core::mark_automation_ok(ran)
        };
    })?;
    Ok(RunResult {
        text: done.text,
        error: done.error,
        stop: done.stop_reason,
    })
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
    }
}

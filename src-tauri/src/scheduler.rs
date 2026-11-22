//! The shell's scheduler (cross-platform Phase 3b). It fires the jobs in
//! `automations.json` while the window is open or in the tray, but only when
//! the cabin is not running: the cabin beats `scheduler-lease.json` from its
//! pulse, and the shell defers while that beat is fresh.
//!
//! A due job gated by a check command runs that command first; only a passing
//! check is claimed and fired, and the model gets what it printed
//! ([`grokhub_core::automation_check`]). Under "only when it changes" an
//! unchanged output waits with no model call, and a "without AI" job's run is
//! the output alone. Other due jobs are claimed first (last run,
//! count and next run saved), then run unattended under your Approvals, as
//! Run now does: Ask stays read-only, and anything that would need an answer
//! is skipped.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grokhub_agent::check_decide::Decision;
use grokhub_agent::ModelClient;
use grokhub_core::automation_check::{check_step, CheckStep};
use grokhub_core::scheduler_lease::{
    headless_may_fire, read_lease, shell_may_fire, write_lease, LeaseOwner, SchedulerLease,
};
use grokhub_core::{Automation, LocalClock};
use serde::Serialize;

use crate::automations::{self, Clock, RunResult};

/// The event the UI hears when a scheduled run ends.
pub const RAN_EVENT: &str = "automations://ran";
/// The stop reason of a run that was the check's output alone ("without AI").
pub const SCRIPT_ONLY: &str = "script_only";
/// The stop reason of a run the decision step ended: the change didn't call
/// for action.
pub const DECIDED_NO: &str = "decided_no";
/// The stop reason when preflight blocked the job before a claim or model call.
pub const PREFLIGHT: &str = grokhub_core::PREFLIGHT_STOP;
/// How often the scheduler looks for due jobs.
const TICK: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fired {
    pub id: String,
    pub name: String,
    pub result: RunResult,
}

/// Minutes east of UTC, from the window's clock. The webview knows the local
/// time zone on every platform; until it reports, nothing fires.
static UTC_OFFSET: Mutex<Option<i32>> = Mutex::new(None);

/// The window reports its wall clock at start and every few minutes, so a
/// daylight-saving change reaches the scheduler.
pub fn note_ui_clock(clock: Clock, now_ms: u64) {
    *UTC_OFFSET.lock().unwrap_or_else(|e| e.into_inner()) = Some(offset_of(clock, now_ms));
}

/// Hour and minute alone leave a day open (22:00 local at 08:00 UTC is
/// UTC-10 or UTC+14), so the weekday picks the offset, in 15-minute steps.
fn offset_of(clock: Clock, now_ms: u64) -> i32 {
    let guess = grokhub_core::pulse_writer::utc_offset_min(clock.hour, clock.minute, now_ms);
    [guess, guess - 1440, guess + 1440]
        .into_iter()
        .filter(|o| (-720..=840).contains(o))
        .find(|&o| local_clock(now_ms, o).weekday == clock.weekday % 7)
        .unwrap_or(guess)
}

pub fn utc_offset() -> Option<i32> {
    *UTC_OFFSET.lock().unwrap_or_else(|e| e.into_inner())
}

/// The local wall clock at `now_ms` for a UTC offset. 1 Jan 1970 was a Thursday.
pub fn local_clock(now_ms: u64, offset_min: i32) -> LocalClock {
    LocalClock::at_offset(now_ms, offset_min)
}

/// One look for due jobs. With the lease, each due job is claimed and then
/// handed to `run`, one at a time, at most once per tick. The lease is checked
/// again before each claim, so a cabin that opens mid-tick takes over. Jobs
/// with a check command run that check first via `check`; a failing receipt
/// advances the next run without firing.
pub fn tick(
    config_dir: &Path,
    clock: &dyn Fn() -> LocalClock,
    pid: u32,
    run: &mut dyn FnMut(&Automation) -> RunResult,
) -> Result<Vec<Fired>, String> {
    tick_with_check(config_dir, clock, pid, run, &mut |cmd| {
        grokhub_core::run_night_check_command(cmd)
    })
}

/// `tick`, with a custom check-command runner (for tests). Nothing is
/// judged before a run.
pub fn tick_with_check(
    config_dir: &Path,
    clock: &dyn Fn() -> LocalClock,
    pid: u32,
    run: &mut dyn FnMut(&Automation) -> RunResult,
    check: &mut dyn FnMut(&str) -> String,
) -> Result<Vec<Fired>, String> {
    tick_deciding(config_dir, clock, pid, run, check, &mut |_, _| None)
}

/// `tick_with_check`, with the decision step: for an "only when it changes"
/// job whose output changed, `decide(job, new lines)` is asked first. A no
/// ends the run with the reason and no agent turn; a yes runs the job with
/// only the lines the judge picked. `None` (the judge failed) runs the job
/// with every new line.
pub fn tick_deciding(
    config_dir: &Path,
    clock: &dyn Fn() -> LocalClock,
    pid: u32,
    run: &mut dyn FnMut(&Automation) -> RunResult,
    check: &mut dyn FnMut(&str) -> String,
    decide: &mut dyn FnMut(&Automation, &str) -> Option<Decision>,
) -> Result<Vec<Fired>, String> {
    tick_as(
        LeaseOwner::Shell,
        config_dir,
        clock,
        pid,
        run,
        check,
        decide,
        &mut |_, _| None,
    )
}

/// `tick_deciding` for the lease `owner`: the open app as `Shell`, or the
/// headless run as `Headless`, which yields to an open app.
/// `preflight(job, script_only)` returns a block reason before claim/decide.
pub fn tick_as(
    owner: LeaseOwner,
    config_dir: &Path,
    clock: &dyn Fn() -> LocalClock,
    pid: u32,
    run: &mut dyn FnMut(&Automation) -> RunResult,
    check: &mut dyn FnMut(&str) -> String,
    decide: &mut dyn FnMut(&Automation, &str) -> Option<Decision>,
    preflight: &mut dyn FnMut(&Automation, bool) -> Option<String>,
) -> Result<Vec<Fired>, String> {
    let may_fire = match owner {
        LeaseOwner::Headless => headless_may_fire,
        _ => shell_may_fire,
    };
    let mut fired = Vec::new();
    let mut seen = HashSet::new();
    loop {
        let now = clock();
        if !may_fire(read_lease(config_dir).as_ref(), pid, now.now_ms) {
            return Ok(fired);
        }
        let beat = SchedulerLease {
            owner,
            pid,
            beat_ms: now.now_ms,
        };
        write_lease(config_dir, &beat)?;
        // Check-gated jobs: run the gate before claiming a run. A failed or
        // empty check, or an unchanged one under "only when it changes",
        // waits with no model call.
        let mut step = None;
        if let Some(pending) = automations::next_due(config_dir, now, &seen)? {
            if let Some(cmd) = grokhub_core::night_check_command(&pending.check_command) {
                let next = check_step(&pending.gate, &check(cmd));
                if next.waits() {
                    automations::settle_skip(config_dir, &pending.id, now)?;
                    seen.insert(pending.id);
                    continue;
                }
                step = Some((pending.id.clone(), next));
            }
            let script_only = matches!(
                step.as_ref().map(|(_, s)| s),
                Some(CheckStep::ScriptOnly { .. })
            );
            if let Some(why) = preflight(&pending, script_only) {
                automations::settle_preflight(config_dir, &pending.id, &why, now)?;
                seen.insert(pending.id.clone());
                fired.push(Fired {
                    id: pending.id,
                    name: pending.name,
                    result: RunResult {
                        text: String::new(),
                        error: why,
                        stop: PREFLIGHT.into(),
                    },
                });
                continue;
            }
        }
        let Some(job) = automations::claim_due(config_dir, now, &seen)? else {
            return Ok(fired);
        };
        seen.insert(job.id.clone());
        let step = step.filter(|(id, _)| *id == job.id).map(|(_, s)| s);
        let result = match automations::Running::claim(&job.id) {
            Ok(_held) => match &step {
                Some(CheckStep::ScriptOnly { output }) => RunResult {
                    text: output.clone(),
                    error: String::new(),
                    stop: SCRIPT_ONLY.into(),
                },
                Some(CheckStep::Ask { shown, .. }) => {
                    let decision = if job.gate.on_change {
                        decide(&job, shown)
                    } else {
                        None
                    };
                    match decision {
                        Some(d) if !d.act => RunResult {
                            text: format!("Checked, nothing to do: {}", d.why),
                            error: String::new(),
                            stop: DECIDED_NO.into(),
                        },
                        d => {
                            let picked = d.map(|d| d.items.join("\n")).unwrap_or_default();
                            let shown = if picked.trim().is_empty() {
                                shown.as_str()
                            } else {
                                picked.as_str()
                            };
                            run(&Automation {
                                instructions: grokhub_core::automation_check::check_prompt(
                                    &job.instructions,
                                    &job.check_command,
                                    shown,
                                    job.gate.on_change,
                                ),
                                ..job.clone()
                            })
                        }
                    }
                }
                _ => run(&job),
            },
            // A Run now of it started after the claim; that run counts.
            Err(_) => continue,
        };
        // The output that ran is the next diff's base. A failed model run
        // keeps the old one, so the change is shown again next time.
        if let Some(CheckStep::ScriptOnly { output } | CheckStep::Ask { output, .. }) = &step {
            if !result.failed() {
                let _ = automations::remember_check(config_dir, &job.id, output);
            }
        }
        // The job may have been removed while it ran.
        let _ = automations::settle_run(config_dir, &job.id, &result);
        fired.push(Fired {
            id: job.id,
            name: job.name,
            result,
        });
    }
}

/// What a scheduled run needs: the model client, model, work folder and gate
/// from the current settings, or why there is none (no API key).
pub type Route = (
    Arc<dyn ModelClient + Send + Sync>,
    String,
    PathBuf,
    crate::unattended::RunGate,
);

/// The background thread: a tick every 30 seconds, and once right after a
/// laptop wakes, since the sleep ends then. Missed slots fire once.
pub fn start(
    config_dir: PathBuf,
    route: impl Fn() -> Result<Route, String> + Send + 'static,
    emit: impl Fn(&Fired) + Send + 'static,
) {
    // KB-03: runs this shell left in Doing when it last quit are Interrupted.
    automations::interrupt_board_orphans(&config_dir);
    std::thread::spawn(move || loop {
        std::thread::sleep(TICK);
        let Some(offset) = utc_offset() else {
            continue;
        };
        if let Ok(fired) = tick_live(LeaseOwner::Shell, &config_dir, offset, &route) {
            for f in &fired {
                emit(f);
            }
        }
    });
}

/// `grokhub-shell --automations`: the same ticks with no window, for when
/// the app is closed (a login service runs it). The local clock comes from
/// the OS, since there is no window to report it. An open app takes the
/// lease, and this waits until its beat goes stale. Never returns.
pub fn run_headless(
    config_dir: &Path,
    route: impl Fn() -> Result<Route, String>,
    report: impl Fn(&Fired),
) -> ! {
    loop {
        let offset = chrono::Local::now().offset().local_minus_utc() / 60;
        if let Ok(fired) = tick_live(LeaseOwner::Headless, config_dir, offset, &route) {
            for f in &fired {
                report(f);
            }
        }
        std::thread::sleep(TICK);
    }
}

/// One tick with the live route, the real check runner and the decision step.
fn tick_live(
    owner: LeaseOwner,
    config_dir: &Path,
    offset: i32,
    route: &dyn Fn() -> Result<Route, String>,
) -> Result<Vec<Fired>, String> {
    let clock = || local_clock(grokhub_core::now_ms(), offset);
    let mut run = |job: &Automation| match route() {
        Ok((client, model, workspace, gate)) => {
            automations::execute(config_dir, job, client, &model, workspace, gate)
        }
        Err(error) => RunResult {
            text: String::new(),
            error,
            stop: "error".into(),
        },
    };
    // The decision step uses the same route as the run, with no tools.
    let mut decide = |job: &Automation, shown: &str| {
        let (client, model, _, _) = route().ok()?;
        let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
        grokhub_agent::check_decide::decide(
            client.as_ref(),
            &grokhub_agent::CancelToken::new(),
            &model,
            &format!("shell-auto-{}-decide", job.id),
            &job.instructions,
            &job.check_command,
            shown,
        )
    };
    let mut preflight = |job: &Automation, script_only: bool| {
        live_preflight_block(config_dir, job, script_only, route)
    };
    let mut fired = tick_as(
        owner,
        config_dir,
        &clock,
        std::process::id(),
        &mut run,
        &mut |cmd| grokhub_core::run_night_check_command(cmd),
        &mut decide,
        &mut preflight,
    )?;
    // Routines' timer and file triggers, under the same lease.
    fired.extend(crate::routine_runs::tick(owner, config_dir, clock(), route));
    Ok(fired)
}

/// Build a preflight block from the live route and configured connectors.
fn live_preflight_block(
    config_dir: &Path,
    job: &Automation,
    script_only: bool,
    route: &dyn Fn() -> Result<Route, String>,
) -> Option<String> {
    let (route_ready, route_error, workspace) = match route() {
        Ok((_, _, workspace, _)) => (true, String::new(), workspace),
        Err(e) => (false, e, config_dir.to_path_buf()),
    };
    let rows = crate::connectors::list(&workspace);
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
    let ctx = grokhub_core::PreflightCtx {
        route_ready,
        route_error: &route_error,
        ready_connectors: &ready,
        blocked_connectors: &blocked,
        configured_connectors: &configured,
        path_env: None,
        home: &home,
    };
    grokhub_core::preflight(job, script_only, &ctx).map(|b| b.line())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::{
        CancelToken, ClientError, FunctionCall, InputItem, ResponsesRequest, StreamEvent,
        TurnOutput,
    };
    use std::cell::RefCell;

    /// Friday 15 January 2027, 08:00 UTC.
    const FRI_8AM: u64 = 1_800_000_000_000;
    const HOUR: u64 = 3_600_000;
    const PID: u32 = 4242;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-sched-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn at(ms: u64) -> impl Fn() -> LocalClock {
        move || local_clock(ms, 0)
    }

    fn ok(text: &str) -> RunResult {
        RunResult {
            text: text.into(),
            error: String::new(),
            stop: "end_turn".into(),
        }
    }

    /// A daily 9am job added at Friday 8am: next run Friday 9am.
    fn nine_am_job(name: &str) -> (PathBuf, String) {
        let dir = temp_dir(name);
        let list = automations::add(
            &dir,
            "every day at 9am summarize my inbox",
            local_clock(FRI_8AM, 0),
        )
        .expect("added");
        assert_eq!(list[0].next_run, Some(FRI_8AM + HOUR));
        (dir, list[0].id.clone())
    }

    #[test]
    fn the_clock_follows_the_window_offset() {
        let utc = local_clock(FRI_8AM, 0);
        assert_eq!((utc.weekday, utc.hour, utc.minute), (5, 8, 0));
        let hawaii = local_clock(FRI_8AM, -600);
        assert_eq!((hawaii.weekday, hawaii.hour, hawaii.minute), (4, 22, 0));
        let india = local_clock(FRI_8AM + 59_000, 330);
        assert_eq!((india.weekday, india.hour, india.minute), (5, 13, 30));
        note_ui_clock(
            Clock {
                weekday: 4,
                hour: 22,
                minute: 0,
            },
            FRI_8AM,
        );
        assert_eq!(utc_offset(), Some(-600));
        // The same 22:00 on Friday is a day ahead: UTC+14.
        let kiribati = Clock {
            weekday: 5,
            hour: 22,
            minute: 0,
        };
        assert_eq!(offset_of(kiribati, FRI_8AM), 840);
        let india = Clock {
            weekday: 5,
            hour: 13,
            minute: 30,
        };
        assert_eq!(offset_of(india, FRI_8AM), 330);
    }

    #[test]
    fn a_due_job_is_claimed_before_it_runs_and_fires_once() {
        let (dir, id) = nine_am_job("once");
        let nine = FRI_8AM + HOUR;
        let seen = RefCell::new(Vec::new());
        let mut run = |job: &Automation| {
            let row = automations::load(&dir).expect("load")[0].clone();
            seen.borrow_mut()
                .push((job.id.clone(), row.run_count, row.last_run, row.next_run));
            ok("3 new mails.")
        };
        let fired = tick(&dir, &at(nine), PID, &mut run).expect("tick");
        assert_eq!(
            *seen.borrow(),
            vec![(id.clone(), 1, Some(nine), Some(nine + 24 * HOUR))]
        );
        assert_eq!(
            fired,
            vec![Fired {
                id: id.clone(),
                name: "summarize my inbox".into(),
                result: ok("3 new mails."),
            }]
        );

        // The next tick, 30 s on, finds nothing due.
        let fired = tick(&dir, &at(nine + 30_000), PID, &mut run).expect("second tick");
        assert_eq!(fired, Vec::new());
        assert_eq!(seen.borrow().len(), 1);
        let row = &automations::load(&dir).expect("reload")[0];
        assert_eq!(row.run_count, 1);
        assert_eq!(row.health.outcome, grokhub_core::AutoOutcome::Ok);
        assert_eq!(
            read_lease(&dir),
            Some(SchedulerLease {
                owner: LeaseOwner::Shell,
                pid: PID,
                beat_ms: nine + 30_000,
            })
        );
    }

    #[test]
    fn a_fresh_cabin_lease_holds_every_job() {
        let (dir, _id) = nine_am_job("cabin");
        let nine = FRI_8AM + HOUR;
        let cabin = SchedulerLease {
            owner: LeaseOwner::Cabin,
            pid: 7,
            beat_ms: nine - 60_000,
        };
        write_lease(&dir, &cabin).expect("lease");
        let before = std::fs::read_to_string(automations::path(&dir)).expect("before");
        let mut calls = 0;
        let fired = tick(&dir, &at(nine), PID, &mut |_| {
            calls += 1;
            ok("")
        })
        .expect("tick");
        assert_eq!((fired, calls), (Vec::new(), 0));
        assert_eq!(
            std::fs::read_to_string(automations::path(&dir)).expect("after"),
            before
        );
        assert_eq!(read_lease(&dir), Some(cabin));

        // Another shell process's fresh lease holds them too.
        let other = SchedulerLease {
            owner: LeaseOwner::Shell,
            pid: PID + 1,
            beat_ms: nine,
        };
        write_lease(&dir, &other).expect("other");
        assert_eq!(
            tick(&dir, &at(nine), PID, &mut |_| ok("")).expect("tick"),
            Vec::new()
        );
    }

    #[test]
    fn a_stale_cabin_lease_lets_the_shell_fire() {
        let (dir, id) = nine_am_job("stale");
        let nine = FRI_8AM + HOUR;
        write_lease(
            &dir,
            &SchedulerLease {
                owner: LeaseOwner::Cabin,
                pid: 7,
                beat_ms: nine - 90_000,
            },
        )
        .expect("lease");
        let fired = tick(&dir, &at(nine), PID, &mut |_| ok("done")).expect("tick");
        assert_eq!(
            fired.iter().map(|f| f.id.clone()).collect::<Vec<_>>(),
            vec![id]
        );
        assert_eq!(
            read_lease(&dir).map(|l| (l.owner, l.pid)),
            Some((LeaseOwner::Shell, PID))
        );
    }

    #[test]
    fn missed_slots_after_a_sleep_fire_once() {
        let (dir, _id) = nine_am_job("sleep");
        // Asleep from Friday 8am to Monday 11am: three 9am slots went by.
        let monday_11 = FRI_8AM + (3 * 24 + 3) * HOUR;
        let mut calls = 0;
        tick(&dir, &at(monday_11), PID, &mut |_| {
            calls += 1;
            ok("")
        })
        .expect("tick");
        assert_eq!(calls, 1);
        let row = &automations::load(&dir).expect("reload")[0];
        assert_eq!(row.run_count, 1);
        // Tuesday 9am, the next slot to come.
        assert_eq!(row.next_run, Some(monday_11 + 22 * HOUR));
    }

    #[test]
    fn a_failing_check_command_skips_without_firing() {
        let dir = temp_dir("check-skip");
        let nine = FRI_8AM + HOUR;
        std::fs::write(
            automations::path(&dir),
            format!(
                r#"[{{"id":"auto-gated","name":"tidy","schedule":"daily","time":"09:00","instructions":"tidy downloads","checkCommand":"git status --porcelain","enabled":true,"nextRun":{nine}}}]"#
            ),
        )
        .expect("seed");
        let mut runs = 0;
        let fired = tick_with_check(
            &dir,
            &at(nine),
            PID,
            &mut |_| {
                runs += 1;
                ok("should not run")
            },
            &mut |_cmd| "$ git status --porcelain\nexit 0 · 3ms\n".into(),
        )
        .expect("tick");
        assert_eq!(fired, Vec::new());
        assert_eq!(runs, 0);
        let row = &automations::load(&dir).expect("reload")[0];
        assert_eq!(row.run_count, 0);
        assert!(row.next_run.unwrap() > nine);
    }

    #[test]
    fn a_passing_check_command_lets_the_shell_fire() {
        let dir = temp_dir("check-pass");
        let nine = FRI_8AM + HOUR;
        std::fs::write(
            automations::path(&dir),
            format!(
                r#"[{{"id":"auto-gated","name":"tidy","schedule":"daily","time":"09:00","instructions":"tidy downloads","checkCommand":"git status --porcelain","enabled":true,"nextRun":{nine}}}]"#
            ),
        )
        .expect("seed");
        let mut checks = Vec::new();
        let fired = tick_with_check(&dir, &at(nine), PID, &mut |_| ok("cleaned"), &mut |cmd| {
            checks.push(cmd.to_string());
            "$ git status --porcelain\nexit 0 · 3ms\n M downloads\n".into()
        })
        .expect("tick");
        assert_eq!(checks, vec!["git status --porcelain".to_string()]);
        assert_eq!(
            fired.iter().map(|f| f.id.clone()).collect::<Vec<_>>(),
            vec!["auto-gated".to_string()]
        );
        assert_eq!(fired[0].result.text, "cleaned");
        let row = &automations::load(&dir).expect("reload")[0];
        assert_eq!(row.run_count, 1);
    }

    #[test]
    fn a_failed_run_marks_the_row() {
        let (dir, _id) = nine_am_job("fail");
        let fired = tick(&dir, &at(FRI_8AM + HOUR), PID, &mut |_| RunResult {
            text: String::new(),
            error: "Add an xAI API key in Settings first.".into(),
            stop: "error".into(),
        })
        .expect("tick");
        assert_eq!(fired.len(), 1);
        let row = &automations::load(&dir).expect("reload")[0];
        assert_eq!(
            grokhub_core::automation_health_line(row).as_deref(),
            Some("Last run failed — Add an xAI API key in Settings first.")
        );
        assert_eq!(row.run_count, 1);
    }

    const REFUSED: &str =
        "read-only in this phase: `write` is not available. Use read_file, list_dir, grep, or glob.";

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
    fn a_scheduled_run_cannot_write_even_with_an_allow_rule() {
        let (dir, _id) = nine_am_job("write");
        std::fs::write(
            dir.join("permission-rules.json"),
            r#"{"allow":["Edit(**)","Write(**)"]}"#,
        )
        .expect("rules");
        let workspace = temp_dir("write-ws");
        let client = Arc::new(WantsToWrite {
            requests: Mutex::new(Vec::new()),
        });
        let fired = tick(&dir, &at(FRI_8AM + HOUR), PID, &mut |job| {
            // Approvals on Ask: a scheduled run stays read-only.
            let ask =
                crate::unattended::RunGate::scheduled(grokhub_agent::PermMode::Ask, false, None);
            automations::execute(
                &dir,
                job,
                client.clone(),
                "grok-4.7",
                workspace.clone(),
                ask,
            )
        })
        .expect("tick");
        assert!(!workspace.join("out.txt").exists());
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].result.stop, "end_turn");
        assert!(
            fired[0].result.text.contains("Could not write."),
            "{}",
            fired[0].result.text
        );
        let requests = client.requests.lock().expect("requests");
        let refusal = requests[1].input.iter().find_map(|item| match item {
            InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
            _ => None,
        });
        assert_eq!(refusal.as_deref(), Some(REFUSED));
    }

    const DAY: u64 = 24 * HOUR;

    fn gated_job(name: &str, gate: &str) -> (PathBuf, u64) {
        let dir = temp_dir(name);
        let nine = FRI_8AM + HOUR;
        std::fs::write(
            automations::path(&dir),
            format!(
                r#"[{{"id":"auto-disk","name":"disk","schedule":"daily","time":"09:00","instructions":"Tell me if a disk is nearly full.","checkCommand":"df -h","gate":{gate},"enabled":true,"nextRun":{nine}}}]"#
            ),
        )
        .expect("seed");
        (dir, nine)
    }

    fn receipt(lines: &str) -> String {
        format!("$ df -h\nexit 0 · 4ms\n{lines}\n")
    }

    #[test]
    fn only_when_it_changes_wakes_the_model_with_the_new_lines_and_not_again_for_the_same() {
        let (dir, nine) = gated_job("on-change", r#"{"onChange":true}"#);
        let mut asks = Vec::new();
        let mut run = |job: &Automation| {
            asks.push(job.instructions.clone());
            ok("noted")
        };
        let first = tick_with_check(&dir, &at(nine), PID, &mut run, &mut |_| {
            receipt("/dev/sda1 80%\n/dev/sdb1 12%")
        })
        .expect("tick");
        assert_eq!(first.len(), 1);
        let same = tick_with_check(&dir, &at(nine + DAY), PID, &mut run, &mut |_| {
            receipt("/dev/sda1 80%\n/dev/sdb1 12%")
        })
        .expect("tick");
        assert_eq!(same, Vec::new());
        let moved = tick_with_check(&dir, &at(nine + 2 * DAY), PID, &mut run, &mut |_| {
            receipt("/dev/sda1 91%\n/dev/sdb1 12%")
        })
        .expect("tick");
        assert_eq!(moved.len(), 1);
        assert_eq!(
            asks,
            vec![
                "Tell me if a disk is nearly full.\n\nThe check command `df -h` printed these new lines since the last run:\n```\n/dev/sda1 80%\n/dev/sdb1 12%\n```".to_string(),
                "Tell me if a disk is nearly full.\n\nThe check command `df -h` printed these new lines since the last run:\n```\n/dev/sda1 91%\n```".to_string(),
            ]
        );
        let row = &automations::load(&dir).expect("reload")[0];
        assert_eq!(row.run_count, 2);
        assert_eq!(row.gate.last_output, "/dev/sda1 91%\n/dev/sdb1 12%");
        assert!(row.gate.on_change);
    }

    #[test]
    fn a_failed_model_run_keeps_the_old_output_so_the_change_shows_again() {
        let (dir, nine) = gated_job("on-change-fail", r#"{"onChange":true}"#);
        let mut run = |_: &Automation| RunResult {
            text: String::new(),
            error: "busy".into(),
            stop: "error".into(),
        };
        tick_with_check(&dir, &at(nine), PID, &mut run, &mut |_| {
            receipt("/dev/sda1 91%")
        })
        .expect("tick");
        let row = &automations::load(&dir).expect("reload")[0];
        assert_eq!(row.gate.last_output, "");
        assert_eq!(row.gate.last_hash, "");
    }

    #[test]
    fn a_without_ai_job_records_the_output_and_wakes_no_model() {
        let (dir, nine) = gated_job("no-agent", r#"{"noAgent":true}"#);
        let mut runs = 0;
        let fired = tick_with_check(
            &dir,
            &at(nine),
            PID,
            &mut |_| {
                runs += 1;
                ok("should not run")
            },
            &mut |_| receipt("backed up 3 files"),
        )
        .expect("tick");
        assert_eq!(runs, 0);
        assert_eq!(
            fired,
            vec![Fired {
                id: "auto-disk".into(),
                name: "disk".into(),
                result: RunResult {
                    text: "backed up 3 files".into(),
                    error: String::new(),
                    stop: SCRIPT_ONLY.into(),
                },
            }]
        );
        let row = &automations::load(&dir).expect("reload")[0];
        assert_eq!(row.run_count, 1);
        assert_eq!(row.health.outcome, grokhub_core::AutoOutcome::Ok);
        assert_eq!(row.gate.last_output, "backed up 3 files");
    }

    #[test]
    fn the_decision_step_ends_a_no_with_its_reason_and_runs_a_yes_on_the_picked_lines() {
        let (dir, nine) = gated_job("decide", r#"{"onChange":true}"#);
        let mut asks = Vec::new();
        let mut judged = Vec::new();
        let mut answers = vec![
            Some(Decision {
                act: true,
                why: "sda1 is nearly full.".into(),
                items: vec!["/dev/sda1 91%".into()],
            }),
            Some(Decision {
                act: false,
                why: "Every disk has room.".into(),
                items: Vec::new(),
            }),
        ]
        .into_iter();
        let mut run = |job: &Automation| {
            asks.push(job.instructions.clone());
            ok("warned")
        };
        let mut decide = |job: &Automation, shown: &str| {
            judged.push((job.id.clone(), shown.to_string()));
            answers.next().flatten()
        };
        let yes = tick_deciding(
            &dir,
            &at(nine),
            PID,
            &mut run,
            &mut |_| receipt("/dev/sda1 91%\n/dev/sdb1 12%"),
            &mut decide,
        )
        .expect("tick");
        assert_eq!(yes[0].result.text, "warned");
        let no = tick_deciding(
            &dir,
            &at(nine + DAY),
            PID,
            &mut run,
            &mut |_| receipt("/dev/sda1 91%\n/dev/sdb1 13%"),
            &mut decide,
        )
        .expect("tick");
        assert_eq!(
            no[0].result,
            RunResult {
                text: "Checked, nothing to do: Every disk has room.".into(),
                error: String::new(),
                stop: DECIDED_NO.into(),
            }
        );
        assert_eq!(
            judged,
            vec![
                (
                    "auto-disk".to_string(),
                    "/dev/sda1 91%\n/dev/sdb1 12%".to_string()
                ),
                ("auto-disk".to_string(), "/dev/sdb1 13%".to_string()),
            ]
        );
        assert_eq!(
            asks,
            vec!["Tell me if a disk is nearly full.\n\nThe check command `df -h` printed these new lines since the last run:\n```\n/dev/sda1 91%\n```".to_string()]
        );
        // A judged no still moves the diff base, so the same lines aren't judged again.
        let row = &automations::load(&dir).expect("reload")[0];
        assert_eq!(row.gate.last_output, "/dev/sda1 91%\n/dev/sdb1 13%");
        assert_eq!(row.health.outcome, grokhub_core::AutoOutcome::Ok);
    }

    #[test]
    fn a_failed_judge_runs_the_job_on_every_new_line_and_a_plain_check_is_never_judged() {
        let (dir, nine) = gated_job("judge-down", r#"{"onChange":true}"#);
        let mut asks = Vec::new();
        let fired = tick_deciding(
            &dir,
            &at(nine),
            PID,
            &mut |job| {
                asks.push(job.instructions.clone());
                ok("ran")
            },
            &mut |_| receipt("/dev/sda1 91%"),
            &mut |_, _| None,
        )
        .expect("tick");
        assert_eq!(fired[0].result.text, "ran");
        assert!(asks[0].ends_with("```\n/dev/sda1 91%\n```"), "{}", asks[0]);
        let (plain, nine) = gated_job("plain", "{}");
        let mut judged = 0;
        tick_deciding(
            &plain,
            &at(nine),
            PID,
            &mut |_| ok("ran"),
            &mut |_| receipt("/dev/sda1 91%"),
            &mut |_, _| {
                judged += 1;
                None
            },
        )
        .expect("tick");
        assert_eq!(judged, 0);
    }

    #[test]
    fn a_headless_tick_yields_to_an_open_app_and_the_app_takes_over_from_it() {
        let (dir, _id) = nine_am_job("headless");
        let nine = FRI_8AM + HOUR;
        let app = SchedulerLease {
            owner: LeaseOwner::Shell,
            pid: PID,
            beat_ms: nine - 10_000,
        };
        write_lease(&dir, &app).expect("app beat");
        let mut runs = 0;
        let fired = tick_as(
            LeaseOwner::Headless,
            &dir,
            &at(nine),
            PID + 1,
            &mut |_| {
                runs += 1;
                ok("headless ran")
            },
            &mut |_| String::new(),
            &mut |_, _| None,
            &mut |_, _| None,
        )
        .expect("tick");
        assert_eq!((fired.len(), runs), (0, 0));
        // The app's beat goes stale: the headless run fires and beats as headless.
        let fired = tick_as(
            LeaseOwner::Headless,
            &dir,
            &at(nine + 90_000),
            PID + 1,
            &mut |_| ok("headless ran"),
            &mut |_| String::new(),
            &mut |_, _| None,
            &mut |_, _| None,
        )
        .expect("tick");
        assert_eq!(fired.len(), 1);
        assert_eq!(
            read_lease(&dir),
            Some(SchedulerLease {
                owner: LeaseOwner::Headless,
                pid: PID + 1,
                beat_ms: nine + 90_000,
            })
        );
        // An app that opens now is not held off by the headless beat.
        tick(&dir, &at(nine + 100_000), PID, &mut |_| ok("app ran")).expect("tick");
        assert_eq!(read_lease(&dir).map(|l| l.owner), Some(LeaseOwner::Shell));
    }

    #[test]
    fn preflight_blocks_before_claim_and_decide() {
        let (dir, _id) = nine_am_job("preflight");
        let nine = FRI_8AM + HOUR;
        let mut runs = 0;
        let mut decides = 0;
        let fired = tick_as(
            LeaseOwner::Shell,
            &dir,
            &at(nine),
            PID,
            &mut |_| {
                runs += 1;
                ok("should not run")
            },
            &mut |_| String::new(),
            &mut |_, _| {
                decides += 1;
                None
            },
            &mut |_, _| Some("Preflight: Sign in with Grok or add an API key.".into()),
        )
        .expect("tick");
        assert_eq!(runs, 0);
        assert_eq!(decides, 0);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].result.stop, PREFLIGHT);
        assert!(fired[0].result.error.contains("Sign in with Grok"));
        let row = &automations::load(&dir).expect("reload")[0];
        assert_eq!(row.run_count, 0);
        assert!(
            grokhub_core::automation_health_line(row)
                .unwrap()
                .contains("Preflight"),
            "{:?}",
            row.health
        );
        assert!(row.next_run.unwrap() > nine);
    }
}

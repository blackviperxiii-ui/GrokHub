//! The shell's scheduler (cross-platform Phase 3b). It fires the jobs in
//! `automations.json` while the window is open or in the tray, but only when
//! the cabin is not running: the cabin beats `scheduler-lease.json` from its
//! pulse, and the shell defers while that beat is fresh.
//!
//! A due job is claimed first (its last run, count and next run are saved),
//! then it runs unattended and read-only, as Run now does, whatever the
//! approvals setting. Jobs gated by a check command stay with the cabin.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grokhub_agent::ModelClient;
use grokhub_core::scheduler_lease::{
    read_lease, shell_may_fire, write_lease, LeaseOwner, SchedulerLease,
};
use grokhub_core::{Automation, LocalClock};
use serde::Serialize;

use crate::automations::{self, Clock, RunResult};

/// The event the UI hears when a scheduled run ends.
pub const RAN_EVENT: &str = "automations://ran";
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

fn utc_offset() -> Option<i32> {
    *UTC_OFFSET.lock().unwrap_or_else(|e| e.into_inner())
}

/// The local wall clock at `now_ms` for a UTC offset. 1 Jan 1970 was a Thursday.
pub fn local_clock(now_ms: u64, offset_min: i32) -> LocalClock {
    let local = (now_ms / 60_000) as i64 + i64::from(offset_min);
    let (day, tod) = (local.div_euclid(1440), local.rem_euclid(1440));
    LocalClock {
        now_ms,
        weekday: (day + 4).rem_euclid(7) as u8,
        hour: (tod / 60) as u32,
        minute: (tod % 60) as u32,
    }
}

/// One look for due jobs. With the lease, each due job is claimed and then
/// handed to `run`, one at a time, at most once per tick. The lease is checked
/// again before each claim, so a cabin that opens mid-tick takes over.
pub fn tick(
    config_dir: &Path,
    clock: &dyn Fn() -> LocalClock,
    pid: u32,
    run: &mut dyn FnMut(&Automation) -> RunResult,
) -> Result<Vec<Fired>, String> {
    let mut fired = Vec::new();
    let mut seen = HashSet::new();
    loop {
        let now = clock();
        if !shell_may_fire(read_lease(config_dir).as_ref(), pid, now.now_ms) {
            return Ok(fired);
        }
        let beat = SchedulerLease {
            owner: LeaseOwner::Shell,
            pid,
            beat_ms: now.now_ms,
        };
        write_lease(config_dir, &beat)?;
        let Some(job) = automations::claim_due(config_dir, now, &seen)? else {
            return Ok(fired);
        };
        seen.insert(job.id.clone());
        let result = match automations::Running::claim(&job.id) {
            Ok(_held) => run(&job),
            // A Run now of it started after the claim; that run counts.
            Err(_) => continue,
        };
        // The job may have been removed while it ran.
        let _ = automations::settle_run(config_dir, &job.id, &result);
        fired.push(Fired {
            id: job.id,
            name: job.name,
            result,
        });
    }
}

/// What a scheduled run needs: the model client, model and work folder from
/// the current settings, or why there is none (no API key).
pub type Route = (Arc<dyn ModelClient + Send + Sync>, String, PathBuf);

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
        let clock = || local_clock(grokhub_core::now_ms(), offset);
        let mut run = |job: &Automation| match route() {
            Ok((client, model, workspace)) => {
                automations::execute(&config_dir, job, client, &model, workspace)
            }
            Err(error) => RunResult {
                text: String::new(),
                error,
                stop: "error".into(),
            },
        };
        if let Ok(fired) = tick(&config_dir, &clock, std::process::id(), &mut run) {
            for f in &fired {
                emit(f);
            }
        }
    });
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
    fn a_job_with_a_check_command_is_left_to_the_cabin() {
        let dir = temp_dir("check");
        let nine = FRI_8AM + HOUR;
        std::fs::write(
            automations::path(&dir),
            format!(
                r#"[{{"id":"auto-gated","name":"tidy","schedule":"daily","time":"09:00","instructions":"tidy downloads","checkCommand":"test -d ~/Downloads","enabled":true,"nextRun":{nine}}}]"#
            ),
        )
        .expect("seed");
        let before = std::fs::read_to_string(automations::path(&dir)).expect("before");
        let fired = tick(&dir, &at(nine), PID, &mut |_| ok("")).expect("tick");
        assert_eq!(fired, Vec::new());
        assert_eq!(
            std::fs::read_to_string(automations::path(&dir)).expect("after"),
            before
        );
        let rows = automations::rows(&automations::load(&dir).expect("load"), at(nine)());
        assert!(rows[0].cabin_only);
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
            automations::execute(&dir, job, client.clone(), "grok-4.7", workspace.clone())
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
}

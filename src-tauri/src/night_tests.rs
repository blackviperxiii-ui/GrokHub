use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use grokhub_agent::episode::lessons::{self, Lesson};
use grokhub_agent::CancelToken;
use grokhub_agent::{ClientError, ResponsesRequest, StreamEvent, TurnOutput};
use grokhub_core::amr::{AmrStore, NodeDraft, NodeType, Sensitivity};
use grokhub_core::scheduler_lease::{write_lease, LeaseOwner, SchedulerLease};

use super::*;

/// 2026-10-07 at 02:00, 03:00, 03:05 and 23:00 UTC.
const AT_2AM: u64 = 1_791_338_400_000;
const AT_3AM: u64 = 1_791_342_000_000;
const AT_305: u64 = 1_791_342_300_000;
const AT_11PM: u64 = 1_791_414_000_000;
const MIN: u64 = 60_000;
const PID: u32 = 4242;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shell-night-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn app_json(dir: &Path, text: &str) {
    std::fs::write(dir.join("app.json"), text).unwrap();
}

fn no_model() -> Result<Box<dyn ModelClient>, String> {
    panic!("no model call expected")
}

fn run(dir: &Path, now: u64, offset: i32, busy: bool, state: &mut NightState) -> Look {
    look(dir, PID, now, Some(offset), busy, &no_model, state)
}

/// Two notes that say the same thing, as the cabin's dream test seeds.
fn repeat_notes(dir: &Path) -> AmrStore {
    let _config = grokhub_agent::perm::ConfigGuard::set(dir);
    let store = grokhub_agent::amr_store();
    store.init().unwrap();
    let stamp = grokhub_core::oauth::unix_ms_to_rfc3339(AT_2AM);
    for (id, body, confidence) in [
        ("quay-a", "The quay lantern is green at night", 0.9),
        ("quay-b", "the quay lantern is green at night.", 0.5),
    ] {
        store
            .remember(&NodeDraft {
                id: id.into(),
                node_type: NodeType::Fact,
                created: stamp.clone(),
                updated: stamp.clone(),
                source: "user".into(),
                confidence,
                tags: vec![],
                body: format!("{body}\n"),
                sensitivity: Sensitivity::Plain,
                consent_ref: String::new(),
            })
            .unwrap();
    }
    store
}

fn twin_lessons(dir: &Path) {
    for ts in [AT_2AM - 2, AT_2AM - 1] {
        let lesson = Lesson {
            goal_keywords: vec!["save".into()],
            app: "Gedit".into(),
            what_failed: "replan: a".into(),
            what_worked: "then b".into(),
            outcome: "verified".into(),
            episode_id: format!("ep-{ts}"),
            ts,
            seen: 1,
            failed_after: 0,
        };
        lessons::append(dir, &lesson).unwrap();
    }
}

fn memory_file(dir: &Path, name: &str, text: &str) {
    std::fs::create_dir_all(dir.join("memory")).unwrap();
    std::fs::write(dir.join("memory").join(name), text).unwrap();
}

fn report(dir: &Path, day: &str) -> PathBuf {
    dir.join("amr").join("dreams").join(format!("{day}.md"))
}

#[test]
fn a_night_runs_once_after_the_hour_and_tries_again_when_busy() {
    let d = temp_dir("once");
    app_json(&d, "{\"memory_backend\":\"amr\"}\n");
    memory_file(&d, "USER.md", "- Likes tea\n");
    memory_file(&d, "SOUL.md", "Be brief.\n");
    let store = repeat_notes(&d);
    twin_lessons(&d);
    let mut state = NightState::default();

    assert_eq!(
        run(&d, AT_2AM, 0, false, &mut state),
        Look::Ran(Night {
            lessons: false,
            dream: Dream::NotDue
        })
    );
    assert!(!report(&d, "2026-10-07").exists());
    assert_eq!(lessons::load(&d).len(), 2, "no tidy before the hour");

    // Busy at the hour: the lessons tidy (local, no model) still runs; the
    // dream waits.
    assert_eq!(run(&d, AT_3AM, 0, true, &mut state), Look::Busy);
    assert_eq!(lessons::load(&d).len(), 1);
    assert!(!report(&d, "2026-10-07").exists());

    assert_eq!(
        run(&d, AT_305, 0, false, &mut state),
        Look::Ran(Night {
            lessons: false,
            dream: Dream::Dreamed { card: false }
        })
    );
    let text = std::fs::read_to_string(report(&d, "2026-10-07")).unwrap();
    assert!(text.starts_with("# Memory dream 2026-10-07\n"), "{text}");
    assert!(text.contains("- Merged: 1\n"), "{text}");
    assert!(text.contains("Kept `quay-a`"), "{text}");
    assert!(store.is_forgotten("quay-b") && !store.is_forgotten("quay-a"));
    assert_eq!(
        std::fs::read_to_string(d.join("memory").join("USER.md")).unwrap(),
        "- Likes tea\n"
    );
    assert_eq!(
        std::fs::read_to_string(d.join("memory").join("SOUL.md")).unwrap(),
        "Be brief.\n"
    );

    assert_eq!(
        run(&d, AT_11PM, 0, false, &mut state),
        Look::Ran(Night {
            lessons: false,
            dream: Dream::NotDue
        })
    );
    // A relaunch the same night finds the report.
    assert_eq!(
        run(&d, AT_11PM, 0, false, &mut NightState::default()),
        Look::Ran(Night {
            lessons: true,
            dream: Dream::AlreadyDone
        })
    );
    let shown = crate::memory::dream_report(&d);
    assert!(shown.contains("# Memory dream 2026-10-07"), "{shown}");
    assert!(shown.contains("merged `quay-b`"), "{shown}");
}

#[test]
fn a_halt_skips_the_night_and_a_later_send_does_not_bring_it_back() {
    let d = temp_dir("halt");
    app_json(&d, "{\"memory_backend\":\"amr\"}\n");
    repeat_notes(&d);
    std::fs::write(
        d.join(grokhub_core::desktop_mcp::HALT_STAMP_FILE),
        (AT_3AM - MIN).to_string(),
    )
    .unwrap();
    let mut state = NightState::default();
    assert_eq!(run(&d, AT_3AM, 0, false, &mut state), Look::Halted);
    assert_eq!(
        run(&d, AT_3AM + 20 * MIN, 0, false, &mut state),
        Look::Ran(Night {
            lessons: false,
            dream: Dream::NotDue
        })
    );
    assert!(!report(&d, "2026-10-07").exists());
}

#[test]
fn the_halt_hold_ends_after_its_minutes_or_at_zero_lasts() {
    let d = temp_dir("hold");
    let stamp = d.join(grokhub_core::desktop_mcp::HALT_STAMP_FILE);
    assert!(!halted(&d, AT_3AM), "no stamp, no hold");
    std::fs::write(&stamp, (AT_3AM - 5 * MIN).to_string()).unwrap();
    assert!(halted(&d, AT_3AM));
    std::fs::write(&stamp, (AT_3AM - 16 * MIN).to_string()).unwrap();
    assert!(!halted(&d, AT_3AM), "the 15-minute hold is over");
    app_json(&d, "{\"heartbeat\":{\"haltHoldMin\":0}}\n");
    assert!(halted(&d, AT_3AM + 24 * 60 * MIN), "0 holds until a send");
    app_json(&d, "{\"heartbeat\":{\"haltHoldMin\":30}}\n");
    assert!(halted(&d, AT_3AM));
}

#[test]
fn a_send_after_the_halt_ends_the_hold() {
    let forever = HeartbeatPace {
        halt_hold_min: 0,
        ..PACE_NORMAL
    };
    let stamp = AT_3AM - 5 * MIN;
    assert!(halt_holds(stamp, None, &PACE_NORMAL, AT_3AM));
    assert!(halt_holds(stamp, Some(stamp - MIN), &PACE_NORMAL, AT_3AM));
    assert!(!halt_holds(stamp, Some(stamp + MIN), &PACE_NORMAL, AT_3AM));
    assert!(halt_holds(stamp, Some(stamp - MIN), &forever, AT_11PM));
    assert!(!halt_holds(stamp, Some(stamp + MIN), &forever, AT_11PM));
}

#[test]
fn legacy_memory_never_dreams_or_makes_a_memory_repo() {
    let d = temp_dir("legacy");
    app_json(&d, "{\"memory_backend\":\"legacy\"}\n");
    twin_lessons(&d);
    assert_eq!(
        run(&d, AT_3AM, 0, false, &mut NightState::default()),
        Look::Ran(Night {
            lessons: true,
            dream: Dream::Legacy
        })
    );
    assert!(!d.join("amr").exists());
    assert_eq!(lessons::load(&d).len(), 1, "lessons are tidied in any mode");
}

#[test]
fn memory_writes_off_dreams_nothing() {
    let d = temp_dir("writes-off");
    app_json(&d, "{\"memory_backend\":\"amr\",\"memory_writes\":false}\n");
    assert_eq!(
        run(&d, AT_3AM, 0, false, &mut NightState::default()),
        Look::Ran(Night {
            lessons: true,
            dream: Dream::WritesOff
        })
    );
    assert!(!d.join("amr").join("dreams").exists());
}

#[test]
fn the_report_is_named_by_the_local_day() {
    // 2026-10-07 01:30 UTC: before the hour in UTC, 11:30 on 10-07 at
    // UTC+10, and 20:30 on 10-06 at UTC-5.
    let late_utc = AT_2AM - 30 * MIN;
    let d = temp_dir("local-day");
    app_json(&d, "{\"memory_backend\":\"amr\"}\n");
    repeat_notes(&d);
    assert_eq!(
        run(&d, late_utc, 0, false, &mut NightState::default()),
        Look::Ran(Night {
            lessons: false,
            dream: Dream::NotDue
        })
    );
    assert_eq!(
        run(&d, late_utc, 600, false, &mut NightState::default()),
        Look::Ran(Night {
            lessons: true,
            dream: Dream::Dreamed { card: false }
        })
    );
    assert!(report(&d, "2026-10-07").is_file());
    assert!(!report(&d, "2026-10-06").exists());

    let west = temp_dir("local-day-west");
    app_json(&west, "{\"memory_backend\":\"amr\"}\n");
    repeat_notes(&west);
    assert_eq!(
        run(&west, late_utc, -300, false, &mut NightState::default()),
        Look::Ran(Night {
            lessons: true,
            dream: Dream::Dreamed { card: false }
        })
    );
    assert!(report(&west, "2026-10-06").is_file());
    assert!(!report(&west, "2026-10-07").exists());
}

#[test]
fn the_dream_hour_is_the_one_in_app_json() {
    let d = temp_dir("hour");
    app_json(&d, "{\"memory_backend\":\"amr\",\"dreamHour\":22}\n");
    repeat_notes(&d);
    let mut state = NightState::default();
    assert_eq!(
        run(&d, AT_305, 0, false, &mut state),
        Look::Ran(Night {
            lessons: false,
            dream: Dream::NotDue
        })
    );
    assert_eq!(
        run(&d, AT_11PM, 0, false, &mut state),
        Look::Ran(Night {
            lessons: true,
            dream: Dream::Dreamed { card: false }
        })
    );
}

#[test]
fn an_open_cabin_or_no_clock_runs_nothing() {
    let d = temp_dir("lease");
    app_json(&d, "{\"memory_backend\":\"amr\"}\n");
    repeat_notes(&d);
    write_lease(
        &d,
        &SchedulerLease {
            owner: LeaseOwner::Cabin,
            pid: 99,
            beat_ms: AT_3AM - 1_000,
        },
    )
    .unwrap();
    let mut state = NightState::default();
    assert_eq!(run(&d, AT_3AM, 0, false, &mut state), Look::CabinOpen);
    std::fs::remove_file(d.join(grokhub_core::scheduler_lease::LEASE_FILE)).unwrap();
    assert_eq!(
        look(&d, PID, AT_3AM, None, false, &no_model, &mut state),
        Look::NoClock
    );
    assert!(!report(&d, "2026-10-07").exists());
}

struct Script {
    calls: AtomicU32,
}

impl ModelClient for Script {
    fn stream(
        &self,
        _req: &ResponsesRequest,
        _cancel: &CancelToken,
        _sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(TurnOutput {
            text: r#"{"lessons":[],"insights":[]}"#.into(),
            ..TurnOutput::default()
        })
    }
}

#[test]
fn the_model_dream_posts_its_card_and_signed_out_falls_back() {
    let d = temp_dir("model");
    app_json(&d, "{\"memory_backend\":\"amr\",\"dream_model\":true}\n");
    repeat_notes(&d);
    let asked = AtomicU32::new(0);
    let signed_out = || -> Result<Box<dyn ModelClient>, String> {
        asked.fetch_add(1, Ordering::SeqCst);
        Err("Sign in first".into())
    };
    assert_eq!(
        look(
            &d,
            PID,
            AT_3AM,
            Some(0),
            false,
            &signed_out,
            &mut NightState::default()
        ),
        Look::Ran(Night {
            lessons: true,
            dream: Dream::Dreamed { card: false }
        })
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert!(report(&d, "2026-10-07").is_file());
    assert!(crate::pulse::cards(&d).unwrap().is_empty());

    // The next night, signed in: the model pass runs once and posts its card.
    let next = AT_3AM + 24 * 60 * MIN;
    let signed_in = || -> Result<Box<dyn ModelClient>, String> {
        Ok(Box::new(Script {
            calls: AtomicU32::new(0),
        }))
    };
    assert_eq!(
        look(
            &d,
            PID,
            next,
            Some(0),
            false,
            &signed_in,
            &mut NightState::default()
        ),
        Look::Ran(Night {
            lessons: true,
            dream: Dream::Dreamed { card: true }
        })
    );
    let cards = crate::pulse::cards(&d).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "Memory dream");
    assert!(report(&d, "2026-10-08").is_file());
}

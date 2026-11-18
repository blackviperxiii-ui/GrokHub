use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use grokhub_agent::episode::lessons::{self, Lesson};
use grokhub_agent::CancelToken;
use grokhub_agent::{ClientError, ContentPart, InputItem, ResponsesRequest, StreamEvent};
use grokhub_agent::{TurnOutput, Usage};
use grokhub_core::amr::{AmrStore, NodeDraft, NodeType, Sensitivity};
use grokhub_core::scheduler_lease::{write_lease, LeaseOwner, SchedulerLease};

use super::*;

/// 2026-10-07 at 02:00, 03:00, 03:05 and 23:00 UTC.
const AT_2AM: u64 = 1_791_338_400_000;
const AT_3AM: u64 = 1_791_342_000_000;
const AT_305: u64 = 1_791_342_300_000;
const AT_11PM: u64 = 1_791_414_000_000;
const MIN: u64 = 60_000;
const DAY: u64 = 24 * 60 * MIN;
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

/// One learned note, updated at `at`.
fn note(store: &AmrStore, id: &str, node_type: NodeType, body: &str, confidence: f32, at: u64) {
    let stamp = grokhub_core::oauth::unix_ms_to_rfc3339(at);
    store
        .remember(&NodeDraft {
            id: id.into(),
            node_type,
            created: stamp.clone(),
            updated: stamp,
            source: "user".into(),
            confidence,
            tags: vec![],
            body: format!("{body}\n"),
            sensitivity: Sensitivity::Plain,
            consent_ref: String::new(),
        })
        .unwrap();
}

fn store(dir: &Path) -> AmrStore {
    let _config = grokhub_agent::perm::ConfigGuard::set(dir);
    let store = grokhub_agent::amr_store();
    store.init().unwrap();
    store
}

/// Two notes that say the same thing, as the cabin's dream test seeds.
fn repeat_notes(dir: &Path) -> AmrStore {
    let store = store(dir);
    note(
        &store,
        "quay-a",
        NodeType::Fact,
        "The quay lantern is green at night",
        0.9,
        AT_2AM,
    );
    note(
        &store,
        "quay-b",
        NodeType::Fact,
        "the quay lantern is green at night.",
        0.5,
        AT_2AM,
    );
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
            review: Some(Reviewed::NotDue),
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
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
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
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
            dream: Dream::NotDue
        })
    );
    // A relaunch the same night finds the report.
    assert_eq!(
        run(&d, AT_11PM, 0, false, &mut NightState::default()),
        Look::Ran(Night {
            lessons: true,
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
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
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
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
fn only_a_message_you_send_ends_the_hold() {
    let d = temp_dir("sent");
    app_json(&d, "{\"heartbeat\":{\"haltHoldMin\":0}}\n");
    std::fs::write(
        d.join(grokhub_core::desktop_mcp::HALT_STAMP_FILE),
        (AT_3AM - 5 * MIN).to_string(),
    )
    .unwrap();
    // A turn saved after the Halt (a halted reply, an automation) is not a send.
    {
        let _config = grokhub_agent::perm::ConfigGuard::set(&d);
        let say = |role: &str, text: &str| InputItem::Message {
            role: role.into(),
            content: vec![ContentPart::InputText(text.into())],
        };
        grokhub_agent::record_turn(
            &grokhub_agent::scratch::shell_id(false, "1"),
            "/home/user",
            "grok-test",
            &[say("user", "check the feed"), say("assistant", "Stopped.")],
            &Usage::default(),
            "",
            "check the feed",
        )
        .unwrap();
    }
    let saved = crate::history::list(&d).iter().map(|r| r.updated).max();
    assert!(saved.is_some_and(|at| at > AT_3AM), "{saved:?}");
    assert!(halted(&d, AT_3AM));

    note_send(&d, AT_3AM - 6 * MIN);
    assert!(halted(&d, AT_3AM), "a send before the Halt doesn't end it");
    note_send(&d, AT_3AM - MIN);
    assert!(!halted(&d, AT_3AM));
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
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
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
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
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
            review: Some(Reviewed::NotDue),
            dream: Dream::NotDue
        })
    );
    assert_eq!(
        run(&d, late_utc, 600, false, &mut NightState::default()),
        Look::Ran(Night {
            lessons: true,
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
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
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
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
            review: Some(Reviewed::NotDue),
            dream: Dream::NotDue
        })
    );
    assert_eq!(
        run(&d, AT_11PM, 0, false, &mut state),
        Look::Ran(Night {
            lessons: true,
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
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
    calls: Arc<AtomicU32>,
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
            text: r#"{"lessons":[{"text":"Save the quay chart before closing Gedit","source":"ep-7"}],"insights":[]}"#.into(),
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
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
            dream: Dream::Dreamed { card: false }
        })
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert!(report(&d, "2026-10-07").is_file());
    assert!(crate::pulse::cards(&d).unwrap().is_empty());

    // The next night, signed in: the model is called once, its lesson is
    // learned, and the card is posted.
    let next = AT_3AM + DAY;
    let calls = Arc::new(AtomicU32::new(0));
    let signed_in = || -> Result<Box<dyn ModelClient>, String> {
        Ok(Box::new(Script {
            calls: Arc::clone(&calls),
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
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
            dream: Dream::Dreamed { card: true }
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let skill =
        std::fs::read_to_string(d.join("skills").join("dream-lessons").join("SKILL.md")).unwrap();
    assert!(
        skill.contains("Save the quay chart before closing Gedit (ep-7)\n"),
        "{skill}"
    );
    let cards = crate::pulse::cards(&d).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "Memory dream");
    assert!(report(&d, "2026-10-08").is_file());
}

#[test]
fn missing_preferences_are_proposed_even_with_no_user_md() {
    let seeded = |name: &str| {
        let d = temp_dir(name);
        app_json(&d, "{\"memory_backend\":\"amr\"}\n");
        let store = store(&d);
        note(
            &store,
            "pref-dark",
            NodeType::Preference,
            "Prefers dark mode in every editor",
            0.95,
            AT_2AM,
        );
        note(
            &store,
            "pref-terse",
            NodeType::Preference,
            "Wants terse replies without emoji",
            0.95,
            AT_2AM,
        );
        note(
            &store,
            "pref-maybe",
            NodeType::Preference,
            "Might like tabs over spaces",
            0.5,
            AT_2AM,
        );
        d
    };
    let proposed = |d: &Path| {
        assert_eq!(
            run(d, AT_3AM, 0, false, &mut NightState::default()),
            Look::Ran(Night {
                lessons: true,
                review: Some(Reviewed::NotWorth { new_turns: 0 }),
                dream: Dream::Dreamed { card: false }
            })
        );
        let text = std::fs::read_to_string(report(d, "2026-10-07")).unwrap();
        let (_, diff) = text
            .split_once("## Proposed USER.md changes (not applied)")
            .unwrap_or_else(|| panic!("no proposals: {text}"));
        diff.lines()
            .filter(|l| l.starts_with("+- "))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };

    let with = seeded("prefs");
    memory_file(&with, "USER.md", "- Prefers dark mode in every editor\n");
    assert_eq!(
        proposed(&with),
        ["+- Wants terse replies without emoji"],
        "only the one missing from USER.md"
    );
    assert_eq!(
        std::fs::read_to_string(with.join("memory").join("USER.md")).unwrap(),
        "- Prefers dark mode in every editor\n",
        "USER.md is never written"
    );

    let without = seeded("prefs-none");
    assert_eq!(
        proposed(&without),
        [
            "+- Prefers dark mode in every editor",
            "+- Wants terse replies without emoji"
        ]
    );
    assert!(!without.join("memory").join("USER.md").exists());
}

#[test]
fn memory_retention_sets_when_an_unsure_note_is_retired() {
    let seeded = |name: &str, app: &str| {
        let d = temp_dir(name);
        app_json(&d, app);
        let store = store(&d);
        note(
            &store,
            "old-guess",
            NodeType::Fact,
            "The ferry might leave at noon",
            0.2,
            AT_3AM - 40 * DAY,
        );
        (d, store)
    };
    let (month, store) = seeded(
        "retain-30",
        "{\"memory_backend\":\"amr\",\"amrRetentionDays\":30}\n",
    );
    run(&month, AT_3AM, 0, false, &mut NightState::default());
    let text = std::fs::read_to_string(report(&month, "2026-10-07")).unwrap();
    assert!(text.contains("- Retired: 1\n"), "{text}");
    assert!(text.contains("Retired `old-guess`"), "{text}");
    assert!(text.contains("(over 30 days)"), "{text}");
    assert!(store.is_forgotten("old-guess"));

    let (default, store) = seeded("retain-90", "{\"memory_backend\":\"amr\"}\n");
    run(&default, AT_3AM, 0, false, &mut NightState::default());
    let text = std::fs::read_to_string(report(&default, "2026-10-07")).unwrap();
    assert!(text.contains("- Retired: 0\n"), "{text}");
    assert!(!store.is_forgotten("old-guess"));
}

#[test]
fn a_failed_dream_is_not_retried_the_same_night() {
    let d = temp_dir("failed");
    app_json(&d, "{\"memory_backend\":\"amr\"}\n");
    // A file where the memory repo should be.
    std::fs::write(d.join("amr"), "not a folder").unwrap();
    let mut state = NightState::default();
    let first = run(&d, AT_3AM, 0, false, &mut state);
    assert!(
        matches!(
            &first,
            Look::Ran(Night {
                lessons: true,
                review: Some(Reviewed::NotWorth { new_turns: 0 }),
                dream: Dream::Failed(_)
            })
        ),
        "{first:?}"
    );
    assert_eq!(
        run(&d, AT_305, 0, false, &mut state),
        Look::Ran(Night {
            lessons: false,
            review: None,
            dream: Dream::NotDue
        })
    );
    assert!(d.join("amr").is_file());
}

#[test]
fn slash_dream_names_its_report_by_your_day_and_never_waits_on_the_night() {
    let d = temp_dir("slash");
    app_json(&d, "{\"memory_backend\":\"amr\"}\n");
    repeat_notes(&d);
    let _config = grokhub_agent::perm::ConfigGuard::set(&d);
    {
        // The night holds the dream through its model call.
        let _night = crate::memory::DREAM.lock().unwrap();
        assert_eq!(
            crate::memory::dream_now(&d).unwrap(),
            "A dream is already running. Its report shows here when it's done."
        );
    }
    let reports = || {
        std::fs::read_dir(d.join("amr").join("dreams"))
            .map(|dir| dir.count())
            .unwrap_or(0)
    };
    assert_eq!(reports(), 0);

    // UTC+14 and UTC-12 are always on different days.
    let mut days = Vec::new();
    for offset in [840, -720] {
        let now = grokhub_core::now_ms();
        let at = crate::scheduler::local_clock(now, offset);
        crate::scheduler::note_ui_clock(
            crate::automations::Clock {
                weekday: at.weekday,
                hour: at.hour,
                minute: at.minute,
            },
            now,
        );
        let said = crate::memory::dream_now(&d).unwrap();
        assert!(said.contains("# Memory dream"), "{said}");
        days.push(crate::self_review::local_day(now, offset));
    }
    assert_ne!(days[0], days[1]);
    for day in &days {
        assert!(report(&d, day).is_file(), "no report for {day}");
    }
    assert_eq!(reports(), 2);
}

/// Answers every call with `text`, counting calls and keeping the last
/// conversation id.
struct Answer {
    text: &'static str,
    calls: Arc<AtomicU32>,
    conversation: Arc<std::sync::Mutex<String>>,
}

impl ModelClient for Answer {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        _sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.conversation.lock().unwrap() = req.conversation_id.clone();
        Ok(TurnOutput {
            text: self.text.into(),
            ..TurnOutput::default()
        })
    }
}

#[test]
fn the_chat_review_runs_before_the_dream_and_the_dream_waits_a_look() {
    let d = temp_dir("review");
    app_json(&d, "{\"memory_backend\":\"amr\"}\n");
    repeat_notes(&d);
    {
        let _config = grokhub_agent::perm::ConfigGuard::set(&d);
        let say = |role: &str, text: &str| InputItem::Message {
            role: role.into(),
            content: vec![ContentPart::InputText(text.into())],
        };
        let asks = [
            "Can you tidy the board cards for me",
            "Please tidy the board cards again today",
            "Draft a note about the garden plans",
            "Read me the last release notes",
            "Find the receipt from the bike shop",
            "Rename the photos from the trip",
            "Open the reading list in notes",
            "What is the weather like in the harbor",
        ];
        let items: Vec<InputItem> = asks
            .iter()
            .flat_map(|a| [say("user", a), say("assistant", "Done.")])
            .collect();
        grokhub_agent::record_turn(
            &grokhub_agent::scratch::shell_id(false, "1"),
            "/home/user",
            "grok-test",
            &items,
            &Usage::default(),
            "",
            asks[0],
        )
        .unwrap();
    }
    let calls = Arc::new(AtomicU32::new(0));
    let conversation = Arc::new(std::sync::Mutex::new(String::new()));
    let model = || -> Result<Box<dyn ModelClient>, String> {
        Ok(Box::new(Answer {
            text: "SUGGEST_SKILL: tidy-board | Tidy the board | Sort open cards | when the board is full | sort cards by due date",
            calls: Arc::clone(&calls),
            conversation: Arc::clone(&conversation),
        }))
    };
    let mut state = NightState::default();
    // Busy: neither the review nor the dream runs.
    assert_eq!(
        look(&d, PID, AT_3AM, Some(0), true, &model, &mut state),
        Look::Busy
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        look(&d, PID, AT_305, Some(0), false, &model, &mut state),
        Look::Ran(Night {
            lessons: false,
            review: Some(Reviewed::Ran {
                kept: 1,
                ideas: 1,
                patched: 0
            }),
            dream: Dream::NotDue
        })
    );
    // The dream waits for the next look, which reads busy again.
    assert_eq!(
        look(&d, PID, AT_305 + 5 * MIN, Some(0), true, &model, &mut state),
        Look::Busy
    );
    assert!(!report(&d, "2026-10-07").exists());
    assert_eq!(
        look(
            &d,
            PID,
            AT_305 + 10 * MIN,
            Some(0),
            false,
            &model,
            &mut state
        ),
        Look::Ran(Night {
            lessons: false,
            review: None,
            dream: Dream::Dreamed { card: false }
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the dream made no call");
    assert_eq!(*conversation.lock().unwrap(), "background:review");
    let ideas: Vec<String> = crate::pulse::cards(&d)
        .unwrap()
        .into_iter()
        .map(|c| c.source_id)
        .collect();
    assert_eq!(ideas, vec!["skill-tidy-board".to_string()]);
    // Later the same night: the review does not look again.
    assert_eq!(
        look(&d, PID, AT_11PM, Some(0), false, &model, &mut state),
        Look::Ran(Night {
            lessons: false,
            review: None,
            dream: Dream::NotDue
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_heartbeat_pace_that_is_off_spends_no_review_call() {
    let d = temp_dir("pace-off");
    app_json(
        &d,
        "{\"memory_backend\":\"amr\",\"heartbeat\":{\"maxPerDay\":0}}\n",
    );
    let off = pace(&d);
    assert!(off.is_off(), "{off:?}");
    assert_eq!(
        run(&d, AT_3AM, 0, false, &mut NightState::default()),
        Look::Ran(Night {
            lessons: true,
            review: None,
            dream: Dream::Dreamed { card: false }
        })
    );
}

fn seed_review_turns(d: &Path, id: &str, count: usize) {
    let _config = grokhub_agent::perm::ConfigGuard::set(d);
    let say = |role: &str, text: &str| InputItem::Message {
        role: role.into(),
        content: vec![ContentPart::InputText(text.into())],
    };
    let items: Vec<InputItem> = (0..count)
        .flat_map(|i| {
            [
                say("user", &format!("Check item {id}-{i} on the harbor list")),
                say("assistant", "Done."),
            ]
        })
        .collect();
    grokhub_agent::record_turn(
        &grokhub_agent::scratch::shell_id(false, id),
        "/home/user",
        "grok-test",
        &items,
        &Usage::default(),
        "",
        "harbor list",
    )
    .unwrap();
}

#[test]
fn a_spent_heartbeat_day_cap_spends_no_review_call() {
    let d = temp_dir("day-cap");
    app_json(
        &d,
        "{\"memory_backend\":\"amr\",\"heartbeat\":{\"maxPerDay\":1,\"minIntervalMin\":0}}\n",
    );
    seed_review_turns(&d, "1", 8);
    let calls = Arc::new(AtomicU32::new(0));
    let conversation = Arc::new(std::sync::Mutex::new(String::new()));
    let model = || -> Result<Box<dyn ModelClient>, String> {
        Ok(Box::new(Answer {
            text: "NOTHING",
            calls: Arc::clone(&calls),
            conversation: Arc::clone(&conversation),
        }))
    };

    let mut state = NightState::default();
    let spent_at = AT_3AM - 2 * 60 * MIN;
    state
        .throttle
        .record_act(ProactiveAct::Anticipate, spent_at);
    state
        .throttle
        .record_outcome(ProactiveAct::Anticipate, ActOutcome::Useful);

    assert_eq!(
        look(&d, PID, AT_3AM, Some(0), false, &model, &mut state),
        Look::Ran(Night {
            lessons: true,
            review: None,
            dream: Dream::Dreamed { card: false },
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // Once the 24-hour window clears (and at or after the dream hour), the
    // missed review catches up.
    assert_eq!(
        look(&d, PID, AT_3AM + DAY, Some(0), false, &model, &mut state),
        Look::Ran(Night {
            lessons: true,
            review: Some(Reviewed::Ran {
                kept: 0,
                ideas: 0,
                patched: 0,
            }),
            dream: Dream::NotDue,
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_review_call_counts_against_the_day_and_hour_caps() {
    let d = temp_dir("review-caps");
    app_json(
        &d,
        "{\"memory_backend\":\"amr\",\"dreamHour\":3,\"heartbeat\":{\"maxPerDay\":1,\"maxPerHour\":1,\"minIntervalMin\":0}}\n",
    );
    seed_review_turns(&d, "1", 8);
    let calls = Arc::new(AtomicU32::new(0));
    let conversation = Arc::new(std::sync::Mutex::new(String::new()));
    let model = || -> Result<Box<dyn ModelClient>, String> {
        Ok(Box::new(Answer {
            text: "NOTHING",
            calls: Arc::clone(&calls),
            conversation: Arc::clone(&conversation),
        }))
    };

    let mut state = NightState::default();
    // Day 1 late night: review #1 runs and spends the single day/hour slot.
    assert_eq!(
        look(&d, PID, AT_11PM, Some(0), false, &model, &mut state),
        Look::Ran(Night {
            lessons: true,
            review: Some(Reviewed::Ran {
                kept: 0,
                ideas: 0,
                patched: 0,
            }),
            dream: Dream::NotDue,
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Day 2 early morning (4h later): new local day, still inside the 24h cap.
    seed_review_turns(&d, "2", 8);
    assert_eq!(
        look(&d, PID, AT_3AM + DAY, Some(0), false, &model, &mut state),
        Look::Ran(Night {
            lessons: true,
            review: None,
            dream: Dream::Dreamed { card: false },
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Day 2 after the 24h window clears: review #2 runs.
    assert_eq!(
        look(
            &d,
            PID,
            AT_11PM + DAY + MIN,
            Some(0),
            false,
            &model,
            &mut state
        ),
        Look::Ran(Night {
            lessons: false,
            review: Some(Reviewed::Ran {
                kept: 0,
                ideas: 0,
                patched: 0,
            }),
            dream: Dream::NotDue,
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn a_spent_hour_cap_holds_review_until_the_hour_clears_and_not_worth_spends_nothing() {
    let d = temp_dir("hour-cap");
    app_json(
        &d,
        "{\"memory_backend\":\"amr\",\"heartbeat\":{\"maxPerHour\":1,\"maxPerDay\":1,\"minIntervalMin\":0}}\n",
    );
    let calls = Arc::new(AtomicU32::new(0));
    let conversation = Arc::new(std::sync::Mutex::new(String::new()));
    let model = || -> Result<Box<dyn ModelClient>, String> {
        Ok(Box::new(Answer {
            text: "NOTHING",
            calls: Arc::clone(&calls),
            conversation: Arc::clone(&conversation),
        }))
    };

    let mut state = NightState::default();
    // NotWorth (< 8 new turns) spends no cap slot.
    assert_eq!(
        look(&d, PID, AT_3AM, Some(0), false, &model, &mut state),
        Look::Ran(Night {
            lessons: true,
            review: Some(Reviewed::NotWorth { new_turns: 0 }),
            dream: Dream::Dreamed { card: false },
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // Another heartbeat act inside the hour holds the review once 8 new turns arrive.
    seed_review_turns(&d, "1", 8);
    let after_recount = AT_3AM + REVIEW_RECOUNT_MS;
    state
        .throttle
        .record_act(ProactiveAct::Ideas, after_recount - 10 * MIN);
    state
        .throttle
        .record_outcome(ProactiveAct::Ideas, ActOutcome::Useful);
    // Only test HourCap by loosening day cap via app.json.
    app_json(
        &d,
        "{\"memory_backend\":\"amr\",\"heartbeat\":{\"maxPerHour\":1,\"maxPerDay\":5,\"minIntervalMin\":0}}\n",
    );
    assert_eq!(
        look(&d, PID, after_recount, Some(0), false, &model, &mut state),
        Look::Ran(Night {
            lessons: false,
            review: None,
            dream: Dream::NotDue,
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    assert_eq!(
        look(
            &d,
            PID,
            after_recount + 51 * MIN,
            Some(0),
            false,
            &model,
            &mut state
        ),
        Look::Ran(Night {
            lessons: false,
            review: Some(Reviewed::Ran {
                kept: 0,
                ideas: 0,
                patched: 0,
            }),
            dream: Dream::NotDue,
        })
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

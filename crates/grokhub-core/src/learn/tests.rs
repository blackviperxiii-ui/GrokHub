use std::fs;
use std::path::PathBuf;

use super::belief::{BELIEF_CAP, HALF_LIFE_MS};
use super::signal::{FLUSH_AT, FLUSH_MS, RING, SUBJECT_MAX};
use super::store::{self, DISK_BUDGET, SIGNAL_BYTES};
use super::*;

const T0: u64 = 1_800_000_000_000;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grokhub-learn-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn subject(text: &str) -> Subject {
    Subject::parse(text).unwrap()
}

fn close(got: f32, want: f32) {
    assert!((got - want).abs() < 0.001, "got {got}, want {want}");
}

fn votes(beliefs: &mut Beliefs, key: &Subject, yes: usize, no: usize, at: u64) {
    for _ in 0..yes {
        beliefs.vote(key, true, at, "test");
    }
    for _ in 0..no {
        beliefs.vote(key, false, at, "test");
    }
}

#[test]
fn a_subject_is_an_id_never_a_sentence() {
    assert_eq!(
        Subject::new("setting", "textSize").unwrap().as_str(),
        "setting:textSize"
    );
    assert_eq!(subject("model:x-ai/grok-code.1").area(), "model");
    for bad in [
        "my name is Alex",
        "chip:two words",
        "chip:",
        ":id",
        "noarea",
        "chip:caf\u{e9}",
        "chip:line\nbreak",
    ] {
        assert_eq!(Subject::parse(bad), None, "{bad:?}");
    }
    assert_eq!(Subject::new("a:b", "c"), None);
    assert_eq!(Subject::new("chip", ""), None);
    let long = format!("chip:{}", "a".repeat(SUBJECT_MAX));
    assert_eq!(Subject::parse(&long), None);
    // The same guard reads a file back: a hand-edited line can't smuggle text in.
    let line = r#"{"t":1,"k":"picked","s":"chip:two words"}"#;
    assert!(serde_json::from_str::<Signal>(line).is_err());
}

#[test]
fn a_signal_line_is_short_and_reads_back() {
    let picked = Signal::new(T0, Kind::Picked, subject("chip:3fa9c1"));
    let line = serde_json::to_string(&picked).unwrap();
    assert_eq!(
        line,
        r#"{"t":1800000000000,"k":"picked","s":"chip:3fa9c1"}"#
    );
    assert_eq!(serde_json::from_str::<Signal>(&line).unwrap(), picked);

    let slow = Signal::measured(T0, subject("model:grok-code/first-word-ms"), 840.0);
    let line = serde_json::to_string(&slow).unwrap();
    assert_eq!(
        line,
        r#"{"t":1800000000000,"k":"measured","s":"model:grok-code/first-word-ms","v":840.0}"#
    );
    assert_eq!(serde_json::from_str::<Signal>(&line).unwrap(), slow);
}

#[test]
fn confidence_grows_with_agreeing_evidence() {
    let mut b = Beliefs::default();
    let (three, twelve, split, never) = (
        subject("chip:three"),
        subject("chip:twelve"),
        subject("chip:split"),
        subject("chip:never"),
    );
    votes(&mut b, &three, 3, 0, T0);
    votes(&mut b, &twelve, 12, 0, T0);
    votes(&mut b, &split, 6, 6, T0);
    votes(&mut b, &never, 0, 12, T0);

    close(b.get(&three).unwrap().confidence(T0), 0.526);
    close(b.get(&twelve).unwrap().confidence(T0), 0.816);
    close(b.get(&split).unwrap().confidence(T0), 0.286);
    assert_eq!(b.get(&twelve).unwrap().rate(), Some(1.0));
    assert_eq!(b.get(&twelve).unwrap().leans_yes(), Some(true));
    // Twelve times passed over is as sure as twelve times picked, the other way.
    close(b.get(&never).unwrap().confidence(T0), 0.816);
    assert_eq!(b.get(&never).unwrap().rate(), Some(0.0));
    assert_eq!(b.get(&never).unwrap().leans_yes(), Some(false));
}

#[test]
fn old_evidence_fades_by_half_every_thirty_days() {
    let mut b = Beliefs::default();
    let key = subject("habit:thoughts-closed");
    votes(&mut b, &key, 12, 0, T0);
    let later = T0 + HALF_LIFE_MS;
    // Read a month on, nothing written: the belief is weaker on its own.
    close(b.get(&key).unwrap().evidence(later), 6.0);
    close(b.get(&key).unwrap().confidence(later), 0.689);
    // One new vote lands on the faded counts: 6 + 1, not 12 + 1.
    b.vote(&key, true, later, "test");
    close(b.get(&key).unwrap().evidence(later), 7.0);
    // A signal older than the last update neither fades nor rewinds.
    b.vote(&key, false, T0, "test");
    assert_eq!(b.get(&key).unwrap().last_ms, later);
    close(b.get(&key).unwrap().evidence(later), 8.0);
    close(b.get(&key).unwrap().rate().unwrap(), 0.875);
}

#[test]
fn a_measured_number_keeps_a_running_average() {
    let mut b = Beliefs::default();
    let key = subject("model:grok-code/first-word-ms");
    for ms in [100.0, 200.0, 300.0] {
        assert!(b.apply(&Signal::measured(T0, key.clone(), ms)));
    }
    let belief = b.get(&key).unwrap();
    assert_eq!(belief.level(), Some(200.0));
    assert_eq!(belief.rate(), None);
    close(belief.evidence(T0), 3.0);
    close(belief.confidence(T0), 0.526);
    // A subject keeps one shape, and a number that isn't one is refused.
    assert!(!b.vote(&key, true, T0, "test"));
    assert!(!b.measure(&key, f32::NAN, T0, "test"));
    assert_eq!(b.get(&key).unwrap().level(), Some(200.0));
}

#[test]
fn signals_vote_by_kind() {
    let mut b = Beliefs::default();
    let key = subject("chip:patch-notes");
    for kind in [Kind::Picked, Kind::Used, Kind::Accepted, Kind::Liked] {
        assert!(b.apply(&Signal::new(T0, kind, key.clone())));
    }
    for kind in [Kind::Ignored, Kind::Undone] {
        assert!(b.apply(&Signal::new(T0, kind, key.clone())));
    }
    // Being shown, or set by hand, says nothing about liking it.
    for kind in [Kind::Shown, Kind::SetByUser] {
        assert!(!b.apply(&Signal::new(T0, kind, key.clone())));
    }
    assert_eq!(
        b.get(&key).unwrap().shape,
        Shape::Rate { yes: 4.0, no: 2.0 }
    );
    assert_eq!(b.get(&key).unwrap().source, "signals");
}

#[test]
fn forget_drops_the_counts_and_old_signals_stay_forgotten() {
    let mut b = Beliefs::default();
    let key = subject("taste:topic/racing");
    votes(&mut b, &key, 12, 0, T0);
    assert!(b.forget(&key, T0 + 10));
    assert_eq!(b.get(&key).unwrap().confidence(T0 + 10), 0.0);
    assert_eq!(b.get(&key).unwrap().rate(), None);
    // A replay of the old log can't bring it back.
    assert!(!b.vote(&key, true, T0, "test"));
    assert!(!b.vote(&key, true, T0 + 10, "test"));
    assert_eq!(b.get(&key).unwrap().evidence(T0 + 10), 0.0);
    // What the user does afterwards starts it fresh.
    assert!(b.vote(&key, false, T0 + 11, "test"));
    let fresh = b.get(&key).unwrap();
    assert_eq!(fresh.standing, Standing::Live);
    assert_eq!(fresh.shape, Shape::Rate { yes: 0.0, no: 1.0 });
    assert_eq!(fresh.first_ms, T0 + 11);
    // Nothing to forget is not an error and makes no line.
    assert!(!b.forget(&subject("taste:topic/none"), T0));
    assert_eq!(b.len(), 1);
}

#[test]
fn wrong_locks_a_belief_for_good() {
    let mut b = Beliefs::default();
    let key = subject("habit:night-owl");
    votes(&mut b, &key, 20, 0, T0);
    b.mark_wrong(&key, T0 + 1);
    for at in [T0, T0 + 2, T0 + HALF_LIFE_MS] {
        assert!(!b.vote(&key, true, at, "test"));
    }
    let locked = b.get(&key).unwrap();
    assert_eq!(locked.standing, Standing::Wrong);
    assert_eq!(locked.confidence(T0 + 2), 0.0);
    assert_eq!(locked.leans_yes(), None);
    // Forget does not unlock it.
    assert!(!b.forget(&key, T0 + 3));
    assert_eq!(b.get(&key).unwrap().standing, Standing::Wrong);
    // A subject can be ruled out before anything was learned about it.
    let ahead = subject("habit:early-bird");
    b.mark_wrong(&ahead, T0);
    assert!(!b.vote(&ahead, true, T0 + 1, "test"));
    assert_eq!(b.get(&ahead).unwrap().source, "user");
}

#[test]
fn a_full_store_drops_its_weakest_line_and_keeps_locked_ones() {
    let mut b = Beliefs::default();
    let locked = subject("habit:locked");
    b.mark_wrong(&locked, T0);
    let weak = subject("chip:weak");
    b.vote(&weak, true, T0, "test");
    for i in 0..BELIEF_CAP - 2 {
        let key = subject(&format!("chip:strong-{i}"));
        votes(&mut b, &key, 2, 0, T0 + 1);
    }
    assert_eq!(b.len(), BELIEF_CAP);
    b.vote(&subject("chip:new"), true, T0 + 2, "test");
    assert_eq!(b.len(), BELIEF_CAP);
    assert!(b.get(&weak).is_none());
    assert!(b.get(&locked).is_some());
    assert!(b.get(&subject("chip:new")).is_some());
    assert!(b.get(&subject("chip:strong-0")).is_some());
}

#[test]
fn the_ladder_matches_auto_setup_and_asks_more_for_a_restore() {
    assert_eq!(ladder::SUGGEST_AT, 0.5);
    assert_eq!(ladder::ACT_AT, 0.8);
    assert_eq!(step(0.49, 100.0, Undo::OneClick), Step::Watch);
    assert_eq!(step(0.5, 3.0, Undo::OneClick), Step::Suggest);
    assert_eq!(step(0.79, 100.0, Undo::OneClick), Step::Suggest);
    assert_eq!(step(0.8, 12.0, Undo::OneClick), Step::Act);
    // A change that needs a restore point: surer, and never on thin evidence.
    assert_eq!(step(0.89, 100.0, Undo::Restore), Step::Suggest);
    assert_eq!(step(0.95, 9.9, Undo::Restore), Step::Suggest);
    assert_eq!(step(0.9, 10.0, Undo::Restore), Step::Act);
    // What can't be taken back is only ever offered.
    assert_eq!(step(1.0, 1_000.0, Undo::Never), Step::Suggest);
    assert_eq!(step(0.2, 1_000.0, Undo::Never), Step::Watch);
}

#[test]
fn the_stream_is_due_by_count_or_by_age() {
    let mut s = Stream::default();
    assert!(!s.due(T0));
    s.record(Signal::new(T0, Kind::Used, subject("page:pulse")));
    // Nothing was ever written, so the first signal is due at once.
    assert!(s.due(T0));
    assert_eq!(s.drain(T0).len(), 1);
    s.record(Signal::new(T0 + 1, Kind::Used, subject("page:pulse")));
    assert!(!s.due(T0 + FLUSH_MS - 1));
    assert!(s.due(T0 + FLUSH_MS));
    for _ in 1..FLUSH_AT {
        s.record(Signal::new(T0 + 2, Kind::Used, subject("page:pulse")));
    }
    assert_eq!(s.waiting(), FLUSH_AT);
    assert!(s.due(T0 + 2));
}

#[test]
fn recording_never_touches_the_disk() {
    let dir = temp("no-disk");
    let mut e = Engine::open(&dir);
    for i in 0..10_000u64 {
        let key = subject(&format!("chip:c{}", i % 50));
        e.record(Signal::new(T0 + i, Kind::Picked, key));
    }
    assert_eq!(e.writes(), 0);
    assert!(!store::dir(&dir).exists());
    // Memory holds one ring, however many arrive; the beliefs saw them all.
    assert_eq!(e.dropped(), 10_000 - RING as u64);
    assert_eq!(e.beliefs().len(), 50);
    close(
        e.beliefs()
            .get(&subject("chip:c0"))
            .unwrap()
            .evidence(T0 + 9_999),
        200.0,
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_tick_writes_one_batch_and_an_idle_tick_writes_nothing() {
    let dir = temp("tick");
    let mut e = Engine::open(&dir);
    assert!(!e.tick(T0));
    assert_eq!(e.writes(), 0);

    for i in 0..5 {
        e.record(Signal::new(T0 + i, Kind::Picked, subject("chip:a")));
    }
    assert!(e.tick(T0 + 5));
    // One write for the five signals, one for the beliefs.
    assert_eq!(e.writes(), 2);
    assert_eq!(store::read_signals(&dir).len(), 5);
    assert!(!e.tick(T0 + 6));
    assert!(!e.tick(T0 + 6 + FLUSH_MS));
    assert_eq!(e.writes(), 2);

    // The next signals wait their 30 seconds; the beliefs wait five minutes.
    e.record(Signal::new(T0 + 7, Kind::Ignored, subject("chip:a")));
    assert!(!e.tick(T0 + 8));
    assert!(e.tick(T0 + 5 + FLUSH_MS));
    assert_eq!(e.writes(), 3);
    assert!(e.tick(T0 + 5 + BELIEF_SAVE_MS));
    assert_eq!(e.writes(), 4);

    let again = Engine::open(&dir);
    assert_eq!(
        again.beliefs().get(&subject("chip:a")).unwrap().shape,
        Shape::Rate { yes: 5.0, no: 1.0 }
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn flush_saves_what_waits_and_the_users_choices_save_at_once() {
    let dir = temp("flush");
    let mut e = Engine::open(&dir);
    let key = subject("taste:topic/racing");
    e.record(Signal::new(T0, Kind::Liked, key.clone()));
    assert!(e.tick(T0));
    e.record(Signal::new(T0 + 1, Kind::Liked, key.clone()));
    e.flush(T0 + 2);
    assert_eq!(store::read_signals(&dir).len(), 2);
    assert_eq!(
        Engine::open(&dir).beliefs().get(&key).unwrap().shape,
        Shape::Rate { yes: 2.0, no: 0.0 }
    );

    e.mark_wrong(&key, T0 + 3);
    assert_eq!(
        Engine::open(&dir).beliefs().get(&key).unwrap().standing,
        Standing::Wrong
    );
    let other = subject("taste:topic/chess");
    e.record(Signal::new(T0 + 4, Kind::Liked, other.clone()));
    e.forget(&other, T0 + 5);
    assert_eq!(
        Engine::open(&dir).beliefs().get(&other).unwrap().standing,
        Standing::Forgotten { at_ms: T0 + 5 }
    );

    e.forget_all().unwrap();
    assert_eq!(store::disk_used(&dir), 0);
    assert!(store::read_signals(&dir).is_empty());
    assert!(e.beliefs().is_empty());
    e.flush(T0 + 6);
    assert_eq!(store::disk_used(&dir), 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_files_stay_inside_the_disk_budget() {
    let dir = temp("budget");
    let mut e = Engine::open(&dir);
    // The worst case: every subject as long as one may be, enough signals to
    // rotate the log three times and more subjects than the store keeps.
    let pad = "x".repeat(SUBJECT_MAX - "model:".len() - 6);
    let total = 15_000;
    for i in 0..total {
        let key = subject(&format!("model:{pad}{:06}", i % (BELIEF_CAP + 500)));
        let at = T0 + i as u64;
        e.record(Signal::measured(at, key, 123_456.79));
        if (i + 1) % FLUSH_AT == 0 {
            e.tick(at);
        }
    }
    e.flush(T0 + total as u64);
    assert_eq!(e.dropped(), 0);
    assert_eq!(e.beliefs().len(), BELIEF_CAP);

    let size = |name: &str| fs::metadata(store::dir(&dir).join(name)).unwrap().len();
    // A file rotates at the first write after it reaches the cap, so it can
    // pass the cap by one batch at most.
    let batch = (FLUSH_AT * 120) as u64;
    assert!(size(store::SIGNALS) < SIGNAL_BYTES + batch);
    assert!(size(store::SIGNALS_OLD) >= SIGNAL_BYTES);
    assert!(size(store::SIGNALS_OLD) < SIGNAL_BYTES + batch);
    // The newest signal is on disk and the oldest has rotated out.
    let kept = store::read_signals(&dir);
    assert_eq!(kept.last().unwrap().at_ms, T0 + total as u64 - 1);
    assert!(kept.first().unwrap().at_ms > T0);
    assert!(kept.len() < total);

    let used = store::disk_used(&dir);
    assert!(used > SIGNAL_BYTES);
    assert!(used <= DISK_BUDGET, "{used} bytes is over the budget");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn two_apps_on_one_folder_keep_what_each_learned() {
    let dir = temp("two-apps");
    let key = subject("chip:shared");
    let mut cabin = Engine::open(&dir);
    let mut shell = Engine::open(&dir);
    for i in 0..3 {
        cabin.record(Signal::new(T0 + i, Kind::Picked, key.clone()));
    }
    for i in 0..2 {
        shell.record(Signal::new(T0 + 10 + i, Kind::Ignored, key.clone()));
    }
    cabin.flush(T0 + 20);
    shell.flush(T0 + 21);
    // The later save added its own two to the three already there.
    let merged = Shape::Rate { yes: 3.0, no: 2.0 };
    assert_eq!(shell.beliefs().get(&key).unwrap().shape, merged);
    assert_eq!(
        Engine::open(&dir).beliefs().get(&key).unwrap().shape,
        merged
    );
    assert_eq!(store::read_signals(&dir).len(), 5);

    // The other app sees it at its own next save, without counting twice.
    cabin.record(Signal::new(T0 + 30, Kind::Picked, key.clone()));
    cabin.flush(T0 + 31);
    assert_eq!(
        cabin.beliefs().get(&key).unwrap().shape,
        Shape::Rate { yes: 4.0, no: 2.0 }
    );

    // "That's wrong" in one app holds in the other: its unsaved votes are
    // replayed onto the locked line and change nothing.
    shell.record(Signal::new(T0 + 40, Kind::Picked, key.clone()));
    cabin.mark_wrong(&key, T0 + 41);
    shell.flush(T0 + 42);
    assert_eq!(shell.beliefs().get(&key).unwrap().standing, Standing::Wrong);
    assert_eq!(
        Engine::open(&dir).beliefs().get(&key).unwrap().standing,
        Standing::Wrong
    );

    // Forget everything in one app: the other keeps nothing it had saved.
    let other = subject("chip:other");
    shell.record(Signal::new(T0 + 50, Kind::Picked, other.clone()));
    shell.flush(T0 + 51);
    cabin.forget_all().unwrap();
    shell.record(Signal::new(T0 + 60, Kind::Picked, subject("chip:new")));
    shell.flush(T0 + 61);
    assert!(shell.beliefs().get(&other).is_none());
    assert!(shell.beliefs().get(&key).is_none());
    assert_eq!(shell.beliefs().len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn many_signals_between_ticks_bring_the_belief_save_forward() {
    let dir = temp("save-forward");
    let mut e = Engine::open(&dir);
    e.record(Signal::new(T0, Kind::Picked, subject("chip:a")));
    assert!(e.tick(T0));
    assert_eq!(e.writes(), 2);
    for i in 0..UNSAVED_MAX as u64 {
        e.record(Signal::new(T0 + 1 + i, Kind::Picked, subject("chip:a")));
    }
    // Well inside the five minutes, but the replay list is full.
    assert!(e.tick(T0 + 2_000));
    assert_eq!(e.writes(), 4);
    close(
        Engine::open(&dir)
            .beliefs()
            .get(&subject("chip:a"))
            .unwrap()
            .evidence(T0 + 2_000),
        1.0 + UNSAVED_MAX as f32,
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_unreadable_belief_file_starts_empty() {
    let dir = temp("broken");
    fs::create_dir_all(store::dir(&dir)).unwrap();
    fs::write(store::dir(&dir).join(store::BELIEFS), "{not json").unwrap();
    assert!(Engine::open(&dir).beliefs().is_empty());
    // A belief keyed by a sentence is refused whole, not half-read.
    let text = r#"{"my name is Alex":{"shape":{"rate":{"yes":1.0,"no":0.0}},"first_ms":1,"last_ms":1,"source":"x","standing":"live"}}"#;
    fs::write(store::dir(&dir).join(store::BELIEFS), text).unwrap();
    assert!(Engine::open(&dir).beliefs().is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn chip_rows_become_signals_for_the_chip_and_its_surface() {
    use super::sources;
    let subjects = |v: &[Signal]| -> Vec<(Kind, String)> {
        v.iter()
            .map(|s| (s.kind, s.subject.as_str().to_string()))
            .collect()
    };
    assert_eq!(
        subjects(&sources::chips_shown("composer", ["3fa9c1", "77aa01"], T0)),
        vec![
            (Kind::Shown, "chips:composer".to_string()),
            (Kind::Shown, "chip:3fa9c1".to_string()),
            (Kind::Shown, "chip:77aa01".to_string()),
        ]
    );
    assert_eq!(
        subjects(&sources::chips_passed_over("home", ["3fa9c1"], T0)),
        vec![
            (Kind::Ignored, "chips:home".to_string()),
            (Kind::Ignored, "chip:3fa9c1".to_string()),
        ]
    );
    assert_eq!(
        subjects(&sources::chip_outcome("composer", "3fa9c1", true, T0)),
        vec![
            (Kind::Picked, "chips:composer".to_string()),
            (Kind::Picked, "chip:3fa9c1".to_string()),
        ]
    );
    // Hiding one chip is not a verdict on chips there.
    assert_eq!(
        subjects(&sources::chip_outcome("composer", "3fa9c1", false, T0)),
        vec![(Kind::Hidden, "chip:3fa9c1".to_string())]
    );
    // A key that isn't an id never becomes a subject.
    assert_eq!(
        subjects(&sources::chips_shown("composer", ["two words"], T0)),
        vec![(Kind::Shown, "chips:composer".to_string())]
    );
}

#[test]
fn card_events_become_signals_and_a_read_card_closed_is_not_a_no() {
    use super::sources;
    use crate::card_signals::{signal_for, CardEvent};
    use crate::update_feed::automation_done_card;
    let card = automation_done_card("nightly", "ZephyrTitle9f3a", "ZephyrBody9f3a", 1);
    let of = |event, after| {
        sources::card(&signal_for(&card, event, T0, after))
            .into_iter()
            .map(|s| (s.kind, s.subject.as_str().to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        of(CardEvent::More, None),
        vec![
            (Kind::Liked, "card:automation_done".to_string()),
            (Kind::Liked, "card-group:run:nightly".to_string()),
        ]
    );
    assert_eq!(of(CardEvent::Less, None)[0].0, Kind::Disliked);
    assert_eq!(of(CardEvent::Ran, None)[0].0, Kind::Accepted);
    assert_eq!(of(CardEvent::Rejected, None)[0].0, Kind::Declined);
    assert_eq!(of(CardEvent::Dismissed, Some(false))[0].0, Kind::Dismissed);
    assert_eq!(of(CardEvent::Dismissed, Some(true)), vec![]);
    // The card's words are nowhere in what is recorded.
    let opened = sources::card(&signal_for(&card, CardEvent::Opened, T0, None));
    let line = serde_json::to_string(&opened).unwrap();
    assert!(!line.contains("Zephyr"), "{line}");
}

#[test]
fn a_run_counts_for_its_model_and_skill_and_a_late_line_counts_against() {
    use super::sources;
    use crate::outcome::OutcomeResult;
    use crate::run_metrics::RunMetrics;
    let run = RunMetrics {
        run: "chat-a:3".into(),
        class: "skill:weekly-report".into(),
        result: OutcomeResult::Success,
        wall_ms: 4_200,
        cost_usd: 0.25,
        model: "grok-code".into(),
        finished_at: T0,
        at: T0,
        ..RunMetrics::default()
    };
    let of = |m: &RunMetrics| {
        sources::run(m)
            .into_iter()
            .map(|s| (s.kind, s.subject.as_str().to_string(), s.value))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        of(&run),
        vec![
            (Kind::Succeeded, "model:grok-code".to_string(), 1.0),
            (Kind::Succeeded, "skill:weekly-report".to_string(), 1.0),
            (
                Kind::Measured,
                "model:grok-code/run-ms".to_string(),
                4_200.0
            ),
            (Kind::Measured, "model:grok-code/run-usd".to_string(), 0.25),
        ]
    );
    // The correction is a new line for the same run: one vote against, and
    // the run's time and cost are not measured twice.
    assert_eq!(
        of(&run.corrected(T0 + 5)),
        vec![
            (Kind::Corrected, "model:grok-code".to_string(), 1.0),
            (Kind::Corrected, "skill:weekly-report".to_string(), 1.0),
        ]
    );
    assert_eq!(of(&run.undone(T0 + 9))[0].0, Kind::Undone);
    // A topic class is made of the user's words: it never becomes a subject.
    let topic = RunMetrics {
        class: "topic:my private plans".into(),
        result: OutcomeResult::Failure,
        ..run.clone()
    };
    assert_eq!(
        of(&topic)[0],
        (Kind::Failed, "model:grok-code".to_string(), 1.0)
    );
    assert!(of(&topic).iter().all(|(_, s, _)| !s.contains("private")));
    // A run still inside its undo window votes nothing yet.
    let pending = RunMetrics {
        result: OutcomeResult::Pending,
        model: String::new(),
        ..run
    };
    assert_eq!(of(&pending), vec![]);
}

#[test]
fn the_learned_view_says_each_belief_in_plain_words() {
    use super::view::{view, Stage};
    let mut beliefs = Beliefs::default();
    votes(&mut beliefs, &subject("card:suggestion"), 8, 2, T0);
    votes(&mut beliefs, &subject("skill:weekly-report"), 30, 0, T0);
    votes(&mut beliefs, &subject("chips:home"), 1, 3, T0);
    votes(&mut beliefs, &subject("chip:0a1b2c3d4e5f"), 2, 0, T0);
    beliefs.measure(&subject("model:grok-code/run-ms"), 3_000.0, T0, "test");
    beliefs.mark_wrong(&subject("card:news"), T0);
    votes(&mut beliefs, &subject("card:old"), 4, 0, T0);
    beliefs.forget(&subject("card:old"), T0);

    let got = view(&beliefs, T0);
    let keys: Vec<&str> = got.rows.iter().map(|r| r.key.as_str()).collect();
    // Surest first, the user's "wrong" last, the forgotten one and the
    // hashed chip not listed.
    assert_eq!(
        keys,
        [
            "skill:weekly-report",
            "card:suggestion",
            "chips:home",
            "model:grok-code/run-ms",
            "card:news"
        ]
    );
    assert_eq!(got.single_chips, 1);

    let skill = &got.rows[0];
    assert_eq!(skill.title, "Skill weekly-report");
    assert_eq!(skill.says, "Runs go well about 10 times in 10.");
    assert_eq!(
        (skill.evidence, skill.sure, skill.stage),
        (30, 92, Stage::Acting)
    );

    let card = &got.rows[1];
    assert_eq!(card.title, "Suggestion cards");
    assert_eq!(card.says, "You take these about 8 times in 10.");
    assert_eq!(
        (card.evidence, card.sure, card.stage),
        (10, 54, Stage::Suggesting)
    );

    let speed = &got.rows[3];
    assert_eq!(speed.title, "How long grok-code takes");
    assert_eq!(speed.says, "A run takes about 3.0 seconds.");
    assert_eq!(speed.stage, Stage::Measuring);

    let chips = &got.rows[2];
    assert_eq!(chips.title, "Quick chips on home");
    assert_eq!(
        chips.says,
        "You tap one about 3 times in 10 when they show."
    );
    assert_eq!(chips.stage, Stage::Watching);

    let wrong = &got.rows[4];
    assert_eq!(
        wrong.says,
        "You said this is wrong. It is not counted or used."
    );
    assert_eq!(
        (wrong.evidence, wrong.sure, wrong.stage),
        (0, 0, Stage::Wrong)
    );
}

#[test]
fn a_belief_that_faded_away_has_no_line() {
    use super::view::view;
    let mut beliefs = Beliefs::default();
    votes(&mut beliefs, &subject("card:suggestion"), 2, 0, T0);
    assert_eq!(view(&beliefs, T0).rows.len(), 1);
    // Two half-lives later 2 sightings count as 0.5, three later as 0.25.
    assert_eq!(view(&beliefs, T0 + 2 * HALF_LIFE_MS).rows.len(), 1);
    assert_eq!(view(&beliefs, T0 + 3 * HALF_LIFE_MS).rows.len(), 0);
}

#[test]
fn the_learned_text_lists_ids_and_how_to_change_them() {
    use super::view::{text, view, HOW_TO_CHANGE, NOTHING_YET};
    assert_eq!(text(&view(&Beliefs::default(), T0)), NOTHING_YET);
    let mut beliefs = Beliefs::default();
    votes(&mut beliefs, &subject("card:suggestion"), 8, 2, T0);
    votes(&mut beliefs, &subject("chip:0a1b2c3d4e5f"), 2, 0, T0);
    beliefs.mark_wrong(&subject("card:news"), T0);
    assert_eq!(
        text(&view(&beliefs, T0)),
        format!(
            "**What I've learned**\n\
             - **Suggestion cards**: You take these about 8 times in 10. Seen 10 times, 54% sure, sure enough to suggest. `card:suggestion`\n\
             - **News cards**: You said this is wrong. It is not counted or used. `card:news`\n\
             - Counts for 1 single quick chips, which rank the chip rows.\n\n{HOW_TO_CHANGE}"
        )
    );
}

#[test]
fn a_learned_command_names_one_belief_or_all() {
    use super::view::{Command, USAGE};
    assert_eq!(Command::parse(""), Ok(Command::List));
    assert_eq!(Command::parse("  "), Ok(Command::List));
    assert_eq!(
        Command::parse("forget card:suggestion"),
        Ok(Command::Forget(subject("card:suggestion")))
    );
    assert_eq!(
        Command::parse("WRONG model:grok-code/run-ms"),
        Ok(Command::Wrong(subject("model:grok-code/run-ms")))
    );
    assert_eq!(Command::parse("forget All"), Ok(Command::ForgetAll));
    for bad in [
        "forget",
        "wrong",
        "forget suggestion",
        "wrong all",
        "list",
        "forget a:b c:d",
    ] {
        assert_eq!(Command::parse(bad), Err(USAGE.to_string()), "{bad}");
    }
}

#[test]
fn a_plan_reply_pick_counts_for_it_and_against_the_replies_passed_over() {
    use super::sources::{reply_picked, reply_subject, PLAN_REPLIES};
    let got: Vec<(Kind, String)> = reply_picked("plan:change", T0)
        .into_iter()
        .map(|s| (s.kind, s.subject.as_str().to_string()))
        .collect();
    assert_eq!(
        got,
        [
            (Kind::Ignored, "reply:plan/go".to_string()),
            (Kind::Ignored, "reply:plan/more".to_string()),
            (Kind::Ignored, "reply:plan/go-but".to_string()),
            (Kind::Ignored, "reply:plan/not-now".to_string()),
            (Kind::Picked, "reply:plan/change".to_string()),
        ]
    );
    // "More on step N" is not on every card, so picking it passes over the
    // five that are, and nothing ever counts against it.
    assert_eq!(reply_picked("plan:step", T0).len(), 6);
    // A question's option is the agent's words: it never becomes a subject.
    assert_eq!(reply_subject("ask:deploy to prod now"), None);
    assert_eq!(reply_picked("ask:deploy to prod now", T0), Vec::new());
    // Every reply counted as passed over is one a plan card always shows.
    let shown = crate::reply_chips::plan_reply_chips("Just do it.", &crate::ChipMemory::default());
    for id in PLAN_REPLIES {
        assert!(shown.iter().any(|c| c.key == format!("plan:{id}")), "{id}");
    }
}

#[test]
fn the_learned_view_names_a_plan_reply_by_its_chip() {
    use super::view::view;
    let mut beliefs = Beliefs::default();
    votes(&mut beliefs, &subject("reply:plan/go-but"), 3, 1, T0);
    let got = view(&beliefs, T0);
    assert_eq!(got.rows[0].title, "Plan reply: Go, but\u{2026}");
    assert_eq!(
        got.rows[0].says,
        "You pick this about 8 times in 10 when a plan waits."
    );
}

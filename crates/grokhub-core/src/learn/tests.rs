use std::fs;
use std::path::PathBuf;

use super::belief::{BELIEF_CAP, HALF_LIFE_MS};
use super::signal::{FLUSH_AT, FLUSH_MS, RING, SUBJECT_MAX};
use super::store::{self, DISK_BUDGET, SIGNAL_CAP};
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
    assert!(!store::dir(&dir).exists());
    assert!(e.beliefs().is_empty());
    e.flush(T0 + 6);
    assert!(!store::dir(&dir).exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_files_stay_inside_the_disk_budget() {
    let dir = temp("budget");
    let mut e = Engine::open(&dir);
    // The worst case: every subject as long as one may be, more signals than
    // both files hold and more subjects than the belief store keeps.
    let pad = "x".repeat(SUBJECT_MAX - "model:".len() - 6);
    let total = SIGNAL_CAP * 2 + SIGNAL_CAP / 2;
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

    let active = fs::read_to_string(store::dir(&dir).join(store::SIGNALS)).unwrap();
    let old = fs::read_to_string(store::dir(&dir).join(store::SIGNALS_OLD)).unwrap();
    assert!(active.lines().count() <= SIGNAL_CAP);
    assert!(old.lines().count() <= SIGNAL_CAP);
    assert_eq!(
        store::read_signals(&dir).len(),
        active.lines().count() + old.lines().count()
    );
    // The newest signal is on disk and the oldest has rotated out.
    assert!(active.contains(&format!("\"t\":{}", T0 + total as u64 - 1)));
    assert!(!old.contains(&format!("\"t\":{T0},")));

    let used = store::disk_used(&dir);
    assert!(used > 0);
    assert!(used <= DISK_BUDGET, "{used} bytes is over the budget");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_restart_keeps_counting_lines_from_the_file() {
    let dir = temp("restart");
    let mut log = store::Log::new(&dir);
    let batch: Vec<Signal> = (0..SIGNAL_CAP as u64 - 1)
        .map(|i| Signal::new(T0 + i, Kind::Used, subject("page:home")))
        .collect();
    log.append(&batch).unwrap();
    // A new process has no count in memory: it reads it once from the file.
    let mut log = store::Log::new(&dir);
    let two: Vec<Signal> = (0..2)
        .map(|i| Signal::new(T0 + 9_000 + i, Kind::Used, subject("page:pulse")))
        .collect();
    log.append(&two).unwrap();
    let active = fs::read_to_string(store::dir(&dir).join(store::SIGNALS)).unwrap();
    let old = fs::read_to_string(store::dir(&dir).join(store::SIGNALS_OLD)).unwrap();
    assert_eq!(active.lines().count(), 2);
    assert_eq!(old.lines().count(), SIGNAL_CAP - 1);
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

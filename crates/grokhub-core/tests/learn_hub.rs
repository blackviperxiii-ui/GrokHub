//! The learning engine as a running app uses it: started once for the
//! process, fed by the shared chip, card and run logs, written by `flush`.
//! Its own test binary, because the hub is one per process.

use std::fs;

use grokhub_core::card_signals::{append_signal, signal_for, CardEvent};
use grokhub_core::chips::{ChipKind, QuickChip};
use grokhub_core::learn::{hub, store, Kind, Shape, Subject};
use grokhub_core::outcome::OutcomeResult;
use grokhub_core::run_metrics::{append_run, RunMetrics};
use grokhub_core::update_feed::automation_done_card;
use grokhub_core::{chip_log_key, log_chip_outcome, log_chips_shown, ChipMemory};

fn chip(id: &str) -> QuickChip {
    QuickChip {
        id: id.into(),
        label: id.into(),
        value: format!("/{id}"),
        kind: ChipKind::Chat,
        score: 1.0,
        hint: String::new(),
        primary: false,
    }
}

#[test]
fn the_shared_logs_feed_one_engine() {
    let dir = std::env::temp_dir().join(format!("grokhub-learn-hub-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let t0 = 1_800_000_000_000u64;

    // Before an app starts the engine, the logs work as before and nothing
    // is recorded anywhere.
    let mut mem = ChipMemory::default();
    let (brief, patch) = (chip("brief"), chip("patch-notes"));
    log_chips_shown(&mut mem, "composer", &[brief.clone(), patch.clone()], t0);
    assert!(!hub::started());
    assert!(!store::dir(&dir).exists());

    hub::start(&dir);
    hub::start(&dir);
    assert!(hub::started());

    // Chips: a row passed over, a second row with a pick, a hide on Home.
    let mut mem = ChipMemory::default();
    log_chips_shown(&mut mem, "composer", &[brief.clone(), patch.clone()], t0);
    // The same row again is the same showing: nothing new.
    log_chips_shown(
        &mut mem,
        "composer",
        &[brief.clone(), patch.clone()],
        t0 + 1,
    );
    log_chips_shown(&mut mem, "composer", std::slice::from_ref(&patch), t0 + 2);
    log_chip_outcome(&mut mem, "composer", &patch, true, t0 + 3);
    log_chips_shown(&mut mem, "home", std::slice::from_ref(&brief), t0 + 4);
    log_chip_outcome(&mut mem, "home", &brief, false, t0 + 5);

    // Cards and a run.
    let card = automation_done_card("nightly", "Name", "body", 1);
    append_signal(&dir, &signal_for(&card, CardEvent::More, t0 + 6, None));
    let run = RunMetrics {
        run: "chat-a:1".into(),
        class: "skill:weekly-report".into(),
        result: OutcomeResult::Success,
        wall_ms: 3_000,
        model: "grok-code".into(),
        finished_at: t0 + 7,
        at: t0 + 7,
        ..RunMetrics::default()
    };
    append_run(&dir, &run, 0).unwrap();

    // Nothing of the engine's is on disk until a write is due.
    assert_eq!(store::disk_used(&dir), 0);
    hub::flush();

    let key = |c: &QuickChip| Subject::new("chip", &chip_log_key(c)).unwrap();
    let subject = |s: &str| Subject::parse(s).unwrap();
    let shape = |s: &Subject| {
        hub::read(|e| e.beliefs().get(s).map(|b| b.shape))
            .unwrap()
            .unwrap()
    };
    // patch-notes: passed over once, then picked. brief: passed over, then hidden.
    assert_eq!(shape(&key(&patch)), Shape::Rate { yes: 1.0, no: 1.0 });
    assert_eq!(shape(&key(&brief)), Shape::Rate { yes: 0.0, no: 2.0 });
    assert_eq!(
        shape(&subject("chips:composer")),
        Shape::Rate { yes: 1.0, no: 1.0 }
    );
    assert_eq!(
        shape(&subject("card:automation_done")),
        Shape::Rate { yes: 1.0, no: 0.0 }
    );
    assert_eq!(
        shape(&subject("model:grok-code")),
        Shape::Rate { yes: 1.0, no: 0.0 }
    );
    assert_eq!(
        shape(&subject("skill:weekly-report")),
        Shape::Rate { yes: 1.0, no: 0.0 }
    );
    assert_eq!(
        shape(&subject("model:grok-code/run-ms")),
        Shape::Level {
            mean: 3_000.0,
            n: 1.0
        }
    );
    // A hide on Home voted for the chip only, so Home's chips have no belief.
    assert!(hub::read(|e| e.beliefs().get(&subject("chips:home")).is_none()).unwrap());

    // The stream on disk: every signal, in order, ids only.
    let lines = store::read_signals(&dir);
    let shown = lines.iter().filter(|s| s.kind == Kind::Shown).count();
    // Three rows went up: 1 surface + 2 chips, 1 + 1, 1 + 1.
    assert_eq!(shown, 7);
    assert_eq!(lines.first().unwrap().subject.as_str(), "chips:composer");
    assert_eq!(
        lines.last().unwrap().subject.as_str(),
        "model:grok-code/run-usd"
    );
    let text = fs::read_to_string(store::dir(&dir).join(store::SIGNALS)).unwrap();
    for word in ["brief", "patch-notes", "Name", "body"] {
        assert!(!text.contains(word), "{word} leaked into {text}");
    }
    // A new app on the same folder starts from what was saved.
    assert_eq!(
        store::load_beliefs(&dir)
            .get(&subject("skill:weekly-report"))
            .unwrap()
            .shape,
        Shape::Rate { yes: 1.0, no: 0.0 }
    );
    let _ = fs::remove_dir_all(&dir);
}

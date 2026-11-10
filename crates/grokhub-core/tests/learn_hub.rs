//! The learning engine as a running app uses it: started once for the
//! process, fed by the shared chip, card and run logs, written by `flush`.
//! Its own test binary, because the hub is one per process.

use std::fs;

use grokhub_core::card_signals::{append_signal, signal_for, CardEvent};
use grokhub_core::chips::{ChipKind, QuickChip};
use grokhub_core::learn::{hub, store, Kind, Shape, Standing, Subject};
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

    // A run from yesterday, from before this folder had an engine.
    let day = 24 * 60 * 60 * 1_000u64;
    let yesterday = grokhub_core::now_ms() - day;
    let old = RunMetrics {
        run: "chat-old:1".into(),
        class: "skill:old-habit".into(),
        result: OutcomeResult::Success,
        model: "old-model".into(),
        finished_at: yesterday,
        at: yesterday,
        ..RunMetrics::default()
    };
    append_run(&dir, &old, 0).unwrap();
    assert!(!store::dir(&dir).exists());

    hub::start(&dir);
    hub::start(&dir);
    assert!(hub::started());

    // The head start: once the app gives its clock, the old run is learned
    // from as if the engine had seen it, in its own hour. Forget everything
    // then clears it like anything else learned, and it is not read again.
    assert_eq!(store::disk_used(&dir), 0);
    hub::set_utc_offset_min(0);
    let old_skill = Subject::new("skill", "old-habit").unwrap();
    let waited = std::time::Instant::now();
    while !store::load_beliefs(&dir).get(&old_skill).is_some() {
        assert!(waited.elapsed().as_secs() < 20, "no head start");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let learned = store::load_beliefs(&dir);
    assert_eq!(
        learned.get(&old_skill).unwrap().shape,
        Shape::Rate { yes: 1.0, no: 0.0 }
    );
    let old_model = Subject::new("model", "old-model").unwrap();
    assert_eq!(
        learned.get(&old_model).unwrap().shape,
        Shape::Rate { yes: 1.0, no: 0.0 }
    );
    let part = grokhub_core::learn::habit::Part::of_hour(hub::local_hour(yesterday));
    let old_habit = Subject::new("habit", &format!("old-habit/{}", part.as_str())).unwrap();
    assert_eq!(
        learned.get(&old_habit).unwrap().shape,
        Shape::Rate { yes: 1.0, no: 0.0 }
    );
    hub::forget_all().unwrap();
    assert_eq!(store::disk_used(&dir), 0);
    hub::set_utc_offset_min(0);
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(store::disk_used(&dir), 0);

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
    // The run's last four lines are its habit votes: `t0` is 08:00 UTC, so
    // one for the morning and one against each other part of the day.
    let tail: Vec<(&str, Kind)> = lines[lines.len() - 5..]
        .iter()
        .map(|s| (s.subject.as_str(), s.kind))
        .collect();
    assert_eq!(
        tail,
        vec![
            ("model:grok-code/run-usd", Kind::Measured),
            ("habit:weekly-report/morning", Kind::Used),
            ("habit:weekly-report/afternoon", Kind::Ignored),
            ("habit:weekly-report/evening", Kind::Ignored),
            ("habit:weekly-report/night", Kind::Ignored),
        ]
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

    // `/learned` reads the same engine, and the user's word is saved at once.
    use grokhub_core::learn::view;
    let listed = view::run("", grokhub_core::now_ms());
    assert!(
        listed.contains("- **Skill weekly-report**: Runs go well about 10 times in 10."),
        "{listed}"
    );
    assert_eq!(
        view::run("wrong skill:weekly-report", 0),
        "Marked `skill:weekly-report` wrong. It won't be counted or used again."
    );
    assert_eq!(
        store::load_beliefs(&dir)
            .get(&subject("skill:weekly-report"))
            .unwrap()
            .standing,
        Standing::Wrong
    );
    // Plan replies: a pick is counted by the engine, the engine orders the
    // row, and Forget there puts the row back even though the old pick
    // count is still saved.
    use grokhub_core::reply_chips::{plan_reply_chips, remember_reply_pick};
    let lead = |mem: &ChipMemory| plan_reply_chips("Just do it.", mem)[0].key.clone();
    assert_eq!(lead(&mem), "plan:go");
    remember_reply_pick(&mut mem, "plan:change", grokhub_core::now_ms());
    remember_reply_pick(&mut mem, "plan:change", grokhub_core::now_ms());
    assert_eq!(lead(&mem), "plan:change");
    // A person with no saved counts gets the same row: the engine decides.
    assert_eq!(lead(&ChipMemory::default()), "plan:change");
    assert_eq!(
        view::run("forget reply:plan/change", 0),
        "Forgot `reply:plan/change`. It starts fresh from here."
    );
    assert_eq!(mem.reply_picks["plan:change"], 2);
    assert_eq!(lead(&mem), "plan:go");

    // The desktop app's buttons run the same code and get the list back.
    let act: view::Act =
        serde_json::from_str(r#"{"kind":"forget","key":"card:automation_done"}"#).unwrap();
    let before = hub::view(grokhub_core::now_ms());
    assert!(
        before.rows.iter().any(|r| r.key == "card:automation_done"),
        "{before:?}"
    );
    let after = view::act(act, grokhub_core::now_ms()).unwrap();
    assert!(
        !after.rows.iter().any(|r| r.key == "card:automation_done"),
        "{after:?}"
    );
    assert_eq!(after.rows.len(), before.rows.len() - 1);
    assert_eq!(
        view::act(
            view::Act::Wrong {
                key: "two words".into()
            },
            0
        ),
        Err(view::NO_SUCH_LINE.to_string())
    );
    let json = serde_json::to_value(&after.rows[0]).unwrap();
    for field in [
        "key", "title", "says", "evidence", "sure", "stage", "lastMs",
    ] {
        assert!(json.get(field).is_some(), "{field} missing from {json}");
    }
    // The habit miner: twelve good runs of one skill at this time of day
    // become a chip, through the decision engine. A tap is a yes to its
    // rule; a hide is a no, and the chip rests.
    use grokhub_core::learn::decide::Situation as Now;
    use grokhub_core::learn::habit;
    hub::set_utc_offset_min(0);
    let live = grokhub_core::now_ms();
    let skills = vec![grokhub_core::SkillMd {
        name: "tidy-up".into(),
        slash: "/tidy-up".into(),
        ..Default::default()
    }];
    assert_eq!(
        hub::habit_chips(&skills, Now::default(), 99.0, live),
        Vec::new()
    );
    for i in 0..12u64 {
        let run = RunMetrics {
            run: format!("chat-h:{i}"),
            class: "skill:tidy-up".into(),
            result: OutcomeResult::Success,
            model: "grok-code".into(),
            finished_at: live,
            at: live,
            ..RunMetrics::default()
        };
        append_run(&dir, &run, 0).unwrap();
    }
    let part = habit::Part::of_hour(hub::local_hour(live));
    let chips = hub::habit_chips(&skills, Now::default(), 99.0, live);
    assert_eq!(chips.len(), 1, "{chips:?}");
    assert_eq!(chips[0].id, format!("habit-{}-tidy-up", part.as_str()));
    assert_eq!(chips[0].value, "/tidy-up");
    assert_eq!(chips[0].hint, format!("You often run this {}", part.when()));
    // The skill was removed: no chip, and nothing is weighed.
    assert_eq!(
        hub::habit_chips(&[], Now::default(), 99.0, live),
        Vec::new()
    );
    // While a game runs, only habits learned while gaming are due, and a
    // run then counts under the gaming habit.
    hub::set_now(grokhub_core::learn::now::Snapshot {
        gaming: true,
        ..Default::default()
    });
    assert_eq!(
        hub::habit_chips(&skills, Now::default(), 99.0, live),
        Vec::new()
    );
    let gaming_run = RunMetrics {
        run: "chat-g:0".into(),
        class: "skill:tidy-up".into(),
        result: OutcomeResult::Success,
        model: "grok-code".into(),
        finished_at: live,
        at: live,
        ..RunMetrics::default()
    };
    append_run(&dir, &gaming_run, 0).unwrap();
    let while_gaming = habit::subject("tidy-up", part, true).unwrap();
    assert_eq!(
        hub::read(|e| e.beliefs().get(&while_gaming).map(|b| b.shape)).flatten(),
        Some(Shape::Rate { yes: 1.0, no: 0.0 })
    );
    hub::set_now(Default::default());
    let rule = habit::chip_rule(&chips[0].id).unwrap();
    let state = || grokhub_core::learn::decide::load(&dir);
    let mut habit_mem = ChipMemory::default();
    for i in 0..3u64 {
        log_chips_shown(&mut habit_mem, "composer", &chips, live + 10 * i);
        log_chip_outcome(
            &mut habit_mem,
            "composer",
            &chips[0],
            true,
            live + 10 * i + 1,
        );
    }
    assert!(state().graduated(&rule), "three taps are three yes answers");
    assert_eq!(
        hub::habit_chips(&skills, Now::default(), 99.0, live + 40).len(),
        1
    );
    log_chips_shown(&mut habit_mem, "composer", &chips, live + 50);
    log_chip_outcome(&mut habit_mem, "composer", &chips[0], false, live + 51);
    assert!(!state().graduated(&rule));
    assert_eq!(
        hub::habit_chips(&skills, Now::default(), 99.0, live + 60),
        Vec::new(),
        "a hide rests the habit chip"
    );
    let why: Vec<_> = state()
        .log()
        .filter(|d| d.rule == rule)
        .map(|d| (d.verdict, d.reason))
        .collect();
    use grokhub_core::learn::decide::{Reason as Why, Verdict as Said};
    assert_eq!(
        why,
        vec![
            (Said::Offer, Why::Shadow),
            (Said::Act, Why::Sure),
            (Said::Wait, Why::Resting),
        ]
    );
    let listed = view::run("", grokhub_core::now_ms());
    assert!(
        listed.contains(&format!(
            "**Skill tidy-up {}**: About 10 in 10 of its runs happen then.",
            part.when()
        )),
        "{listed}"
    );

    // Pause: nothing is counted, what was learned stays, and resume counts again.
    let probe = Subject::new("skill", "pause-probe").unwrap();
    let seen = |key: &Subject| hub::read(|e| e.beliefs().get(key).map(|b| b.shape)).flatten();
    assert_eq!(
        view::run("pause", 0),
        "Learning is paused. Nothing new is counted until `/learned resume`."
    );
    assert!(hub::paused());
    assert!(store::paused(&dir));
    hub::note(grokhub_core::learn::Signal::new(
        1,
        Kind::Succeeded,
        probe.clone(),
    ));
    hub::note_all([grokhub_core::learn::Signal::new(
        2,
        Kind::Succeeded,
        probe.clone(),
    )]);
    assert_eq!(seen(&probe), None);
    assert!(view::run("", grokhub_core::now_ms()).starts_with(view::PAUSED_NOTE));
    assert!(hub::view(0).paused);
    assert_eq!(view::run("resume", 0), "Learning is back on.");
    assert!(!store::paused(&dir));
    hub::note(grokhub_core::learn::Signal::new(
        3,
        Kind::Succeeded,
        probe.clone(),
    ));
    assert_eq!(seen(&probe), Some(Shape::Rate { yes: 1.0, no: 0.0 }));

    // "Always": the user's own rule, from the chat and from the desktop app.
    assert_eq!(
        view::run("always skill:pause-probe", 0),
        "`skill:pause-probe` is a rule now. Nothing counts against it until you forget it."
    );
    hub::note(grokhub_core::learn::Signal::new(
        4,
        Kind::Failed,
        probe.clone(),
    ));
    let pinned = hub::view(grokhub_core::now_ms());
    let line = pinned
        .rows
        .iter()
        .find(|r| r.key == "skill:pause-probe")
        .unwrap();
    assert_eq!((line.stage, line.sure), (view::Stage::Pinned, 100));
    assert_eq!(
        store::load_beliefs(&dir).get(&probe).map(|b| b.standing),
        Some(Standing::Pinned)
    );
    let pin: view::Act =
        serde_json::from_str(r#"{"kind":"always","key":"reply:plan/go"}"#).unwrap();
    assert!(view::act(pin, 0).is_ok());
    let cost = view::Act::Always {
        key: "model:grok-code/run-ms".into(),
    };
    assert_eq!(view::act(cost, 0), Err(view::CANT_PIN.to_string()));
    let toggled: view::Act = serde_json::from_str(r#"{"kind":"pause"}"#).unwrap();
    assert!(view::act(toggled, 0).unwrap().paused);
    assert!(!view::act(view::Act::Resume, 0).unwrap().paused);

    // The decision engine on the same hub: a new rule offers until it has
    // three yes answers, then acts, and each answer is also a signal.
    use grokhub_core::learn::decide::{self, Area, Outcome, Proposal, Reason, Situation, Verdict};
    use grokhub_core::learn::Undo;
    let rule = Subject::new("rule", "settings/text-size").unwrap();
    let proposal = Proposal {
        area: Area::Settings,
        rule: rule.clone(),
        confidence: 0.85,
        evidence: 14.0,
        undo: Undo::OneClick,
        value: 0.5,
        hard: false,
        helps: None,
        hurts: None,
        tested: false,
    };
    let at = grokhub_core::now_ms();
    let decide_one = || {
        let d = hub::decide_all(vec![proposal.clone()], Situation::default(), at);
        (d[0].verdict, d[0].reason)
    };
    assert_eq!(decide_one(), (Verdict::Offer, Reason::Shadow));
    for _ in 0..decide::SHADOW_PASSES {
        assert_eq!(hub::decision_outcome(&rule, Outcome::Accepted, at), None);
    }
    assert_eq!(decide_one(), (Verdict::Act, Reason::Sure));
    let learned = hub::read(|e| e.beliefs().get(&rule).map(|b| b.shape)).flatten();
    assert_eq!(learned, Some(Shape::Rate { yes: 3.0, no: 0.0 }));
    assert_eq!(
        view::run("trust settings offer", at),
        "Settings is set to offer only."
    );
    assert_eq!(decide_one(), (Verdict::Offer, Reason::DialOffer));
    assert_eq!(view::run("trust settings maybe", at), decide::TRUST_USAGE);
    assert_eq!(view::run("trust settings act now", at), view::USAGE);
    // Goals: set in the cabin or by the page's button, kept in order.
    assert_eq!(
        view::run("goals fastest cheapest", at),
        "What matters most, in order: Fastest answers, Lowest cost."
    );
    assert_eq!(view::run("goals", at), decide::GOALS_USAGE);
    assert_eq!(view::run("goals loudest", at), decide::GOALS_USAGE);
    let goals: view::Act =
        serde_json::from_str(r#"{"kind":"goals","goals":["quiet","fastest"]}"#).unwrap();
    let named: Vec<&str> = view::act(goals, at)
        .unwrap()
        .trust
        .goals
        .iter()
        .map(|g| g.label)
        .collect();
    assert_eq!(named, vec!["Fewest interruptions", "Fastest answers"]);
    assert_eq!(view::run("goals none", at), "No goals set.");
    assert!(hub::view(at).trust.goals.is_empty());
    // The app's picture of right now reaches the page.
    hub::set_now(grokhub_core::learn::now::Snapshot {
        gaming: true,
        ..Default::default()
    });
    assert!(hub::view(at).trust.now.gaming);
    assert!(hub::now().situation().busy);
    hub::set_now(Default::default());

    let shown = view::run("decisions", at);
    assert!(shown.contains("- Settings: offer `settings`\n"), "{shown}");
    assert!(
        shown.contains("- Did it: It was sure, you said yes to this before, and it can be undone. `rule:settings/text-size`\n"),
        "{shown}"
    );
    // The desktop page reads the same dials and log, and sets a dial with
    // a button.
    let dial: view::Act =
        serde_json::from_str(r#"{"kind":"trust","area":"local-ai","dial":"watch"}"#).unwrap();
    let page = view::act(dial, at).unwrap().trust;
    assert_eq!(
        page.dials[3],
        view::DialRow {
            area: Area::LocalAi,
            label: "Local AI",
            dial: decide::Dial::Watch,
            went: None,
            before: None,
            trend: grokhub_core::learn::score::Trend::TooEarly,
            resting: false,
        }
    );
    assert_eq!(page.dials[0].dial, decide::Dial::Offer);
    assert_eq!((page.asking_first, page.left_today), (false, 3));
    // The habit is named for the part of the day the test runs in.
    let tidy = format!("Skill tidy-up {}", habit::when(part, false));
    let lines: Vec<(&str, &str)> = page
        .decisions
        .iter()
        .map(|d| (d.title.as_str(), d.did))
        .collect();
    assert_eq!(
        lines,
        vec![
            ("Settings text size", "Asked"),
            ("Settings text size", "Did it"),
            ("Settings text size", "Asked"),
            (tidy.as_str(), "Held back"),
            (tidy.as_str(), "Did it"),
            (tidy.as_str(), "Asked"),
        ]
    );
    assert_eq!(page.decisions[0].why, "This area is set to offer only.");
    // Two undos in a day make everything ask first until Carry on.
    assert_eq!(hub::decision_outcome(&rule, Outcome::Undone, at), None);
    assert!(hub::decision_outcome(&rule, Outcome::Undone, at).is_some());
    assert!(hub::view(at).trust.asking_first);
    let carry_on: view::Act = serde_json::from_str(r#"{"kind":"carryOn"}"#).unwrap();
    assert!(!view::act(carry_on, at).unwrap().trust.asking_first);
    assert!(store::disk_used(&dir) < store::DISK_BUDGET);

    assert_eq!(
        view::run("forget all", 0),
        "Forgot everything the app learned."
    );
    // Forget everything clears the rule's passes and the log. The dial is
    // the user's own setting and stays.
    let kept = decide::load(&dir);
    assert_eq!(kept.dial(Area::Settings), decide::Dial::Offer);
    assert!(!kept.graduated(&rule));
    assert_eq!(kept.log().count(), 0);
    assert!(store::load_beliefs(&dir).is_empty());
    assert_eq!(store::read_signals(&dir), Vec::new());
    assert_eq!(view::run("", grokhub_core::now_ms()), view::NOTHING_YET);

    // Auto setup's plan goes through the same engine. Settings is still on
    // offer, so a change it would make on its own becomes a suggestion.
    use grokhub_core::autotune::plan::{self, Change, Moment, Plan};
    use grokhub_core::autotune::settings::{Key, Value};
    use grokhub_core::autotune::store::AutoStore;
    let change = Change {
        key: Key::ChatObjectives,
        from: Some(Value::Bool(false)),
        to: Value::Bool(true),
        rule: "wide_screen",
        asked: false,
        reason: "wide".into(),
    };
    let planned = Plan {
        apply: vec![change.clone()],
        ..Plan::default()
    };
    let weighed = hub::weigh_setup(planned.clone(), Moment::Launch, at);
    assert_eq!(
        (weighed.apply, weighed.suggest),
        (vec![], vec![change.clone()])
    );
    // Yes to the suggestion, then the dial back to act: the next one is made.
    let mut auto = AutoStore::default();
    plan::accept(&mut auto, &change, at);
    hub::set_dial(Area::Settings, decide::Dial::Act);
    assert_eq!(
        hub::weigh_setup(planned.clone(), Moment::Launch, at),
        planned
    );
    // The app made it and the user took it back: one good and one bad
    // outcome for Settings, and both decisions on the page.
    let mut auto = AutoStore::default();
    plan::apply(&mut auto, std::slice::from_ref(&change), Moment::Launch, at);
    assert!(auto.undo(Key::ChatObjectives, at).is_some());
    let page = hub::view(at).trust;
    assert_eq!(page.dials[0].went, Some(50));
    let lines: Vec<(&str, &str)> = page
        .decisions
        .iter()
        .map(|d| (d.title.as_str(), d.did))
        .collect();
    assert_eq!(
        lines,
        vec![
            ("Settings wide screen", "Did it"),
            ("Settings wide screen", "Asked"),
        ]
    );
    // A learner outside the core reads one dial straight from the hub.
    hub::set_dial(Area::LocalAi, decide::Dial::Watch);
    assert_eq!(hub::dial(Area::LocalAi), decide::Dial::Watch);
    hub::set_dial(Area::LocalAi, decide::Dial::Act);
    assert_eq!(hub::dial(Area::LocalAi), decide::Dial::Act);

    // An experiment: a skill run only twice around now is not a habit yet,
    // so the app tries it once as a chip that says so. Hiding it is a no.
    let mut both = skills.clone();
    both.push(grokhub_core::SkillMd {
        name: "try-me".into(),
        slash: "/try-me".into(),
        ..Default::default()
    });
    assert_eq!(
        hub::habit_chips(&both, Now::default(), 99.0, live),
        Vec::new()
    );
    for i in 0..2u64 {
        let run = RunMetrics {
            run: format!("chat-try:{i}"),
            class: "skill:try-me".into(),
            result: OutcomeResult::Success,
            model: "grok-code".into(),
            finished_at: live,
            at: live,
            ..RunMetrics::default()
        };
        append_run(&dir, &run, 0).unwrap();
    }
    let busy = Now {
        busy: true,
        ..Now::default()
    };
    assert_eq!(hub::habit_chips(&both, busy, 99.0, live), Vec::new());
    let tried = hub::habit_chips(&both, Now::default(), 99.0, live);
    assert_eq!(tried.len(), 1);
    assert_eq!(tried[0].value, "/try-me");
    assert_eq!(
        tried[0].hint,
        format!("Trying this: you sometimes run it {}", part.when())
    );
    assert_eq!(
        hub::habit_chips(&both, Now::default(), 99.0, live + 5),
        tried
    );
    // The try is on the list of decisions, once.
    let decided = hub::view(live + 6).trust.decisions;
    let tries: Vec<_> = decided
        .iter()
        .filter(|d| d.title.starts_with("Skill try-me"))
        .map(|d| (d.title.as_str(), d.did, d.why))
        .collect();
    assert_eq!(
        tries,
        vec![(
            format!("Skill try-me {}", part.when()).as_str(),
            "Asked",
            "It wasn't sure, so it showed this once to see if you want it."
        )]
    );
    log_chips_shown(&mut habit_mem, "composer", &tried, live + 70);
    log_chip_outcome(&mut habit_mem, "composer", &tried[0], false, live + 71);
    assert_eq!(
        hub::habit_chips(&both, Now::default(), 99.0, live + 80),
        Vec::new()
    );

    // The fault report: nothing while the app's own numbers are fine, and
    // after a model failed twelve runs, the text to read. Nothing is sent.
    use grokhub_core::learn::review;
    assert!(!hub::paused());
    assert_eq!(view::run("report", at), review::NOTHING_TO_REPORT);
    for i in 0..12u64 {
        let run = RunMetrics {
            run: format!("chat-bad:{i}"),
            class: "chat".into(),
            result: OutcomeResult::Failure,
            model: "bad-model".into(),
            finished_at: at,
            at,
            ..RunMetrics::default()
        };
        append_run(&dir, &run, 0).unwrap();
    }
    assert_eq!(
        view::run("report", at),
        format!(
            "{}\n\n## What the app measured going wrong\n\n\
             - `model:bad-model` went well in 0 of its last 12 runs (0%).\n\n\
             ---\nMeasured by {} {} on one PC. Ids and counts only.\n",
            review::REPORT_NOTE,
            grokhub_core::brand::product_name(),
            env!("CARGO_PKG_VERSION")
        )
    );

    // Backup and move, from the chat: the file lands in the config folder,
    // an import takes it and keeps what was there, and restore puts it back.
    assert_eq!(
        view::run("backup", at),
        format!(
            "Saved everything the app learned to `{}`. To move it, copy that file into the config folder on the other PC and run `/learned import` there.",
            dir.join("learned-backup.json").display()
        )
    );
    let held = store::load_beliefs(&dir);
    assert!(held.len() > 1);
    let bad = Subject::new("model", "bad-model").unwrap();
    hub::forget(&bad);
    assert_ne!(store::load_beliefs(&dir), held);
    assert_eq!(
        view::run("import", at),
        format!(
            "Took {} learned lines from the backup. What was here before is kept: `/learned restore` puts it back.",
            held.len()
        )
    );
    assert_eq!(store::load_beliefs(&dir), held);
    assert_eq!(
        view::run("restore", at),
        "Put back what the app had learned before the import."
    );
    assert_ne!(store::load_beliefs(&dir), held);
    assert_eq!(
        view::run("restore", at),
        grokhub_core::learn::backup::NO_RESTORE
    );
    // The desktop page runs those four lines for their text, and no other.
    assert_eq!(
        view::page_say("restore", at),
        Ok(grokhub_core::learn::backup::NO_RESTORE.to_string())
    );
    assert_eq!(view::page_say("report", at), Ok(view::run("report", at)));
    for line in ["forget all", "", "pause", "report now"] {
        assert_eq!(view::page_say(line, at), Err(view::USAGE.to_string()));
    }
    assert!(store::load_beliefs(&dir).len() > 1);

    // A model-choice change the router dial holds is answered from the list.
    let route = Subject::new("rule", "router/chat:default/order").unwrap();
    let change = Proposal {
        area: Area::Router,
        rule: route.clone(),
        confidence: 1.0,
        evidence: 60.0,
        undo: grokhub_core::learn::Undo::OneClick,
        value: 0.2,
        hard: false,
        helps: None,
        hurts: None,
        tested: true,
    };
    let weigh = |at: u64| {
        let d = hub::decide_all(vec![change.clone()], Situation::default(), at);
        (d[0].verdict, d[0].reason)
    };
    assert_eq!(
        view::run("yes rule:router/chat:default/order", at),
        decide::NO_ASK
    );
    hub::set_dial(Area::Router, decide::Dial::Offer);
    assert_eq!(weigh(at), (Verdict::Offer, Reason::DialOffer));
    assert_eq!(
        hub::view(at).trust.asks,
        vec![view::AskRow {
            key: "rule:router/chat:default/order".into(),
            title: "Router: which model goes first for chat:default".into(),
            why: "This area is set to offer only.",
        }]
    );
    let listed = view::run("decisions", at);
    assert!(
        listed.contains(
            "**Waiting for your yes**\n- This area is set to offer only. `rule:router/chat:default/order`\nAnswer with `/learned yes <id>` or `/learned no <id>`.\n"
        ),
        "{listed}"
    );
    assert_eq!(
        view::run("yes rule:router/chat:default/order", at),
        decide::YES_SAID
    );
    assert_eq!(hub::view(at).trust.asks, vec![]);
    assert_eq!(weigh(at + 1), (Verdict::Act, Reason::YouSaidYes));
    // The next one asks again, and the page's No button rests it.
    assert_eq!(weigh(at + 2), (Verdict::Offer, Reason::DialOffer));
    let page = view::act(
        view::Act::Answer {
            key: "rule:router/chat:default/order".into(),
            yes: false,
        },
        at + 3,
    )
    .unwrap();
    assert_eq!(page.trust.asks, vec![]);
    assert_eq!(weigh(at + 4), (Verdict::Wait, Reason::Resting));
    assert_eq!(
        view::act(
            view::Act::Answer {
                key: "rule:router/chat:default/order".into(),
                yes: true,
            },
            at + 5,
        ),
        Err(view::NO_SUCH_LINE.to_string())
    );
    assert_eq!(view::run("yes", at), view::USAGE);
    hub::set_dial(Area::Router, decide::Dial::Act);
    let _ = fs::remove_dir_all(&dir);
}

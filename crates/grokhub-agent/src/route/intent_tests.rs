//! The turn reader against a labeled set: what each typed message should do.

use super::*;

const IDLE: TurnCx = TurnCx {
    running: false,
    prior: None,
};
const RUNNING: TurnCx = TurnCx {
    running: true,
    prior: None,
};

/// Messages typed while nothing runs.
const IDLE_SET: &[(&str, TurnRoute)] = &[
    ("hi", TurnRoute::Chat),
    ("thanks!", TurnRoute::Chat),
    ("gg that was a sick match", TurnRoute::Chat),
    ("what's the weather like tomorrow?", TurnRoute::Chat),
    ("how much RAM does my PC have", TurnRoute::Chat),
    ("explain how a GPU renders a frame", TurnRoute::Chat),
    ("write me a short poem about cats", TurnRoute::Chat),
    ("what's a good plan for my week?", TurnRoute::Chat),
    ("how do I set up a python venv?", TurnRoute::Chat),
    ("fix this typo: teh", TurnRoute::Chat),
    ("rename notes.txt to todo.txt", TurnRoute::Chat),
    ("open my downloads folder", TurnRoute::Chat),
    ("why is my fan so loud", TurnRoute::Chat),
    ("tell me a joke", TurnRoute::Chat),
    ("can you check my disk space", TurnRoute::Chat),
    ("what model are you", TurnRoute::Chat),
    ("I'm bored, any game ideas?", TurnRoute::Chat),
    ("summarize this article for me", TurnRoute::Chat),
    ("just answer: how do I build a react app", TurnRoute::Chat),
    ("just do it, set up the whole project", TurnRoute::Chat),
    (
        "build me a full website for my print shop with a store, a gallery and a contact page",
        TurnRoute::Plan,
    ),
    (
        "refactor the whole app to use the new settings system and update all the tests",
        TurnRoute::Plan,
    ),
    (
        "set up a backup system for all my folders, then schedule it every night and test a restore",
        TurnRoute::Plan,
    ),
    (
        "can you migrate the entire codebase from the old database to the new one and fix anything that breaks",
        TurnRoute::Plan,
    ),
    (
        "please organize everything on my drive: move photos into folders by year, delete duplicates, then back it all up",
        TurnRoute::Plan,
    ),
    (
        "I need you to:\n1. install the drivers\n2. update the BIOS\n3. run a stress test\n4. write up the results",
        TurnRoute::Plan,
    ),
    ("plan: move my game library to the new SSD", TurnRoute::Plan),
    ("make a plan to clean up my desktop", TurnRoute::Plan),
    (
        "implement a dark mode feature across the whole app and add tests for it",
        TurnRoute::Plan,
    ),
    (
        "create a discord bot that tracks my raid schedule, posts reminders and logs attendance to a sheet",
        TurnRoute::Plan,
    ),
];

/// Messages typed while a reply runs.
const RUNNING_SET: &[(&str, TurnRoute)] = &[
    ("stop", TurnRoute::Stop),
    ("STOP!", TurnRoute::Stop),
    ("cancel that", TurnRoute::Stop),
    ("wait", TurnRoute::Stop),
    ("hold on", TurnRoute::Stop),
    ("nvm", TurnRoute::Stop),
    ("btw what's the RAM on this machine?", TurnRoute::Side),
    ("by the way, when does the patch drop?", TurnRoute::Side),
    ("quick question: is python 3.12 out yet", TurnRoute::Side),
    ("what time is it in tokyo?", TurnRoute::Side),
    ("how's it going?", TurnRoute::Side),
    ("what are you doing right now?", TurnRoute::Side),
    ("is it almost done?", TurnRoute::Side),
    ("why did you pick that folder?", TurnRoute::Side),
    ("who made this game", TurnRoute::Side),
    ("does my GPU support ray tracing?", TurnRoute::Side),
    ("can you tell me what the weather is", TurnRoute::Side),
    ("side note, what's 15% of 240?", TurnRoute::Side),
    ("btw how do i take a screenshot on windows", TurnRoute::Side),
    ("actually use the other folder", TurnRoute::Steer),
    ("no, put them in Documents", TurnRoute::Steer),
    ("don't delete anything yet", TurnRoute::Steer),
    ("use the beta branch", TurnRoute::Steer),
    ("make sure you keep a backup", TurnRoute::Steer),
    ("also add tests", TurnRoute::Steer),
    ("skip the images", TurnRoute::Steer),
    ("wait, use python not node", TurnRoute::Steer),
    ("put it in the downloads folder instead", TurnRoute::Steer),
    ("that's the wrong one, it's the D drive", TurnRoute::Steer),
    ("only do the first two", TurnRoute::Steer),
    ("after that, write the docs", TurnRoute::Queue),
    ("when you're done, clean up my desktop", TurnRoute::Queue),
    ("next, back up the save files", TurnRoute::Queue),
    ("write a poem about cats", TurnRoute::Queue),
    ("build me a website for my shop", TurnRoute::Queue),
    ("btw can you refactor the whole app", TurnRoute::Queue),
    ("by the way please install steam", TurnRoute::Queue),
    ("do the docs next", TurnRoute::Queue),
    ("can you also check my disk space", TurnRoute::Queue),
    ("organize my photos by year", TurnRoute::Queue),
];

fn misses(set: &[(&str, TurnRoute)], cx: &TurnCx) -> Vec<String> {
    set.iter()
        .filter_map(|(text, want)| {
            let got = read_turn(text, cx);
            (got.route != *want).then(|| {
                format!(
                    "{text:?}: want {} got {} ({:?})",
                    want.id(),
                    got.route.id(),
                    got.rules
                )
            })
        })
        .collect()
}

#[test]
fn every_idle_message_lands_where_it_should() {
    let m = misses(IDLE_SET, &IDLE);
    assert!(m.is_empty(), "idle misroutes:\n{}", m.join("\n"));
}

#[test]
fn every_message_during_a_reply_lands_where_it_should() {
    let m = misses(RUNNING_SET, &RUNNING);
    assert!(m.is_empty(), "running misroutes:\n{}", m.join("\n"));
}

#[test]
fn no_job_is_ever_sent_to_the_side() {
    // A side run can't write, so a job read as a side question is lost work.
    for (text, want) in RUNNING_SET {
        if matches!(want, TurnRoute::Queue | TurnRoute::Steer) {
            let got = read_turn(text, &RUNNING);
            assert_ne!(got.route, TurnRoute::Side, "{text:?} went to the side");
        }
    }
}

#[test]
fn routes_only_fit_their_moment() {
    for (text, _) in IDLE_SET {
        assert!(read_turn(text, &IDLE).route.fits(false), "{text:?}");
    }
    for (text, _) in RUNNING_SET {
        assert!(read_turn(text, &RUNNING).route.fits(true), "{text:?}");
    }
}

#[test]
fn a_typed_btw_answers_on_the_side_and_drops_the_command() {
    let r = read_turn("/btw what's my IP", &RUNNING);
    assert_eq!(r.route, TurnRoute::Side);
    assert_eq!(r.text, "what's my IP");
    assert_eq!(r.rules[0], "turn:side:typed");
    let idle = read_turn("/btw what's my IP", &IDLE);
    assert_eq!(idle.route, TurnRoute::Chat);
    assert_eq!(idle.text, "what's my IP");
}

#[test]
fn a_typed_plan_plans_now_or_waits_with_its_command() {
    let r = read_turn("/plan clean my desktop", &IDLE);
    assert_eq!(
        (r.route, r.text.as_str()),
        (TurnRoute::Plan, "clean my desktop")
    );
    let q = read_turn("/plan clean my desktop", &RUNNING);
    assert_eq!(
        (q.route, q.text.as_str()),
        (TurnRoute::Queue, "/plan clean my desktop")
    );
}

#[test]
fn a_bare_command_is_not_an_override() {
    assert_eq!(read_turn("/btw", &RUNNING).rules[0], "turn:queue");
    assert_eq!(read_turn("/planner", &IDLE).route, TurnRoute::Chat);
}

#[test]
fn each_route_has_an_instant_line() {
    assert_eq!(TurnRoute::Side.ack(), "Answering on the side");
    assert_eq!(
        TurnRoute::Steer.ack(),
        "Got it, I'll fold that in after this step"
    );
    assert_eq!(TurnRoute::Queue.ack(), "Queued for after this reply");
    assert_eq!(TurnRoute::Stop.ack(), "Stopped");
    assert_eq!(TurnRoute::Plan.ack(), "Making a plan first");
    assert_eq!(TurnRoute::Chat.ack(), "");
}

#[test]
fn route_ids_round_trip() {
    for r in TurnRoute::ALL {
        assert_eq!(TurnRoute::parse(r.id()), Some(r));
    }
    assert_eq!(TurnRoute::parse("mode"), None);
    assert_eq!(serde_json::to_string(&TurnRoute::Side).unwrap(), "\"side\"");
}

#[test]
fn the_whole_set_reads_in_well_under_fifty_ms() {
    let start = std::time::Instant::now();
    for _ in 0..10 {
        let _ = misses(IDLE_SET, &IDLE);
        let _ = misses(RUNNING_SET, &RUNNING);
    }
    let per_pass = start.elapsed() / 10;
    assert!(
        per_pass < std::time::Duration::from_millis(50),
        "one pass took {per_pass:?}"
    );
}

#[test]
fn shape_keys_keep_no_words_and_split_by_moment() {
    let a = shape_key("why is the sky blue?", true);
    assert_eq!(a.len(), 16);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, shape_key("why is the sky blue?", false));
    // Same first words, same kind, same length bucket: same shape.
    assert_eq!(a, shape_key("why is grass green?", true));
}

#[test]
fn two_corrections_flip_a_shape_and_a_typed_override_still_stands() {
    let dir = crate::harness::test_dir("intent-flip");
    let cfg = dir.as_path();
    let text = "why did you pick that folder?";
    assert_eq!(
        read_turn_learned(cfg, text, &RUNNING).route,
        TurnRoute::Side
    );
    note_correction(cfg, text, true, TurnRoute::Steer);
    assert_eq!(
        read_turn_learned(cfg, text, &RUNNING).route,
        TurnRoute::Side,
        "one correction is not enough"
    );
    note_correction(cfg, "why did you skip the images?", true, TurnRoute::Steer);
    let r = read_turn_learned(cfg, text, &RUNNING);
    assert_eq!(r.route, TurnRoute::Steer);
    assert_eq!(r.rules[0], "turn:steer:learned");
    // Saved without words.
    let saved = std::fs::read_to_string(nudge_path(cfg)).expect("nudges saved");
    assert!(
        !saved.contains("folder") && !saved.contains("why"),
        "{saved}"
    );
    assert!(saved.contains("\"steer\": 2"), "{saved}");
    // The same shape while idle is a different shape.
    assert_eq!(read_turn_learned(cfg, text, &IDLE).route, TurnRoute::Chat);
    // A typed override is never relearned.
    let typed = read_turn_learned(cfg, "/btw why did you pick that folder?", &RUNNING);
    assert_eq!(typed.route, TurnRoute::Side);
}

#[test]
fn a_correction_that_does_not_fit_its_moment_is_ignored() {
    let dir = crate::harness::test_dir("intent-fit");
    note_correction(dir.as_path(), "hello there", false, TurnRoute::Side);
    note_correction(dir.as_path(), "hello there", false, TurnRoute::Side);
    assert!(!nudge_path(&dir).exists());
}

#[test]
fn the_nudge_store_stays_capped() {
    let dir = crate::harness::test_dir("intent-cap");
    for i in 0..(NUDGE_CAP + 20) {
        note_correction(&dir, &format!("w{i} x{i}"), false, TurnRoute::Plan);
    }
    let file: NudgeFile =
        grokhub_core::model_registry::store::read_json(&nudge_path(&dir)).expect("saved");
    assert_eq!(file.shapes.len(), NUDGE_CAP);
    assert_eq!(file.schema, NUDGE_SCHEMA);
}

#[test]
fn just_answer_and_plan_first_correct_the_previous_turn() {
    assert_eq!(
        correction_of_previous("no, just answer"),
        Some(TurnRoute::Chat)
    );
    assert_eq!(correction_of_previous("Just do it."), Some(TurnRoute::Chat));
    assert_eq!(
        correction_of_previous("make a plan first"),
        Some(TurnRoute::Plan)
    );
    assert_eq!(correction_of_previous("make a plan for dinner"), None);
}

#[test]
fn the_shell_smoke_plan_prompt_plans() {
    // ui/e2e/smoke.mjs types this with no picker and expects a plan card.
    let read = read_turn("make a plan to tidy the smoke notes", &IDLE);
    assert_eq!(read.route, TurnRoute::Plan);
}

#[test]
fn a_go_after_a_plan_is_told_apart_from_a_change() {
    for go in [
        "yes",
        "Go.",
        "go ahead",
        "Go ahead with the plan.",
        "Go ahead, but skip step 3",
        "do it but add a backup first",
        "LGTM!",
        "sounds good",
        "yes, go",
    ] {
        assert!(approves_plan(go), "{go}");
    }
    for not_go in [
        "Tell me more about this plan before I say go.",
        "Change the plan: drop step 2",
        "yes but change step 2",
        "no",
        "going to bed",
        "what does step 3 do?",
    ] {
        assert!(!approves_plan(not_go), "{not_go}");
    }
}

use std::cell::Cell;
use std::path::{Path, PathBuf};

use grokhub_agent::harness::{self as hx, append_span, fixture_span, outcome_from_spans};
use grokhub_agent::{ContentPart, InputItem, Usage};
use grokhub_core::review_pass::{load_suggestions, SUGGESTIONS_FILE};
use grokhub_core::{LocalClock, UpdateKind};

use super::*;

/// 2026-10-07, a Wednesday.
const NOW: u64 = 1_791_403_200_000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const TODAY: &str = "2026-10-07";
const TOMORROW: &str = "2026-10-08";
const HOUR: u32 = 21;

const TIDY_SKILL: &str = "SUGGEST_SKILL: tidy-board | Tidy the board | Sort open cards | when the board is full | sort cards by due date";

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shell-chat-review-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn at(hour: u32) -> LocalClock {
    LocalClock {
        now_ms: NOW,
        weekday: 3,
        hour,
        minute: 0,
    }
}

/// A saved chat where you said each of `asks`, each answered.
fn chat(dir: &Path, id: &str, asks: &[&str]) {
    let _config = grokhub_agent::perm::ConfigGuard::set(dir);
    let say = |role: &str, text: &str| InputItem::Message {
        role: role.into(),
        content: vec![ContentPart::InputText(text.into())],
    };
    let items: Vec<InputItem> = asks
        .iter()
        .flat_map(|a| [say("user", a), say("assistant", "Done.")])
        .collect();
    grokhub_agent::record_turn(
        id,
        "/home/user",
        "grok-test",
        &items,
        &Usage::default(),
        "",
        asks[0],
    )
    .unwrap();
}

/// Eight turns of yours, two of them asking to tidy the board.
fn tidy_week(dir: &Path, id: &str) {
    chat(
        dir,
        id,
        &[
            "Can you tidy the board cards for me",
            "What is the weather like in the harbor",
            "Please tidy the board cards again today",
            "Draft a note about the garden plans",
            "Read me the last release notes",
            "Find the receipt from the bike shop",
            "Rename the photos from the trip",
            "Open the reading list in notes",
        ],
    );
}

/// A call that counts and answers `reply`.
struct Model {
    calls: Cell<u32>,
    reply: Result<String, String>,
}

impl Model {
    fn answering(reply: &str) -> Self {
        Self {
            calls: Cell::new(0),
            reply: Ok(reply.into()),
        }
    }
    fn failing(why: &str) -> Self {
        Self {
            calls: Cell::new(0),
            reply: Err(why.into()),
        }
    }
    fn step(&self, dir: &Path, today: &str, hour: u32) -> Reviewed {
        let call = |system: &str, digest: &str| {
            assert!(system.contains("SUGGEST_SKILL:"), "the review prompt");
            assert!(digest.contains("Recent chat:"), "{digest}");
            self.calls.set(self.calls.get() + 1);
            self.reply.clone()
        };
        review_step(dir, today, &at(hour), HOUR, NOW, &call)
    }
}

fn skill_ideas(dir: &Path) -> Vec<String> {
    crate::pulse::cards(dir)
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == UpdateKind::Idea && c.skill.is_some())
        .map(|c| c.source_id)
        .collect()
}

fn day() -> crate::pulse::Day {
    serde_json::from_str(r#"{"date":"2026-10-07","hour":21,"minute":0}"#).unwrap()
}

#[test]
fn a_first_review_waits_for_the_night_hour() {
    let d = temp_dir("first");
    tidy_week(&d, "shell-1");
    let model = Model::answering("");
    assert_eq!(model.step(&d, TODAY, 20), Reviewed::NotDue);
    assert_eq!(model.calls.get(), 0);
    assert_eq!(load_suggestions(&d).last_review_day, None);
    assert!(!d.join(SUGGESTIONS_FILE).exists(), "nothing to write yet");
}

#[test]
fn seven_new_turns_wait_and_eight_run_once() {
    let d = temp_dir("worth");
    chat(
        &d,
        "shell-1",
        &[
            "one ask here",
            "two ask here",
            "three ask",
            "four ask",
            "five ask",
            "six ask",
            "seven ask",
        ],
    );
    let model = Model::answering("Nothing useful tonight.");
    assert_eq!(
        model.step(&d, TODAY, 21),
        Reviewed::NotWorth { new_turns: 7 }
    );
    assert_eq!(model.calls.get(), 0);
    assert_eq!(load_suggestions(&d).last_review_day, None);

    chat(&d, "shell-2", &["eighth ask today"]);
    assert_eq!(
        model.step(&d, TODAY, 21),
        Reviewed::Ran {
            kept: 0,
            ideas: 0,
            patched: 0
        }
    );
    assert_eq!(model.calls.get(), 1);
    let store = load_suggestions(&d);
    assert_eq!(store.reviewed_turns, 8);
    assert_eq!(store.last_review_day.as_deref(), Some(TODAY));
    assert_eq!(store.last_review_ms, NOW);

    assert_eq!(model.step(&d, TODAY, 23), Reviewed::NotDue);
    // Tomorrow, nothing new to read: no call.
    assert_eq!(
        model.step(&d, TOMORROW, 9),
        Reviewed::NotWorth { new_turns: 0 }
    );
    assert_eq!(model.calls.get(), 1);
}

#[test]
fn a_failed_call_holds_until_the_next_night() {
    let d = temp_dir("held");
    tidy_week(&d, "shell-1");
    let model = Model::failing("no key");
    assert_eq!(model.step(&d, TODAY, 21), Reviewed::Held("no key".into()));
    assert_eq!(model.calls.get(), 1);
    let store = load_suggestions(&d);
    assert_eq!(store.last_review_day.as_deref(), Some(TODAY));
    assert_eq!(store.reviewed_turns, 8);
    assert_eq!(model.step(&d, TODAY, 23), Reviewed::NotDue);
    assert_eq!(model.calls.get(), 1);
}

#[test]
fn a_broken_suggestions_file_holds_without_a_call() {
    let d = temp_dir("broken");
    tidy_week(&d, "shell-1");
    std::fs::write(d.join(SUGGESTIONS_FILE), "{ not json").unwrap();
    let model = Model::answering(TIDY_SKILL);
    let Reviewed::Held(why) = model.step(&d, TODAY, 21) else {
        panic!("a broken file holds the review");
    };
    assert!(why.starts_with("suggestions.json: "), "{why}");
    assert_eq!(model.calls.get(), 0);
    assert_eq!(
        std::fs::read_to_string(d.join(SUGGESTIONS_FILE)).unwrap(),
        "{ not json"
    );
}

#[test]
fn a_scratch_chat_is_never_read() {
    let d = temp_dir("scratch");
    tidy_week(&d, &grokhub_agent::scratch::shell_id(true, "1"));
    let model = Model::answering(TIDY_SKILL);
    assert_eq!(
        model.step(&d, TODAY, 21),
        Reviewed::NotWorth { new_turns: 0 }
    );
    assert_eq!(model.calls.get(), 0);
}

#[test]
fn a_skill_suggestion_lands_once_as_a_learn_idea() {
    let d = temp_dir("idea");
    tidy_week(&d, "shell-1");
    let model = Model::answering(TIDY_SKILL);
    assert_eq!(
        model.step(&d, TODAY, 21),
        Reviewed::Ran {
            kept: 1,
            ideas: 1,
            patched: 0
        }
    );
    assert_eq!(skill_ideas(&d), vec!["skill-tidy-board".to_string()]);
    let page = crate::pulse::peek(&d, NOW).unwrap();
    let idea = page
        .ideas
        .iter()
        .flat_map(|g| g.ideas.iter())
        .find(|i| i.skill)
        .expect("the skill idea is on the page");
    assert_eq!(idea.kind, "Learn");
    assert_eq!(idea.action, "sort cards by due date");
    let store = load_suggestions(&d);
    assert!(store.skills.is_empty(), "posted skills leave the list");
    assert_eq!(store.idea_titles, vec!["Tidy the board".to_string()]);

    // Tomorrow the same suggestion does not post a second idea.
    tidy_week(&d, "shell-2");
    assert_eq!(
        model.step(&d, TOMORROW, 21),
        Reviewed::Ran {
            kept: 1,
            ideas: 0,
            patched: 0
        }
    );
    assert_eq!(skill_ideas(&d).len(), 1);
}

#[test]
fn a_dismissed_or_turned_down_skill_does_not_come_back() {
    let d = temp_dir("gone");
    tidy_week(&d, "shell-1");
    let model = Model::answering(TIDY_SKILL);
    model.step(&d, TODAY, 21);
    let id = crate::pulse::cards(&d).unwrap()[0].id.clone();
    {
        let _config = grokhub_agent::perm::ConfigGuard::set(&d);
        crate::pulse::act(&d, &id, crate::pulse::Act::Dismiss, &day(), NOW).unwrap();
    }
    assert!(skill_ideas(&d).is_empty());
    tidy_week(&d, "shell-2");
    model.step(&d, TOMORROW, 21);
    assert!(skill_ideas(&d).is_empty(), "a dismissed idea stays gone");

    // Not interested on Pulse: the review keeps nothing on that topic.
    let d = temp_dir("turned-down");
    tidy_week(&d, "shell-1");
    crate::pulse_feed::with_ideas_state(&d, |s| s.note_not_interested("Tidy the board")).unwrap();
    assert_eq!(
        model.step(&d, TODAY, 21),
        Reviewed::Ran {
            kept: 0,
            ideas: 0,
            patched: 0
        }
    );
    assert!(skill_ideas(&d).is_empty());
}

#[test]
fn add_skill_saves_it_and_takes_the_idea_off() {
    let d = temp_dir("add");
    tidy_week(&d, "shell-1");
    Model::answering(TIDY_SKILL).step(&d, TODAY, 21);
    let id = crate::pulse::cards(&d).unwrap()[0].id.clone();
    let _config = grokhub_agent::perm::ConfigGuard::set(&d);
    let page = crate::pulse::act(&d, &id, crate::pulse::Act::AddSkill, &day(), NOW).unwrap();
    assert!(page.ideas.iter().all(|g| g.ideas.is_empty()));
    let md = std::fs::read_to_string(d.join("skills/tidy-board/SKILL.md")).unwrap();
    assert!(md.contains("sort cards by due date"), "{md}");
    assert!(md.contains("when the board is full"), "{md}");
    let ledger = std::fs::read_to_string(hx::skill_ledger_path(&d)).unwrap();
    assert_eq!(ledger.lines().count(), 1, "{ledger}");
    assert!(ledger.contains("added from Suggested"), "{ledger}");
    assert!(skill_ideas(&d).is_empty());
}

#[test]
fn add_skill_leaves_a_skill_you_already_have() {
    let d = temp_dir("have");
    tidy_week(&d, "shell-1");
    Model::answering(TIDY_SKILL).step(&d, TODAY, 21);
    let id = crate::pulse::cards(&d).unwrap()[0].id.clone();
    let mine = d.join("skills/tidy-board");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(mine.join("SKILL.md"), "my own steps\n").unwrap();
    let _config = grokhub_agent::perm::ConfigGuard::set(&d);
    assert_eq!(
        crate::pulse::act(&d, &id, crate::pulse::Act::AddSkill, &day(), NOW).unwrap_err(),
        "You already have a skill called tidy-board."
    );
    assert_eq!(
        std::fs::read_to_string(mine.join("SKILL.md")).unwrap(),
        "my own steps\n"
    );
    assert_eq!(skill_ideas(&d), vec!["skill-tidy-board".to_string()]);
}

const REPORT_STEPS: &str = "1. open the sheet\n2. click Export";
const PATCH: &str = "SUGGEST_SKILL_PATCH: weekly-report | every Friday | 1. open the sheet\n2. click Export\n3. save it as csv";

fn report_skill(dir: &Path) {
    let folder = dir.join("skills/weekly-report");
    std::fs::create_dir_all(&folder).unwrap();
    let md = grokhub_core::render_skill_md(&grokhub_core::SkillMd {
        name: "weekly-report".into(),
        description: "a report".into(),
        instructions: REPORT_STEPS.into(),
        ..Default::default()
    });
    std::fs::write(folder.join("SKILL.md"), md).unwrap();
}

/// A recorded good run of weekly-report, for the replay gate.
fn report_run(dir: &Path) {
    let at = NOW - DAY_MS;
    let mut click = fixture_span(
        at,
        "click",
        r#"{"x":1}"#,
        "allow",
        "clicked",
        Some(true),
        "",
    );
    click.session_id = "shell-run".into();
    let mut verify = fixture_span(at + 1, "verify_script", "{}", "allow", "pass", None, "");
    verify.session_id = "shell-run".into();
    for s in [&click, &verify] {
        append_span(dir, s).unwrap();
    }
    let o = outcome_from_spans(
        "shell-run:1",
        "skill:weekly-report",
        &[click, verify],
        &[],
        false,
        at + 2,
    );
    grokhub_core::outcome::append_outcome(dir, &o).unwrap();
}

#[test]
fn a_skill_patch_goes_through_the_replay_gate() {
    let d = temp_dir("patch");
    report_skill(&d);
    tidy_week(&d, "shell-1");
    let before = std::fs::read_to_string(d.join("skills/weekly-report/SKILL.md")).unwrap();
    // No recorded run to replay: the patch is held.
    let one_line = PATCH.replace('\n', " ");
    let model = Model::answering(&one_line);
    assert_eq!(
        model.step(&d, TODAY, 21),
        Reviewed::Ran {
            kept: 0,
            ideas: 0,
            patched: 0
        }
    );
    assert_eq!(
        std::fs::read_to_string(d.join("skills/weekly-report/SKILL.md")).unwrap(),
        before
    );

    report_run(&d);
    tidy_week(&d, "shell-2");
    assert_eq!(
        model.step(&d, TOMORROW, 21),
        Reviewed::Ran {
            kept: 0,
            ideas: 0,
            patched: 1
        }
    );
    let after = std::fs::read_to_string(d.join("skills/weekly-report/SKILL.md")).unwrap();
    assert!(after.contains("save it as csv"), "{after}");
    assert!(after.contains("every Friday"), "{after}");
    let ledger = std::fs::read_to_string(hx::skill_ledger_path(&d)).unwrap();
    assert!(
        ledger.contains("nightly review: 1. open the sheet"),
        "{ledger}"
    );

    // You undid it: the next night does not write it again.
    hx::undo_skill_change(
        &d,
        &d.join("skills"),
        "weekly-report",
        hx::UndoAsk::from_click(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(d.join("skills/weekly-report/SKILL.md")).unwrap(),
        before
    );
    tidy_week(&d, "shell-3");
    let later = Model::answering(&one_line);
    assert_eq!(
        later.step(&d, "2026-10-09", 21),
        Reviewed::Ran {
            kept: 0,
            ideas: 0,
            patched: 0
        }
    );
    assert_eq!(
        std::fs::read_to_string(d.join("skills/weekly-report/SKILL.md")).unwrap(),
        before
    );
}

#[test]
fn the_quiet_pass_adds_a_tile_without_a_call() {
    let d = temp_dir("quiet");
    chat(
        &d,
        "shell-1",
        &[
            "summarize the workboard for me",
            "what is on the workboard now",
        ],
    );
    let model = Model::answering("");
    assert_eq!(
        model.step(&d, TODAY, 21),
        Reviewed::NotWorth { new_turns: 2 }
    );
    assert_eq!(model.calls.get(), 0);
    let store = load_suggestions(&d);
    assert_eq!(store.last_session_suggest_day.as_deref(), Some(TODAY));
    assert_eq!(store.autos.len(), 1);
    assert_eq!(store.autos[0].title, "Midday board");
    // It shows first on the Automations page.
    assert_eq!(crate::suggested::suggested(&d)[0].title, "Midday board");
}

//! Spike-7 acceptance tests: outcome records from real turn ends, the weekly
//! pass's card budget, skill drafts, the replay gate on Apply, and the revert
//! card. Every config is a temp `GROKHUB_CONFIG`; the model is a fake that
//! counts its calls.

use super::self_review_ui::set_self_review_fake;
use super::*;
use grokhub_agent::harness as hx;
use grokhub_core::outcome::{self as oc, OutcomeResult, OutcomeSignals, TaskOutcome};
use grokhub_core::run_metrics;
use grokhub_core::{UpdateCard, UpdateKind, UpdateStatus};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const DAY: u64 = 24 * 60 * 60 * 1000;

fn pin(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
    let root = crate::config::test_config_root(label);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    (crate::config::TestConfigDir::set(root.clone()), root)
}

fn skill(name: &str, steps: &str) -> SkillMd {
    SkillMd {
        name: name.into(),
        description: format!("Run {name}."),
        slash: format!("/{name}"),
        trigger: format!("the user asks for {name}"),
        instructions: steps.into(),
        ..Default::default()
    }
}

/// A recorded run of `name` on disk: its spans and one outcome line.
fn recorded(root: &std::path::Path, name: &str, task: &str, ok: bool, at: u64, tools: &[&str]) {
    let session = format!("chat-{name}");
    let mut spans = Vec::new();
    for (i, tool) in tools.iter().enumerate() {
        let mut s = hx::fixture_span(
            at + i as u64,
            tool,
            "{}",
            "allow",
            "clicked",
            Some(true),
            "",
        );
        s.session_id = session.clone();
        spans.push(s);
    }
    let mut v = hx::fixture_span(
        at + 50,
        "verify_script",
        "{}",
        "allow",
        if ok { "pass" } else { "fail" },
        None,
        "",
    );
    v.session_id = session;
    spans.push(v);
    for s in &spans {
        hx::append_span(root, s).unwrap();
    }
    let findings: Vec<String> = if ok {
        Vec::new()
    } else {
        vec!["action_loop".into()]
    };
    let o = hx::outcome_from_spans(
        task,
        &oc::skill_signature(name),
        &spans,
        &findings,
        false,
        at + 60,
    );
    oc::append_outcome(root, &o).unwrap();
}

struct Fake {
    calls: Arc<AtomicUsize>,
    reply: String,
}

impl grokhub_agent::ModelClient for Fake {
    fn stream(
        &self,
        _req: &grokhub_agent::ResponsesRequest,
        _cancel: &grokhub_agent::CancelToken,
        _sink: &mut dyn FnMut(grokhub_agent::StreamEvent),
    ) -> Result<grokhub_agent::TurnOutput, grokhub_agent::ClientError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(grokhub_agent::TurnOutput {
            text: self.reply.clone(),
            reasoning: String::new(),
            calls: Vec::new(),
            usage: grokhub_agent::Usage::default(),
            ..Default::default()
        })
    }
}

fn fake(reply: &str) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    set_self_review_fake(Box::new(Fake {
        calls: calls.clone(),
        reply: reply.into(),
    }));
    calls
}

fn wait_pass(cabin: &mut Cabin) {
    for _ in 0..500 {
        cabin.poll_self_review();
        if cabin.harness.self_review.rx.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the weekly pass never answered");
}

fn self_review_cards(cabin: &Cabin) -> Vec<UpdateCard> {
    cabin
        .updates
        .iter()
        .filter(|c| {
            c.source_id
                .starts_with(grokhub_core::self_review::CARD_SOURCE_PREFIX)
                && c.status != UpdateStatus::Dismissed
        })
        .cloned()
        .collect()
}

#[test]
fn a_turn_end_writes_one_outcome_and_a_later_undo_supersedes_it() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("spike7-outcome");
    let v1 = skill("weekly-report", "1. Open report.md\n2. Fill the numbers");
    skills::save_skill(&v1).unwrap();
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.skill_name = "weekly-report".into();
    let trace = cabin.trace_id();
    let mut v = hx::fixture_span(1_000, "verify_script", "{}", "allow", "pass", None, "")
        .in_turn(&trace, 1);
    v.session_id = trace.clone();
    hx::append_span(&root, &v).unwrap();
    cabin.harness_turn_end("Report filled. VERIFY_OK", 1);
    let all = oc::read_outcomes(&root);
    assert_eq!(all.len(), 1);
    assert_eq!(
        (
            all[0].task.as_str(),
            all[0].signature.as_str(),
            all[0].result
        ),
        (
            format!("{trace}:1").as_str(),
            "skill:weekly-report",
            OutcomeResult::Success
        )
    );
    assert_eq!(all[0].reasons, vec!["verify_ok".to_string()]);
    assert_eq!(all[0].span_ids.len(), 2, "the verify step and the reply");
    assert_eq!(all[0].span_ids[0], v.span_ref());
    let runs = run_metrics::read_runs(&root);
    assert_eq!(runs.len(), 1, "one metrics line per finished run");
    assert_eq!(
        (
            runs[0].run.as_str(),
            runs[0].class.as_str(),
            runs[0].result,
            runs[0].verified,
            runs[0].steps,
            runs[0].undos
        ),
        (
            format!("{trace}:1").as_str(),
            "skill:weekly-report",
            OutcomeResult::Success,
            true,
            0,
            0
        )
    );

    // GrokHub changes the skill, then the user undoes it within the day.
    let mut v2 = v1.clone();
    v2.instructions = "1. Open report.md\n2. Fill the numbers\n3. Save a PDF".into();
    skills::save_skill_logged(&v2, hx::Origin::SelfManage, "nightly review").unwrap();
    cabin.send_from_composer("/skills undo weekly-report".into());
    let all = oc::read_outcomes(&root);
    assert_eq!(all.len(), 1, "still one record for the task");
    assert_eq!(
        (all[0].result, all[0].reasons.clone()),
        (OutcomeResult::Failure, vec!["undo".to_string()])
    );
    assert_eq!(
        oc::read_outcome_lines(&root).len(),
        2,
        "the undo is a superseding line"
    );
    let runs = run_metrics::read_runs(&root);
    assert_eq!(
        (
            runs.len(),
            runs[0].result,
            runs[0].undos,
            runs[0].corrections
        ),
        (1, OutcomeResult::Failure, 1, 0)
    );
    assert_eq!(run_metrics::read_run_lines(&root).len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_correction_after_the_turn_makes_it_a_failure() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("spike7-correction");
    let mut cabin = Cabin::quiet_for_test();
    cabin
        .live_mut()
        .push(("user".into(), "Summarize the March invoices".into()));
    let trace = cabin.trace_id();
    let mut click =
        hx::fixture_span(5, "click", "{}", "allow", "clicked", Some(true), "").in_turn(&trace, 3);
    click.session_id = trace.clone();
    hx::append_span(&root, &click).unwrap();
    cabin.harness_turn_end("Here is the summary.", 3);
    let first = oc::read_outcomes(&root);
    assert_eq!(
        (first[0].signature.as_str(), first[0].result),
        ("topic:summarize march invoic", OutcomeResult::Pending)
    );
    cabin.outcome_user_said("thanks, now April");
    assert_eq!(oc::read_outcome_lines(&root).len(), 1, "not a correction");
    assert_eq!(run_metrics::read_run_lines(&root).len(), 1);
    cabin.outcome_user_said("No, that's the wrong sheet");
    let all = oc::read_outcomes(&root);
    assert_eq!(all.len(), 1);
    assert_eq!(
        (all[0].result, all[0].reasons.clone()),
        (OutcomeResult::Failure, vec!["correction".to_string()])
    );
    let runs = run_metrics::read_runs(&root);
    assert_eq!(
        (
            runs.len(),
            runs[0].steps,
            runs[0].result,
            runs[0].corrections,
            runs[0].undos
        ),
        (1, 1, OutcomeResult::Failure, 1, 0),
        "the click counts as a step; the correction is a newer line"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_weekly_pass_posts_at_most_five_cards_with_numbers_and_refuses_a_consent_target() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("spike7-weekly");
    let now = grokhub_core::now_ms();
    let mut reply = String::new();
    for i in 0..7 {
        let name = format!("report-{i}");
        skills::save_skill(&skill(&name, "1. Open report.md\n2. click Export")).unwrap();
        for k in 0..5u64 {
            recorded(
                &root,
                &name,
                &format!("{name}:{k}"),
                k >= 3,
                now - (k + 1) * DAY / 2,
                &["click"],
            );
        }
        reply.push_str(&format!("PATCH_SKILL: skill:{name} | Wait for Export | 1. Open report.md 2. click Export 3. wait for the file\n"));
    }
    reply.push_str("PATCH_SKILL: consent.jsonl | Grant all | yes\n");
    let calls = fake(&reply);
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.run_self_review_now(now);
    wait_pass(&mut cabin);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let cards = self_review_cards(&cabin);
    assert_eq!(cards.len(), 5, "at most five cards a week");
    assert!(cards.iter().all(|c| c.kind == UpdateKind::Suggestion));
    let first = cards
        .iter()
        .find(|c| c.source_id == "self-review:patch:report-0")
        .expect("report-0 card");
    assert_eq!(first.title, "Wait for Export");
    assert_eq!(
        first.body.as_deref(),
        Some("3 of 5 report-0 runs needed a fix")
    );
    let details = first.details.clone().unwrap_or_default();
    assert!(
        details.starts_with("What changed: 1 added, 0 merged, 0 removed\n- Added: \"3. wait for the file\"\n\n3 of 5"),
        "the What changed block sits above the numbers: {details}"
    );
    assert!(
        details.contains("  2. click Export\n+ 3. wait for the file\n"),
        "{details}"
    );
    let findings = hx::read_scope_findings(&root);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        (findings[0].target.as_str(), findings[0].detail.as_str()),
        (
            "consent.jsonl",
            "GrokHub never changes the consent store on its own"
        )
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn apply_runs_the_replay_gate_and_a_failing_replay_leaves_skill_md_byte_identical() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("spike7-replay");
    let now = grokhub_core::now_ms();
    let path =
        skills::save_skill(&skill("weekly-report", "1. Open report.md\n2. click Save")).unwrap();
    let before = std::fs::read(&path).unwrap();
    recorded(
        &root,
        "weekly-report",
        "w:1",
        false,
        now - 2 * DAY,
        &["click"],
    );
    recorded(&root, "weekly-report", "w:2", true, now - DAY, &["click"]);
    let calls = fake(
        "PATCH_SKILL: skill:weekly-report | Use the keyboard | 1. Open report.md 2. press Ctrl+S",
    );
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.run_self_review_now(now);
    wait_pass(&mut cabin);
    let card = self_review_cards(&cabin).pop().expect("one card");
    cabin.pulse_accept(&card.id, "2026-10-11");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "SKILL.md is byte-identical"
    );
    assert_eq!(
        cabin.status,
        "Replay failed, so the change to weekly-report was not applied: the patch drops the `click` step the last run needed."
    );
    let shown = cabin.updates.iter().find(|c| c.id == card.id).unwrap();
    assert!(
        shown
            .details
            .as_deref()
            .unwrap_or("")
            .starts_with("Replay failed"),
        "the note is on the card"
    );
    let spans = hx::read_spans(&root, hx::SELF_REVIEW_SESSION).unwrap();
    assert!(
        spans
            .iter()
            .any(|s| s.tool == hx::REPLAY_TOOL && s.decision == "deny"),
        "a span for the refused replay"
    );
    assert!(
        hx::ChangeLedger::load(&root).all().is_empty(),
        "nothing went through the ledger"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the replay made no model call"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_passing_replay_applies_the_patch_through_the_ledger() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("spike7-apply");
    let now = grokhub_core::now_ms();
    let path =
        skills::save_skill(&skill("weekly-report", "1. Open report.md\n2. click Save")).unwrap();
    recorded(
        &root,
        "weekly-report",
        "w:1",
        false,
        now - 2 * DAY,
        &["click"],
    );
    recorded(&root, "weekly-report", "w:2", true, now - DAY, &["click"]);
    fake("PATCH_SKILL: skill:weekly-report | Wait for the save | 1. Open report.md 2. click Save 3. wait for the toast");
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.run_self_review_now(now);
    wait_pass(&mut cabin);
    let card = self_review_cards(&cabin).pop().expect("one card");
    cabin.pulse_accept(&card.id, "2026-10-11");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("3. wait for the toast"), "{text}");
    let ledger = hx::ChangeLedger::load(&root);
    let c = &ledger.all()[0];
    assert_eq!(
        (c.op.as_str(), c.origin.as_str()),
        ("modify", "self_manage")
    );
    assert_eq!(c.reason, "weekly self-review: Wait for the save");
    assert!(self_review_cards(&cabin).is_empty(), "the card is done");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn three_similar_runs_in_two_weeks_stage_one_draft_and_apply_creates_it() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("spike7-draft");
    let now = grokhub_core::now_ms();
    for (i, ask) in [
        "Summarize the March invoices",
        "summarize invoices for March",
        "Summarize March invoices again",
    ]
    .iter()
    .enumerate()
    {
        let mut s = hx::fixture_span(
            10 + i as u64,
            "open_file",
            "{}",
            "allow",
            "opened",
            Some(true),
            "",
        );
        s.session_id = format!("chat-d{i}");
        hx::append_span(&root, &s).unwrap();
        let sig = OutcomeSignals {
            verify_ok: true,
            ..Default::default()
        };
        let o = TaskOutcome::finished(
            &format!("chat-d{i}:1"),
            &oc::topic_signature(ask),
            &sig,
            now - (i as u64 + 1) * DAY,
        )
        .with_spans([s.span_ref()]);
        oc::append_outcome(&root, &o).unwrap();
    }
    let calls = fake("");
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_self_review_now(now);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "no failures, no model call"
    );
    let cards = self_review_cards(&cabin);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "Make a skill: summarize-march-invoic");
    assert_eq!(
        cards[0].body.as_deref(),
        Some("You did this 3 times in the last two weeks")
    );
    let details = cards[0].details.clone().unwrap_or_default();
    assert!(
        details.starts_with("What changed: 1 added, 0 merged, 0 removed\n- Added: \"1. open_file\"\n\nYou did this 3 times"),
        "the What changed block sits above the numbers: {details}"
    );
    assert!(
        details.contains("+ name: summarize-march-invoic\n")
            && details.contains("+ 1. open_file\n"),
        "{details}"
    );
    cabin.pulse_accept(&cards[0].id, "2026-10-11");
    assert!(skills::skill_folder("summarize-march-invoic")
        .join("SKILL.md")
        .exists());
    let ledger = hx::ChangeLedger::load(&root);
    assert_eq!(
        (ledger.all()[0].op.as_str(), ledger.all()[0].origin.as_str()),
        ("create", "self_manage")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_success_drop_after_a_patch_gives_a_revert_card_and_revert_puts_v1_back() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("spike7-revert");
    let now = grokhub_core::now_ms();
    let path =
        skills::save_skill(&skill("weekly-report", "1. Open report.md\n2. click Save")).unwrap();
    let v1 = std::fs::read(&path).unwrap();
    for k in 0..3u64 {
        recorded(
            &root,
            "weekly-report",
            &format!("b:{k}"),
            true,
            now - (10 + k) * DAY,
            &["click"],
        );
    }
    skills::save_skill_logged(
        &skill("weekly-report", "1. click Save"),
        hx::Origin::SelfManage,
        "nightly review",
    )
    .unwrap();
    let patch_at = hx::ChangeLedger::load(&root).all()[0].at;
    for k in 0..3u64 {
        recorded(
            &root,
            "weekly-report",
            &format!("a:{k}"),
            k == 0,
            patch_at + (k + 1) * 1_000,
            &["click"],
        );
    }
    fake("");
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.run_self_review_now(patch_at + DAY);
    wait_pass(&mut cabin);
    let reverts: Vec<UpdateCard> = self_review_cards(&cabin)
        .into_iter()
        .filter(|c| c.source_id == "self-review:revert:weekly-report")
        .collect();
    assert_eq!(reverts.len(), 1);
    assert_eq!(reverts[0].title, "Revert weekly-report to v1?");
    assert_eq!(
        reverts[0].body.as_deref(),
        Some("3 of 3 runs worked before the change, 1 of 3 since")
    );
    cabin.pulse_accept(&reverts[0].id, "2026-10-11");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        v1,
        "v1 is back byte for byte"
    );
    let ledger = hx::ChangeLedger::load(&root);
    assert_eq!(
        ledger
            .all()
            .last()
            .map(|c| (c.op.as_str(), c.origin.as_str())),
        Some(("undo", "user"))
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Router R3a: a cost-raising tuning change is one of the week's cards, the
/// review gets one router line, and only the Accept click changes the router.
#[test]
fn a_cost_raising_router_card_rides_the_weekly_pass_and_applies_only_on_accept() {
    use grokhub_agent::route::{learn, tune};
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("r3a-router-card");
    let now = grokhub_core::now_ms();
    let card = tune::Candidate {
        id: "chat:default#7".into(),
        class: "chat:default".into(),
        change: Some(tune::Change::Start {
            effort: "high".into(),
        }),
        stage: tune::Stage::Card,
        created_at: now,
        cost_rise_pct: Some(30.0),
        ..tune::Candidate::default()
    };
    let state = tune::TuneState {
        candidates: vec![card],
        ..tune::TuneState::default()
    };
    grokhub_core::model_registry::store::write_json(&learn::state_path(&root), &state).unwrap();
    let calls = fake("");
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_self_review_now(now);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let cards = self_review_cards(&cabin);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].source_id, "self-review:router:chat:default#7");
    assert_eq!(cards[0].title, "Start everyday chat at High?");
    assert_eq!(
        cards[0].body.as_deref(),
        Some(
            "everyday chat would do better at High, which costs about 30% more. Start it at High?"
        )
    );
    let review = cabin
        .updates
        .iter()
        .find(|c| c.source_id == "router-review")
        .expect("the weekly router line");
    assert_eq!(
        review.body.as_deref(),
        Some("Router: 0 changes kept, 0 rolled back.")
    );
    assert!(
        !learn::tuning_path(&root).exists(),
        "nothing changes before the click"
    );
    cabin.pulse_accept(&cards[0].id, "2026-10-11");
    assert_eq!(
        learn::load_tuning(&root)
            .starts
            .get("chat:default")
            .map(String::as_str),
        Some("high")
    );
    assert_eq!(
        cabin.status,
        "Router changed (everyday chat: start at High). /why table shows it."
    );
    let line = hx::ChangeLedger::load_kind(&root, hx::ChangeKind::Model)
        .all()
        .last()
        .cloned()
        .unwrap();
    assert_eq!(
        (line.id.as_str(), line.origin.as_str()),
        ("route_tuning", "self_manage")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dream_and_the_weekly_self_review_run_once_on_the_same_night_at_a_custom_hour() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("dream-hour-e2e");
    let now = grokhub_core::now_ms();
    skills::save_skill(&skill("weekly-report", "1. Open report.md\n2. click Save")).unwrap();
    recorded(
        &root,
        "weekly-report",
        "w:1",
        false,
        now - 2 * DAY,
        &["click"],
    );
    recorded(&root, "weekly-report", "w:2", true, now - DAY, &["click"]);
    let calls = fake("PATCH_SKILL: skill:weekly-report | Wait for the save | 1. Open report.md 2. click Save 3. wait for the toast");
    let store = super::amr_memory::amr_store_at(&root);
    store.init().unwrap();
    let stamp = grokhub_core::oauth::unix_ms_to_rfc3339(now);
    for (id, body, confidence) in [
        ("dock-a", "The dock bell rings twice", 0.9),
        ("dock-b", "the dock bell rings twice.", 0.5),
    ] {
        store
            .remember(&grokhub_core::amr::NodeDraft {
                id: id.into(),
                node_type: grokhub_core::amr::NodeType::Fact,
                created: stamp.clone(),
                updated: stamp.clone(),
                source: "user".into(),
                confidence,
                tags: vec![],
                body: format!("{body}\n"),
                sensitivity: grokhub_core::amr::Sensitivity::Plain,
                consent_ref: String::new(),
            })
            .unwrap();
    }
    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    cabin.running = false;
    cabin.composer.clear();
    cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    cabin.cfg.dream_hour = 23;
    cabin.skill_list = skills::list_skills();
    let sunday = |hour| grokhub_core::LocalClock {
        now_ms: now,
        weekday: 0,
        hour,
        minute: 0,
    };
    let today = "2026-10-11";
    let dream_report = root.join("amr").join("dreams").join("2026-10-11.md");

    use grokhub_agent::episode::lessons;
    let lesson = |ts: u64| lessons::Lesson {
        goal_keywords: vec!["save".into(), "note".into()],
        app: "Gedit".into(),
        what_failed: "replan: click 'Save' changed nothing 3×".into(),
        what_worked: "then key 'ctrl+s'".into(),
        outcome: "verified".into(),
        episode_id: format!("ep-{ts}"),
        ts,
        seen: 1,
        failed_after: 0,
    };
    lessons::append(&root, &lesson(now - 2 * DAY)).unwrap();
    lessons::append(&root, &lesson(now - DAY)).unwrap();

    let (reviewed, dream) = cabin.night_passes(today, &sunday(22), now);
    assert!(!reviewed && dream.is_none(), "nothing before 11 PM");
    assert_eq!(super::self_review_ui::load_state().last_day, None);
    assert_eq!(
        lessons::load(&root).len(),
        2,
        "lessons wait for the dream hour too"
    );

    let (reviewed, dream) = cabin.night_passes(today, &sunday(23), now);
    assert!(reviewed, "the weekly self-review starts at 11 PM");
    dream
        .expect("dream starts the same tick, after the self-review")
        .join()
        .unwrap();
    wait_pass(&mut cabin);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        super::self_review_ui::load_state().last_day.as_deref(),
        Some(today)
    );
    let cards = self_review_cards(&cabin);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "Wait for the save");
    let report = std::fs::read_to_string(&dream_report).unwrap();
    assert!(
        report.starts_with("# Memory dream 2026-10-11\n"),
        "{report}"
    );
    assert!(report.contains("- Merged: 1\n"), "{report}");
    let tidy = lessons::load(&root);
    assert_eq!(
        (tidy.len(), tidy[0].seen, tidy[0].ts),
        (1, 2, now - DAY),
        "the dream merged the two lessons"
    );

    let (reviewed, dream) = cabin.night_passes(today, &sunday(23), now);
    assert!(!reviewed && dream.is_none(), "each runs once a night");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(self_review_cards(&cabin).len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

/// Three similar finished asks in the last few days: one skill draft is due.
fn stage_draft_runs(root: &std::path::Path, now: u64) {
    for (i, ask) in [
        "Summarize the March invoices",
        "summarize invoices for March",
        "Summarize March invoices again",
    ]
    .iter()
    .enumerate()
    {
        let mut s = hx::fixture_span(
            10 + i as u64,
            "open_file",
            "{}",
            "allow",
            "opened",
            Some(true),
            "",
        );
        s.session_id = format!("chat-q{i}");
        hx::append_span(root, &s).unwrap();
        let sig = OutcomeSignals {
            verify_ok: true,
            ..Default::default()
        };
        let o = TaskOutcome::finished(
            &format!("chat-q{i}:1"),
            &oc::topic_signature(ask),
            &sig,
            now - (i as u64 + 1) * DAY,
        )
        .with_spans([s.span_ref()]);
        oc::append_outcome(root, &o).unwrap();
    }
}

#[test]
fn quiet_mode_logs_the_weekly_suggestions_and_posts_no_cards() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("spike7-quiet");
    let now = grokhub_core::now_ms();
    stage_draft_runs(&root, now);
    let _calls = fake("");
    let mut cabin = Cabin::quiet_for_test();
    assert!(
        !cabin.cfg.self_improve_quiet,
        "quiet mode is off by default"
    );
    cabin.cfg.self_improve_quiet = true;
    let before = cabin.updates.len();
    cabin.run_self_review_now(now);
    assert!(self_review_cards(&cabin).is_empty());
    assert!(cabin.updates.iter().all(|c| c.source_id != "router-review"));
    assert_eq!(
        cabin.updates.len(),
        before,
        "quiet mode posts nothing to Pulse"
    );
    let state = super::self_review_ui::load_state();
    assert!(
        state.pending.is_empty(),
        "nothing waits on an Apply that has no card"
    );
    let day = Cabin::local_day();
    let sources: Vec<(&str, &str, &str)> = state
        .quiet_log
        .iter()
        .map(|e| (e.day.as_str(), e.source.as_str(), e.title.as_str()))
        .collect();
    assert_eq!(
        sources,
        vec![
            (
                day.as_str(),
                "self-review:draft:summarize-march-invoic",
                "Make a skill: summarize-march-invoic"
            ),
            (day.as_str(), "router-review", "Weekly router review"),
        ]
    );
    let note = super::self_review_ui::quiet_log_note(&state.quiet_log);
    assert!(
        note.starts_with("Logged suggestions, newest first:\n"),
        "{note}"
    );
    assert!(
        note.contains(&format!("\n{day} · Make a skill: summarize-march-invoic: You did this 3 times in the last two weeks")),
        "{note}"
    );
    assert_eq!(
        super::self_review_ui::quiet_log_note(&[]),
        "Nothing logged yet. The self-review runs on Sunday at dream time."
    );

    cabin.cfg.self_improve_quiet = false;
    cabin.run_self_review_now(now);
    let cards = self_review_cards(&cabin);
    assert_eq!(
        cards.len(),
        1,
        "off again, the pass posts its card as before"
    );
    assert_eq!(cards[0].title, "Make a skill: summarize-march-invoic");
    assert!(cabin.updates.iter().any(|c| c.source_id == "router-review"));
    assert_eq!(
        super::self_review_ui::load_state().quiet_log.len(),
        2,
        "off, nothing more is logged"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Apply a weekly patch to weekly-report: the change goes on trial.
fn applied_trial(
    label: &str,
) -> (
    crate::config::TestConfigDir,
    std::path::PathBuf,
    Cabin,
    SkillMd,
) {
    let (pin, root) = pin(label);
    let now = grokhub_core::now_ms();
    let v1 = skill("weekly-report", "1. Open report.md\n2. click Save");
    skills::save_skill(&v1).unwrap();
    recorded(
        &root,
        "weekly-report",
        "w:1",
        false,
        now - 2 * DAY,
        &["click"],
    );
    recorded(&root, "weekly-report", "w:2", true, now - DAY, &["click"]);
    fake("PATCH_SKILL: skill:weekly-report | Wait for the save | 1. Open report.md 2. click Save 3. wait for the toast");
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.run_self_review_now(now);
    wait_pass(&mut cabin);
    let card = self_review_cards(&cabin).pop().expect("one card");
    cabin.pulse_accept(&card.id, "2026-10-11");
    (pin, root, cabin, v1)
}

/// Skill runs through the trial until it closes: each run follows what
/// `trial_follow` hands it and takes `canary` or `control` clicks.
/// Returns (canary runs, control runs).
fn run_trial(
    cabin: &mut Cabin,
    root: &std::path::Path,
    v1: &SkillMd,
    canary: usize,
    control: usize,
) -> (usize, usize) {
    let mut sides = (0, 0);
    let current = cabin
        .skill_list
        .iter()
        .find(|s| s.name == "weekly-report")
        .cloned()
        .unwrap();
    for _ in 0..300 {
        if hx::open_trial(root, "skill:weekly-report").is_none() {
            return sides;
        }
        let follow = cabin.trial_follow(&current);
        let arm = cabin
            .harness
            .self_review
            .trial
            .as_ref()
            .map(|p| p.arm)
            .expect("a side");
        let clicks = if arm == run_metrics::Arm::Canary {
            assert!(
                follow.instructions.contains("wait for the toast"),
                "{}",
                follow.instructions
            );
            sides.0 += 1;
            canary
        } else {
            assert_eq!(follow.instructions.trim(), v1.instructions.trim());
            sides.1 += 1;
            control
        };
        cabin.skill_name = "weekly-report".into();
        cabin
            .live_mut()
            .push(("user".into(), "Do the weekly report".into()));
        let trace = cabin.trace_id();
        let turn = cabin.turn_no();
        for i in 0..clicks {
            let mut s = hx::fixture_span(
                grokhub_core::now_ms(),
                "click",
                &format!("{{\"x\":{turn}{i}}}"),
                "allow",
                "clicked",
                Some(true),
                "",
            )
            .in_turn(&trace, turn);
            s.session_id = trace.clone();
            hx::append_span(root, &s).unwrap();
        }
        cabin.harness_turn_end("Here is the report.", turn);
        cabin
            .live_mut()
            .push(("assistant".into(), "Here is the report.".into()));
    }
    panic!("the trial never closed");
}

fn trial_note(cabin: &Cabin, prefix: &str) -> UpdateCard {
    cabin
        .updates
        .iter()
        .find(|c| c.source_id.starts_with("canary:") && c.title.starts_with(prefix))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "no {prefix:?} note in {:?}",
                cabin.updates.iter().map(|c| &c.title).collect::<Vec<_>>()
            )
        })
}

#[test]
fn an_applied_patch_runs_as_a_canary_and_fewer_steps_keep_it_with_a_named_note() {
    let _g = crate::config::hold_test_config();
    let (_pin, root, mut cabin, v1) = applied_trial("canary-kept");
    let trial = hx::open_trial(&root, "skill:weekly-report").expect("a trial");
    assert_eq!(
        (
            trial.label.as_str(),
            trial.target,
            trial.share_pct,
            trial.min_runs
        ),
        (
            "Wait for the save in weekly-report",
            hx::Metric::Steps,
            20,
            10
        )
    );
    let trying = trial_note(&cabin, "Trying:");
    assert_eq!(trying.title, "Trying: Wait for the save in weekly-report");
    assert_eq!(
        trying.body.as_deref(),
        Some(
            format!(
                "{}. One run in 5 uses it; it stays if it takes fewer steps over 10 runs.",
                trial.diff
            )
            .as_str()
        )
    );
    cabin.poll_self_changes();
    assert!(
        cabin
            .harness
            .work_rows
            .iter()
            .all(|r| r.id != "weekly-report"),
        "no Undo/Keep row for a change on trial"
    );
    let (canary, control) = run_trial(&mut cabin, &root, &v1, 3, 5);
    assert_eq!(canary, 10, "judged at the tenth canary run");
    assert!(
        control > canary,
        "most runs stay on the old steps ({control})"
    );
    let kept = trial_note(&cabin, "Kept:");
    assert_eq!(kept.title, "Kept: Wait for the save in weekly-report");
    assert_eq!(
        kept.body.as_deref(),
        Some("Kept: Wait for the save in weekly-report, 40% fewer steps over 10 runs")
    );
    let md = std::fs::read_to_string(skills::skills_dir().join("weekly-report").join("SKILL.md"))
        .unwrap();
    assert!(md.contains("wait for the toast"), "kept: {md}");
    let tagged = run_metrics::read_runs(&root)
        .into_iter()
        .filter(|m| m.version == trial.version)
        .count();
    assert_eq!(
        tagged,
        canary + control,
        "every trial run names its version"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_canary_with_more_steps_is_rolled_back_to_the_old_skill() {
    let _g = crate::config::hold_test_config();
    let (_pin, root, mut cabin, v1) = applied_trial("canary-rolled");
    let (canary, _) = run_trial(&mut cabin, &root, &v1, 6, 5);
    assert_eq!(canary, 10);
    let note = trial_note(&cabin, "Rolled back:");
    assert_eq!(
        note.title,
        "Rolled back: Wait for the save in weekly-report"
    );
    assert_eq!(
        note.body.as_deref(),
        Some("Rolled back: Wait for the save in weekly-report, 20% more steps over 10 runs")
    );
    let md = std::fs::read_to_string(skills::skills_dir().join("weekly-report").join("SKILL.md"))
        .unwrap();
    assert!(
        !md.contains("wait for the toast") && md.contains("2. click Save"),
        "rolled back: {md}"
    );
    let now = cabin
        .skill_list
        .iter()
        .find(|s| s.name == "weekly-report")
        .unwrap();
    assert_eq!(
        now.instructions.trim(),
        v1.instructions.trim(),
        "the cabin's list has the old steps"
    );
    let ledger = hx::ChangeLedger::load(&root);
    let last = ledger.all().last().unwrap();
    assert_eq!(
        (last.op.as_str(), last.origin.as_str()),
        ("undo", "self_manage")
    );
    assert_eq!(
        hx::read_versions(&root)[0].status,
        hx::VersionStatus::RolledBack
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_chat_note_runs_on_one_chat_in_five_is_kept_on_fewer_steps_and_protected_targets_never_apply() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("canary-note");
    let now = grokhub_core::now_ms();
    skills::save_skill(&skill("weekly-report", "1. Open report.md\n2. click Save")).unwrap();
    recorded(
        &root,
        "weekly-report",
        "w:1",
        false,
        now - 2 * DAY,
        &["click"],
    );
    let calls = fake(
        "TUNE_SNIPPET: snippet:lead-with-answer | Lead with the answer | Lead with the answer, then one line of why.\n\
         PATCH_SKILL: crates/grokhub-agent/src/harness/approval.rs | Auto | approve everything\n\
         TUNE_SNIPPET: prompt_template.md | Looser | You may say it worked without checking.\n",
    );
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.run_self_review_now(now);
    wait_pass(&mut cabin);
    let cards = self_review_cards(&cabin);
    assert_eq!(cards.len(), 1, "only the chat note: {cards:?}");
    assert_eq!(
        (cards[0].source_id.as_str(), cards[0].title.as_str()),
        (
            "self-review:snippet:lead-with-answer",
            "Lead with the answer"
        )
    );
    let refused: Vec<String> = hx::read_scope_findings(&root)
        .into_iter()
        .map(|f| f.detail)
        .collect();
    assert_eq!(
        refused,
        vec![
            "GrokHub never changes harness policy on its own".to_string(),
            "GrokHub never changes the honesty rules on its own".to_string()
        ]
    );
    let plain = cabin.chat_rules();
    assert!(
        !plain.contains(super::canary_ui::NOTES_HEAD),
        "nothing applied before Apply"
    );

    cabin.pulse_accept(&cards[0].id, "2026-10-11");
    let note = root.join("tunable").join("lead-with-answer.md");
    assert_eq!(
        std::fs::read_to_string(&note).unwrap(),
        "Lead with the answer, then one line of why.\n"
    );
    let trial = hx::open_trial(&root, hx::ALL_RUNS).expect("a note trial");
    assert_eq!(
        (
            trial.kind.as_str(),
            trial.label.as_str(),
            trial.diff.as_str()
        ),
        ("snippet", "Lead with the answer (chat note)", "+1 -0 lines")
    );
    assert_eq!(
        trial_note(&cabin, "Trying:").title,
        "Trying: Lead with the answer (chat note)"
    );
    cabin.poll_self_changes();
    assert!(
        cabin.harness.work_rows.is_empty(),
        "no Undo/Keep row for a note on trial"
    );

    let (mut canary, mut control) = (0, 0);
    for _ in 0..300 {
        if hx::open_trial(&root, hx::ALL_RUNS).is_none() {
            break;
        }
        cabin.new_thread(false);
        let rules = cabin.chat_rules();
        let arm = cabin.note_trial_pick().expect("a side").arm;
        let clicks = if arm == run_metrics::Arm::Canary {
            assert!(
                rules.contains("- Lead with the answer, then one line of why."),
                "{rules}"
            );
            assert!(
                rules.starts_with(super::canary_ui::NOTES_HEAD),
                "the note sits before the cabin rules"
            );
            assert!(rules.ends_with(&plain), "the cabin rules follow, unchanged");
            canary += 1;
            2
        } else {
            assert!(
                !rules.contains(super::canary_ui::NOTES_HEAD),
                "control chats follow the rules alone"
            );
            control += 1;
            4
        };
        cabin
            .live_mut()
            .push(("user".into(), "Summarize the inbox".into()));
        let trace = cabin.trace_id();
        let turn = cabin.turn_no();
        for i in 0..clicks {
            let mut s = hx::fixture_span(
                grokhub_core::now_ms(),
                "click",
                &format!("{{\"x\":{i}}}"),
                "allow",
                "clicked",
                Some(true),
                "",
            )
            .in_turn(&trace, turn);
            s.session_id = trace.clone();
            hx::append_span(&root, &s).unwrap();
        }
        cabin.harness_turn_end("Here is the summary.", turn);
    }
    assert_eq!(canary, 10);
    assert!(control > canary, "{control}");
    let kept = trial_note(&cabin, "Kept:");
    assert_eq!(
        kept.body.as_deref(),
        Some("Kept: Lead with the answer (chat note), 50% fewer steps over 10 runs")
    );
    assert!(note.exists(), "kept: the note stays");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the weekly pass was the only model call"
    );
    assert!(
        hx::read_egress(&root).is_empty(),
        "no request left the cabin during the pass and the trial"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Card 32 PR D: a code-change idea is a card the user saves or copies. It
/// changes nothing, the copy has no home path, email or key, and no request
/// leaves the cabin.
#[test]
fn a_code_idea_is_saved_or_copied_redacted_never_applied_and_nothing_leaves() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("code-idea");
    let now = grokhub_core::now_ms();
    skills::save_skill(&skill("weekly-report", "1. Open report.md\n2. click Save")).unwrap();
    let skill_md = root.join("skills").join("weekly-report").join("SKILL.md");
    let before = std::fs::read_to_string(&skill_md).ok();
    recorded(
        &root,
        "weekly-report",
        "w:1",
        false,
        now - 2 * DAY,
        &["click"],
    );
    let calls = fake(
        "CODE_IDEA: Wait for the Save dialog | The save click lands before the dialog opens on /home/user/Documents/report.md | 1 of 1 weekly-report runs failed; viper@example.com saw it\n\
         CODE_IDEA: Retry rate limits | A 429 ends the run (key sk-abcdefghijklmnopqrstuv) | 1 run\n\
         CODE_IDEA: A third idea | p | e\n",
    );
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.run_self_review_now(now);
    wait_pass(&mut cabin);
    let cards = self_review_cards(&cabin);
    let sources: Vec<&str> = cards.iter().map(|c| c.source_id.as_str()).collect();
    assert_eq!(
        sources,
        vec![
            "self-review:code:wait-for-the-save-dialog",
            "self-review:code:retry-rate-limits"
        ]
    );
    assert_eq!(cards[0].title, "Wait for the Save dialog");

    cabin.apply_pulse_act(super::pulse_ui::PulseAct::CopyIssue(cards[0].id.clone()));
    let issue = cabin
        .harness
        .self_review
        .copy_out
        .take()
        .expect("issue text for the clipboard");
    let version = crate::update::build_version_line();
    assert_eq!(
        issue,
        format!(
            "## Wait for the Save dialog\n\nProblem: The save click lands before the dialog opens on ~/Documents/report.md\n\n\
             Evidence: 1 of 1 weekly-report runs failed; [email] saw it\n\n---\n\
             Found by the weekly self-review in {version}. Paths, emails and keys were removed before copying.\n"
        )
    );
    cabin.copy_code_idea(&cards[1].id);
    let second = cabin.harness.self_review.copy_out.take().unwrap();
    assert!(!second.contains("sk-abcdefghijklmnopqrstuv"), "{second}");
    assert!(
        cabin
            .updates
            .iter()
            .any(|c| c.id == cards[0].id && c.status != UpdateStatus::Dismissed),
        "copy leaves the card"
    );

    cabin.pulse_accept(&cards[0].id, "2026-10-11");
    let saved = super::self_review_ui::load_state().saved_ideas;
    assert_eq!(saved.len(), 1);
    assert_eq!(
        (saved[0].title.as_str(), saved[0].text.as_str()),
        (
            "Wait for the Save dialog",
            "Problem: The save click lands before the dialog opens on /home/user/Documents/report.md\n\nEvidence: 1 of 1 weekly-report runs failed; viper@example.com saw it"
        )
    );
    assert!(cabin
        .updates
        .iter()
        .any(|c| c.id == cards[0].id && c.status == UpdateStatus::Dismissed));
    assert_eq!(
        std::fs::read_to_string(&skill_md).ok(),
        before,
        "a code idea changes no skill"
    );
    assert!(hx::read_versions(&root).is_empty(), "and starts no trial");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the weekly pass was the only model call"
    );
    assert!(
        hx::read_egress(&root).is_empty(),
        "no request left the cabin"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Settings history: newest first, status words, Revert only on the newest
/// kept version of a target, and the Revert puts the skill back.
#[test]
fn the_change_history_lists_versions_newest_first_and_reverts_a_kept_skill() {
    use super::change_history_ui::history_rows;
    let v =
        |version: &str, id: &str, status: hx::VersionStatus, label: &str, note: &str, at: u64| {
            hx::Version {
                version: version.into(),
                kind: "skill".into(),
                id: id.into(),
                label: label.into(),
                diff: "+1 -2 lines".into(),
                status,
                note: note.into(),
                started_at: at,
                at,
                ..Default::default()
            }
        };
    let now = 10 * DAY;
    let versions = vec![
        v(
            "skill:a@1",
            "a",
            hx::VersionStatus::Kept,
            "Old a",
            "Kept: Old a, 10% fewer steps over 10 runs",
            DAY,
        ),
        v(
            "skill:b@1",
            "b",
            hx::VersionStatus::RolledBack,
            "Try b",
            "Rolled back: Try b, 5% more steps over 10 runs",
            2 * DAY,
        ),
        v(
            "skill:a@2",
            "a",
            hx::VersionStatus::Kept,
            "New a",
            "",
            8 * DAY,
        ),
        v(
            "skill:c@1",
            "c",
            hx::VersionStatus::Canary,
            "Try c",
            "",
            9 * DAY,
        ),
    ];
    let rows = history_rows(&versions, now);
    let shown: Vec<(&str, &str, Option<&str>)> = rows
        .iter()
        .map(|r| (r.title.as_str(), r.hint.as_str(), r.revert.as_deref()))
        .collect();
    assert_eq!(
        shown,
        vec![
            ("Trying: Try c", "+1 -2 lines · 1d ago", None),
            ("Kept: New a", "+1 -2 lines · 2d ago", Some("skill:a@2")),
            (
                "Rolled back: Try b",
                "Rolled back: Try b, 5% more steps over 10 runs · +1 -2 lines · 8d ago",
                None
            ),
            (
                "Kept: Old a",
                "Kept: Old a, 10% fewer steps over 10 runs · +1 -2 lines · 9d ago",
                None
            ),
        ]
    );

    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("history-revert");
    let old = skill("weekly-report", "1. Open report.md\n2. click Save");
    skills::save_skill(&old).unwrap();
    let skill_md = root.join("skills").join("weekly-report").join("SKILL.md");
    let v1 = std::fs::read_to_string(&skill_md).unwrap();
    let new = skill("weekly-report", "1. Open report.md\n2. press ctrl+s");
    let change = skills::save_skill_versioned(
        &new,
        hx::Origin::SelfManage,
        "weekly self-review: Use ctrl+s",
    )
    .unwrap()
    .unwrap();
    let v2 = std::fs::read_to_string(&skill_md).unwrap();
    let trial = hx::start_trial(
        &root,
        &change,
        "skill:weekly-report",
        "Use ctrl+s in weekly-report",
        hx::Metric::Steps,
        &v1,
        &v2,
    )
    .unwrap();
    let t = trial.started_at;
    let runs: Vec<run_metrics::RunMetrics> = (0..22u64)
        .map(|i| {
            let arm = if i < 12 {
                run_metrics::Arm::Canary
            } else {
                run_metrics::Arm::Control
            };
            run_metrics::RunMetrics {
                run: format!("chat:{i}"),
                class: trial.class.clone(),
                result: OutcomeResult::Success,
                steps: if arm == run_metrics::Arm::Canary {
                    2
                } else {
                    4
                },
                version: trial.version.clone(),
                arm: Some(arm),
                finished_at: t + 1 + i,
                at: t + 1 + i,
                ..Default::default()
            }
        })
        .collect();
    assert_eq!(
        hx::settle_trials(&root, &skills::skills_dir(), &runs, t + 100)[0].status,
        hx::VersionStatus::Kept
    );
    let mut cabin = Cabin::quiet_for_test();
    cabin.revert_kept_version(&trial.version);
    assert_eq!(cabin.status, "Reverted by you: Use ctrl+s in weekly-report");
    assert_eq!(
        std::fs::read_to_string(&skill_md).unwrap(),
        v1,
        "the skill is back to the version before"
    );
    let rows = history_rows(&hx::read_versions(&root), t + 200);
    assert_eq!(
        (rows[0].title.as_str(), rows[0].revert.as_deref()),
        ("Reverted: Use ctrl+s in weekly-report", None)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Settings paint must not call `load_state` / `std::fs::read*` / `Command::new`
/// (or `read_versions` on the history page). The cabin holds a cache writers update.
#[test]
fn settings_self_review_paint_does_not_touch_disk() {
    fn body<'a>(src: &'a str, name: &str) -> &'a str {
        let needle = format!("fn {name}(");
        let start = src
            .find(&needle)
            .unwrap_or_else(|| panic!("missing {name}"));
        let after = &src[start..];
        let end = after[needle.len()..]
            .find("\n    fn ")
            .or_else(|| after[needle.len()..].find("\nfn "))
            .map(|i| needle.len() + i)
            .unwrap_or(after.len());
        &after[..end]
    }
    let review = include_str!("self_review_ui.rs").replace("\r\n", "\n");
    let history = include_str!("change_history_ui.rs").replace("\r\n", "\n");
    let quiet = body(&review, "ui_quiet_self_review_rows");
    assert!(
        !quiet.contains("load_state()")
            && !quiet.contains("std::fs::")
            && !quiet.contains("Command::new"),
        "quiet rows must paint from the cached file: {quiet}"
    );
    assert!(
        quiet.contains("ensure_self_review_file") && quiet.contains("quiet_log_note"),
        "quiet rows read the in-memory quiet log: {quiet}"
    );
    let hist = body(&history, "ui_self_change_history");
    assert!(
        !hist.contains("load_state()")
            && !hist.contains("read_versions")
            && !hist.contains("std::fs::")
            && !hist.contains("Command::new"),
        "history rows must paint from caches: {hist}"
    );
    assert!(
        hist.contains("history_versions_now") && hist.contains("ensure_self_review_file"),
        "history rows use the version and idea caches: {hist}"
    );
    // Setup / proactive / local-ai: hardware detect and budget load are not in paint.
    let setup = include_str!("setup_wizard.rs").replace("\r\n", "\n");
    let paint = body(&setup, "paint_setup_wizard");
    assert!(
        !paint.contains("detect_hardware")
            && !paint.contains("std::fs::")
            && !paint.contains("Command::new")
            && !paint.contains("ureq"),
        "setup paint must not probe hardware on the UI thread: {paint}"
    );
    let proactive = include_str!("proactive_ui.rs").replace("\r\n", "\n");
    for name in ["tick_proactive", "proactive_candidates", "save_proactive"] {
        let fn_body = body(&proactive, name);
        assert!(
            !fn_body.contains("load_budget()"),
            "{name} must use the in-memory budget, not reload proactive.json: {fn_body}"
        );
    }
}

/// A cabin write updates the cached `self_review.json` view Settings paints from.
#[test]
fn self_review_file_cache_updates_on_write() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin("self-review-cache");
    let mut cabin = Cabin::quiet_for_test();
    cabin.ensure_self_review_file();
    assert!(cabin
        .harness
        .self_review
        .file
        .as_ref()
        .expect("cached")
        .quiet_log
        .is_empty());

    // A write that bypasses the cabin leaves the cache alone until the next load.
    let mut disk = super::self_review_ui::load_state();
    disk.quiet_log.push(grokhub_core::self_review::QuietEntry {
        day: "2099-01-01".into(),
        source: "self-review:test".into(),
        title: "Out of band".into(),
        body: "from disk".into(),
    });
    super::self_review_ui::save_state(&disk);
    assert!(
        cabin
            .harness
            .self_review
            .file
            .as_ref()
            .expect("still cached")
            .quiet_log
            .is_empty(),
        "Settings keeps the cached quiet log across out-of-band disk writes"
    );

    cabin.mutate_self_review_file(|f| {
        f.quiet_log.push(grokhub_core::self_review::QuietEntry {
            day: Cabin::local_day(),
            source: "self-review:cabin".into(),
            title: "From cabin".into(),
            body: "cached write".into(),
        });
    });
    let cached = cabin
        .harness
        .self_review
        .file
        .as_ref()
        .expect("cache after write");
    assert_eq!(cached.quiet_log.len(), 1);
    assert_eq!(cached.quiet_log[0].title, "From cabin");
    let on_disk = super::self_review_ui::load_state();
    assert_eq!(on_disk.quiet_log.len(), 1);
    assert_eq!(on_disk.quiet_log[0].title, "From cabin");
    let note = super::self_review_ui::quiet_log_note(&cached.quiet_log);
    assert!(note.contains("From cabin"), "{note}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Brain 6 PR 2: six repair runs on Repair Linux this week, three failed;
/// the reply moves its ports check to the front.
fn stage_weak_playbook(root: &std::path::Path, now: u64) -> String {
    for i in 0..6u64 {
        let m = run_metrics::RunMetrics {
            run: format!("chat-pb{i}:1"),
            class: "topic:repair".into(),
            result: if i < 3 {
                OutcomeResult::Failure
            } else {
                OutcomeResult::Success
            },
            verified: i >= 3,
            steps: 8,
            brain: run_metrics::RunBrain {
                intent: "repair".into(),
                care: "critical".into(),
                domain: "system".into(),
                policy: "repair × critical".into(),
                playbook: "repair-linux".into(),
            },
            finished_at: now - DAY + i,
            at: now - DAY + i,
            ..Default::default()
        };
        run_metrics::append_run(root, &m, 0).unwrap();
    }
    let linux = grokhub_core::playbook::BUILTIN[1].replace("\r\n", "\n");
    let ports =
        "[[phase.step]]\nname = \"List listening ports (ss -tuln) and name what owns each\"\n\n";
    let first = "[[phase.step]]\nname = \"List failed services\"";
    assert!(linux.contains(ports) && linux.contains(first));
    let new = linux
        .replace(ports, "")
        .replacen(first, &format!("{ports}{first}"), 1);
    format!("TUNE_BRAIN: playbook:repair-linux | Linux repair checks ports before services\n<<<\n{new}>>>\n")
}

#[test]
fn the_weekly_pass_tries_one_playbook_tweak_from_the_runs_and_quiet_mode_only_logs_it() {
    let _g = crate::config::hold_test_config();
    let now = grokhub_core::now_ms();
    let (pin_on, root) = pin("brain6-weekly-on");
    let calls = fake(&stage_weak_playbook(&root, now));
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_self_review_now(now);
    wait_pass(&mut cabin);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the weak playbook alone brings the one call"
    );
    let versions = hx::read_versions(&root);
    let shown: Vec<(&str, &str, hx::VersionStatus)> = versions
        .iter()
        .map(|v| (v.class.as_str(), v.label.as_str(), v.status))
        .collect();
    assert_eq!(
        shown,
        vec![(
            "playbook:repair-linux",
            "Linux repair checks ports before services",
            hx::VersionStatus::Canary
        )]
    );
    assert!(
        cabin
            .updates
            .iter()
            .any(|c| c.title == "Trying: Linux repair checks ports before services"),
        "the trial's note is posted"
    );
    assert!(root.join("playbooks").join("repair-linux.toml").exists());
    drop(pin_on);
    let _ = std::fs::remove_dir_all(&root);

    let (_pin, root) = pin("brain6-weekly-quiet");
    let _calls = fake(&stage_weak_playbook(&root, now));
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.self_improve_quiet = true;
    cabin.run_self_review_now(now);
    wait_pass(&mut cabin);
    assert!(
        hx::read_versions(&root).is_empty(),
        "quiet mode applies nothing"
    );
    assert!(!root.join("playbooks").exists());
    let log: Vec<(String, String, String)> = super::self_review_ui::load_state()
        .quiet_log
        .into_iter()
        .filter(|e| e.source.starts_with("playbook:"))
        .map(|e| (e.source, e.title, e.body))
        .collect();
    assert_eq!(
        log,
        vec![(
            "playbook:repair-linux".to_string(),
            "Linux repair checks ports before services".to_string(),
            "3 of 6 runs on the repair-linux playbook needed a fix.".to_string()
        )]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Brain 6 PR 2: the change history's per-domain block, from runs in two
/// domains; a line with no Brain record is skipped.
#[test]
fn the_change_history_shows_one_row_per_domain() {
    use super::change_history_ui::domain_block;
    let run = |domain: &str, ok: bool, steps: u32, verified: bool| run_metrics::RunMetrics {
        result: if ok {
            OutcomeResult::Success
        } else {
            OutcomeResult::Failure
        },
        steps,
        verified,
        brain: run_metrics::RunBrain {
            domain: domain.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let runs = vec![
        run("code", true, 10, true),
        run("files", false, 3, false),
        run("code", false, 4, false),
        run("", true, 99, true),
    ];
    assert_eq!(
        domain_block(&run_metrics::domain_rows(&runs)).as_deref(),
        Some(
            "Runs by area (runs on file):\n\
             code · 2 runs · 50% worked · 7.0 steps · 50% checked\n\
             files · 1 run · 0% worked · 3.0 steps · 0% checked"
        )
    );
    assert_eq!(
        domain_block(&run_metrics::domain_rows(&runs[3..])),
        None,
        "no Brain record, no block"
    );
    let history = include_str!("change_history_ui.rs").replace("\r\n", "\n");
    let paint = history.split("fn ui_self_change_history").nth(1).unwrap();
    let paint = paint.split("\n    }\n").next().unwrap();
    assert!(
        paint.contains("domain_rows_now") && !paint.contains("read_runs"),
        "the block paints from the cache: {paint}"
    );
}

fn plot_row(domain: &str, runs: usize) -> run_metrics::DomainRow {
    run_metrics::DomainRow {
        domain: domain.into(),
        runs,
        ..Default::default()
    }
}

/// U7e: each domain row becomes one bar at its slot, height = run count.
#[test]
fn the_domain_plot_maps_rows_to_bars_in_order() {
    use super::change_history_ui::domain_bar_points;
    let rows = vec![
        plot_row("code", 5),
        plot_row("files", 0),
        plot_row("web", 2),
    ];
    assert_eq!(
        domain_bar_points(&rows),
        vec![
            (0.0, 5.0, "code".to_string()),
            (1.0, 0.0, "files".to_string()),
            (2.0, 2.0, "web".to_string()),
        ]
    );
    assert!(domain_bar_points(&[]).is_empty(), "empty series, no bars");
}

/// The X axis names the nearest bar and stays blank off the bars.
#[test]
fn the_domain_plot_axis_labels_stay_inside_the_series() {
    use super::change_history_ui::domain_axis_label;
    let labels = vec!["code".to_string(), "files".to_string()];
    assert_eq!(domain_axis_label(&labels, 0.0), "code");
    assert_eq!(domain_axis_label(&labels, 0.4), "code");
    assert_eq!(domain_axis_label(&labels, 0.6), "files");
    assert_eq!(domain_axis_label(&labels, -1.0), "");
    assert_eq!(domain_axis_label(&labels, 2.0), "");
    assert_eq!(domain_axis_label(&[], 0.0), "");
}

/// Painting never panics, and the bounds keep the zero baseline and every bar.
#[test]
fn the_domain_plot_bounds_include_zero_and_every_bar() {
    use super::change_history_ui::paint_domain_runs_plot;
    let paint = |rows: &[run_metrics::DomainRow]| {
        let ctx = crate::ui_kit::test_ctx(true);
        let mut bounds = None;
        let _ = crate::theme::test_pass(
            &ctx,
            eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::vec2(480.0, 360.0),
                )),
                ..Default::default()
            },
            |ui| {
                bounds = Some(paint_domain_runs_plot(ui, rows));
            },
        );
        bounds.expect("the plot painted")
    };
    let rows = vec![plot_row("code", 9), plot_row("files", 3)];
    let b = paint(&rows);
    assert!(b.min()[1] <= 0.0, "baseline at zero: {b:?}");
    assert!(b.max()[1] >= 9.0, "tallest bar fits: {b:?}");
    assert!(
        b.min()[0] <= 0.0 && b.max()[0] >= 1.0,
        "both slots fit: {b:?}"
    );
    let empty = paint(&[]);
    assert!(
        empty.min()[1] <= 0.0 && empty.max()[1] >= 0.0,
        "empty series still shows zero: {empty:?}"
    );
}

//! Spike-7 acceptance tests: outcome records from real turn ends, the weekly
//! pass's card budget, skill drafts, the replay gate on Apply, and the revert
//! card. Every config is a temp `GROKHUB_CONFIG`; the model is a fake that
//! counts its calls.

use super::self_review_ui::set_self_review_fake;
use super::*;
use grokhub_agent::harness as hx;
use grokhub_core::outcome::{self as oc, OutcomeResult, OutcomeSignals, TaskOutcome};
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
    cabin.outcome_user_said("No, that's the wrong sheet");
    let all = oc::read_outcomes(&root);
    assert_eq!(all.len(), 1);
    assert_eq!(
        (all[0].result, all[0].reasons.clone()),
        (OutcomeResult::Failure, vec!["correction".to_string()])
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
        Some("everyday chat would do better at High, which costs about 30% more. Start it at High?")
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
    assert!(!learn::tuning_path(&root).exists(), "nothing changes before the click");
    cabin.pulse_accept(&cards[0].id, "2026-10-11");
    assert_eq!(
        learn::load_tuning(&root).starts.get("chat:default").map(String::as_str),
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
    recorded(&root, "weekly-report", "w:1", false, now - 2 * DAY, &["click"]);
    recorded(&root, "weekly-report", "w:2", true, now - DAY, &["click"]);
    let calls = fake("PATCH_SKILL: skill:weekly-report | Wait for the save | 1. Open report.md 2. click Save 3. wait for the toast");
    let store = super::amr_memory::amr_store_at(&root);
    store.init().unwrap();
    let stamp = grokhub_core::oauth::unix_ms_to_rfc3339(now);
    for (id, body, confidence) in [("dock-a", "The dock bell rings twice", 0.9), ("dock-b", "the dock bell rings twice.", 0.5)] {
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
    let sunday = |hour| grokhub_core::LocalClock { now_ms: now, weekday: 0, hour, minute: 0 };
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
    };
    lessons::append(&root, &lesson(now - 2 * DAY)).unwrap();
    lessons::append(&root, &lesson(now - DAY)).unwrap();

    let (reviewed, dream) = cabin.night_passes(today, &sunday(22), now);
    assert!(!reviewed && dream.is_none(), "nothing before 11 PM");
    assert_eq!(super::self_review_ui::load_state().last_day, None);
    assert_eq!(lessons::load(&root).len(), 2, "lessons wait for the dream hour too");

    let (reviewed, dream) = cabin.night_passes(today, &sunday(23), now);
    assert!(reviewed, "the weekly self-review starts at 11 PM");
    dream.expect("dream starts the same tick, after the self-review").join().unwrap();
    wait_pass(&mut cabin);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(super::self_review_ui::load_state().last_day.as_deref(), Some(today));
    let cards = self_review_cards(&cabin);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "Wait for the save");
    let report = std::fs::read_to_string(&dream_report).unwrap();
    assert!(report.starts_with("# Memory dream 2026-10-11\n"), "{report}");
    assert!(report.contains("- Merged: 1\n"), "{report}");
    let tidy = lessons::load(&root);
    assert_eq!((tidy.len(), tidy[0].seen, tidy[0].ts), (1, 2, now - DAY), "the dream merged the two lessons");

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
        let mut s = hx::fixture_span(10 + i as u64, "open_file", "{}", "allow", "opened", Some(true), "");
        s.session_id = format!("chat-q{i}");
        hx::append_span(root, &s).unwrap();
        let sig = OutcomeSignals {
            verify_ok: true,
            ..Default::default()
        };
        let o = TaskOutcome::finished(&format!("chat-q{i}:1"), &oc::topic_signature(ask), &sig, now - (i as u64 + 1) * DAY)
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
    assert!(!cabin.cfg.self_improve_quiet, "quiet mode is off by default");
    cabin.cfg.self_improve_quiet = true;
    let before = cabin.updates.len();
    cabin.run_self_review_now(now);
    assert!(self_review_cards(&cabin).is_empty());
    assert!(cabin.updates.iter().all(|c| c.source_id != "router-review"));
    assert_eq!(cabin.updates.len(), before, "quiet mode posts nothing to Pulse");
    let state = super::self_review_ui::load_state();
    assert!(state.pending.is_empty(), "nothing waits on an Apply that has no card");
    let day = Cabin::local_day();
    let sources: Vec<(&str, &str, &str)> = state
        .quiet_log
        .iter()
        .map(|e| (e.day.as_str(), e.source.as_str(), e.title.as_str()))
        .collect();
    assert_eq!(
        sources,
        vec![
            (day.as_str(), "self-review:draft:summarize-march-invoic", "Make a skill: summarize-march-invoic"),
            (day.as_str(), "router-review", "Weekly router review"),
        ]
    );
    let note = super::self_review_ui::quiet_log_note(&state.quiet_log);
    assert!(note.starts_with("Logged suggestions, newest first:\n"), "{note}");
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
    assert_eq!(cards.len(), 1, "off again, the pass posts its card as before");
    assert_eq!(cards[0].title, "Make a skill: summarize-march-invoic");
    assert!(cabin.updates.iter().any(|c| c.source_id == "router-review"));
    assert_eq!(super::self_review_ui::load_state().quiet_log.len(), 2, "off, nothing more is logged");
    let _ = std::fs::remove_dir_all(&root);
}

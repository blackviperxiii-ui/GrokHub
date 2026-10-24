//! Heartbeat pacing and self-made change row tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

fn pace_spans() -> Vec<grokhub_agent::harness::Span> {
    grokhub_agent::harness::read_spans(
        &crate::config::config_dir(),
        super::heartbeat_gate::HEARTBEAT_TRACE,
    )
    .expect("heartbeat spans")
}

#[test]
fn heartbeat_pace_holds_while_busy_and_traces_reasons_without_content() {
    use grokhub_core::{HeartbeatPace, ProactiveAct, PACE_NORMAL};
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("heartbeat-pace-busy");
    let _ = std::fs::remove_file(grokhub_agent::harness::span_path(
        &crate::config::config_dir(),
        "heartbeat",
    ));
    const T0: u64 = 1_800_000_000_000;
    const MIN: u64 = 60_000;
    cabin.running = false;
    cabin.composer = "half a thought about the standup".into();
    assert!(cabin.heartbeat_busy());
    assert!(!cabin.heartbeat_may(ProactiveAct::Anticipate, T0));
    assert!(!cabin.heartbeat_may(ProactiveAct::Anticipate, T0 + 15_000));
    cabin.composer.clear();
    cabin.running = true;
    assert!(
        !cabin.heartbeat_may(ProactiveAct::Anticipate, T0 + 30_000),
        "mid-turn"
    );
    cabin.running = false;
    assert!(!cabin.heartbeat_busy());
    assert!(cabin.heartbeat_may(ProactiveAct::Anticipate, T0 + 45_000));
    assert!(!cabin.heartbeat_may(ProactiveAct::Ideas, T0 + 46_000));
    let spans = pace_spans();
    let lines: Vec<(&str, &str, &str)> = spans
        .iter()
        .map(|s| (s.tool.as_str(), s.decision.as_str(), s.result.as_str()))
        .collect();
    assert_eq!(
        lines,
        vec![
            ("heartbeat.anticipate", "hold", "busy"),
            ("heartbeat.anticipate", "allow", "under_budget"),
            ("heartbeat.ideas", "hold", "min_interval"),
        ],
        "a repeated hold is traced once; each act and change is traced"
    );
    for s in &spans {
        assert_eq!(s.session_id, "heartbeat");
        assert_eq!(s.args_redacted, "", "no content in a pace span");
        assert_eq!(s.claim, "pace");
        assert_eq!(s.chat_id, "");
        assert_eq!(s.path, "heartbeat");
        assert_eq!(s.origin, grokhub_agent::harness::Origin::Proactive);
    }
    let raw = std::fs::read_to_string(grokhub_agent::harness::span_path(
        &crate::config::config_dir(),
        "heartbeat",
    ))
    .expect("raw spans");
    assert!(
        !raw.contains("standup"),
        "the draft never reaches the trace: {raw}"
    );

    // Config override (as from app.json): one an hour instead of three.
    assert!(cabin.heartbeat_may(ProactiveAct::Ideas, T0 + 45_000 + 15 * MIN));
    cabin.cfg.heartbeat = HeartbeatPace {
        min_interval_min: 60,
        max_per_hour: 1,
        ..PACE_NORMAL
    };
    cabin.heartbeat_outcome(ProactiveAct::Ideas, grokhub_core::ActOutcome::Useful);
    assert!(!cabin.heartbeat_may(ProactiveAct::Review, T0 + 45_000 + 45 * MIN));
    assert_eq!(
        pace_spans().last().map(|s| s.result.clone()).as_deref(),
        Some("hour_cap")
    );
    release_isolated(&root, cabin);
}

#[test]
fn halt_stops_the_heartbeat_at_once_and_your_send_resumes_it() {
    use grokhub_core::ProactiveAct;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("heartbeat-pace-halt");
    let _ = std::fs::remove_file(grokhub_agent::harness::span_path(
        &crate::config::config_dir(),
        "heartbeat",
    ));
    let now = grokhub_core::now_ms();
    cabin.running = false;
    cabin.composer.clear();
    let (_tx, rx) = std::sync::mpsc::channel();
    cabin.ideas_rx = Some((rx, Default::default()));
    let (_rtx, rrx) = std::sync::mpsc::channel();
    cabin.review_rx = Some(rrx);
    cabin.review_busy = true;
    cabin.halt_everything("Stopped");
    assert!(
        cabin.ideas_rx.is_none(),
        "an ideas ask already out lands nowhere"
    );
    assert!(cabin.review_rx.is_none() && !cabin.review_busy);
    assert!(cabin.heartbeat_halted(now));
    assert!(!cabin.heartbeat_may(ProactiveAct::Anticipate, now));
    assert!(!cabin.heartbeat_may(ProactiveAct::Review, now + 14 * 60_000));
    let tail: Vec<(String, String)> = pace_spans()
        .iter()
        .map(|s| (s.tool.clone(), s.result.clone()))
        .collect();
    assert_eq!(
        tail,
        vec![
            ("heartbeat.halt".to_string(), "halted".to_string()),
            ("heartbeat.anticipate".to_string(), "halted".to_string()),
            ("heartbeat.review".to_string(), "halted".to_string()),
        ]
    );
    cabin.heartbeat_user_sent();
    assert!(!cabin.heartbeat_halted(now));
    assert!(cabin.heartbeat_may(ProactiveAct::Anticipate, now));
    // Stop on that live anticipate turn counts as a dismissal.
    cabin.running = true;
    cabin.heartbeat_turn_stopped();
    assert_eq!(cabin.pace.quiet_streak(), 1);
    assert_eq!(cabin.pace.pending(), None);
    release_isolated(&root, cabin);
}

#[test]
fn heartbeat_halt_skips_every_organ_that_starts_work() {
    let src = cabin_src();
    let beat = src
        .split("fn tick_heartbeat")
        .nth(1)
        .and_then(|s| s.split("fn tick_anticipate").next())
        .expect("tick_heartbeat");
    assert!(
        beat.contains("heartbeat_halted(") && beat.contains("runs_while_halted()"),
        "Halt must hold the pulse before any organ runs: {beat}"
    );
    let digest = beat
        .split("JobKind::Digest =>")
        .nth(1)
        .and_then(|s| s.split("JobKind::Review =>").next())
        .expect("digest job");
    assert!(
        digest.contains("self.follow_feed_lookup();") && beat.contains("self.governor.tick("),
        "the digest lookup runs only from the governor's queue: {beat}"
    );
    let spec = grokhub_core::Governor::cabin();
    let spec = spec
        .spec(grokhub_core::JobKind::Digest)
        .expect("digest spec");
    assert!(
        !spec.while_halted,
        "the digest lookup is a model call and waits out a Halt"
    );
    let anticipate = fn_src(&src, "tick_anticipate");
    let gate = anticipate
        .find("heartbeat_may(")
        .expect("anticipate meets the pace gate");
    let fire = anticipate
        .find("send_scheduled_chat(")
        .expect("anticipate send");
    assert!(gate < fire, "the gate comes before the turn: {anticipate}");
    let review = src
        .split("fn tick_review(")
        .nth(1)
        .and_then(|s| s.split("fn review_chat_digest(").next())
        .expect("tick_review");
    let gate = review
        .find("ProactiveAct::Review")
        .expect("review meets the pace gate");
    assert!(
        gate < review.find("spawn_review()").expect("spawn"),
        "{review}"
    );
    let ideas = include_str!("feed_ui.rs")
        .split("fn maybe_suggest_ideas(")
        .nth(1)
        .and_then(|s| s.split("fn idea_request(").next())
        .expect("maybe_suggest_ideas");
    let gate = ideas
        .find("ProactiveAct::Ideas")
        .expect("ideas meet the pace gate");
    assert!(
        gate < ideas.find("spawn_native_ideas").expect("ask"),
        "{ideas}"
    );
    // Scheduled jobs are not budgeted and never take the composer.
    let night = src
        .split("fn tick_night(")
        .nth(1)
        .and_then(|s| s.split("fn poll_night_check(").next())
        .expect("tick_night");
    assert!(
        !night.contains("heartbeat_may") && !night.contains("send_scheduled_chat"),
        "{night}"
    );
    let halt = fn_src(&src, "halt_everything");
    assert!(
        halt.find("heartbeat_halt(").expect("halt") < halt.find("halt_work(").expect("work"),
        "Halt holds the pulse before anything else: {halt}"
    );
}

/// Spike-5c: a skill Grok made through `grokhub-self` under Always is one
/// self_manage ledger line, and the user's Undo click removes it.
#[test]
fn self_made_skill_undo_click_removes_it() {
    use crate::self_mcp::tests::{call, FakeIo};
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("self-made-undo");
    let mut server = crate::self_mcp::SelfServer::new(&root);
    let mut io = FakeIo::answering(false);
    let (ok, text) = call(
        &mut server,
        &mut io,
        "skill_create",
        serde_json::json!({ "name": "inbox-zero", "instructions": "1. Archive read mail\n2. Star replies", "reason": "you do this every morning" }),
    );
    assert_eq!(
        (ok, text.as_str()),
        (
            true,
            "created skill inbox-zero (change #1; the user can Undo it)"
        )
    );
    let folder = skills::skill_folder("inbox-zero");
    assert!(folder.join("SKILL.md").exists());
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    let rows = cabin.skill_rows_now();
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].label.starts_with("inbox-zero · added "),
        "{}",
        rows[0].label
    );
    cabin.skill_row_clicked(&rows[0]);
    assert!(!folder.exists(), "Undo removes the self-made skill");
    let ledger = grokhub_agent::harness::ChangeLedger::load(&root);
    let ops: Vec<(&str, &str)> = ledger
        .all()
        .iter()
        .map(|c| (c.op.as_str(), c.origin.as_str()))
        .collect();
    assert_eq!(ops, vec![("create", "self_manage"), ("undo", "user")]);
    let _ = std::fs::remove_dir_all(&root);
}
/// Spike-5c: `grokhub --mcp-self` runs in its own process, so its ledger
/// lines never reach the cabin's in-process queue. The cabin reads them from
/// disk: a Work-tree row, a Home update, and an automations list that matches
/// the file, each shown once.
#[test]
fn changes_from_the_self_server_process_show_as_work_rows() {
    use crate::self_mcp::tests::{call, FakeIo};
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("self-elsewhere");
    let _agent_pin = grokhub_agent::perm::ConfigGuard::set(&root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.poll_self_changes();
    assert!(cabin.harness.work_rows.is_empty());
    let mut server = crate::self_mcp::SelfServer::new(&root);
    let mut io = FakeIo::answering(false);
    assert!(
        call(
            &mut server,
            &mut io,
            "skill_create",
            serde_json::json!({ "name": "inbox-zero", "instructions": "archive read mail" })
        )
        .0
    );
    let (ok, made) = call(
        &mut server,
        &mut io,
        "automation_create",
        serde_json::json!({ "instructions": "sweep the inbox", "every": "1h" }),
    );
    assert!(ok, "{made}");
    let id = made
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("created ")
        .to_string();
    // What the other process left behind: lines on disk only.
    let _ = grokhub_agent::harness::take_self_changes(&root);
    let _ = grokhub_agent::take_automation_changes();
    assert!(cabin.automations.is_empty());
    cabin.harness.ledger_watch.next_ms = 0;
    cabin.poll_self_changes();
    let labels: Vec<&str> = cabin
        .harness
        .work_rows
        .iter()
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(labels.len(), 2, "{labels:?}");
    assert!(
        labels.contains(&"Grok added skill inbox-zero"),
        "{labels:?}"
    );
    let ids: Vec<&str> = cabin.automations.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec![id.as_str()], "the cabin's list matches the file");
    let cards = cabin
        .updates
        .iter()
        .filter(|u| u.kind == grokhub_core::UpdateKind::SelfChange)
        .count();
    assert_eq!(cards, 2);
    // Looked again: nothing new, nothing posted twice.
    cabin.harness.ledger_watch.next_ms = 0;
    cabin.poll_self_changes();
    assert_eq!(cabin.harness.work_rows.len(), 2);
    assert_eq!(
        cabin
            .updates
            .iter()
            .filter(|u| u.kind == grokhub_core::UpdateKind::SelfChange)
            .count(),
        2
    );
    let _ = std::fs::remove_dir_all(&root);
}
/// Spike-5b: an automation the model wrote (an Ideas Add, an Automate offer)
/// is saved through the ChangeLedger. It shows as a Work-tree row and a Home
/// update, and the row's Undo click removes it from the file and the
/// cabin's own list, so the next save does not bring it back.
#[test]
fn a_model_written_automation_gets_a_work_row_a_home_update_and_undo() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("self-change-auto");
    let mut cabin = Cabin::quiet_for_test();
    cabin.poll_self_changes();
    assert!(cabin.harness.work_rows.is_empty());
    let route = route_schedule("every weekday at 9, summarize the board").expect("clock route");
    let said = cabin.commit_schedule(
        route,
        grokhub_agent::harness::Origin::SelfManage,
        "from the idea Board digest",
    );
    assert!(said.starts_with("Automation added"), "{said}");
    let id = cabin.automations[0].id.clone();
    assert!(
        std::fs::read_to_string(crate::night::path())
            .unwrap()
            .contains(&id),
        "saved now, not later"
    );
    let ledger = grokhub_agent::harness::ChangeLedger::load_kind(
        &root,
        grokhub_agent::harness::ChangeKind::Automation,
    );
    let c = &ledger.all()[0];
    assert_eq!(
        (c.op.as_str(), c.origin.as_str(), c.id.as_str()),
        ("create", "self_manage", id.as_str())
    );
    assert_eq!(c.reason, "from the idea Board digest");
    cabin.poll_self_changes();
    assert_eq!(cabin.harness.work_rows.len(), 1);
    let row = cabin.harness.work_rows[0].clone();
    assert_eq!(
        row.label,
        format!("Grok added automation {}", cabin.automations[0].name)
    );
    let card = cabin
        .updates
        .iter()
        .find(|u| u.kind == grokhub_core::UpdateKind::SelfChange)
        .expect("a Home update");
    assert_eq!(
        card.title,
        format!("GrokHub added automation {}", cabin.automations[0].name)
    );
    assert_eq!(
        card.body.as_deref(),
        Some(
            "from the idea Board digest · Undo it from its Work-tree row or /automations changes."
        )
    );
    cabin.change_row_clicked(&row, super::change_undo::ChangeAct::Undo);
    assert!(
        cabin.automations.is_empty(),
        "the cabin's list follows the file"
    );
    assert!(!std::fs::read_to_string(crate::night::path())
        .unwrap()
        .contains(&id));
    assert!(cabin.harness.work_rows.is_empty());
    assert!(
        cabin.status.starts_with("Removed the automation "),
        "{}",
        cabin.status
    );
    // A user-typed job stays outside the ledger, as before.
    let route = route_schedule("every day at 7, water the plants").expect("clock route");
    cabin.commit_schedule(route, grokhub_agent::harness::Origin::User, "");
    std::thread::sleep(Duration::from_millis(100));
    let ledger = grokhub_agent::harness::ChangeLedger::load_kind(
        &root,
        grokhub_agent::harness::ChangeKind::Automation,
    );
    assert_eq!(
        ledger.all().len(),
        2,
        "create and its undo only: {:?}",
        ledger.all()
    );
    cabin.poll_self_changes();
    assert!(cabin.harness.work_rows.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// Bug 4: a routing-table write or an R3a tuning change is tell-only. Each
/// posts exactly one Home note naming it and makes no Undo/Keep row; the
/// ledger lines stay for `/why table`. A connection change in the same poll
/// still gets its Work-tree row and Home update.
#[test]
fn router_model_changes_post_one_home_note_and_no_undo_row() {
    use grokhub_agent::harness as hx;
    use grokhub_agent::route::{learn, table, tune};
    use hx::ChangeTarget as _;
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("self-change-model");
    let mut cabin = Cabin::quiet_for_test();
    cabin.poll_self_changes();
    assert!(cabin.harness.work_rows.is_empty());
    let mut t = table::RoutingTable::default();
    assert_eq!(table::write_table(&root, &mut t, "first build"), Ok(true));
    let mut tuning = tune::Tuning::default();
    tuning.starts.insert("chat:default".into(), "high".into());
    learn::write_tuning(&root, &tuning, "chat:default starts at High").expect("tuning");
    let mcp = root.join("mcp_test.json");
    let target = hx::McpFile {
        path: &mcp,
        name: "wiki",
    };
    hx::record_change(
        &root,
        &target,
        hx::Origin::SelfManage,
        "you asked for the wiki",
        || target.put(Some(br#"{"url":"http://127.0.0.1:9/wiki"}"#)),
    )
    .expect("connection")
    .expect("a ledger line");
    cabin.poll_self_changes();
    let labels: Vec<&str> = cabin
        .harness
        .work_rows
        .iter()
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(
        labels,
        vec!["Grok added connection wiki"],
        "no Undo/Keep row for router changes"
    );
    let notes: Vec<(&str, Option<&str>)> = cabin
        .updates
        .iter()
        .filter(|u| u.source_id.starts_with("model:"))
        .map(|u| (u.title.as_str(), u.body.as_deref()))
        .collect();
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(
        notes.contains(&(
            "Router added routing table",
            Some("routing table v1: first build · /why table shows the picks.")
        )),
        "{notes:?}"
    );
    assert!(
        notes.contains(&(
            "Router added router tuning",
            Some("chat:default starts at High · /why table shows the picks.")
        )),
        "{notes:?}"
    );
    let conn = cabin
        .updates
        .iter()
        .find(|u| u.source_id == "connection:wiki")
        .expect("connection Home update");
    assert_eq!(conn.title, "GrokHub added connection wiki");
    // The ledger lines stay, so `/why table` and history still read them.
    let ids: Vec<String> = hx::ChangeLedger::load_kind(&root, hx::ChangeKind::Model)
        .all()
        .iter()
        .map(|c| c.id.clone())
        .collect();
    assert_eq!(ids.len(), 2, "{ids:?}");
    assert!(ids.contains(&"route_tuning".to_string()), "{ids:?}");
    // Looked again and after a restart: no router row, no second note.
    cabin.harness.ledger_watch.next_ms = 0;
    cabin.poll_self_changes();
    assert_eq!(
        cabin
            .updates
            .iter()
            .filter(|u| u.source_id.starts_with("model:"))
            .count(),
        2
    );
    let mut fresh = Cabin::quiet_for_test();
    fresh.poll_self_changes();
    let labels: Vec<&str> = fresh
        .harness
        .work_rows
        .iter()
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(labels, vec!["Grok added connection wiki"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// Card 12: a self-tuning promotion posts one info-only "why" card naming the
/// model, the class, the scores and the day, and no ledger note, Undo or Keep.
#[test]
fn a_router_promotion_posts_one_why_card_and_no_prompt() {
    use grokhub_agent::route::{learn, tune};
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("router-why-card");
    let mut cabin = Cabin::quiet_for_test();
    cabin.poll_self_changes();
    let mut tuning = tune::Tuning::default();
    tuning
        .orders
        .insert("chat:default".into(), "grok-4-fast".into());
    let reason = format!(
        "{}everyday chat: grok-4-fast first (12% cheaper, 20% faster, quality held)",
        learn::TUNED_PREFIX
    );
    learn::write_tuning(&root, &tuning, &reason).expect("tuning");
    cabin.poll_self_changes();
    assert!(
        cabin.harness.work_rows.is_empty(),
        "no Undo/Keep row for a router change"
    );
    assert_eq!(
        cabin
            .updates
            .iter()
            .filter(|u| u.source_id.starts_with("model:"))
            .count(),
        0,
        "the why card replaces the note"
    );

    let promoted = tune::Candidate {
        id: "chat:default#3".into(),
        class: "chat:default".into(),
        change: Some(tune::Change::Order {
            model: "grok-4-fast".into(),
        }),
        stage: tune::Stage::Watch,
        // 2026-10-06 15:00 UTC.
        promoted_at: Some(1_791_298_800_000),
        baseline: Some(tune::Snap {
            n: 400,
            pass_pct: 85.0,
            rework_pct: 4.0,
            cost_per_step: 0.0100,
            p50_ms: 2_000,
            p95_ms: 4_000,
        }),
        promoted_snap: Some(tune::Snap {
            n: 60,
            pass_pct: 92.0,
            rework_pct: 3.0,
            cost_per_step: 0.0088,
            p50_ms: 1_600,
            p95_ms: 3_200,
        }),
        ..tune::Candidate::default()
    };
    let shadow = tune::Candidate {
        id: "chat:default#4".into(),
        class: "chat:default".into(),
        ..tune::Candidate::default()
    };
    let state = tune::TuneState {
        candidates: vec![promoted, shadow],
        ..tune::TuneState::default()
    };
    cabin.post_why_cards(
        &state,
        &["chat:default#3".to_string(), "chat:default#4".to_string()],
    );
    let why: Vec<&grokhub_core::UpdateCard> = cabin
        .updates
        .iter()
        .filter(|u| u.source_id.starts_with("router-why:"))
        .collect();
    assert_eq!(why.len(), 1, "only the promoted candidate gets a card");
    assert_eq!(why[0].source_id, "router-why:chat:default#3");
    assert_eq!(
        why[0].title,
        "Why grok-4-fast for everyday chat: 92% pass vs 85%, promoted Oct 6"
    );
    assert_eq!(
        why[0].body.as_deref(),
        Some("Auto now runs everyday chat with grok-4-fast first: 12% cheaper and 20% faster over 60 canary steps. Info only; Auto keeps tuning on its own.")
    );
    assert_eq!(why[0].action, None, "info only: no Accept, Undo or Keep");
    assert!(cabin.harness.work_rows.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// `/connections changes` and `/automations changes` post the report
/// bubble; from a scheduled send they still only show rows (no slash undoes).
#[test]
fn connection_and_automation_changes_reports_post_a_bubble() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("self-change-report");
    let mut cabin = Cabin::quiet_for_test();
    cabin.send_from_composer("/connections changes".into());
    cabin.send_scheduled_chat("/automations changes".into());
    let bodies: Vec<&str> = cabin.messages.iter().map(|m| m.1.as_str()).collect();
    assert!(
        bodies
            .iter()
            .any(|b| b.contains(super::change_undo::CONNECTION_CHANGES_HEAD)),
        "{bodies:?}"
    );
    assert!(
        bodies
            .iter()
            .any(|b| b.contains(super::change_undo::AUTOMATION_CHANGES_HEAD)),
        "{bodies:?}"
    );
    assert!(cabin
        .change_rows_now(grokhub_agent::harness::ChangeKind::Connection)
        .is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

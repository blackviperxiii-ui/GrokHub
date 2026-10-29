//! Spike-8a local indexer tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

// ---- Spike-8a local indexers: the in-context ask card and "Forget these" ----

fn paint_work_cards(
    ctx: &egui::Context,
    cabin: &mut Cabin,
    events: Vec<egui::Event>,
) -> ScopePaint {
    fn walk(shape: &egui::Shape, p: &mut ScopePaint) {
        match shape {
            egui::Shape::Text(t) => p.texts.push((
                t.galley.text().to_string(),
                t.pos + t.galley.rect.center().to_vec2(),
            )),
            egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, p)),
            _ => {}
        }
    }
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 1200.0),
        )),
        events,
        ..Default::default()
    };
    let out = crate::theme::test_pass(ctx, input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| cabin.paint_harness_cards(ui));
    });
    let mut p = ScopePaint {
        texts: Vec::new(),
        filled: Vec::new(),
    };
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut p);
    }
    p
}

fn click_work_card(ctx: &egui::Context, cabin: &mut Cabin, at: egui::Pos2) {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = paint_work_cards(ctx, cabin, vec![egui::Event::PointerMoved(at)]);
    let _ = paint_work_cards(ctx, cabin, vec![press(true)]);
    let _ = paint_work_cards(ctx, cabin, vec![press(false)]);
}

/// P5: the agent can ask for a scope in context; only the click on the card's
/// Allow writes the grant. Enter never does, and Not now leaves it off.
#[test]
fn a_scope_ask_card_grants_on_a_click_only() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-ask");
    std::fs::create_dir_all(&root).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let ledger = hx::ConsentLedger::load(&root);
    let why = "To suggest the right app for a task I'd read which apps you have installed.";
    assert!(cabin
        .harness
        .indexer
        .asks
        .ask(hx::Scope::Apps, why, &ledger, now_ms()));
    assert!(cabin.harness.indexer.asks.ask(
        hx::Scope::SystemState,
        "To spot a full disk early I'd read your disk and services.",
        &ledger,
        now_ms()
    ));
    assert_eq!(cabin.decisions_waiting(), 2, "asks count as decisions");

    let p = paint_work_cards(&ctx, &mut cabin, vec![]);
    assert!(
        p.has("Learn from your computer") && p.has("Allow Installed apps?") && p.has(why),
        "{:?}",
        p.texts
    );
    assert!(!p.has("Allow System state?"), "one card at a time");
    // Keys never grant: Tab onto the card and press Enter and Space. (A
    // focused Not now may answer; that only keeps the scope off.)
    for _ in 0..6 {
        let _ = paint_work_cards(&ctx, &mut cabin, vec![key(egui::Key::Tab)]);
        let _ = paint_work_cards(&ctx, &mut cabin, vec![key(egui::Key::Enter)]);
        let _ = paint_work_cards(&ctx, &mut cabin, vec![key(egui::Key::Space)]);
    }
    assert_eq!(
        hx::ConsentLedger::load(&root).active().count(),
        0,
        "keyboard never grants"
    );
    cabin.harness.indexer.asks = Default::default();
    let ledger = hx::ConsentLedger::load(&root);
    assert!(cabin
        .harness
        .indexer
        .asks
        .ask(hx::Scope::Apps, why, &ledger, now_ms()));
    assert!(cabin.harness.indexer.asks.ask(
        hx::Scope::SystemState,
        "To spot a full disk early I'd read your disk and services.",
        &ledger,
        now_ms()
    ));
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);

    let p = paint_work_cards(&ctx, &mut cabin, vec![]);
    click_work_card(&ctx, &mut cabin, p.at("Allow"));
    assert_eq!(
        cabin.status,
        "Installed apps allowed. Loadout learns from it on this computer. Revoke it here any time."
    );
    assert!(hx::ConsentLedger::load(&root)
        .scope_grant(&hx::Scope::Apps)
        .is_some());

    let p = paint_work_cards(&ctx, &mut cabin, vec![]);
    assert!(p.has("Allow System state?"));
    click_work_card(&ctx, &mut cabin, p.at("Not now"));
    assert_eq!(cabin.status, "System state stays off.");
    assert_eq!(
        hx::ConsentLedger::load(&root).scope_grant(&hx::Scope::SystemState),
        None
    );
    assert_eq!(cabin.harness.indexer.asks.len(), 0);
    assert_eq!(hx::ConsentLedger::load(&root).active().count(), 1);
    release_isolated(&root, cabin);
}

/// A granted scope lists what it taught GrokHub; "Forget these" retires them.
#[test]
fn a_granted_scope_lists_its_facts_and_forget_these_purges_them() {
    use grokhub_agent::harness as hx;
    use grokhub_agent::indexers as idx;
    struct Mains;
    impl idx::power::PowerSource for Mains {
        fn on_battery(&self) -> bool {
            false
        }
    }
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-facts");
    std::fs::create_dir_all(&root).unwrap();
    let docs = root.join("fixture-home").join("Documents");
    std::fs::create_dir_all(docs.join(".ssh")).unwrap();
    std::fs::write(docs.join("plan.md"), "# Q4 launch plan\n## Budget\n").unwrap();
    std::fs::write(docs.join("todo.txt"), "Call the venue\n").unwrap();
    std::fs::write(docs.join(".ssh").join("id_ed25519"), "not a real key\n").unwrap();
    let scope = hx::Scope::Files(docs.display().to_string());
    hx::grant_scope(
        &root,
        &scope,
        Some(&root.join("fixture-home")),
        hx::UserClick::from_click(),
    )
    .unwrap();
    let dirs = idx::paths::PlatformDirs::from_env();
    let env = idx::TickEnv {
        config_dir: &root,
        now_ms: now_ms(),
        quiet: false,
        power: &Mains,
        fs: &idx::fs::RealFs,
        probe: &idx::system_state::OsProbe,
        dirs: &dirs,
    };
    let out = idx::Scheduler::default().tick(&env);
    assert!(
        matches!(out, idx::TickOutcome::Ran { written: 2, .. }),
        "{out:?}"
    );
    cabin.harness.indexer.index = std::sync::Arc::new(idx::ScopeIndex::load(&root));

    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(
        p.has("Learned 2 things, sealed on this computer:"),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    assert!(p.texts.iter().any(|t| t.0.starts_with("File plan.md · ")
        && t.0.ends_with("title: Q4 launch plan · headings: Budget")));
    assert!(p.texts.iter().any(|t| t.0.starts_with("File todo.txt · ")));
    assert!(
        !p.texts
            .iter()
            .any(|t| t.0.contains("id_ed25519") || t.0.contains(".ssh")),
        "hard excludes never show"
    );
    assert!(
        p.at(super::indexer_ui::FORGET_THESE).y > p.at("Files in Documents").y,
        "under its row"
    );

    click_at(&ctx, &mut cabin, p.at(super::indexer_ui::FORGET_THESE));
    let start = std::time::Instant::now();
    while cabin
        .harness
        .indexer
        .index
        .facts
        .values()
        .any(|v| !v.is_empty())
        && start.elapsed() < std::time::Duration::from_secs(4)
    {
        cabin.poll_forget();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(cabin.status, "Forgot 2 things learned from Files in Documents. It stays allowed; revoke it to stop reading.");
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(!p.has(super::indexer_ui::FORGET_THESE));
    assert!(p.has("Nothing learned to show. Loadout reads it on a heartbeat, never on battery or in quiet hours."));
    assert!(
        hx::ConsentLedger::load(&root).scope_grant(&scope).is_some(),
        "forgetting is not revoking"
    );
    assert_eq!(
        idx::ScopeIndex::load(&root).for_scope(&scope.key()).len(),
        0
    );
    release_isolated(&root, cabin);
}

#[test]
fn failed_settings_write_replaces_saved_on_the_status_line() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("persist-err");
    std::fs::create_dir_all(&root).unwrap();
    let blocker = root.join("not-a-dir");
    std::fs::write(&blocker, b"x").unwrap();
    std::env::set_var("GROKHUB_CONFIG", blocker.join("cfg"));
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Saved".into();
    cabin.persist_cfg();
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(5) && cabin.status == "Saved" {
        std::thread::sleep(std::time::Duration::from_millis(20));
        cabin.poll_persist_err();
    }
    assert!(
        cabin.status.starts_with("Could not save settings: "),
        "{}",
        cabin.status
    );
    release_isolated(&root, cabin);
}

fn crash_cards(cabin: &super::Cabin) -> Vec<grokhub_core::UpdateCard> {
    cabin
        .updates
        .iter()
        .filter(|c| grokhub_core::is_crash_card(c))
        .cloned()
        .collect()
}

fn bg_run_on(
    thread_id: &str,
    title: &str,
    rx: Option<mpsc::Receiver<grokhub_core::wire::GrokPEvent>>,
) -> super::background::BgRun {
    super::background::BgRun {
        id: 41,
        thread_id: thread_id.into(),
        title: title.into(),
        origin: grokhub_core::BgOrigin::User,
        rx,
        say: String::new(),
        action: String::new(),
        started: std::time::Instant::now(),
        end: None,
        session: String::new(),
        resumed: None,
        fork_hold: false,
        native_session: None,
        automation: None,
    }
}

#[test]
fn a_background_run_killed_from_outside_posts_one_named_crash_card_and_stop_posts_none() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("crash-bg");
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    let id = cabin.threads[0].id.clone();

    let (tx, rx) = mpsc::channel();
    tx.send(grokhub_core::wire::GrokPEvent::Err(
        "agent closed (exit 143)".into(),
    ))
    .unwrap();
    cabin
        .bg
        .runs
        .push(bg_run_on(&id, "Index ~/Projects", Some(rx)));
    cabin.poll_bg_runs();
    let cards = crash_cards(&cabin);
    assert_eq!(cards.len(), 1, "one card for one crash: {cards:?}");
    assert_eq!(
        cards[0].title,
        "Crashed: Index ~/Projects (exit 143, killed)"
    );
    assert_eq!(cards[0].prompt.as_deref(), Some("/bg Index ~/Projects"));
    assert_eq!(
        cards[0].action,
        Some(grokhub_core::UpdateAction::OpenSession {
            thread_id: id.clone()
        })
    );
    assert!(cabin.bg.runs.is_empty(), "the run ended");

    // Stop from GrokHub drops the receiver first: no crash card.
    let (_tx2, rx2) = mpsc::channel();
    let mut stopped = bg_run_on(&id, "Sort the inbox", Some(rx2));
    stopped.id = 42;
    cabin.bg.runs.push(stopped);
    cabin.stop_bg_run(42);
    assert_eq!(crash_cards(&cabin).len(), 1, "Stop is not a crash");

    // Retry opens the chat and starts the run again.
    let card_id = cards[0].id.clone();
    cabin.crash_retry(&card_id);
    assert_eq!(
        cabin
            .updates
            .iter()
            .find(|c| c.id == card_id)
            .map(|c| c.status),
        Some(grokhub_core::UpdateStatus::Opened)
    );
    release_isolated(&root, cabin);
}

#[test]
fn a_killed_reply_retries_once_quietly_and_a_second_kill_posts_one_crash_card() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("crash-turn");
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    let id = cabin.threads[0].id.clone();
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "Index ~/Projects".into())]);
    cabin.chat_job_thread = Some(id.clone());

    // First kill with nothing streamed: the automatic retry, no card.
    let (tx, rx) = mpsc::channel();
    tx.send(grokhub_core::wire::GrokPEvent::Err(
        "agent closed (exit 143)".into(),
    ))
    .unwrap();
    cabin.grok_p_rx = Some(rx);
    cabin.running = true;
    cabin.turn_retried = false;
    cabin.poll_single();
    assert!(cabin.turn_retried, "the first kill is retried");
    assert!(
        crash_cards(&cabin).is_empty(),
        "a retry in flight is not a crash"
    );

    // The retry finishes: still no card.
    let (tx, rx) = mpsc::channel();
    tx.send(grokhub_core::wire::GrokPEvent::End(
        grokhub_core::wire::SingleTurn {
            session_id: "s-retry".into(),
            text: "Indexed 40 folders.".into(),
            thought: String::new(),
            usage: Default::default(),
            stop_reason: "end_turn".into(),
        },
    ))
    .unwrap();
    cabin.grok_p_rx = Some(rx);
    cabin.running = true;
    cabin.chat_job_thread = Some(id.clone());
    cabin.poll_single();
    assert!(
        crash_cards(&cabin).is_empty(),
        "a successful retry posts nothing"
    );

    // Killed again after the retry was spent: one named card.
    let (tx, rx) = mpsc::channel();
    tx.send(grokhub_core::wire::GrokPEvent::Err("signal 15".into()))
        .unwrap();
    cabin.grok_p_rx = Some(rx);
    cabin.running = true;
    cabin.turn_retried = true;
    cabin.chat_job_thread = Some(id.clone());
    cabin.poll_single();
    let cards = crash_cards(&cabin);
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert_eq!(
        cards[0].title,
        "Crashed: Index ~/Projects (exit 143, killed)"
    );
    assert_eq!(cards[0].prompt.as_deref(), Some("/retry"));
    assert!(!cabin.running);
    release_isolated(&root, cabin);
}

/// Brain 4: every background job is started from exactly one place, its
/// governor arm, so nothing is scheduled twice after the migration.
#[test]
fn each_background_job_runs_only_from_its_governor_arm() {
    let src = cabin_src()
        + &[
            include_str!("feed_ui.rs"),
            include_str!("indexer_ui.rs"),
            include_str!("self_review_ui.rs"),
            include_str!("amr_memory.rs"),
            include_str!("router_ui.rs"),
            include_str!("proactive_auto.rs"),
            include_str!("proactive_ui.rs"),
            include_str!("pulse_sources_ui.rs"),
            include_str!("heartbeat_gate.rs"),
            include_str!("native_unattended.rs"),
        ]
        .concat()
        .replace("\r\n", "\n");
    let arm = |kind: &str| -> String {
        let start = format!("JobKind::{kind} =>");
        let rest = src
            .split(start.as_str())
            .nth(1)
            .unwrap_or_else(|| panic!("no arm for {kind}"));
        let end = rest
            .find("\n            JobKind::")
            .or_else(|| rest.find("\n        }\n"))
            .unwrap_or(rest.len());
        rest[..end].to_string()
    };
    for (kind, call) in [
        ("Automations", "self.tick_loops("),
        ("Automations", "self.tick_night("),
        ("PulseRefresh", "self.tick_feed_pulse("),
        ("ModelRegistry", "self.tick_model_registry("),
        ("Proactive", "self.tick_proactive_cards("),
        ("Ideas", "self.maybe_suggest_ideas(false)"),
        ("Anticipate", "self.tick_anticipate("),
        ("Digest", "self.follow_feed_lookup("),
        ("Digest", "self.follow_source_fetch("),
        ("Review", "self.tick_review("),
        ("NightPasses", "self.tick_night_passes("),
        ("MemoryRetention", "self.tick_amr_retention("),
        ("DailyDrop", "self.tick_daily_drop("),
        ("Indexers", "self.tick_indexers("),
    ] {
        assert_eq!(
            src.matches(call).count(),
            1,
            "{call} is started from one place only"
        );
        assert!(
            arm(kind).contains(call),
            "{call} runs from the {kind} arm: {}",
            arm(kind)
        );
    }
    // Every registered kind has exactly one arm, and the pulse reaches them only
    // through the governor's queue.
    let kinds = grokhub_core::Governor::cabin().kinds();
    assert_eq!(kinds.len(), 13);
    for kind in kinds {
        let name = format!("{kind:?}");
        assert_eq!(
            src.matches(&format!("JobKind::{name} =>")).count(),
            1,
            "{name} has one arm"
        );
    }
    let beat = fn_src(&src, "tick_heartbeat");
    assert!(
        beat.contains("HeartbeatAct::Background => self.run_background(now, halted)"),
        "{beat}"
    );
    let run = fn_src(&src, "run_background");
    assert!(
        run.contains("self.background_plan(") && run.contains("for job in plan.run"),
        "{run}"
    );
    let plan = fn_src(&src, "background_plan");
    assert!(
        plan.contains("self.governor.tick(") && plan.contains("hold_heavy: active"),
        "{plan}"
    );
}

/// Brain 4 PR 2: a draft in the composer or a chat run holds heavy jobs and
/// puts light model jobs on the router's cost goal; going idle resumes them.
#[test]
fn being_active_holds_heavy_jobs_and_light_model_jobs_go_cheap_until_idle() {
    use grokhub_core::{JobHold, JobKind};
    const T0: u64 = 1_800_000_000_000;
    const DAY: &str = "2026-10-11";
    let mut cabin = quiet_cabin();
    // Equal start and end: quiet hours never apply, whatever the real clock says.
    cabin.cfg.quiet_start = "00:00".into();
    cabin.cfg.quiet_end = "00:00".into();
    let long_ago = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(60))
        .expect("a minute of uptime");
    cabin.last_activity = long_ago;
    cabin.composer = "half a thought".into();
    assert!(cabin.user_active_now());
    let plan = cabin.background_plan(T0, false, DAY);
    assert_eq!(
        plan.held,
        vec![
            (JobKind::Automations, JobHold::Heavy),
            (JobKind::Anticipate, JobHold::Heavy),
            (JobKind::NightPasses, JobHold::Heavy),
            (JobKind::Indexers, JobHold::Heavy),
        ]
    );
    assert_eq!(
        plan.cheap,
        vec![JobKind::Ideas, JobKind::Digest, JobKind::Review]
    );
    assert_eq!(
        cabin.bg_goal(JobKind::Review).map(|g| g.label()),
        Some("goal cost (user active)".to_string())
    );
    assert_eq!(
        cabin.bg_goal(JobKind::Indexers),
        None,
        "heavy jobs wait instead"
    );
    assert_eq!(
        cabin.bg_goal(JobKind::PulseRefresh),
        None,
        "local upkeep calls no model"
    );
    // An empty composer with a chat run going is active too.
    cabin.composer.clear();
    cabin.running = true;
    assert!(cabin.user_active_now());
    let plan = cabin.background_plan(T0 + 1_000, false, DAY);
    assert_eq!(plan.run, Vec::<JobKind>::new());
    assert_eq!(plan.held.len(), 4);
    // Idle: the held jobs run on the next beat, once, and nothing is cheap.
    cabin.running = false;
    assert!(!cabin.user_active_now());
    let plan = cabin.background_plan(T0 + 2_000, false, DAY);
    assert_eq!(
        plan.run,
        vec![
            JobKind::Automations,
            JobKind::Anticipate,
            JobKind::NightPasses,
            JobKind::Indexers
        ]
    );
    assert_eq!(plan.cheap, Vec::<JobKind>::new());
    assert_eq!(cabin.bg_goal(JobKind::Review), None);
    assert_eq!(
        cabin.background_plan(T0 + 2_000, false, DAY).run,
        Vec::<JobKind>::new()
    );
    // A key or click a moment ago counts as active again.
    cabin.last_activity = std::time::Instant::now();
    assert!(cabin.user_active_now());
}

#[test]
fn journal_create_promote_persists_and_slash_opens() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("journal-v1");
    let _ = std::fs::create_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/journal");
    assert!(
        matches!(cabin.nav, Nav::Journal),
        "/journal must open the Journal page"
    );

    let id = grokhub_core::create_entry(&mut cabin.journal, "Should we keep a decision journal?")
        .expect("create");
    assert_eq!(cabin.journal.len(), 1);
    assert!(grokhub_core::edit_entry(
        &mut cabin.journal,
        &id,
        None,
        Some("Ideas were becoming silent commitments."),
        Some("Ship local decisions.json."),
        None,
        None,
    ));
    let board_id =
        grokhub_core::promote_to_board(&mut cabin.journal, &mut cabin.board, &id).expect("promote");
    assert_eq!(
        cabin.journal[0].board_id.as_deref(),
        Some(board_id.as_str())
    );
    assert_eq!(cabin.board.len(), 1);
    assert!(
        cabin.board[0].detail.contains("Ideas were becoming")
            || cabin.board[0].detail.contains("journal"),
        "context must survive promote: {}",
        cabin.board[0].detail
    );

    crate::config::save_journal(&cabin.journal).expect("save decisions.json");
    let path = crate::config::decisions_path();
    assert!(
        path.starts_with(&root),
        "journal must land under GROKHUB_CONFIG: {} vs {}",
        path.display(),
        root.display()
    );
    let loaded = crate::config::load_journal();
    assert_eq!(loaded.len(), 1, "decisions.json at {}", path.display());
    assert_eq!(loaded[0].question, "Should we keep a decision journal?");
    assert_eq!(loaded[0].board_id.as_deref(), Some(board_id.as_str()));
    // Background flush uses the same save path.
    cabin.flush_journal();
    let src = include_str!("journal_ui.rs");
    assert!(
        src.contains("config::save_journal") && src.contains("flush_journal"),
        "UI flush must write decisions.json"
    );

    assert!(grokhub_core::decide_entry(
        &mut cabin.journal,
        &id,
        None,
        "Local journal is enough for v1."
    ));
    assert_eq!(
        cabin.journal[0].status,
        grokhub_core::JournalStatus::Decided
    );

    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn g4_show_panel_paints_in_chat_when_enabled() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("g4-panel-chat");
    cabin.messages = std::sync::Arc::new(vec![("You".into(), "show ci".into())]);
    let marker = "Panel: CI\n\n{\n  \"panel\": {\n  \"type\": \"ci_status\",\n  \"pr_number\": 756,\n  \"checks\": [{\"name\": \"linux\", \"state\": \"pass\"}],\n  \"actions\": []\n}\n}";
    assert!(cabin.panels.upsert_from_tool_result("call-756", marker));
    super::panels::PANELS_UI_ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
    let ctx = crate::ui_kit::test_ctx(true);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 1200.0),
        )),
        ..Default::default()
    };
    let out = crate::theme::test_pass(&ctx, input, |ui| {
        cabin.ui_chat(ui);
    });
    let has_pr = out.shapes.iter().any(|c| {
        if let egui::Shape::Text(t) = &c.shape {
            t.galley.text().contains("PR #756")
        } else {
            false
        }
    });
    assert!(has_pr, "enabled chat should paint PR #756");
    super::panels::PANELS_UI_ENABLED.store(false, std::sync::atomic::Ordering::Relaxed);
    let out_off = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        cabin.ui_chat(ui);
    });
    let has_pr_off = out_off.shapes.iter().any(|c| {
        if let egui::Shape::Text(t) = &c.shape {
            t.galley.text().contains("PR #756")
        } else {
            false
        }
    });
    assert!(!has_pr_off, "disabled chat must not paint the panel");
    release_isolated(&root, cabin);
}

#[test]
fn g4_panel_action_queues_followup_without_tool_dispatch() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("g4-panel-action");
    assert!(cabin.followup_queue.is_empty());
    assert!(cabin.tool_cards.is_empty());
    cabin.queue_panel_actions(vec![super::panels::PanelAction {
        panel_id: "panel-1".into(),
        action: "rerun_ci".into(),
    }]);
    assert_eq!(
        cabin.followup_queue,
        vec!["[panel action] rerun_ci on panel-1".to_string()]
    );
    assert!(
        cabin.tool_cards.is_empty(),
        "panel actions must not dispatch tools directly"
    );
    assert!(!cabin.running);
    release_isolated(&root, cabin);
}

#[test]
fn g4_drain_show_panel_from_pending_queue() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("g4-drain-card");
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Chat", false));
    cabin.thread_idx = 0;
    let detail = "{\n  \"panel\": {\n  \"type\": \"ci_status\",\n  \"pr_number\": 42,\n  \"checks\": [],\n  \"actions\": []\n}\n}";
    cabin.enqueue_show_panel_for_test("tc-1", detail);
    cabin.drain_show_panel_results();
    assert_eq!(cabin.panels.len(), 1);
    // Re-drain must not revive a dismissal or require cloning tool_cards.
    cabin.panels.dismiss("tc-1");
    cabin.enqueue_show_panel_for_test("tc-1", detail);
    cabin.drain_show_panel_results();
    assert_eq!(cabin.panels.len(), 0);
    release_isolated(&root, cabin);
}

#[test]
fn g4_panels_clear_on_thread_bind() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("g4-thread-panels");
    let open = cabin.visible_thread_id();
    cabin.panels.bind_thread(&open);
    assert!(cabin.panels.upsert_from_tool_result(
        "c",
        "{\n  \"panel\": {\n  \"type\": \"ci_status\",\n  \"pr_number\": 1,\n  \"checks\": [],\n  \"actions\": []\n}\n}"
    ));
    assert_eq!(cabin.panels.len(), 1);
    cabin.panels.bind_thread("other-thread");
    assert!(cabin.panels.is_empty());
    release_isolated(&root, cabin);
}

#[test]
fn g4_drain_defers_show_panel_for_other_thread() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("g4-drain-other");
    let mut owner_thread = crate::threads::ChatThread::new("Owner", false);
    owner_thread.id = "owner-thread".into();
    let mut other = crate::threads::ChatThread::new("Other", false);
    other.id = "other-visible".into();
    cabin.threads = vec![owner_thread, other];
    cabin.thread_idx = 0;
    let detail = "{\n  \"panel\": {\n  \"type\": \"ci_status\",\n  \"pr_number\": 99,\n  \"checks\": [],\n  \"actions\": []\n}\n}";
    cabin.enqueue_show_panel_for_thread_for_test("owner-thread", "tc-owner", detail);
    // User switched tabs mid-turn before drain.
    cabin.thread_idx = 1;
    cabin.drain_show_panel_results();
    assert_eq!(
        cabin.panels.len(),
        0,
        "must not paint owner panels onto the visible other chat"
    );
    // Switch back to owner — deferred row should apply.
    cabin.thread_idx = 0;
    cabin.drain_show_panel_results();
    assert_eq!(cabin.panels.len(), 1, "owner chat receives deferred panel");
    release_isolated(&root, cabin);
}

const G4_PANEL_DETAIL: &str = "{\n  \"panel\": {\n  \"type\": \"ci_status\",\n  \"pr_number\": 7,\n  \"checks\": [],\n  \"actions\": []\n}\n}";

fn g4_two_thread_cabin(tag: &str) -> (std::path::PathBuf, Cabin, String, String) {
    let (root, mut cabin) = isolated_cabin(tag);
    let mut visible = crate::threads::ChatThread::new("Visible", false);
    visible.id = format!("{tag}-visible");
    let mut hidden = crate::threads::ChatThread::new("Hidden", false);
    hidden.id = format!("{tag}-hidden");
    let ids = (visible.id.clone(), hidden.id.clone());
    cabin.threads = vec![visible, hidden];
    cabin.thread_idx = 0;
    (root, cabin, ids.0, ids.1)
}

#[test]
fn g4_drain_drops_show_panel_for_unknown_thread() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin, _visible, _hidden) = g4_two_thread_cabin("g4-drop-unknown");
    cabin.enqueue_show_panel_for_thread_for_test(
        "g4-drop-unknown-gone",
        "tc-gone",
        G4_PANEL_DETAIL,
    );
    cabin.drain_show_panel_results();
    let (ids, _) = Cabin::pending_show_panel_snapshot_for_test();
    assert!(
        !ids.contains(&"tc-gone".to_string()),
        "deleted-thread row must not re-queue"
    );
    assert_eq!(cabin.panels.len(), 0);
    release_isolated(&root, cabin);
}

#[test]
fn g4_drain_keeps_show_panel_for_known_hidden_thread() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin, _visible, hidden) = g4_two_thread_cabin("g4-keep-hidden");
    cabin.enqueue_show_panel_for_thread_for_test(&hidden, "tc-kept", G4_PANEL_DETAIL);
    cabin.drain_show_panel_results();
    let (ids, _) = Cabin::pending_show_panel_snapshot_for_test();
    assert_eq!(ids.iter().filter(|id| *id == "tc-kept").count(), 1);
    cabin.thread_idx = 1;
    cabin.drain_show_panel_results();
    assert_eq!(cabin.panels.len(), 1);
    release_isolated(&root, cabin);
}

#[test]
fn g4_drain_caps_deferred_show_panel_queue() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin, _visible, hidden) = g4_two_thread_cabin("g4-cap");
    for i in 0..100 {
        cabin.enqueue_show_panel_for_thread_for_test(
            &hidden,
            &format!("g4cap-{i}"),
            G4_PANEL_DETAIL,
        );
    }
    cabin.drain_show_panel_results();
    let (ids, total) = Cabin::pending_show_panel_snapshot_for_test();
    assert_eq!(total, 64);
    assert!(
        !ids.contains(&"g4cap-0".to_string()),
        "oldest dropped first"
    );
    assert!(ids.contains(&"g4cap-99".to_string()), "newest kept");
    release_isolated(&root, cabin);
}

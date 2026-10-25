//! Slash commands tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

#[test]
fn slash_arrows_move_and_clamp() {
    assert_eq!(super::slash_pick_step(0, 5, 1), 1);
    assert_eq!(super::slash_pick_step(0, 5, -1), 0);
    assert_eq!(super::slash_pick_step(4, 5, 1), 4);
    assert_eq!(super::slash_pick_step(9, 3, 0), 2);
}

#[test]
fn tab_accept_runs_on_pick() {
    let mut composer = "/fi".into();
    let run = super::slash_pick_take(&mut composer, "/fix", true);
    assert_eq!(run.as_deref(), Some("/fix"));
    assert!(composer.is_empty());
}

#[test]
fn tab_accept_stays_for_args() {
    let mut composer = "/proj".into();
    let run = super::slash_pick_take(&mut composer, "/project bind ", false);
    assert!(run.is_none());
    assert_eq!(composer, "/project bind ");
}

#[test]
fn slash_pick_resets_when_the_list_changes() {
    assert_eq!(super::slash_pick_retain(2, true, 4), 0);
    assert_eq!(super::slash_pick_retain(2, false, 4), 2);
    assert_eq!(super::slash_pick_retain(9, false, 3), 2);
    assert_eq!(super::slash_pick_retain(1, true, 0), 0);
}

#[test]
fn run_slash_pin_flips_the_pin() {
    let mut quiet = QuietCabin::boot("slash-pin");
    let cabin = &mut quiet.cabin;
    let idx = cabin.thread_idx;
    let title = cabin.threads[idx].title.clone();
    assert!(!cabin.threads[idx].pinned);
    cabin.run_slash(grokhub_core::Slash::Pin);
    assert!(cabin.threads[idx].pinned, " /pin must pin the open chat");
    assert!(cabin.threads[idx].pinned_ms > 0);
    assert_eq!(cabin.status, format!("Pinned {title}"));
    quiet.settle();
}

#[test]
fn run_slash_help_export_and_clear() {
    let mut quiet = QuietCabin::boot("slash-help-export-clear");
    let cabin = &mut quiet.cabin;
    cabin.run_slash(grokhub_core::Slash::Help);
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| m.0 == "assistant" && m.1.contains("/help — this list")),
        " /help must write the help list on the open chat, got {:?}",
        cabin.messages
    );
    assert!(!cabin.running);

    cabin.messages = std::sync::Arc::new(vec![("user".into(), "harbor export line".into())]);
    cabin.cfg.project_dir.clear();
    cabin.run_slash(grokhub_core::Slash::Export);
    let path = cabin
        .status
        .strip_prefix("Wrote ")
        .unwrap_or("")
        .to_string();
    assert!(
        path.ends_with("export.md"),
        " /export must name export.md, got {}",
        cabin.status
    );
    wait_for("export.md must contain the chat line", || {
        std::fs::read_to_string(&path)
            .unwrap_or_default()
            .contains("harbor export line")
    });

    cabin.run_slash(grokhub_core::Slash::Clear);
    assert!(
        cabin.messages.is_empty(),
        " /clear must empty the transcript, got {:?}",
        cabin.messages
    );
    assert_eq!(cabin.status, "Cleared");
    assert!(!cabin.running);
    quiet.settle();
}

/// Router R1: `/effort <level>` changes nothing and says effort is automatic.
#[test]
fn effort_slash_leaves_the_config_byte_identical_and_says_effort_is_automatic() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("effort-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    crate::config::save(&cabin.cfg).expect("save");
    let app_json = crate::config::config_dir().join("app.json");
    let before = std::fs::read(&app_json).expect("app.json");
    for line in ["/effort high", "/effort xhigh", "/effort banana", "/effort"] {
        cabin.status.clear();
        cabin.run_slash_line(line);
        assert_eq!(cabin.status, "Effort is automatic now, see /why", "{line}");
        assert_eq!(
            std::fs::read(&app_json).expect("app.json"),
            before,
            "{line} must not touch app.json"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

/// Router R1: there is no effort control in the composer, Settings or the slash list.
#[test]
fn no_effort_selector_anywhere() {
    // The composer row returns a mode, a permission, and the Auto chip's /why click. Nothing else.
    let crate::cards::SessionRowOut {
        mode: _,
        perm: _,
        why: _,
    } = crate::cards::SessionRowOut {
        mode: None,
        perm: None,
        why: false,
    };
    for (id, label) in crate::cards::composer_modes()
        .iter()
        .chain(crate::cards::permission_modes())
    {
        assert!(
            !id.contains("effort")
                && !grokhub_core::REASONING_EFFORTS
                    .iter()
                    .any(|(_, l)| l == label),
            "{id}"
        );
    }
    assert!(super::settings::DEFAULTS_ROWS
        .iter()
        .all(|r| !r.to_ascii_lowercase().contains("effort")));
    assert!(grokhub_core::SLASH_COMMANDS
        .iter()
        .all(|d| d.cmd != "/effort"));
    let help = grokhub_core::slash_help();
    assert!(help.contains("/effort — effort is automatic now"), "{help}");
    assert!(!help.contains("/effort <"), "{help}");
    let job = crate::cards::auto_chip_job("Medium");
    assert_eq!(job.text, "Auto · Medium");
}

// Landed from PR #106. `/health` is one chat block since card 10.
#[test]
fn health_slash_posts_updates_failed_services_and_the_last_dream_in_one_block() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("health-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/health");
    let empty = last_chat_text(&cabin);
    assert!(
        empty.contains("Health\n\nUpdates: none pending.\nServices: ")
            && empty.ends_with("\nLast dream: none yet. GrokHub dreams once a night after the review, in memory repo mode."),
        "{empty}"
    );
    assert!(
        cabin.status.starts_with("Health: no updates · "),
        "{}",
        cabin.status
    );

    let dreams = root.join("amr").join("dreams");
    std::fs::create_dir_all(&dreams).unwrap();
    std::fs::write(
        dreams.join("2026-10-08.md"),
        "# Memory dream 2026-10-08\n\n## Merged\n\n- Kept `n1` \"Uses pnpm\", merged `n2` \"uses pnpm\" because 80% shared words.\n\n## Retired\n\n- Retired `n9` \"old token path\" because confidence 0.2.\n",
    )
    .unwrap();
    cabin.cabin_latest = Some("v99.0.0".into());
    let failing = grokhub_core::mark_automation_failed(health_job("a1"), "disk full");
    cabin.automations = vec![failing];
    cabin.run_slash_line("/health");
    let block = last_chat_text(&cabin);
    assert!(
        block.contains("Health\n\nUpdates: GrokHub 99.0.0 is ready. Run /update.\nServices: "),
        "{block}"
    );
    assert!(
        block.contains("\n- Automation Board summary: Last run failed — disk full\n"),
        "{block}"
    );
    assert!(
        block.ends_with("\nLast dream: 2026-10-08, 1 merged, 1 retired."),
        "{block}"
    );
    assert!(
        cabin.status.starts_with("Health: 1 update · "),
        "{}",
        cabin.status
    );
    assert!(
        !cabin.status.ends_with("services ok"),
        "a failed automation is a failed service: {}",
        cabin.status
    );
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #107.
#[test]
fn undo_retry_and_context_on_an_empty_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("undo-retry");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.messages = std::sync::Arc::new(Vec::new());
    if let Some(t) = cabin.threads.get_mut(cabin.thread_idx) {
        t.messages = cabin.messages.clone();
    }
    cabin.run_slash_line("/undo");
    assert_eq!(cabin.status, "Nothing to undo");
    assert!(!cabin.running);
    cabin.run_slash_line("/retry");
    assert_eq!(cabin.status, "Nothing to retry");
    assert!(!cabin.running);
    cabin.run_slash_line("/context");
    assert_eq!(cabin.status, "0 turns · 0 tokens · 0% · pin none");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #108.
#[test]
fn mode_slash_sets_think_and_auto() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("mode-slash");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.model.clear();
    cabin.run_slash_line("/mode think");
    assert_eq!(cabin.cfg.mode, "think");
    assert_eq!(cabin.status, "Mode think → grok-4.7 · high");
    cabin.cfg.model.clear();
    cabin.run_slash_line("/mode auto");
    assert_eq!(cabin.cfg.mode, "auto");
    assert_eq!(
        cabin.status,
        "Mode auto — routes Fast / Balance / Think / Max"
    );
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #109.
#[test]
fn usage_and_host_slashes_set_status_without_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("usage-host");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/usage");
    assert!(cabin.status.starts_with("today "));
    assert!(cabin.status.contains(" · chat "));
    assert!(cabin.status.contains(" · imagine "));
    assert!(!cabin.running);
    cabin.run_slash_line("/host");
    assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #110.
#[test]
fn board_slash_opens_the_workboard_and_scratch_blocks_memory() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("board-scratch");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.board.clear();
    cabin.run_slash_line("/board");
    assert!(matches!(cabin.nav, Nav::Workboard));
    assert_eq!(cabin.status, "0 cards");
    cabin.threads = vec![crate::threads::ChatThread::new("Scratch", true)];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.run_slash_line("/remember harbor note");
    assert_eq!(cabin.status, "Scratch — no memory writes");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #112.
#[test]
fn fix_opens_about_and_dream_refuses_without_login() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("fix-dream");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("HOME", &root);
    std::env::set_var("GROKHUB_GROK", "/tmp/grokhub-no-such-grok");
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.api_key.clear();
    cabin.running = true;
    cabin.imagine_pending = true;
    cabin.run_slash_line("/fix");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(matches!(cabin.settings_sec, SettingsSec::About));
    assert!(!cabin.running, "fix must halt a live job");
    assert!(!cabin.imagine_pending, "fix must clear a pending imagine");
    assert_eq!(cabin.status, cabin.doctor_text());
    assert!(
        !cabin.llm_ready(),
        "this test must have no key and no grok binary"
    );
    cabin.run_slash_line("/dream");
    assert_eq!(cabin.status, "Sign in with Grok or add an API key.");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.imagine_want_focus);
    std::env::remove_var("GROKHUB_CONFIG");
    std::env::remove_var("GROKHUB_GROK");
}

// Landed from PR #113.
#[test]
fn remember_appends_memory_and_refuses_a_secret() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("remember-write");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body.clear();
    cabin.run_slash_line("/remember harbor light");
    assert_eq!(cabin.status, "Wrote MEMORY.md");
    assert!(
        cabin.mem_body.contains("harbor light"),
        "memory body was {}",
        cabin.mem_body
    );
    let before = cabin.mem_body.clone();
    cabin.run_slash_line("/remember sk-abcdefghijklmnopqrst");
    assert_eq!(cabin.status, "Secrets never in markdown");
    assert_eq!(cabin.mem_body, before);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #114.
#[test]
fn memory_goal_model_loop_forget_and_empty_imagine() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("memory-goal");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body = "harbor light\nkeep the dock\n".into();
    cabin.run_slash_line("/memory");
    assert!(matches!(cabin.nav, Nav::Memory));
    assert_eq!(cabin.status, "Memory");
    cabin.run_slash_line("/goal migrate auth");
    assert_eq!(cabin.cfg.goal_pin, "migrate auth");
    assert_eq!(cabin.status, "Goal: migrate auth");
    cabin.run_slash_line("/goal");
    assert_eq!(cabin.status, "Goal: migrate auth");
    cabin.run_slash_line("/goal clear");
    assert!(cabin.cfg.goal_pin.is_empty());
    assert_eq!(cabin.status, "Goal cleared");
    cabin.run_slash_line("/model grok-4.7");
    assert_eq!(cabin.cfg.model, "grok-4.7");
    assert_eq!(cabin.status, "grok --model grok-4.7");
    cabin.run_slash_line("/loop");
    assert!(matches!(cabin.nav, Nav::Night));
    assert!(cabin.auto_compose);
    cabin.run_slash_line("/forget harbor");
    assert_eq!(cabin.status, "Forgot harbor");
    assert!(!cabin.mem_body.to_ascii_lowercase().contains("harbor"));
    assert!(cabin.mem_body.contains("keep the dock"));
    cabin.run_slash_line("/forget");
    assert_eq!(cabin.status, "Forgot MEMORY.md");
    assert!(cabin.mem_body.is_empty());
    cabin.imagine_prompt.clear();
    cabin.run_slash_line("/imagine");
    assert!(matches!(cabin.nav, Nav::Imagine));
    assert!(cabin.imagine_want_focus);
    assert!(!cabin.running);
    assert!(!cabin.imagine_pending);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #115.
#[test]
fn skill_hub_help_and_clear() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("skill-hub");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.hub_on = false;
    cabin.skill_list.clear();
    cabin.run_slash_line("/skill missing-harbor");
    assert_eq!(cabin.status, "No skill missing-harbor");
    assert!(matches!(cabin.nav, Nav::Chat), "{:?}", cabin.nav);
    cabin.run_slash_line("/hub");
    assert!(matches!(cabin.nav, Nav::Devices));
    assert_eq!(cabin.status, "Start share on Devices");
    assert!(!cabin.hub_on);
    cabin.run_slash_line("/help");
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| m.0 == "assistant" && m.1.contains("/clear")),
        "help did not land on the transcript"
    );
    cabin.running = true;
    cabin.imagine_pending = true;
    cabin.live_mut().push(("user".into(), "harbor".into()));
    cabin.run_slash_line("/clear");
    assert!(!cabin.running, "clear must halt a live job");
    assert!(!cabin.imagine_pending);
    assert!(cabin.messages.is_empty());
    assert_eq!(cabin.status, "Cleared");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #116.
#[test]
fn rename_pin_and_delete_the_open_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("rename-pin");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/scratch");
    assert_eq!(cabin.threads.len(), 1);
    cabin.run_slash_line("/rename Harbor");
    assert_eq!(cabin.status, "Renamed Harbor");
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Harbor");
    assert!(cabin.threads[cabin.thread_idx].title_locked);
    cabin.run_slash_line("/pin");
    assert_eq!(cabin.status, "Pinned Harbor");
    assert!(cabin.threads[cabin.thread_idx].pinned);
    cabin.run_slash_line("/pin");
    assert_eq!(cabin.status, "Unpinned Harbor");
    assert!(!cabin.threads[cabin.thread_idx].pinned);
    cabin.run_slash_line("/delete");
    assert_eq!(cabin.status, "Chat deleted");
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].title, "Chat");
    assert!(!cabin.threads[0].scratch);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #117.
#[test]
fn export_writes_a_chat_and_empty_video_does_not_send() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("export-video");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/scratch");
    cabin.cfg.project_dir.clear();
    cabin.run_slash_line("/export");
    assert!(
        cabin.status.starts_with("Wrote ") && cabin.status.ends_with("export.md"),
        "export status was {}",
        cabin.status
    );
    cabin.imagine_prompt.clear();
    cabin.run_slash_line("/imagine-video");
    assert!(matches!(cabin.nav, Nav::Imagine));
    assert!(matches!(
        cabin.imagine_kind,
        grokhub_core::ImagineKind::Video
    ));
    assert!(cabin.imagine_want_focus);
    assert!(cabin.imagine_prompt.is_empty());
    assert!(!cabin.running);
    assert!(!cabin.imagine_pending);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #118.
#[test]
fn voice_without_login_stays_off_and_profile_clears() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("voice-profile");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let old_path = std::env::var_os("PATH");
    std::env::set_var("PATH", "");
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.api_key.clear();
    cabin.listen_voice();
    assert_eq!(cabin.status, "Connect Grok OAuth for STT/TTS.");
    assert!(!cabin.running, "voice must not start a listen job");
    cabin.cfg.profile_picture = "harbor.png".into();
    cabin.clear_profile_picture();
    assert!(cabin.cfg.profile_picture.is_empty());
    assert_eq!(cabin.status, "Saved");
    match old_path {
        Some(p) => std::env::set_var("PATH", p),
        None => std::env::remove_var("PATH"),
    }
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #119.
#[test]
fn scratch_btw_worktree_and_plan() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("scratch-btw");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.running = false;
    cabin.run_slash_line("/view-plan");
    assert_eq!(cabin.status, "No plan yet — use Plan mode");
    assert!(!cabin.plan_open);
    cabin.run_slash_line("/btw");
    assert!(matches!(cabin.session_mode, SessionMode::Ask));
    assert_eq!(cabin.cfg.session_mode, "ask");
    assert_eq!(cabin.status, "btw — look-safe side ask");
    cabin.run_slash_line("/scratch");
    assert_eq!(cabin.status, "Scratch — no memory writes");
    assert!(cabin.scratch());
    assert!(cabin.composer_want_focus);
    cabin.run_slash_line("/worktree");
    assert_eq!(
        cabin.status,
        "Worktrees were a grok CLI feature; chats run in the bound project folder"
    );
    cabin.threads[cabin.thread_idx].grok_session = Some("sess-harbor".into());
    let (count, idx) = (cabin.threads.len(), cabin.thread_idx);
    cabin.run_slash_line("/fork");
    assert_eq!(cabin.threads.len(), count, "/fork no longer opens a chat");
    assert_eq!(cabin.thread_idx, idx);
    assert!(!cabin.threads[idx].grok_fork);
    assert_eq!(
        cabin.status, "Worktrees were a grok CLI feature; chats run in the bound project folder",
        "/fork leaves the status alone"
    );
    cabin.threads[cabin.thread_idx].plan_body = "harbor steps".into();
    cabin.run_slash_line("/view-plan");
    assert!(cabin.plan_open);
    assert_eq!(cabin.status, "View plan");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #120.
#[test]
fn import_without_openclaw_and_consult_without_login() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("import-consult");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("HOME", &root);
    std::env::set_var("GROKHUB_GROK", "/tmp/grokhub-no-such-grok");
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.api_key.clear();
    cabin.run_slash_line("/import");
    assert_eq!(cabin.status, "Importing OpenClaw…");
    let start = std::time::Instant::now();
    while cabin.status == "Importing OpenClaw…"
        && start.elapsed() < std::time::Duration::from_secs(2)
    {
        cabin.poll_import_openclaw();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        cabin.status,
        "No OpenClaw workspace (~/.openclaw/workspace)"
    );
    assert!(!matches!(cabin.nav, Nav::Memory));
    assert!(
        !cabin.llm_ready(),
        "consult must have no key and no grok binary"
    );
    cabin.run_slash_line("/consult harbor");
    assert_eq!(cabin.status, "Sign in with Grok or add an API key.");
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
    std::env::remove_var("GROKHUB_GROK");
}

// Landed from PR #122.
#[test]
fn skills_connectors_and_sessions_without_grok() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("skills-sessions");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", "/tmp/grokhub-no-such-grok");
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/skills");
    assert!(matches!(cabin.nav, Nav::Skills));
    cabin.run_slash_line("/connectors");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    cabin.run_slash_line("/dashboard");
    assert!(matches!(cabin.nav, Nav::History));
    assert_eq!(cabin.status, "History");
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
    std::env::remove_var("GROKHUB_GROK");
}

// Landed from PR #124.
#[test]
fn room_binds_a_work_tree_and_rewind_files_needs_a_project() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("room");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home_s = home.display().to_string().trim_end_matches('/').to_string();
    let prev_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &home_s);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/rewind --files");
    assert_eq!(cabin.status, "Bind a project first — /project bind");
    assert!(!cabin.running);
    cabin.run_slash_line("/room harbor");
    let bound = format!("{home_s}/GrokHub-Work/harbor");
    assert_eq!(cabin.cfg.project_dir, bound);
    assert!(cabin.project_sel.is_some());
    assert_eq!(cabin.status, format!("Room harbor → {bound}"));
    assert!(!cabin.running);
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| m.0 == "user" && m.1.contains("blocked: outside bound project")),
        "room host script should stay inside the host rail: {:?}",
        cabin.messages
    );
    let dir = std::path::PathBuf::from(&bound);
    let start = std::time::Instant::now();
    while !dir.is_dir() && start.elapsed() < std::time::Duration::from_secs(2) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(dir.is_dir(), "room directory was not created");
    std::env::remove_var("GROKHUB_CONFIG");
    match prev_home {
        Some(h) => std::env::set_var("HOME", h),
        None => std::env::remove_var("HOME"),
    }
}

// Landed from PR #125.
#[test]
fn inhabit_refuses_a_missing_peer() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("inhabit");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/inhabit cabin-2");
    assert_eq!(cabin.status, "No paired peer named cabin-2");
    assert!(!cabin.running);
    assert!(cabin.inhabit_rx.is_none());
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #126.
#[test]
fn rewind_and_compact_stay_closed_without_grok() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("rewind-compact");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", "/tmp/grokhub-no-such-grok");
    let mut cabin = Cabin::quiet_for_test();
    cabin.live_mut().push(("user".into(), "harbor".into()));
    cabin.live_mut().push(("assistant".into(), "light".into()));
    cabin.run_slash_line("/rewind");
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| m.0 == "assistant" && m.1 == "light"),
        "rewind dropped the reply without a grok agent: {:?}",
        cabin.messages
    );
    assert!(!cabin.running);
    assert_eq!(
        cabin.status, "N/A on native threads: the session is append-only.",
        "rewind is not offered on native threads"
    );
    assert_ne!(cabin.status, "Rewinding Grok conversation…");
    cabin.live_mut().clear();
    for i in 0..9 {
        cabin.live_mut().push(("user".into(), format!("turn-{i}")));
    }
    cabin.run_slash_line("/compact");
    assert!(
        cabin.messages.iter().any(|m| m.1 == "turn-0"),
        "compact dropped history without a grok agent: {:?}",
        cabin.messages
    );
    assert_eq!(cabin.messages.len(), 9);
    assert!(!cabin.running);
    assert_ne!(cabin.status, "Compacting…", "no chat, nothing to compact");
    std::env::remove_var("GROKHUB_CONFIG");
    std::env::remove_var("GROKHUB_GROK");
}

// Landed from PR #128.
#[test]
fn models_and_inspect_without_grok() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("models-inspect");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", "/tmp/grokhub-no-such-grok");
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/models");
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| { m.0 == "assistant" && m.1.contains("grok-4.7 — Grok 4.7 (chat)") }),
        "models catalog did not land: {:?}",
        cabin.messages
    );
    assert!(!cabin.running);
    cabin.run_slash_line("/inspect");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
    std::env::remove_var("GROKHUB_GROK");
}

// Landed from PR #129.
#[test]
fn permission_slashes_arm_always_and_set_auto() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("permission-slash");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert!(matches!(cabin.permission_mode, PermissionMode::Ask));
    cabin.run_slash_line("/always-approve");
    assert_eq!(cabin.status, "Confirm Always…");
    assert!(matches!(
        cabin.confirm,
        Some(super::confirm::ConfirmKind::AlwaysSession)
    ));
    assert!(matches!(cabin.permission_mode, PermissionMode::Ask));
    assert!(!cabin.running);
    cabin.apply_session_always();
    assert!(matches!(
        cabin.permission_mode,
        PermissionMode::AlwaysApprove
    ));
    assert_eq!(cabin.cfg.permission_mode, "ask");
    assert_eq!(cabin.status, "Permission always-approve");
    assert!(cabin.confirm.is_none());
    cabin.run_slash_line("/always-approve");
    assert!(matches!(cabin.permission_mode, PermissionMode::Ask));
    assert_eq!(cabin.cfg.permission_mode, "ask");
    assert_eq!(cabin.status, "Permission ask");
    cabin.run_slash_line("/auto");
    assert!(matches!(cabin.permission_mode, PermissionMode::Auto));
    assert_eq!(cabin.cfg.permission_mode, "auto");
    assert_eq!(cabin.status, "Permission auto");
    assert!(cabin.confirm.is_none());
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #130.
#[test]
fn learn_reflects_nothing_new_and_scratch_refuses() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("learn-reflect");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/learn");
    assert_eq!(cabin.status, "Reflecting…");
    assert!(cabin.reflect_rx.is_some());
    cabin.run_slash_line("/learn");
    assert_eq!(cabin.status, "Reflecting…");
    let start = std::time::Instant::now();
    while cabin.reflect_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(2) {
        cabin.poll_reflect();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(cabin.status, "Reflect: nothing new");
    assert!(cabin.reflect_rx.is_none());
    assert!(!cabin.running);
    cabin.run_slash_line("/scratch");
    cabin.run_slash_line("/learn");
    assert_eq!(cabin.status, "Scratch — no reflect");
    assert!(cabin.reflect_rx.is_none());
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #131.
#[test]
fn appearance_theme_tray_and_wall_save() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("appearance");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert_eq!(cabin.cfg.theme, "dark");
    assert!(cabin.cfg.close_to_tray);
    assert!(cabin.cfg.imagine_wall);
    cabin.choose_theme(grokhub_core::ThemeChoice::Light);
    assert_eq!(cabin.cfg.theme, "light");
    assert_eq!(cabin.status, "Saved");
    cabin.choose_theme(grokhub_core::ThemeChoice::Light);
    assert_eq!(cabin.cfg.theme, "light");
    cabin.choose_theme(grokhub_core::ThemeChoice::System);
    assert_eq!(cabin.cfg.theme, "system");
    assert_eq!(cabin.status, "Saved");
    cabin.set_close_to_tray(false);
    assert!(!cabin.cfg.close_to_tray);
    assert_eq!(cabin.status, "Saved");
    cabin.set_living_wall(false);
    assert!(!cabin.cfg.imagine_wall);
    assert_eq!(cabin.status, "Saved");
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #134.
#[test]
fn composer_slash_runs_and_plain_text_refuses_without_grok() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("composer-send");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let missing = root.join("no-grok");
    let prev_grok = std::env::var_os("GROKHUB_GROK");
    std::env::set_var("GROKHUB_GROK", &missing);
    let mut cabin = Cabin::quiet_for_test();
    cabin.send_from_composer("/help".into());
    assert!(!cabin.running);
    assert!(
        cabin
            .messages
            .iter()
            .any(|(_, body)| body.contains("/clear")),
        "help lands on the open chat"
    );
    let before = cabin.messages.len();
    cabin.send_from_composer("hello harbor".into());
    assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
    assert!(!cabin.running);
    assert_eq!(cabin.messages.len(), before);
    assert!(cabin.messages.iter().all(|(role, _)| role != "user"));
    match prev_grok {
        Some(v) => std::env::set_var("GROKHUB_GROK", v),
        None => std::env::remove_var("GROKHUB_GROK"),
    }
}

#[test]
fn workflow_slashes_say_workflows_are_gone_and_never_touch_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("workflow-usage");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    for with_thread in [false, true] {
        let mut cabin = Cabin::quiet_for_test();
        if with_thread {
            cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
            cabin.thread_idx = 0;
        }
        cabin.running = true;
        for line in [
            "/workflow pause",
            "/workflow pause review-2",
            "/workflow review",
            "/workflow",
            "/workflows",
        ] {
            cabin.status.clear();
            cabin.run_slash_line(line);
            assert_eq!(
                cabin.status, "Workflows were a grok CLI feature and are not in GrokHub",
                "{line} (thread: {with_thread})"
            );
            assert!(cabin.running, "{line} must not touch the live turn");
            assert!(matches!(cabin.nav, Nav::Chat), "{line}");
        }
    }

    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/mcps");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    cabin.run_slash_line("/plugins");
    assert!(matches!(cabin.nav, Nav::Settings));
    cabin.run_slash_line("/marketplace");
    assert!(matches!(cabin.nav, Nav::Settings));
    cabin.run_slash_line("/connectors");
    assert!(matches!(cabin.nav, Nav::Settings));
    cabin.run_slash_line("/hooks");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.running);

    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn hooks_slash_opens_connectors_on_the_hooks_section() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("hooks-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", root.join("missing-grok"));

    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/mcps");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    assert!(!cabin.scroll_to_hooks);
    cabin.run_slash_line("/plugins");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.scroll_to_hooks);
    cabin.run_slash_line("/marketplace");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.scroll_to_hooks);
    cabin.run_slash_line("/connectors");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.scroll_to_hooks);
    cabin.run_slash_line("/hooks");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    assert!(cabin.scroll_to_hooks);

    std::env::remove_var("GROKHUB_GROK");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #161.
#[test]
fn model_slash_saves_without_a_run() {
    let _hold = config::hold_test_config();
    let root = config::test_config_root("model-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.run_slash(Slash::Model("grok-4.7".into()));

    assert_eq!(cabin.cfg.model, "grok-4.7");
    assert_eq!(cabin.status, "grok --model grok-4.7");
    assert!(!cabin.running);

    let path = root.join("app.json");
    let started = Instant::now();
    let mut body = String::new();
    while started.elapsed() < Duration::from_secs(3) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            body = text;
            if body.contains("\"model\": \"grok-4.7\"") {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        path.starts_with(&root),
        "model file must stay under the test config root"
    );
    assert!(
        body.contains("\"model\": \"grok-4.7\""),
        "model was not persisted under {}: {body}",
        path.display()
    );
    assert_eq!(config::load().model, "grok-4.7");

    std::env::remove_var("GROKHUB_CONFIG");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn help_slash_lists_commands_without_a_run() {
    struct RestoreConfig(Option<String>);
    impl Drop for RestoreConfig {
        fn drop(&mut self) {
            match self.0.take() {
                Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
                None => std::env::remove_var("GROKHUB_CONFIG"),
            }
        }
    }

    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("help-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    let _restore = RestoreConfig(std::env::var("GROKHUB_CONFIG").ok());
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    assert!(!cabin.running, "help starts from an idle cabin");
    let before = cabin.messages.len();
    let grok_before = grok_process_ids();
    cabin.run_slash(Slash::Help);
    let gained: Vec<&(String, String)> = cabin.messages.iter().skip(before).collect();
    assert!(
        matches!(
            gained.as_slice(),
            [(role, text)] if role == "assistant" && text.contains("/help — this list")
        ),
        "transcript must gain one assistant line containing /help — this list, got {gained:?}"
    );
    assert!(!cabin.running, "help must not start a run");
    assert_eq!(
        grok_before,
        grok_process_ids(),
        "help must not start a grok process"
    );
}

// Landed from PR #167.
#[test]
fn unknown_slash_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("unknown-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }

    let status = cabin.status.clone();
    let transcript = cabin.messages.clone();
    let running = cabin.running;

    cabin.send_chat(String::new());
    assert_eq!(cabin.status, status);
    assert_eq!(&*cabin.messages, &*transcript);
    assert_eq!(cabin.running, running);

    cabin.send_chat("/not-a-command".to_string());
    assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
    assert_eq!(&*cabin.messages, &*transcript);
    assert!(!cabin.running);
    let transcript = cabin.messages.clone();

    cabin.send_chat("/approve".to_string());
    assert_eq!(cabin.status, "Unknown command — /help");
    assert_eq!(&*cabin.messages, &*transcript);
    assert!(!cabin.running);
}

// Landed from PR #168.
#[test]
fn slash_pick_fills_or_returns_the_command() {
    let mut composer = String::new();
    let run = super::slash_pick_take(&mut composer, "/help", true);
    assert_eq!(run.as_deref(), Some("/help"));
    assert!(composer.is_empty());

    let mut composer = String::new();
    let run = super::slash_pick_take(&mut composer, "/model ", false);
    assert!(run.is_none());
    assert_eq!(composer, "/model ");

    assert_eq!(super::slash_pick_step(0, 3, 1), 1);
    assert_eq!(super::slash_pick_step(2, 3, 1), 2);
    assert_eq!(super::slash_pick_step(0, 3, -1), 0);
    assert_eq!(super::slash_pick_step(0, 0, 1), 0);

    assert_eq!(super::slash_pick_retain(2, true, 5), 0);
    assert_eq!(super::slash_pick_retain(2, false, 5), 2);
}

// Landed from PR #175.
#[test]
fn clear_slash_empties_the_chat() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("clear-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    let idx = cabin.thread_idx;
    let thread_id = cabin.threads[idx].id.clone();
    {
        let thread = &mut cabin.threads[idx];
        thread.messages_mut().push(("user".into(), "hello".into()));
        thread.grok_session = Some("sess".into());
    }
    cabin.messages = cabin.threads[idx].messages.clone();
    assert!(!cabin.running);

    cabin.run_slash(Slash::Clear);

    assert_eq!(cabin.status, "Cleared");
    assert!(cabin.messages.is_empty());
    let thread = cabin
        .threads
        .iter()
        .find(|t| t.id == thread_id)
        .expect("seeded thread");
    assert!(thread.messages.is_empty());
    assert!(thread.grok_session.as_deref().unwrap_or("").is_empty());
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #176.
#[test]
fn context_slash_counts_an_empty_chat() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("context-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    if cabin.threads.get(cabin.thread_idx).is_none() {
        cabin.new_thread(false);
    }
    assert!(cabin.grok_usage.is_empty());
    assert!(cabin.cfg.goal_pin.is_empty());
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(super::Slash::Context);

    assert_eq!(cabin.status, "0 turns · 0 tokens · 0% · pin none");
    assert!(!cabin.running);
    assert!(cabin.messages.is_empty());
}

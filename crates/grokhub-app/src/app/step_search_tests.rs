//! Spike-3a step search tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

// Spike-3a: tool steps join History and palette search (legacy mode too),
// Enter goes back to that chat and turn, and a turn with harness spans leaves
// one trail node in memory repo mode only.
fn spike3a_click(chat: &str, turn: u32, ts: u64) -> grokhub_agent::harness::Span {
    let mut s = grokhub_agent::harness::Span::soft_allow(
        chat,
        "click",
        r#"{"x":40,"y":12}"#,
        "clicked",
        "click OK",
        grokhub_agent::harness::AccessMode::Supervised,
        "grok_build",
    )
    .on_path("A")
    .in_turn(chat, turn);
    s.ts_ms = ts;
    s
}

fn spike3a_wait(done: &mut dyn FnMut() -> bool) {
    let start = std::time::Instant::now();
    while !done() && start.elapsed() < std::time::Duration::from_secs(5) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn step_search_finds_click_ok_off_the_ui_thread_and_enter_jumps_to_the_turn() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("spike3a-search");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert!(
        cabin.cfg.memory_backend.is_legacy(),
        "search needs no memory repo"
    );
    cabin.threads.clear();
    let harbor = crate::threads::ChatThread::new("Harbor", false);
    let pier = crate::threads::ChatThread::new("Pier", false);
    let mut hidden = crate::threads::ChatThread::new("Hidden", false);
    hidden.scratch = true;
    let (harbor_id, pier_id, hidden_id) = (harbor.id.clone(), pier.id.clone(), hidden.id.clone());
    cabin.threads.push(harbor);
    cabin.threads.push(pier);
    cabin.threads.push(hidden);
    cabin.thread_idx = 0;
    grokhub_agent::harness::append_span(&root, &spike3a_click(&pier_id, 2, 1_000)).unwrap();
    grokhub_agent::harness::append_span(&root, &spike3a_click(&hidden_id, 1, 2_000)).unwrap();

    cabin.history_q = "click OK".into();
    cabin.kick_history_search();
    assert!(
        cabin.history_rx.is_some() && cabin.history_hits.is_empty(),
        "search runs off the UI thread"
    );
    spike3a_wait(&mut || {
        cabin.poll_history_search();
        cabin.history_rx.is_none()
    });
    let target = format!("step:2:{pier_id}");
    assert_eq!(
        cabin.history_hits,
        vec![(target.clone(), "Pier · turn 2 · click · allow".to_string())],
        "the scratch chat's step stays out"
    );
    cabin.open_history_hit(&target);
    assert_eq!(cabin.threads[cabin.thread_idx].id, pier_id);
    assert_eq!(cabin.jump_turn, Some(2));
    assert!(matches!(cabin.nav, Nav::Chat));

    // The palette: the same row after the commands, and Enter takes it.
    cabin.jump_turn = None;
    let harbor_idx = cabin
        .threads
        .iter()
        .position(|t| t.id == harbor_id)
        .unwrap();
    cabin.switch_thread(harbor_idx);
    cabin.open_palette();
    cabin.palette_q = "click OK".into();
    cabin.kick_palette_steps();
    spike3a_wait(&mut || {
        cabin.poll_palette_search();
        cabin.palette_step_rx.is_none()
    });
    assert_eq!(
        cabin.palette_steps,
        vec![(target.clone(), "Pier · turn 2 · click · allow".to_string())]
    );
    let cmds = grokhub_core::filter_palette(&cabin.palette_q);
    let pick = super::palette::palette_pick(&cmds, &[], "", &cabin.palette_steps, cmds.len());
    assert_eq!(pick.as_deref(), Some(target.as_str()));
    cabin.run_palette(&egui::Context::default(), &target);
    assert_eq!(cabin.threads[cabin.thread_idx].id, pier_id);
    assert_eq!(cabin.jump_turn, Some(2));
    cabin.open_history_hit("step:2:gone-chat");
    assert_eq!(cabin.status, "That chat is gone");

    // Legacy mode writes no trail.
    assert!(cabin.write_turn_trail(&pier_id, 2).is_none());
    cabin.harness_turn_end("Clicked OK.", 2);
    assert!(!root.join("amr").exists());
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn memory_repo_turn_end_writes_one_trail_and_a_scratch_chat_writes_none() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("spike3a-trail");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    hx::use_key_store_for(&root, std::sync::Arc::new(hx::MemoryKeyStore::new()));
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    cabin.threads.clear();
    let dock = crate::threads::ChatThread::new("Dock", false);
    let mut scratch = crate::threads::ChatThread::new("Scratch", false);
    scratch.scratch = true;
    let (dock_id, scratch_id) = (dock.id.clone(), scratch.id.clone());
    cabin.threads.push(dock);
    cabin.threads.push(scratch);
    cabin.thread_idx = 0;
    hx::append_span(&root, &spike3a_click(&dock_id, 1, 1_791_331_200_000)).unwrap();
    cabin.harness_turn_end("Clicked OK.", 1);
    let trail = root
        .join("amr")
        .join("nodes")
        .join(format!("{}.md", grokhub_core::amr::trail_id(&dock_id, 1)));
    let expected_tail = format!(
        "---\nTurn 1: 1 step.\nTools: click x1\nDecisions: allow 1\nScreen changed: not measured\nFindings: none\nVerify: none\nSpans: {dock_id}:1791331200000\n"
    );
    // Wait for the body, not just create: remember used to publish an empty file first.
    let mut md = String::new();
    spike3a_wait(&mut || {
        md = std::fs::read_to_string(&trail).unwrap_or_default();
        md.ends_with(&expected_tail)
    });
    assert!(
        md.ends_with(&expected_tail),
        "trail body missing after wait: {md}"
    );
    assert!(
        md.contains(&format!("source: span:{dock_id}:1791331200000\n")),
        "{md}"
    );
    let trails = |root: &std::path::Path| {
        std::fs::read_dir(root.join("amr").join("nodes"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("trail-"))
            .count()
    };
    assert_eq!(trails(&root), 1);

    // A scratch chat: same kind of turn, no trail.
    cabin.thread_idx = 1;
    hx::append_span(&root, &spike3a_click(&scratch_id, 1, 1_791_331_300_000)).unwrap();
    assert!(cabin.write_turn_trail(&scratch_id, 1).is_none());
    cabin.harness_turn_end("Clicked OK.", 1);
    assert_eq!(trails(&root), 1);
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn spike3a_adds_no_nav_page() {
    // Exhaustive on purpose: a new page fails to compile here.
    let pages = [
        Nav::Chat,
        Nav::Devices,
        Nav::Memory,
        Nav::Workboard,
        Nav::Pulse,
        Nav::Imagine,
        Nav::Skills,
        Nav::Library,
        Nav::Night,
        Nav::History,
        Nav::Journal,
        Nav::Command,
        Nav::Agents,
        Nav::Settings,
    ];
    let named: Vec<&str> = pages
        .iter()
        .map(|nav| match nav {
            Nav::Chat => "chat",
            Nav::Devices => "devices",
            Nav::Memory => "memory",
            Nav::Workboard => "workboard",
            Nav::Pulse => "pulse",
            Nav::Imagine => "imagine",
            Nav::Skills => "skills",
            Nav::Library => "library",
            Nav::Night => "night",
            Nav::History => "history",
            Nav::Journal => "journal",
            Nav::Command => "command",
            Nav::Agents => "agents",
            Nav::Settings => "settings",
        })
        .collect();
    assert_eq!(named.len(), 14);
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("spike3a-nav");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads.clear();
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Pier", false));
    let id = cabin.threads[0].id.clone();
    cabin.open_history_hit(&format!("step:1:{id}"));
    assert!(
        matches!(cabin.nav, Nav::Chat),
        "a step hit opens the chat, not a page of its own"
    );
    std::env::remove_var("GROKHUB_CONFIG");
}

//! The app reads each typed message: side answers, steer, stop and queue
//! while a reply runs; plan first or just answer while nothing does.

use super::*;
use grokhub_agent::route::intent::TurnRoute;

#[test]
fn the_side_prompt_answers_only_and_sees_the_work() {
    let p = super::super::side_run::side_prompt(
        "what's the RAM on this?",
        "refactor the settings page",
        "I moved the toggles into one list",
    );
    assert!(p.contains("Side question: what's the RAM on this?"), "{p}");
    assert!(p.contains("refactor the settings page"), "{p}");
    assert!(p.contains("I moved the toggles"), "{p}");
    assert!(
        p.contains("Do not continue, change or redo the main task"),
        "{p}"
    );
    let long = "x".repeat(5_000);
    let clipped = super::super::side_run::side_prompt("q", &long, &long);
    assert!(
        clipped.len() < 1_600,
        "the snapshot stays short: {}",
        clipped.len()
    );
    let told = super::super::side_run::tell_main_text("which port?", "8080");
    assert!(
        told.contains("which port?") && told.contains("8080"),
        "{told}"
    );
}

#[cfg(unix)]
#[test]
fn a_side_question_during_a_reply_gets_its_own_answer_and_leaves_the_work_alone() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("side-run");
    let _tx = live_reply(&mut cabin, "slow-turn please", "Working on it.");
    let before = cabin.messages.clone();
    cabin.send_chat("btw what time zone is the server in?".into());
    assert_eq!(cabin.status, TurnRoute::Side.ack());
    assert!(cabin.running, "the main reply keeps going");
    assert!(cabin.followup_queue.is_empty(), "nothing waits in line");
    assert_eq!(
        cabin.messages, before,
        "the side question never enters the chat"
    );
    assert_eq!(cabin.side.runs.len(), 1);
    assert_eq!(
        cabin.side.runs[0].question,
        "btw what time zone is the server in?"
    );
    let start = std::time::Instant::now();
    while cabin.side.runs[0].live() && start.elapsed() < std::time::Duration::from_secs(10) {
        cabin.poll_side_runs();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        !cabin.side.runs[0].live(),
        "the side run ends after its answer"
    );
    assert!(
        !cabin.side.runs[0].answer.trim().is_empty() || cabin.side.runs[0].failed.is_some(),
        "the side card has something to show"
    );
    assert_eq!(
        cabin.messages, before,
        "its answer stays off the chat history"
    );
    assert!(
        grokhub_agent::load_session(&cabin.side.runs[0].session).is_err(),
        "the side run's copy of the chat is gone, so History never lists it"
    );
    end_bg_test(root, cabin);
}

#[cfg(unix)]
#[test]
fn stop_words_stop_and_a_new_job_waits_its_turn() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("route-stop");
    let _tx = live_reply(&mut cabin, "slow-turn please", "Working on it.");
    cabin.send_chat("then write the release notes for the docs site".into());
    assert_eq!(
        cabin.followup_queue,
        vec!["then write the release notes for the docs site".to_string()]
    );
    assert!(
        cabin.status.starts_with(TurnRoute::Queue.ack()),
        "{}",
        cabin.status
    );
    assert!(cabin.side.runs.is_empty());
    cabin.send_chat("stop".into());
    assert!(!cabin.running, "stop stops the reply");
    end_bg_test(root, cabin);
}

#[test]
fn a_big_job_plans_first_and_a_question_is_just_answered() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("route-idle");
    cabin.settle_turn_mode_for_test(
        "build me a full website with a login page, a dashboard and a settings page",
    );
    assert_eq!(cabin.session_mode, SessionMode::Plan);
    assert_eq!(cabin.status, TurnRoute::Plan.ack());
    // The plan is still waiting on approval: feedback keeps planning.
    cabin.settle_turn_mode_for_test("make step 2 shorter");
    assert_eq!(cabin.session_mode, SessionMode::Plan);
    // "No, just answer" leaves planning and teaches the reader.
    cabin.settle_turn_mode_for_test("no, just answer");
    assert_eq!(cabin.session_mode, SessionMode::Chat);
    cabin.settle_turn_mode_for_test("what's the capital of France?");
    assert_eq!(cabin.session_mode, SessionMode::Chat);
    // Bare /plan: the next message plans, once.
    cabin.run_slash(Slash::Plan);
    assert_eq!(cabin.status, super::super::slash::PLAN_NEXT_NOTE);
    cabin.settle_turn_mode_for_test("rename the button");
    assert_eq!(cabin.session_mode, SessionMode::Plan);
    assert!(!cabin.plan_next);
    // A go runs the waiting plan instead of planning it again.
    cabin.settle_turn_mode_for_test(grokhub_core::reply_chips::PLAN_GO_TEXT);
    assert_eq!(cabin.session_mode, SessionMode::Chat);
    cabin.run_slash(Slash::Plan);
    cabin.settle_turn_mode_for_test("rename the button");
    cabin.settle_turn_mode_for_test("Go ahead, but skip step 3");
    assert_eq!(cabin.session_mode, SessionMode::Chat);
    release_isolated(&root, cabin);
}

#[test]
fn nothing_saves_a_session_mode_and_btw_is_not_a_mode() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("route-btw");
    cabin.run_slash(Slash::Btw);
    assert_eq!(cabin.session_mode, SessionMode::Chat);
    assert_eq!(cabin.status, super::super::slash::BTW_HINT);
    cabin.set_session_mode(SessionMode::Plan);
    let body = serde_json::to_string(&cabin.cfg).expect("cfg");
    assert!(!body.contains("sessionMode"), "{body}");
    release_isolated(&root, cabin);
}

#[test]
fn permissions_change_only_in_settings_or_the_prompt() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("perm-settings");
    // /auto and /always-approve point at Settings and change nothing.
    let before = cabin.permission_mode;
    cabin.run_slash(Slash::AutoPerm);
    assert_eq!(cabin.permission_mode, before);
    assert_eq!(cabin.nav, Nav::Settings);
    assert_eq!(cabin.settings_sec, SettingsSec::Defaults);
    assert_eq!(cabin.status, super::super::slash::PERMISSION_IN_SETTINGS);
    cabin.run_slash(Slash::AlwaysApprove);
    assert_eq!(cabin.permission_mode, before);
    // Settings offers all three in plain words and shows Always as Always.
    let ids: Vec<_> = super::super::settings::cabin_default_permissions()
        .iter()
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(ids, ["ask", "auto", "always-approve"]);
    assert_eq!(
        super::super::settings::cabin_default_permission_id("always-approve"),
        "always-approve"
    );
    assert_eq!(
        super::super::settings::cabin_default_permission_id("nonsense"),
        "ask"
    );
    cabin.pick_permission_mode(PermissionMode::Auto);
    assert_eq!(cabin.permission_mode, PermissionMode::Auto);
    assert_eq!(cabin.cfg.permission_mode, "auto");
    // Don't ask goes behind the one-time confirm.
    cabin.pick_permission_mode(PermissionMode::AlwaysApprove);
    assert_eq!(cabin.permission_mode, PermissionMode::Auto);
    assert!(matches!(cabin.confirm, Some(ConfirmKind::AlwaysSession)));
    // No permission control is painted anywhere near the chat box.
    let chat = include_str!("chat_ui.rs");
    assert!(!chat.contains(["session", "_row("].concat().as_str()));
    let privacy = include_str!("privacy_page.rs");
    assert!(!privacy.contains(["set_asks", "_first"].concat().as_str()));
    release_isolated(&root, cabin);
}

#[test]
fn the_permission_card_offers_once_always_deny_and_stop_asking() {
    let src = include_str!("chat_ui.rs");
    let card = src
        .split("pub(super) fn paint_perm_ask(")
        .nth(1)
        .expect("paint_perm_ask");
    for label in [
        "\"Allow once\"",
        "\"Always allow\"",
        "\"Deny\"",
        "\"Stop asking me\"",
    ] {
        assert!(card.contains(label), "{label} missing");
    }
    let always = card
        .split("if always_this {")
        .nth(1)
        .and_then(|s| s.split("if stop_asking {").next())
        .expect("always branch");
    assert!(
        always.contains("answer_permission_always") && !always.contains("set_permission_mode"),
        "Always allow grants this kind of step and keeps the setting: {always}"
    );
    let stop = card
        .split("if stop_asking {")
        .nth(1)
        .and_then(|s| s.split("if always_confirm_matches_rpc").next())
        .expect("stop branch");
    assert!(
        stop.contains("PermissionMode::Auto") && stop.contains("perm_always_confirm"),
        "Stop asking me moves one step, Don't ask behind the confirm: {stop}"
    );
}

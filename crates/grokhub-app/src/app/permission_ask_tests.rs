//! Permission asks and session modes tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

#[test]
fn always_permission_keeps_the_acp_session() {
    let src = cabin_src();
    let ask = src
        .split("fn paint_perm_ask(")
        .nth(1)
        .and_then(|s| s.split("fn ui_empty_home").next())
        .expect("paint_perm_ask");
    assert!(
        !ask.contains("self.acp = None"),
        "Always on a live prompt must not drop the ACP session: {ask}"
    );
    assert!(
        ask.contains("p.reason") && ask.contains("p.action") && src.contains("fn paint_try_again("),
        "the Ask card says the action on one line, and hook reasons still paint: {ask}"
    );
    assert!(
        !ask.contains("!action.starts_with('{')"),
        "a bracket test or brace group must still paint on the Ask card: {ask}"
    );
    assert!(
        src.contains("fn paint_elicit_ask(")
            && src.contains("answer_elicit")
            && src.contains(".password(")
            && src.contains("redact_held_secrets")
            && src.contains("fn scrub_live_blocks("),
        "a secret elicit is masked and its value stays out of the transcript: {src}"
    );
    let finish = src
        .split("fn finish_acp_turn(")
        .nth(1)
        .and_then(|s| s.split("fn poll_host_diff(").next())
        .expect("finish_acp_turn");
    assert!(
        finish.contains("scrub_transcript") && finish.contains("scrub_live_blocks"),
        "held secrets must leave the live transcript and TTS: {finish}"
    );
    let grok_p = src
        .split("fn poll_single(")
        .nth(1)
        .and_then(|s| s.split("fn apply_single_turn(").next())
        .expect("poll_single");
    assert!(
        grok_p.contains("redact_held_secrets") && grok_p.contains("scrub_live_blocks"),
        "the grok-p pump must scrub held secrets: {grok_p}"
    );
    assert!(
        ask.contains("perm_key(")
            && ask.contains("PermKey::Allow")
            && ask.contains("PermKey::Deny"),
        "the shortcut sheet promises Enter / Esc on a permission card: {ask}"
    );
    assert!(
        ask.contains("perm_always_confirm")
            && ask.contains("paint_confirm_sheet")
            && ask.contains("always_session_spec")
            && ask.contains("ALWAYS_CONFIRM_LINE2")
            && ask.contains("ConfirmAct::Confirm")
            && ask.contains("ConfirmAct::Cancel"),
        "Ask Always is a second beat that names session skip and scheduled inherit: {ask}"
    );
    let always_click = ask
        .find("RichText::new(\"Stop asking me\")")
        .expect("Stop asking me");
    let set_always = ask
        .find("set_permission_mode(PermissionMode::AlwaysApprove)")
        .expect("Always mode");
    assert!(
        always_click < set_always && ask[always_click..set_always].contains("perm_always_confirm"),
        "Stop asking me must confirm before the setting becomes Don't ask: {ask}"
    );
    assert!(
        ask.contains("ghost_pill(ui, \"Always allow\")"),
        "Always allow is a per-step grant on the card: {ask}"
    );
    assert!(
        ask.contains("always_confirm_matches_rpc") && ask.contains("p.rpc_id"),
        "Always confirm must drop when the prompt/rpc_id changes: {ask}"
    );
    assert!(
        ask.contains("self.composer"),
        "Enter must send a typed follow-up instead of approving a tool: {ask}"
    );
    let poll = src
        .split("fn poll_acp(")
        .nth(1)
        .and_then(|s| s.split("fn finish_acp_turn").next())
        .expect("poll_acp");
    assert!(
        poll.contains("auto_allows()"),
        "Auto permission must answer ACP prompts, not only Always: {poll}"
    );
    assert!(
        poll.contains("answer_permission_always"),
        "Always must answer allow-always, not allow-once: {poll}"
    );
    assert!(
        poll.contains("perm_always_confirm = None"),
        "a replacement Ask must drop the Always confirm beat: {poll}"
    );
    let err = poll.split("AcpEvent::Err").nth(1).expect("acp err");
    let classify = err
        .find("classify_stream_error")
        .expect("classify 1.0.13 errors");
    let drop_acp = err.find("self.acp = None").expect("drop acp");
    assert!(
        classify < drop_acp,
        "transient 5xx / truncation must not drop ACP: {err}"
    );
    let confirm_src = include_str!("confirm.rs");
    let retake = fn_src(confirm_src, "retake_permission_session");
    assert!(
        retake.contains("grok_session = None"),
        "a new permission setting must session/new or it does not take: {retake}"
    );
    assert!(
        retake.contains("persist_idle_key") && !retake.contains("self.persist()"),
        "a permission change must not clone every thread; bump the idle key so persist_bg skips: {retake}"
    );
    for (file, func) in [
        (confirm_src, "pick_permission_mode"),
        (confirm_src, "apply_session_always"),
    ] {
        let body = fn_src(file, func);
        assert!(body.contains("retake_permission_session"), "{func}: {body}");
    }
    let appearance = src
        .split("SettingsSec::Appearance => {")
        .nth(1)
        .and_then(|s| s.split("SettingsSec::Behavior => {").next())
        .expect("Appearance");
    assert!(
        appearance.contains("self.choose_theme(")
            && !appearance.contains("save = true")
            && !appearance.contains("self.persist()")
            && !appearance.contains("persist_snap"),
        "Appearance must not clone every thread just to write app.json: {appearance}"
    );
    let choose_theme = fn_src(&src, "choose_theme");
    assert!(
        choose_theme.contains("self.persist_cfg()") && !choose_theme.contains("self.persist()"),
        "choose_theme must write app.json without cloning every thread: {choose_theme}"
    );
    let set_close_to_tray = fn_src(&src, "set_close_to_tray");
    assert!(
        set_close_to_tray.contains("self.persist_cfg()")
            && !set_close_to_tray.contains("self.persist()"),
        "set_close_to_tray must write app.json without cloning every thread: {set_close_to_tray}"
    );
    let set_living_wall = fn_src(&src, "set_living_wall");
    assert!(
        set_living_wall.contains("self.persist_cfg()")
            && !set_living_wall.contains("self.persist()"),
        "set_living_wall must write app.json without cloning every thread: {set_living_wall}"
    );
    let behavior = src
        .split("SettingsSec::Behavior => {")
        .nth(1)
        .and_then(|s| s.split("SettingsSec::Update => {").next())
        .expect("Behavior");
    assert!(
            behavior.contains("self.set_close_to_tray(")
                && behavior.contains("self.set_living_wall(")
                && behavior.contains("self.persist_cfg()")
                && !behavior.contains("save = true")
                && !behavior.contains("self.persist()")
                && !behavior.contains("persist_snap"),
            "Close to tray, Living wall, and quiet hours must not clone every thread to write app.json: {behavior}"
        );
    assert!(
        behavior.contains("settings_dropdown")
            && behavior.contains("quiet_hours_menu")
            && behavior.contains("quiet_start_buf")
            && behavior.contains("quiet_end_buf")
            && !behavior.contains("cap_auto_buf")
            && !behavior.contains("cap_host_buf"),
        "quiet hours is one dropdown; a-day/host caps are gone: {behavior}"
    );
    let saved = src
        .split("fn save_settings(")
        .nth(1)
        .and_then(|s| s.split("fn ui_settings(").next())
        .expect("save_settings");
    assert!(
        saved.contains("normalize_hm") && saved.contains("cap_from_text"),
        "a typo in a clock or a cap must keep the old value, not switch the guard off: {saved}"
    );
    assert!(
        saved.contains("quiet_start_buf")
            && saved.contains("&self.cfg.quiet_start")
            && !saved.contains("default_quiet_start"),
        "Save must keep the last good clock, not the factory window: {saved}"
    );
    // Split so these assertions are not their own counter-examples.
    let gone = ["Host", "Voice", "Night", "Imagine", "Github"]
        .iter()
        .map(|s| format!("SettingsSec{}{s}", "::"))
        .chain(std::iter::once(format!("SettingsGroup{}Cabin", "::")))
        .chain(std::iter::once(format!("SettingsGroup{}Data", "::")))
        .find(|needle| src.contains(needle));
    assert_eq!(
        gone, None,
        "unreachable Settings sections are gone, not left painting into the void"
    );
    let plan = src
        .split("Slash::Plan =>")
        .nth(1)
        .and_then(|s| s.split("Slash::AlwaysApprove").next())
        .expect("Plan");
    assert!(
        plan.contains("plan_next") && !plan.contains("grok_session = None"),
        "bare /plan plans the next message and keeps the chat's session: {plan}"
    );
}

// Landed from PR #103.
#[test]
fn set_permission_mode_saves_auto_and_always() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("permission-mode");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
    let mut cabin = Cabin::quiet_for_test();
    cabin.set_permission_mode(PermissionMode::Auto);
    assert_eq!(cabin.permission_mode, PermissionMode::Auto);
    assert_eq!(cabin.cfg.permission_mode, "auto");
    assert!(!cabin.cfg.always_confirmed);
    cabin.set_permission_mode(PermissionMode::AlwaysApprove);
    assert_eq!(cabin.permission_mode, PermissionMode::AlwaysApprove);
    assert_eq!(cabin.cfg.permission_mode, "always-approve");
    assert!(cabin.cfg.always_confirmed);
    cabin.set_permission_mode(PermissionMode::Ask);
    assert_eq!(cabin.permission_mode, PermissionMode::Ask);
    assert_eq!(cabin.cfg.permission_mode, "ask");
    assert!(
        cabin.cfg.always_confirmed,
        "confirm flag stays so the next Always pick skips the sheet"
    );
    unsafe { std::env::remove_var("GROKHUB_CONFIG") };
}

fn perm_ask_for_test(id: u64, title: &str) -> grokhub_core::wire::PermissionAsk {
    grokhub_core::wire::PermissionAsk {
        rpc_id: serde_json::json!(id),
        session_id: "sess".into(),
        title: title.into(),
        tool_call_id: format!("tool-{id}"),
        action: title.into(),
        reason: String::new(),
        reject_option: Some("reject-once".into()),
    }
}

#[test]
fn a_second_ask_waits_its_turn_instead_of_cancelling_the_first() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("perm-queue");
    cabin.running = true;
    cabin.show_perm_ask(perm_ask_for_test(1, "Get-CimInstance"));
    cabin.perm_always_confirm = Some(serde_json::json!(1));
    cabin.show_perm_ask(perm_ask_for_test(2, "send_feedback"));
    assert_eq!(
        cabin.perm_ask.as_ref().map(|p| p.title.as_str()),
        Some("Get-CimInstance"),
        "a parallel ask must not replace (and cancel) the one on screen"
    );
    assert_eq!(
        cabin.perm_always_confirm,
        Some(serde_json::json!(1)),
        "a queued ask leaves the Always beat for the card on screen"
    );
    assert_eq!(cabin.perm_queue.len(), 1);
    cabin.next_perm_ask();
    assert_eq!(
        cabin.perm_ask.as_ref().map(|p| p.title.as_str()),
        Some("send_feedback")
    );
    assert!(cabin.perm_always_confirm.is_none());
    assert!(cabin.perm_queue.is_empty());
    cabin.show_perm_ask(perm_ask_for_test(3, "third"));
    cabin.withdraw_perm_asks();
    assert!(cabin.perm_ask.is_none() && cabin.perm_queue.is_empty());
    cabin.show_perm_ask(perm_ask_for_test(4, "four"));
    cabin.show_perm_ask(perm_ask_for_test(5, "five"));
    cabin.halt_in_flight();
    assert!(
        cabin.perm_ask.is_none() && cabin.perm_queue.is_empty(),
        "Stop withdraws the queued asks too"
    );

    let src = cabin_src();
    let ask = src
        .split("fn paint_perm_ask(")
        .nth(1)
        .and_then(|s| s.split("fn paint_elicit_ask(").next())
        .expect("paint_perm_ask");
    // The card and the inbox answer through one function.
    let answer = src
        .split("fn answer_perm_at(")
        .nth(1)
        .and_then(|s| s.split("fn paint_perm_ask(").next())
        .expect("answer_perm_at");
    assert!(ask.contains("self.answer_perm_at(0, allow,"), "{ask}");
    let ask = format!("{ask}{answer}");
    assert!(
        ask.contains("reject_permission(&p)")
            && !ask.contains("answer_permission(p.rpc_id.clone(), false)"),
        "Deny selects Grok's reject option; `cancelled` reads as \"User cancelled\": {ask}"
    );
    assert!(
        ask.contains("next_perm_ask()") && ask.contains("perm_queue.drain(..)"),
        "an answer brings up the next ask, and Always covers the queued ones: {ask}"
    );
    assert!(
        ask.contains("bare_press(ui, egui::Key::Escape)")
            && ask.contains("overlay_over_chat(")
            && !ask.contains("key_pressed(egui::Key::Escape)"),
        "only a bare Esc with nothing over the chat denies: {ask}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn only_a_bare_key_with_nothing_over_the_chat_answers_the_card() {
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    // egui marks a repeat itself: a second press with no release in between.
    let pressed = |ctx: &egui::Context, ev: egui::Event| {
        let mut hit = (false, false);
        let input = egui::RawInput {
            events: vec![ev],
            ..Default::default()
        };
        let _ = crate::theme::test_pass(ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                hit = (
                    super::bare_press(ui, egui::Key::Escape),
                    super::bare_press(ui, egui::Key::Enter),
                );
            });
        });
        hit
    };
    let esc = egui::Context::default();
    assert_eq!(
        pressed(&esc, key(egui::Key::Escape, egui::Modifiers::NONE)),
        (true, false)
    );
    assert_eq!(
        pressed(&esc, key(egui::Key::Escape, egui::Modifiers::NONE)),
        (false, false),
        "a held Esc's repeats must not deny the next card"
    );
    let enter = egui::Context::default();
    assert_eq!(
        pressed(&enter, key(egui::Key::Enter, egui::Modifiers::NONE)),
        (false, true)
    );
    assert_eq!(
        pressed(
            &egui::Context::default(),
            key(egui::Key::Escape, egui::Modifiers::SHIFT)
        ),
        (false, false),
        "a chord is another shortcut's key"
    );
    let renamed = egui::Context::default();
    let mut left = true;
    let input = egui::RawInput {
        events: vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        ..Default::default()
    };
    let _ = crate::theme::test_pass(&renamed, input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            super::drop_key(ui, egui::Key::Escape);
            left = super::bare_press(ui, egui::Key::Escape);
        });
    });
    assert!(
        !left,
        "Esc that cancelled a rename must not also deny the card"
    );
    let side = include_str!("sidebar.rs");
    assert_eq!(
        side.matches("drop_key(ui, egui::Key::Escape)").count(),
        2,
        "chat and project rename both keep their Esc: {side}"
    );

    let ctx = egui::Context::default();
    let frame = |area: Option<&str>| {
        let mut over = false;
        let _ = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            if let Some(id) = area {
                egui::Area::new(egui::Id::unique(id))
                    .order(egui::Order::Foreground)
                    .show(&ctx, |ui| ui.label("menu"));
            }
            over = super::overlay_over_chat(&ctx);
        });
        over
    };
    frame(Some("chat-jump"));
    assert!(!frame(Some("chat-jump")), "the jump pill is not an overlay");
    frame(Some("plus-menu"));
    assert!(frame(None), "Esc that closes a menu belongs to the menu");
    frame(None);
    assert!(!frame(None));
}

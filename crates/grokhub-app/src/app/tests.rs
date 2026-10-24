use super::pages::BoardAct;
use super::*;
use eframe::egui;
use grokhub_core::ChatRunPhase;
use grokhub_core::UpdateAction;
use grokhub_core::REVIEW_NIGHT_HOUR;
use grokhub_core::{ProjectKind, ProjectNode};
use grokhub_core::{UpdateCard, UpdateKind, UpdateStatus};
use std::sync::Arc;

#[path = "test_support_tests.rs"]
mod test_support_tests;
use test_support_tests::*;

#[path = "composer_attach_tests.rs"]
mod composer_attach_tests;

#[path = "background_poll_tests.rs"]
mod background_poll_tests;

#[path = "local_indexer_tests.rs"]
mod local_indexer_tests;

#[path = "pulse_page_tests.rs"]
mod pulse_page_tests;

#[path = "background_run_tests.rs"]
mod background_run_tests;

#[path = "self_work_tests.rs"]
mod self_work_tests;

#[path = "step_search_tests.rs"]
mod step_search_tests;

#[path = "board_ideas_tests.rs"]
mod board_ideas_tests;

#[path = "idle_chat_state_tests.rs"]
mod idle_chat_state_tests;

#[path = "secrets_skills_memory_tests.rs"]
mod secrets_skills_memory_tests;

#[path = "settings_lanes_tests.rs"]
mod settings_lanes_tests;

#[path = "feed_home_tests.rs"]
mod feed_home_tests;

#[path = "voice_clock_tests.rs"]
mod voice_clock_tests;

#[path = "recall_memory_tests.rs"]
mod recall_memory_tests;

#[path = "privacy_sync_tests.rs"]
mod privacy_sync_tests;

#[path = "automations_page_tests.rs"]
mod automations_page_tests;

#[path = "projects_folders_tests.rs"]
mod projects_folders_tests;

#[path = "imagine_tests.rs"]
mod imagine_tests;

#[path = "chat_bubble_tests.rs"]
mod chat_bubble_tests;

#[path = "profile_devices_tests.rs"]
mod profile_devices_tests;

fn cabin_src() -> String {
    // Windows CI checkouts often embed CRLF via include_str; source-inspection
    // asserts match on \n, so normalize once here.
    concat!(
        include_str!("mod.rs"),
        include_str!("persist.rs"),
        include_str!("persist_service.rs"),
        include_str!("acp.rs"),
        include_str!("chat_kick.rs"),
        include_str!("palette.rs"),
        include_str!("settings.rs"),
        include_str!("plus.rs"),
        include_str!("projects.rs"),
        include_str!("slash.rs"),
        include_str!("oauth.rs"),
        include_str!("imagine.rs"),
        include_str!("night.rs"),
        include_str!("chat_ui.rs"),
        include_str!("sidebar.rs"),
        include_str!("pages.rs"),
        include_str!("connectors_ui.rs"),
        include_str!("marketplace_ui.rs"),
        include_str!("jobs.rs"),
        include_str!("chips.rs"),
        include_str!("voice.rs"),
        include_str!("threads_nav.rs"),
        include_str!("ideas_ui.rs"),
        include_str!("board_ui.rs"),
        include_str!("background.rs"),
    )
    .replace("\r\n", "\n")
    .replace("pub(super) ", "")
}

/// One `fn name(` body in the concatenated cabin, stopping at the next same-indent fn.
fn fn_src<'a>(src: &'a str, name: &str) -> &'a str {
    let needle = format!("fn {name}(");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("missing fn {name}"));
    let after = &src[start..];
    let rest = &after[needle.len()..];
    let end = rest
        .find("\n    fn ")
        .or_else(|| rest.find("\nfn "))
        .unwrap_or(rest.len());
    &after[..needle.len() + end]
}

#[test]
fn cabin_src_strips_crlf_so_windows_checkout_matches_lf_asserts() {
    // Simulate a Windows autocrlf checkout: the Halt-guard snippet must still
    // match the LF needle used by heartbeat_halt_skips_every_organ_that_starts_work.
    let crlf = "if !halted {\r\n                        self.follow_feed_lookup();";
    let norm = crlf.replace("\r\n", "\n");
    assert!(
        norm.contains("if !halted {\n                        self.follow_feed_lookup();"),
        "CRLF must normalize before the Halt-digest assert"
    );
    assert!(
        !cabin_src().contains('\r'),
        "cabin_src must not leave CR for source-inspection tests"
    );
}

#[test]
fn rename_focus_selects_the_placeholder() {
    egui::__run_test_ui(|ui| {
        let mut buf = String::from("Project");
        let edit = ui.add(egui::TextEdit::singleline(&mut buf));
        select_all_edit(ui, edit.id, &buf);
        let state = egui::TextEdit::load_state(ui.ctx(), edit.id).expect("edit state");
        let range = state.cursor.char_range().expect("selection");
        let [a, b] = range.sorted_cursors();
        assert_eq!(a.index, egui::text::CharIndex(0));
        assert_eq!(b.index, egui::text::CharIndex(7));
    });
}

#[test]
fn health_opens_the_about_page() {
    assert_eq!(super::health_settings_sec(), super::SettingsSec::About);
}

#[test]
fn about_paints_the_version() {
    let src = cabin_src();
    let impl_src = src.as_str();
    let about = impl_src
        .split("SettingsSec::About => {")
        .nth(1)
        .expect("about body");
    let about = about
        .split("if let Some(s) = next_sec")
        .next()
        .unwrap_or(about);
    assert!(
        about.contains("CARGO_PKG_VERSION"),
        "About must show grokhub --version: {about}"
    );
    assert!(
        about.contains("FONT_HEADING"),
        "version is a heading, not a muted note: {about}"
    );
    assert!(
        !about.contains("usage_line") && !about.contains("catalog_line"),
        "About must not paint today-stats or the model catalog: {about}"
    );
}

#[test]
fn a_nav_action_opens_the_page_it_names() {
    let src = cabin_src();
    let run = src
        .split("fn run_palette(")
        .nth(1)
        .and_then(|s| s.split("fn open_palette(").next())
        .unwrap_or(&src);
    assert!(
        run.contains("\"nav:command\" => self.nav = Nav::Command"),
        "Command is a real page — a chip that names it must not land on Chat: {run}"
    );
    assert!(
        super::Cabin::nav_from_id("command") == super::Nav::Command,
        "the chip id and the page have to agree"
    );
    assert!(
        super::Cabin::nav_from_id("ideas") == super::Nav::Pulse,
        "an ideas chip must open Ideas, not Chat"
    );
    assert!(
        grokhub_core::nav_from_chip_value("__nav:ideas") == Some("ideas"),
        "command chips name Ideas as __nav:ideas"
    );
    assert!(
        super::Cabin::nav_from_id("eyes") == super::Nav::Chat,
        "Desk is gone — its id lands on chat"
    );
    assert!(super::Cabin::nav_from_id("nonsense") == super::Nav::Chat);
}

#[test]
fn palette_search_walks_nested_files_off_the_ui_thread() {
    let src = cabin_src();
    let tick = src
        .split("fn tick_palette_search(")
        .nth(1)
        .and_then(|s| s.split("fn kick_palette_search(").next())
        .expect("tick_palette_search");
    assert!(
        !tick.contains("search_place") && tick.contains("palette_search_is_saved"),
        "a saved empty palette result must not walk again, and not on the UI thread: {tick}"
    );
    assert!(
        tick.contains("palette_forget_stale_walk") && tick.contains("palette_search_is_saved"),
        "a query change must forget the last finished key or a revert keeps the empty list: {tick}"
    );
    let open = src
        .split("fn open_palette(")
        .nth(1)
        .and_then(|s| s.split("fn run_palette(").next())
        .expect("open_palette");
    assert!(
        open.contains("palette_file_rx = None"),
        "reopen must drop a leftover walk or it can block the next search: {open}"
    );
    let kick = src
        .split("fn kick_palette_search(")
        .nth(1)
        .and_then(|s| s.split("fn poll_palette_search(").next())
        .expect("kick_palette_search");
    assert!(
        kick.contains("thread::spawn") && kick.contains("search_place"),
        "palette search walks the current place off the UI thread: {kick}"
    );
    assert!(
        src.contains("RailIcon::Search") && src.contains("self.open_palette()"),
        "Search stays the palette — no second search page"
    );
}

#[test]
fn palette_query_revert_forgets_empty_saved_hits() {
    let mut files = vec!["nested/deep/buried.txt".to_string()];
    let mut files_q = "buried".to_string();
    let mut files_root = "/place".to_string();
    grokhub_core::palette_forget_stale_walk(
        &mut files,
        &mut files_q,
        &mut files_root,
        "buriexx",
        "/place",
    );
    assert!(files.is_empty(), "a query change clears the last hits");
    assert!(
        files_q.is_empty() && files_root.is_empty(),
        "the finished key must be forgotten or a revert matches the empty list"
    );
    assert!(
        !grokhub_core::palette_search_is_saved(&files_q, &files_root, "buried", "/place"),
        "reverting to the earlier query must walk again so the file hits come back"
    );
    grokhub_core::palette_forget_stale_walk(
        &mut files,
        &mut files_q,
        &mut files_root,
        "buried",
        "/place",
    );
    assert!(
        files_q.is_empty() && files.is_empty(),
        "revert still has no finished key, so tick kicks a real walk: {files_q:?} {files:?}"
    );
}

#[test]
fn picking_a_palette_file_opens_it() {
    let action =
        grokhub_core::palette_row_action(&[], &["nested/deep/buried.txt".to_string()], "/place", 0)
            .expect("file row");
    let shown = grokhub_core::palette_file_shown(&action).expect("picked path");
    assert!(
        shown.ends_with("nested/deep/buried.txt") && !shown.contains(".."),
        "the pick must be the nested file, not a status-only label: {shown}"
    );
    assert_eq!(grokhub_core::palette_file_shown("file:"), None);
    assert_eq!(grokhub_core::palette_file_shown("nav:chat"), None);
    let src = cabin_src();
    let run = src
        .split("fn run_palette(")
        .nth(1)
        .and_then(|s| s.split("fn run_slash_line(").next())
        .expect("run_palette");
    assert!(
        run.contains("palette_file_shown") && run.contains("desktop::open_path"),
        "picking a palette file must open it, not only write status: {run}"
    );
}

#[test]
fn settings_drops_cabin_tabs() {
    let src = cabin_src();
    let settings = src
        .split("fn ui_settings(")
        .nth(1)
        .and_then(|s| s.split("fn add_automation_seed(").next())
        .expect("ui_settings");
    assert!(
        !settings.contains("section_label(ui, \"Cabin\")"),
        "Cabin group is gone from Settings: {settings}"
    );
    assert!(
        !settings.contains("Cabin eyes"),
        "Cabin eyes toggle is gone: {settings}"
    );
    // The Host tab itself is gone; `always_permission_keeps_the_acp_session` proves
    // the variant no longer exists anywhere in the file.
    assert!(
        settings.contains("(SettingsSec::Behavior, \"Behavior\")"),
        "the tabs that remain are the ones with a home: {settings}"
    );
    assert!(
        !settings.contains("section_label(ui, \"Data\")")
            && !settings.contains("settings_nav(ui, \"GitHub\""),
        "GitHub is connector-managed — Settings must not keep a Data/GitHub tab: {settings}"
    );
    let account = settings
        .split("SettingsSec::Account => {")
        .nth(1)
        .and_then(|s| s.split("SettingsSec::Appearance => {").next())
        .expect("Account");
    assert!(
        !account.contains("Install Grok Build CLI")
            && !account.contains("Console key")
            && !account.contains("Device name")
            && !account.contains("Imagine override"),
        "Account is OAuth connect/sign-out only: {account}"
    );
    assert!(
        !settings.contains("Automations a day")
            && !settings.contains("Host commands")
            && !settings.contains("Quiet hours start"),
        "Behavior dropped a-day/host caps and split quiet clocks: {settings}"
    );
    assert!(
        settings.contains("settings_dropdown") && settings.contains("Quiet hours"),
        "Quiet hours is one dropdown: {settings}"
    );
    let update = settings
        .split("SettingsSec::Update => {")
        .nth(1)
        .and_then(|s| s.split("SettingsSec::About => {").next())
        .expect("Update");
    assert!(
        !update.contains("settings_update_note()")
            && !update.contains("Source clone")
            && !update.contains("Install overlay")
            && !update.contains("Update Grok Build CLI")
            && !update.contains("show_cli_update")
            && update.contains("update_label")
            && update.contains("update_hint")
            && !update.contains("Install Grok Build CLI")
            && !update.contains("show_cli_install")
            && update.contains("cabin_notice")
            && !update.contains("cli_notice")
            && update.contains("settings_action")
            && !update.contains("if let Some(label) = update_label"),
        "Update is one always-visible control for the cabin, never the CLI: {update}"
    );
    assert!(
        settings.contains("cabin_update_notice")
            && !settings.contains("cli_update_notice")
            && !settings.contains("should_update_cli_alpha")
            && settings.contains("queue_cabin_update")
            && settings.contains("settings_update_label")
            && settings.contains("settings_update_hint"),
        "Settings Update stays visible; titlebar chip still hides when current: {settings}"
    );
    assert!(
        !account.contains("Update CLI")
            && !account.contains("cabin_update_notice")
            && !account.contains("cli_update_notice")
            && !account.contains("GitHub Latest")
            && !account.contains("Update Grok Build CLI")
            && !account.contains("update_chip_label"),
        "cabin notify must not live on Account: {account}"
    );
}

#[test]
fn about_section_opens_update() {
    assert_eq!(
        super::settings_group_home(super::SettingsGroup::About),
        super::SettingsSec::Update
    );
}

#[test]
fn overlay_update_skips_chat() {
    let v = grokhub_core::overlay_update_begin(2);
    assert!(v.stay_on_update);
    assert!(!v.posts_chat);
    let done = grokhub_core::overlay_update_finish(true, 50);
    assert!(!done.posts_chat);
    assert!(done.stay_on_update);
    assert!(done.can_restart);
    assert!(grokhub_core::overlay_update_can_restart(true, false));
    assert!(!grokhub_core::overlay_update_can_restart(true, true));
}

#[test]
fn general_section_opens_account() {
    assert_eq!(
        super::settings_group_home(super::SettingsGroup::General),
        super::SettingsSec::Account
    );
}

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
        .find("ghost_pill(ui, \"Always\")")
        .expect("Always ghost");
    let set_always = ask
        .find("set_permission_mode(PermissionMode::AlwaysApprove)")
        .expect("Always mode");
    assert!(
        always_click < set_always && ask[always_click..set_always].contains("perm_always_confirm"),
        "Always on the Ask card must confirm before flipping the pill: {ask}"
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
    let always = src
        .split("Slash::AlwaysApprove =>")
        .nth(1)
        .and_then(|s| s.split("Slash::AutoPerm =>").next())
        .expect("AlwaysApprove");
    assert!(
        always.contains("grok_session = None"),
        "/always must session/new or Ask vs Always does not take: {always}"
    );
    assert!(
        always.contains("persist_idle_key") && !always.contains("self.persist()"),
        "/always must not clone every thread — bump the idle key so persist_bg skips: {always}"
    );
    let auto = src
        .split("Slash::AutoPerm =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Effort =>").next())
        .expect("AutoPerm");
    assert!(
        auto.contains("grok_session = None"),
        "/auto must session/new or permission mode does not take: {auto}"
    );
    assert!(
        auto.contains("persist_idle_key") && !auto.contains("self.persist()"),
        "/auto must not clone every thread — bump the idle key so persist_bg skips: {auto}"
    );
    let mode = src
        .split("Slash::Mode(mode)")
        .nth(1)
        .and_then(|s| s.split("Slash::Dream").next())
        .expect("Mode");
    assert!(
        mode.contains("self.persist_cfg()")
            && !mode.contains("self.persist()")
            && !mode.contains("persist_snap"),
        "/mode must not clone every thread just to write app.json: {mode}"
    );
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
        .and_then(|s| s.split("Slash::AlwaysApprove =>").next())
        .expect("Plan");
    assert!(
        plan.contains("grok_session = None"),
        "/plan must session/new or Chat vs Plan does not take: {plan}"
    );
    assert!(
        plan.contains("persist_idle_key") && !plan.contains("self.persist()"),
        "/plan must not clone every thread — bump the idle key so persist_bg skips: {plan}"
    );
    assert!(
        plan.contains("halt_in_flight"),
        "/plan mid-turn must halt or Thinking sticks after the agent is dropped: {plan}"
    );
    // rustfmt may break `let row = crate::cards::session_row(...)` across lines.
    let row = src
        .split("crate::cards::session_row(")
        .nth(1)
        .and_then(|s| s.split("ui.allocate_ui_with_layout").next())
        .expect("session_row");
    assert_eq!(
        row.matches("grok_session = None").count(),
        2,
        "session/permission row must session/new so mode takes: {row}"
    );
    assert_eq!(
            row.matches("persist_idle_key").count(),
            2,
            "session/permission row must not clone every thread — bump the idle key so persist_bg skips: {row}"
        );
    assert!(
        row.contains("select_plan_without_rename") && !row.contains("t.title ="),
        "clicking Plan must switch mode without writing the chat title: {row}"
    );
    let plan_pick = src
        .split("fn select_plan_without_rename(")
        .nth(1)
        .and_then(|s| s.split("fn hold_chat_name_for_plan(").next())
        .expect("select_plan_without_rename");
    assert!(
            plan_pick.contains("hold_chat_name_for_plan")
                && plan_pick.contains("grok_session = None")
                && plan_pick.contains("set_session_mode(SessionMode::Plan)")
                && !plan_pick.contains("t.title ="),
            "selecting Plan keeps the session-id clear and does not write the thread title: {plan_pick}"
        );
    let hold = src
        .split("fn hold_chat_name_for_plan(")
        .nth(1)
        .and_then(|s| s.split("fn thread_rail_title(").next())
        .expect("hold_chat_name_for_plan");
    assert!(
        hold.contains("title_after_selecting_plan")
            && hold.contains("history_label_after_plan")
            && !hold.contains("t.title ="),
        "Plan must keep the thread title and the History label already on screen: {hold}"
    );
    assert!(
        plan.contains("hold_chat_name_for_plan"),
        "/plan must keep the chat name when it clears the session id: {plan}"
    );
}

#[test]
fn selecting_plan_does_not_change_the_thread_title() {
    assert_eq!(
        grokhub_core::cli_history::title_after_selecting_plan("Night watch"),
        "Night watch"
    );
    assert_eq!(
        grokhub_core::cli_history::preferred_history_title(
            "Night watch",
            false,
            Some("Plan"),
            Some("abc")
        ),
        "Night watch"
    );
    assert_eq!(
        grokhub_core::cli_history::history_label_after_plan("Night watch", "fix the dock"),
        "Night watch"
    );
}

#[test]
fn slash_pick_resets_when_the_list_changes() {
    assert_eq!(super::slash_pick_retain(2, true, 4), 0);
    assert_eq!(super::slash_pick_retain(2, false, 4), 2);
    assert_eq!(super::slash_pick_retain(9, false, 3), 2);
    assert_eq!(super::slash_pick_retain(1, true, 0), 0);
}

#[test]
#[allow(clippy::assertions_on_constants)] // pins design constants
fn idle_visible_cabin_does_not_spin() {
    assert!(!super::wants_live_repaint(
        false, false, false, true, false, false
    ));
    assert!(!super::wants_live_repaint(
        false, false, false, false, false, false
    ));
    assert!(super::wants_live_repaint(
        true, false, false, true, false, false
    ));
    assert!(super::wants_live_repaint(
        false, false, false, false, false, true
    ));
    assert!(super::HIDDEN_HEARTBEAT_MS > 80);
    assert_eq!(
        grokhub_core::heartbeat_repaint_ms(
            false,
            false,
            grokhub_core::HEARTBEAT_MS,
            super::HIDDEN_HEARTBEAT_MS
        ),
        grokhub_core::HEARTBEAT_MS
    );
    assert_eq!(
        grokhub_core::heartbeat_repaint_ms(
            false,
            true,
            grokhub_core::HEARTBEAT_MS,
            super::HIDDEN_HEARTBEAT_MS
        ),
        super::HIDDEN_HEARTBEAT_MS,
        "hidden idle must wake for tray Quit, not the 15s pulse"
    );
    assert_eq!(
        grokhub_core::heartbeat_repaint_ms(
            true,
            true,
            grokhub_core::HEARTBEAT_MS,
            super::HIDDEN_HEARTBEAT_MS
        ),
        80
    );
    let src = cabin_src();
    let live = src
        .split("let live = wants_live_repaint(")
        .nth(1)
        .and_then(|s| s.split("ctx.request_repaint_after").next())
        .expect("wants_live_repaint call");
    assert!(
            live.contains("persist_rx")
                && live.contains("history_rx")
                && live.contains("mem_restore_rx")
                && live.contains("mem_file_rx")
                && live.contains("recall_rx")
                && live.contains("sync_rx")
                && live.contains("inhabit_rx")
                && live.contains("reflect_rx")
                && live.contains("session_show_rx")
                && live.contains("import_rx")
                && live.contains("recipe_desk_rx")
                && live.contains("host_diff_rx")
                && live.contains("pick_rx")
                && live.contains("pick_list_rx")
                && live.contains("oauth_start_rx")
                && live.contains("oauth_poll_rx")
                && live.contains("greeting_busy")
                && live.contains("greeting_files_rx")
                && live.contains("night_check_rx")
                && live.contains("eyes_cap_rx")
                && live.contains("grok_p_rx"),
            "History listing / inspect / greeting / night check / Eyes capture / plus-upload / Settings doctor must not wait on the 15s heartbeat: {live}"
        );
}

#[test]
fn show_cabin_keeps_the_tray_icon() {
    let src = cabin_src();
    let show = fn_src(&src, "show_from_tray");
    assert!(
        !show.contains("drop_off_thread"),
        "Show cabin must not tear down the tray icon: {show}"
    );
    assert!(
        show.contains("ensure_tray_spawn"),
        "Show cabin should keep a live tray: {show}"
    );
    assert!(
        src.contains("StartDrag") && src.contains("titlebar_should_start_drag"),
        "undecorated cabin must drag from the titlebar body"
    );
    assert!(
        src.contains("force_x11_for_close_to_tray")
            || include_str!("../main.rs").contains("force_x11_for_close_to_tray"),
        "winit 0.30 must drop WAYLAND_DISPLAY so × can unmap"
    );
    assert!(
        src.contains("hidden_window_tick"),
        "a pinned taskbar click must raise the hidden cabin instead of re-unmapping it"
    );
    let hide = src
        .split("fn unmap_to_tray")
        .nth(1)
        .and_then(|s| s.split("fn ensure_tray_spawn").next())
        .expect("unmap_to_tray");
    assert!(
            hide.contains("tray_saw_unfocused = false"),
            "× must clear the focus-raise latch so the next focused frame does not map the cabin: {hide}"
        );
    assert!(
        hide.contains("persist_if_dirty") && !hide.contains("self.persist()"),
        "hide to tray must not clone every thread when idle persist already wrote: {hide}"
    );
    assert!(
        hide.contains("hide_cabin"),
        "Windows × must cloak and leave the taskbar, not minimize: {hide}"
    );
    let tick = src
        .split("hidden_window_tick(")
        .nth(1)
        .and_then(|s| s.split("match").next())
        .expect("hidden_window_tick call");
    assert!(
        tick.contains("tray_saw_unfocused"),
        "taskbar raise waits until the hidden cabin actually lost focus: {tick}"
    );
    assert!(
        src.contains("hidden_raise_ready") && src.contains("reapply_unmap"),
        "× must not flash back from a FocusLost/FocusGained bounce or Visible(false) spam"
    );
    let stay = src
        .split("HiddenTick::StayHidden =>")
        .nth(1)
        .and_then(|s| s.split("ctx.request_repaint_after").next())
        .expect("StayHidden");
    assert!(
        stay.contains("hide_cabin"),
        "leftover focus after × must re-unmap the Windows taskbar stub: {stay}"
    );
    let finish = fn_src(&src, "finish_acp_turn");
    let apply = fn_src(&src, "apply_single_turn");
    assert!(
        !finish.contains("save_schedule") && !apply.contains("save_schedule"),
        "ordinary replies that mention every day at / heartbeat every must not become live jobs"
    );
    let saver = format!(
        "{}{}",
        fn_src(&src, "save_schedule"),
        fn_src(&src, "commit_schedule")
    );
    assert!(
        saver.contains("route_schedule") && saver.contains("ScheduleRoute::Clock"),
        "`every day at 9` must keep its hour instead of becoming a 1d loop: {saver}"
    );
    assert!(
            saver.contains("persist_loops")
                && saver.contains("persist_automations")
                && !saver.contains("self.persist()"),
            "a saved job must not clone every thread 2s later — the persist helpers bump the idle key: {saver}"
        );
    assert!(
        src.contains("ignore_close_request") && src.contains("self.want_quit"),
        "sticky close_requested must not hide after a taskbar raise, and tray Quit must still exit"
    );
    assert!(
        include_str!("../main.rs").contains("try_claim_cabin"),
        "a second grokhub from the taskbar must raise the running cabin and exit"
    );
    assert!(
        cabin_src().contains("honor_cabin_raise(self.want_quit)"),
        "Restart must not CancelClose when a sibling spawn writes cabin.raise"
    );
    assert!(
        show.contains("CancelClose"),
        "Show cabin must clear a sticky close so the window does not hide again"
    );
    let restart = src
        .split("fn restart_after_update")
        .nth(1)
        .and_then(|s| s.split("fn start_overlay_update").next())
        .expect("restart_after_update");
    let spawn_at = restart.find("restart_system").expect("restart_system");
    let drop_at = restart.find("drop_tray");
    assert!(
        drop_at.is_some_and(|d| d > spawn_at),
        "dropping the tray before spawn leaves a headless cabin when restart fails: {restart}"
    );
}

#[test]
fn ui_date_spawns_must_time_out() {
    let src = cabin_src();
    let date = src
        .split("fn date_out(")
        .nth(1)
        .and_then(|s| s.split("\n    fn local_clock()").next())
        .expect("date_out");
    assert!(
        date.contains("run_limited("),
        "date_out must kill a hung date: {date}"
    );
    let clock = src
        .split("fn local_clock()")
        .nth(1)
        .and_then(|s| s.split("\n    fn local_day()").next())
        .expect("local_clock");
    assert!(
        clock.contains("date_out(") && !clock.contains(".output()"),
        "local_clock must use the timed date helper: {clock}"
    );
    assert!(
        clock.contains("CLOCK_TTL"),
        "chips and greeting must not spawn date on every paint: {clock}"
    );
    assert!(
        clock.contains("thread::spawn") && clock.contains("inflight"),
        "stale date must refresh off the UI thread: {clock}"
    );
    let day = src
        .split("fn local_day()")
        .nth(1)
        .and_then(|s| s.split("\n    fn tick_heartbeat").next())
        .expect("local_day");
    assert!(
        day.contains("date_out(") && !day.contains(".output()"),
        "local_day must use the timed date helper: {day}"
    );
    assert!(
        day.contains("thread::spawn") && day.contains("inflight"),
        "stale local_day must refresh off the UI thread: {day}"
    );
    let roll = fn_src(&src, "roll_today");
    assert!(
        roll.contains("local_day(") && !roll.contains(".output()"),
        "roll_today must reuse the cached day, not spawn date on the UI thread: {roll}"
    );
    assert!(
        roll.contains("persist_usage")
            && roll.contains("persist_idle_key")
            && !roll.contains("self.persist()"),
        "day rollover must not clone every thread just to write usage.json: {roll}"
    );
}

#[test]
fn persist_does_not_hold_hub_lock_across_disk() {
    let src = cabin_src();
    let persist = format!(
        "{}{}",
        fn_src(&src, "persist"),
        fn_src(&src, "persist_snap")
    );
    assert!(
        persist.contains("self.hub.clone()") && !persist.contains("state_for_disk"),
        "persist must not clone hub snapshot/last_frame on the UI thread: {persist}"
    );
    assert!(
        !persist.contains("if let Ok(st) = self.hub.lock()"),
        "persist must not hold hub.lock() across save_hub_state: {persist}"
    );
    let write = fn_src(&src, "write_persist_disk");
    let lock = write.find("hub.lock").expect("worker hub lock");
    let save = write.find("save_hub_state").expect("save_hub_state");
    assert!(
        lock < save && write.contains("state_for_disk(&st)"),
        "persist worker must clone hub state then drop the lock before hub-state.json: {write}"
    );
}

#[test]
fn greeting_and_chips_use_the_native_fast_model() {
    let src = cabin_src();
    let native = include_str!("native_unattended.rs").replace("\r\n", "\n");
    let greet = fn_src(&src, "spawn_greeting_llm");
    assert!(greet.contains("spawn_native_greeting"), "{greet}");
    let chips = fn_src(&src, "spawn_chip_llm");
    assert!(chips.contains("spawn_native_chips"), "{chips}");
    let fast = fn_src(&native, "fast_text");
    assert!(
        fast.contains("CABIN_FAST_MODEL") && fast.contains("CABIN_FAST_FALLBACK"),
        "chips/greeting use the fast model and fall back if it is empty: {fast}"
    );
    for body in [greet, chips, fast] {
        assert!(
            !body.contains("find_grok") && !body.contains("grok_cli_key"),
            "{body}"
        );
    }
    let ready = fn_src(&src, "llm_ready");
    assert!(
        !ready.contains("find_grok"),
        "llm_ready never counts the Grok Build CLI: {ready}"
    );
    let chip = src
        .split("fn apply_chip(")
        .nth(1)
        .and_then(|s| s.split("fn nav_from_id").next())
        .expect("apply_chip");
    let chip_spawn = chip
        .find("thread::spawn")
        .expect("chip save must leave the UI thread");
    let chip_save = chip.find("save_chips").expect("save_chips");
    assert!(
        chip_spawn < chip_save && chip.contains("persist_io"),
        "chip click must not freeze the cabin writing chips.json: {chip}"
    );
    assert!(
        chip.contains("send_chat(chip.value)"),
        "/learn chip click must use the typed send path, not a silent parse miss: {chip}"
    );
}

#[test]
fn grok_login_powers_history_and_imagine() {
    let src = cabin_src();
    assert!(
        !src.contains("fn ensure_acp("),
        "the engine starts through ensure_native_engine only"
    );
    let bearer = fn_src(&src, "bearer");
    assert!(
        !bearer.contains("grok_cli_key") && !bearer.contains("refresh_grok_login"),
        "the Imagine/chips bearer never reads or refreshes the Grok CLI login: {bearer}"
    );
    assert!(
        bearer.contains("refresh_cabin_oauth") && !bearer.contains("ensure_access"),
        "cabin OAuth refresh HTTP must leave the UI thread: {bearer}"
    );
    assert!(
        bearer.contains("console_key()"),
        "Imagine/ACP console-key fallback must read secrets.json: {bearer}"
    );
    let disk = fn_src(&src, "write_persist_disk");
    assert!(
        disk.contains("secrets::save"),
        "persist must write the console key to secrets.json: {disk}"
    );
    assert!(
        disk.contains("if let Some(s) = &snap.secrets") || disk.contains("snap.secrets"),
        "idle persist must not write secrets.json from a stale snap: {disk}"
    );
    assert!(
        src.contains("migrate_console_key"),
        "boot must move a leftover app.json console key into secrets.json"
    );
    assert!(
        src.contains("secrets::ensure_private"),
        "boot must rewrite a world-readable leftover secrets.json"
    );
    assert!(
        src.contains("secrets::console_key") && src.contains("migrate_console_key"),
        "Console key lives in secrets.json; Settings must not keep a leftover app.json field"
    );
    assert!(
        !fn_src(&src, "ui_settings").contains("Console key"),
        "Settings must not paint a Console key editor"
    );
    let settings_save = fn_src(&src, "save_settings");
    assert!(
        settings_save.contains("api_key.clear"),
        "Settings Save must not keep a leftover console key on cfg: {settings_save}"
    );
    assert!(
        settings_save.contains("self.persist()") && !settings_save.contains("secrets::save"),
        "Settings Save must not freeze the cabin writing secrets.json: {settings_save}"
    );
    assert!(
            settings_save.contains("self.persist_cfg()")
                && settings_save.contains("self.flush_projects()")
                && settings_save.contains("self.persist_hub()")
                && settings_save.contains("self.persist_secrets()")
                && settings_save.contains("tree_changed"),
            "Settings Save must not clone every thread when the worktree did not change: {settings_save}"
        );
    let persist_if = src
        .split("fn persist_if_dirty")
        .nth(1)
        .and_then(|s| s.split("fn persist_secrets").next())
        .expect("persist_if_dirty");
    assert!(
        persist_if.contains("persist_idle_key")
            && persist_if.contains("persist_cfg")
            && persist_if.contains("self.persist()"),
        "hide/quit must skip the thread clone when idle persist already wrote: {persist_if}"
    );
    let persist_secrets = src
        .split("fn persist_secrets(")
        .nth(1)
        .and_then(|s| s.split("fn persist_usage").next())
        .expect("persist_secrets");
    let secrets_spawn = persist_secrets
        .find("thread::spawn")
        .expect("secrets write must leave the UI thread");
    let secrets_save = persist_secrets
        .find("secrets::save")
        .expect("secrets::save");
    assert!(
        secrets_spawn < secrets_save
            && persist_secrets.contains("persist_io")
            && !persist_secrets.contains("self.persist()"),
        "Settings Save must not freeze the cabin writing secrets.json: {persist_secrets}"
    );
    let persist_usage = fn_src(&src, "persist_usage");
    let usage_spawn = persist_usage
        .find("thread::spawn")
        .expect("usage write must leave the UI thread");
    let usage_save = persist_usage.find("save_usage").expect("save_usage");
    assert!(
        usage_spawn < usage_save
            && persist_usage.contains("persist_io")
            && !persist_usage.contains("self.persist()"),
        "night usage must not freeze the cabin writing usage.json: {persist_usage}"
    );
    for name in [
        "persist_loops",
        "persist_suggestions",
        "persist_automations",
        "persist_updates",
    ] {
        let body = fn_src(&src, name);
        assert!(
            body.contains("persist_io") && body.contains("thread::spawn"),
            "{name} must take persist_io off the UI thread so FullSnap cannot tear JSON: {body}"
        );
    }
    let poll_err = fn_src(&src, "poll_persist_err");
    assert!(
        poll_err.contains("persist_idle_key.clear"),
        "a failed persist must clear the idle key so the next tick retries: {poll_err}"
    );
    let persist_svc_src = include_str!("persist_service.rs");
    assert!(
        persist_svc_src.contains("note_persist_err")
            && persist_svc_src.contains("persist lock poisoned"),
        "FullSnap must surface a poisoned persist_io instead of claiming disk is clean: {persist_svc_src}"
    );
    assert!(
        !include_str!("change_undo.rs").contains("salt: &str"),
        "egui id namespace param must not be named salt (CodeQL hard-coded salt FP)"
    );
    let bg = src
        .split("fn persist_idle_now(")
        .nth(1)
        .and_then(|s| s.split("\n    fn poll_persist").next())
        .expect("persist_bg");
    assert!(
        !bg.contains("secrets.api_key.len"),
        "idle persist must not race a just-saved console key: {bg}"
    );
    assert!(
        bg.contains("grok_session") && bg.contains("grok_cwd"),
        "idle persist must notice a handshake session stamp: {bg}"
    );
    let snap = src
        .split("fn persist_snap(")
        .nth(1)
        .and_then(|s| s.split("fn persist_bg(").next())
        .expect("persist_snap");
    assert!(
        snap.contains("secrets: None"),
        "idle persist_snap must omit secrets.json: {snap}"
    );
    assert!(
            !snap.contains("msgs.clone()"),
            "persist_snap must copy the live pane into the thread once, not again into PersistSnap.msgs: {snap}"
        );
    assert!(
        snap.contains("t.messages = self.messages.clone()")
            && snap.contains("self.threads.clone()"),
        "persist must share the live pane Arc, then bump other threads: {snap}"
    );
    assert!(
        !snap.contains("parked_last") && !snap.contains("live_last"),
        "persist_snap must not recopy bodies when the live pane already is the parked Arc: {snap}"
    );
    assert!(
        snap.contains("self.hub.clone()") && !snap.contains("state_for_disk"),
        "persist_snap must not clone hub last_frame/snapshot on the UI thread: {snap}"
    );
    assert!(
        disk.contains("current_thread") && disk.contains("save_chat"),
        "persist must write chat.json from the snapped thread, not a second 8MB clone: {disk}"
    );
    let persist = src
        .split("fn persist(&mut self)")
        .nth(1)
        .and_then(|s| s.split("fn persist_snap(").next())
        .expect("persist");
    assert!(
        persist.contains("snap.secrets = Some") && persist.contains("submit_full"),
        "foreground persist must hand secrets to PersistService: {persist}"
    );
    assert!(
        persist.contains("persist_svc") && !persist.contains("thread::spawn"),
        "foreground persist must leave the UI thread via PersistService: {persist}"
    );
    let svc = fn_src(&src, "persist_service_worker");
    assert!(
        svc.contains("write_persist_disk_in_order") && svc.contains("io.lock()"),
        "PersistService worker must take persist_io before threads.json: {svc}"
    );
    assert!(
            persist.contains("persist_idle_key") && persist.contains("persist_idle_now"),
            "persist must bump the idle key or persist_bg clones every thread again 2s later: {persist}"
        );
    assert!(
        bearer.contains("persist_io") && bearer.contains("secrets::save"),
        "OAuth refresh must take persist_io before writing secrets.json: {bearer}"
    );
    let bearer_spawn = bearer
        .find("thread::spawn")
        .expect("oauth persist must leave the UI thread");
    let bearer_save = bearer.find("secrets::save").expect("oauth persist writes");
    assert!(
        bearer_spawn < bearer_save,
        "OAuth refresh must not freeze the cabin writing secrets.json: {bearer}"
    );
    let kick = fn_src(&src, "kick_model");
    assert!(
        kick.contains("next_chat_image")
            && kick.contains("kick_native_turn(&last_user, image.as_deref()"),
        "a plus-button still must ride the native turn: {kick}"
    );
    assert!(
        kick.contains("consume_attach") && kick.contains("attach_url"),
        "follow-up kicks must leave the attached image for the next send: {kick}"
    );
    let send_attach = src
        .split("fn send_chat(")
        .nth(1)
        .and_then(|s| s.split("fn send_followup_turn").next())
        .expect("send_chat attach");
    assert!(
        send_attach.contains("attach_prompt_line") && send_attach.contains("attach_name"),
        "the visible user turn must mention the attached still: {send_attach}"
    );
    let cwd = fn_src(&src, "grok_cwd");
    assert!(
            cwd.contains("cabin_session_cwd"),
            "ACP cwd must be the bound project or ~/GrokHub-Work, and History must list that same directory: {cwd}"
        );
    assert!(
        !cwd.contains("unwrap_or_else(|_| self.grok_cwd())"),
        "listing HOME drops a dialogue session stored under the chat cwd: {cwd}"
    );
    assert!(
        !cwd.contains("current_dir"),
        "unbound ACP must not inherit the overlay or cargo tree cwd: {cwd}"
    );
    assert!(
        !cwd.contains("ensure_session_cwd"),
        "ACP cwd lookup must not probe disk on the UI thread: {cwd}"
    );
    let saved = src
        .split("fn apply_single_turn(")
        .nth(1)
        .and_then(|s| s.split("\n    pub(super) fn ").next())
        .expect("apply_single_turn");
    assert!(
        saved.contains("session_saved")
            && (saved.contains("grok_session = Some")
                || saved.contains("bind_reported_grok_session")),
        "a dialogue session stays on the cabin chat, which is History: {saved}"
    );
    assert!(
        !saved.contains("reload_grok_sessions"),
        "a finished headless turn must not rebuild History from grok sessions list: {saved}"
    );
    let bind = src
        .split("Slash::ProjectBind(path)")
        .nth(1)
        .and_then(|s| s.split("Slash::ProjectClear").next())
        .expect("project bind");
    assert!(
        bind.contains("resolve_bind_path"),
        "/project bind . must not inherit the cabin process cwd: {bind}"
    );
    assert!(
        bind.contains("halt_in_flight") && !bind.contains("self.acp.is_some()"),
        "/project bind during handshake must halt, not only drop a live ACP handle: {bind}"
    );
    assert!(
            bind.contains("grok_cwd = None") && bind.contains("grok_session = None"),
            "/project bind must forget the thread worktree or the next send stays in a History tree: {bind}"
        );
    assert!(
        bind.contains("self.persist_cfg()")
            && bind.contains("self.flush_projects()")
            && bind.contains("self.persist()")
            && bind.contains("tree_changed"),
        "/project bind to the current tree must not clone every thread: {bind}"
    );
    let clear = src
        .split("Slash::ProjectClear =>")
        .nth(1)
        .and_then(|s| s.split("Slash::ProjectShow =>").next())
        .expect("ProjectClear handshake");
    assert!(
        clear.contains("self.acp = None"),
        "/project clear must drop the engine so the next turn starts in the new tree: {clear}"
    );
    assert!(
        clear.contains("halt_in_flight") && !clear.contains("self.acp.is_some()"),
        "/project clear during handshake must halt, not only drop a live ACP handle: {clear}"
    );
    assert!(
            clear.contains("grok_cwd = None") && clear.contains("grok_session = None"),
            "/project clear must forget the thread worktree or the next send stays in a History tree: {clear}"
        );
    let sidebar = src
        .split("fn bind_project_id(")
        .nth(1)
        .and_then(|s| s.split("fn make_project(").next())
        .expect("bind_project_id");
    assert!(
        sidebar.contains("self.acp = None"),
        "sidebar bind must drop the engine so the next turn starts in the new tree: {sidebar}"
    );
    assert!(
        sidebar.contains("halt_in_flight") && !sidebar.contains("self.acp.is_some()"),
        "sidebar bind during handshake must halt, not only drop a live ACP handle: {sidebar}"
    );
    assert!(
        !sidebar.contains("grok_session = None") && !sidebar.contains("messages.clear"),
        "sidebar bind must keep the chat transcript and its headless session: {sidebar}"
    );
    let bind_spawn = sidebar
        .find("thread::spawn")
        .expect("bind mkdir must leave the UI thread");
    let bind_mkdir = sidebar.find("create_dir_all").expect("create_dir_all");
    assert!(
        bind_spawn < bind_mkdir,
        "sidebar bind must not freeze the cabin creating the project folder: {sidebar}"
    );
    assert!(
        sidebar.contains("self.persist_cfg()")
            && sidebar.contains("self.persist()")
            && sidebar.contains("tree_changed"),
        "re-clicking a bound project must not clone every thread just to write app.json: {sidebar}"
    );
    let room = src
        .split("Slash::Room(name)")
        .nth(1)
        .and_then(|s| s.split("Slash::Export =>").next())
        .expect("Room");
    assert!(
        room.contains("self.acp = None"),
        "/room must drop the engine so the next turn starts in the new tree: {room}"
    );
    assert!(
        room.contains("halt_in_flight") && !room.contains("self.acp.is_some()"),
        "/room during handshake must halt, not only drop a live ACP handle: {room}"
    );
    assert!(
        room.contains("grok_cwd = None") && room.contains("grok_session = None"),
        "/room must forget the thread worktree or the next send stays in a History tree: {room}"
    );
    assert!(
        room.contains("self.persist_cfg()")
            && room.contains("self.flush_projects()")
            && room.contains("self.persist()")
            && room.contains("tree_changed"),
        "/room to the current tree must not clone every thread: {room}"
    );
    let open = fn_src(&src, "open_grok_session");
    assert!(
        open.contains("grok_cwd"),
        "History open must remember the session worktree: {open}"
    );
    assert!(
        open.contains("apply_switch_thread") && open.contains("self.persist()"),
        "opening a grok session must not clone every thread twice: {open}"
    );
    let show = fn_src(&src, "kick_session_show");
    let show_spawn = show
        .find("thread::spawn")
        .expect("the transcript read must leave the UI thread");
    let show_read = show.find("read_file_capped").expect("read_file_capped");
    assert!(
        show_spawn < show_read
            && !show.contains("read_to_string")
            && !show.contains("find_grok")
            && !show.contains("show_session"),
        "opening a CLI-era session reads its saved file off the UI thread, never the CLI: {show}"
    );
    let open_body = src
        .split("fn open_grok_session(")
        .nth(1)
        .and_then(|s| s.split("fn kick_session_show(").next())
        .expect("open_grok_session body");
    assert!(
        open_body.contains("kick_session_show") && open_body.contains("grok_show_pending = true"),
        "opening a session bound by pin or rename must still load the transcript: {open_body}"
    );
    let kick = fn_src(&src, "kick_imagine");
    assert!(
        kick.contains("imagine_cred()")
            && kick.contains("console_key()")
            && !kick.contains("self.bearer()")
            && !kick.contains("has_key()"),
        "Imagine uses imagine_cred (keychain, then Account, then console key): {kick}"
    );
    assert!(
        kick.contains("bump_usage(&mut self.usage, \"imagine\")"),
        "the imagine bucket has to count something for /usage to mean anything: {kick}"
    );
    let tokens = src
        .split("fn merge_grok_usage(")
        .nth(1)
        .and_then(|s| s.split("fn persist_usage(").next())
        .expect("merge_grok_usage");
    assert!(
        tokens.contains("token_delta") && tokens.contains("add_tokens"),
        "Grok reports session totals — the day must bank the delta: {tokens}"
    );
    // Split so this assertion is not its own counter-example.
    let direct = format!("self.grok_usage{}", ".merge(&");
    assert!(
        !src.contains(&direct),
        "every usage merge goes through merge_grok_usage or the day loses tokens"
    );
    assert_eq!(
        kick.matches("self.bearer()").count(),
        0,
        "Imagine must not fall back to CLI login via bearer(): {kick}"
    );
    let cred = fn_src(&src, "imagine_cred");
    assert!(
        cred.contains("native_account_access")
            && cred.contains("imagine_oauth_preferred")
            && cred.contains("choose_imagine_bearer"),
        "imagine_cred must try Imagine keychain, then Account, then console key: {cred}"
    );
    let imag = src
        .split("fn ui_imagine(")
        .nth(1)
        .and_then(|s| s.split("fn ui_imagine_bar(").next())
        .expect("ui_imagine");
    assert!(
        imag.contains("imagine_stage_visible") && imag.contains("imagine_stage("),
        "Imagine must paint a generating/result box: {imag}"
    );
    assert!(
        imag.contains("imagine_masonry") && imag.contains("imagine-scroll"),
        "the photogif wall stays reachable by scrolling under the generating box: {imag}"
    );
    assert!(
        imag.contains("ImagineToolboxDock::Bottom")
            && imag.contains("imagine-lightbox")
            && imag.contains("start_imagine_save")
            && imag.contains("play_imagine_media")
            && imag.contains("stage_hit.play")
            && imag.contains("imagine_is_video_path"),
        "send docks the chat box; generated stills expand and save: {imag}"
    );
    assert!(
        imag.contains("!grokhub_core::imagine_is_video_path(&last)"),
        "a ready video must play, not open the still lightbox: {imag}"
    );
    assert!(
        src.contains("pin_generation_to_wall") && src.contains("wall_gif_from_generation"),
        "generated stills must land on the Imagine wall"
    );
    let poll = src
        .split("fn poll_acp(")
        .nth(1)
        .and_then(|s| s.split("fn finish_acp_turn(").next())
        .expect("poll_acp");
    assert!(
        poll.contains("grok_session") && poll.contains("session_id"),
        "ACP Ready must stamp the grok session id: {poll}"
    );
    assert!(
        poll.contains("cancelled") && poll.contains("!self.running"),
        "session/cancel Done must not finish a live or redirected turn: {poll}"
    );
    assert!(
        poll.contains("answer_permission") && poll.contains("AcpEvent::Err"),
        "ACP Err must deny leftover Ask or the next send hangs: {poll}"
    );
    assert!(
        poll.contains("chat_job_thread"),
        "ACP Ready must stamp the job thread, not whichever tab is visible: {poll}"
    );
    let done = poll
        .split("AcpEvent::Done")
        .nth(1)
        .and_then(|s| s.split("AcpEvent::Err").next())
        .expect("AcpEvent::Done");
    assert!(
        done.contains("mem::take")
            && !done.contains("stream_buf.clone()")
            && !done.contains("thought_buf.clone()"),
        "ACP Done must take the stream buffers, not clone an 8MB complete on the UI thread: {done}"
    );
    let err = poll.split("AcpEvent::Err").nth(1).expect("err arm");
    assert!(
        !err.contains("grok_session = None")
            && err.contains("self.acp = None")
            && err.contains("maybe_continue_ptt"),
        "agent exit must keep the attached Grok Build session id and resume PTT: {err}"
    );
    let show = fn_src(&src, "poll_session_show");
    assert!(
        show.contains("persist_bg") && show.contains("parse_session_markdown"),
        "History show must persist the transcript, not wait for the next idle tick: {show}"
    );
}

#[test]
fn history_rail_uses_session_names_and_can_delete() {
    let src = cabin_src();
    let rail = src
        .split("id_salt(\"rail-history\")")
        .nth(1)
        .and_then(|s| s.split("fn cached_chat_views(").next())
        .expect("rail-history");
    assert!(
        rail.contains("chat_section_indices") && rail.contains("TabAct::Switch"),
        "sidebar History is cabin chats, not grok sessions list: {rail}"
    );
    assert!(
        !rail.contains("discover_session_files") && !rail.contains("reload_grok_sessions"),
        "sidebar History must not walk session dirs or relist the CLI: {rail}"
    );
    assert!(
        rail.contains("TabAct::Delete") && rail.contains("button(\"Delete\")"),
        "sidebar chat rows must offer Delete: {rail}"
    );
    assert!(
        rail.contains("session_list_order")
            && rail.contains("\"Unpin\"")
            && rail.contains("\"Pin\"")
            && rail.contains("\"Rename\""),
        "sidebar chat rows must pin and rename: {rail}"
    );
    assert!(
        rail.contains("is_background_history_title"),
        "sidebar History must drop workboard summarize and similar: {rail}"
    );
    let side = include_str!("sidebar.rs");
    assert!(
        !side.contains("rail-archived")
            && !side.contains("RichText::new(\"Archived\")")
            && !side.contains("No archived chats"),
        "sidebar must not paint an Archived section: {side}"
    );
    let page = src
        .split("crate::cards::section_label(ui, \"Chats\")")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("history chats section");
    assert!(
        page.contains("thread_rail_title") && page.contains("chat_section_indices"),
        "History page must paint cabin chats: {page}"
    );
    assert!(
        page.contains("Delete") && page.contains("delete_thread_at"),
        "History page must delete the cabin chat: {page}"
    );
    assert!(
        page.contains("is_background_history_title"),
        "History page must not list workboard summarize: {page}"
    );
    assert!(
        !page.contains("Archived chats")
            && !page.contains("No archived chats")
            && !page.contains("\"Unarchive\""),
        "History page must not paint an Archived drawer: {page}"
    );
    let dta = fn_src(&src, "delete_thread_at");
    assert!(
        dta.contains("delete_session") && !dta.contains("grok_user_stdout"),
        "deleting a chat drops its native session and never runs the CLI: {dta}"
    );
    assert!(
        page.contains("self.nav = Nav::History"),
        "deleting a History chat must keep the See all pane: {page}"
    );
    let hist = src
        .split("fn ui_history(")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("ui_history");
    assert!(
        hist.contains("Delete all") && hist.contains("delete_all_history"),
        "History See all must offer Delete all: {hist}"
    );
    let poll = src
        .split("fn poll_single(")
        .nth(1)
        .and_then(|s| s.split("fn upsert_stream_assistant(").next())
        .expect("poll_single");
    assert!(
        poll.contains("apply_auto_title"),
        "a finished turn must name the tab from the session, not leave Chat: {poll}"
    );
    assert!(
        poll.contains("GrokPEvent::Usage")
            && poll.contains("GrokPEvent::Compact")
            && poll.contains("thinking_status")
            && poll.contains("turn_footer"),
        "1.0.13 stream must paint usage, compact, and a turn footer: {poll}"
    );
    assert!(
        poll.contains("GrokPEvent::Recovering") && poll.contains("apply_compact_status"),
        "1.0.13 truncation/5xx recovery and compact errors must not kill the turn: {poll}"
    );
    assert!(
        poll.contains("retry_status_line"),
        "1.0.14 retry status must show a short reason: {poll}"
    );
    assert!(
        poll.contains("scheduled_perm = false"),
        "a finished or failed grok -p turn must drop scheduled_perm: {poll}"
    );
    let deleted = src
        .split("fn delete_thread_at")
        .nth(1)
        .and_then(|s| s.split("fn send_chat").next())
        .expect("delete_thread_at");
    assert!(
        deleted.contains("grokhub_agent::delete_session"),
        "deleting a History chat must drop its native session: {deleted}"
    );
    let title = fn_src(&src, "thread_rail_title");
    assert!(
        title.contains("preferred_history_title"),
        "rail titles must prefer the Grok Build session name: {title}"
    );
}

#[test]
fn refresh_chips_does_not_rebuild_every_frame() {
    let src = cabin_src();
    let chips = src
        .split("fn refresh_chips(")
        .nth(1)
        .and_then(|s| s.split("fn spawn_chip_llm(").next())
        .expect("refresh_chips");
    assert!(
        chips.contains("chip_paint_key") && chips.contains("return;"),
        "chips must not clone the transcript and walk other threads on every paint: {chips}"
    );
    let pairs = chips.find("chat_pairs").expect("chip chat_pairs");
    assert!(
        chips[..pairs].contains("self.running") && chips[..pairs].contains("return"),
        "a growing stream must not clone the transcript to rebuild chips: {chips}"
    );
    assert!(
        chips.contains("chip_chat_pairs") || chips.contains("chip_scan"),
        "chip rebuild must not clone an 8MB complete into chat_pairs: {chips}"
    );
    assert!(
        chips.contains("host_on: false"),
        "empty chips must not inject HOST_CMD host_chips: {chips}"
    );
    assert!(
        chips.contains("grok_connected: self.cabin_signed_in()"),
        "Connect chip must follow signed-in, not a PATH grok: {chips}"
    );
    assert!(
        !chips.contains("grok_connected: self.llm_ready()"),
        "PATH grok must not hide Connect Grok: {chips}"
    );
}

#[test]
fn speak_reply_does_not_clone_an_8mb_complete() {
    let src = cabin_src();
    let speak = src
        .split("fn speak_reply(")
        .nth(1)
        .and_then(|s| s.split("fn poll_eyes_cap(").next())
        .expect("speak_reply");
    let clone = speak.find("to_string()").expect("tts clone");
    assert!(
        speak[..clone].contains("TEXT_FILE_CAP")
            || speak[..clone].contains("chip_scan")
            || speak[..clone].contains("take_ui"),
        "voice speak must not clone an 8MB complete onto the UI thread: {speak}"
    );
    assert!(
        speak.contains("voice_tts_script"),
        "TTS must strip THINKING: / thoughts before grok_tts: {speak}"
    );
}

#[test]
fn the_night_slot_runs_loops_and_clock_time_automations() {
    let src = cabin_src();
    let job = src
        .split("JobKind::Automations =>")
        .nth(1)
        .and_then(|s| s.split("JobKind::PulseRefresh =>").next())
        .expect("automations job");
    assert!(
        job.contains("self.tick_loops() || self.tick_night()"),
        "the Automations job owns both schedulers, loops first, and a 09:00 automation \
             still fires when no loop is due: {job}"
    );
    // The governor sets the cadence: neither keeps a private clock, so an idle
    // loop list cannot starve automations and nothing is scheduled twice.
    let night = src
        .split("fn tick_night(")
        .nth(1)
        .and_then(|s| s.split("fn poll_night_check(").next())
        .expect("tick_night");
    let loops = src
        .split("fn tick_loops(")
        .nth(1)
        .and_then(|s| s.split("fn poll_grok_loop(").next())
        .expect("tick_loops");
    for body in [night, loops] {
        assert!(
            !body.contains("last_auto_tick")
                && !body.contains("last_night_tick")
                && !body.contains(".elapsed()"),
            "no self-timing left in the night schedulers: {body}"
        );
    }
    assert!(
        !src.contains("last_auto_tick") && !src.contains("last_night_tick"),
        "the old debounce clocks are gone"
    );
}

#[test]
fn periodic_persist_leaves_the_ui_thread() {
    let src = cabin_src();
    let beat = src
        .split("fn tick_heartbeat")
        .nth(1)
        .and_then(|s| s.split("fn tick_anticipate").next())
        .expect("tick_heartbeat");
    assert!(
        beat.contains("persist_bg(") && !beat.contains("self.persist()"),
        "2s housekeep persist must not block the cabin: {beat}"
    );
    let paint = src
        .split("self.flush_window(ctx)")
        .nth(1)
        .and_then(|s| s.split("next_heartbeat_wait_ms").next())
        .expect("update persist");
    assert!(
        paint.contains("persist_bg(") && !paint.contains("self.persist()"),
        "2s paint persist must not block the cabin: {paint}"
    );
    let apply = src
        .split("fn apply_saved_geom(")
        .nth(1)
        .and_then(|s| s.split("fn capture_window(").next())
        .expect("apply_saved_geom");
    assert!(
        apply.contains("InnerSize") && apply.contains("OuterPosition"),
        "launch must apply the remembered inner size and outer position: {apply}"
    );
    let capture = src
        .split("fn capture_window(")
        .nth(1)
        .and_then(|s| s.split("fn flush_window(").next())
        .expect("capture_window");
    assert!(
        capture.contains("geom_can_remember") && capture.contains("apply_saved_geom"),
        "first frames must restore size/position, not clobber app.json: {capture}"
    );
    let show = src
        .split("fn show_from_tray(")
        .nth(1)
        .and_then(|s| s.split("fn poll_tray(").next())
        .expect("show_from_tray");
    assert!(
        show.contains("apply_saved_geom"),
        "Show cabin must restore size and position: {show}"
    );
    let exit = src
        .split("fn on_exit(")
        .nth(1)
        .and_then(|s| s.split("fn logic(").next())
        .expect("on_exit");
    assert!(
        exit.contains("config::save") && exit.contains("cfg.window")
            || exit.contains("config::save(&cfg)"),
        "SIGTERM must write the remembered window: {exit}"
    );
    let flush = fn_src(&src, "flush_window");
    assert!(
        flush.contains("submit_window_cfg")
            && flush.contains("persist_svc")
            && !flush.contains("thread::spawn"),
        "window geom must not freeze the cabin writing app.json: {flush}"
    );
    let bg = fn_src(&src, "persist_bg");
    assert!(
        bg.contains("submit_full") && bg.contains("persist_svc") && !bg.contains("thread::spawn"),
        "persist_bg must leave the UI thread via PersistService: {bg}"
    );
    let svc = include_str!("persist_service.rs").replace("\r\n", "\n");
    assert!(
        svc.contains("thread::spawn") && svc.contains("write_persist_disk_in_order"),
        "PersistService must own the one worker that writes threads.json: {svc}"
    );
    assert!(
        bg.contains("persist_idle_key") && bg.contains("return;"),
        "idle 2s persist must not clone every thread on the UI thread: {bg}"
    );
    let snap = bg.find("persist_snap").expect("persist_snap");
    assert!(
        bg[..snap].contains("self.running") && bg[..snap].contains("return"),
        "a growing stream must not clone every thread to persist an 8MB bubble: {bg}"
    );
    assert!(
        !bg[..snap].contains("geom_dirty"),
        "window drag must not clone every thread — flush_window owns geom: {bg}"
    );
    let idle_key = src
        .split("fn persist_idle_now(")
        .nth(1)
        .and_then(|s| s.split("fn persist_bg(").next())
        .expect("persist idle key");
    assert!(
            !idle_key.contains("projects_dirty"),
            "folder click must not clone every thread twice — persist_idle_key must ignore the dirty flag: {idle_key}"
        );
}

#[test]
fn chat_capture_leaves_the_ui_thread() {
    let src = cabin_src();
    let cap = src
        .split("fn capture_cabin_frame_this_turn")
        .nth(1)
        .and_then(|s| s.split("fn apply_job_fail").next())
        .expect("capture_cabin_frame_this_turn");
    let spawn = cap
        .find("thread::spawn")
        .expect("chat grim must leave the UI thread");
    let shot = cap.find("capture_data_url").expect("screen capture");
    let rows = cap.find("collect_rows").expect("desk scan");
    assert!(
        spawn < rows && rows < shot,
        "send/HostDone capture must not block the cabin: {cap}"
    );
    let ask_kick = fn_src(&src, "kick_model");
    assert!(
        ask_kick.contains("pending_kick") && ask_kick.contains("kick_cap_rx"),
        "kick_model must wait for the off-thread frame instead of blocking: {ask_kick}"
    );
    assert!(
        ask_kick
            .trim_end()
            .trim_end_matches('}')
            .trim_end()
            .ends_with(
                "self.kick_native_turn(&last_user, image.as_deref(), &raw_ask, &thread_label);"
            ),
        "every turn goes to the native engine: {ask_kick}"
    );
    for gone in [
        "spawn_grok_p_stream",
        "uses_acp",
        "scheduled_flags",
        "route_gb_turn(",
    ] {
        assert!(!ask_kick.contains(gone), "{gone}: {ask_kick}");
    }
    assert!(
        ask_kick.contains("apply_skill_follow") && ask_kick.contains("active_skill_follow"),
        "selecting a skill must inject the follow block into grok -p / ACP: {ask_kick}"
    );
}

#[test]
fn scheduled_night_loop_and_send_inherit_permission_mode() {
    let src = cabin_src();
    let fire_loop = fn_src(&src, "fire_loop");
    let native_src = include_str!("native_unattended.rs").replace("\r\n", "\n");
    let native_loop = fn_src(&native_src, "spawn_native_loop");
    assert!(
        fire_loop.contains("spawn_native_loop")
            && native_loop.contains("scheduled_gate(self, self.session_mode)"),
        "loop spawn must read the composer PermissionMode pill: {native_loop}"
    );
    let gate = fn_src(&native_src, "scheduled_gate");
    assert!(
        gate.contains("cabin.permission_mode") && gate.contains("attended: false"),
        "{gate}"
    );
    assert!(
        !fire_loop.contains("\"--always-approve\""),
        "Ask must not silent always-approve a loop: {fire_loop}"
    );
    let fire_night = fn_src(&src, "fire_night");
    let start = fn_src(&src, "start_scheduled_run");
    assert!(
        start.contains("start_bg_task"),
        "night runs in its own background process: {start}"
    );
    let bg = fn_src(&src, "native_bg_gate");
    assert!(
        bg.contains("self.permission_mode") && bg.contains("attended: false"),
        "a background run must inherit PermissionMode, not a separate yolo path: {bg}"
    );
    let send_at = fire_night.find("start_scheduled_run").expect("night start");
    let ran_at = fire_night.rfind("mark_auto_ran").expect("night mark ran");
    assert!(
        send_at < ran_at,
        "night must mark ran after the run started, not before: {fire_night}"
    );
    let send_block = &fire_night[send_at..];
    assert!(
        send_block.contains("Ok(_)")
            && send_block.contains("mark_auto_ran")
            && send_block.contains("mark_auto_skipped"),
        "a night run that did not start must skip, not retry every 5s: {fire_night}"
    );
    let inbox = fn_src(&src, "drain_inbox");
    assert!(
        inbox.contains("send_scheduled_chat"),
        "a queued /send task must inherit PermissionMode: {inbox}"
    );
    let anticipate = fn_src(&src, "tick_anticipate");
    assert!(
        anticipate.contains("send_scheduled_chat"),
        "heartbeat anticipate must inherit PermissionMode: {anticipate}"
    );
    let native_src = include_str!("native_engine.rs").replace("\r\n", "\n");
    let gate = fn_src(&native_src, "native_gate");
    assert!(
        gate.contains("PermissionMode::Ask => PermMode::Ask")
            && gate.contains("PermissionMode::Auto => PermMode::Auto")
            && gate.contains("PermissionMode::AlwaysApprove => PermMode::Always")
            && gate.contains("attended: !self.scheduled_perm"),
        "native turns must map Auto/Always and run scheduled Ask unattended (fail-closed): {gate}"
    );
    let kick = fn_src(&src, "kick_model");
    assert!(
        kick.contains("perm_always_confirm = None"),
        "a kick that clears perm_ask must also drop Always confirm: {kick}"
    );
    let scheduled = fn_src(&src, "send_scheduled_chat");
    assert!(
        scheduled.contains("scheduled_perm = true") && scheduled.contains("send_chat"),
        "scheduled enqueue must run the turn unattended: {scheduled}"
    );
    let fail_ask = fn_src(&src, "fail_turn_start");
    assert!(
        fail_ask.contains("maybe_continue_ptt"),
        "Ask deny must resume PTT: {fail_ask}"
    );
    assert!(
        fail_ask.contains("scheduled_perm = false"),
        "Ask deny must drop scheduled_perm so the next typed Ask uses ACP: {fail_ask}"
    );
    let finish = fn_src(&src, "finish_acp_turn");
    assert!(
        finish.contains("scheduled_perm = false"),
        "a finished chat turn must drop scheduled_perm so the next typed Ask uses ACP: {finish}"
    );
    let halt = fn_src(&src, "halt_in_flight");
    assert!(
        halt.contains("scheduled_perm = false"),
        "Stop must drop scheduled_perm: {halt}"
    );
    let kick_err = fn_src(&native_src, "kick_native_turn");
    assert!(
        kick_err.matches("self.fail_turn_start(").count() == 3,
        "a native turn that fails to start drops scheduled_perm and resumes PTT: {kick_err}"
    );
    let poll_acp = fn_src(&src, "poll_acp");
    let err = poll_acp
        .split("AcpEvent::Err")
        .nth(1)
        .expect("AcpEvent::Err");
    assert!(
        err.contains("maybe_continue_ptt"),
        "fatal ACP Err must resume PTT: {err}"
    );
}

#[test]
fn plus_upload_does_not_rescan_the_folder_every_frame() {
    let src = cabin_src();
    let overlay = src
        .split("fn ui_plus_overlays(")
        .nth(1)
        .and_then(|s| s.split("fn ui_imagine_overlays(").next())
        .expect("ui_plus_overlays");
    assert!(
        overlay.contains("cached_pick_entries") && !overlay.contains("Self::pick_entries"),
        "Upload window must not read_dir every paint: {overlay}"
    );
    let cache = src
        .split("fn cached_pick_entries(")
        .nth(1)
        .and_then(|s| s.split("fn ui_plus_overlays(").next())
        .expect("cached_pick_entries");
    assert!(
        cache.contains("pick_cache") && cache.contains("pick_entries("),
        "folder listing must reuse the last scan until pick_dir changes: {cache}"
    );
    let cache_spawn = cache
        .find("thread::spawn")
        .expect("listing must leave the UI thread");
    let cache_walk = cache.find("pick_entries(").expect("pick_entries");
    assert!(
        cache_spawn < cache_walk,
        "Upload folder listing must not read_dir on the UI thread: {cache}"
    );
    let upload = src
        .split("PlusAct::Upload =>")
        .nth(1)
        .and_then(|s| s.split("PlusAct::Paste =>").next())
        .expect("Upload");
    let spawn = upload.find("thread::spawn").expect("picker worker");
    let pick = upload.find("pick_file()").expect("native picker");
    let load = upload.find("plus_from_path").expect("decode off-thread");
    assert!(
        spawn < pick && pick < load && upload.contains("pick_rx") && !upload.contains("apply_path"),
        "zenity/kdialog and JPEG decode must not freeze the cabin on plus-upload: {upload}"
    );
    let paste = src
        .split("PlusAct::Paste =>")
        .nth(1)
        .and_then(|s| s.split("fn poll_pick(").next())
        .expect("Paste");
    let paste_spawn = paste.find("thread::spawn").expect("clipboard worker");
    let clip = paste.find("clipboard_image()").expect("clipboard image");
    assert!(
        paste_spawn < clip
            && paste.contains("clipboard_once")
            && paste.contains("plus_from_path")
            && !paste.contains("apply_path"),
        "xclip/wl-paste and JPEG decode must not freeze the cabin on plus-paste: {paste}"
    );
    let poll = src
        .split("fn poll_pick(")
        .nth(1)
        .and_then(|s| s.split("fn apply_clipboard(").next())
        .expect("poll_pick");
    assert!(
        poll.contains("apply_plus_ready")
            && poll.contains("file_pick")
            && !poll.contains("load_image_data_url"),
        "plus-upload worker must land the still or fall back to the in-app picker: {poll}"
    );
    assert!(
        src.contains("self.poll_pick()"),
        "plus-upload worker must be polled each frame"
    );
    assert!(
        overlay.contains("start_plus_path") && !overlay.contains("apply_path"),
        "in-app Upload clicks must decode off the UI thread: {overlay}"
    );
}

#[test]
fn live_room_captures_off_the_ui_thread() {
    let src = cabin_src();
    let live = src
        .split("fn live_room")
        .nth(1)
        .and_then(|s| s.split("fn tick_mid_thought").next())
        .expect("live_room");
    let spawn = live
        .find("thread::spawn")
        .expect("grim/ffmpeg must leave the UI thread");
    let shot = live.find("capture_data_url").expect("screen capture");
    let cam = live.find("capture_webcam").expect("webcam");
    assert!(
        spawn < shot && spawn < cam,
        "presence capture must not block the cabin: {live}"
    );
    assert!(
        live.contains("try_recv") && live.contains("live_cap_rx"),
        "UI thread must apply one in-flight frame without stacking grim: {live}"
    );
    assert!(
        live.contains("collect_rows")
            && live.contains("lock_titles")
            && live.contains("should_send_screenshot"),
        "lock and title gates stay on the UI thread: {live}"
    );
    assert!(
        !live.contains("webcam_url") && !live.contains("cam:"),
        "live room must not land an unbounded webcam data URL on the UI thread: {live}"
    );
}

#[test]
fn chat_side_effects_keep_the_origin_thread() {
    let src = cabin_src();
    assert!(
        src.contains("let origin = self.chat_job_thread.take()"),
        "host/connector/imagine after Chat must rebind the origin tab"
    );
    let agent_job = src
        .split("struct AgentJob")
        .nth(1)
        .and_then(|s| s.split("fn listen_turn").next())
        .expect("AgentJob");
    assert!(
        agent_job.contains("thread_id"),
        "Queue jobs must remember the origin thread: {agent_job}"
    );
    let listen = src
        .split("fn listen_turn(")
        .nth(1)
        .and_then(|s| s.split("fn fit_rail_label").next())
        .expect("listen_turn");
    let wav_read = listen.find("std::fs::read(&wav)").expect("wav read");
    assert!(
        listen.contains("IMAGE_FILE_CAP")
            && listen.find("IMAGE_FILE_CAP").expect("wav cap") < wav_read,
        "voice STT must not slurp a huge wav: {listen}"
    );
    let queue = fn_src(&src, "ui_agents");
    let queued = fn_src(&src, "start_queued_job");
    assert!(
        !queue.contains("send_chat")
            && queue.contains("kick_model")
            && queued.contains("chat_job_thread")
            && queued.contains("push_bound_msg")
            && !queued.contains("kick_model"),
        "Queue Run must kick the origin thread, not send_chat on the visible tab: {queue} {queued}"
    );
    assert!(
        src.contains("finish_hub_dispatch"),
        "a queued /send task must complete its hub row"
    );
    assert!(
        src.contains("hub_dispatch_ok(&text)"),
        "GOAL_BLOCKED must not complete a queued task as done"
    );
    assert!(
        src.contains("oauth_access_live"),
        "expired OAuth without refresh must not hide a console key"
    );
    assert!(
        src.contains("next_oauth_poll_secs"),
        "Settings OAuth must honor interval and slow_down"
    );
    let cmds = src
        .split("fn run_cmds")
        .nth(1)
        .and_then(|s| s.split("fn run_connector").next())
        .expect("run_cmds");
    assert!(
        cmds.contains("if self.chat_job_thread.is_none()"),
        "run_cmds must not retarget a job that started on another tab"
    );
    assert!(
        cmds.contains("push_bound_msg"),
        "blocked host receipts must stay on the job thread"
    );
    assert!(
        cmds.contains("host_working_dir(&self.cfg.project_dir)")
            && cmds.contains("run_host_stream"),
        "host shell must start in the bound project, not the cabin process cwd: {cmds}"
    );
    assert!(
        cmds.contains("parse_computer_op") && !cmds.contains("parse_computer_cmd_loose"),
        "HOST_CMD / Command-pane type cargo must stay shell, not desktop type-in: {cmds}"
    );
    let ret = cmds.find("return blocked;").expect("return blocked");
    let rewind = cmds.rfind("is_rewind_copy_cmd").expect("rewind copy");
    assert!(
        ret < rewind && cmds[ret..rewind].contains("self.persist()"),
        "mixed blocked+allowed host must persist block receipts before spawn: {cmds}"
    );
    let host_done = fn_src(&src, "poll_job");
    assert!(
        host_done.contains("pending_connectors") && host_done.contains("self.kick_model(false)"),
        "an all-blocked host plan must still kick the model after connectors"
    );
    let halt = src
        .split("fn halt_work")
        .nth(1)
        .and_then(|s| s.split("fn drain_inbox").next())
        .expect("halt_work");
    assert!(
        halt.contains("finish_hub_dispatch"),
        "Stop / tray halt must complete a claimed /send task"
    );
    assert!(
        src.contains("self.finish_hub_dispatch(worker_gone_status(), false)"),
        "a dropped worker must fail the claimed /send task"
    );
    assert!(
        src.contains("requeue_claimed_for"),
        "unstick claimed rows on boot"
    );
    let inbox = src
        .split("fn drain_inbox")
        .nth(1)
        .and_then(|s| s.split("fn finish_hub_dispatch").next())
        .expect("drain_inbox");
    assert!(
        inbox.contains("pending_hub_task.is_some()"),
        "do not claim a second /send task while one is still pending: {inbox}"
    );
    assert!(
        inbox.contains("land_on_real_chat"),
        "a claimed /send task must not land on Scratch: {inbox}"
    );
    assert!(
        src.contains("night_counts_run"),
        "a night replay that did not start must not consume the slot"
    );
    let fire_night = src
        .split("fn fire_night")
        .nth(1)
        .and_then(|s| s.split("fn tick_review").next())
        .expect("fire_night");
    assert!(
        fire_night.contains("mark_auto_skipped"),
        "a skipped night slot is marked: {fire_night}"
    );
    let counts = fire_night
        .find("night_counts_run")
        .expect("night_counts_run");
    assert!(
        fire_night[counts..].contains("mark_auto_skipped"),
        "a missing night recipe must skip the slot, not hammer every 5s: {fire_night}"
    );
    let bump = fire_night.find("bump_usage").expect("night usage");
    let usage_save = fire_night
        .find("persist_usage")
        .expect("night persist_usage");
    assert!(
        bump < usage_save && !fire_night.contains("self.persist()"),
        "a night replay must stamp usage.json without cloning every thread: {fire_night}"
    );
    assert!(
        !fire_night.contains("land_on_real_chat") && !fire_night.contains("self.nav"),
        "a night job runs in the background and never opens the chat page: {fire_night}"
    );
    assert!(!fire_night.contains("can_agent"), "{fire_night}");
    let night_check = src
        .split("fn poll_night_check")
        .nth(1)
        .and_then(|s| s.split("fn spawn_night_check").next())
        .expect("poll_night_check");
    assert!(
        night_check.contains("night_check_may_fire"),
        "a finished night check must not halt a live job: {night_check}"
    );
    let send = src
        .split("fn send_chat")
        .nth(1)
        .and_then(|s| s.split("fn send_followup_turn").next())
        .expect("send_chat");
    let redirect = send
        .split("ChatSendKind::Redirect")
        .nth(1)
        .and_then(|s| s.split("ChatSendKind::Fresh").next())
        .expect("redirect");
    assert!(
        !redirect.contains("content.clone()"),
        "redirect must not clone the transcript to read the last user turn: {redirect}"
    );
    let slash_at = send.find("parse_slash").expect("parse_slash");
    let kind_at = send.find("chat_send_kind").expect("chat_send_kind");
    assert!(
        slash_at < kind_at,
        "/compact during a live job must stay local, not become a redirect: {send}"
    );
    assert!(
        send.contains("unknown_cabin_slash") && send.contains("Unknown command"),
        "unknown /project binding must stay local, not go to Grok: {send}"
    );
    let compact = src
        .split("Slash::Compact =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Skill").next())
        .expect("Compact");
    assert!(
        compact.contains("stamp_current_access") || compact.contains("accessed_ms"),
        "/compact must bump accessed_ms or /sync LWW can restore the dropped turns: {compact}"
    );
    assert!(
        !compact.contains("content.clone()"),
        "/compact must not clone an 8MB pane: {compact}"
    );
    let pushed = send.find("live_mut().push").expect("user turn");
    let saved = send.find("self.persist()").expect("send persist");
    assert!(
        send[pushed..saved].contains("stamp_current_access")
            || send[pushed..saved].contains("accessed_ms"),
        "a sent turn must bump accessed_ms or /sync LWW can drop it: {send}"
    );
    let fail = src
        .split("fn apply_job_fail")
        .nth(1)
        .and_then(|s| s.split("fn queue_update").next())
        .expect("apply_job_fail");
    assert!(
        fail.contains("accessed_ms") || fail.contains("stamp_current_access"),
        "a job error on the origin thread must bump accessed_ms or /sync LWW can drop it: {fail}"
    );
    assert!(
        fail.contains("apply_job_error") && !fail.contains("content.clone()"),
        "a job error must not clone an 8MB pane to replace the last assistant: {fail}"
    );
    let queued = src
        .split("fn queue_update(")
        .nth(1)
        .and_then(|s| s.split("fn restart_after_update").next())
        .expect("queue_update");
    assert!(
        queued.contains("queue_cabin_update")
            && !queued.contains("update_cmds_for")
            && !queued.contains("config::save")
            && !queued.contains("persist_snap"),
        "/update must use the same pending plan as the chip: {queued}"
    );
    assert!(
        src.contains("settings_update_hint")
            && src.contains("queue_cabin_update")
            && src.contains("UPDATE_CHECK_EVERY"),
        "one Update control must describe the cabin update and recheck on the 2h interval: {src}"
    );
    for gone in [
        "Install Grok Build CLI",
        "queue_grok_cli_install",
        "begin_grok_install",
    ] {
        assert!(
            !src.contains(gone),
            "GrokHub never installs the Grok Build CLI: {gone}"
        );
    }
    let queued_cli = src
        .split("fn queue_cabin_update(")
        .nth(1)
        .and_then(|s| s.split("fn note_cabin_update_landed(").next())
        .expect("queue_cabin_update");
    assert!(
        queued_cli.contains("update_cmds_for_host_in(src.as_deref(), cfg!(windows), channel)")
            && queued_cli.contains("start_overlay_update")
            && !queued_cli.contains("UpdatePending")
            && !queued_cli.contains("grok update")
            && queued_cli.contains("self.open_update_overlay()")
            && queued_cli.find("self.open_update_overlay()").unwrap()
                < queued_cli.find("update_cmds_for_host_in").unwrap()
            && queued_cli.contains("self.persist_cfg()")
            && queued_cli.contains("overlay_clone_usable")
            && !queued_cli.contains("config::save")
            && !queued_cli.contains("persist_snap")
            && !queued_cli.contains("Set Settings → source")
            && !queued_cli.contains("--stable")
            && !queued_cli.contains("begin_grok_install")
            && !queued_cli.contains("update_hint(UpdatePending::None)"),
        "the one Update control overlays the cabin, even when the probe missed: {queued_cli}"
    );
    let overlay = src
        .split("fn start_overlay_update(")
        .nth(1)
        .and_then(|s| s.split("fn drain_queued_update(").next())
        .expect("start_overlay_update");
    let settings_at = overlay.find("Nav::Settings").expect("settings");
    let running_at = overlay.find("if self.running").expect("running");
    assert!(
        settings_at < running_at
            && overlay.contains("queued_overlay")
            && src.contains("drain_queued_update"),
        "a busy chip click must open Settings and queue the update: {overlay}"
    );
    let boot = src
        .split("pub fn new(hidden: bool)")
        .nth(1)
        .and_then(|s| s.split("fn apply_saved_geom(").next())
        .expect("Cabin::new");
    assert!(
        boot.contains("begin_update_probe"),
        "boot must check GitHub Latest for the in-app chip: {boot}"
    );
    assert!(
        src.contains("titlebar_update_chip")
            && src.contains("update_chip_label")
            && src.contains("queue_cabin_update")
            && !src.contains(concat!("Update ", "available")),
        "the titlebar chip names CLI, cabin, or both and runs that update: {src}"
    );
    let flush_p = src
        .split("fn flush_projects(")
        .nth(1)
        .and_then(|s| s.split("fn bind_project_id").next())
        .expect("flush_projects");
    assert!(
        flush_p.contains("submit_projects")
            && flush_p.contains("persist_svc")
            && !flush_p.contains("thread::spawn"),
        "folder click must not freeze the cabin writing projects.json: {flush_p}"
    );
    let drop_proj = src
        .split("fn remove_project_id(")
        .nth(1)
        .and_then(|s| s.split("fn apply_project_menu(").next())
        .expect("remove_project_id");
    assert!(
            drop_proj.contains("self.flush_projects()")
                && drop_proj.contains("self.persist()")
                && drop_proj.contains("out.unbound"),
            "deleting an unbound project must not clone every thread just to write projects.json: {drop_proj}"
        );
    let folders = format!(
        "{}{}{}{}{}{}",
        fn_src(&src, "make_folder"),
        fn_src(&src, "stage_new_folder"),
        fn_src(&src, "begin_proj_rename"),
        fn_src(&src, "cancel_proj_rename"),
        fn_src(&src, "finish_proj_rename"),
        fn_src(&src, "move_sel_to_folder_name"),
    );
    assert!(
            folders.contains("self.flush_projects()")
                && !folders.contains("self.persist()")
                && !folders.contains("persist_snap"),
            "folder create/rename/move must not clone every thread just to write projects.json: {folders}"
        );
    let menu = src
        .split("fn apply_project_menu(")
        .nth(1)
        .and_then(|s| s.split("fn make_folder(").next())
        .expect("apply_project_menu");
    assert!(
        menu.contains("self.flush_projects()")
            && !menu.contains("self.persist()")
            && !menu.contains("persist_snap"),
        "Remove from folder must not clone every thread just to write projects.json: {menu}"
    );
    let overlays = fn_src(&src, "ui_project_overlays");
    assert!(
        !overlays.contains("New project") && !src.contains("New project or folder"),
        "the projects section does not offer New project"
    );
    assert!(
        src.contains("on_hover_text(\"New folder\")") && src.contains("self.stage_new_folder()"),
        "projects + creates a folder"
    );
    let rename = src
        .split("Slash::ProjectRename")
        .nth(1)
        .and_then(|s| s.split("Slash::ProjectMove").next())
        .expect("ProjectRename");
    assert!(
        rename.contains("self.flush_projects()")
            && !rename.contains("self.persist()")
            && !rename.contains("persist_snap"),
        "/project rename must not clone every thread just to write projects.json: {rename}"
    );
    let overlay = fn_src(&src, "ui_project_overlays");
    assert!(
        overlay.contains("self.flush_projects()")
            && !overlay.contains("self.persist()")
            && !overlay.contains("persist_snap"),
        "Add to folder must not clone every thread just to write projects.json: {overlay}"
    );
    let renamed = src
        .split("fn rename_thread")
        .nth(1)
        .and_then(|s| s.split("fn pin_thread").next())
        .expect("rename_thread");
    assert!(
        renamed.contains("accessed_ms"),
        "rename must bump accessed_ms or /sync LWW can drop the new title: {renamed}"
    );
    let pinned = src
        .split("fn pin_thread")
        .nth(1)
        .and_then(|s| s.split("fn delete_thread_at").next())
        .expect("pin_thread");
    assert!(
        pinned.contains("accessed_ms") && pinned.contains("pinned_ms"),
        "pin must bump accessed_ms and record pinned_ms or /sync LWW can drop the pin: {pinned}"
    );
    let goal = src
        .split("fn apply_thread_goal")
        .nth(1)
        .and_then(|s| s.split("fn spawn_thread_goal").next())
        .expect("apply_thread_goal");
    assert!(
        goal.contains("accessed_ms"),
        "auto-title must bump accessed_ms or /sync LWW can drop the new name: {goal}"
    );
    assert!(
        goal.contains("self.persist()") && !goal.contains("threads::save"),
        "auto-title must not freeze the cabin writing threads.json: {goal}"
    );
    let spawn_goal = src
        .split("fn spawn_thread_goal_on(")
        .nth(1)
        .and_then(|s| s.split("fn refresh_chips(").next())
        .expect("spawn_thread_goal_on");
    assert!(
        spawn_goal.contains("visible_turn_count") || spawn_goal.contains("is_workload_user"),
        "auto-title must ignore HOST_RESULT or a Command-pane job names the thread: {spawn_goal}"
    );
    assert!(
        spawn_goal.contains("chip_chat_pairs") || spawn_goal.contains("chip_scan"),
        "auto-title must not clone an 8MB complete into chat_pairs: {spawn_goal}"
    );
    let created = src
        .split("fn new_thread")
        .nth(1)
        .and_then(|s| s.split("fn begin_chat_rename").next())
        .expect("new_thread");
    assert!(
        created.contains("flush_visible_goal"),
        "/new must persist the left tab's goal before clearing it: {created}"
    );
    assert!(
        created.contains("drop_leaving_thread_chrome"),
        "/new must drop plus-attach, followup budget, and skill follow: {created}"
    );
    assert!(
        created.contains("reuse_empty_thread_idx"),
        "/new must reuse an empty Chat instead of stacking leftover tabs: {created}"
    );
    assert!(
        created.contains("grok_session = None"),
        "New chat must forget the last ACP session id so the next send is session/new: {created}"
    );
    assert!(
            created.contains("grok_cwd = None"),
            "New chat must forget the last worktree or the next send session/new stays in a History tree: {created}"
        );
    assert!(
        created.contains("self.persist()"),
        "forgetting the ACP session on New chat must hit disk or restart reloads Chat 1: {created}"
    );
    assert!(
        created.contains("composer_want_focus = true"),
        "New chat must put the cursor in the composer: {created}"
    );
    assert!(
            created.contains("apply_switch_thread") && !created.contains("self.switch_thread("),
            "/new reuse must not clone every thread twice — switch without persist_bg, then persist once: {created}"
        );
    assert!(
        created.contains("self.messages.clone()") && created.contains("Arc::new"),
        "/new must share the leaving pane Arc, not clone an 8MB HOST_RESULT: {created}"
    );
    let boot = src
        .split("pub fn new(hidden: bool)")
        .nth(1)
        .and_then(|s| s.split("fn apply_saved_geom(").next())
        .expect("Cabin::new");
    assert!(
        boot.contains("ensure_memory_seeds") && boot.contains("default_device_name"),
        "first run must seed Memory files and a device name: {boot}"
    );
    assert!(
            boot.contains("keep_loaded_thread"),
            "boot must drop leftover empty Chat tabs and keep a chat that still has a Grok session: {boot}"
        );
    assert!(
        !boot.contains("threads::save"),
        "boot leftover drop must not freeze the cabin writing threads.json: {boot}"
    );
    assert!(
        boot.contains("persist_bg"),
        "boot leftover drop must persist off-thread or restart restores empty Chat tabs: {boot}"
    );
    assert!(
        src.contains("leftover_empty_thread"),
        "History/boot must hide leftover empty Chat rows"
    );
    let switched = src
        .split("fn switch_thread")
        .nth(1)
        .and_then(|s| s.split("fn open_recent_chat").next())
        .expect("switch_thread");
    assert!(
        switched.contains("drop_leaving_thread_chrome"),
        "switching tabs must not send the previous tab's image or skill follow: {switched}"
    );
    assert!(
        switched.contains("persist_bg") && !switched.contains("self.persist()"),
        "tab switch must not freeze the cabin writing threads.json: {switched}"
    );
    assert!(
        switched.contains("apply_switch_thread"),
        "tab switch persist_bg must share the pane swap with /new reuse: {switched}"
    );
    assert!(
        switched.contains("composer_want_focus = true"),
        "opening a sidebar chat must put the cursor in the composer: {switched}"
    );
    assert!(
        switched.contains("self.messages.clone()")
            && switched.contains("live_mut")
            && switched.contains("Arc::make_mut"),
        "tab switch must share the parked pane Arc; first mutation copy-on-writes: {switched}"
    );
    let chrome = src
        .split("fn drop_leaving_thread_chrome")
        .nth(1)
        .and_then(|s| s.split("fn pick_entries").next())
        .expect("drop_leaving_thread_chrome");
    assert!(
        chrome.contains("hands_attach = false") && chrome.contains("eyes_attach = false"),
        "leaving a tab must not leave windshield/hands armed for the next tab: {chrome}"
    );
    assert!(
            chrome.contains("self.acp = None") && chrome.contains("tool_cards.clear()"),
            "leaving a tab must drop the ACP handle so New chat does not reuse the last session: {chrome}"
        );
    assert!(
            chrome.contains("halt_in_flight"),
            "dropping the ACP handle on tab switch must halt or the cabin stays Thinking on Chat 2: {chrome}"
        );
    assert!(
        chrome.contains("last_receipt_ok = None"),
        "leaving a tab must not put the next composer into the other tab's error chips: {chrome}"
    );
    let deleted = fn_src(&src, "delete_thread_at");
    assert!(
        deleted.contains("drop_leaving_thread_chrome"),
        "deleting the visible tab must drop plus-attach and followup budget: {deleted}"
    );
    let verify = src
        .split("fn run_skill_verify")
        .nth(1)
        .and_then(|s| s.split("fn replay_saved_recipe").next())
        .expect("run_skill_verify");
    let pushed = verify.find("push_bound_msg").expect("VERIFY_RESULT push");
    let saved = verify.find("self.persist()").expect("verify persist");
    assert!(
        pushed < saved,
        "VERIFY_RESULT must hit disk or a restart drops it: {verify}"
    );
    assert!(
        verify.contains("host_working_dir") && verify.contains("run_verify"),
        "skill verify must run in the bound project, not the cabin cwd: {verify}"
    );
    let spawn = verify
        .find("thread::spawn")
        .expect("skill verify must leave the UI thread");
    let run = verify.find("run_verify").expect("run_verify");
    assert!(
        spawn < run,
        "HostDone verify must not block the cabin for 12s: {verify}"
    );
    let kick = fn_src(&src, "kick_model");
    assert!(
        kick.contains("verify_rx"),
        "kick_model must wait for off-thread verify before the follow-up turn: {kick}"
    );
    assert!(
        !kick.contains("t.messages.clone()") && !kick.contains("kick_messages_for_job"),
        "kick_model must not clone the transcript to read the last user turn: {kick}"
    );
    let reflect = src
        .split("fn run_reflect")
        .nth(1)
        .and_then(|s| s.split("fn run_skill_verify").next())
        .expect("run_reflect");
    assert!(
        reflect.contains("kick_messages_for_job")
            || (reflect.contains("chat_job_thread") && reflect.contains("self.threads")),
        "/learn reflect must read the origin thread, not only the visible tab: {reflect}"
    );
    assert!(
        !reflect.contains("t.messages.clone()") && !reflect.contains("content.clone()"),
        "/learn reflect must not clone an 8MB transcript to harvest facts: {reflect}"
    );
    let mem = reflect
        .find("read_memory(\"MEMORY.md\")")
        .expect("reflect memory");
    assert!(
        reflect[..mem].contains("write_memory") && reflect[..mem].contains("mem_body"),
        "/learn reflect must flush the Memory editor before surgical edit: {reflect}"
    );
    let mem_spawn = reflect[..mem]
        .rfind("thread::spawn")
        .expect("reflect memory must leave the UI thread");
    let flush = reflect[..mem].find("write_memory").expect("reflect flush");
    assert!(
        mem_spawn > flush && reflect.contains("reflect_rx"),
        "idle reflect must not freeze the cabin slurping MEMORY.md: {reflect}"
    );
    let insights = reflect.find("extract_insights").expect("reflect insights");
    assert!(
        reflect[insights..].contains("save_learning")
            && reflect[insights..].contains("persist_io")
            && !reflect[insights..].contains("persist_snap")
            && !reflect[insights..].contains("self.persist()"),
        "/learn reflect must persist insights without cloning every thread: {reflect}"
    );
    let impl_src = src.as_str();
    assert!(
        !impl_src.contains("fn take_over_desktop")
            && !impl_src.contains("white_pill(ui, \"Take over\")"),
        "Desk Take over is gone — Grok Build computer-use runs from chat"
    );
    assert!(
        !crate::theme::CABIN_MENU.iter().any(|(id, _)| *id == "eyes"),
        "Desk must not sit in the cabin menu"
    );
    assert!(
        !crate::theme::CABIN_MENU
            .iter()
            .any(|(id, _)| *id == "command"),
        "Command is not a cabin menu row"
    );
    for gone in ["history", "workboard", "memory", "devices", "queue"] {
        assert!(
            !crate::theme::CABIN_MENU.iter().any(|(id, _)| *id == gone),
            "{gone} must not sit in the avatar menu"
        );
    }
    let menu = fn_src(&src, "ui_settings_menu");
    assert!(
        menu.contains("CABIN_MENU")
            && menu.contains("\"Help\"")
            && menu.contains("\"Sign out\"")
            && menu.contains("\"Connect Grok\""),
        "avatar menu must keep Settings, Help, and Sign in / Sign out: {menu}"
    );
    assert!(
        !menu.contains("\"History\"")
            && !menu.contains("\"Workboard\"")
            && !menu.contains("\"Memory\"")
            && !menu.contains("\"Devices\"")
            && !menu.contains("\"Queue\""),
        "avatar menu must not hardcode leftover panes: {menu}"
    );
    assert!(
        menu.contains("chrome.name")
            && menu.contains("chrome.picture_path")
            && !menu.contains("email"),
        "avatar menu paints the saved name and picture path, not the email: {menu}"
    );
    let replay = src
        .split("fn replay_recipe(")
        .nth(1)
        .and_then(|s| s.split("fn speak_reply").next())
        .expect("replay_recipe");
    assert!(
        replay.contains("run_cmds") && !replay.contains("run_computer_op("),
        "recipe replay must use host gates, not raw desktop ops: {replay}"
    );
    assert!(
        replay.contains("self.running"),
        "recipe replay must report whether host actually started: {replay}"
    );
    let reshoot = replay.split("ReplayOp::Reshoot").nth(1).expect("reshoot");
    assert!(
        reshoot.contains("lock_blocks_hands"),
        "recipe reshoot must not capture a lock screen: {reshoot}"
    );
    assert!(
        reshoot.contains("lock_titles"),
        "recipe reshoot must see lock windows that collect_rows drops: {reshoot}"
    );
    let saved_replay = src
        .split("fn replay_saved_recipe")
        .nth(1)
        .and_then(|s| s.split("fn replay_recipe(").next())
        .expect("replay_saved_recipe");
    assert!(
        saved_replay.contains("self.replay_recipe()") && !saved_replay.contains("true"),
        "night must not count a blocked recipe replay as started: {saved_replay}"
    );
    let send_auth = src
        .split("fn send_chat")
        .nth(1)
        .and_then(|s| s.split("fn send_followup_turn").next())
        .expect("send_chat auth");
    assert!(
        send_auth.contains("persist_user_turn(self.agent_ready())")
            && send_auth.contains("kick_model(true)"),
        "typed send must check the native sign-in, then kick the turn: {send_auth}"
    );
    let gate = send_auth.find("persist_user_turn").expect("send auth");
    assert!(
        send_auth[gate..].contains("hands_attach = false")
            && send_auth[gate..].contains("eyes_attach = false"),
        "auth-fail send must disarm leftover take-over flags: {send_auth}"
    );
    assert!(
        send_auth[gate..].contains("speak_next = false"),
        "auth-fail send must not leave TTS armed for the next reply: {send_auth}"
    );
    let last_user = src
        .split("fn last_user_on_job")
        .nth(1)
        .and_then(|s| s.split("fn commit_proposed_skill").next())
        .expect("last_user_on_job");
    assert!(
        (last_user.contains("last_user_for_job") || last_user.contains("last_user_scan"))
            && last_user.contains("self.threads")
            && !last_user.contains("t.messages.clone()"),
        "skill draft after host must not clone every thread: {last_user}"
    );
    assert!(
        !last_user.contains("content.clone()"),
        "skill draft after host must not clone an 8MB complete to read the last user: {last_user}"
    );
    let halt_flight = src
        .split("fn halt_in_flight")
        .nth(1)
        .and_then(|s| s.split("fn apply_assistant_snapshot").next())
        .expect("halt_in_flight");
    assert!(
        halt_flight.contains("speak_next = false"),
        "Stop must cancel a pending voice speak: {halt_flight}"
    );
    assert!(
        halt_flight.contains("scheduled_perm = false"),
        "Stop must drop scheduled_perm so the next typed Ask uses ACP: {halt_flight}"
    );
    assert!(
        halt_flight.contains("perm_ask = None"),
        "Stop must drop the permission bar or Allow continues the cancelled turn: {halt_flight}"
    );
    assert!(
        halt_flight.contains("answer_permission"),
        "Stop must deny leftover Ask or the next send hangs on the unanswered RPC: {halt_flight}"
    );
    assert!(
        halt_flight.contains("h.cancel()")
            && !halt_flight.contains("kill_pid")
            && !halt_flight.contains("grok_p_pid"),
        "Stop cancels the native turn; there is no grok -p child to kill: {halt_flight}"
    );
    assert!(
            halt_flight.contains("try_recv"),
            "Stop must drain leftover ACP tokens so they do not paint on the next prompt: {halt_flight}"
        );
    let halt_persist = halt_flight.find("self.persist()").expect("halt persist");
    assert!(
        halt_flight[..halt_persist].contains("accessed_ms")
            || halt_flight[..halt_persist].contains("stamp_current_access"),
        "halt must bump accessed_ms or /sync LWW can restore the dropped assistant: {halt_flight}"
    );
    assert!(
        halt_flight.contains("stamp_current_access") && halt_flight.contains("accessed_ms"),
        "halt must stamp the origin thread when it is not the visible tab: {halt_flight}"
    );
    assert!(
        !halt_flight.contains("t.messages.clone()") && !halt_flight.contains("content.clone()"),
        "Stop must not clone an 8MB transcript to drop one trailing assistant: {halt_flight}"
    );
    assert!(
        halt_flight.contains("imagine_pending = false"),
        "Stop must clear Imagine pending or a later job error paints on the stage: {halt_flight}"
    );
    let host_done_facts = src
        .split("Ok(JobOut::HostDone(block))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::Connector").next())
        .expect("HostDone facts");
    assert!(host_done_facts.contains("HOST_DIFF:"), "HOST_DIFF push");
    assert!(
        host_done_facts.contains("resolve_host_cite_path"),
        "HOST_DIFF must read the write from the bound tree, not the cabin cwd: {host_done_facts}"
    );
    let after_cite = host_done_facts.find("bump_usage").expect("host usage");
    let diff_spawn = host_done_facts[after_cite..]
        .find("thread::spawn")
        .expect("HOST_DIFF worker")
        + after_cite;
    let diff_read = host_done_facts[after_cite..]
        .find("read_text_capped")
        .expect("HOST_DIFF read")
        + after_cite;
    assert!(
        diff_spawn < diff_read && !host_done_facts.contains("read_to_string"),
        "HOST_DIFF must not slurp a huge host write on the UI thread: {host_done_facts}"
    );
    let host_diff_poll = src
        .split("fn poll_host_diff(")
        .nth(1)
        .and_then(|s| s.split("fn finish_host_diff_kick(").next())
        .expect("poll_host_diff");
    assert!(
        host_diff_poll.contains("HOST_DIFF") || host_diff_poll.contains("push_bound_msg"),
        "HOST_DIFF must land in the transcript: {host_diff_poll}"
    );
    assert!(
        host_diff_poll.contains("self.persist()"),
        "HOST_DIFF must persist after the cite push: {host_diff_poll}"
    );
    let deleted = src
        .split("fn delete_thread_at")
        .nth(1)
        .and_then(|s| s.split("fn send_chat").next())
        .expect("delete_thread_at");
    assert!(
        deleted.contains("goal.step") && deleted.contains("self.goal_step"),
        "deleting the visible tab must adopt the next tab's goal step: {deleted}"
    );
    assert!(
        src.contains("let goal_step = threads.get(thread_idx)"),
        "boot must restore the current thread's goal step, not always 0"
    );
    let skills_ui = src
        .split("fn ui_skills")
        .nth(1)
        .and_then(|s| s.split("fn project_row_active(").next())
        .expect("ui_skills");
    assert!(
        skills_ui.contains("ensure_native_listing") && !skills_ui.contains("reload_grok_catalog"),
        "Skills reads the native engine's listing, never grok inspect: {skills_ui}"
    );
    assert!(
            skills_ui.contains("Cabin skills") && skills_ui.contains("self.skill_list"),
            "the skills the cabin follows and writes must be listed, not only the Grok catalog: {skills_ui}"
        );
    assert!(
        skills_ui.contains("skills::list_skills()"),
        "Refresh must re-read ~/.config/GrokHub/skills, not just the Grok catalog: {skills_ui}"
    );
    assert!(
        skills_ui.contains("skill_use_in_chat_prompt"),
        "Use in chat must send a Grok skill slash, not Follow skill: {skills_ui}"
    );
    let connectors = fn_src(include_str!("connectors_ui.rs"), "ui_settings_connectors");
    assert!(
        connectors.contains("ConnectorsTab::Marketplace") && connectors.contains("ui_marketplace"),
        "Settings, Connectors must show the cabin's MCP servers and the marketplace: {connectors}"
    );
    assert!(
        !src.contains("fn run_grok_user_cmd") && !src.contains("fn reload_grok_catalog"),
        "install and enable never run grok"
    );
    let skill_slash = src
        .split("Slash::Skill(name)")
        .nth(1)
        .and_then(|s| s.split("Slash::LearnReflect").next())
        .expect("Skill slash");
    assert!(
        skill_slash.contains("skill_use_in_chat_prompt") && skill_slash.contains("send_chat"),
        "/skill must run the skill, not only open the editor: {skill_slash}"
    );
    let send = src
        .split("fn send_chat")
        .nth(1)
        .and_then(|s| s.split("fn send_followup_turn").next())
        .expect("send_chat attach");
    assert!(
        send.contains("kick_model(true)"),
        "typed send must consume the plus-button image: {send}"
    );
    let retry = src
        .split("fn kick_model_retry")
        .nth(1)
        .and_then(|s| s.split("fn policy(").next())
        .expect("kick_model_retry");
    assert!(
        retry.contains("match_skill") && retry.contains("skill_follow_block"),
        "/retry must re-inject the skill follow that halt_in_flight cleared: {retry}"
    );
    let host_done = src
        .split("Ok(JobOut::HostDone(block))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::Connector").next())
        .expect("HostDone attach");
    assert!(
        host_done.contains("kick_model(false)"),
        "HostDone must not steal the attached image: {host_done}"
    );
    assert!(
        host_done.contains("watch_once")
            && host_done.contains("teachable_steps")
            && !host_done.contains("save_schedule"),
        "watching a host run must not save a job or the internal rewind snapshot: {host_done}"
    );
    let pushed = host_done.find("push_bound_msg").expect("host result");
    let recipe = host_done.find("save_recipe").expect("host recipe");
    let saved = host_done.find("self.persist()").expect("host persist");
    assert!(
        pushed < saved && saved < recipe,
        "HOST_RESULT must hit disk before recipe/skill side effects: {host_done}"
    );
    let bump = host_done.find("bump_usage").expect("host usage persist");
    assert!(
        bump < saved,
        "HostDone must persist host usage or persist_bg skips the bump: {host_done}"
    );
    assert_eq!(
        host_done.matches("self.persist()").count(),
        1,
        "HostDone must not clone every thread twice: {host_done}"
    );
    assert!(
        host_done.contains("lock_titles"),
        "HostDone capture must see lock windows that collect_rows drops: {host_done}"
    );
    assert!(
        host_done.contains("eyes_attach = true") && host_done.contains("hands_attach = true"),
        "after COMPUTER_CMD, HostDone must re-arm eyes and hands for the next shot: {host_done}"
    );
    let host_scan = host_done.find("collect_rows").expect("HostDone desk scan");
    let host_spawn = host_done
        .find("thread::spawn")
        .expect("HostDone desk scan worker");
    assert!(
        host_spawn < host_scan,
        "HostDone AT-SPI must not freeze the cabin: {host_done}"
    );
    let import = src
        .split("fn import_openclaw")
        .nth(1)
        .and_then(|s| s.split("fn run_consult").next())
        .expect("import_openclaw");
    assert!(
        import.contains("merge_imported_memory"),
        "/import must merge MEMORY.md instead of last-file-wins: {import}"
    );
    assert!(
        import.contains("mem_name") && import.contains("mem_body"),
        "/import must reload the Memory editor onto MEMORY.md: {import}"
    );
    let merge_read = import
        .find("read_memory(\"MEMORY.md\")")
        .expect("import memory");
    assert!(
        import[..merge_read].contains("write_memory") && import[..merge_read].contains("mem_body"),
        "/import must flush the Memory editor before merging MEMORY.md: {import}"
    );
    let dest_arm = import
        .split("import_memory_file")
        .nth(1)
        .expect("import files");
    let dest_write = dest_arm
        .find("write_memory(&dest")
        .expect("import dest write");
    assert!(
        dest_arm[..dest_write].contains("read_memory(&dest)"),
        "/import must not rotate .prev when SOUL/USER already match disk: {dest_arm}"
    );
    let after_loop = import
        .split("if imported > 0")
        .nth(1)
        .expect("import persist memory");
    assert!(
        after_loop.contains("read_memory(\"MEMORY.md\")")
            && after_loop.contains("write_memory(\"MEMORY.md\""),
        "/import must not rotate MEMORY.md.prev when the merge is unchanged: {after_loop}"
    );
    assert!(
        import.contains("read_text_capped") && !import.contains("read_to_string"),
        "/import must not slurp huge OpenClaw files on the UI thread: {import}"
    );
    let import_spawn = import
        .find("thread::spawn")
        .expect("import must leave the UI thread");
    let import_walk = import.find("read_dir").expect("import walks OpenClaw");
    assert!(
        import_spawn < import_walk,
        "/import must not walk OpenClaw on the UI thread: {import}"
    );
    let sign_out = src
        .split("fn sign_out_oauth")
        .nth(1)
        .and_then(|s| s.split("fn poll_oauth_photo").next())
        .expect("sign_out_oauth");
    assert!(
        sign_out.contains("oauth_pending = None"),
        "Sign out during device-code poll must not reconnect when the browser finishes: {sign_out}"
    );
    assert!(
        sign_out.contains("oauth_start_rx") && sign_out.contains("oauth_poll_rx"),
        "Sign out must drop in-flight OAuth HTTP: {sign_out}"
    );
    assert!(
        sign_out.contains("persist_io") && sign_out.contains("secrets::save"),
        "Sign out must not freeze the cabin writing secrets.json: {sign_out}"
    );
    assert!(
        sign_out.contains("imagine_pending = false"),
        "Sign out must clear Imagine pending or a later job error paints on the stage: {sign_out}"
    );
    assert!(
        !sign_out.contains("auth.json"),
        "Sign out must not wipe ~/.grok/auth.json: {sign_out}"
    );
    let start_o = src
        .split("fn start_oauth(")
        .nth(1)
        .and_then(|s| s.split("fn poll_oauth(").next())
        .expect("start_oauth");
    assert!(
        start_o.contains("oauth_pending") && start_o.contains("oauth_poll_rx"),
        "Connect must not restart an in-flight device-code wait: {start_o}"
    );
    let start_spawn = start_o.find("thread::spawn").expect("start_oauth spawn");
    let start_dev = start_o.find("start_device").expect("start_device");
    assert!(
        start_spawn < start_dev,
        "Connect Grok OAuth must not freeze the cabin on device-code HTTP: {start_o}"
    );
    let poll_o = src
        .split("fn poll_oauth(")
        .nth(1)
        .and_then(|s| s.split("fn clear_oauth_photo(").next())
        .expect("poll_oauth");
    let poll_spawn = poll_o.find("thread::spawn").expect("poll_oauth spawn");
    let poll_dev = poll_o.find("poll_device").expect("poll_device");
    assert!(
        poll_spawn < poll_dev,
        "OAuth poll must not freeze the cabin on token HTTP: {poll_o}"
    );
    assert!(
        poll_o.contains("persist_io") && poll_o.contains("secrets::save"),
        "OAuth Ready must not freeze the cabin writing secrets.json: {poll_o}"
    );
    let ready = src
        .split("PollStatus::Ready")
        .nth(1)
        .and_then(|s| s.split("PollStatus::Expired").next())
        .expect("oauth ready");
    assert!(
        !ready.contains("write_cli_auth_if_needed")
            && !ready.contains("sync_cli_auth_from_oauth")
            && (ready.contains("get_started_done = true")
                || ready.contains("mark_get_started_done")),
        "Connect Grok signs GrokHub in and never writes the CLI's auth.json: {ready}"
    );
    let boot = src
        .split("pub fn new(hidden: bool)")
        .nth(1)
        .and_then(|s| s.split("fn apply_saved_geom(").next())
        .expect("Cabin::new");
    assert!(
        !boot.contains("write_cli_auth_if_needed")
            && !boot.contains("sync_cli_auth_from_oauth")
            && !boot.contains("begin_ensure_grok_alpha")
            && !boot.contains("grok_cli_key")
            && !boot.contains("official_cli_session")
            && boot.contains("silence_windows_hard_errors"),
        "boot never installs, signs in, or reads the Grok Build CLI: {boot}"
    );
    let started = src
        .split("fn ui_get_started(")
        .nth(1)
        .and_then(|s| s.split("fn ui_settings(").next())
        .expect("ui_get_started");
    assert!(
        src.contains("if !self.ui_get_started(ui)"),
        "first-run sheet must replace empty-cabin chat so it paints on Windows"
    );
    let paint = src
        .split("self.ui_titlebar(ui);")
        .nth(1)
        .and_then(|s| s.split("if self.palette_open").next())
        .expect("titlebar then panes");
    let settings_nav = paint
        .find("if self.nav == Nav::Settings")
        .expect("Settings after Get Started skip");
    let skip = paint
        .find("if !self.ui_get_started(ui)")
        .expect("get started skip");
    assert!(
        settings_nav > skip
            && paint[skip..settings_nav].contains("ui_sidebar")
            && !paint[settings_nav..].contains("ui_sidebar"),
        "Latest chip must open Settings on top of Get Started: {paint}"
    );
    assert!(
        started.contains("get_started_oauth_error"),
        "Get Started must not paint leftover wall/install status as an OAuth error: {started}"
    );
    let poll = src
        .split("fn poll_oauth(")
        .nth(1)
        .and_then(|s| s.split("fn clear_oauth_photo(").next())
        .expect("poll_oauth");
    assert!(
        poll.matches("oauth_error_status").count() >= 3
            && poll.contains("PollStatus::Expired")
            && poll.contains("PollStatus::Denied"),
        "Get Started must show live device-code start/poll/deny failures: {poll}"
    );
    assert!(
        started.contains("egui::CentralPanel::default()"),
        "Get Started must paint in CentralPanel, not a first-frame Area: {started}"
    );
    assert!(
        !started.contains("egui::Area::new"),
        "Windows first-run Area over chat does not paint: {started}"
    );
    assert!(
        started.contains("ui_wants_get_started") && started.contains("start_oauth"),
        "Get Started must use cabin device-code OAuth: {started}"
    );
    assert!(
        started.contains("oauth_err") && started.contains("oauth_busy"),
        "Get Started must show OAuth errors and not restart an in-flight wait: {started}"
    );
    assert!(
        !started.contains("Installing Grok Build CLI")
            && !started.contains("queue_grok_cli_install")
            && !started.contains("official_cli_session")
            && !started.contains("should_show_cli_install_wait"),
        "Get Started never installs or waits on the Grok Build CLI: {started}"
    );
    let photo = src
        .split("fn kick_oauth_photo(")
        .nth(1)
        .and_then(|s| s.split("fn kick_model(").next())
        .expect("kick_oauth_photo");
    let photo_spawn = photo
        .find("thread::spawn")
        .expect("avatar fetch must leave the UI thread");
    let photo_decode = photo.find("oauth_photo_image").expect("avatar JPEG decode");
    assert!(
        photo_spawn < photo_decode,
        "OAuth avatar JPEG must not decode on the UI thread: {photo}"
    );
    let photo_poll = src
        .split("fn poll_oauth_photo(")
        .nth(1)
        .and_then(|s| s.split("fn kick_oauth_photo(").next())
        .expect("poll_oauth_photo");
    assert!(
        photo_poll.contains("persist_io") && photo_poll.contains("secrets::save"),
        "OAuth profile enrich must not freeze the cabin writing secrets.json: {photo_poll}"
    );
    let kick = fn_src(&src, "kick_model");
    assert!(
        kick.contains("kick_native_turn") && !kick.contains("spawn_grok_p_stream"),
        "kick_model runs the native turn, never grok -p: {kick}"
    );
    let cap_fn = src
        .split("fn capture_cabin_frame_this_turn")
        .nth(1)
        .and_then(|s| s.split("fn apply_job_fail").next())
        .expect("capture_cabin_frame_this_turn");
    assert!(
        cap_fn.contains("lock_titles"),
        "leftover capture helper must still see lock windows: {cap_fn}"
    );
    let anticipate = fn_src(&src, "tick_anticipate");
    let bump = anticipate.find("bump_usage").expect("anticipate usage");
    let gate = anticipate
        .find("anticipate_consumes_slot")
        .expect("anticipate auth");
    assert!(
        gate < bump,
        "anticipate must not burn quota before auth: {anticipate}"
    );
    assert!(
        anticipate.contains("scratch()"),
        "anticipate must not burn a slot on Scratch: {anticipate}"
    );
    assert!(
        anticipate.contains("anticipate_consumes_slot(self.agent_ready())")
            && !anticipate.contains("self.llm_ready()"),
        "a signed-out cabin must not burn an anticipate slot: {anticipate}"
    );
    let start_hub = format!(
        "{}{}",
        fn_src(&src, "start_hub"),
        fn_src(&src, "bind_lan_hub")
    );
    assert!(
        start_hub.contains("start_hub_rotates_pair"),
        "Start share must rotate an expired leftover code: {start_hub}"
    );
    let start_err = start_hub.split("Err(e)").nth(1).expect("start hub err");
    assert!(
        start_err.contains("sharing = false"),
        "Start share must not leave sharing on when serve_lan fails: {start_hub}"
    );
    assert!(
        start_hub.contains("lan_bind_in_use")
            && start_hub.contains("grokhub-hub.service")
            && start_hub.contains("serve_lan"),
        "Start share must take :18766 from grokhub-hub.service and retry serve_lan: {start_hub}"
    );
    assert!(
        start_hub.contains("self.persist_hub()") && !start_hub.contains("persist_snap"),
        "Start share must not clone every thread just to stamp sharing: {start_hub}"
    );
    let live = src
        .split("fn live_room")
        .nth(1)
        .and_then(|s| s.split("fn tick_mid_thought").next())
        .expect("live_room");
    let title = live.find("last_window_title").expect("live title");
    let gate = live.find("should_send_screenshot").expect("live gate");
    assert!(
        live.contains("collect_rows") && title < gate,
        "presence stream must refresh the foreground title before sending a frame: {live}"
    );
    assert!(
        live.contains("lock_titles"),
        "presence stream must see lock windows that collect_rows drops: {live}"
    );
    let devices = src
        .split("fn ui_devices")
        .nth(1)
        .and_then(|s| s.split("fn ui_memory").next())
        .expect("ui_devices");
    assert!(
        devices.contains("pair_code_is_live") && devices.contains("devices_shows_pair_code"),
        "Devices must hide a dead pair code and any code while the hub is off: {devices}"
    );
    let new_code = devices
        .split("New code")
        .nth(1)
        .and_then(|s| s.split("empty_prompt_tile").next())
        .expect("new code");
    let rotated = new_code.find("rotate_pair").expect("rotate");
    assert!(
        new_code[rotated..].contains("self.persist_hub()"),
        "New code must persist the rotated pair before a restart: {new_code}"
    );
    let expired = devices
        .split("rotate_pair")
        .nth(1)
        .and_then(|s| s.split("New code").next())
        .expect("expired rotate");
    assert!(
        expired.contains("self.persist_hub()"),
        "an expired pair rotate must persist or restart shows the dead code: {expired}"
    );
    let clear = src
        .split("Slash::Clear =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Undo =>").next())
        .expect("Clear");
    assert!(
        clear.contains("halt_in_flight"),
        "/clear during a job must halt or the stream refills the pane: {clear}"
    );
    assert!(
        clear.contains("followup_step = 0") && clear.contains("active_skill_follow = None"),
        "/clear must reset followup budget and skill follow with the pane: {clear}"
    );
    assert!(
        clear.contains("stamp_current_access") || clear.contains("accessed_ms"),
        "/clear must bump accessed_ms or /sync LWW can restore the cleared turns: {clear}"
    );
    assert!(
        clear.contains("drop_leaving_thread_chrome") && clear.contains("grok_session = None"),
        "/clear must drop ACP and forget the session id or the next send loads Chat 1: {clear}"
    );
    assert!(
            clear.contains("grok_cwd = None"),
            "/clear must forget the worktree or the next send session/new stays in a History tree: {clear}"
        );
    let help = src
        .split("Slash::Help =>")
        .nth(1)
        .and_then(|s| s.split("Slash::New =>").next())
        .expect("Help");
    assert!(
        help.contains("stamp_current_access") || help.contains("accessed_ms"),
        "/help must bump accessed_ms or /sync LWW can drop the help turn: {help}"
    );
    assert!(
        help.contains("mark_slash_result") || help.contains("SLASH_RESULT"),
        "/help must not become the next model turn: {help}"
    );
    let models = src
        .split("Slash::Models =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Palette =>").next())
        .expect("Models");
    assert!(
        models.contains("stamp_current_access") || models.contains("accessed_ms"),
        "/models must bump accessed_ms or /sync LWW can drop the catalog turn: {models}"
    );
    assert!(
        models.contains("mark_slash_result") || models.contains("SLASH_RESULT"),
        "/models must not become the next model turn: {models}"
    );
    let undo = src
        .split("Slash::Undo =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Retry =>").next())
        .expect("Undo");
    assert!(
            undo.contains("halt_in_flight")
                && undo.contains("REWIND_NA")
                && !undo.contains("live_mut().remove")
                && !undo.contains("send_grok_slash"),
            "/undo stops a live reply; a finished one stays, since the native session is append-only: {undo}"
        );
    let forget = src
        .split("Slash::Forget")
        .nth(1)
        .and_then(|s| s.split("Slash::MemoryShow").next())
        .expect("Forget");
    assert!(
        forget.contains("self.scratch()") && forget.contains("no memory writes"),
        "/forget on Scratch must not wipe MEMORY.md: {forget}"
    );
    let note = src
        .split("Slash::MemoryNote")
        .nth(1)
        .and_then(|s| s.split("Slash::Board").next())
        .expect("MemoryNote");
    let append = note.find("append_memory").expect("append");
    assert!(
        note[..append].contains("write_memory") && note[..append].contains("mem_body"),
        "/remember must flush the Memory editor before appending to disk: {note}"
    );
    let append_spawn = note
        .find("thread::spawn")
        .expect("remember append must leave the UI thread");
    assert!(
        append_spawn < append,
        "/remember must not freeze the cabin appending MEMORY.md: {note}"
    );
    let topic = forget.split("Some(q)").nth(1).expect("forget topic");
    let read = topic
        .find("read_memory(\"MEMORY.md\")")
        .expect("forget read");
    assert!(
        topic[..read].contains("write_memory") && topic[..read].contains("mem_body"),
        "/forget topic must flush the Memory editor before editing disk: {topic}"
    );
    let forget_spawn = topic[..read]
        .rfind("thread::spawn")
        .expect("forget slurp must leave the UI thread");
    assert!(
        forget_spawn < read,
        "/forget topic must not freeze the cabin slurping MEMORY.md: {topic}"
    );
    let memory_ui = src
        .split("fn ui_memory")
        .nth(1)
        .and_then(|s| s.split("fn save_settings").next())
        .expect("ui_memory");
    assert!(
        memory_ui.contains("self.scratch()") && memory_ui.contains("no memory writes"),
        "Memory Save on Scratch must not write MEMORY.md: {memory_ui}"
    );
    let restore = memory_ui
        .split("ghost_pill(ui, \"Restore\")")
        .nth(1)
        .and_then(|s| s.split("Reflect").next())
        .expect("memory restore");
    let restore_spawn = restore
        .find("thread::spawn")
        .expect("restore must leave the UI thread");
    let restore_fn = restore.find("restore_memory").expect("restore_memory");
    assert!(
        restore_spawn < restore_fn,
        "Memory Restore must not freeze the cabin reading MEMORY.md.prev: {restore}"
    );
    assert!(
        memory_ui.contains("self.open_memory_file(name)"),
        "the Memory tabs and a History hit open a file the same way: {memory_ui}"
    );
    let tabs = src
        .split("fn open_memory_file(")
        .nth(1)
        .and_then(|s| s.split("fn ui_memory(").next())
        .expect("open_memory_file");
    assert!(
        tabs.contains("if self.mem_name == name") && tabs.contains("return;"),
        "re-opening the file already in the editor must keep unsaved edits: {tabs}"
    );
    let flush = tabs.find("write_memory").expect("flush leaving memory");
    let switch = tabs.find("mem_name = name").expect("switch name");
    assert!(
        flush < switch && tabs.contains("scratch()"),
        "Memory tab switch must flush the leaving file like thread switch: {tabs}"
    );
    assert!(
        tabs[..flush].contains("read_memory"),
        "Memory tab switch must not rotate .prev when the leaving file is unchanged: {tabs}"
    );
    assert!(
        tabs.contains("thread::spawn"),
        "Memory tab switch must flush off the UI thread: {tabs}"
    );
    assert!(
        tabs.contains("memory_updated_at")
            && tabs.contains("mem_file_rx")
            && tabs.contains("mem_cache_at"),
        "Memory tab switch must not slurp SOUL.md on every click after the first miss: {tabs}"
    );
    assert!(
        tabs.contains("read_memory(name)") && tabs.contains("mem_cache_at[i] == 0"),
        "first Memory tab miss must still read on-thread so tests stay deterministic: {tabs}"
    );
    let save = memory_ui
        .split("white_pill(ui, \"Save\")")
        .nth(1)
        .expect("memory save");
    let write = save.find("write_memory").expect("save write");
    assert!(
        save[..write].contains("read_memory") && save[..write].contains("mem_body"),
        "Memory Save must not rotate .prev when the file is unchanged: {save}"
    );
    assert!(
        save.contains("thread::spawn"),
        "Memory Save must not freeze the cabin writing MEMORY.md: {save}"
    );
    let settings_save = src
        .split("fn save_settings")
        .nth(1)
        .and_then(|s| s.split("fn ui_settings").next())
        .expect("save_settings");
    assert!(
        settings_save.contains("upsert_bound") && settings_save.contains("touch_projects"),
        "Settings Save must keep the sidebar selection on the bound path: {settings_save}"
    );
    let settings_spawn = settings_save
        .find("thread::spawn")
        .expect("settings mkdir must leave the UI thread");
    let settings_mkdir = settings_save
        .find("create_dir_all")
        .expect("create_dir_all");
    assert!(
        settings_spawn < settings_mkdir,
        "Settings Save must not freeze the cabin creating the bound folder: {settings_save}"
    );
    let hub_name = settings_save.find("device_name").expect("hub device name");
    let saved = settings_save
        .find("self.persist()")
        .expect("settings persist");
    assert!(
        hub_name < saved,
        "Settings Save must persist hub device_name or restart keeps the old name: {settings_save}"
    );
    let unbound = src
        .split("Slash::ProjectClear =>")
        .nth(1)
        .and_then(|s| s.split("Slash::ProjectShow =>").next())
        .expect("ProjectClear");
    assert!(
        unbound.contains("project_sel = None") && unbound.contains("touch_projects"),
        "/project clear must drop the sidebar selection: {unbound}"
    );
    let export = src
        .split("Slash::Export =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Recall").next())
        .expect("Export");
    let flushed = export.find("self.persist()").expect("export persist");
    let wrote = export.find("export_markdown").expect("export_markdown");
    assert!(
        flushed < wrote,
        "/export must flush the live pane before writing the thread file: {export}"
    );
    assert!(
        export.contains("expand_home"),
        "/export must expand ~ in the bound project or it writes a literal tilde folder: {export}"
    );
    let export_spawn = export
        .find("thread::spawn")
        .expect("export write must leave the UI thread");
    let export_write = export.find("fs::write").expect("export.md");
    assert!(
        export_spawn < export_write && export_spawn < wrote,
        "/export must not freeze the cabin formatting an 8MB thread into markdown: {export}"
    );
    let recall = src
        .split("Slash::Recall(q)")
        .nth(1)
        .and_then(|s| s.split("fn kick_model_retry").next())
        .expect("Recall");
    let mem = recall
        .find("read_memory(\"SOUL.md\")")
        .expect("recall soul");
    assert!(
        recall[..mem].contains("write_memory")
            && recall[..mem].contains("mem_body")
            && recall[..mem].contains("scratch()"),
        "/recall must flush the Memory editor before searching disk: {recall}"
    );
    assert!(
        recall[..mem].contains("thread::spawn"),
        "/recall must slurp SOUL/USER/MEMORY off the UI thread: {recall}"
    );
    assert!(
        recall.contains("learning") && recall.contains("(\"learned\", insights)"),
        "what the cabin learned by itself is memory too — /recall must search it: {recall}"
    );
    assert!(
        recall.contains("dedupe_hits") && !recall.contains("hits.sort()"),
        "sorting hits alphabetically buries the memory line under chat titles: {recall}"
    );
    let recall_poll = src
        .split("fn poll_recall(")
        .nth(1)
        .and_then(|s| s.split("fn poll_session_show").next())
        .expect("poll_recall");
    assert!(
        recall_poll.contains("stamp_current_access") || recall_poll.contains("accessed_ms"),
        "/recall must bump accessed_ms or /sync LWW can drop the recall turn: {recall_poll}"
    );
    assert!(
        recall_poll.contains("mark_slash_result") || recall_poll.contains("SLASH_RESULT"),
        "/recall must not become the next model turn: {recall_poll}"
    );
    let sync = src
        .split("fn sync_hub(&mut self)")
        .nth(1)
        .and_then(|s| s.split("fn local_clock").next())
        .expect("sync_hub");
    assert!(
        sync.contains("merge_hub_snapshots"),
        "/sync must merge the hub snapshot, not replace peer threads: {sync}"
    );
    let flushed = sync.find("persist_snap").expect("sync persist");
    let built = sync.find("build_hub_snapshot").expect("build snapshot");
    assert!(
        flushed < built,
        "/sync must flush the live pane before publishing threads: {sync}"
    );
    let merged = sync.find("st.snapshot =").expect("store merge");
    assert!(
        sync[merged..].contains("self.persist_hub()"),
        "/sync must persist the merged snapshot or a restart drops peer LWW: {sync}"
    );
    let poll = src
        .split("fn poll_sync(")
        .nth(1)
        .and_then(|s| s.split("fn date_out").next())
        .expect("poll_sync");
    assert!(
        poll.contains("persist_hub") && !poll.contains("persist_snap"),
        "/sync inbound must not clone every thread just to flush hub-state.json: {poll}"
    );
    let mem_write = sync.find("write_memory").expect("sync memory flush");
    assert!(
        mem_write < built && sync.contains("mem_body") && sync.contains("scratch()"),
        "/sync must flush the Memory editor before publishing files: {sync}"
    );
    let sync_spawn = sync.find("thread::spawn").expect("sync worker");
    let sync_read = sync.find("read_memory(n)").expect("sync memory slurp");
    assert!(
        sync_spawn < sync_read,
        "/sync must slurp SOUL/USER/MEMORY off the UI thread: {sync}"
    );
    assert!(
        sync.contains("submit_full") && sync.contains("persist_done"),
        "/sync must schedule PersistService before egress: {sync}"
    );
    let thread_rows = sync
        .split("let threads = snap")
        .nth(1)
        .and_then(|s| s.split("let skills = skills").next())
        .expect("sync threads");
    assert!(
        thread_rows.contains("accessed_ms") && !thread_rows.contains("now_ms()"),
        "/sync must not stamp every thread now or local stale data wins LWW: {thread_rows}"
    );
    assert!(
        !sync.contains("t.messages.clone()") && sync.contains("snap.clone()"),
        "/sync must reuse the persist snap instead of cloning every thread twice: {sync}"
    );
    let push = src
        .split("fn push_bound_msg")
        .nth(1)
        .and_then(|s| s.split("fn apply_live_assistant").next())
        .expect("push_bound_msg");
    assert!(
        push.contains("IMAGE_FILE_CAP") || push.contains("take_ui_text"),
        "host/consult receipts must not land a huge body in the transcript: {push}"
    );
    assert!(
        push.contains("accessed_ms"),
        "background job writes must bump accessed_ms or /sync LWW drops the new messages: {push}"
    );
    assert!(
        !push.contains("t.messages.clone()"),
        "host receipts must not clone every thread: {push}"
    );
    let snap = src
        .split("fn apply_assistant_snapshot")
        .nth(1)
        .and_then(|s| s.split("fn push_bound_msg").next())
        .expect("apply_assistant_snapshot");
    assert!(
            snap.contains("accessed_ms"),
            "background stream writes must bump accessed_ms or /sync LWW drops the new messages: {snap}"
        );
    assert!(
        !snap.contains("t.messages.clone()"),
        "stream deltas must not clone every thread: {snap}"
    );
    let mem_rows = sync
        .split("let mem = ")
        .nth(1)
        .and_then(|s| s.split("let mut snap = self.persist_snap").next())
        .expect("sync mem");
    assert!(
        mem_rows.contains("memory_updated_at") && !mem_rows.contains("now_ms()"),
        "/sync must not stamp MEMORY.md now or stale local wins LWW: {mem_rows}"
    );
    let skill_rows = sync
        .split("let skills = self")
        .nth(1)
        .and_then(|s| s.split("let autos = self").next())
        .expect("sync skills");
    assert!(
        skill_rows.contains("skill_updated_at") && !skill_rows.contains("now_ms()"),
        "/sync must not stamp every skill now or local stale data wins LWW: {skill_rows}"
    );
    let inbound = src
        .split("fn apply_inbound_snapshot")
        .nth(1)
        .and_then(|s| s.split("fn push_presence").next())
        .expect("apply_inbound_snapshot");
    assert!(
        inbound.contains("mem_body") && inbound.contains("mem_name"),
        "inbound MEMORY.md must refresh the open Memory editor: {inbound}"
    );
    assert!(
            inbound.contains("mem_cache_body") && inbound.contains("mem_file_idx"),
            "inbound MEMORY.md must refresh the Memory tab cache or a later click shows a stale slurp: {inbound}"
        );
    let wrote = inbound.find("write_memory").expect("inbound write");
    assert!(
        inbound[..wrote].contains("read_memory"),
        "inbound must not rotate .prev when the merged file is unchanged: {inbound}"
    );
    let inbound_spawn = inbound
        .find("thread::spawn")
        .expect("inbound write must leave the UI thread");
    assert!(
        inbound_spawn < wrote,
        "inbound MEMORY.md must not freeze the cabin writing markdown: {inbound}"
    );
    assert!(
        !inbound.contains("snapshot.clone()") && !inbound.contains("from_value"),
        "/sync inbound must not clone the hub snapshot on the UI thread: {inbound}"
    );
    let send = src
        .split("fn dispatch_send")
        .nth(1)
        .and_then(|s| s.split("fn sync_hub(&mut self)").next())
        .expect("dispatch_send");
    let queued = send.find("enqueue_local").expect("enqueue");
    assert!(
        send[queued..].contains("self.persist_hub()") && !send[queued..].contains("persist_snap"),
        "/send must persist a queued hub task without cloning every thread: {send}"
    );
    let inhabit = src
        .split("fn queue_inhabit")
        .nth(1)
        .and_then(|s| s.split("fn rewind_project").next())
        .expect("queue_inhabit");
    let staged = inhabit.find("inhabit = Some").expect("stage inhabit");
    assert!(
        inhabit[staged..].contains("self.persist_hub()") && !inhabit.contains("persist_snap"),
        "/inhabit must persist the staged bundle without cloning every thread: {inhabit}"
    );
    assert!(
        inhabit.contains("No paired peer named") && inhabit.contains("to_id"),
        "/inhabit must name a real peer: {inhabit}"
    );
    let soul = inhabit
        .find("read_memory(\"SOUL.md\")")
        .expect("inhabit soul");
    assert!(
        inhabit[..soul].contains("write_memory") && inhabit[..soul].contains("mem_body"),
        "/inhabit must flush the Memory editor before packing SOUL.md: {inhabit}"
    );
    let soul_spawn = inhabit[..soul]
        .rfind("thread::spawn")
        .expect("inhabit soul must leave the UI thread");
    let flush = inhabit[..soul].find("write_memory").expect("inhabit flush");
    assert!(
        soul_spawn > flush && inhabit.contains("inhabit_rx"),
        "/inhabit must not freeze the cabin packing a 1MB SOUL.md: {inhabit}"
    );
    let greet = src
        .split("fn refresh_greeting")
        .nth(1)
        .and_then(|s| s.split("fn spawn_greeting_llm").next())
        .expect("refresh_greeting");
    let user = greet.find("read_memory(\"USER.md\")").expect("greet user");
    assert!(
        greet[..user].contains("write_memory")
            && greet[..user].contains("mem_body")
            && greet[..user].contains("scratch()"),
        "empty-chat greeting must flush the Memory editor before reading USER/MEMORY: {greet}"
    );
    assert!(
        greet[..user].contains("memory_updated_at"),
        "empty-chat greeting must not slurp USER/MEMORY on every paint: {greet}"
    );
    assert!(
        greet.contains("greeting_files_rx") && greet.contains("greeting_user_at == 0"),
        "mtime-changed USER/MEMORY must leave the UI thread after the first miss: {greet}"
    );
    let greet_spawn = greet
        .find("thread::spawn")
        .expect("greeting flush must leave the UI thread");
    let greet_write = greet.find("write_memory").expect("greeting write_memory");
    assert!(
        greet_spawn < greet_write,
        "empty-chat greeting must not freeze the cabin writing MEMORY.md: {greet}"
    );
    assert!(
        !greet.contains("device_name"),
        "hostname must not paint as the empty-home greeting: {greet}"
    );
    assert!(
        greet.contains("as_str()")
            && !greet.contains("greeting_user_md.clone()")
            && !greet.contains("greeting_memory_md.clone()"),
        "empty-chat greeting must not clone USER/MEMORY every paint: {greet}"
    );
    assert!(
        greet.contains("local_greeting") && !greet.contains("spawn_greeting_llm"),
        "empty-home greeting must stay local (no Fast-model paraphrase on paint): {greet}"
    );
    let dream = src
        .split("fn run_dream")
        .nth(1)
        .and_then(|s| s.split("fn dispatch_send").next())
        .expect("run_dream");
    assert!(
        dream.contains("visible_host_receipts") && dream.contains("dream_rewind_id"),
        "/dream must use this tab's host receipts, not cabin-global last_receipts: {dream}"
    );
    let key = dream.find("llm_ready").expect("dream auth");
    let push = dream.find("live_mut().push").expect("dream push");
    assert!(
        key < push && dream.contains("self.running"),
        "/dream must not persist a turn when Imagine cannot start: {dream}"
    );
    assert!(
        dream.contains("stamp_current_access") || dream.contains("accessed_ms"),
        "/dream must bump accessed_ms or /sync LWW can drop the dream turn: {dream}"
    );
    let night = src
        .split("fn last_night_hint")
        .nth(1)
        .and_then(|s| s.split("fn mark_auto_ran").next())
        .expect("last_night_hint");
    assert!(
        night.contains("visible_host_receipts") && !night.contains("last_receipts"),
        "greeting last-night must not mix another tab's receipts: {night}"
    );
    let vis = src
        .split("fn visible_host_receipts(")
        .nth(1)
        .and_then(|s| s.split("fn dream_rewind_id").next())
        .expect("visible_host_receipts");
    assert!(
        vis.contains("thread_host_receipts_from") && !vis.contains("content.clone()"),
        "empty-chat greeting must not clone an 8MB transcript to read host receipts: {vis}"
    );
    let context = src
        .split("Slash::Context =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Health =>").next())
        .expect("Context");
    assert!(
            context.contains("visible_turn_count")
                && context.contains("estimate_messages")
                && context.contains("grok_usage")
                && context.contains("grok_context_line")
                && !context.contains("content.clone()"),
            "/context must prefer Grok Build server tokens without cloning an 8MB transcript: {context}"
        );
    let finish = src
        .split("fn finish_hub_dispatch")
        .nth(1)
        .and_then(|s| s.split("fn hide_to_tray").next())
        .expect("finish_hub_dispatch");
    assert!(
        finish.contains("self.pending_hub_task.clone()")
            && finish.contains("clear_pending_after_complete"),
        "do not drop pending_hub_task before the hub mutex is held"
    );
    let complete_at = finish.find("complete_task").expect("complete_task");
    let persist_at = finish
        .find("self.persist_hub()")
        .expect("persist hub complete");
    assert!(
        complete_at < persist_at && !finish.contains("persist_snap"),
        "/send task completion must hit hub-state.json without cloning every thread: {finish}"
    );
    let host_done = src
        .split("Ok(JobOut::HostDone(block))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::Connector").next())
        .expect("HostDone");
    assert!(
        host_done.contains("pending_connectors"),
        "queued connectors must run after host before the next kick_model"
    );
    let consult = src
        .split("fn run_consult")
        .nth(1)
        .and_then(|s| s.split("fn open_palette").next())
        .expect("run_consult");
    assert!(
        consult.contains("if self.running") && consult.contains("halt_in_flight"),
        "consult must not drop a finished parent reply: {consult}"
    );
    assert!(
        consult.contains("if self.chat_job_thread.is_none()"),
        "consult must stay on the origin thread: {consult}"
    );
    assert!(
        consult.contains("Interrupted by consult"),
        "slash consult during a /send job must fail the dispatch: {consult}"
    );
    let consult_out = src
        .split("Ok(JobOut::Consult(detail))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::HostLine").next())
        .expect("Consult");
    assert!(
        !consult_out.contains("finish_hub_dispatch"),
        "consult must not complete a /send task as the consult reply: {consult_out}"
    );
    assert!(
        consult_out.contains("status.clear()") || consult_out.contains("status ="),
        "consult must not leave the status bar on Consult… after the reply lands: {consult_out}"
    );
    let slash_consult = src
        .split("Slash::Consult")
        .nth(1)
        .and_then(|s| s.split("Slash::Usage").next())
        .expect("Slash::Consult");
    assert!(
        slash_consult.contains("run_consult"),
        "typed /consult must use the consult worker: {slash_consult}"
    );
    let chat_consult = fn_src(&src, "run_consult");
    let finish_at = chat_consult.find("finish_hub_dispatch");
    let run_at = chat_consult.find("grok_chat");
    assert!(
        finish_at.is_some_and(|f| run_at.is_some_and(|r| f < r)),
        "finish the /send task before starting consult: {chat_consult}"
    );
    let rewind = src
        .split("fn rewind_project")
        .nth(1)
        .and_then(|s| s.split("fn snapshot_project").next())
        .expect("rewind_project");
    let restoring = rewind.find("Restoring").expect("Restoring");
    let blocked = rewind.find("rewind_blocked_reason").expect("rewind gate");
    assert!(
        blocked < restoring && !rewind.contains("Restored"),
        "/rewind must not claim Restored before cp finishes or when host cannot start: {rewind}"
    );
    let queued = rewind.find("queue_sh").expect("queue restore");
    assert!(
        queued < restoring && rewind[queued..restoring].contains("self.running"),
        "/rewind must not claim Restoring when host did not start: {rewind}"
    );
    let took = rewind.find("took one").expect("first snapshot");
    let snap_q = rewind.rfind("queue_sh").expect("queue snapshot");
    assert!(
        snap_q < took && rewind[snap_q..took].contains("self.running"),
        "/rewind must not claim a snapshot started when host did not start: {rewind}"
    );
    assert!(
            rewind.contains("rewind_copy_cmd"),
            "/rewind restore must copy snapshot contents into the project, not nest the dest folder: {rewind}"
        );
    assert!(
        rewind.contains("expand_home"),
        "/rewind must expand ~ before quoting the bound tree: {rewind}"
    );
    let snap = src
        .split("fn snapshot_project")
        .nth(1)
        .and_then(|s| s.split("fn doctor_text").next())
        .expect("snapshot_project");
    assert!(
        snap.contains("rewind_blocked_reason")
            && snap.contains("rewind_copy_cmd")
            && !snap.contains("run_cmds"),
        "snapshot must record a dest and return the cp, not nest run_cmds: {snap}"
    );
    assert!(
        snap.contains("expand_home"),
        "snapshot must expand ~ before quoting the bound tree: {snap}"
    );
    let snap_spawn = snap
        .find("thread::spawn")
        .expect("rewind index must leave the UI thread");
    let snap_write = snap.find("save_rewinds").expect("save_rewinds");
    assert!(
        snap_spawn < snap_write,
        "snapshot must not freeze the cabin writing rewind.json: {snap}"
    );
    assert!(
        src.contains("is_rewind_copy_cmd"),
        "host jobs must prepend a snapshot instead of nesting run_cmds"
    );
    assert!(
        src.contains("expand_home(&restore_bound_path"),
        "boot must expand a tilde-bound project or rewind quotes a literal ~ folder"
    );
    let dream_id = src
        .split("fn dream_rewind_id")
        .nth(1)
        .and_then(|s| s.split("fn run_dream").next())
        .expect("dream_rewind_id");
    assert!(
        dream_id.contains("expand_home"),
        "/dream rewind cite must expand ~ before matching the snapshot root: {dream_id}"
    );
    let host_done = src
        .split("Ok(JobOut::HostDone(block))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::Connector").next())
        .expect("HostDone");
    assert!(
        host_done.contains("job_is_scratch"),
        "HostDone must use the origin thread scratch flag: {host_done}"
    );
    assert!(
        host_done.contains("parse_computer_op") && !host_done.contains("parse_computer_cmd_loose"),
        "a leftover type cargo in last_host must not be labeled COMPUTER_RESULT: {host_done}"
    );
    assert!(
        host_done.contains("append_host_trajectory") && host_done.contains("trim_job_result_dumps"),
        "HostDone must record a trajectory line and trim old tool dumps: {host_done}"
    );
    let traj = src
        .split("fn append_host_trajectory(")
        .nth(1)
        .and_then(|s| s.split("fn trim_job_result_dumps").next())
        .expect("append_host_trajectory");
    let traj_spawn = traj
        .find("thread::spawn")
        .expect("trajectory must leave the UI thread");
    let traj_write = traj.find("append_trajectory").expect("append_trajectory");
    assert!(
        traj_spawn < traj_write,
        "HostDone must not freeze the cabin rewriting a 2MB trajectory.jsonl: {traj}"
    );
    let trim = src
        .split("fn trim_job_result_dumps(")
        .nth(1)
        .and_then(|s| s.split("fn queue_sh(").next())
        .expect("trim_job_result_dumps");
    let est = trim
        .find("should_trim_result_bodies")
        .expect("estimate before clone");
    assert!(
        trim.contains("trim_result_bodies_in_place") && !trim.contains("content.clone()"),
        "result trim must rewrite old dumps in place, not clone an 8MB pane: {trim}"
    );
    assert!(
        trim[est..].contains("trim_result_bodies_in_place"),
        "result trim must estimate borrowed tokens before touching dumps: {trim}"
    );
    let commit = src
        .split("fn commit_proposed_skill(")
        .nth(1)
        .and_then(|s| s.split("fn apply_review_skill_patches").next())
        .expect("commit_proposed_skill");
    let commit_spawn = commit
        .find("thread::spawn")
        .expect("skill write must leave the UI thread");
    let commit_save = commit.find("save_skill").expect("save_skill");
    assert!(
        commit_spawn < commit_save && !commit.contains("list_skills"),
        "HostDone must not freeze the cabin writing SKILL.md: {commit}"
    );
    let verify = src
        .split("fn apply_verify_result(")
        .nth(1)
        .and_then(|s| s.split("fn replay_saved_recipe(").next())
        .expect("apply_verify_result");
    let verify_spawn = verify
        .find("thread::spawn")
        .expect("verify skill write must leave the UI thread");
    let verify_save = verify.find("save_skill").expect("save_skill");
    assert!(
        verify_spawn < verify_save,
        "verify pass must not freeze the cabin writing SKILL.md: {verify}"
    );
    let ran = src
        .split("fn mark_auto_ran(")
        .nth(1)
        .and_then(|s| s.split("fn mark_auto_skipped(").next())
        .expect("mark_auto_ran");
    let ran_spawn = ran
        .find("thread::spawn")
        .expect("night save must leave the UI thread");
    let ran_save = ran.find("night::save").expect("night::save");
    assert!(
        ran_spawn < ran_save,
        "night run stamp must not freeze the cabin writing automations.json: {ran}"
    );
    let run_cmds = src
        .split("fn run_cmds")
        .nth(1)
        .and_then(|s| s.split("fn run_connector").next())
        .expect("run_cmds");
    assert!(
        run_cmds.contains("mint_host_halt") && !run_cmds.contains("host_halt.store(false"),
        "a new host job must not clear the previous job's halt flag: {run_cmds}"
    );
    assert!(
        run_cmds.contains("!is_rewind_copy_cmd") && run_cmds.contains("host_cmd_leaves_project"),
        "cabin rewind copies must run when YOLO is off: {run_cmds}"
    );
    assert!(
        run_cmds.contains("AlwaysApprove") && !run_cmds.contains("cfg.yolo"),
        "bound-tree jail follows the Always pill, not leftover app.json yolo: {run_cmds}"
    );
    let impl_src = src.as_str();
    assert!(
        !impl_src.contains("if let Some(plan) = plan_from_text"),
        "Chat complete must not parse HOST_CMD / COMPUTER_CMD; Grok Build owns tools"
    );
}

#[test]
fn mode_status_does_not_treat_ladder_default_as_auto_pin() {
    assert_eq!(
        super::mode_status_line("auto", "grok-3-mini-fast"),
        "Mode auto — routes Fast / Balance / Think / Max"
    );
    assert_eq!(
        super::mode_status_line("auto", "grok-4.6"),
        "Mode auto — routes Fast / Balance / Think / Max"
    );
    assert_eq!(
        super::mode_status_line("auto", "grok-4.7"),
        "Mode auto — routes Fast / Balance / Think / Max"
    );
    assert_eq!(
        super::mode_status_line("auto", "grok-3"),
        "Mode auto → grok-3"
    );
    assert_eq!(
        super::mode_status_line("think", "grok-3"),
        "Mode think → grok-4.7 · high"
    );
    assert_eq!(
        super::mode_status_line("max", ""),
        "Mode max → grok-4.7 · xhigh"
    );
}

#[test]
fn empty_home_paints_faint_greeting() {
    let src = cabin_src();
    let slice = src
        .split("fn ui_empty_home")
        .nth(1)
        .and_then(|s| s.split("fn ui_composer_stack(").next())
        .expect("empty home");
    assert!(
        slice.contains("self.greeting"),
        "new chats paint a greeting blurb: {slice}"
    );
    assert!(
        slice.contains("GREET_HERO") && slice.contains("title_font"),
        "greeting is the empty-home hero, not a 56px wordmark: {slice}"
    );
    assert!(
        slice.contains("theme::mark"),
        "empty home paints a quiet Grok mark above the greeting: {slice}"
    );
    assert!(
        !slice.contains("italics"),
        "greeting is regular/medium weight: {slice}"
    );
    assert!(
        slice.contains("paint_approval_stack"),
        "empty home must still show a live permission bar: {slice}"
    );
    assert!(
            slice.contains("pulse_should_paint")
                && slice.contains("empty_home_composer_top")
                && slice.contains("paint_device_glance_row")
                && !slice.contains("paint_update_feed")
                && !slice.contains("paint_home_deck_over_chat")
                && !slice.contains("home_feed_count")
                && !slice.contains("collapsed_stack_h")
                && !slice.contains("paint_empty_pulse")
                && !slice.contains("paint_lane_chip")
                && !slice.contains("No updates"),
            "signed-in empty home is chat-first: greeting, composer, device glance; no Home card deck: {slice}"
        );
    assert!(
        !slice.contains("weather")
            && !slice.contains("Outlook")
            && !slice.contains("Gmail")
            && !slice.contains("calendar"),
        "empty-home pulse must not invent mail or weather: {slice}"
    );
    let greet = slice.find("self.greeting").expect("greeting");
    let composer = slice.find("ui_composer_stack").expect("composer");
    assert!(greet < composer, "greeting sits above the chat box");
    assert!(
        slice.contains("muted()"),
        "greeting uses secondary paint, not a washed-out whisper"
    );
    assert!(
        !slice.contains("Native Grok Build cabin"),
        "empty home is the greeting, not a product tagline: {slice}"
    );
    assert!(
        !slice.contains("RichText::new(\"GrokHub\")"),
        "empty home must not paint a GrokHub wordmark: {slice}"
    );
    assert!(
        !slice.contains("device_name"),
        "empty home must not paint the hostname: {slice}"
    );
    assert!(
        slice.contains("empty_home_composer_top") && slice.contains("empty_home_greet_top"),
        "greeting sits in the title-to-composer gap; the chat box stays on the midline: {slice}"
    );
    assert!(
        slice.contains("greeting_galley_h") || slice.contains("fonts(|"),
        "wrapped greeting height must drive vertical placement: {slice}"
    );
    assert!(
        !slice.contains("* 0.38"),
        "empty-home composer sits in the vertical center, not the upper third: {slice}"
    );
    assert!(
        slice.contains("12.0") && !slice.contains("add_space(28.0)"),
        "greeting-to-composer gap stays tight: {slice}"
    );
    let chips = src.find("ComposerStackSlot::Chips =>").expect("chips arm");
    let chips = &src[chips..chips + 700];
    assert!(
        chips.contains("add_space(6.0)"),
        "chips sit a tight gap under the pill: {chips}"
    );
    assert!(
        chips.contains("composer_chips()") && !chips.contains("messages.is_empty()"),
        "suggestion chips stay up mid-thread, not only on an empty chat: {chips}"
    );
    assert_eq!(super::empty_home_side_gap(1800.0, 800.0), 500.0);
    assert_eq!(super::empty_home_side_gap(700.0, 800.0), 0.0);
    assert_eq!(super::empty_home_composer_top(800.0, 60.0), 370.0);
    assert_eq!(super::empty_home_greet_top(370.0, 40.0, 12.0), 159.0);
    let short = super::empty_home_greet_top(370.0, 40.0, 12.0);
    let wrapped = super::empty_home_greet_top(370.0, 100.0, 12.0);
    assert!(
        wrapped < short,
        "a wrapped greeting rises so it stays centered in the gap: {wrapped} vs {short}"
    );
    assert!(
        (wrapped + 50.0 - (370.0 - 12.0) * 0.5).abs() < 0.5,
        "wrapped greeting midpoint is the title-to-composer midpoint: {wrapped}"
    );
    assert_eq!(super::empty_home_greet_top(80.0, 90.0, 12.0), 0.0);
    assert!(
        slice.contains("empty_home_side_gap"),
        "empty-home column must be centered in leftover width, not left-packed: {slice}"
    );
}

#[test]
#[allow(clippy::assertions_on_constants)] // pins design constants
fn rail_footer_is_reserved() {
    assert_eq!(super::RAIL_FOOTER_H, 52.0);
    assert!(super::PALETTE_LIST_H < 400.0);
}

#[test]
fn rail_chat_title_stays_short() {
    assert_eq!(
        grokhub_core::display_tab_title("chowder and food interest and cho"),
        "chowder"
    );
    with_fonts_ui(|ui| {
        let painted = super::fit_rail_label(ui, "chowder and food interest and cho", 72.0);
        assert!(
            painted.chars().count() < 20,
            "rail label must not run off the pill: {painted}"
        );
        assert!(painted.ends_with('…') || painted == "chowder", "{painted}");
    });
}

#[test]
fn appearance_tab_offers_light() {
    let ids: Vec<&str> = grokhub_core::appearance_choices()
        .iter()
        .copied()
        .map(grokhub_core::theme_id)
        .collect();
    assert_eq!(ids, vec!["dark", "light", "system"]);
    assert_eq!(
        grokhub_core::parse_theme("light"),
        grokhub_core::ThemeChoice::Light
    );
    assert!(!grokhub_core::resolve_dark(
        grokhub_core::ThemeChoice::Light,
        true
    ));
    assert!(grokhub_core::resolve_dark(
        grokhub_core::ThemeChoice::Dark,
        false
    ));
    assert!(!grokhub_core::resolve_dark(
        grokhub_core::ThemeChoice::System,
        false
    ));
}

#[test]
fn presence_ring_drops_a_huge_frame() {
    let src = cabin_src();
    let push = src
        .split("fn push_presence(")
        .nth(1)
        .and_then(|s| s.split("fn live_room(").next())
        .expect("push_presence");
    assert!(
        push.contains("FRAME_CAP"),
        "live presence must not keep an 8MB JPEG data URL for ten minutes: {push}"
    );
    assert!(
        push.contains("PRESENCE_RING_MAX")
            || (push.contains("presence_ring.len()") && push.contains("32")),
        "a 10-minute ring of FRAME_CAP JPEGs can still OOM live Eyes: {push}"
    );
}

#[test]
fn last_frame_url_drops_a_huge_capture() {
    let src = cabin_src();
    let impl_src = src.as_str();
    let remember = impl_src
        .split("fn remember_last_frame(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .expect("remember_last_frame");
    assert!(
        remember.contains("FRAME_CAP"),
        "last_frame_url must not keep an 8MB grim data URL: {remember}"
    );
    let assigns = impl_src.matches("last_frame_url = Some").count();
    assert!(
        assigns <= 1,
        "every last-frame write must go through remember_last_frame, found {assigns}"
    );
    let hub_frame = impl_src
        .split("fn store_hub_frame(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .expect("store_hub_frame");
    let parse = hub_frame.find("store_frame").expect("parse jpeg");
    let lock = hub_frame.find("hub.lock").expect("hub lock");
    assert!(
        parse < lock && hub_frame.contains("install_frame"),
        "cabin must not decode a 400KB JPEG under hub.lock(): {hub_frame}"
    );
}

#[test]
fn eyes_frame_tex_rejects_a_huge_frame() {
    let src = cabin_src();
    let tex = src
        .split("fn eyes_frame_tex(")
        .nth(1)
        .and_then(|s| s.split("fn project_row_active(").next())
        .expect("eyes_frame_tex");
    let cap = tex
        .find("IMAGE_FILE_CAP")
        .expect("size check before decode");
    let decode = tex.find("load_from_memory").expect("decode");
    let spawn = tex
        .find("thread::spawn")
        .expect("decode must leave the UI thread");
    assert!(
        spawn < decode && cap < decode,
        "Eyes last-frame paint must not decode a huge JPEG on the UI thread: {tex}"
    );
    assert!(
        tex.contains("image_pixels_ok") || tex.contains("IMAGE_PIXEL_CAP"),
        "Eyes last-frame paint must not decode a pixel bomb on the UI thread: {tex}"
    );
}

#[test]
fn thought_uses_live_theme_tokens() {
    let src = cabin_src();
    let start = src.find("ChatKind::Thought =>").expect("thought");
    let slice = &src[start..start + 1600];
    assert!(slice.contains("theme::muted()"), "{slice}");
    assert!(
        slice.contains("Thought process"),
        "thinking must be marked as thought process, not a chat bubble: {slice}"
    );
    assert!(!slice.contains("theme::MUTED"));
    assert!(!slice.contains("theme::SUBTLE"));
    let bubble = src
        .split("fn paint_thought_bubble(")
        .nth(1)
        .and_then(|s| s.split("fn paint_chat_block(").next())
        .expect("paint_thought_bubble");
    assert!(
        bubble.contains("theme::subtle()"),
        "thought words must be darker than chat fg: {bubble}"
    );
    assert!(
        !bubble.contains("theme::fg()"),
        "thought words must not use chat fg: {bubble}"
    );
    assert!(
        bubble.contains("TRANSPARENT"),
        "thoughts stay flush on the canvas, no elevated card: {bubble}"
    );
    assert!(
        !bubble.contains("theme::surface()"),
        "thoughts must not sit on a surface card: {bubble}"
    );
    assert!(
        !bubble.contains("USER_BUBBLE_RADIUS") && !bubble.contains("bubble_assistant()"),
        "thought process is not a chat bubble: {bubble}"
    );
}

#[test]
fn tool_work_starts_collapsed() {
    let src = cabin_src();
    let tools = src
        .split("fn paint_tool_cards(")
        .nth(1)
        .and_then(|s| s.split("fn paint_running(").next())
        .expect("paint_tool_cards");
    assert!(
        tools.contains("CollapsingHeader") && tools.contains("default_open(false)"),
        "finished work must sit in a collapsed tree: {tools}"
    );
    assert!(
        tools.contains("Work"),
        "the collapsed tree is labeled Work: {tools}"
    );
    let live = src
        .split("fn paint_one_tool_card(")
        .nth(1)
        .and_then(|s| s.split("fn paint_tool_card_body(").next())
        .expect("paint_one_tool_card");
    assert!(
        live.contains("CollapsingHeader") && live.contains("default_open(false)"),
        "live tool calls must start collapsed: {live}"
    );
    let block = src
        .split("ChatKind::Tool => {")
        .nth(1)
        .and_then(|s| s.split("fn screen_from_rows(").next())
        .expect("tool arm");
    assert!(
        block.contains("CollapsingHeader") && block.contains("default_open(false)"),
        "Hands / tool rows must start collapsed: {block}"
    );
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 400.0), |ui| {
            ui.set_max_width(800.0);
            let card = grokhub_core::wire::ToolCard {
                id: "t1".into(),
                title: "run_terminal_cmd".into(),
                kind: "execute".into(),
                status: "completed".into(),
                detail: "ran ls -la and printed a long listing that must stay hidden".into(),
                diff: String::new(),
                image_data_url: None,
                raw_input: String::new(),
            };
            let body_h = ui
                .scope(|ui| {
                    super::paint_tool_card_body(ui, &card);
                })
                .response
                .rect
                .height();
            let closed_h = ui
                .scope(|ui| {
                    super::paint_one_tool_card(ui, &card);
                })
                .response
                .rect
                .height();
            assert!(
                closed_h + 16.0 < body_h,
                "collapsed tool still showed the body: closed {closed_h} body {body_h}"
            );
        });
    });
}

#[test]
fn composer_stack_drops_approve_slots() {
    let src = cabin_src();
    let start = src.find("fn ui_composer_stack").expect("composer stack");
    let end = src[start..]
        .find("fn ui_devices")
        .map(|i| start + i)
        .unwrap_or(src.len());
    let stack = &src[start..end];
    assert!(!stack.contains("SkillApprove"), "{stack}");
    assert!(!stack.contains("SaveAsSkill"), "{stack}");
    assert!(!stack.contains("HostPlan"), "{stack}");
    let bar = stack
        .split("ComposerStackSlot::ContextBar =>")
        .nth(1)
        .and_then(|s| s.split("ComposerStackSlot::SlashPalette").next())
        .expect("context bar");
    assert!(
        bar.contains("rect_filled") && bar.contains("grok_context_line"),
        "the context usage bar stays above the composer: {bar}"
    );
    assert!(
        bar.contains("paint_connectivity_status_chip"),
        "connectivity chip sits in the context bar above the composer: {bar}"
    );
    let chip = fn_src(&src, "paint_connectivity_status_chip");
    assert!(
        chip.contains("global_ui_phase") && chip.contains("badge(ui"),
        "connectivity chip uses the U1 kit badge: {chip}"
    );
    assert!(
        !bar.contains("ghost_pill(ui, \"Compact\")") && !bar.contains("Slash::Compact"),
        "Compact leaves the context bar for the titlebar menu: {bar}"
    );
    let tools = stack
        .split("ComposerStackSlot::SessionTools =>")
        .nth(1)
        .and_then(|s| s.split("ComposerStackSlot::SlashPalette").next())
        .expect("session tools");
    assert!(
        !tools.contains("Copy session") && !tools.contains("\"Export\""),
        "Copy session and Export leave the composer: {tools}"
    );
    assert!(src.contains("View plan"), "view plan stays on the thread");
    assert!(
        !src.contains("How fork works") && !src.contains("fork_offer_why"),
        "the fork offer is gone from the thread"
    );
    let menu = src
        .split("fn paint_session_actions_menu")
        .nth(1)
        .and_then(|s| s.split("fn ui_composer_stack").next())
        .expect("session menu");
    assert!(
        menu.contains("ChromeBtn::Menu")
            && menu.contains("titlebar_chrome_hit")
            && !menu.contains("resp.clicked()")
            && menu.contains("\"Compact\"")
            && menu.contains("Slash::Compact")
            && menu.contains("\"Copy session\"")
            && menu.contains("export_markdown")
            && menu.contains("Copied session")
            && menu.contains("\"Export\"")
            && menu.contains("Slash::Export"),
        "the titlebar menu keeps Compact, Copy session, and Export: {menu}"
    );
    let title = src
        .split("fn ui_titlebar(")
        .nth(1)
        .and_then(|s| s.split("fn nav_row(").next())
        .expect("titlebar");
    let min = title.find("ChromeBtn::Minimize").expect("minimize");
    let ham = title.find("paint_session_actions_menu").expect("menu");
    let drag = title.find("titlebar_should_start_drag").expect("drag");
    assert!(
        min < ham && ham < drag,
        "hamburger is allocated immediately after minimize (RTL: directly to its left)"
    );
    assert_eq!(
        super::session_menu_enabled(false, true),
        (false, false, false)
    );
    assert_eq!(super::session_menu_enabled(true, true), (true, true, true));
    assert_eq!(
        super::session_menu_enabled(false, false),
        (true, false, false),
        "usage alone still offers Compact, not Copy or Export"
    );
    let send = fn_src(&src, "send_chat");
    assert!(
        send.contains("btw_queues_without_interrupt") && send.contains("side_ask_queue"),
        "btw must queue a side ask instead of halting the live run: {send}"
    );
    let order = super::composer_stack_order();
    assert_eq!(
        order,
        &[
            super::ComposerStackSlot::AuthBanner,
            super::ComposerStackSlot::ContextBar,
            super::ComposerStackSlot::SessionTools,
            super::ComposerStackSlot::LiveWork,
            super::ComposerStackSlot::SlashPalette,
            super::ComposerStackSlot::Attach,
            super::ComposerStackSlot::Voice,
            super::ComposerStackSlot::Pill,
            super::ComposerStackSlot::Chips,
        ]
    );
}

#[test]
fn composer_chips_stay_visible_mid_thread() {
    let src = include_str!("chips.rs");
    let body = src
        .split("fn composer_chips(")
        .nth(1)
        .and_then(|s| s.split("fn take_chip_act(").next())
        .expect("composer_chips");
    assert!(
        body.contains("self.visible_chips.clone()"),
        "mid-chat uses the ranked pool, including the habit/static fallback: {body}"
    );
    assert!(
        !body.contains("Vec::new()") && !body.contains("if home"),
        "a thread with messages must not clear ranked chips: {body}"
    );
    assert!(
        body.contains("skill_offer_chip"),
        "skill offer still inserts on the same row: {body}"
    );
    let ui = include_str!("chat_ui.rs");
    let chips = ui
        .split("ComposerStackSlot::Chips =>")
        .nth(1)
        .and_then(|s| s.split("ComposerStackSlot::Attach =>").next())
        .expect("chips slot");
    assert!(
        chips.contains("composer_chips()") && chips.contains("quick_chip_row"),
        "{chips}"
    );
    assert!(
        !chips.contains("messages.is_empty()") && !chips.contains("is_empty()"),
        "chips paint when the thread has messages: {chips}"
    );
}

#[test]
fn chips_sit_below_the_composer_pill() {
    let order = super::composer_stack_order();
    let chips = order
        .iter()
        .position(|s| *s == super::ComposerStackSlot::Chips);
    let pill = order
        .iter()
        .position(|s| *s == super::ComposerStackSlot::Pill)
        .expect("pill");
    assert!(chips.is_some(), "chips belong below the composer pill");
    assert!(chips.unwrap() > pill);
    let voice = order
        .iter()
        .position(|s| *s == super::ComposerStackSlot::Voice)
        .expect("voice");
    assert!(voice < pill, "voice indicator sits above the composer pill");
}

#[test]
fn voice_mode_has_indicator_and_stop() {
    let src = cabin_src();
    let listen = src
        .split("fn listen_voice(")
        .nth(1)
        .and_then(|s| s.split("fn voice_is_on(").next())
        .expect("listen_voice");
    assert!(
        listen.contains("voice_is_on()") && listen.contains("self.leave_voice()"),
        "mic / Ctrl+G must leave a live voice session: {listen}"
    );
    assert!(
        listen.contains("listen_turn") && !listen.contains("voice_ws::start"),
        "Hey Grok is PTT on every platform — duplex must not own the mic: {listen}"
    );
    let leave = src
        .split("fn leave_voice(")
        .nth(1)
        .and_then(|s| s.split("fn paint_voice_mode_row(").next())
        .expect("leave_voice");
    assert!(
        leave.contains("Voice off"),
        "leave voice must say Voice off: {leave}"
    );
    let row = src
        .split("fn paint_voice_mode_row(")
        .nth(1)
        .and_then(|s| s.split("fn paint_voice_mic(").next())
        .expect("paint_voice_mode_row");
    assert!(
        row.contains("voice_mode_row")
            && row.contains("leave_voice")
            && row.contains("voice_strip_visible")
            && row.contains("VoiceState::Ready"),
        "voice chrome must paint the indicator and Stop: {row}"
    );
    let mic = src
        .split("fn paint_voice_mic(")
        .nth(1)
        .and_then(|s| s.split("fn capture_cabin_frame_this_turn(").next())
        .expect("paint_voice_mic");
    assert!(
        mic.contains("paint_composer_mic")
            && mic.contains("MicMood::Speaking")
            && mic.contains("Leave voice"),
        "live mic must ease while speaking and read as leave: {mic}"
    );
    let stack = src
        .split("fn ui_composer_stack")
        .nth(1)
        .and_then(|s| s.split("fn ui_devices").next())
        .expect("composer stack");
    assert!(
        stack.contains("ComposerStackSlot::Voice") && stack.contains("paint_voice_mode_row"),
        "chat composer must show the voice indicator: {stack}"
    );
    let imagine = src
        .split("fn ui_imagine(")
        .nth(1)
        .and_then(|s| s.split("fn ui_imagine_bar(").next())
        .expect("ui_imagine");
    assert!(
        imagine.contains("paint_voice_mode_row"),
        "Imagine must show the same voice indicator: {imagine}"
    );
    let voice_job = src
        .split("Ok(JobOut::Voice(t))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::UpdateProgress").next())
        .expect("JobOut::Voice");
    assert!(
        !voice_job.contains("voice_state = VoiceState::Idle")
            && voice_job.contains("voice_state_after_ptt_stt")
            && voice_job.contains("ptt_after_stt")
            && voice_job.contains("maybe_continue_ptt"),
        "PTT must not idle after one listen: {voice_job}"
    );
    let speak = src
        .split("fn speak_reply(")
        .nth(1)
        .and_then(|s| s.split("fn poll_eyes_cap(").next())
        .expect("speak_reply");
    assert!(
        speak.contains("voice_hold_rx") && speak.contains("maybe_continue_ptt"),
        "TTS done must re-arm listen while the line is open: {speak}"
    );
    let start = src
        .split("fn start_ptt_listen(")
        .nth(1)
        .and_then(|s| s.split("fn maybe_continue_ptt(").next())
        .expect("start_ptt_listen");
    assert!(
        start.contains("listen_turn") && !start.contains("leave_voice"),
        "the next utterance must listen without re-entering voice: {start}"
    );
    let perm = fn_src(&src, "set_permission_mode");
    assert!(
        perm.contains("persistable_permission_mode"),
        "Always must not persist: {perm}"
    );
}

#[test]
fn other_chip_threads_skip_current_scratch_and_background() {
    let mut current = crate::threads::ChatThread::new("Now", false);
    current.id = "cur".into();
    current
        .messages_mut()
        .push(("user".into(), "this chat".into()));
    let mut prev = crate::threads::ChatThread::new("Night cabin", false);
    prev.id = "prev".into();
    prev.messages_mut()
        .push(("user".into(), "paint the wall".into()));
    prev.messages_mut()
        .push(("assistant".into(), "I can sketch the first coat.".into()));
    let mut scratch = crate::threads::ChatThread::new("Scratch", true);
    scratch.id = "scr".into();
    scratch
        .messages_mut()
        .push(("user".into(), "ignore me".into()));
    // A Follow up card's chat is scheduled work: no "Continue Follow up · …" chip.
    let mut follow = crate::threads::ChatThread::new("Follow up · Host snapshot", false);
    follow.id = "fu".into();
    follow.background = true;
    follow
        .messages_mut()
        .push(("user".into(), "which ports are new".into()));
    follow
        .messages_mut()
        .push(("assistant".into(), "Disk at 91% on /home.".into()));
    let others = super::collect_other_chip_threads(&[current, prev, scratch, follow], "cur");
    assert_eq!(others.len(), 1);
    assert_eq!(others[0].title, "Night cabin");
    assert_eq!(others[0].last_user, "paint the wall");
}

#[test]
fn chat_composer_pins_stop_on_the_right() {
    let src = cabin_src();
    let start = src.find("ComposerStackSlot::Pill =>").expect("pill arm");
    // 15_000: rustfmt wrapping pushed Stop / primary_pressed past the old 12_000 window.
    // Keep Stop near the end of the arm; do not grow logic before the Stop handler.
    let pill = &src[start..start + 15000];
    assert!(
        pill.contains("composer_go_cluster_w()"),
        "Fast + mic + Stop need a reserved strip: {pill}"
    );
    assert!(
        pill.contains("composer_mid_w(") && pill.contains("composer_go_hit_w("),
        "Plus/mid/Stop widths come from the window pill, not inflated available: {pill}"
    );
    let stop = pill.find("ComposerGo::Stop").expect("stop glyph");
    let edit = pill.find("TextEdit::multiline").expect("composer field");
    assert!(
        edit < stop,
        "Send/Stop is the last sibling after an exact-width mid strip"
    );
    assert!(
        pill.contains("is_pointer_button_down_on"),
        "Stop must halt on press; click-release is eaten by the shrink feel: {pill}"
    );
    assert!(
        pill.contains("primary_pressed"),
        "go press is edge-triggered so holding Send does not immediately Stop: {pill}"
    );
    assert!(
        !pill.contains("- 180.0"),
        "180px left Fast as the pill's right edge on a 900-wide cabin"
    );
    let home = src
        .split("fn ui_empty_home")
        .nth(1)
        .and_then(|s| s.split("fn ui_composer_stack(").next())
        .expect("empty home");
    let cap = home.find("composer_pill_w").expect("pane cap");
    let after = &home[cap..];
    assert!(
        after.contains("self.greeting"),
        "greeting paints inside the capped column"
    );
    assert!(
        home.contains("scope_builder")
            && home.contains("empty_home_side_gap")
            && home.contains("top_down_justified"),
        "empty-home cluster is a tight centered column, not a full-height justified fill: {home}"
    );
    assert!(
        !home.contains("vertical_centered_justified"),
        "vertical_centered_justified fills leftover height and drops the chips: {home}"
    );
    let stack = src
        .find("for slot in composer_stack_order()")
        .expect("stack");
    let cap = &src[stack.saturating_sub(280)..stack];
    assert!(
        cap.contains("composer_pill_w("),
        "chip row must not stretch the centered column past the pane: {cap}"
    );
}

#[test]
fn composer_go_disc_stays_inside_the_pill_beside_the_mic() {
    let _paint = crate::theme::hold_paint_test();
    let states = ["send", "stop", "idle"];
    for width in [1280.0_f32, 900.0, 1920.0] {
        for row_n in [1_usize, 3] {
            for state in states {
                let mut cabin = quiet_cabin();
                let thread = crate::threads::ChatThread::new("Chat", false);
                let tid = thread.id.clone();
                cabin.threads = vec![thread];
                cabin.thread_idx = 0;
                cabin.composer = match (state, row_n) {
                    ("idle", 1) | ("stop", 1) => String::new(),
                    ("idle", _) | ("stop", _) => "\n\n".into(),
                    (_, 1) => "hello".into(),
                    _ => "one\ntwo\nthree".into(),
                };
                let rows = (cabin.composer.matches('\n').count() + 1).clamp(1, 6);
                assert_eq!(rows, row_n, "{state} at {width} did not build {row_n} rows");
                if state == "stop" {
                    cabin.running = true;
                    cabin.chat_job_thread = Some(tid);
                } else {
                    cabin.running = false;
                    cabin.chat_job_thread = None;
                }
                assert_eq!(
                    cabin.thinking_here(),
                    state == "stop",
                    "{state} rows={row_n} width={width}: thinking_here={}",
                    cabin.thinking_here()
                );
                if state != "stop" {
                    assert!(
                        cabin.composer.trim().is_empty() != (state == "send"),
                        "{state} rows={row_n} width={width}: composer={:?}",
                        cabin.composer
                    );
                }
                let ctx = egui::Context::default();
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 760.0),
                    )),
                    ..Default::default()
                };
                for _ in 0..2 {
                    let _ = crate::theme::test_pass(&ctx, raw.clone(), |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            cabin.ui_composer_stack(ui);
                        });
                    });
                }
                let (pill, mic, go) = cabin.composer_geom.expect("composer geom");
                assert!(
                        go.right() <= pill.right() - 24.0 + 0.5,
                        "{state} rows={row_n} width={width}: go.right()={} pill.right()={} (go.right()-pill.right()={})",
                        go.right(),
                        pill.right(),
                        go.right() - pill.right()
                    );
                assert!(
                    mic.right() + 8.0 <= go.left() + 0.5,
                    "{state} rows={row_n} width={width}: mic.right()={} go.left()={}",
                    mic.right(),
                    go.left()
                );
                assert!(
                    go.left() > mic.left(),
                    "{state} rows={row_n} width={width}: go.left()={} mic.left()={}",
                    go.left(),
                    mic.left()
                );
            }
        }
    }
}

#[test]
fn nightly_review_stays_quiet() {
    let src = cabin_src();
    let tick = src
        .split("fn tick_review(")
        .nth(1)
        .and_then(|s| s.split("fn review_chat_digest(").next())
        .expect("tick_review");
    assert!(
        !tick.contains("send_chat")
            && !tick.contains("Nav::Chat")
            && !tick.contains("self.running"),
        "tick_review must not open Chat or take the composer: {tick}"
    );
    let native_src = include_str!("native_unattended.rs").replace("\r\n", "\n");
    assert!(fn_src(&src, "spawn_review").contains("spawn_native_review"));
    let spawn = fn_src(&native_src, "spawn_native_review");
    assert!(
        !spawn.contains("send_chat") && !spawn.contains("Nav::Chat"),
        "spawn_review must not dump the review into chat: {spawn}"
    );
    assert!(
        !spawn.contains("self.running"),
        "spawn_review leaves the user chat free: {spawn}"
    );
    assert!(
        spawn.contains("model_for_mode(\"balanced\")"),
        "nightly review forces Balance: {spawn}"
    );
    let spawn_at = spawn
        .find("thread::spawn")
        .expect("review HTTP must leave the UI thread");
    let write = spawn.find("write_memory").expect("flush memory");
    let traj = spawn.find("read_trajectory").expect("trajectory digest");
    assert!(
        spawn_at < write && spawn_at < traj && spawn.contains("mem_body"),
        "nightly review must flush Memory and slurp trajectory off the UI thread: {spawn}"
    );
    assert!(
            !spawn[..spawn_at].contains("scratch()"),
            "Scratch is a chat tab — unsaved Memory editor edits must still reach the nightly digest: {spawn}"
        );
    let digest_fn = src
        .split("fn review_chat_digest(")
        .nth(1)
        .and_then(|s| s.split("fn spawn_review(").next())
        .expect("review_chat_digest");
    assert!(
            digest_fn.contains("thread_host_receipts")
                && !digest_fn.contains("last_receipts")
                && !digest_fn.contains("last_host"),
            "nightly review must take host receipts from the digested threads, not cabin-global last_host: {digest_fn}"
        );
    assert!(
        digest_fn.contains("digest_line_from")
            && !digest_fn.contains("content.clone()")
            && !digest_fn.contains("text.clone()"),
        "nightly review must not clone an 8MB complete into the digest: {digest_fn}"
    );
    let apply = fn_src(&src, "apply_review_reply");
    assert!(
        !apply.contains("send_chat") && !apply.contains("Nav::Chat"),
        "applying suggestions stays off the chat: {apply}"
    );
    let held = apply.split("Err(e)").nth(1).expect("review held");
    assert!(
        held.contains("last_review_day") && held.contains("save_suggestions"),
        "a held nightly review must not retry every heartbeat: {apply}"
    );
    assert!(
        apply.contains("thread::spawn") && apply.contains("save_suggestions"),
        "nightly review must not freeze the cabin writing suggestions.json: {apply}"
    );
    assert!(
        apply.contains("merge_suggestion_store"),
        "a partial nightly review must not wipe the other suggestion grids: {apply}"
    );
    assert!(
        apply.contains("CABIN_GITHUB_TOOLS") && !apply.contains("&[]"),
        "nightly review must drop already-wired GitHub tools: {apply}"
    );
    assert!(
        apply.contains("prune_live_suggestions"),
        "a successful review must drop wired GitHub tiles already sitting in the store: {apply}"
    );
    let wall = src
        .split("fn poll_wall(")
        .nth(1)
        .and_then(|s| s.split("fn tick_wall(").next())
        .expect("poll_wall");
    let ok_wall = wall
        .split("Ok(Ok(gif))")
        .nth(1)
        .and_then(|s| s.split("Ok(Err(e))").next())
        .expect("wall ok");
    let ok_spawn = ok_wall
        .find("thread::spawn")
        .expect("wall cover save must leave the UI thread");
    let ok_save = ok_wall.find("save_wall").expect("ok save_wall");
    assert!(
        ok_spawn < ok_save
            && ok_wall.contains("persist_io")
            && !ok_wall.contains("persist_snap")
            && !ok_wall.contains("self.persist()"),
        "a new wall cover must not clone every thread just to write imagine-wall.json: {wall}"
    );
    let held_wall = wall.split("Ok(Err(e))").nth(1).expect("wall held");
    let wall_spawn = held_wall
        .find("thread::spawn")
        .expect("wall save must leave the UI thread");
    let wall_save = held_wall.find("save_wall").expect("save_wall");
    assert!(
        wall_spawn < wall_save && held_wall.contains("persist_io"),
        "a held wall cover must not freeze the cabin writing imagine-wall.json: {wall}"
    );
    let tick_wall = src
        .split("fn tick_wall(")
        .nth(1)
        .and_then(|s| s.split("fn kick_wall(").next())
        .expect("tick_wall");
    assert!(
            tick_wall.contains("self.has_key()") && !tick_wall.contains("self.llm_ready()"),
            "first-run Get Started must not kick a wall cover just because grok is on disk: {tick_wall}"
        );
    assert!(
        apply.contains("apply_review_skill_patches"),
        "nightly review must patch existing skills from SUGGEST_SKILL_PATCH: {apply}"
    );
    assert!(src.contains("self.tick_review()"));
    assert!(
        src.contains("fn tick_session_suggestions(")
            && src.contains("suggestions_from_sessions")
            && src.contains("last_session_suggest_day")
            && src.contains("self.tick_session_suggestions()"),
        "quiet daily session suggestions must feed Suggestions: {src}"
    );
    assert!(
        src.contains("if !night_fired && !self.running"),
        "Review waits if Night just fired or chat is running"
    );
    let history = src
        .split("fn ui_history(")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("ui_history");
    assert!(
        history.contains("tick_history_search") && history.contains("open_history_hit"),
        "History searches as you type and a hit opens its source: {history}"
    );
    let debounce = src
        .split("fn tick_history_search(")
        .nth(1)
        .and_then(|s| s.split("fn kick_history_search(").next())
        .expect("tick_history_search");
    assert!(
        debounce.contains("HISTORY_TYPE_DELAY") && debounce.contains("history_rx.is_some()"),
        "typing must not spawn a walk of every thread per keystroke: {debounce}"
    );
    assert!(
            debounce.contains("self.history_hits.clear()"),
            "a new query must drop the previous needle's hits so a click cannot open the wrong thread: {debounce}"
        );
    let poll = src
        .split("fn poll_history_search(")
        .nth(1)
        .and_then(|s| s.split("fn poll_mem_restore(").next())
        .expect("poll_history_search");
    assert!(
        poll.contains("q == self.history_q"),
        "a finished walk must not install hits for a query the box no longer holds: {poll}"
    );
    let search = src
        .split("fn kick_history_search(")
        .nth(1)
        .and_then(|s| s.split("fn open_history_hit(").next())
        .expect("history search");
    assert!(
        search.contains("write_memory")
            && search.contains("mem_body")
            && search.contains("scratch()"),
        "History Search must flush the Memory editor before reading disk: {search}"
    );
    assert!(
        search.contains("thread::spawn"),
        "History Search must flush MEMORY.md off the UI thread: {search}"
    );
    assert!(
        search.contains("TEXT_FILE_CAP") || search.contains("search_thread_body"),
        "History Search must not join every 8MB thread on the UI thread: {search}"
    );
    assert!(
        search.contains("thread_idx") && search.contains("self.messages"),
        "History Search must include the live pane, not only persisted thread copies: {search}"
    );
    assert!(
        !search.contains("content.clone()"),
        "History Search must not clone an 8MB pane to include the live tab: {search}"
    );
    let soul = search
        .find("read_memory(\"SOUL.md\")")
        .expect("history soul");
    assert!(
        search[..soul].contains("thread::spawn") && search.contains("history_rx"),
        "History Search must slurp SOUL/USER/MEMORY off the UI thread: {search}"
    );
    assert!(
        search.contains("search_corpus_tagged")
            && search.contains("format!(\"thread:{}\", t.id)")
            && search.contains("mem:MEMORY.md"),
        "every hit must carry the thread or file it came from: {search}"
    );
    let hit = src
        .split("fn open_history_hit(")
        .nth(1)
        .and_then(|s| s.split("fn open_memory_file(").next())
        .expect("open_history_hit");
    assert!(
        hit.contains("open_memory_file") && hit.contains("Nav::Memory"),
        "a memory hit opens that file in the editor: {hit}"
    );
    assert!(
        hit.contains("switch_thread") && hit.contains("Nav::Chat"),
        "a chat hit opens the thread it came from: {hit}"
    );
    assert!(
        hit.contains("That chat is gone"),
        "a hit for a deleted thread must say so, not open the wrong chat: {hit}"
    );
    let board = fn_src(&src, "ui_board");
    assert!(
        board.contains("self.flush_board()")
            && !board.contains("self.persist()")
            && !board.contains("persist_snap"),
        "Workboard add/status must not clone every thread just to write board.json: {board}"
    );
    let flush_b = fn_src(&src, "flush_board");
    assert!(
            flush_b.contains("persist_idle_now") && flush_b.contains("save_board"),
            "Workboard flush must bump the idle key or persist_bg clones every thread 2s later: {flush_b}"
        );
    let native_src = include_str!("native_engine.rs").replace("\r\n", "\n");
    let kick = fn_src(&native_src, "kick_native_turn");
    let prompt_at = kick.find("prompt_with_image").expect("prompt");
    let note_at = kick
        .find("note_inflight_card")
        .expect("run start files a Workboards card");
    assert!(
        prompt_at < note_at,
        "Doing card is filed only after the prompt succeeds: {kick}"
    );
    assert!(
        kick.contains("self.fail_turn_start(")
            && fn_src(&src, "fail_turn_start").contains("abandon_turn_card")
            && fn_src(&src, "poll_acp").contains("abandon_turn_card")
            && fn_src(&src, "poll_single").contains("abandon_turn_card"),
        "prompt and stream failures undo the Doing card"
    );
    let finish = fn_src(&src, "finish_acp_turn");
    assert!(
        finish.contains("settle_turn_card"),
        "run complete settles that card: {finish}"
    );
    let note = fn_src(&src, "note_inflight_card");
    assert!(
        note.contains("upsert_inflight_card") && note.contains("flush_board"),
        "inflight hook writes workboard.json, not a full thread persist: {note}"
    );
    let settled = fn_src(&src, "settle_turn_card");
    assert!(
        settled.contains("settle_inflight_card")
            && settled.contains("apply_assistant_work_marks")
            && settled.contains("flush_board"),
        "complete moves doing to done and applies WORK_PIN lines: {settled}"
    );
    let menu = fn_src(&src, "board_card_menu");
    assert!(
        board.contains("Workboards")
            && board.contains("paint_board_card")
            && menu.contains("Open chat")
            && menu.contains("Archive"),
        "Workboards page is the kanban, with a thread link in each card's menu: {board}"
    );
    let open = fn_src(&src, "open_board_thread");
    assert!(
        open.contains("switch_thread") && open.contains("Nav::Chat"),
        "Open chat leaves the board on the rail and shows the linked thread: {open}"
    );
    let night = format!(
        "{}{}",
        fn_src(&src, "ui_night"),
        fn_src(&src, "ui_scheduled_automations")
    );
    assert!(
        night.contains("merge_suggested_autos"),
        "Loops Suggested uses learned tiles first: {night}"
    );
    assert!(
        night.contains("Suggested from your recent work"),
        "Suggested says these tiles come from recent work: {night}"
    );
    assert!(
        !night.contains("review_status_line"),
        "night review status stays off the Automations Suggested line: {night}"
    );
    assert!(
        night.contains("/loop") && night.contains("New job") && night.contains("grok_loops"),
        "Automations page still owns the Grok Build /loop list: {night}"
    );
    assert!(
        !night.contains("Follow along")
            && !night.contains("Teach this once")
            && !night.contains("teach_watched_routine")
            && !night.contains("watch_once")
            && !night.contains("teach_nl")
            && night.contains("New job")
            && night.contains("Suggested")
            && night.contains("Loops")
            && !night.contains("thread::spawn"),
        "Automations must drop Follow along / Teach this once: {night}"
    );
    assert!(
        night.contains("ui_scheduled_automations") && night.contains("self.automations"),
        "Automations page must also show the clock jobs the pulse fires: {night}"
    );
    let sched = fn_src(&src, "ui_scheduled_automations");
    assert!(
        sched.contains("automation_summary_line")
            && sched.contains("fire_night")
            && sched.contains("persist_automations")
            && !sched.contains("self.persist()"),
        "a clock job needs its schedule, Run, Remove, and an off-thread persist: {sched}"
    );
    assert!(
        sched.contains("ensure_automation_schedule"),
        "re-enabling a paused job has to find its next slot: {sched}"
    );
    let enable = night
        .split("enabled_switch")
        .nth(1)
        .and_then(|s| s.split("ui.vertical").next())
        .expect("loop enable");
    assert!(
        enable.contains("persist_loops"),
        "toggling a loop must persist enabled before restart: {enable}"
    );
    assert!(
            night.contains("persist_loops") && !night.contains("self.persist()"),
            "removing a loop must not clone every thread 2s later — persist_loops bumps the idle key: {night}"
        );
    let added = src
        .split("fn add_automation_seed(")
        .nth(1)
        .and_then(|s| s.split("fn ui_night(").next())
        .expect("add_automation_seed");
    assert!(
        added.contains("save_schedule") && added.contains("persist_loops"),
        "Add must route the seed and persist loops.json off the UI thread: {added}"
    );
    assert!(
        added.contains("dismiss_accepted_auto") && added.contains("persist_suggestions"),
        "Accept must drop the Suggested tile from store + persist every time: {added}"
    );
    assert!(
        !added.contains("self.persist()"),
        "Add must not clone every thread 2s later: {added}"
    );
    assert!(
        !night.contains("take(40)"),
        "loop and scheduled titles wrap — no mid-word take(40): {night}"
    );
    let fire = fn_src(&src, "fire_loop");
    let unattended = include_str!("native_unattended.rs").replace("\r\n", "\n");
    let native_loop = fn_src(&unattended, "spawn_native_loop");
    let gate = fn_src(&unattended, "scheduled_gate");
    assert!(
        fire.contains("spawn_native_loop")
            && native_loop.contains("thread::spawn")
            && native_loop.contains("scheduled_gate(self")
            && gate.contains("cabin.permission_mode")
            && gate.contains("attended: false")
            && gate.contains("mode: perm,"),
        "loop Run must inherit the PermissionMode pill — no silent always-approve: {fire}"
    );
    let skills = src
        .split("fn ui_skills(")
        .nth(1)
        .and_then(|s| s.split("fn tick_history_search(").next())
        .expect("ui_skills");
    assert!(
        skills.contains("Project and user skills")
            && skills.contains("native_skills")
            && !skills.contains("Grok Build skills")
            && !skills.contains("grok_catalog"),
        "Skills shows the native engine's skills, never the CLI catalog: {skills}"
    );
    let connectors = include_str!("connectors_ui.rs")
        .split("#[cfg(test)]")
        .next()
        .expect("connectors body");
    assert!(
        connectors.contains("\"This cabin\"")
            && !connectors.contains("\"Grok Build\"")
            && !connectors.contains("run_grok_user_cmd")
            && !connectors.contains("run_mcp_doctor"),
        "Settings, Connectors lists the cabin's own servers and never runs the CLI: {connectors}"
    );
    assert!(
        !skills.contains("merge_suggested_skills") && !skills.contains("add_suggested_skill"),
        "Suggested skills live on the Ideas board, not the Skills page: {skills}"
    );
    let github = fn_src(&src, "ui_github_connector");
    assert!(
        github.contains("GITHUB_TILES")
            && github.contains("Save PAT")
            && github.contains("run_connector")
            && github.contains("github_token")
            && !github.contains("create_pr")
            && !github.contains("outlook")
            && !github.contains("gmail"),
        "Connectors GitHub tiles + PAT must stay read-only: {github}"
    );
    let add_skill = fn_src(&src, "add_suggested_skill");
    assert!(
        add_skill.contains("skill_from_suggestion")
            && add_skill.contains("save_skill")
            && add_skill.contains("thread::spawn"),
        "Suggested Add must write SKILL.md off the UI thread: {add_skill}"
    );
    assert!(
        !src.contains("fn send_grok_slash("),
        "no slash is sent to the engine as a literal CLI command"
    );
    let skills = src
        .split("fn ui_skills(")
        .nth(1)
        .and_then(|s| s.split("fn project_row_active(").next())
        .expect("ui_skills");
    assert!(
        !skills.contains("workflow_status_live") && !skills.contains("send_workflow_ctl"),
        "Workflows were a CLI feature; the Skills page has no Runs strip: {skills}"
    );
}

#[test]
fn stream_deltas_do_not_grow_without_bound() {
    let src = cabin_src();
    let poll = fn_src(&src, "poll_single");
    let ingest = fn_src(&src, "ingest_stream_chunk");
    assert!(
        poll.contains("GrokPEvent::Thought")
            && poll.contains("GrokPEvent::Text")
            && poll.contains("ingest_stream_chunk")
            && ingest.contains("push_stream_capped")
            && ingest.contains("IMAGE_FILE_CAP"),
        "live grok -p thought and text deltas must not grow stream buffers without bound: {poll}"
    );
    let snap = fn_src(&src, "apply_assistant_snapshot");
    assert!(
        snap.contains("take_ui_text") && snap.contains("IMAGE_FILE_CAP"),
        "a huge complete reply must not land in the transcript unbounded: {snap}"
    );
    let live = fn_src(&src, "apply_live_assistant");
    assert!(
            live.contains("merge_thinking_capped") && live.contains("TEXT_FILE_CAP"),
            "live thought+stream merge must not copy an 8MB stream into the transcript every delta: {live}"
        );
    let finish = fn_src(&src, "finish_acp_turn");
    assert!(
        finish.contains("take_ui_text") && finish.contains("IMAGE_FILE_CAP"),
        "complete grok -p text must be capped before the UI thread merges: {finish}"
    );
    let apply = fn_src(&src, "apply_single_turn");
    assert!(
        apply.contains("merge_thinking_capped") && apply.contains("TEXT_FILE_CAP"),
        "single-turn merge must stay under TEXT_FILE_CAP: {apply}"
    );
}

#[test]
fn chat_arm_checks_stream_end_followup() {
    let src = cabin_src();
    let apply = fn_src(&src, "apply_single_turn");
    let finish = fn_src(&src, "finish_acp_turn");
    let drain = fn_src(&src, "drain_followup_queue");
    let poll = fn_src(&src, "poll_single");
    assert!(
        finish.contains("take_ui_text") && finish.contains("IMAGE_FILE_CAP"),
        "complete must not strip/merge a 64MB worker body on the UI thread: {finish}"
    );
    assert!(
        apply.contains("drain_followup_queue"),
        "stream-end follow-up belongs on the grok -p complete path: {apply}"
    );
    assert!(
        drain.contains("send_chat"),
        "a queued composer follow-up still uses the typed send path: {drain}"
    );
    assert!(
            poll.contains("mem::take")
                && !poll.contains("stream_buf.clone()")
                && !poll.contains("thought_buf.clone()"),
            "disconnect complete must take the stream buffers, not clone an 8MB complete on the UI thread: {poll}"
        );
    assert!(
        finish.contains("self.persist()") && finish.contains("chat_job_thread"),
        "complete must persist the origin thread: {finish}"
    );
    let mid = fn_src(&src, "tick_mid_thought");
    assert!(
        !mid.contains("send_chat") && !mid.contains("send_followup_turn"),
        "MidThought must not auto-continue chat: {mid}"
    );
}

#[test]
fn mid_thought_stays_out_of_chat() {
    let src = cabin_src();
    let impl_src = src.as_str();
    assert!(
        !impl_src.contains("You sit down. Last night"),
        "MidThought must not inject a fake assistant turn"
    );
    let mid = fn_src(&src, "tick_mid_thought");
    assert!(
        !mid.contains("send_chat") && !mid.contains("Nav::Chat") && !mid.contains("self.running"),
        "MidThought stays quiet: {mid}"
    );
    assert!(
        mid.contains("continue_thread_hint"),
        "MidThought folds Continue {{title}} into the greeting path: {mid}"
    );
    let hint = fn_src(&src, "last_night_hint");
    assert!(
        !hint.contains("messages.push") && !hint.contains("send_chat"),
        "last-night context stays in the greeting: {hint}"
    );
    assert!(
        hint.contains("continue_hint"),
        "empty last-night falls back to continue hint: {hint}"
    );
    assert!(
        src.contains("last_night: &last_night")
            || src.contains("last_night: &self.last_night_hint()")
    );
    assert!(src.contains("self.tick_mid_thought()"));
}

#[test]
fn chat_rail_reuses_empty_draft() {
    let src = cabin_src();
    let theme = include_str!("../theme.rs");
    let chat = theme.find("(\"chat\", \"Chat\")").expect("chat rail");
    let imagine = theme
        .find("(\"imagine\", \"Imagine\")")
        .expect("imagine rail");
    assert!(chat < imagine, "Chat sits above Imagine on the rail");
    let set_nav = fn_src(&src, "set_nav_id");
    let chat_arm = set_nav
        .split("\"chat\" =>")
        .nth(1)
        .and_then(|s| s.split("_ =>").next())
        .expect("chat arm");
    assert!(
        chat_arm.contains("self.new_thread(false)"),
        "Chat rail click reuses or starts one empty draft: {chat_arm}"
    );
    assert!(
        !chat_arm.contains("open_recent_chat"),
        "Chat rail must not jump to last-access; History is for old convos: {chat_arm}"
    );
    let created = src
        .split("fn new_thread")
        .nth(1)
        .and_then(|s| s.split("fn begin_chat_rename").next())
        .expect("new_thread");
    assert!(
        created.contains("reuse_empty_thread_idx") && created.contains("has_session"),
        "Chat must reuse an empty no-session draft instead of stacking Chats: {created}"
    );
    assert!(
        created.contains("composer_want_focus = true"),
        "Chat rail must put the cursor in the composer: {created}"
    );
    let side = fn_src(&src, "ui_sidebar");
    assert!(
        !side.contains("\"New chat\"") && !side.contains("RailIcon::Compose"),
        "sidebar must not keep a separate New chat button: {side}"
    );
    assert!(
        side.contains("apply_tab_act"),
        "sidebar History clicks go through the shared chat-row action: {side}"
    );
    let tab_act = fn_src(&src, "apply_tab_act");
    assert!(
        tab_act.contains("composer_want_focus = true"),
        "sidebar History clicks must focus the composer: {tab_act}"
    );
    let palette = src
        .split("fn run_palette(")
        .nth(1)
        .and_then(|s| s.split("fn run_slash_line(").next())
        .expect("run_palette");
    assert!(
        palette.contains("\"nav:chat\"") && palette.contains("self.new_thread(false)"),
        "palette Chat uses the same empty-draft reuse as the rail: {palette}"
    );
    assert!(
        palette.contains("file:")
            && palette.contains("palette_file_shown")
            && palette.contains("desktop::open_path"),
        "picking a palette file must open it, not only write status: {palette}"
    );
    let land = src
        .split("fn land_on_real_chat(")
        .nth(1)
        .and_then(|s| s.split("fn new_thread(").next())
        .expect("land_on_real_chat");
    assert!(
        land.contains("scratch()") && land.contains("apply_switch_thread"),
        "background chat must leave Scratch for the last real thread: {land}"
    );
    assert!(
        land.contains("most_recently_accessed_index") && !land.contains("self.persist()"),
        "leaving Scratch for a night/inbox job must not clone every thread twice: {land}"
    );
    let house = src
        .split("HeartbeatAct::Housekeep =>")
        .nth(1)
        .and_then(|s| s.split("HeartbeatAct::Inbox =>").next())
        .expect("housekeep");
    assert!(
        !house.contains("stamp_current_access"),
        "sitting on a chat is not activity and must not move that History row: {house}"
    );
    let idle = src
        .split("JobKind::Reflect =>")
        .nth(1)
        .and_then(|s| s.split("JobKind::Indexers =>").next())
        .expect("idle reflect");
    assert!(
        idle.contains("scratch()"),
        "idle reflect must not consume the slot on Scratch: {idle}"
    );
    let mut older = crate::threads::ChatThread::new("Older", false);
    older.accessed_ms = 1_000;
    let mut newer = crate::threads::ChatThread::new("Night cabin", false);
    newer.accessed_ms = 8_000;
    let mut scratch = crate::threads::ChatThread::new("Scratch", true);
    scratch.accessed_ms = 9_000;
    assert_eq!(
        crate::threads::most_recently_accessed_index(&[older, newer, scratch]),
        Some(1)
    );
}

#[test]
fn empty_home_pulse_stays_off_scratch_and_off_about() {
    let src = cabin_src();
    let home = src
        .split("fn ui_empty_home")
        .nth(1)
        .and_then(|s| s.split("fn ui_composer_stack(").next())
        .expect("empty home");
    assert!(
        home.contains("pulse_should_paint") && !home.contains("vertical_centered_justified"),
        "the home slot must not re-center the chip row: {home}"
    );
    assert!(
        !home.contains("grok_tile("),
        "full grok_tiles shove the composer off a short cabin: {home}"
    );
    let about = src.split("SettingsSec::About").nth(1).unwrap_or("");
    assert!(
        !about.contains("usage_line") && !about.contains("paint_empty_pulse"),
        "About must not grow today's buckets: {about}"
    );
    assert!(
        src.contains("Nav::Workboard") && src.contains("Nav::Night"),
        "pulse clicks reuse Workboard and Automations"
    );
    assert!(
        home.contains("paint_device_glance_row")
            && home.contains("pulse_should_paint")
            && !home.contains("paint_update_feed")
            && !home.contains("paint_home_deck_over_chat")
            && !home.contains("collapsed_stack_h")
            && !home.contains("home_feed_count")
            && !home.contains("stacked_feed_h")
            && !home.contains("paint_lane_chip")
            && !home.contains("No updates"),
        "empty home keeps device glance and does not paint the Home card deck: {home}"
    );
    assert!(
        !home.contains("\"Personal\"") && !home.contains("Nav::"),
        "home chrome must not add a Personal rail or nav id: {home}"
    );
    let feed = include_str!("feed_ui.rs");
    assert!(
        feed.contains("note_automation_done")
            && feed.contains("note_schedule_created")
            && feed.contains("close_home_card")
            && feed.contains("apply_feed_act")
            && !feed.contains("paint_update_feed")
            && !feed.contains("paint_home_deck_over_chat")
            && !feed.contains("HOME_STACK_SHOW")
            && !feed.contains("Order::Foreground")
            && !feed.contains("No updates")
            && !feed.contains("interest_update"),
        "feed posting and card actions stay; Home deck paint is gone: {feed}"
    );
    let night = include_str!("night.rs");
    let poll = night
        .split("fn poll_grok_loop(")
        .nth(1)
        .and_then(|s| s.split("fn fire_loop(").next())
        .expect("poll_grok_loop");
    assert!(
        poll.contains("note_automation_done"),
        "a finished /loop posts automation_done: {poll}"
    );
    let commit = night
        .split("fn commit_schedule(")
        .nth(1)
        .and_then(|s| s.split("fn ui_night(").next())
        .expect("commit_schedule");
    assert!(
        commit.contains("note_schedule_created"),
        "saving a schedule posts schedule_created: {commit}"
    );
    let title = src
        .split("fn ui_titlebar(")
        .nth(1)
        .and_then(|s| s.split("fn nav_row(").next())
        .expect("titlebar");
    assert!(
        title.contains("quiet_until_chip") && title.contains("SettingsSec::Behavior"),
        "quiet-hours chip is titlebar chrome; Behavior stays SoT: {title}"
    );
    let hist = src
        .split("fn ui_history(")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("history");
    assert!(
        hist.contains("session_markers")
            && hist.contains("jump_last_you")
            && hist.contains("apply_switch_thread")
            && !hist.contains("self.thread_idx = i"),
        "History map must swap the visible thread, not only the index: {hist}"
    );
    let reserved = src
        .split("if reserve_offscreen_chat_row(ui, cached_h)")
        .nth(1)
        .and_then(|s| s.split("let y0 = ui.cursor().min.y").next())
        .expect("reserved last-you");
    assert!(
        reserved.contains("scroll_to_rect")
            && reserved.contains("jump_you")
            && reserved.contains("last_you_i"),
        "Last you must scroll a reserved off-screen row: {reserved}"
    );
    let auto = src
        .split("Slash::AutoPerm =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Effort =>").next())
        .expect("AutoPerm");
    assert!(
        auto.contains("self.confirm = None"),
        "/auto must drop a session Always overlay: {auto}"
    );
    // rustfmt may break `let row = crate::cards::session_row(...)` across lines.
    let row = src
        .split("crate::cards::session_row(")
        .nth(1)
        .and_then(|s| s.split("ui.allocate_ui_with_layout").next())
        .expect("session_row");
    let perm = row
        .split("if let Some(perm) = row.perm")
        .nth(1)
        .and_then(|s| s.split("if row.why").next())
        .expect("perm pills");
    let ask_auto = perm
        .split("self.arm_session_always();")
        .nth(1)
        .expect("Ask/Auto after Always");
    assert!(
        ask_auto.contains("self.confirm = None"),
        "Auto/Ask must disarm the session Always overlay: {ask_auto}"
    );
}

#[test]
fn cabin_2109_must_ships() {
    let src = cabin_src();
    let night = fn_src(&src, "ui_night");
    assert!(
        !night.contains("Follow along") && !night.contains("Teach this once"),
        "{night}"
    );
    assert!(
        night.contains("New job")
            && night.contains("Loops")
            && night.contains("Suggested")
            && night.contains("ui_scheduled_automations"),
        "{night}"
    );
    let added = fn_src(&src, "add_automation_seed");
    assert!(
        added.contains("dismiss_accepted_auto") && added.contains("persist_suggestions"),
        "{added}"
    );
    let session = fn_src(&src, "tick_session_suggestions");
    assert!(
        session.contains("suggestions_from_sessions")
            && session.contains("persist_suggestions")
            && session.contains("last_session_suggest_day")
            && !session.contains("send_chat"),
        "{session}"
    );
    let review = fn_src(&src, "tick_review");
    assert!(review.contains("tick_session_suggestions"), "{review}");
    let chips = include_str!("chips.rs");
    let chip_row = chips
        .split("pub fn quick_chip_row(")
        .nth(1)
        .and_then(|s| s.split("// quick_chip_row_end").next())
        .expect("quick_chip_row");
    assert!(
        chip_row.contains("with_main_wrap(false)")
            && chip_row.contains("fluid_chip_count")
            && chip_row.contains("layout_chip_label")
            && chip_row.contains("CHIP_ROW_H")
            && chip_row.contains("chip_row_visible_w")
            && !chip_row.contains("CHIP_CLUSTER_H")
            && !chip_row.contains("with_main_wrap(true)"),
        "chips are one fixed line, ellipsized, and drop overflow inside Ask anything"
    );
}

#[test]
fn settings_cabin_defaults_section() {
    let settings = include_str!("settings.rs");
    assert!(
        settings.contains("(SettingsSec::Defaults, \"Cabin defaults\")"),
        "Cabin defaults is a Settings section"
    );
    let defaults = settings
        .split("SettingsSec::Defaults => {")
        .nth(1)
        .and_then(|s| s.split("if let Some(s) = next_sec").next())
        .expect("defaults arm");
    for label in ["ROW_MODEL", "ROW_PERMISSION", "ROW_SESSION", "ROW_COLLAPSE"] {
        assert!(defaults.contains(label), "missing {label}: {defaults}");
    }
    assert_eq!(defaults.matches("settings_dropdown").count(), 3);
    assert!(defaults.contains("settings_toggle"));
    assert!(
        !defaults.contains("effort"),
        "R1: no effort row: {defaults}"
    );
    assert!(defaults.contains("cabin_default_model_id"));
    assert!(defaults.contains("set_permission_mode"));
    assert!(defaults.contains("set_session_mode"));
    assert!(defaults.contains("always_collapse_thoughts"));
    assert!(defaults.contains("persist_cfg"));
    assert!(!defaults.contains("always-approve"));
    assert!(!defaults.contains("grok_tile"));
    assert!(!defaults.contains("quick_chip_row"));
    let cards = include_str!("../cards.rs");
    assert!(
        cards.contains("pub fn session_row(") && cards.contains("(\"always-approve\", \"Always\")"),
        "composer pills stay the live session controls, including Always"
    );
}

#[test]
fn settings_permissions_editor_lists_rules_and_grants() {
    let settings = include_str!("settings.rs");
    assert!(settings.contains("(SettingsSec::Permissions, \"Permissions\")"));
    assert!(settings.contains("SettingsSec::Permissions => \"Permissions\""));
    assert!(settings.contains("SettingsSec::Permissions => self.ui_permission_editor(ui)"));
    assert!(settings.contains("fn ui_permission_editor"));
    assert!(settings.contains("load_rules"));
    assert!(settings.contains("load_grants"));
    assert!(settings.contains("load_claude_project"));
    assert!(settings.contains("remember_allow_always"));
    let chat = include_str!("chat_ui.rs");
    assert!(!chat.contains("ui_permission_editor"));
    assert!(!chat.contains("SettingsSec::Permissions"));
    let defaults = settings
        .split("SettingsSec::Defaults => {")
        .nth(1)
        .and_then(|slice| slice.split("if let Some(s) = next_sec").next())
        .expect("defaults arm");
    // Model, Permission and Session mode. R1 removed the effort dropdown.
    assert_eq!(defaults.matches("settings_dropdown").count(), 3);
}

#[test]
fn session_thought_collapse_stays_on_one_thread() {
    let ctx = egui::Context::default();
    let a = "thread-a";
    let b = "thread-b";
    assert!(!super::read_session_thoughts_collapsed(&ctx, a));
    assert!(!super::read_session_thoughts_collapsed(&ctx, b));
    let one = grokhub_core::thought_body_key("need a snapshot");
    let two = grokhub_core::thought_body_key("of the restore path");
    let bee = grokhub_core::thought_body_key("session b stays open");
    let id = |thread, key| super::thought_fold_id(thread, "body", key);
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(a, one), false),
        grokhub_core::ThoughtFold::Expanded
    );
    super::write_session_thoughts_collapsed(&ctx, a, true);
    super::write_thought_fold(&ctx, id(a, one), grokhub_core::ThoughtFold::Minimized);
    super::write_thought_fold(&ctx, id(a, two), grokhub_core::ThoughtFold::Minimized);
    assert!(super::read_session_thoughts_collapsed(&ctx, a));
    assert!(!super::read_session_thoughts_collapsed(&ctx, b));
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(b, bee), false),
        grokhub_core::ThoughtFold::Expanded,
        "session B stays expanded when Always collapse is off"
    );
    let fresh = grokhub_core::thought_body_key("a new thought in A");
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(a, fresh), true),
        grokhub_core::ThoughtFold::Minimized,
        "a new thought in a collapsed session arrives folded"
    );
    super::write_thought_fold(&ctx, id(a, one), grokhub_core::ThoughtFold::Expanded);
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(a, one), true),
        grokhub_core::ThoughtFold::Expanded
    );
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(a, two), true),
        grokhub_core::ThoughtFold::Minimized,
        "expand opens one thought"
    );
    assert!(super::read_session_thoughts_collapsed(&ctx, a));
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(b, bee), true),
        grokhub_core::ThoughtFold::Minimized,
        "Always collapse starts every session folded"
    );
    let chat = include_str!("chat_ui.rs");
    assert_eq!(
        grokhub_core::thought_fold_controls(grokhub_core::ThoughtFold::Expanded),
        &["Collapse"]
    );
    assert_eq!(
        grokhub_core::thought_fold_controls(grokhub_core::ThoughtFold::Minimized),
        &["Expand"]
    );
    assert!(grokhub_core::thought_fold_controls(grokhub_core::ThoughtFold::Hidden).is_empty());
    assert!(
        chat.contains("minimize_session_thoughts")
            && chat.contains("session_thoughts_start_collapsed")
            && !chat.contains("paint_thought_fold_buttons(ui, \"Hide\")"),
        "quiet Collapse/Expand stays; Hide is not painted"
    );
}

#[test]
fn feed_pulse_and_fresh_home_stay_off_the_review() {
    let src = cabin_src();
    let house = src
        .split("HeartbeatAct::Housekeep =>")
        .nth(1)
        .and_then(|s| s.split("HeartbeatAct::Inbox =>").next())
        .expect("housekeep");
    assert!(!house.contains("stamp_current_access"), "{house}");
    let pulse = src
        .split("JobKind::PulseRefresh =>")
        .nth(1)
        .and_then(|s| s.split("JobKind::ModelRegistry =>").next())
        .expect("pulse refresh job");
    assert!(
        pulse.contains("tick_feed_pulse") && !pulse.contains("stamp_current_access"),
        "expiry, quiet release, and the digest clock run on the Pulse refresh job without moving History: {pulse}"
    );
    let tick = src
        .split("fn tick_review(")
        .nth(1)
        .and_then(|s| s.split("fn review_digest(").next())
        .expect("tick_review");
    assert!(
        !tick.contains("tick_feed_pulse") && !tick.contains("expire_ideas"),
        "the feed pulse must not run from the nightly review: {tick}"
    );
    let spawn = src
        .split("fn spawn_review(")
        .nth(1)
        .and_then(|s| s.split("fn poll_review(").next())
        .expect("spawn_review");
    assert!(
        !spawn.contains("tick_feed_pulse")
            && !spawn.contains("expire_ideas")
            && !spawn.contains("suggestions_from_sessions"),
        "spawn_review stays the transcript review: {spawn}"
    );
    let show = src
        .split("fn show_from_tray(")
        .nth(1)
        .and_then(|s| s.split("fn note_window_resume(").next())
        .expect("show_from_tray");
    assert!(
        show.contains("open_fresh_home") && show.contains("apply_saved_geom"),
        "tray show opens a fresh chat and restores the window: {show}"
    );
    let fresh = fn_src(&src, "open_fresh_home");
    assert!(
        fresh.contains("new_thread(false)")
            && fresh.contains("park_fresh_chat")
            && fresh.contains("resume_needs_fresh_chat")
            && !fresh.contains("halt_in_flight")
            && !fresh.contains("drop_leaving_thread_chrome")
            && !fresh.contains("delete_thread")
            && !fresh.contains("pinned = false")
            && !fresh.contains("No updates")
            && !fresh.contains("Nothing queued"),
        "fresh home keeps the previous chat and does not stop a live reply: {fresh}"
    );
    let park = fn_src(&src, "park_fresh_chat");
    assert!(
        park.contains("self.messages.clone()")
            && !park.contains("halt_in_flight")
            && !park.contains("drop_leaving_thread_chrome")
            && !park.contains("delete_thread")
            && !park.contains("pinned = false"),
        "parking a fresh chat leaves the running thread alone: {park}"
    );
    let pulse = include_str!("feed_ui.rs");
    let body = pulse
        .split("fn tick_feed_pulse(")
        .nth(1)
        .and_then(|s| s.split("fn digest_taste(").next())
        .expect("tick_feed_pulse");
    assert!(
        body.contains("read_memory(\"USER.md\")")
            && body.contains("read_memory(\"MEMORY.md\")")
            && body.contains("read_memory(\"SOUL.md\")")
            && !body.contains("last_review_day")
            && !body.contains("suggestions_from_sessions")
            && !body.contains("spawn_review")
            && !body.contains("notify::")
            && !body.contains("send_chat")
            && !body.contains("send_scheduled")
            && !body.contains("grok_chat"),
        "digest authoring stays off transcripts, chat, and pings: {body}"
    );
    let ping = pulse
        .split("fn release_situation_ping(")
        .nth(1)
        .and_then(|s| s.split("fn apply_feed_act(").next())
        .expect("release_situation_ping");
    assert!(
        ping.contains("notify::ping")
            && ping.contains("quiet_now")
            && ping.contains("window_focused")
            && ping.contains("GrokHub"),
        "only the situation card pings, and only when the window is away and quiet hours are off: {ping}"
    );
    assert!(
        !pulse.contains("No updates") && !pulse.contains("Nothing queued"),
        "the feed must not paint an empty label"
    );
    let ideas = include_str!("ideas_ui.rs")
        .split("fn ui_ideas(")
        .nth(1)
        .and_then(|s| s.split("fn paint_idea_open(").next())
        .expect("ui_ideas");
    assert!(
        ideas.contains("Suggest ideas") && ideas.contains("maybe_suggest_ideas(true)"),
        "the Ideas button asks the model for ideas now, not a disk reload: {ideas}"
    );
    let offer = pulse
        .split("fn accept_automate_offer(")
        .nth(1)
        .and_then(|s| s.split("fn ensure_useful_ideas(").next())
        .expect("accept_automate_offer");
    assert!(
        offer.contains("c.title.clone()")
            && offer.contains("route_schedule")
            && !offer.contains("c.body"),
        "Accept seeds the schedule from the title, not the canned offer body: {offer}"
    );
    let build = pulse
        .split("fn build_idea(")
        .nth(1)
        .and_then(|s| s.split("fn accept_automate_offer(").next())
        .expect("build_idea");
    assert!(
        build.contains("idea_todo_title(")
            && build.contains("file_idea_todo(&mut self.board, &task, \"\")")
            && !build.contains("idea.body.clone()"),
        "Idea Accept files the task line, not the raw message: {build}"
    );
    assert!(
        pulse.contains("FeedAct::Discuss")
            && pulse.contains("FeedAct::Dismiss")
            && pulse.contains("FeedAct::Open")
            && !pulse.contains("FeedAct::Archive")
            && pulse.contains("fn close_home_card(")
            && !pulse.contains("fn paint_feed_card(")
            && !pulse.contains("fn paint_update_feed("),
        "feed card acts and close_home_card remain; Home deck paint is gone: {pulse}"
    );
    let offer_open = pulse
        .split("fn open_offer_on_automations(")
        .nth(1)
        .and_then(|s| s.split("fn accept_automate_offer(").next())
        .expect("open_offer_on_automations");
    assert!(
        offer_open.contains("self.night_nl = seed")
            && offer_open.contains("self.auto_compose = true")
            && offer_open.contains("Nav::Night")
            && !offer_open.contains("commit_schedule")
            && !offer_open.contains("route_schedule"),
        "an offer click opens the schedule box and does not save it: {offer_open}"
    );
}

/// Sidebar History order: pins last-pinned-first, then last used.
fn history_row_titles(cabin: &Cabin) -> Vec<String> {
    let listed = crate::threads::chat_section_indices(
        &cabin.threads,
        Some(cabin.thread_idx),
        cabin.messages.is_empty(),
    );
    let keys: Vec<crate::threads::SessionSortKey> = listed
        .iter()
        .map(|&i| {
            let t = &cabin.threads[i];
            crate::threads::SessionSortKey {
                pinned: t.pinned,
                pinned_ms: t.pinned_ms,
                accessed_ms: t.accessed_ms,
                list_rank: 0,
            }
        })
        .collect();
    crate::threads::session_list_order(&keys)
        .into_iter()
        .map(|pos| cabin.threads[listed[pos]].title.clone())
        .collect()
}

fn spoke(title: &str, accessed_ms: u64) -> crate::threads::ChatThread {
    let mut thread = crate::threads::ChatThread::new(title, false);
    thread.accessed_ms = accessed_ms;
    thread
        .messages_mut()
        .push(("user".into(), format!("keep {title}")));
    thread
}

#[test]
fn opening_a_history_row_keeps_its_place() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("history-click");
    let _ = std::fs::remove_dir_all(&root);
    let _cfg = RestoreEnv::set("GROKHUB_CONFIG", &root);
    let _tray = RestoreEnv::set("GROKHUB_TRAY", "0");

    let mut last_pin = spoke("Last pin", 1);
    last_pin.pinned = true;
    last_pin.pinned_ms = 80;
    let mut earlier_pin = spoke("Earlier pin", 9_000);
    earlier_pin.pinned = true;
    earlier_pin.pinned_ms = 20;
    let newer = spoke("Newer chat", 500);
    let older = spoke("Older chat", 100);
    let earlier_idx = 1;
    let newer_idx = 2;
    let older_idx = 3;
    let mut cabin = Cabin::quiet_cabin(vec![last_pin, earlier_pin, newer, older], newer_idx);
    assert!(!cabin.running, "the cabin is quiet before the click");

    let before = history_row_titles(&cabin);
    assert_eq!(
        before,
        ["Last pin", "Earlier pin", "Newer chat", "Older chat"],
        "pins stay last-pinned-first, then last used: {before:?}"
    );
    let opened_at = before.iter().position(|t| t == "Older chat").unwrap();
    assert_ne!(opened_at, 0, "the chat we open is not already first");
    let older_accessed = cabin.threads[older_idx].accessed_ms;
    let earlier_pin_ms = cabin.threads[earlier_idx].pinned_ms;
    let earlier_accessed = cabin.threads[earlier_idx].accessed_ms;

    cabin.switch_thread(older_idx);
    let after_click = history_row_titles(&cabin);
    assert_eq!(
        after_click, before,
        "switch_thread is the History click and must not move the row: {after_click:?}"
    );
    assert_eq!(cabin.thread_idx, older_idx);
    assert_eq!(
        cabin.threads[older_idx].accessed_ms, older_accessed,
        "the click must not bump accessed_ms"
    );

    // send_chat stamps the open thread with stamp_current_access. That is the
    // sent-turn bump. The click above did not call it.
    cabin.stamp_current_access();
    let after_stamp = history_row_titles(&cabin);
    assert_eq!(
        after_stamp,
        ["Last pin", "Earlier pin", "Older chat", "Newer chat"],
        "a sent turn moves that chat up, under the pins: {after_stamp:?}"
    );
    let stamped_at = after_stamp.iter().position(|t| t == "Older chat").unwrap();
    assert!(stamped_at < opened_at, "the messaged chat moved up");
    assert!(cabin.threads[older_idx].accessed_ms > older_accessed);

    let pins_before: Vec<_> = after_stamp
        .iter()
        .filter(|title| cabin.threads.iter().any(|t| t.title == **title && t.pinned))
        .cloned()
        .collect();
    assert_eq!(pins_before, ["Last pin", "Earlier pin"]);
    cabin.switch_thread(earlier_idx);
    let after_pin_click = history_row_titles(&cabin);
    let pins_after: Vec<_> = after_pin_click.iter().take(2).cloned().collect::<Vec<_>>();
    assert_eq!(
        pins_after, pins_before,
        "clicking a pinned chat leaves the pin group last-pinned-first: {after_pin_click:?}"
    );
    assert!(cabin.threads[earlier_idx].pinned);
    assert_eq!(cabin.threads[earlier_idx].pinned_ms, earlier_pin_ms);
    assert_eq!(
        cabin.threads[earlier_idx].accessed_ms, earlier_accessed,
        "clicking a pin is not activity"
    );
    assert_eq!(
        after_pin_click, after_stamp,
        "the pin click does not reorder History"
    );

    let start = std::time::Instant::now();
    while cabin.persist_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(5) {
        cabin.poll_persist();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(&root);
}

// Landed from PR #89.
#[test]
fn queue_rows_label_done_failed_and_running() {
    let rows = [
        ("job-done", "Flash the pi", true),
        ("job-fail", "Failed · boot", false),
        ("job-run", "Write the image", false),
    ];
    let painted: Vec<(&str, String)> = rows
        .iter()
        .map(|(id, title, done)| {
            let st = super::pages::queue_task_label(title, *done);
            (st, format!("{st} · {id}"))
        })
        .collect();
    assert_eq!(
        painted,
        vec![
            ("done", "done · job-done".into()),
            ("failed", "failed · job-fail".into()),
            ("running", "running · job-run".into()),
        ]
    );
}

#[test]
fn park_fresh_chat_keeps_the_running_reply_and_opens_an_empty_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("park-fresh");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut live = crate::threads::ChatThread::new("Night watch", false);
    live.pinned = true;
    live.messages = std::sync::Arc::new(vec![
        ("user".into(), "keep this reply".into()),
        ("assistant".into(), "still writing".into()),
    ]);
    let live_id = live.id.clone();
    let mut cabin = super::Cabin::quiet_for_test();
    cabin.threads = vec![live];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.running = true;
    cabin.chat_job_thread = Some(live_id.clone());
    cabin.park_fresh_chat();

    assert!(cabin.running, "parking must not halt the reply");
    assert_eq!(cabin.chat_job_thread.as_deref(), Some(live_id.as_str()));
    assert!(
        cabin.messages.is_empty(),
        "the open pane must be an empty chat"
    );
    assert_ne!(
        cabin.threads[cabin.thread_idx].id, live_id,
        "the empty chat must not be the thread that is still replying"
    );
    let old = cabin
        .threads
        .iter()
        .find(|t| t.id == live_id)
        .expect("the running thread stays in the list");
    assert!(old.pinned);
    assert_eq!(old.title, "Night watch");
    assert_eq!(
        old.messages.as_slice(),
        [
            ("user".into(), "keep this reply".into()),
            ("assistant".into(), "still writing".into()),
        ]
    );

    let io = cabin.persist_io.clone();
    drop(cabin);
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(3) {
        if io.try_lock().is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(30));
            if io.try_lock().is_ok() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

/// A live turn of `rows` reply/thought rows, each followed by a tool call.
fn live_turn(rows: usize, tag: &str, lines: usize) -> Vec<grokhub_core::LiveBlock> {
    let mut blocks = Vec::new();
    for r in 0..rows {
        let body = (0..lines)
            .map(|l| {
                format!("{tag} row {r} line {l}: the dock wraps and the rail keeps its footer.")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        if r % 2 == 0 {
            grokhub_core::append_say(&mut blocks, &body);
        } else {
            grokhub_core::append_thought(&mut blocks, &body);
        }
        grokhub_core::append_tool(
            &mut blocks,
            &format!("{tag}-t{r}"),
            "Edit src/dock.rs",
            "completed",
            "",
        );
    }
    blocks
}

/// Paint the live turn with only the top `clip_h` px visible.
/// Returns the turn's height and the pane width it was laid out at.
fn paint_live_clipped(ctx: &egui::Context, cabin: &mut super::Cabin, clip_h: f32) -> (f32, f32) {
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 4000.0),
        )),
        ..Default::default()
    };
    let (mut height, mut width) = (0.0, 0.0);
    let _ = crate::theme::test_pass(ctx, raw, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            let top = ui.cursor().min;
            width = ui.available_width();
            ui.set_clip_rect(egui::Rect::from_min_size(top, egui::vec2(900.0, clip_h)));
            let mut collapse = false;
            let _ = cabin.paint_live_blocks(ui, true, false, &mut collapse);
            height = ui.cursor().min.y - top.y;
        });
    });
    (height, width)
}

#[test]
fn offscreen_live_rows_reserve_their_height_and_rekey_on_a_new_turn() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("live-cull");
    let tid = cabin.visible_thread_id();
    cabin.live_blocks = live_turn(20, "first", 3);
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    // Cold: nothing cached, every row lays out.
    let (cold, pane_w) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    let row_h = |ctx: &egui::Context| -> Vec<(u64, u8, f32)> {
        ctx.data(|d| d.get_temp(super::live_row_height_id(&tid, pane_w)))
            .unwrap_or_default()
    };
    let cold_rows = row_h(&ctx);
    assert_eq!(
        cold_rows.len(),
        cabin.live_blocks.len(),
        "one slot per live block"
    );
    // Warm: rows below the clip reserve their cached height instead.
    let (warm, _) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    assert!(cold > 1000.0, "the turn must run past the clip: {cold}");
    assert!(
        (warm - cold).abs() < 0.5,
        "culled rows must reserve exactly their painted height: cold {cold} warm {warm}"
    );
    assert_eq!(
        row_h(&ctx),
        cold_rows,
        "a steady turn keeps its row heights"
    );
    for (i, b) in cabin.live_blocks.iter().enumerate() {
        let (ident, _, h) = cold_rows[i];
        if b.kind == grokhub_core::LiveKind::Tool
            && i > 0
            && cabin.live_blocks[i - 1].kind == b.kind
        {
            continue;
        }
        assert_eq!(
            ident,
            super::live_row_ident(b),
            "row {i} is keyed by its own block"
        );
        assert!(h > 0.0, "row {i} has a height");
    }

    // Minimize all while the turn streams: off-screen thoughts shrink too, so they
    // must re-measure instead of reserving their expanded height.
    for b in cabin
        .live_blocks
        .iter()
        .filter(|b| b.kind == grokhub_core::LiveKind::Thought)
    {
        super::write_thought_fold(
            &ctx,
            super::thought_fold_id(&tid, "slot", b.fold_slot),
            grokhub_core::ThoughtFold::Minimized,
        );
    }
    let (folded, _) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    let (folded_truth, _) = paint_live_clipped(&ctx, &mut cabin, 10_000.0);
    assert!(
        folded < cold,
        "minimized thoughts are shorter: {folded} vs {cold}"
    );
    assert!(
        (folded - folded_truth).abs() < 0.5,
        "a fold change must repaint off-screen rows: clipped {folded} painted {folded_truth}"
    );
    for b in cabin
        .live_blocks
        .iter()
        .filter(|b| b.kind == grokhub_core::LiveKind::Thought)
    {
        super::write_thought_fold(
            &ctx,
            super::thought_fold_id(&tid, "slot", b.fold_slot),
            grokhub_core::ThoughtFold::Expanded,
        );
    }

    // The row still streaming grows below the clip and must still be measured.
    let last_i = cabin
        .live_blocks
        .iter()
        .rposition(|b| b.kind != grokhub_core::LiveKind::Tool)
        .expect("text row");
    cabin.live_blocks.truncate(last_i + 1);
    let (before, _) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    let grown_body = "\n\nmore streamed text for the tail row.".repeat(12);
    cabin.live_blocks[last_i].body.push_str(&grown_body);
    let (after, _) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    assert!(
        after > before + 100.0,
        "the growing tail row must repaint off-screen: before {before} after {after}"
    );

    // Next turn: same row indices, new blocks and much shorter rows. No stale height.
    cabin.live_blocks = live_turn(20, "second", 1);
    let (reused, _) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    // Ground truth: nothing clipped, so every row paints and measures itself.
    let (painted, _) = paint_live_clipped(&ctx, &mut cabin, 10_000.0);
    assert!(
        (reused - painted).abs() < 0.5,
        "a new turn must not reserve the last turn's heights: reused {reused} painted {painted}"
    );
    assert!(
        painted < cold,
        "the second turn's rows are shorter: {painted} vs {cold}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn transcript_caches_hold_while_the_turn_streams_and_catch_up_after() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("stream-hold");
    let mut t = crate::threads::ChatThread::new("Dock", false);
    t.messages = std::sync::Arc::new(vec![
        ("user".into(), "fix the dock".into()),
        (
            "assistant".into(),
            "THINKING:\nThe dock wraps early.\n\nFixed it.".into(),
        ),
        ("user".into(), "and the rail".into()),
        (
            "assistant".into(),
            "THINKING:\nCheck the rail.\n\nOn it".into(),
        ),
    ]);
    cabin.threads = vec![t];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    let _ = cabin.cached_chat_views();

    // A turn streams here. The first delta unshares the transcript from its thread,
    // so the views rebuild once.
    cabin.running = true;
    cabin.chat_job_thread = Some(cabin.visible_thread_id());
    grokhub_core::append_say(&mut cabin.live_blocks, "On it, the rail");
    if let Some(last) = cabin.live_tail_mut().last_mut() {
        last.1.push_str(", the rail");
    }
    let streaming = cabin.cached_chat_views().to_vec();
    // Later deltas only grow the last message, and the pane paints it from live
    // blocks: the views do not rebuild on them.
    if let Some(last) = cabin.live_tail_mut().last_mut() {
        last.1
            .push_str(" now keeps its footer and the dock stops wrapping.");
    }
    assert_eq!(
        cabin.cached_chat_views(),
        &streaming[..],
        "stream deltas skip the view rebuild"
    );

    // An edit before the last message is not a stream delta: rebuild even mid-turn.
    cabin.live_mut()[1].1 = "THINKING:\nThe dock wraps late.\n\nFixed it.".into();
    let mid = cabin.cached_chat_views().to_vec();
    assert!(
        mid.iter().any(|v| v.body.contains("wraps late")),
        "an earlier edit must rebuild the views while a turn streams: {mid:?}"
    );

    // The turn ends: the tail catches up once.
    if let Some(last) = cabin.live_tail_mut().last_mut() {
        last.1.push_str(" Done.");
    }
    cabin.running = false;
    let after = cabin.cached_chat_views().to_vec();
    assert!(
        after.last().is_some_and(|v| v.body.ends_with("Done.")),
        "the finished reply must reach the views: {after:?}"
    );
    assert!(
        after.iter().any(|v| v.body.contains("wraps late")),
        "the earlier edit survives the tail refresh: {after:?}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn live_row_height_cache_is_per_thread_and_width() {
    let a = super::live_row_height_id("thread-a", 720.0);
    assert_eq!(a, super::live_row_height_id("thread-a", 720.2));
    assert_ne!(a, super::live_row_height_id("thread-a", 1100.0));
    assert_ne!(a, super::live_row_height_id("thread-b", 720.0));
    assert_ne!(
        a,
        super::chat_row_height_id("thread-a", 720.0),
        "live rows must not share the stored rows' height list"
    );
}

#[test]
fn chat_view_fold_keys_follow_the_views_as_the_chat_changes() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("view-keys");
    let expect = |cabin: &mut super::Cabin| -> Vec<u64> {
        cabin
            .cached_chat_views()
            .iter()
            .map(|v| {
                if v.kind == grokhub_core::ChatKind::Thought {
                    grokhub_core::thought_body_key(&v.body)
                } else {
                    0
                }
            })
            .collect()
    };
    let mut dock = crate::threads::ChatThread::new("Dock", false);
    dock.messages = std::sync::Arc::new(vec![
        ("user".into(), "fix the dock".into()),
        (
            "assistant".into(),
            "THINKING:\nThe dock wraps early.\n\nHOST_CMD ls".into(),
        ),
        ("user".into(), "HOST_RESULT\nsrc".into()),
        (
            "assistant".into(),
            "THINKING:\nNow patch it.\n\nDone.".into(),
        ),
    ]);
    let mut other = crate::threads::ChatThread::new("Other", false);
    other.messages = std::sync::Arc::new(vec![
        ("user".into(), "hello".into()),
        (
            "assistant".into(),
            "THINKING:\nA different thought.\n\nHi.".into(),
        ),
    ]);
    cabin.threads = vec![dock, other];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();

    let first = expect(&mut cabin);
    assert!(
        first.iter().any(|&k| k != 0),
        "the dock chat has thoughts: {first:?}"
    );
    assert_eq!(cabin.chat_view_keys, first, "keys are built with the views");

    // A stream delta grows the last message: only the trailing stretch rekeys.
    if let Some(last) = cabin.live_mut().last_mut() {
        last.1 = "THINKING:\nNow patch it and run the tests.\n\nDone.".into();
    }
    let grown = expect(&mut cabin);
    assert_ne!(grown, first, "the grown thought has a new key");
    assert_eq!(cabin.chat_view_keys, grown, "a grown thought rekeys");

    cabin
        .live_mut()
        .push(("user".into(), "and the rail".into()));
    cabin.live_mut().push((
        "assistant".into(),
        "THINKING:\nCheck the rail.\n\nFixed.".into(),
    ));
    let turn = expect(&mut cabin);
    assert_eq!(cabin.chat_view_keys, turn, "a new turn keys its views");

    cabin.apply_switch_thread(1);
    let switched = expect(&mut cabin);
    assert_eq!(
        cabin.chat_view_keys, switched,
        "another chat rebuilds its keys"
    );
    release_isolated(&root, cabin);
}

#[test]
fn in_place_edit_refreshes_chat_views() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("view-rev");
    let last = "THINKING:\nNow patch it.\n\nDone.";
    let mut dock = crate::threads::ChatThread::new("Dock", false);
    dock.messages = std::sync::Arc::new(vec![
        ("user".into(), "fix the dock".into()),
        (
            "assistant".into(),
            "THINKING:\nThe dock wraps early.\n\nDone early.".into(),
        ),
        ("user".into(), "and the rail".into()),
        ("assistant".into(), last.into()),
    ]);
    cabin.threads = vec![dock];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();

    let fold_keys = |views: &[grokhub_core::ChatView]| -> Vec<u64> {
        views
            .iter()
            .map(|v| {
                if v.kind == grokhub_core::ChatKind::Thought {
                    grokhub_core::thought_body_key(&v.body)
                } else {
                    0
                }
            })
            .collect()
    };

    let _ = cabin.cached_chat_views();
    let before_keys = cabin.chat_view_keys.clone();
    let before_rev = cabin.messages_rev;
    assert!(
        before_keys.iter().any(|&k| k != 0),
        "the first turn has a thought: {before_keys:?}"
    );

    let count = cabin.messages.len();
    let last_len = cabin.messages.last().map(|m| m.1.len()).unwrap_or(0);
    cabin.live_mut()[1].1 =
        "THINKING:\nThe title clips past the rail and the dock wraps.\n\nDone early.".into();
    assert_eq!(
        cabin.messages.len(),
        count,
        "the edit keeps the message count"
    );
    assert_eq!(
        cabin.messages.last().map(|m| m.1.len()).unwrap_or(0),
        last_len,
        "the edit keeps the last message length"
    );
    assert_eq!(cabin.messages_rev, before_rev.wrapping_add(1));

    let fresh = grokhub_core::visible_chat(&cabin.messages);
    let fresh_keys = fold_keys(&fresh);
    assert_ne!(fresh_keys, before_keys, "the earlier thought must rekey");
    assert!(
        fresh.iter().any(|v| {
            v.kind == grokhub_core::ChatKind::Thought && v.body.contains("clips past the rail")
        }),
        "the edited thought must be visible: {fresh:?}"
    );
    assert_eq!(cabin.cached_chat_views(), fresh.as_slice());
    assert_eq!(cabin.chat_view_keys, fresh_keys);
    assert_eq!(cabin.chat_view_rev, cabin.messages_rev);
    release_isolated(&root, cabin);
}

#[test]
fn idle_chat_caches_keep_the_message_revision() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("view-idle");
    let mut dock = crate::threads::ChatThread::new("Dock", false);
    dock.messages = std::sync::Arc::new(vec![
        ("user".into(), "fix the dock".into()),
        (
            "assistant".into(),
            "THINKING:\nThe dock wraps early.\n\nDone.".into(),
        ),
    ]);
    cabin.threads = vec![dock];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();

    let _ = cabin.cached_chat_views();
    let rev = cabin.messages_rev;
    let view_rev = cabin.chat_view_rev;
    let keys = cabin.chat_view_keys.clone();
    let views = cabin.chat_views.clone();
    let views_ptr = cabin.chat_views.as_ptr();
    assert_eq!(cabin.cached_chat_views().as_ptr(), views_ptr);
    assert_eq!(cabin.messages_rev, rev);
    assert_eq!(cabin.chat_view_rev, view_rev);
    assert_eq!(cabin.chat_view_keys, keys);
    assert_eq!(cabin.chat_views, views);
    release_isolated(&root, cabin);
}

#[test]
fn scrub_live_blocks_clears_live_keys_when_redaction_keeps_length() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("live-key-redact");
    let secret = "TenCharSec";
    assert_eq!(secret.len(), "[redacted]".len());
    let body = format!("look at {secret} please");
    let len = body.len();
    cabin.live_blocks.push(grokhub_core::LiveBlock {
        kind: grokhub_core::LiveKind::Thought,
        body,
        tool_id: String::new(),
        tool_title: String::new(),
        tool_status: String::new(),
        tool_detail: String::new(),
        fold_slot: 4,
    });
    cabin.live_keys = vec![(4, len, 99)];
    cabin.secret_hold.push(secret.to_string());
    cabin.scrub_live_blocks();
    assert!(
        cabin.live_keys.is_empty(),
        "same-length [redacted] must not keep the cached thought key"
    );
    assert_eq!(cabin.live_blocks[0].body.len(), len);
    assert!(cabin.live_blocks[0].body.contains("[redacted]"));
    assert!(!cabin.live_blocks[0].body.contains(secret));
    cabin.live_keys = vec![(4, len, 7)];
    cabin.scrub_live_blocks();
    assert_eq!(
        cabin.live_keys,
        vec![(4, len, 7)],
        "a scrub that does not change a body must leave the keys"
    );
    release_isolated(&root, cabin);
}

#[test]
fn selecting_plan_leaves_the_title_and_sets_plan() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("plan-title");
    let mut thread = crate::threads::ChatThread::new("Night watch", false);
    thread.messages = std::sync::Arc::new(vec![("user".into(), "dock".into())]);
    cabin.threads = vec![thread];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.session_mode = SessionMode::Chat;

    cabin.select_plan_without_rename();

    assert_eq!(cabin.threads[0].title, "Night watch");
    assert_eq!(cabin.session_mode, SessionMode::Plan);
    release_isolated(&root, cabin);
}

#[test]
fn feed_accept_files_one_todo() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("idea-accept");
    let card = grokhub_core::idea_card("src", "More F1", "from the brief", 1);
    let id = card.id.clone();
    cabin.updates = vec![card];

    cabin.build_idea(&id);

    assert_eq!(cabin.board.len(), 1);
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Todo);
    assert_eq!(cabin.board[0].title, "Cover F1");
    release_isolated(&root, cabin);
}

#[test]
fn queue_run_starts_the_origin_thread() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("queue-run");
    let visible = crate::threads::ChatThread::new("Visible", false);
    let mut origin = crate::threads::ChatThread::new("Origin", false);
    let origin_id = origin.id.clone();
    origin.messages = std::sync::Arc::new(Vec::new());
    cabin.threads = vec![visible, origin];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.agents.push(AgentJob {
        title: "Flash the pi".into(),
        status: "queued".into(),
        prompt: "write the image".into(),
        thread_id: origin_id.clone(),
    });

    assert!(cabin.start_queued_job(0));
    assert!(!cabin.running, "the state change does not spawn grok");
    assert_eq!(cabin.agents[0].status, "running");
    assert_eq!(cabin.chat_job_thread.as_deref(), Some(origin_id.as_str()));
    assert!(matches!(cabin.nav, Nav::Chat));
    let origin = cabin.threads.iter().find(|t| t.id == origin_id).unwrap();
    assert_eq!(
        origin.messages.as_slice(),
        [("user".into(), "write the image".into())]
    );
    assert!(cabin.threads[0].messages.is_empty());
    release_isolated(&root, cabin);
}

#[test]
fn shell_echo_lands_on_the_open_chat() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("shell-echo");
    let dir = root.join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    cabin.cfg.host_on = true;
    cabin.cfg.project_dir = dir.display().to_string();
    let thread = crate::threads::ChatThread::new("Shell", false);
    cabin.threads = vec![thread];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();

    cabin.queue_sh("echo grokhub-proof".into());
    // PowerShell on a loaded Windows runner can sit past 5s before the first
    // line. Same 45s budget as `echo_ok`. The product host cap is 90s.
    let budget = std::time::Duration::from_secs(if cfg!(windows) { 45 } else { 5 });
    let start = std::time::Instant::now();
    while start.elapsed() < budget {
        cabin.poll_job();
        let so_far = cabin
            .messages
            .iter()
            .map(|m| m.1.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if so_far.contains("grokhub-proof") {
            break;
        }
        if !cabin.running {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    cabin.poll_job();
    cabin.halt_in_flight();
    assert!(
        !cabin.running,
        "echo should finish, status {}",
        cabin.status
    );
    let text = cabin
        .messages
        .iter()
        .map(|m| m.1.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("grokhub-proof"),
        "the shell output should land on the chat: {text}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn session_menu_sits_left_of_minimize() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("menu-place");
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 80.0),
        )),
        ..Default::default()
    };
    let mut menu = egui::Rect::NOTHING;
    let mut mini = egui::Rect::NOTHING;
    let _ = crate::theme::test_pass(&ctx, raw, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                mini = titlebar_chrome_btn(ui, ChromeBtn::Minimize).rect;
                menu = cabin.paint_session_actions_menu(ui);
            });
        });
    });
    assert!(
        menu.right() <= mini.left() + 0.5,
        "the session menu must sit left of Minimize: menu {menu:?} minimize {mini:?}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn update_queues_while_a_job_is_running() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("update-queue");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = super::Cabin::quiet_for_test();
    cabin.running = true;
    cabin.start_overlay_update(vec!["echo grokhub-update-queued".into()]);
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Update);
    assert_eq!(
        cabin.status,
        "Update queued — it starts when this job finishes."
    );
    assert_eq!(
        cabin.queued_overlay,
        Some(vec!["echo grokhub-update-queued".into()])
    );
    assert!(cabin.running);
    assert!(cabin.rx.is_none(), "a busy cabin must not start the update");
    drop(cabin);
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn send_from_composer_runs_help_and_refuses_without_grok() {
    let mut quiet = QuietCabin::boot("send-help");
    let cabin = &mut quiet.cabin;
    cabin.send_from_composer("/help".into());
    let help = cabin
        .messages
        .iter()
        .find(|m| m.0 == "assistant" && m.1.contains("/help — this list"));
    assert!(
        help.is_some(),
        " /help must land on the open chat, got {:?}",
        cabin.messages
    );
    assert!(
        !cabin.running,
        "a local slash must not start a run: {}",
        cabin.status
    );

    let before = cabin.messages.len();
    cabin.send_from_composer("paint the harbor".into());
    assert!(!cabin.running, "no sign-in must refuse before a run starts");
    // The booted cabin is on the native engine, so it names GrokHub's own sign-in.
    assert_eq!(cabin.status, "Sign in with Grok or add an API key.");
    assert_eq!(
        cabin.messages.len(),
        before,
        "the refused line must not be written onto the chat"
    );
    assert!(
        cabin
            .messages
            .iter()
            .all(|m| !m.1.contains("paint the harbor")),
        "the refused prompt must stay off the transcript"
    );
    quiet.settle();
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
fn pin_thread_flips_the_pin() {
    let mut quiet = QuietCabin::boot("pin-thread");
    let cabin = &mut quiet.cabin;
    let idx = cabin.thread_idx;
    let title = cabin.threads[idx].title.clone();
    assert!(!cabin.threads[idx].pinned);
    cabin.pin_thread(idx);
    assert!(
        cabin.threads[idx].pinned,
        "the pin control must pin this chat"
    );
    assert!(cabin.threads[idx].pinned_ms > 0);
    assert_eq!(cabin.status, format!("Pinned {title}"));
    quiet.settle();
}

#[test]
fn composer_and_history_omit_archived_idle_chrome() {
    let src = cabin_src();
    let toast = fn_src(&src, "paint_archive_toast");
    assert!(
        !toast.contains("idle chat")
            && !toast.contains("idle chats")
            && !toast.contains("auto_archive_note")
            && !toast.contains("ghost_pill(ui, \"Archived\")")
            && !toast.contains("Nav::History"),
        "composer archive toast must not show idle-archive chrome: {toast}"
    );
    assert!(
        toast.contains("ghost_pill(ui, \"Undo\")"),
        "manual archive Undo toast stays: {toast}"
    );
    let live = src
        .split("ComposerStackSlot::LiveWork =>")
        .nth(1)
        .and_then(|s| s.split("ComposerStackSlot::SlashPalette =>").next())
        .expect("LiveWork slot");
    assert!(
        live.contains("paint_archive_toast") && !live.contains("idle chat"),
        "LiveWork must not embed idle-archive labels: {live}"
    );
    let hist = src
        .split("fn ui_history(")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("ui_history");
    assert!(
        !hist.contains("history_archived")
            && !hist.contains("\"Archived\"")
            && !hist.contains("Archived chats"),
        "History must not offer an Archived view: {hist}"
    );
    let side = include_str!("sidebar.rs");
    assert!(
        !side.contains("rail-archived")
            && !side.contains("RichText::new(\"Archived\")")
            && !side.contains("No archived chats"),
        "sidebar must not list Archived chats: {side}"
    );
    let kick = fn_src(&src, "kick_history_search");
    assert!(
        kick.contains("is_archived") && kick.contains("continue"),
        "History search must skip archived chats: {kick}"
    );
    let archive_row = fn_src(&src, "ui_chat_archive_row");
    assert!(
        !archive_row.contains("move to Archived")
            && archive_row.contains("leave History and search")
            && archive_row.contains("still on disk")
            && archive_row.contains("no Archived drawer")
            && !archive_row.contains("open the chat or Unarchive from its menu"),
        "Settings auto-archive must not promise an Archived drawer or post-leave recovery: {archive_row}"
    );
    let recall = src
        .split("Slash::Recall(q)")
        .nth(1)
        .and_then(|s| s.split("fn kick_model_retry").next())
        .expect("Recall");
    assert!(
        recall.contains("is_archived") && recall.contains("continue"),
        "/recall must skip archived chats like History search: {recall}"
    );
}

#[test]
fn archive_hides_from_main_list_and_reply_restores() {
    let mut quiet = QuietCabin::boot("chat-archive");
    let cabin = &mut quiet.cabin;
    cabin.threads.clear();
    let mut keep = crate::threads::ChatThread::new("Keep", false);
    keep.messages_mut().push(("user".into(), "stay".into()));
    keep.accessed_ms = 100;
    let mut done = crate::threads::ChatThread::new("Done chat", false);
    done.messages_mut().push(("user".into(), "old".into()));
    done.accessed_ms = 50;
    cabin.threads = vec![keep, done];
    cabin.thread_idx = 1;
    cabin.messages = cabin.threads[1].messages.clone();
    cabin.archive_thread_at(1);
    assert!(crate::threads::is_archived(&cabin.threads[1]));
    assert_eq!(
        crate::threads::chat_section_indices(&cabin.threads, Some(1), false),
        vec![0]
    );
    assert_eq!(cabin.status, "Archived Done chat");
    assert_eq!(
        cabin.archive_undo.as_deref(),
        Some(cabin.threads[1].id.as_str())
    );
    cabin.unarchive_on_reply();
    assert!(!crate::threads::is_archived(&cabin.threads[1]));
    assert_eq!(cabin.status, "Restored Done chat to chats");
    assert_eq!(
        crate::threads::chat_section_indices(&cabin.threads, Some(1), false),
        vec![0, 1]
    );
    quiet.settle();
}

#[test]
fn auto_archive_15_day_idle_skips_13_day_pinned_and_busy() {
    let mut quiet = QuietCabin::boot("auto-archive");
    let cabin = &mut quiet.cabin;
    let now = 30 * crate::threads::DAY_MS;
    cabin.cfg.chat_archive_days = 14;
    cabin.threads.clear();
    let mut idle15 = crate::threads::ChatThread::new("Fifteen", false);
    idle15.messages_mut().push(("user".into(), "hi".into()));
    idle15.accessed_ms = now - 15 * crate::threads::DAY_MS;
    let mut idle13 = crate::threads::ChatThread::new("Thirteen", false);
    idle13.messages_mut().push(("user".into(), "hi".into()));
    idle13.accessed_ms = now - 13 * crate::threads::DAY_MS;
    let mut pinned = crate::threads::ChatThread::new("Pinned", false);
    pinned.messages_mut().push(("user".into(), "hi".into()));
    pinned.accessed_ms = now - 20 * crate::threads::DAY_MS;
    pinned.pinned = true;
    let mut busy = crate::threads::ChatThread::new("Busy", false);
    busy.messages_mut().push(("user".into(), "hi".into()));
    busy.accessed_ms = now - 20 * crate::threads::DAY_MS;
    let busy_id = busy.id.clone();
    cabin.threads = vec![idle15, idle13, pinned, busy];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.running = true;
    cabin.chat_job_thread = Some(busy_id);
    let n = cabin.run_auto_archive(now);
    assert_eq!(n, 1, "only the 15-day idle chat archives");
    assert!(crate::threads::is_archived(&cabin.threads[0]));
    assert!(!crate::threads::is_archived(&cabin.threads[1]));
    assert!(!crate::threads::is_archived(&cabin.threads[2]));
    assert!(!crate::threads::is_archived(&cabin.threads[3]));
    assert_eq!(cabin.status, "Archived 1 idle chat");
    quiet.settle();
}

#[test]
fn persist_keeps_threads_in_the_dir_it_was_scheduled_with() {
    let _g = crate::config::hold_test_config();
    let first = crate::config::test_config_root("persist-pin-a");
    let second = crate::config::test_config_root("persist-pin-b");
    let prev = std::env::var_os("GROKHUB_CONFIG");
    struct Restore {
        prev: Option<std::ffi::OsString>,
        roots: Vec<std::path::PathBuf>,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            restore_env("GROKHUB_CONFIG", self.prev.take());
            for root in &self.roots {
                let _ = std::fs::remove_dir_all(root);
            }
        }
    }
    let _restore = Restore {
        prev,
        roots: vec![first.clone(), second.clone()],
    };
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &first);
    let mut cabin = super::Cabin::quiet_for_test();
    let mut thread = crate::threads::ChatThread::new("PinnedLeak", false);
    thread.pinned = true;
    cabin.threads = vec![thread];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    let io = cabin.persist_io.clone();
    let hold = io.lock().unwrap_or_else(|e| e.into_inner());
    cabin.persist();
    std::env::set_var("GROKHUB_CONFIG", &second);
    drop(hold);
    let scheduled = first.join("threads.json");
    let moved = second.join("threads.json");
    let mut landed = String::new();
    for _ in 0..80 {
        if let Ok(body) = std::fs::read_to_string(&scheduled) {
            if body.contains("PinnedLeak") {
                landed = body;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let leaked = std::fs::read_to_string(&moved).unwrap_or_default();
    assert!(
        landed.contains("PinnedLeak"),
        "scheduled persist must write threads.json into the captured config dir; the later dir has {leaked}"
    );
    assert!(
        !leaked.contains("PinnedLeak"),
        "a later GROKHUB_CONFIG must not receive that threads.json: {leaked}"
    );
}

#[test]
fn apply_board_act_add_files_one_todo() {
    let mut quiet = QuietCabin::boot("board-add");
    let cabin = &mut quiet.cabin;
    assert!(cabin.board.is_empty());
    cabin.board_title = "Cover the dock".into();
    cabin.board_notes = "night shift".into();
    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Add)));
    assert_eq!(cabin.board.len(), 1);
    assert_eq!(cabin.board[0].title, "Cover the dock");
    assert_eq!(cabin.board[0].detail, "night shift");
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Todo);
    assert_eq!(
        cabin.board[0].status.column(),
        Some(grokhub_core::KanbanColumn::Todo)
    );
    assert!(cabin.board_title.is_empty());
    quiet.settle();
}

#[test]
fn apply_plus_ready_image_and_pasted_text() {
    let mut quiet = QuietCabin::boot("plus-ready");
    let cabin = &mut quiet.cabin;
    cabin.apply_plus_ready(
        grokhub_core::PlusTarget::Chat,
        super::plus::PlusReady {
            kind: grokhub_core::AttachKind::Image,
            name: "harbor.png".into(),
            raw: "/tmp/harbor.png".into(),
            image_url: Some("data:image/png;base64,aGFyYm9y".into()),
            text: None,
        },
    );
    assert_eq!(cabin.attach_name.as_deref(), Some("harbor.png"));
    assert_eq!(
        cabin.attach_url.as_deref(),
        Some("data:image/png;base64,aGFyYm9y")
    );
    assert_eq!(
        cabin.status, "Attached harbor.png — sends with the next message",
        "an image attach must show the chip status"
    );
    assert!(!cabin.running);
    assert!(
        cabin.composer.is_empty(),
        "an image must not dump into the composer"
    );

    cabin.apply_plus_ready(
        grokhub_core::PlusTarget::Chat,
        super::plus::PlusReady {
            kind: grokhub_core::AttachKind::Text,
            name: "notes.txt".into(),
            raw: "notes.txt".into(),
            image_url: None,
            text: Some("pasted harbor line".into()),
        },
    );
    assert!(
        cabin.composer.is_empty(),
        "a file must stay attached, not pasted, got {}",
        cabin.composer
    );
    assert_eq!(cabin.attach_tray.len(), 2);
    assert_eq!(cabin.attach_name.as_deref(), Some("harbor.png"));
    assert_eq!(cabin.status, "File notes.txt — the agent will read it");
    assert!(!cabin.running);
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

#[test]
fn apply_board_act_move_archive_restore_and_link() {
    let mut quiet = QuietCabin::boot("board-acts");
    let cabin = &mut quiet.cabin;
    cabin.board_title = "Move the buoy".into();
    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Add)));
    let id = cabin.board[0].id.clone();
    let thread = cabin.threads[cabin.thread_idx].id.clone();

    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Move {
        id: id.clone(),
        status: grokhub_core::BoardStatus::InProgress,
    })));
    assert_eq!(
        cabin.board[0].status.column(),
        Some(grokhub_core::KanbanColumn::Doing)
    );

    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Archive(id.clone()))));
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Dismissed);
    assert!(cabin.board[0].status.column().is_none());

    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Restore(id.clone()))));
    assert_eq!(
        cabin.board[0].status.column(),
        Some(grokhub_core::KanbanColumn::Todo)
    );

    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Link(id.clone()))));
    assert_eq!(cabin.board[0].thread_id.as_deref(), Some(thread.as_str()));

    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Unlink(id))));
    assert!(cabin.board[0].thread_id.is_none());
    quiet.settle();
}

#[test]
fn add_automation_seed_daily_clock_and_loop() {
    let mut quiet = QuietCabin::boot("auto-seed");
    let root = quiet.boot.root.clone();
    let cabin = &mut quiet.cabin;
    cabin.add_automation_seed("every day at 9, summarize the board");
    assert_eq!(
        cabin.automations.len(),
        1,
        "a daily clock must land in automations"
    );
    assert!(
        cabin.grok_loops.is_empty(),
        "a clock time must not become a /loop"
    );
    assert!(
        cabin.status.contains("Automation added") && cabin.status.contains("09:00"),
        "the cabin must show the 09:00 automation, got {}",
        cabin.status
    );
    wait_for("automations.json must record the daily clock", || {
        std::fs::read_to_string(root.join("automations.json"))
            .unwrap_or_default()
            .contains("09")
    });

    cabin.add_automation_seed("/loop 30m check deploy");
    assert_eq!(cabin.grok_loops.len(), 1);
    assert_eq!(cabin.automations.len(), 1, "the clock row must stay");
    assert!(
        cabin.status.contains("Loop added") && cabin.status.contains("30m"),
        " /loop must show the interval, got {}",
        cabin.status
    );
    wait_for("loops.json must record the 30m loop", || {
        std::fs::read_to_string(root.join("loops.json"))
            .unwrap_or_default()
            .contains("30m")
    });
    quiet.settle();
}

#[test]
fn take_chip_act_nav_and_dismiss() {
    let mut quiet = QuietCabin::boot("chips");
    let cabin = &mut quiet.cabin;
    let nav = grokhub_core::QuickChip {
        id: "nav-board".into(),
        label: "Workboard".into(),
        value: "__nav:workboard".into(),
        kind: grokhub_core::ChipKind::Nav,
        score: 1.0,
        hint: String::new(),
        primary: false,
    };
    assert!(matches!(cabin.nav, super::Nav::Chat));
    cabin.take_chip_act(crate::cards::ChipRowAct::Apply(0), &[nav]);
    assert!(matches!(cabin.nav, super::Nav::Workboard));

    let dismiss = grokhub_core::QuickChip {
        id: "nav-ideas".into(),
        label: "Ideas".into(),
        value: "__nav:ideas".into(),
        kind: grokhub_core::ChipKind::Nav,
        score: 1.0,
        hint: String::new(),
        primary: false,
    };
    cabin.chip_busy = true;
    cabin.take_chip_act(crate::cards::ChipRowAct::Dismiss(0), &[dismiss]);
    assert!(cabin.chip_dismissed.iter().any(|d| d == "nav-ideas"));
    assert!(cabin.chip_dismissed.iter().any(|d| d == "__nav:ideas"));
    assert!(
        cabin.visible_chips.iter().all(|c| c.id != "nav-ideas"),
        "dismiss must drop that chip from the row, got {:?}",
        cabin
            .visible_chips
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>()
    );
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    quiet.settle();
}

#[test]
fn open_memory_file_flushes_the_editor_you_left() {
    let mut quiet = QuietCabin::boot("memory-file");
    let cabin = &mut quiet.cabin;
    assert_eq!(cabin.mem_name, "SOUL.md");
    cabin.mem_body = "typed soul line for the switch".into();
    cabin.open_memory_file("USER.md");
    assert_eq!(cabin.mem_name, "USER.md");
    assert!(
        cabin.mem_body.contains("Who you are"),
        "the next file must show in the editor, got {}",
        cabin.mem_body
    );
    assert!(
        !cabin.mem_body.contains("typed soul line"),
        "switching tabs must not wipe the unsaved line into the next file"
    );
    wait_for("leaving SOUL must flush the unsaved typing", || {
        crate::config::read_memory("SOUL.md").contains("typed soul line for the switch")
    });
    assert_eq!(cabin.mem_name, "USER.md");
    quiet.settle();
}

#[test]
fn set_session_mode_ask_and_chat() {
    let mut quiet = QuietCabin::boot("session-mode");
    quiet.settle();
    let app_json = quiet.boot.root.join("app.json");
    let cabin = &mut quiet.cabin;
    cabin.set_session_mode(grokhub_core::wire::SessionMode::Ask);
    assert_eq!(cabin.session_mode, grokhub_core::wire::SessionMode::Ask);
    assert_eq!(cabin.cfg.session_mode, "ask");
    wait_for("Ask must be saved", || {
        std::fs::read_to_string(&app_json)
            .unwrap_or_default()
            .contains("\"sessionMode\": \"ask\"")
    });

    cabin.set_session_mode(grokhub_core::wire::SessionMode::Chat);
    assert_eq!(cabin.session_mode, grokhub_core::wire::SessionMode::Chat);
    assert_eq!(cabin.cfg.session_mode, "chat");
    wait_for("Chat must be saved", || {
        std::fs::read_to_string(&app_json)
            .unwrap_or_default()
            .contains("\"sessionMode\": \"chat\"")
    });
    quiet.settle();
}

#[test]
fn delete_thread_at_removes_that_row() {
    let mut quiet = QuietCabin::boot("delete-thread");
    let cabin = &mut quiet.cabin;
    let keep = cabin.threads[cabin.thread_idx].title.clone();
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Harbor", false));
    let idx = cabin.threads.len() - 1;
    cabin.threads[idx].messages = std::sync::Arc::new(vec![("user".into(), "harbor row".into())]);
    cabin.delete_thread_at(idx);
    assert!(
        cabin.threads.iter().all(|t| t.title != "Harbor"),
        "deleting that chat must remove its sidebar row, left {:?}",
        cabin
            .threads
            .iter()
            .map(|t| t.title.as_str())
            .collect::<Vec<_>>()
    );
    assert!(cabin.threads.iter().any(|t| t.title == keep));
    assert_eq!(cabin.status, "Deleted Harbor");
    quiet.settle();
}

#[test]
fn discuss_card_opens_one_local_chat() {
    let mut quiet = QuietCabin::boot("discuss");
    let root = quiet.boot.root.clone();
    let cabin = &mut quiet.cabin;
    cabin.updates.push(grokhub_core::UpdateCard {
        id: "idea-harbor".into(),
        kind: grokhub_core::UpdateKind::Idea,
        title: "Cover F1".into(),
        body: Some("the night race".into()),
        created_at: 1,
        status: grokhub_core::UpdateStatus::Unread,
        action: None,
        expires_at: None,
        held: false,
        citations: Vec::new(),
        reaction: None,
        discuss_thread: None,
        built: false,
        board_id: None,
        why: Some("because the tide turned".into()),
        feed_pin: false,
        feed_kept: false,
        prompt: None,
        idea_kind: None,
        details: None,
        draft: None,
        modified: false,
        source_id: String::new(),
        skill: None,
        runs: 1,
        dismissed_at: 0,
        done_for_you: None,
        pulse: Default::default(),
    });
    cabin.discuss_card("idea-harbor");
    let open_id = cabin
        .threads
        .iter()
        .find(|t| t.title == "Discuss · Cover F1")
        .expect("idea pop thread")
        .id
        .clone();
    let open = cabin.threads.iter().find(|t| t.id == open_id).unwrap();
    assert!(open.background);
    assert_ne!(cabin.threads[cabin.thread_idx].id, open_id);
    assert!(cabin.idea_board.open.is_some() && cabin.nav == super::Nav::Pulse);
    assert_eq!(
        cabin
            .threads
            .iter()
            .filter(|t| t.title == "Discuss · Cover F1")
            .count(),
        1
    );
    assert!(
        crate::threads::chat_section_indices(&cabin.threads, None, false)
            .iter()
            .all(|&i| cabin.threads[i].id != open_id)
    );
    let body = cabin
        .threads
        .iter()
        .find(|t| t.id == open_id)
        .unwrap()
        .messages
        .iter()
        .find(|m| m.0 == "assistant")
        .map(|m| m.1.clone())
        .unwrap_or_default();
    assert!(
        body.contains("Nothing runs until you press Apply"),
        "the card chat opens with a short line; the card itself shows the details, got {body}"
    );
    assert_eq!(cabin.updates[0].status, grokhub_core::UpdateStatus::Opened);
    assert!(cabin.updates[0].discuss_thread.is_some());
    assert!(!cabin.running);
    wait_for("the discuss card must be saved opened", || {
        std::fs::read_to_string(root.join("updates.json"))
            .unwrap_or_default()
            .contains("idea-harbor")
    });
    quiet.settle();
}

#[test]
fn delete_all_history_clears_seeded_chats() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("delete-all");
    let mut cabin = Cabin::quiet_for_test();
    let mut pier = crate::threads::ChatThread::new("Pier light", false);
    pier.messages_mut()
        .push(("user".into(), "bring the lamp".into()));
    let mut salt = crate::threads::ChatThread::new("Salt lane", false);
    salt.messages_mut()
        .push(("user".into(), "walk the lane".into()));
    cabin.threads = vec![pier, salt];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.delete_all_history();
    wait_tree_contains(&cfg.root, "Chat");
    join_persist(&cabin.persist_svc);
    let saved = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(
        !saved.contains("Pier light") && !saved.contains("Salt lane"),
        "the pre-delete snapshot from halt_in_flight must not land last: {saved}"
    );
    assert!(
        cabin.threads.iter().all(|t| t.title != "Pier light"),
        "Pier light must be gone"
    );
    assert!(
        cabin.threads.iter().all(|t| t.title != "Salt lane"),
        "Salt lane must be gone"
    );
    assert_eq!(cabin.status, "Deleted all chats");
    assert_eq!(cabin.threads.len(), 1, "the fresh chat is the only row");
    assert_eq!(cabin.threads[0].title, "Chat");
    assert!(
        cabin.threads[0].messages.is_empty(),
        "the fresh chat has no transcript"
    );
    drop(cfg);
}

#[test]
fn poisoned_persist_io_surfaces_error_and_retries_idle() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-poison");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Pier", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    // Poison the shared mutex the PersistService worker takes before disk I/O.
    let io = cabin.persist_io.clone();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _g = io.lock().unwrap();
        panic!("poison persist_io");
    }));
    cabin.persist_idle_key.clear();
    cabin.persist_bg();
    let start = std::time::Instant::now();
    while cabin.persist_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(3) {
        cabin.poll_persist();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cabin.poll_persist_err();
    assert!(
        cabin.status.contains("persist lock poisoned") || cabin.status.contains("Could not save"),
        "poisoned lock must surface via persist_err, got status={status}",
        status = cabin.status
    );
    assert!(
        cabin.persist_idle_key.is_empty(),
        "failed write must clear the idle key so the next tick retries"
    );
    drop(cfg);
}

#[test]
fn persist_worker_survives_a_panicking_job_then_writes() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-panic-survive");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Panic pier", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "before panic".into())]);
    // First job panics inside the worker; catch_unwind must keep the thread up.
    super::persist_panic_next();
    cabin.persist();
    cabin.persist_svc.flush();
    cabin.poll_persist_err();
    assert!(
        cabin.status.contains("persist worker panicked") || cabin.status.contains("Could not save"),
        "panicked job must surface via persist_err, got status={status}",
        status = cabin.status
    );
    // Second job must still land on disk through the same worker.
    cabin.status.clear();
    cabin.persist_idle_key.clear();
    if let Ok(mut slot) = cabin.persist_err.lock() {
        *slot = None;
    }
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "after panic".into())]);
    cabin.persist();
    cabin.persist_svc.flush();
    cabin.poll_persist_err();
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(
        threads.contains("after panic") || threads.contains("Panic pier"),
        "worker must still write after a panicked job: {threads}"
    );
    assert!(
        !cabin.status.contains("persist worker panicked"),
        "successful write must not leave the panic status: {status}",
        status = cabin.status
    );
    drop(cfg);
}

#[test]
fn persist_writes_directly_when_the_worker_is_gone() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-dead-worker");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Dead worker dock", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "no worker".into())]);
    cabin.persist_svc = super::dead_persist_service(
        cabin.persist_io.clone(),
        std::sync::Arc::new(std::sync::Mutex::new(PersistMark::default())),
    );
    cabin.persist();
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(threads.contains("Dead worker dock"), "{threads}");
    cabin.poll_persist_err();
    assert!(
        !cabin.status.contains("Could not save"),
        "a successful fallback write must not report failure: {status}",
        status = cabin.status
    );
    // A panic inside the fallback is caught and reported, not propagated.
    super::persist_panic_next();
    cabin.persist();
    cabin.poll_persist_err();
    assert!(
        cabin
            .status
            .starts_with("Could not save chats: persist worker panicked"),
        "got status={status}",
        status = cabin.status
    );
    drop(cfg);
}

#[test]
fn an_older_persist_snapshot_never_overwrites_a_newer_one() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-order");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Pier light", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    cabin.projects_dirty = true;
    let mut old = cabin.persist_snap();
    old.secrets = Some(cabin.secrets.clone());
    let old_gen = cabin.next_persist_gen();
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.projects_dirty = false;
    let new = cabin.persist_snap();
    let new_gen = cabin.next_persist_gen();
    assert_eq!((old_gen, new_gen), (1, 2));
    assert!(old.projects.is_some() && new.projects.is_none());
    assert!(new.secrets.is_none());
    let mark = std::sync::Arc::new(std::sync::Mutex::new(PersistMark::default()));
    // The newer worker takes persist_io first, the older one second.
    let _ = write_persist_disk_in_order(&cfg.root, &new, new_gen, &mark);
    let _ = write_persist_disk_in_order(&cfg.root, &old, old_gen, &mark);
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(threads.contains("\"title\": \"Chat\""), "{threads}");
    assert!(!threads.contains("Pier light"), "{threads}");
    // Only the older snapshot carried projects and secrets, so those still land.
    assert!(crate::store::projects_path().is_file());
    assert!(cfg.root.join("secrets.json").is_file());
    // A second older write of the same generation changes nothing.
    std::fs::remove_file(crate::store::projects_path()).expect("projects");
    let _ = write_persist_disk_in_order(&cfg.root, &old, old_gen, &mark);
    assert!(!crate::store::projects_path().exists());
    let again = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert_eq!(again, threads);
    drop(cfg);
}

#[test]
fn persist_service_queues_out_of_order_full_snaps() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-svc-order");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Pier light", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    cabin.projects_dirty = true;
    let mut old = cabin.persist_snap();
    old.secrets = Some(cabin.secrets.clone());
    let old_gen = cabin.next_persist_gen();
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.projects_dirty = false;
    let new = cabin.persist_snap();
    let new_gen = cabin.next_persist_gen();
    assert_eq!((old_gen, new_gen), (1, 2));
    // Newer gen first, then older — through the actor, not the free fn.
    cabin.persist_svc.submit_full(
        cfg.root.clone(),
        new,
        new_gen,
        None,
        cabin.persist_err.clone(),
    );
    cabin.persist_svc.submit_full(
        cfg.root.clone(),
        old,
        old_gen,
        None,
        cabin.persist_err.clone(),
    );
    join_persist(&cabin.persist_svc);
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(threads.contains("\"title\": \"Chat\""), "{threads}");
    assert!(!threads.contains("Pier light"), "{threads}");
    assert!(crate::store::projects_path().is_file());
    assert!(cfg.root.join("secrets.json").is_file());
    drop(cfg);
}

#[test]
fn home_x_on_a_digest_keeps_it_in_pulse() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("home-x-digest");
    let mut cabin = Cabin::quiet_for_test();
    let card = grokhub_core::digest_card("week", "Week notes", "rolled up", 2);
    let id = card.id.clone();
    cabin.updates.push(card);
    let on_home = |cabin: &Cabin| {
        cabin
            .updates
            .iter()
            .any(|c| c.id == id && !c.pulse.off_home)
    };
    assert!(on_home(&cabin), "the digest starts visible for home");
    cabin.apply_feed_act(Some(FeedAct::Dismiss(id.clone())));
    wait_tree_contains(&cfg.root, "offHome");
    assert!(!on_home(&cabin), "× sets off_home");
    assert!(
        grokhub_core::visible_digests(&cabin.updates)
            .iter()
            .any(|c| c.id == id),
        "Pulse still lists the digest"
    );
    assert!(grokhub_core::archived_digests(&cabin.updates).is_empty());
    let kept = cabin
        .updates
        .iter()
        .find(|c| c.id == id)
        .expect("digest kept");
    assert_eq!(kept.status, grokhub_core::UpdateStatus::Unread);
    assert!(kept.pulse.off_home);
    drop(cfg);
}

// Landed from PR #94.
#[test]
fn rename_thread_sets_locked_title() {
    let _g = config::hold_test_config();
    let root = config::test_config_root("rename-thread");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads.push(ChatThread::new("Chat", false));
    cabin.rename_thread(0, "Harbor watch");
    assert_eq!(cabin.threads[0].title, "Harbor watch");
    assert!(cabin.threads[0].title_locked);
    assert_eq!(cabin.status, "Renamed Harbor watch");
    let _io = cabin.persist_io.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #95.
#[test]
fn command_palette_opens_and_navigates() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.open_palette();
    assert!(cabin.palette_open);
    assert!(cabin.palette_q.is_empty());
    cabin.run_palette(&egui::Context::default(), "nav:board");
    assert!(matches!(cabin.nav, Nav::Workboard));
    assert!(!cabin.palette_open);
    cabin.open_palette();
    cabin.run_palette(&egui::Context::default(), "nav:history");
    assert!(matches!(cabin.nav, Nav::History));
}

// Landed from PR #99.
#[test]
fn clear_chat_attach_clears_name_url_and_status() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.attach_name = Some("harbor.png".into());
    cabin.attach_url = Some("file://harbor.png".into());
    cabin.status = "Attached harbor.png".into();
    cabin.clear_chat_attach();
    assert!(cabin.attach_name.is_none());
    assert!(cabin.attach_url.is_none());
    assert!(cabin.status.is_empty());
}

// Landed from PR #100.
#[test]
fn sign_out_oauth_clears_the_session() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("sign-out");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.imagine_pending = true;
    cabin.sign_out_oauth();
    assert_eq!(cabin.status, "Signed out");
    assert!(cabin.secrets.oauth.is_none());
    assert!(!cabin.imagine_pending);
    assert!(cabin.oauth_pending.is_none());
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #101.
#[test]
fn new_thread_opens_another_chat_when_the_current_one_has_a_message() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("new-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin
            .threads
            .push(crate::threads::ChatThread::new("Harbor", false));
        cabin.thread_idx = 0;
    }
    let line = std::sync::Arc::new(vec![("user".into(), "paint the harbor".into())]);
    for t in &mut cabin.threads {
        t.messages = line.clone();
    }
    cabin.messages = line;
    let before = cabin.threads.len();
    cabin.new_thread(false);
    assert_eq!(cabin.threads.len(), before + 1);
    assert_eq!(cabin.status, "New chat");
    assert!(cabin.messages.is_empty());
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #102.
#[test]
fn open_history_hit_opens_pier_and_a_memory_file() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("history-open");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads.clear();
    let mut harbor = crate::threads::ChatThread::new("Harbor", false);
    let mut pier = crate::threads::ChatThread::new("Pier", false);
    let line = std::sync::Arc::new(vec![("user".into(), "paint the harbor".into())]);
    harbor.messages = line.clone();
    pier.messages = line.clone();
    let pier_id = pier.id.clone();
    cabin.threads.push(harbor);
    cabin.threads.push(pier);
    cabin.thread_idx = 0;
    cabin.messages = line;
    cabin.open_history_hit(&format!("thread:{pier_id}"));
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Pier");
    assert!(matches!(cabin.nav, Nav::Chat));
    cabin.open_history_hit("thread:missing");
    assert_eq!(cabin.status, "That chat is gone");
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Pier");
    cabin.open_history_hit("mem:USER.md");
    assert!(matches!(cabin.nav, Nav::Memory));
    assert_eq!(cabin.status, "USER.md");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #103.
#[test]
fn set_permission_mode_saves_auto_and_always() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("permission-mode");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.set_permission_mode(PermissionMode::Auto);
    assert_eq!(cabin.permission_mode, PermissionMode::Auto);
    assert_eq!(cabin.cfg.permission_mode, "auto");
    cabin.set_permission_mode(PermissionMode::AlwaysApprove);
    assert_eq!(cabin.permission_mode, PermissionMode::AlwaysApprove);
    assert_eq!(cabin.cfg.permission_mode, "ask");
    cabin.set_permission_mode(PermissionMode::Ask);
    assert_eq!(cabin.permission_mode, PermissionMode::Ask);
    assert_eq!(cabin.cfg.permission_mode, "ask");
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn halt_work_stops_a_running_turn() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("halt-work");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.running = true;
    cabin.imagine_pending = true;
    cabin.halt_work("Stopped");
    assert!(!cabin.running);
    assert!(!cabin.imagine_pending);
    assert_eq!(cabin.status, "Stopped");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #104.
#[test]
fn save_settings_stores_quiet_hours_and_clears_the_key() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("save-settings");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.api_key = "secret-key".into();
    cabin.quiet_start_buf = "22:00".into();
    cabin.quiet_end_buf = "07:00".into();
    cabin.cap_auto_buf = "12".into();
    cabin.cap_host_buf = "4".into();
    cabin.save_settings();
    assert_eq!(cabin.status, "Saved");
    assert!(cabin.cfg.api_key.is_empty());
    assert_eq!(cabin.cfg.quiet_start, "22:00");
    assert_eq!(cabin.cfg.quiet_end, "07:00");
    assert_eq!(cabin.cfg.daily_auto_cap, 12);
    assert_eq!(cabin.cfg.host_hour_cap, 4);
    std::env::remove_var("GROKHUB_CONFIG");
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
        "Worktrees were a Grok Build CLI feature; chats run in the bound project folder"
    );
    cabin.threads[cabin.thread_idx].grok_session = Some("sess-harbor".into());
    let (count, idx) = (cabin.threads.len(), cabin.thread_idx);
    cabin.run_slash_line("/fork");
    assert_eq!(cabin.threads.len(), count, "/fork no longer opens a chat");
    assert_eq!(cabin.thread_idx, idx);
    assert!(!cabin.threads[idx].grok_fork);
    assert_eq!(
        cabin.status,
        "Worktrees were a Grok Build CLI feature; chats run in the bound project folder",
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

// Landed from PR #133.
#[test]
fn discuss_card_opens_a_local_chat() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("discuss-card");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.discuss_card("missing");
    assert!(cabin.threads.is_empty());
    assert!(!cabin.running);
    let card = grokhub_core::idea_card("harbor", "Harbor lamp", "fold the charts", 1);
    let id = card.id.clone();
    cabin.updates.push(card);
    cabin.discuss_card(&id);
    assert!(cabin.idea_board.open.is_some() && cabin.nav == super::Nav::Pulse);
    assert!(!cabin.running);
    let thread_id = cabin
        .threads
        .iter()
        .find(|t| t.title == "Discuss · Harbor lamp")
        .expect("discuss thread")
        .id
        .clone();
    {
        let thread = cabin.threads.iter().find(|t| t.id == thread_id).unwrap();
        assert!(thread.background);
        assert!(thread.title_locked);
        assert!(thread
            .messages
            .iter()
            .any(|(_, body)| body.contains("Nothing runs until you press Apply")));
    }
    assert_ne!(cabin.threads[cabin.thread_idx].id, thread_id);
    let stuck = cabin.updates.iter().find(|c| c.id == id).expect("card");
    assert_eq!(stuck.status, grokhub_core::UpdateStatus::Opened);
    assert!(stuck.discuss_thread.is_some());
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

// Landed from PR #135.
#[test]
fn board_add_move_archive_restore_and_link() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("board-acts");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    let thread_id = cabin.threads[cabin.thread_idx].id.clone();
    cabin.board_title = "Harbor lamp".into();
    cabin.board_notes = "fold the charts".into();
    assert!(cabin.apply_board_act(Some(BoardAct::Add)));
    assert_eq!(cabin.board.len(), 1);
    let id = cabin.board[0].id.clone();
    assert_eq!(cabin.board[0].title, "Harbor lamp");
    assert_eq!(cabin.board[0].detail, "fold the charts");
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Todo);
    assert!(cabin.board_title.is_empty());
    assert!(!cabin.board_compose);
    assert!(cabin.board[0].thread_id.is_none());
    assert!(cabin.apply_board_act(Some(BoardAct::Move {
        id: id.clone(),
        status: grokhub_core::BoardStatus::InProgress,
    })));
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::InProgress);
    assert!(cabin.apply_board_act(Some(BoardAct::Archive(id.clone()))));
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Dismissed);
    assert!(cabin.apply_board_act(Some(BoardAct::Restore(id.clone()))));
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Todo);
    assert!(cabin.apply_board_act(Some(BoardAct::Link(id.clone()))));
    assert_eq!(
        cabin.board[0].thread_id.as_deref(),
        Some(thread_id.as_str())
    );
    assert!(cabin.apply_board_act(Some(BoardAct::Unlink(id))));
    assert!(cabin.board[0].thread_id.is_none());
    assert!(!cabin.running);
}

#[test]
fn background_model_calls_run_at_low_effort() {
    assert_eq!(grokhub_core::BACKGROUND_EFFORT, "low");
    let src = cabin_src() + include_str!("feed_ui.rs") + include_str!("native_unattended.rs");
    // Thread titles use built-in topics_from_turns (no model). Remaining
    // background asks still pin BACKGROUND_EFFORT.
    let goal = fn_src(&src, "spawn_thread_goal_on");
    assert!(
        goal.contains("topics_from_turns") && !goal.contains("BACKGROUND_EFFORT"),
        "thread titles must be local, not a low-effort model ask: {goal}"
    );
    for name in ["fast_once", "spawn_native_review", "spawn_native_loop"] {
        let body = fn_src(&src, name);
        assert!(
            body.contains("BACKGROUND_EFFORT"),
            "{name} runs on its own and must use low effort: {body}"
        );
    }
    // Chips, the greeting, ideas and the feed digest go through `fast_once`.
    for name in [
        "spawn_native_chips",
        "spawn_native_greeting",
        "spawn_native_ideas",
        "spawn_native_digest",
    ] {
        let body = fn_src(&src, name);
        assert!(
            body.contains("fast_once") || body.contains("fast_text"),
            "{name}: {body}"
        );
    }
}

#[test]
fn workboard_delete_removes_the_card_and_keeps_its_chat() {
    use super::pages::BoardAct;
    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    let thread_id = cabin.threads[cabin.thread_idx].id.clone();
    cabin.board_title = "Harbor lamp".into();
    assert!(cabin.apply_board_act(Some(BoardAct::Add)));
    cabin.board_title = "Tide chart".into();
    assert!(cabin.apply_board_act(Some(BoardAct::Add)));
    let id = cabin.board[0].id.clone();
    assert!(cabin.apply_board_act(Some(BoardAct::Link(id.clone()))));
    cabin.board_view.open = Some(id.clone());
    cabin
        .board_view
        .composers
        .insert(id.clone(), "half a thought".into());
    // A turn running in the card's chat has to stop first.
    cabin.running = true;
    cabin.chat_job_thread = Some(thread_id.clone());
    assert!(!cabin.apply_board_act(Some(BoardAct::Delete(id.clone()))));
    assert_eq!(cabin.board.len(), 2);
    assert!(cabin
        .board_view
        .note
        .as_ref()
        .is_some_and(|(n, _)| n == &id));
    cabin.running = false;
    cabin.chat_job_thread = None;
    assert!(cabin.apply_board_act(Some(BoardAct::Delete(id.clone()))));
    assert_eq!(cabin.board.len(), 1);
    assert!(cabin.board.iter().all(|c| c.id != id));
    assert!(cabin.board_view.open.is_none());
    assert!(cabin.board_view.composers.is_empty() && cabin.board_view.note.is_none());
    assert!(
        cabin.threads.iter().any(|t| t.id == thread_id),
        "the chat stays in History"
    );
    assert!(
        !cabin.apply_board_act(Some(BoardAct::Delete(id))),
        "already gone"
    );
    let archived = cabin.board[0].id.clone();
    assert!(cabin.apply_board_act(Some(BoardAct::Archive(archived.clone()))));
    assert!(cabin.apply_board_act(Some(BoardAct::Delete(archived))));
    assert!(
        cabin.board.is_empty(),
        "an archived card can be deleted too"
    );
    let src = cabin_src();
    let menu = fn_src(&src, "board_card_menu");
    assert!(
        menu.contains("BoardAct::Delete") && menu.contains("menu_button(\"Delete\""),
        "Delete sits behind a second step in the card menu: {menu}"
    );
}

// Landed from PR #136.
#[test]
fn memory_switch_flushes_the_file_you_left() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("memory-switch");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body = "harbor light".into();
    cabin.open_memory_file("SOUL.md");
    assert_eq!(cabin.mem_name, "SOUL.md");
    assert_eq!(cabin.mem_body, "");
    assert!(!cabin.running);
    // Read the file under this test's own root: the flush pins the directory it
    // was scheduled in, so the check must not depend on the process env later.
    let flushed = root.join("memory").join("MEMORY.md");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !std::fs::read_to_string(&flushed)
        .unwrap_or_default()
        .contains("harbor light")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "MEMORY.md was not flushed"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        std::fs::read_to_string(root.join("memory").join("SOUL.md")).unwrap_or_default(),
        ""
    );
}

// Landed from PR #137.
#[test]
fn night_add_lands_a_daily_job_and_a_loop() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("night-add");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.add_automation_seed("every day at 9, summarize the board");
    assert_eq!(cabin.status, "Automation added · daily at 09:00");
    assert_eq!(cabin.automations.len(), 1);
    assert_eq!(cabin.automations[0].schedule, "daily");
    assert_eq!(cabin.automations[0].time, "09:00");
    assert!(cabin.grok_loops.is_empty());
    cabin.add_automation_seed("/loop 30m check deploy");
    assert_eq!(cabin.status, "Loop added · every 30m");
    assert_eq!(cabin.grok_loops.len(), 1);
    assert_eq!(cabin.grok_loops[0].interval, "30m");
    cabin.add_automation_seed("what is rust");
    assert_eq!(
        cabin.status,
        "Need `/loop 30m …`, `every 2h …`, or `every day at 9 …`"
    );
    assert_eq!(cabin.automations.len(), 1);
    assert_eq!(cabin.grok_loops.len(), 1);
    assert!(!cabin.running);
}

// Landed from PR #138.
#[test]
fn chip_nav_changes_page_and_dismiss_drops_it() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("chip-acts");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    let chips = vec![grokhub_core::QuickChip {
        id: "nav-ideas".into(),
        label: "Ideas".into(),
        value: "__nav:ideas".into(),
        kind: grokhub_core::ChipKind::Nav,
        score: 1.0,
        hint: String::new(),
        primary: false,
    }];
    cabin.take_chip_act(crate::cards::ChipRowAct::Apply(0), &chips);
    assert!(matches!(cabin.nav, Nav::Pulse));
    assert!(!cabin.running);
    cabin.take_chip_act(crate::cards::ChipRowAct::Dismiss(0), &chips);
    assert!(cabin.chip_dismissed.iter().any(|d| d == "nav-ideas"));
    assert!(cabin.chip_dismissed.iter().any(|d| d == "__nav:ideas"));
}

// Landed from PR #139.
#[test]
fn attach_sets_the_chip_and_paste_lands_in_the_composer() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("plus-attach");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.apply_plus_ready(
        grokhub_core::PlusTarget::Chat,
        PlusReady {
            kind: grokhub_core::AttachKind::Image,
            name: "harbor.png".into(),
            raw: "/tmp/harbor.png".into(),
            image_url: Some("data:image/png;base64,aGk=".into()),
            text: None,
        },
    );
    assert_eq!(cabin.attach_name.as_deref(), Some("harbor.png"));
    assert_eq!(
        cabin.attach_url.as_deref(),
        Some("data:image/png;base64,aGk=")
    );
    assert_eq!(
        cabin.status,
        "Attached harbor.png — sends with the next message"
    );
    assert!(!cabin.running);
    assert!(cabin.messages.is_empty());
    cabin.composer = "hello".into();
    cabin.apply_clipboard(grokhub_core::PlusTarget::Chat, "harbor light\n");
    assert_eq!(cabin.composer, "hello\nharbor light");
    assert_eq!(cabin.status, "Pasted clipboard");
    cabin.apply_plus_ready(
        grokhub_core::PlusTarget::Chat,
        PlusReady {
            kind: grokhub_core::AttachKind::Text,
            name: "note.txt".into(),
            raw: "/tmp/note.txt".into(),
            image_url: None,
            text: Some("fold the charts".into()),
        },
    );
    assert_eq!(cabin.composer, "hello\nharbor light");
    assert_eq!(cabin.attach_tray.len(), 2);
    assert_eq!(cabin.attach_name.as_deref(), Some("harbor.png"));
    assert_eq!(cabin.status, "File note.txt — the agent will read it");
    assert!(!cabin.running);
    assert!(cabin.messages.is_empty());
}

// Landed from PR #140.
#[test]
fn feed_open_routes_an_idea_and_dismiss_removes_it() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("feed-open");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.open_feed_card("missing");
    assert!(matches!(cabin.nav, Nav::Chat));
    let card = grokhub_core::idea_card("harbor", "Harbor lamp", "fold the charts", 1);
    let id = card.id.clone();
    cabin.updates.push(card);
    cabin.updates.last_mut().unwrap().feed_pin = true;
    cabin.open_feed_card(&id);
    assert!(cabin.idea_board.open.is_some() && cabin.nav == super::Nav::Pulse);
    assert!(cabin
        .threads
        .iter()
        .any(|t| t.background && t.title == "Discuss · Harbor lamp"));
    assert_ne!(
        cabin.threads[cabin.thread_idx].title,
        "Discuss · Harbor lamp"
    );
    cabin.open_feed_card(&id);
    assert_eq!(
        cabin
            .threads
            .iter()
            .filter(|t| t.title == "Discuss · Harbor lamp")
            .count(),
        1
    );
    cabin.dismiss_feed_card(&id);
    let kept = cabin
        .updates
        .iter()
        .find(|c| c.id == id)
        .expect("stays on the board");
    assert!(!kept.feed_pin && kept.feed_kept);
    assert!(!cabin.running);
}

// Landed from PR #141.
#[test]
fn session_pills_save_chat_and_ask() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("session-pills");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.set_session_mode(SessionMode::Chat);
    assert_eq!(cabin.session_mode, SessionMode::Chat);
    assert_eq!(cabin.cfg.session_mode, "chat");
    cabin.set_session_mode(SessionMode::Ask);
    assert_eq!(cabin.session_mode, SessionMode::Ask);
    assert_eq!(cabin.cfg.session_mode, "ask");
    assert!(!cabin.running);
}

// Landed from PR #142.
#[test]
fn open_board_thread_opens_the_linked_chat() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("board-open");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.open_board_thread("missing");
    assert_eq!(cabin.status, "Linked chat is gone");
    assert!(cabin.threads.is_empty());
    assert!(!cabin.running);
    cabin.new_thread(false);
    let id = cabin.threads[cabin.thread_idx].id.clone();
    cabin.nav = Nav::Workboard;
    cabin.open_board_thread(&id);
    assert!(matches!(cabin.nav, Nav::Chat));
    assert_eq!(cabin.threads[cabin.thread_idx].id, id);
    assert!(!cabin.running);
}

// Landed from PR #143.
#[test]
fn store_session_plan_opens_once_and_keeps_the_first() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("session-plan");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.store_session_plan("   ", true);
    assert!(cabin.threads.is_empty());
    assert!(!cabin.plan_open);
    cabin.new_thread(false);
    cabin.store_session_plan("  fold the charts  ", false);
    assert!(cabin.plan_open);
    assert_eq!(cabin.threads[cabin.thread_idx].plan_body, "fold the charts");
    cabin.store_session_plan("other plan", false);
    assert_eq!(cabin.threads[cabin.thread_idx].plan_body, "fold the charts");
    cabin.store_session_plan("other plan", true);
    assert_eq!(cabin.threads[cabin.thread_idx].plan_body, "other plan");
    assert!(cabin.plan_open);
    assert!(!cabin.running);
}

// Landed from PR #144.
#[test]
fn board_edit_fills_the_form_and_save_writes_it() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("board-edit");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.apply_board_act(Some(BoardAct::Edit("missing".into()))));
    assert!(!cabin.board_compose);
    cabin.new_thread(false);
    let thread_id = cabin.threads[cabin.thread_idx].id.clone();
    cabin.board_title = "Harbor lamp".into();
    cabin.board_notes = "fold the charts".into();
    assert!(cabin.apply_board_act(Some(BoardAct::Add)));
    let id = cabin.board[0].id.clone();
    assert!(!cabin.apply_board_act(Some(BoardAct::Edit(id.clone()))));
    assert!(cabin.board_compose);
    assert_eq!(cabin.board_edit.as_deref(), Some(id.as_str()));
    assert_eq!(cabin.board_title, "Harbor lamp");
    assert_eq!(cabin.board_notes, "fold the charts");
    assert!(!cabin.board_link);
    cabin.board_title = "  Harbor dock  ".into();
    cabin.board_notes = "  tie the lines  ".into();
    cabin.board_link = true;
    assert!(cabin.apply_board_act(Some(BoardAct::Save(id))));
    assert_eq!(cabin.board[0].title, "Harbor dock");
    assert_eq!(cabin.board[0].detail, "tie the lines");
    assert_eq!(
        cabin.board[0].thread_id.as_deref(),
        Some(thread_id.as_str())
    );
    assert!(!cabin.board_compose);
    assert!(cabin.board_edit.is_none());
    assert!(cabin.board_title.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #145.
#[test]
fn bubble_reply_quotes_into_the_composer() {
    fn reply_like_the_button(cabin: &mut Cabin, body: &str) {
        cabin.composer = append_composer(&cabin.composer, &quote_for_reply(body));
        if !cabin.composer.ends_with('\n') {
            cabin.composer.push('\n');
        }
        cabin.composer_want_focus = true;
    }

    let mut plain = Cabin::quiet_for_test();
    plain.composer = "notes for later".into();
    assert!(!plain.running);
    assert!(!plain.composer_want_focus);
    reply_like_the_button(&mut plain, "Ship the harbor");
    assert_eq!(plain.composer, "notes for later\n> Ship the harbor\n");
    assert!(plain.composer.starts_with("notes for later\n"));
    assert!(plain.composer_want_focus);
    assert!(!plain.running);

    let mut marked = Cabin::quiet_for_test();
    marked.composer = "notes for later".into();
    assert!(!marked.running);
    assert!(!marked.composer_want_focus);
    reply_like_the_button(&mut marked, "> already quoted");
    assert_eq!(marked.composer, "notes for later\n> > already quoted\n");
    assert!(marked.composer.starts_with("notes for later\n"));
    assert!(marked.composer_want_focus);
    assert!(!marked.running);
}

// Landed from PR #146.
#[test]
fn grok_history_open_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("grok-hist");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let id = "sess-quiet-known";
    let title = "Known cabin title";
    let mut cabin = super::Cabin::quiet_for_test();
    cabin
        .grok_sessions
        .push(grokhub_core::cli_history::GrokSession {
            id: id.to_string(),
            title: title.to_string(),
            path: None,
            cwd: None,
            cabin: false,
        });

    cabin.open_grok_session(id);
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].grok_session.as_deref(), Some(id));
    assert_eq!(cabin.threads[0].title, title);
    assert!(cabin.nav == super::Nav::Chat);
    assert_eq!(cabin.status, format!("Opened {title}"));
    assert!(!cabin.running);

    cabin.open_grok_session(id);
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].grok_session.as_deref(), Some(id));
    assert!(!cabin.running);
}

// Landed from PR #147.
#[test]
fn grok_tasks_and_commands_stay_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("grok-tasks");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.apply_grok_task("task-1".into(), "Draft the note".into(), false);
    assert_eq!(
        cabin.grok_tasks,
        vec![("task-1".into(), "Draft the note".into(), false)]
    );
    assert!(!cabin.running);

    cabin.apply_grok_task("task-1".into(), "Note is filed".into(), true);
    assert_eq!(
        cabin.grok_tasks,
        vec![("task-1".into(), "Note is filed".into(), true)]
    );
    assert!(!cabin.running);

    let owned = grokhub_core::filter_slash_commands("/")
        .first()
        .expect("cabin slash")
        .cmd
        .to_string();
    let foreign = "create-skill".to_string();
    let names = vec![foreign.clone(), "   ".into(), String::new(), owned.clone()];
    let hits = grokhub_core::grok_command_hits(&names);
    cabin.apply_grok_commands(names);
    assert_eq!(cabin.grok_commands, hits);
    assert!(
        cabin
            .grok_commands
            .iter()
            .any(|h| h.cmd == format!("/{foreign}")),
        "a name the cabin does not own stays"
    );
    assert!(
        cabin.grok_commands.iter().all(|h| {
            let name = h.cmd.trim_start_matches('/').trim();
            !name.is_empty() && h.cmd != owned
        }),
        "a blank name and a cabin-owned slash are dropped"
    );
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #148.
#[test]
fn chip_apply_mode_and_help_stay_off_a_run() {
    struct RestoreEnv148 {
        config: Option<std::ffi::OsString>,
        grok: Option<std::ffi::OsString>,
    }
    impl Drop for RestoreEnv148 {
        fn drop(&mut self) {
            match self.config.take() {
                Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
                None => std::env::remove_var("GROKHUB_CONFIG"),
            }
            match self.grok.take() {
                Some(v) => std::env::set_var("GROKHUB_GROK", v),
                None => std::env::remove_var("GROKHUB_GROK"),
            }
        }
    }

    fn chip(id: &str, value: &str, kind: super::ChipKind) -> super::QuickChip {
        super::QuickChip {
            id: id.into(),
            label: id.into(),
            value: value.into(),
            kind,
            score: 1.0,
            hint: String::new(),
            primary: false,
        }
    }

    fn settle(cabin: &super::Cabin) {
        let _io = cabin.persist_io.lock().unwrap_or_else(|e| e.into_inner());
    }

    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("chip-apply");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    let _restore = RestoreEnv148 {
        config: std::env::var_os("GROKHUB_CONFIG"),
        grok: std::env::var_os("GROKHUB_GROK"),
    };
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", "/no/such/grok-binary-xyz");

    let mut mode = super::Cabin::quiet_for_test();
    mode.apply_chip(chip("mode-fast", "__mode:fast", super::ChipKind::Mode));
    assert_eq!(mode.cfg.mode, "fast");
    assert_eq!(
        mode.status,
        super::mode_status_line("fast", &mode.cfg.model)
    );
    assert!(!mode.running);

    let mut typed_help = super::Cabin::quiet_for_test();
    typed_help.send_from_composer("/help".into());
    let mut help = super::Cabin::quiet_for_test();
    help.apply_chip(chip("help", "/help", super::ChipKind::Chat));
    assert_eq!(help.status, typed_help.status);
    assert!(!help.running);
    assert!(!typed_help.running);
    let help_body = super::mark_slash_result(&super::slash_help());
    assert_eq!(
        help.messages
            .last()
            .map(|(role, text)| (role.as_str(), text.as_str())),
        Some(("assistant", help_body.as_str()))
    );
    assert_eq!(help.messages.last(), typed_help.messages.last());

    let sentence = "Tell me about the harbor.";
    let mut typed_chat = super::Cabin::quiet_for_test();
    typed_chat.send_from_composer(sentence.into());
    let mut chat = super::Cabin::quiet_for_test();
    chat.apply_chip(chip("say", sentence, super::ChipKind::Chat));
    assert_eq!(
        chat.status, typed_chat.status,
        "a plain chip must refuse with the composer install/connect status"
    );
    assert_eq!(chat.status, grokhub_core::XAI_NEED_SIGNIN);
    assert!(!chat.running);
    assert!(!typed_chat.running);
    assert!(chat.messages.is_empty());
    assert!(typed_chat.messages.is_empty());

    settle(&mode);
    settle(&typed_help);
    settle(&help);
    settle(&typed_chat);
    settle(&chat);
}

// Landed from PR #150.
#[test]
fn set_nav_id_opens_each_page() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("set-nav");
    let _ = std::fs::create_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(matches!(cabin.nav, super::Nav::Chat));

    cabin.set_nav_id("settings");
    assert!(matches!(cabin.nav, super::Nav::Settings));
    assert!(matches!(cabin.settings_back, super::Nav::Chat));
    assert!(matches!(cabin.settings_sec, super::SettingsSec::Account));
    assert!(!cabin.running);
    cabin.set_nav_id("settings");
    assert!(matches!(cabin.nav, super::Nav::Settings));
    assert!(matches!(cabin.settings_back, super::Nav::Chat));
    assert!(matches!(cabin.settings_sec, super::SettingsSec::Account));
    assert!(!cabin.running);

    cabin.set_nav_id("history");
    assert!(matches!(cabin.nav, super::Nav::History));
    assert!(!cabin.running);

    cabin.set_nav_id("journal");
    assert!(matches!(cabin.nav, super::Nav::Journal));
    assert!(!cabin.running);

    cabin.set_nav_id("imagine");
    assert!(matches!(cabin.nav, super::Nav::Imagine));
    assert!(cabin.imagine_want_focus);
    assert!(!cabin.running);

    cabin.set_nav_id("workboard");
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    assert!(!cabin.running);

    cabin.set_nav_id("skills");
    assert!(matches!(cabin.nav, super::Nav::Skills));
    assert!(!cabin.running);

    cabin.set_nav_id("automations");
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert!(!cabin.running);

    cabin.set_nav_id("command");
    assert!(matches!(cabin.nav, super::Nav::Command));
    assert!(!cabin.running);

    cabin.set_nav_id("queue");
    assert!(matches!(cabin.nav, super::Nav::Agents));
    assert!(!cabin.running);

    cabin.set_nav_id("devices");
    assert!(matches!(cabin.nav, super::Nav::Devices));
    assert!(!cabin.running);

    cabin.set_nav_id("memory");
    assert!(matches!(cabin.nav, super::Nav::Memory));
    assert!(!cabin.running);

    cabin.set_nav_id("connectors");
    assert!(matches!(cabin.nav, super::Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    assert!(!cabin.running);

    let threads_before = cabin.threads.len();
    let idx_before = cabin.thread_idx;
    cabin.composer_want_focus = false;
    cabin.chat_tail_frames = 0;
    cabin.set_nav_id("eyes");
    assert!(matches!(cabin.nav, super::Nav::Chat));
    assert_eq!(cabin.threads.len(), threads_before);
    assert_eq!(cabin.thread_idx, idx_before);
    assert!(cabin.composer_want_focus);
    assert_eq!(cabin.chat_tail_frames, grokhub_core::CHAT_TAIL_FRAMES);
    assert!(!cabin.running);

    let threads_before = cabin.threads.len();
    let status_before = cabin.status.clone();
    cabin.set_nav_id("not-a-page");
    assert!(matches!(cabin.nav, super::Nav::Chat));
    assert_eq!(cabin.threads.len(), threads_before);
    assert_eq!(cabin.status, status_before);
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);

    let draft = crate::threads::ChatThread::new("Chat", false);
    let id = draft.id.clone();
    cabin.threads.push(draft);
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    let n = cabin.threads.len();
    let io = cabin.persist_io.clone();
    let block = io.lock().unwrap_or_else(|e| e.into_inner());
    cabin.set_nav_id("chat");
    drop(block);
    assert!(matches!(cabin.nav, super::Nav::Chat));
    assert_eq!(cabin.threads.len(), n);
    assert_eq!(cabin.threads[0].id, id);
    assert!(cabin.messages.is_empty());
    assert_eq!(cabin.status, "New chat");
    assert!(!cabin.running);

    std::thread::sleep(std::time::Duration::from_millis(100));
    drop(io.lock().unwrap_or_else(|e| e.into_inner()));
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #152.
#[test]
fn followup_queue_drains_one_and_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("followup-queue");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", root.join("missing-grok"));

    let mut composer = Cabin::quiet_for_test();
    assert!(!composer.running);
    composer.send_chat("/help".into());
    let help_status = composer.status.clone();

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.drain_followup_queue();
    assert!(cabin.followup_queue.is_empty());
    assert!(!cabin.running);

    let second = "second line stays queued".to_string();
    cabin.followup_queue = vec!["/help".into(), second.clone()];
    cabin.drain_followup_queue();
    assert_eq!(cabin.followup_queue, vec![second]);
    assert_eq!(cabin.status, help_status);
    assert_eq!(cabin.messages.as_ref(), composer.messages.as_ref());
    assert!(!cabin.running);

    cabin.followup_queue = vec!["/approve".into()];
    cabin.drain_followup_queue();
    assert_eq!(cabin.status, "Unknown command — /help");
    assert!(!cabin.running);

    let sentence = "plain sentence with no grok".to_string();
    cabin.followup_queue = vec![sentence.clone()];
    cabin.drain_followup_queue();
    assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
    assert!(
        cabin
            .messages
            .iter()
            .all(|(_, body)| !body.contains(&sentence)),
        "the plain sentence must stay off the chat"
    );
    assert!(!cabin.running);

    std::env::remove_var("GROKHUB_GROK");
    std::env::remove_var("GROKHUB_CONFIG");
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
                cabin.status, "Workflows were a Grok Build CLI feature and are not in GrokHub",
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

#[test]
fn connectors_and_hooks_do_not_write_grok_home() {
    let src = cabin_src();
    let mut blob = String::new();
    for name in ["ui_hooks_section", "ui_settings_connectors"] {
        blob.push_str(fn_src(&src, name));
        blob.push('\n');
    }
    for forbidden in [
        "config.toml",
        "mcp_credentials.json",
        "trusted_folders",
        "fs::write",
        "File::create",
        "OpenOptions",
        "grok_user_stdout",
    ] {
        assert!(
            !blob.contains(forbidden),
            "{forbidden} must not appear in the connectors code"
        );
    }
    let hooks = fn_src(&src, "ui_hooks_section");
    assert!(hooks.contains("No hooks in ~/.grok/hooks or this project's .grok/hooks."));
    assert!(hooks.contains("set_folder_trust"), "{hooks}");
    assert!(hooks.contains("scroll_to_me") && hooks.contains("scroll_to_hooks"));
    assert!(hooks.contains("Align::TOP"));
    assert!(!hooks.contains("ghost_pill") && !hooks.contains("white_pill"));
}

// Landed from PR #153.
#[test]
fn thinking_status_shows_context_when_usage_is_present() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("thinking-status");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert_eq!(cabin.thinking_status(), "Thinking…");

    let usage = GrokUsage {
        context_tokens_used: 26000,
        context_window_tokens: 500000,
        ..GrokUsage::default()
    };
    cabin.grok_usage = usage.clone();
    let line = grok_context_line(&usage);
    assert_eq!(cabin.thinking_status(), format!("Thinking… {line}"));
    assert!(!cabin.running);
}

// Landed from PR #156.
#[test]
fn shell_chip_stays_off_a_run_when_host_is_blocked() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("shell-chip");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let project = root.join("proj");
    std::fs::create_dir_all(&project).expect("project");
    let project_dir = project.to_string_lossy().into_owned();
    let cmd = "cat /etc/passwd".to_string();
    assert!(
        grokhub_core::host_cmd_leaves_project(&cmd, &project_dir),
        "cat /etc/passwd must leave the bound project"
    );

    let chip = grokhub_core::QuickChip {
        id: "shell".into(),
        label: "Shell".into(),
        value: cmd,
        kind: grokhub_core::ChipKind::Shell,
        score: 1.0,
        hint: String::new(),
        primary: false,
    };

    let mut off = super::Cabin::quiet_for_test();
    off.cfg.host_on = false;
    off.cfg.project_dir = project_dir.clone();
    off.permission_mode = grokhub_core::wire::PermissionMode::Ask;
    off.running = false;
    off.apply_chip(chip.clone());
    assert_eq!(off.status, "Host off — /host on");
    assert!(!off.running);

    let mut ask = super::Cabin::quiet_for_test();
    ask.cfg.host_on = true;
    ask.cfg.project_dir = project_dir;
    ask.permission_mode = grokhub_core::wire::PermissionMode::Ask;
    ask.running = false;
    ask.apply_chip(chip);
    assert!(
        !ask.running,
        "running became true; the command was inside the project"
    );
    let chat = ask
        .messages
        .iter()
        .map(|(_, body)| body.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        chat.contains("blocked: outside bound project"),
        "chat missing outside-project block: {chat}"
    );
}

// Landed from PR #157.
#[test]
fn feed_pulse_ticks_without_a_chat() {
    let _cfg = crate::config::hold_test_config();
    let prev_config = std::env::var("GROKHUB_CONFIG").ok();
    let root = crate::config::test_config_root("feed-pulse");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    // Whole-day quiet so Housekeep holds the digest clock and does not post a card.
    cabin.cfg.quiet_start = "00:00".into();
    cabin.cfg.quiet_end = "23:59".into();
    let before = cabin.cfg.feed_pulse.clone();
    assert!(!cabin.running);
    assert!(
        cabin.updates.is_empty() && !grokhub_core::feed_visible(&cabin.updates),
        "an empty feed stays hidden"
    );

    cabin.tick_feed_pulse();
    assert_ne!(
        cabin.cfg.feed_pulse, before,
        "one tick advances the stored FeedPulse"
    );
    assert!(!cabin.running, "the feed pulse must not start a run");
    assert!(
        cabin.updates.is_empty() && !grokhub_core::feed_visible(&cabin.updates),
        "an empty feed stays hidden"
    );

    let advanced = cabin.cfg.feed_pulse.clone();
    cabin.tick_feed_pulse();
    assert!(!cabin.running, "a second tick does not start a run");
    assert_eq!(
        cabin.cfg.feed_pulse, advanced,
        "a second tick does not start a run"
    );
    assert!(
        cabin.updates.is_empty() && !grokhub_core::feed_visible(&cabin.updates),
        "an empty feed stays hidden"
    );

    let io = cabin.persist_io.clone();
    let path = root.join("app.json");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if std::fs::read_to_string(&path)
            .map(|body| body.contains("feedPulse"))
            .unwrap_or(false)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _disk = std::mem::ManuallyDrop::new(io.lock().unwrap_or_else(|e| e.into_inner()));
    match prev_config {
        Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
        None => std::env::remove_var("GROKHUB_CONFIG"),
    }
}

// Landed from PR #158.
#[test]
fn composer_action_chips_stay_off_a_run() {
    let _hold = config::hold_test_config();
    let root = config::test_config_root("composer-action-chips");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(!cabin.running);

    const SENT: &str = grokhub_core::XAI_NEED_SIGNIN;

    fn click(cabin: &mut Cabin, label: &str) {
        cabin.refresh_chips();
        let chips = cabin.composer_chips();
        let chip = chips
            .iter()
            .find(|c| c.label == label)
            .cloned()
            .unwrap_or_else(|| {
                let labels: Vec<_> = chips.iter().map(|c| c.label.as_str()).collect();
                panic!("missing {label} among {labels:?}");
            });
        assert!(matches!(chip.kind, ChipKind::Chat), "{label}");
        let before = cabin.messages.len();
        let value = chip.value.clone();
        cabin.apply_chip(chip);
        assert!(!cabin.running, "{label}");
        assert!(matches!(cabin.nav, Nav::Chat));
        assert_eq!(cabin.messages.len(), before, "{label} transcript");
        assert!(
            cabin.messages.iter().all(|(_, line)| line != SENT),
            "{label} transcript gained the status line"
        );
        if cabin.composer.is_empty() {
            assert_eq!(cabin.status, SENT, "{label}");
        } else {
            assert_eq!(cabin.composer, value, "{label}");
        }
    }

    cabin.composer = "paint the north wall".into();
    click(&mut cabin, "Expand & send");

    cabin.messages = Arc::new(vec![
        ("user".into(), "We started the cabin wall.".into()),
        (
            "assistant".into(),
            "The north wall has one coat already.".into(),
        ),
    ]);
    cabin.composer.clear();
    cabin.chip_paint_key.clear();
    click(&mut cabin, "Continue");

    cabin.messages = Arc::new(vec![
        ("user".into(), "Look at the cabin logs.".into()),
        (
            "assistant".into(),
            "I'll check the cabin logs and then run a short probe.".into(),
        ),
    ]);
    cabin.composer.clear();
    cabin.chip_paint_key.clear();
    click(&mut cabin, "Finish — run tools now");
    click(&mut cabin, "Finish the job");
}

// Landed from PR #159.
#[test]
fn housekeep_keeps_ideas_until_newer_ones_push_them_out() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("idea-expiry");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    // Heartbeat must not ask the real model from a test.
    cabin.cfg.feed_pulse.last_ideas_ms = grokhub_core::now_ms();
    assert!(!cabin.running);
    assert!(
        !grokhub_core::feed_visible(&cabin.updates),
        "an empty feed stays hidden"
    );

    let now = grokhub_core::now_ms();
    let stale_at = now
        .saturating_sub(grokhub_core::IDEA_TTL_MS)
        .saturating_sub(1);
    let stale = grokhub_core::idea_card("old", "Stale idea", "aged out", stale_at);
    let fresh = grokhub_core::idea_card("new", "Fresh idea", "still good", now);
    assert!(!stale.kind.event() && !fresh.kind.event());
    crate::feed::save(&[stale.clone(), fresh.clone()]).expect("updates.json");
    cabin.updates = crate::feed::load();

    cabin.tick_feed_pulse();
    assert!(
        cabin.ideas_rx.is_none(),
        "housekeep must not start the ideas model call"
    );

    assert!(
        cabin
            .updates
            .iter()
            .all(|card| card.status != grokhub_core::UpdateStatus::Dismissed),
        "housekeep expiry is the age path, not a dismiss"
    );
    assert!(
        cabin.updates.iter().any(|card| card.id == stale.id),
        "ideas no longer time out; a newer card pushes out the oldest instead"
    );
    assert!(
        cabin.updates.iter().any(|card| card.id == fresh.id),
        "a fresh idea stays"
    );
    assert!(!cabin.running);
    assert!(
        grokhub_core::visible_updates(&cabin.updates).is_empty(),
        "ideas must not invent event rows or take the paint cap of 3"
    );
    assert_eq!(
        grokhub_core::visible_ideas(&cabin.updates)
            .iter()
            .filter(|card| card.id == fresh.id)
            .count(),
        1
    );

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #160.
#[test]
fn worktree_stays_off_a_send() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("worktree-send");
    let _ = std::fs::create_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.new_thread(false);
    let seeded = cabin
        .threads
        .get_mut(cabin.thread_idx)
        .expect("quiet cabin needs one thread before worktree");
    seeded.grok_session = Some("sess".into());

    cabin.run_slash(super::Slash::Worktree);
    assert_eq!(
        cabin.status,
        "Worktrees were a Grok Build CLI feature; chats run in the bound project folder"
    );
    assert!(!cabin.running);
    assert_eq!(
        cabin.threads[cabin.thread_idx].grok_session.as_deref(),
        Some("sess"),
        "/worktree leaves the chat's session alone"
    );
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

// Landed from PR #162.
fn collect_shape_text(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_shape_text(shape, out);
            }
        }
        _ => {}
    }
}

#[test]
fn empty_queue_keeps_nothing_queued_label() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("empty-queue");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.followup_queue.is_empty());

    let chips = cabin.composer_chips();
    assert!(
        chips.is_empty(),
        "a quiet cabin with an empty follow-up queue has an empty chip row"
    );

    let mut texts = Vec::new();
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            assert!(crate::cards::quick_chip_row(ui, &chips).is_none());
            let layer = ui.layer_id();
            ui.ctx().graphics(|layers| {
                if let Some(list) = layers.get(layer) {
                    for clipped in list.all_entries() {
                        collect_shape_text(&clipped.shape, &mut texts);
                    }
                }
            });
        });
    });

    assert_eq!(crate::cards::CHIP_EMPTY_LABEL, "Nothing queued");
    assert_eq!(
        texts,
        vec![crate::cards::CHIP_EMPTY_LABEL.to_string()],
        "an empty follow-up queue shows exactly the empty-queue label"
    );
    assert!(!cabin.running);
    assert!(cabin.followup_queue.is_empty());

    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #163.
#[test]
fn hub_and_memory_show_stay_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("hub-memory");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.run_slash(Slash::Hub);
    assert!(matches!(cabin.nav, Nav::Devices));
    assert_eq!(cabin.status, "Start share on Devices");
    assert!(!cabin.hub_on);
    assert!(!cabin.running);
    let sharing = cabin.hub.lock().expect("hub").sharing;
    assert!(!sharing);

    cabin.hub_on = true;
    cabin.run_slash(Slash::Hub);
    assert_eq!(cabin.status, "Hub sharing");
    assert!(!cabin.running);
    let sharing = cabin.hub.lock().expect("hub").sharing;
    assert!(!sharing);

    cabin.run_slash(Slash::MemoryShow);
    assert!(matches!(cabin.nav, Nav::Memory));
    assert_eq!(cabin.status, "Memory");
    assert!(!cabin.running);

    cabin
        .threads
        .push(crate::threads::ChatThread::new("Scratch", true));
    cabin.thread_idx = cabin.threads.len() - 1;
    assert!(cabin.scratch());
    cabin.mem_body = "sentinel-memory".into();
    let mem_body = cabin.mem_body.clone();
    cabin.run_slash(Slash::Forget(None));
    assert_eq!(cabin.status, "Scratch — no memory writes");
    assert_eq!(cabin.mem_body, mem_body);
    assert!(!cabin.running);
}

// Landed from PR #164.
#[test]
fn palette_opens_and_diagnostics_stay_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("palette-diag");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.open_palette();
    assert!(cabin.palette_open);
    assert!(cabin.palette_focus);
    assert!(cabin.palette_q.is_empty());
    assert!(cabin.palette_file_rx.is_none());
    assert!(!cabin.settings_menu_open);
    assert!(!cabin.running);

    // Copy diagnostics puts the bundle on the clipboard, not in the status line.
    let ctx = egui::Context::default();
    let out = crate::theme::test_pass(&ctx, Default::default(), |_| {
        cabin.run_palette(&ctx, "diag");
    });
    assert!(!cabin.palette_open);
    assert_eq!(cabin.status, "Diagnostics copied");
    let copied: Vec<&String> = out
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(copied.len(), 1, "{copied:?}");
    assert!(
        copied[0].starts_with("app GrokHub\nversion "),
        "{}",
        copied[0]
    );
    assert!(
        copied[0].contains(env!("CARGO_PKG_VERSION")),
        "{}",
        copied[0]
    );
    assert!(!cabin.running);

    cabin.run_palette(&egui::Context::default(), "nav:night");
    assert!(matches!(cabin.nav, Nav::Night));
    assert!(!cabin.palette_open);
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #165.
fn grok_process_ids() -> Vec<u32> {
    let mut ids = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return ids;
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(pid_str) = name.to_str() else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        let comm_is_grok = comm.trim() == "grok";
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        let argv0 = cmdline.split(|b| *b == 0).next().unwrap_or(&[]);
        let exe_is_grok = std::path::Path::new(std::str::from_utf8(argv0).unwrap_or(""))
            .file_name()
            .and_then(|s| s.to_str())
            == Some("grok");
        if comm_is_grok || exe_is_grok {
            ids.push(pid);
        }
    }
    ids.sort_unstable();
    ids
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

// Landed from PR #166.
#[test]
fn heartbeat_pulse_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("heartbeat_pulse_stays_off_a_run");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    // Heartbeat must not ask the real model from a test.
    cabin.cfg.feed_pulse.last_ideas_ms = grokhub_core::now_ms();
    cabin.hub_on = false;
    cabin.automations.clear();
    cabin.running = false;
    cabin.last_activity = std::time::Instant::now();
    cabin.last_heartbeat = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(30))
        .expect("age last_heartbeat");

    cabin.tick_heartbeat();

    assert!(
        cabin.ideas_rx.is_none(),
        "heartbeat must not start the ideas model call"
    );
    assert!(!cabin.running);
    assert!(cabin.pending_hub_task.is_none());
    assert!(cabin.night_check_rx.is_none());
    assert!(cabin.last_heartbeat.elapsed() < std::time::Duration::from_secs(5));

    let stamped = cabin.last_heartbeat;
    cabin.tick_heartbeat();

    assert!(
        cabin.ideas_rx.is_none(),
        "heartbeat must not start the ideas model call"
    );
    assert!(!cabin.running);
    assert!(cabin.pending_hub_task.is_none());
    assert!(cabin.night_check_rx.is_none());
    assert_eq!(cabin.last_heartbeat, stamped);
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

// Landed from PR #169.
#[test]
fn settings_section_titles_match_the_page() {
    assert_eq!(
        super::settings::settings_sec_title(super::SettingsSec::Account),
        "Account"
    );
    assert_eq!(
        super::settings::settings_sec_title(super::SettingsSec::Appearance),
        "Appearance"
    );
    assert_eq!(
        super::settings::settings_sec_title(super::SettingsSec::Behavior),
        "Behavior"
    );
    assert_eq!(
        super::settings::settings_sec_title(super::SettingsSec::Update),
        "Update"
    );
    assert_eq!(
        super::settings::settings_sec_title(super::SettingsSec::About),
        "About"
    );
    assert!(matches!(
        super::settings::settings_group_home(super::SettingsGroup::General),
        super::SettingsSec::Account
    ));
    assert!(matches!(
        super::settings::settings_group_home(super::SettingsGroup::About),
        super::SettingsSec::Update
    ));
}

// Landed from PR #170.
#[test]
fn a_turn_that_cannot_start_stays_off_a_run() {
    struct RestoreConfig170(Option<String>);
    impl Drop for RestoreConfig170 {
        fn drop(&mut self) {
            match self.0.take() {
                Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
                None => std::env::remove_var("GROKHUB_CONFIG"),
            }
        }
    }

    let _cfg = crate::config::hold_test_config();
    let _restore = RestoreConfig170(std::env::var("GROKHUB_CONFIG").ok());
    let root = crate::config::test_config_root("ask-denied");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    for detail in ["native engine is not running", "timeout"] {
        cabin.running = true;
        cabin.pending_kick = Some(true);
        cabin.chat_job_thread = Some("job".into());
        cabin.fail_turn_start(detail);
        assert!(cabin.status.contains(detail), "{detail}: {}", cabin.status);
        assert!(!cabin.running);
        assert!(cabin.pending_kick.is_none());
        assert!(cabin.chat_job_thread.is_none());
    }
}

// Landed from PR #171.
#[test]
fn history_hit_opens_memory_or_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("history-hit");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.open_history_hit("mem:MEMORY.md");
    assert!(matches!(cabin.nav, Nav::Memory));
    assert_eq!(cabin.status, "MEMORY.md");
    assert_eq!(cabin.mem_name, "MEMORY.md");
    assert!(!cabin.running);

    cabin.open_history_hit("thread:nope");
    assert_eq!(cabin.status, "That chat is gone");
    assert!(!cabin.running);

    cabin.new_thread(false);
    cabin.new_thread(false);
    let id = cabin
        .threads
        .get(cabin.thread_idx)
        .map(|t| t.id.clone())
        .expect("seeded thread");
    cabin.open_history_hit(&format!("thread:{id}"));
    assert_eq!(cabin.threads[cabin.thread_idx].id, id);
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(!cabin.running);
}

// Landed from PR #172.
#[test]
fn delete_all_history_leaves_one_empty_chat() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("delete-all-history");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.new_thread(false);
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "note".into())]);
    cabin.new_thread(false);
    cabin.cfg.goal_pin = "harbor".into();
    let seeded = cabin
        .threads
        .iter()
        .position(|t| !t.messages.is_empty())
        .expect("seeded message");
    cabin.threads[seeded].grok_session = Some("sess".into());
    assert!(cabin.threads.len() >= 2);
    assert!(cabin
        .threads
        .iter()
        .any(|t| t.grok_session.as_deref() == Some("sess")));
    assert_eq!(cabin.cfg.goal_pin, "harbor");
    assert!(!cabin.running);

    cabin.delete_all_history();

    assert_eq!(cabin.status, "Deleted all chats");
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].title, "Chat");
    assert!(cabin.threads[0].messages.is_empty());
    assert!(cabin.messages.is_empty());
    assert!(cabin.cfg.goal_pin.is_empty());
    assert!(cabin.grok_sessions.is_empty());
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #174.
#[test]
fn leaving_a_chat_clears_attach_and_asks() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("leave-chrome");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("isolated config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.running = false;
    cabin.attach_name = Some("shot.png".into());
    cabin.hands_attach = true;
    cabin.eyes_attach = true;
    cabin.elicit_draft = "name the shot".into();
    cabin.perm_ask = Some(grokhub_core::wire::PermissionAsk {
        rpc_id: serde_json::Value::Null,
        session_id: "sess".into(),
        title: "Run".into(),
        tool_call_id: "tool".into(),
        action: "shot.png".into(),
        reason: "attach".into(),
        reject_option: None,
    });
    cabin.elicit_ask = Some(grokhub_core::wire::ElicitAsk {
        rpc_id: serde_json::Value::Null,
        session_id: "sess".into(),
        tool_call_id: "tool".into(),
        server_name: "form".into(),
        message: "need a name".into(),
        mode: "form".into(),
        url: String::new(),
        elicitation_id: "elicit".into(),
        field_name: Some("name".into()),
        field_title: "Name".into(),
        secret: false,
    });

    cabin.drop_leaving_thread_chrome();

    assert!(cabin.attach_name.is_none());
    assert!(!cabin.hands_attach);
    assert!(!cabin.eyes_attach);
    assert!(cabin.elicit_draft.is_empty());
    assert!(cabin.perm_ask.is_none());
    assert!(cabin.elicit_ask.is_none());
    assert!(cabin.acp.is_none());
    assert!(!cabin.running);

    std::env::remove_var("GROKHUB_CONFIG");
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
                egui::Area::new(egui::Id::new(id))
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

#[test]
fn escape_closes_plus_folder_avatar_shortcuts_and_upload() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("esc-overlays");
    let esc = || egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let ctx = egui::Context::default();

    cabin.plus_menu = Some(grokhub_core::PlusTarget::Chat);
    cabin.plus_anchor = egui::Pos2::ZERO;
    cabin.plus_ignore_close = true;
    let _ = crate::theme::test_pass(&ctx, esc(), |ui| {
        cabin.ui_plus_overlays(ui.ctx());
    });
    assert!(cabin.plus_menu.is_none(), "Esc closes the plus menu");
    assert!(
        !cabin.plus_ignore_close,
        "the open-click guard stays, and Esc clears it"
    );

    cabin.file_pick = Some(grokhub_core::PlusTarget::Chat);
    cabin.pick_dir = root.display().to_string();
    let _ = crate::theme::test_pass(&ctx, esc(), |ui| {
        cabin.ui_plus_overlays(ui.ctx());
    });
    assert!(cabin.file_pick.is_none(), "Esc closes Upload");

    cabin.proj_add_for = Some("pid".into());
    cabin.proj_menu_pos = egui::Pos2::ZERO;
    cabin.proj_ignore_close = true;
    let _ = crate::theme::test_pass(&ctx, esc(), |ui| {
        cabin.ui_project_overlays(ui.ctx());
    });
    assert!(cabin.proj_add_for.is_none(), "Esc closes Add to folder");
    assert!(!cabin.proj_ignore_close);

    cabin.settings_menu_open = true;
    cabin.settings_menu_ignore = true;
    let _ = crate::theme::test_pass(&ctx, esc(), |ui| {
        cabin.ui_settings_menu(ui.ctx());
    });
    assert!(!cabin.settings_menu_open, "Esc closes the avatar menu");
    assert!(!cabin.settings_menu_ignore);

    cabin.shortcuts_open = true;
    let _ = crate::theme::test_pass(&ctx, esc(), |ui| {
        cabin.paint_shortcuts(ui.ctx());
    });
    assert!(!cabin.shortcuts_open, "Esc closes Shortcuts");

    release_isolated(&root, cabin);
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

// Landed from PR #177.
#[test]
fn build_idea_files_one_todo() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("build-idea");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.board.clear();
    cabin.updates = vec![grokhub_core::UpdateCard {
        id: "idea-1".into(),
        kind: grokhub_core::UpdateKind::Idea,
        title: "Ship the harbor".into(),
        body: Some(String::new()),
        created_at: 0,
        status: grokhub_core::UpdateStatus::Unread,
        action: None,
        expires_at: None,
        held: false,
        citations: Vec::new(),
        reaction: None,
        discuss_thread: None,
        built: false,
        board_id: None,
        why: None,
        feed_pin: false,
        feed_kept: false,
        prompt: None,
        idea_kind: None,
        details: None,
        draft: None,
        modified: false,
        source_id: String::new(),
        skill: None,
        runs: 1,
        dismissed_at: 0,
        done_for_you: None,
        pulse: Default::default(),
    }];

    cabin.build_idea("nope");
    assert!(cabin.board.is_empty());
    assert!(!cabin.running);

    cabin.build_idea("idea-1");
    assert_eq!(cabin.board.len(), 1);
    assert_eq!(
        cabin.board[0].title,
        grokhub_core::idea_todo_title("Ship the harbor", "")
    );
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Todo);
    assert!(cabin.updates[0].built);
    assert_eq!(
        cabin.updates[0].board_id.as_deref(),
        Some(cabin.board[0].id.as_str())
    );
    assert!(!cabin.running);

    cabin.build_idea("idea-1");
    assert_eq!(cabin.board.len(), 1);
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    assert!(!cabin.running);
}

// Landed from PR #178.
#[test]
fn plan_and_empty_video_stay_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("plan-video");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.new_thread(false);
    {
        let thread = cabin
            .threads
            .get_mut(cabin.thread_idx)
            .expect("seeded thread");
        thread.grok_session = Some("sess".into());
    }

    cabin.run_slash(super::Slash::Plan);
    assert_eq!(cabin.status, "Plan mode — Grok Build will plan first");
    assert!(matches!(cabin.session_mode, super::SessionMode::Plan));
    assert_eq!(cabin.cfg.session_mode, "plan");
    assert!(cabin.acp.is_none());
    let session = cabin
        .threads
        .get(cabin.thread_idx)
        .and_then(|t| t.grok_session.as_deref())
        .unwrap_or("");
    assert!(session.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(super::Slash::ImagineVideo(String::new()));
    assert!(matches!(cabin.nav, super::Nav::Imagine));
    assert!(matches!(cabin.imagine_kind, super::ImagineKind::Video));
    assert!(cabin.imagine_want_focus);
    assert!(!cabin.running);
}

// Landed from PR #179.
#[test]
fn react_and_archive_stay_on_the_card() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("react-archive");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.updates = vec![
        feed_card("idea-1", grokhub_core::UpdateKind::Idea, false),
        feed_card("digest-1", grokhub_core::UpdateKind::Digest, true),
        feed_card("event-1", grokhub_core::UpdateKind::AutomationDone, false),
    ];

    cabin.close_home_card("digest-1");
    let digest = cabin
        .updates
        .iter()
        .find(|card| card.id == "digest-1")
        .expect("digest stays");
    assert_ne!(digest.status, grokhub_core::UpdateStatus::Dismissed);
    assert!(digest.pulse.off_home);
    assert!(digest.held);
    assert!(!cabin.running);

    cabin.close_home_card("idea-1");
    let idea = cabin
        .updates
        .iter()
        .find(|card| card.id == "idea-1")
        .expect("idea stays");
    assert_ne!(idea.status, grokhub_core::UpdateStatus::Dismissed);
    assert!(!idea.pulse.off_home);
}

fn feed_card(id: &str, kind: grokhub_core::UpdateKind, held: bool) -> grokhub_core::UpdateCard {
    grokhub_core::UpdateCard {
        id: id.to_string(),
        kind,
        title: id.to_string(),
        body: None,
        created_at: 1,
        status: grokhub_core::UpdateStatus::Unread,
        action: None,
        expires_at: None,
        held,
        citations: Vec::new(),
        reaction: None,
        discuss_thread: None,
        built: false,
        board_id: None,
        why: None,
        feed_pin: false,
        feed_kept: false,
        prompt: None,
        idea_kind: None,
        details: None,
        draft: None,
        modified: false,
        source_id: String::new(),
        skill: None,
        runs: 1,
        dismissed_at: 0,
        done_for_you: None,
        pulse: Default::default(),
    }
}

// Landed from PR #180.
#[test]
fn feed_actions_open_pages_without_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("feed-actions");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.new_thread(false);
    cabin.live_mut().push(("user".into(), "keep".into()));
    let older = cabin.threads[cabin.thread_idx].id.clone();
    cabin.new_thread(false);
    let newer = cabin.threads[cabin.thread_idx].id.clone();
    assert_ne!(older, newer);
    assert!(!cabin.running);

    cabin.follow_update_action(Some(UpdateAction::OpenWorkboard));
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    assert!(!cabin.running);
    let nav_before_unknown = cabin.nav;

    cabin.follow_update_action(Some(UpdateAction::OpenSession {
        thread_id: "missing-session".into(),
    }));
    assert!(cabin.nav == nav_before_unknown);
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    assert!(!cabin.running);

    cabin.follow_update_action(Some(UpdateAction::OpenSession {
        thread_id: older.clone(),
    }));
    assert_eq!(cabin.threads[cabin.thread_idx].id, older);
    assert!(matches!(cabin.nav, super::Nav::Chat));
    assert!(!cabin.running);

    cabin.follow_update_action(Some(UpdateAction::OpenWorkboard));
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    assert!(!cabin.running);

    cabin.follow_update_action(Some(UpdateAction::OpenAutomations));
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert!(!cabin.running);

    cabin.follow_update_action(Some(UpdateAction::DeepLink {
        href: "https://example.test/harbor".into(),
    }));
    assert_eq!(cabin.status, "https://example.test/harbor");
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert!(!cabin.running);

    let status_before_blank = cabin.status.clone();
    cabin.follow_update_action(Some(UpdateAction::DeepLink { href: "   ".into() }));
    assert_eq!(cabin.status, status_before_blank);
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert!(!cabin.running);

    let nav_before_none = cabin.nav;
    let status_before_none = cabin.status.clone();
    cabin.follow_update_action(None);
    assert!(cabin.nav == nav_before_none);
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert_eq!(cabin.status, status_before_none);
    assert!(!cabin.running);
}

// Landed from PR #181.
fn offer_card(id: &str, title: &str, status: UpdateStatus) -> UpdateCard {
    UpdateCard {
        id: id.to_string(),
        kind: UpdateKind::AutomateOffer,
        title: title.to_string(),
        body: None,
        created_at: 1,
        status,
        action: None,
        expires_at: None,
        held: false,
        citations: Vec::new(),
        reaction: None,
        discuss_thread: None,
        built: false,
        board_id: None,
        why: None,
        feed_pin: false,
        feed_kept: false,
        prompt: None,
        idea_kind: None,
        details: None,
        draft: None,
        modified: false,
        source_id: String::new(),
        skill: None,
        runs: 1,
        dismissed_at: 0,
        done_for_you: None,
        pulse: Default::default(),
    }
}

fn card_status(cabin: &Cabin, id: &str) -> UpdateStatus {
    cabin
        .updates
        .iter()
        .find(|card| card.id == id)
        .map(|card| card.status)
        .unwrap_or_else(|| panic!("missing card {id}"))
}

#[test]
fn automate_offer_files_a_daily_job() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("automate-offer");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.automations.clear();
    cabin.updates = vec![
        offer_card("plain", "what is rust", UpdateStatus::Unread),
        offer_card(
            "daily",
            "every day at 9, summarize the board",
            UpdateStatus::Unread,
        ),
        offer_card(
            "gone",
            "every day at 9, summarize the board",
            UpdateStatus::Dismissed,
        ),
    ];

    cabin.accept_automate_offer("missing");
    assert!(cabin.automations.is_empty());
    assert!(!cabin.running);

    cabin.accept_automate_offer("gone");
    assert!(cabin.automations.is_empty());
    assert!(!cabin.running);
    assert_eq!(card_status(&cabin, "gone"), UpdateStatus::Dismissed);

    cabin.accept_automate_offer("plain");
    assert_eq!(card_status(&cabin, "plain"), UpdateStatus::Opened);
    assert!(cabin.automations.is_empty());
    assert!(!cabin.running);

    cabin.accept_automate_offer("daily");
    assert_eq!(card_status(&cabin, "daily"), UpdateStatus::Opened);
    assert_eq!(cabin.automations.len(), 1);
    assert!(!cabin.running);

    cabin.accept_automate_offer("daily");
    assert_eq!(cabin.automations.len(), 2);
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn feed_offer_click_opens_the_schedule_box_without_saving() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("feed-offer-open");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.automations.clear();
    cabin.updates = vec![offer_card(
        "daily",
        "every day at 9, summarize the board",
        UpdateStatus::Unread,
    )];
    cabin.open_feed_card("daily");
    assert!(cabin.auto_compose);
    assert_eq!(cabin.night_nl, "every day at 9, summarize the board");
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert!(cabin.automations.is_empty());
    assert_eq!(card_status(&cabin, "daily"), UpdateStatus::Opened);
    cabin.open_feed_card("daily");
    assert!(cabin.automations.is_empty());

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #183.
#[test]
fn view_plan_opens_a_stored_plan() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("view-plan");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);

    cabin.threads[cabin.thread_idx].plan_body.clear();
    cabin.run_slash(super::Slash::ViewPlan);
    assert_eq!(cabin.status, "No plan yet — use Plan mode");
    assert!(!cabin.plan_open);
    assert!(!cabin.running);

    cabin.threads[cabin.thread_idx].plan_body = " \n\t ".into();
    cabin.run_slash(super::Slash::ViewPlan);
    assert_eq!(cabin.status, "No plan yet — use Plan mode");
    assert!(!cabin.plan_open);
    assert!(!cabin.running);

    cabin.threads[cabin.thread_idx].plan_body = "Ship the harbor".into();
    cabin.run_slash(super::Slash::ViewPlan);
    assert!(cabin.plan_open);
    assert_eq!(cabin.status, "View plan");
    assert!(!cabin.running);
}

// Landed from PR #184.
#[test]
fn rewind_files_asks_for_a_bind() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("rewind-bind");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if !cabin.cfg.project_dir.is_empty() {
        cabin.cfg.project_dir = String::new();
    }
    cabin.run_slash(Slash::RewindFiles);
    assert_eq!(cabin.status, "Bind a project first — /project bind");
    assert!(
        cabin.rewind_rows.is_empty(),
        "empty project dir must not snapshot"
    );
    assert!(!cabin.running, "empty project dir must not queue a shell");
    assert!(cabin.cfg.project_dir.is_empty());
}

// Landed from PR #185.
#[test]
fn goal_pin_sets_and_clears() {
    let _hold = config::hold_test_config();
    let root = config::test_config_root("goal-pin");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let _pin = config::TestConfigDir::set(root.clone());

    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.goal_pin.clear();
    assert!(cabin.cfg.goal_pin.is_empty());
    assert!(!cabin.running);

    let pin_file = root.join("app.json");
    let wait_pin = move |want: &str| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut last = String::new();
        while std::time::Instant::now() < deadline {
            last = std::fs::read_to_string(&pin_file)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|v| {
                    v.get("goalPin")
                        .and_then(|p| p.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();
            if last == want {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("persisted goal_pin {last:?}, want {want:?}");
    };

    for raw in ["", "   ", "\t", "\n  ", "status", "STATUS", "Status"] {
        cabin.run_slash(Slash::Goal(raw.to_string()));
        assert_eq!(cabin.status, "No goal pin", "input {raw:?}");
        assert!(cabin.cfg.goal_pin.is_empty(), "input {raw:?}");
        assert!(!cabin.running);
    }

    cabin.run_slash(Slash::Goal("Ship the harbor".to_string()));
    assert_eq!(cabin.cfg.goal_pin, "Ship the harbor");
    assert_eq!(cabin.status, "Goal: Ship the harbor");
    assert!(!cabin.running);
    wait_pin("Ship the harbor");

    for raw in ["status", "STATUS", "Status"] {
        cabin.run_slash(Slash::Goal(raw.to_string()));
        assert_eq!(cabin.status, "Goal: Ship the harbor", "input {raw:?}");
        assert_eq!(cabin.cfg.goal_pin, "Ship the harbor");
        assert!(!cabin.running);
    }

    for raw in ["clear", "CLEAR", "Clear"] {
        if cabin.cfg.goal_pin.is_empty() {
            cabin.run_slash(Slash::Goal("Ship the harbor".to_string()));
            assert_eq!(cabin.cfg.goal_pin, "Ship the harbor");
            wait_pin("Ship the harbor");
        }
        cabin.run_slash(Slash::Goal(raw.to_string()));
        assert!(cabin.cfg.goal_pin.is_empty(), "input {raw:?}");
        assert_eq!(cabin.status, "Goal cleared", "input {raw:?}");
        assert!(!cabin.running);
        wait_pin("");
    }

    cabin.run_slash(Slash::Goal("status".to_string()));
    assert_eq!(cabin.status, "No goal pin");
    assert!(cabin.cfg.goal_pin.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #186.
#[test]
fn btw_sets_a_side_ask() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("btw-side-ask");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.run_slash(Slash::Btw);

    assert!(matches!(cabin.session_mode, SessionMode::Ask));
    assert_eq!(cabin.cfg.session_mode, "ask");
    assert!(!cabin.running);
    assert_eq!(cabin.status, "btw — look-safe side ask");
    assert_eq!(cabin_default_session_id(&cabin.cfg.session_mode), "ask");
    let persisted = cabin
        .cfg_slot
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .cfg
        .session_mode
        .clone();
    assert_eq!(persisted, "ask");
}

// Landed from PR #188.
#[test]
fn missing_skill_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("missing-skill");
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash(grokhub_core::Slash::Skill("harbor".into()));
    assert_eq!(cabin.status, "No skill harbor");
    assert!(matches!(cabin.nav, super::Nav::Chat));
    assert!(!cabin.running);
    assert!(cabin.messages.is_empty());
}

// Landed from PR #189.
#[test]
fn empty_undo_and_retry_stay_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("empty-undo-retry");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(super::Slash::Undo);
    assert_eq!(cabin.status, "Nothing to undo");
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(super::Slash::Retry);
    assert_eq!(cabin.status, "Nothing to retry");
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #191.
#[test]
fn forget_clears_memory_off_a_run() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("forget-off-run");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body = "wifi stays until forget".into();
    assert!(!cabin.scratch());
    assert!(!cabin.running);

    cabin.run_slash(Slash::Forget(None));

    assert_eq!(cabin.status, "Forgot MEMORY.md");
    assert!(cabin.mem_name == "MEMORY.md" && cabin.mem_body.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #193.
#[test]
fn forget_topic_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("forget-topic");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.new_thread(false);
    cabin.mem_name = "MEMORY.md".into();
    let note = "harbor light at dusk".to_string();
    cabin.mem_body = note.clone();

    cabin.run_slash(grokhub_core::Slash::Forget(Some("harbor".into())));

    assert_eq!(cabin.mem_body, grokhub_core::forget_topic(&note, "harbor"));
    assert_eq!(cabin.status, "Forgot harbor");
    assert!(!cabin.running);
    assert!(cabin.nav == super::Nav::Chat);
}

// Landed from PR #197.
#[allow(clippy::too_many_lines)]
fn quiet_cabin() -> Cabin {
    let cfg = AppConfig::default();
    let persist_io = std::sync::Arc::new(std::sync::Mutex::new(()));
    let persist_svc = PersistService::start(
        persist_io.clone(),
        std::sync::Arc::new(std::sync::Mutex::new(PersistMark::default())),
    );
    Cabin {
        nav: super::Nav::Chat,
        cfg: cfg.clone(),
        composer: String::new(),
        messages: std::sync::Arc::new(Vec::new()),
        messages_rev: 0,
        messages_body_rev: 0,
        status: String::new(),
        running: false,
        turn_retried: false,
        host_halt: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx: None,
        chat_job_thread: None,
        inflight_open: false,
        hub: std::sync::Arc::new(std::sync::Mutex::new(grokhub_core::HubState::empty())),
        hub_on: false,
        hub_port: grokhub_core::DEFAULT_PORT,
        task_prompt: String::new(),
        mem_name: String::new(),
        mem_body: String::new(),
        mem_cache_at: [0, 0, 0],
        mem_cache_body: [String::new(), String::new(), String::new()],
        last_persist: std::time::Instant::now(),
        persist_idle_key: String::new(),
        persist_rx: None,
        persist_io,
        persist_svc,
        setup: super::setup_wizard::SetupWizard::default(),
        persist_gen: 0,
        persist_err: std::sync::Arc::new(std::sync::Mutex::new(None)),
        cfg_slot: std::sync::Arc::new(std::sync::Mutex::new(super::CfgSlot { gen: 0, cfg })),
        board: Vec::new(),
        board_title: String::new(),
        board_notes: String::new(),
        journal: Vec::new(),
        journal_view: super::journal_ui::JournalView::default(),
        imagine_prompt: String::new(),
        imagine_last: String::new(),
        skill_name: String::new(),
        skill_body: String::new(),
        skill_list: Vec::new(),
        eyes_text: String::new(),
        last_host: Vec::new(),
        last_frame_url: None,
        hands_attach: false,
        eyes_attach: false,
        speak_next: false,
        verify_ok_turn: false,
        verify_chip: String::new(),
        reflect_diff: String::new(),
        last_activity: std::time::Instant::now(),
        reflected_idle: false,
        last_recipe: None,
        update_pct: None,
        update_can_restart: false,
        secrets: crate::secrets::Secrets::default(),
        threads: Vec::new(),
        thread_idx: 0,
        oauth_pending: None,
        oauth_next_poll: std::time::Instant::now(),
        oauth_start_rx: None,
        oauth_poll_rx: None,
        host_hour_count: 0,
        host_hour_at: std::time::Instant::now(),
        host_reserved: 0,
        plan_pending: None,
        tray: None,
        tray_rx: None,
        window_visible: true,
        resume_fresh: false,
        saw_minimized: false,
        ideas_filled: false,
        ideas_rx: None,
        tray_saw_unfocused: false,
        tray_hid_at: std::time::Instant::now(),
        want_quit: false,
        told_tray: false,
        pending_hub_task: None,
        automations: Vec::new(),
        grok_loops: Vec::new(),
        updates: Vec::new(),
        daily_drop_file: None,
        card_prefs: grokhub_core::CardPrefs::default(),
        grok_loop_rx: None,
        night_nl: String::new(),
        watch_once: false,
        watched_steps: Vec::new(),
        chat_tail_frames: 0,
        cap_auto_buf: String::new(),
        cap_host_buf: String::new(),
        quiet_start_buf: String::new(),
        quiet_end_buf: String::new(),
        history_q: String::new(),
        history_q_seen: String::new(),
        history_q_at: None,
        history_hits: Vec::new(),
        archive_undo: None,
        chat_archive_day: None,
        last_receipt_ok: None,
        beta_auto_off_checked_at: None,
        last_receipts: Vec::new(),
        try_again: false,
        last_rewind_id: None,
        rewind_rows: Vec::new(),
        host_live: String::new(),
        daily_auto_used: 0,
        daily_auto_day: String::new(),
        slash_pick: 0,
        slash_filter_n: 0,
        slash_filter_first: String::new(),
        last_window_title: String::new(),
        voice_orb: String::new(),
        governor: grokhub_core::Governor::cabin(),
        last_heartbeat: std::time::Instant::now(),
        pace: Default::default(),
        pace_traced: Vec::new(),
        night_check_rx: None,
        learning: grokhub_core::LearningState::default(),
        suggestions: grokhub_core::SuggestionStore::default(),
        review_rx: None,
        review_busy: false,
        digest_rx: None,
        digest_busy: false,
        digest_pending: None,
        digest_wants_lookup: false,
        digest_steer: String::new(),
        digest_interests: String::new(),
        situation_ping: None,
        window_focused: true,
        offer_repeated: false,
        usage: grokhub_core::UsageDay::default(),
        palette_open: false,
        palette_q: String::new(),
        palette_pick: 0,
        palette_focus: false,
        palette_files: Vec::new(),
        palette_files_q: String::new(),
        palette_files_root: String::new(),
        palette_file_rx: None,
        palette_steps: Vec::new(),
        palette_steps_q: String::new(),
        palette_step_rx: None,
        shortcuts_open: false,
        active_skill_follow: None,
        card_notes_follow: None,
        board_notes_edit: None,
        last_anticipate_ms: 0,
        proactive: Default::default(),
        last_proactive_ms: 0,
        goal_step: 0,
        followup_step: 0,
        stream_buf: String::new(),
        thought_buf: String::new(),
        chat_views: Vec::new(),
        chat_view_keys: Vec::new(),
        chat_view_tid: String::new(),
        chat_view_n: 0,
        chat_view_last: 0,
        chat_view_rev: 0,
        chat_view_body: (0, 0),
        presence_ring: Vec::new(),
        voice_state: grokhub_core::VoiceState::Idle,
        voice_ready_at: None,
        voice_hold_rx: None,
        cmd_line: String::new(),
        cmd_hist: Vec::new(),
        agents: Vec::new(),
        last_live: std::time::Instant::now(),
        live_cap_rx: None,
        eyes_cap_rx: None,
        kick_cap_rx: None,
        pending_kick: None,
        kick_frame: None,
        kick_skip: false,
        recipe_cap_rx: None,
        recipe_desk_rx: None,
        host_diff_rx: None,
        host_diff_kick: false,
        verify_rx: None,
        hotkeys: None,
        hotkey_hey: 0,
        hotkey_halt: 0,
        sidebar_q: String::new(),
        rename_idx: None,
        rename_buf: String::new(),
        rename_focus: false,
        rename_lock: None,
        chip_memory: grokhub_core::ChipMemory::default(),
        chip_dismissed: Vec::new(),
        idea_board: Default::default(),
        pulse_view: Default::default(),
        agent_cursor: None,
        board_view: Default::default(),
        llm_chips: Vec::new(),
        visible_chips: Vec::new(),
        chip_rx: None,
        chip_busy: false,
        chip_fp: String::new(),
        chip_paint_key: String::new(),
        chip_llm_at: 0,
        greeting: String::new(),
        greeting_fp: String::new(),
        greeting_user_at: 0,
        greeting_memory_at: 0,
        greeting_user_md: String::new(),
        greeting_memory_md: String::new(),
        greeting_files_rx: None,
        greeting_flush_name: String::new(),
        greeting_flush_len: 0,
        greeting_llm_fp: String::new(),
        greeting_rx: None,
        greeting_busy: false,
        greeting_llm_at: 0,
        continue_hint: String::new(),
        skill_q: String::new(),
        pending_connectors: Vec::new(),
        auto_compose: false,
        board_compose: false,
        board_edit: None,
        board_link: false,
        settings_menu_open: false,
        settings_menu_ignore: false,
        win_max: false,
        geom_dirty: false,
        geom_applied: false,
        geom_apply_frames: 0,
        imagine_want_focus: false,
        composer_want_focus: false,
        settings_sec: super::SettingsSec::Account,
        market: Default::default(),
        picker: Default::default(),
        playbooks: Default::default(),
        interests: Default::default(),
        sources: Default::default(),
        settings_back: super::Nav::Chat,
        imagine_aspect: 0,
        imagine_quality: false,
        imagine_kind: grokhub_core::ImagineKind::Image,
        imagine_style: 0,
        imagine_video_res: 0,
        imagine_video_dur: 0,
        imagine_video_audio: false,
        imagine_aspect_open: false,
        imagine_style_open: false,
        imagine_menu_ignore: false,
        imagine_style_anchor: egui::Rect::NOTHING,
        imagine_aspect_anchor: egui::Rect::NOTHING,
        imagine_expand: false,
        imagine_job_prompt: String::new(),
        imagine_error: String::new(),
        imagine_pending: false,
        imagine_native: super::imagine::ImagineNative::default(),
        imagine_save_rx: None,
        goal_rx: None,
        goal_busy: false,
        goal_stale: false,
        wall: grokhub_core::ImagineWall::default(),
        wall_rx: None,
        logic_input_pass: None,
        wall_busy: false,
        attach_url: None,
        attach_name: None,
        attach_path: None,
        attach_kind: None,
        attach_tray: super::attach_tray::AttachTray::default(),
        attach_ingest: None,
        attach_thumb_cache: Vec::new(),
        attach_pick_rx: None,
        imagine_ref: None,
        plus_menu: None,
        plus_anchor: egui::Pos2::ZERO,
        plus_ignore_close: false,
        file_pick: None,
        pick_rx: None,
        drop_extra: 0,
        pick_list_rx: None,
        pick_dir: String::new(),
        pick_cache: None,
        projects: Vec::new(),
        project_sel: None,
        proj_menu_pos: egui::Pos2::ZERO,
        proj_add_for: None,
        proj_rename: None,
        proj_rename_buf: String::new(),
        proj_rename_focus: false,
        proj_rename_lock: None,
        proj_staged: None,
        proj_ignore_close: false,
        projects_dirty: false,
        oauth_photo: None,
        oauth_photo_key: String::new(),
        oauth_photo_rx: None,
        oauth_photo_busy: false,
        oauth_profile_tried: false,
        profile_photo: None,
        profile_photo_key: String::new(),
        profile_photo_rx: None,
        profile_photo_busy: false,
        profile_pick_rx: None,
        profile_pick_token: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        profile_file_io: std::sync::Arc::new(std::sync::Mutex::new(())),
        cabin_latest: None,
        last_update_probe: None,
        update_probe_rx: None,
        cabin_overlay_done: false,
        queued_overlay: None,
        update_cabin_note: None,
        acp: None,
        grok_p_rx: None,
        grok_usage: grokhub_core::wire::GrokUsage::default(),
        tokens_seen: (0, 0, 0),
        grok_commands: Vec::new(),
        grok_tasks: Vec::new(),
        followup_queue: Vec::new(),
        side_ask_queue: Vec::new(),
        bg: super::background::BgWork::default(),
        side_ask_kick: false,
        plan_open: false,
        tool_cards: Vec::new(),
        panels: super::panels::PanelStore::default(),
        live_blocks: Vec::new(),
        live_keys: Vec::new(),
        turn_log: Vec::new(),
        thought_seam: false,
        say_seam: false,
        desk_frame: None,
        perm_ask: None,
        perm_queue: std::collections::VecDeque::new(),
        perm_always_confirm: None,
        confirm: None,
        jump_last_you: false,
        jump_turn: None,
        find: super::chat_ui::ChatFind::default(),
        elicit_ask: None,
        elicit_draft: String::new(),
        secret_hold: Vec::new(),
        session_mode: grokhub_core::wire::SessionMode::Chat,
        permission_mode: grokhub_core::wire::PermissionMode::Ask,
        harness: Default::default(),
        auto_act: Default::default(),
        scheduled_perm: false,
        grok_sessions: Vec::new(),
        history_rx: None,
        mem_restore_rx: None,
        mem_file_rx: None,
        recall_rx: None,
        sync_rx: None,
        inhabit_rx: None,
        reflect_rx: None,
        amr_imported: false,
        dream_day: None,
        amr_prune: amr_memory::AmrPrune::default(),
        session_show_rx: None,
        import_rx: None,
        native_skills: Vec::new(),
        native_hooks: Vec::new(),
        native_listing_cwd: String::new(),
        native_hooks_trusted: false,
        connector_note: String::new(),
        screen_rec: None,
        screen_diag_rx: None,
        audio_check: None,
        scroll_to_hooks: false,
        composer_geom: None,
        composer_ghost: ghost::ComposerGhost::default(),
        dock_state: super::dock_ui::default_dock_state(),
        toasts: super::status_toasts::cabin_toasts(),
        status_toasted: Default::default(),
    }
}

// Landed from PR #203.
#[test]
fn empty_skill_tile_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("empty-skill-tile");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let tile = grokhub_core::LearnedSuggestion {
        kind: grokhub_core::SuggestionKind::Skill,
        title: String::new(),
        body: String::new(),
        name: None,
        seed: None,
        trigger: None,
        instructions: None,
        provider: None,
        tool: None,
    };
    assert!(matches!(tile.kind, grokhub_core::SuggestionKind::Skill));

    let mut cabin = super::Cabin::quiet_for_test();
    assert!(cabin.suggestions.skills.is_empty());
    assert!(!cabin.running);
    assert!(matches!(cabin.nav, super::Nav::Chat));

    cabin.add_suggested_skill(&tile);

    assert_eq!(cabin.status, "Need a cabin-real skill name and steps");
    assert!(cabin.suggestions.skills.is_empty());
    assert!(!cabin.running);
    assert!(matches!(cabin.nav, super::Nav::Chat));
}

// Landed from PR #205.
#[test]
fn full_loop_list_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("full-loop-list");
    let _ = std::fs::create_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.grok_loops = (0..50)
        .map(|_| grokhub_core::new_loop("30m".into(), "check".into(), 0))
        .collect();
    cabin.add_automation_seed("/loop 30m check deploy");
    assert_eq!(cabin.status, "Maximum 50 scheduled loops");
    assert_eq!(cabin.grok_loops.len(), 50);
    assert!(cabin.automations.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #207.
#[test]
fn full_automation_list_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("full-auto");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.automations = (0..50)
        .map(|i| grokhub_core::Automation {
            id: format!("job-{i}"),
            name: "Job".into(),
            schedule: "daily".into(),
            time: "09:00".into(),
            times: Vec::new(),
            instructions: "check".into(),
            heartbeat_every_min: 0,
            check_command: String::new(),
            enabled: true,
            last_run: None,
            next_run: None,
            run_count: 0,
            health: Default::default(),
        })
        .collect();
    cabin.add_automation_seed("every day at 9, summarize the board");
    assert_eq!(cabin.status, "Maximum 50 scheduled automations");
    assert_eq!(cabin.automations.len(), 50);
    assert!(cabin.grok_loops.is_empty());
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #210.
#[test]
fn empty_update_plan_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("empty-update-plan");
    let prev = std::env::var("GROKHUB_CONFIG").ok();
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut app = Cabin::quiet_for_test();
    app.start_overlay_update(Vec::new());
    assert_eq!(app.status, "Update plan empty");
    assert!(matches!(app.nav, Nav::Settings));
    assert!(matches!(app.settings_sec, SettingsSec::Update));
    assert!(!app.running);
    assert!(app.queued_overlay.is_none());
    match prev {
        Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
        None => std::env::remove_var("GROKHUB_CONFIG"),
    }
}

// Landed from PR #213.
#[test]
fn busy_update_stays_queued() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("busy_update_stays_queued");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.running = true;
    cabin.start_overlay_update(vec!["echo hi".into()]);
    assert_eq!(
        cabin.status,
        "Update queued — it starts when this job finishes."
    );
    assert_eq!(cabin.queued_overlay, Some(vec!["echo hi".into()]));
    assert!(cabin.running);
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(matches!(cabin.settings_sec, SettingsSec::Update));
}

// Landed from PR #214.
#[test]
fn rename_chat_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("rename-chat");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    let prev = std::env::var("GROKHUB_CONFIG").ok();
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    let thread_idx = cabin.thread_idx;
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.running);

    cabin.rename_thread(thread_idx, "");
    assert_eq!(cabin.status, "Kept Chat");
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.threads[thread_idx].title_locked);

    cabin.rename_thread(thread_idx, "Harbor");
    assert_eq!(cabin.status, "Renamed Harbor");
    assert_eq!(cabin.threads[thread_idx].title, "Harbor");
    assert!(cabin.threads[thread_idx].title_locked);
    assert!(!cabin.running);

    match prev {
        Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
        None => std::env::remove_var("GROKHUB_CONFIG"),
    }
}

// Landed from PR #215.
#[test]
fn pin_chat_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("pin-chat");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert!(!cabin.running);

    cabin.pin_thread(cabin.thread_idx);
    assert_eq!(cabin.status, "Pinned Chat");
    assert!(cabin.threads[cabin.thread_idx].pinned);
    assert!(!cabin.running);

    cabin.pin_thread(cabin.thread_idx);
    assert_eq!(cabin.status, "Unpinned Chat");
    assert!(!cabin.threads[cabin.thread_idx].pinned);
    assert!(!cabin.running);
}

// Landed from PR #216.
#[test]
fn fresh_home_keeps_the_old_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("fresh-home");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "hello".into())]);

    cabin.open_fresh_home();

    assert_eq!(cabin.status, "New chat");
    assert!(cabin.messages.is_empty());
    let kept = cabin.threads.iter().enumerate().any(|(i, thread)| {
        i != cabin.thread_idx
            && thread
                .messages
                .iter()
                .any(|(role, line)| role == "user" && line == "hello")
    });
    assert!(kept);
    assert!(!cabin.running);
    assert!(matches!(cabin.nav, Nav::Chat));
}

// Landed from PR #217.
#[test]
fn delete_one_chat_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("delete-one-chat");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.threads[cabin.thread_idx].title = "Harbor".into();
    cabin.messages = Arc::new(vec![("user".into(), "hello".into())]);
    cabin.new_thread(false);

    let harbor = cabin
        .threads
        .iter()
        .position(|t| t.title == "Harbor")
        .expect("Harbor");
    cabin.delete_thread_at(harbor);

    assert_eq!(cabin.status, "Deleted Harbor");
    assert!(cabin.threads.iter().all(|t| t.title != "Harbor"));
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert!(!cabin.running);
    assert!(cabin.messages.is_empty());

    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #218.
#[test]
fn scratch_chat_stays_off_a_run() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("scratch-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.new_thread(true);
    assert_eq!(cabin.status, "Scratch — no memory writes");
    let thread = cabin.threads.get(cabin.thread_idx).expect("current thread");
    assert_eq!(thread.title, "Scratch");
    assert!(thread.scratch);
    assert!(!cabin.running);
}

// Landed from PR #219.
#[test]
fn delete_last_chat_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("delete-last");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.delete_thread_at(0);
    assert_eq!(cabin.status, "Chat deleted");
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].title, "Chat");
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #220.
#[test]
fn land_on_real_chat_leaves_scratch() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("land-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.new_thread(true);
    let current = &cabin.threads[cabin.thread_idx];
    assert_eq!(current.title, "Scratch");
    assert!(current.scratch);
    assert!(cabin
        .threads
        .iter()
        .enumerate()
        .any(|(i, t)| { i != cabin.thread_idx && t.title == "Chat" && !t.scratch }));
    assert_eq!(cabin.status, "Scratch — no memory writes");
    cabin.land_on_real_chat();
    let current = &cabin.threads[cabin.thread_idx];
    assert_eq!(current.title, "Chat");
    assert!(!current.scratch);
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(cabin
        .threads
        .iter()
        .enumerate()
        .any(|(i, t)| { i != cabin.thread_idx && t.title == "Scratch" && t.scratch }));
    assert_eq!(cabin.status, "Scratch — no memory writes");
    assert!(!cabin.running);
}

// Landed from PR #221.
#[test]
fn plan_pill_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("plan-pill");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert!(!cabin.threads[cabin.thread_idx].title_locked);
    cabin.select_plan_without_rename();
    assert_eq!(cabin.status, "Session plan");
    assert!(matches!(cabin.session_mode, SessionMode::Plan));
    assert_eq!(cabin.cfg.session_mode, "plan");
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert!(!cabin.threads[cabin.thread_idx].title_locked);
    assert!(cabin.acp.is_none());
    assert!(!cabin.running);
}

// Landed from PR #222.
#[test]
fn recent_chat_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("recent-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    let idx = cabin.thread_idx;
    assert_eq!(cabin.threads[idx].title, "Chat");
    assert_eq!(cabin.status, "New chat");
    cabin.open_recent_chat();
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert_eq!(cabin.thread_idx, idx);
    assert!(cabin.composer_want_focus);
    assert_eq!(cabin.status, "New chat");
    assert!(!cabin.running);
    assert_eq!(cabin.threads.len(), 1);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #223.
#[test]
fn begin_rename_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("begin-rename");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    let thread_idx = cabin.thread_idx;
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.threads[thread_idx].title_locked);
    assert_eq!(cabin.status, "New chat");
    assert!(!cabin.running);
    cabin.begin_chat_rename(thread_idx);
    assert_eq!(cabin.rename_idx, Some(thread_idx));
    assert!(cabin.rename_focus);
    assert_eq!(cabin.rename_buf, "Chat");
    assert_eq!(cabin.rename_lock, Some("Chat".into()));
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.threads[thread_idx].title_locked);
    assert_eq!(cabin.status, "New chat");
    assert!(!cabin.running);
}

// Landed from PR #226.
#[test]
fn switch_chat_keeps_the_line() {
    let _cfg = crate::config::hold_test_config();
    let root = crate::config::test_config_root("switch-chat-keeps");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut app = Cabin::quiet_for_test();
    let mut held = crate::threads::ChatThread::new("Chat", false);
    held.messages = std::sync::Arc::new(vec![("user".into(), "older line".into())]);
    app.threads.push(held);
    app.thread_idx = 0;
    app.messages = app.threads[0].messages.clone();
    app.new_thread(false);
    let left = app.thread_idx;
    assert!(left > 0);
    app.live_mut().push(("user".into(), "harbor line".into()));
    app.apply_switch_thread(0);
    assert_eq!(app.thread_idx, 0);
    assert!(app.messages.iter().all(|(_, c)| c != "harbor line"));
    assert!(app.threads[left]
        .messages
        .iter()
        .any(|(_, c)| c == "harbor line"));
    assert!(app.composer_want_focus);
    assert!(app.rename_idx.is_none());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    assert_eq!(app.status, "New chat");

    std::env::remove_var("GROKHUB_CONFIG");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn edit_puts_your_message_back_without_dropping_a_draft() {
    assert_eq!(
        super::chat_ui::edit_into_composer("", "fix the tests"),
        "fix the tests"
    );
    assert_eq!(
        super::chat_ui::edit_into_composer("  \n", "fix the tests"),
        "fix the tests"
    );
    assert_eq!(
        super::chat_ui::edit_into_composer("half typed\n", "fix the tests"),
        "half typed\n\nfix the tests"
    );
    assert_eq!(
        super::chat_ui::msg_act_labels(true),
        &["Copy", "Edit", "Reply"]
    );
    assert_eq!(super::chat_ui::msg_act_labels(false), &["Copy", "Reply"]);
    let src = cabin_src();
    let chat = fn_src(&src, "ui_chat");
    assert!(chat.contains("ChatBlockAct::Edit(body)"), "{chat}");
    assert!(chat.contains("edit_into_composer"), "{chat}");
}

#[test]
fn user_act_row_with_edit_stays_inside_a_narrow_pane() {
    for width in [300.0_f32, 360.0, 800.0] {
        let ctx = egui::Context::default();
        let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
            let ctx = ui.ctx().clone();
            crate::theme::apply(&ctx, true);
            egui::CentralPanel::default().show(ui, |ui| {
                ui.allocate_ui(egui::vec2(width, 200.0), |ui| {
                    ui.set_max_width(width);
                    let row = ui.max_rect();
                    let bubble = super::chat_ui::paint_speech_bubble(ui, "hey", true, false);
                    let acts = super::chat_ui::paint_msg_acts(
                        ui,
                        true,
                        "hey",
                        row.width(),
                        bubble.rect.width(),
                    );
                    assert!(
                        acts.row.min.x >= row.min.x - 0.5,
                        "width {width}: {:?}",
                        acts.row
                    );
                    assert!(
                        acts.row.max.x <= row.max.x + 1.0,
                        "width {width}: {:?}",
                        acts.row
                    );
                    let want = super::chat_ui::msg_acts_row_width(ui, true);
                    assert!(
                        acts.row.width() + 1.0 >= want,
                        "width {width}: Edit row clipped {} < {want}",
                        acts.row.width()
                    );
                });
            });
        });
    }
}

fn find_views(bodies: &[(&str, &str)]) -> Vec<ChatView> {
    grokhub_core::visible_chat_refs(bodies.iter().copied())
}

#[test]
fn chat_find_starts_on_the_newest_hit_and_wraps() {
    let views = find_views(&[
        ("user", "build the cabin"),
        ("assistant", "Building it."),
        ("user", "now test"),
        ("assistant", "Tests pass. Build is green."),
    ]);
    let mut find = super::chat_ui::ChatFind::default();
    find.toggle();
    assert!(find.open && find.want_focus);
    find.query = "BUILD".into();
    find.refresh("t1", &views);
    let hits = find.hits().to_vec();
    assert_eq!(hits.len(), 3, "{hits:?}");
    assert_eq!(find.current_row(), hits.last().copied());
    assert!(find.jump, "a new query scrolls to its hit");
    assert_eq!(find.label(), "3 of 3");
    find.jump = false;
    find.step(true);
    assert_eq!(find.current_row(), Some(hits[0]));
    assert!(find.jump);
    find.step(false);
    assert_eq!(find.label(), "3 of 3");
    // Same query, same transcript: no recount, pick stays.
    find.step(false);
    find.refresh("t1", &views);
    assert_eq!(find.label(), "2 of 3");
    find.query = "zzz".into();
    find.refresh("t1", &views);
    assert_eq!(find.label(), "No matches");
    assert!(find.current_row().is_none());
    find.close();
    assert!(!find.open && !find.focused);
    assert!(find.hits().is_empty(), "a closed find marks nothing");
}

#[test]
fn chat_find_is_ctrl_f_on_chat_and_owns_enter_over_a_permission_card() {
    let src = cabin_src();
    assert!(src.contains("egui::Key::F"), "Ctrl+F must open find");
    assert!(src.contains("self.find.toggle()"));
    let chat = fn_src(&src, "ui_chat");
    assert!(chat.contains("paint_find_bar"), "{chat}");
    assert!(chat.contains("find_jump && find_row == Some(i)"), "{chat}");
    assert!(
        src.contains("self.palette_open || self.nav == Nav::Settings || self.find.focused"),
        "Enter in the find box must not allow a tool"
    );
    let confirm = include_str!("confirm.rs");
    assert!(
        confirm.contains("self.find.focused"),
        "Enter in the find box must not confirm a sheet"
    );
    assert!(grokhub_core::shortcut_help().contains("Ctrl+F"));
}

#[test]
fn chat_find_bar_paints_and_steps_with_the_arrows() {
    let mut app = Cabin::quiet_for_test();
    app.find.toggle();
    app.find.query = "x".into();
    let views = find_views(&[("user", "x one"), ("assistant", "x two")]);
    app.find.refresh("t", &views);
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        let ctx = ui.ctx().clone();
        crate::theme::apply(&ctx, true);
        app.paint_find_bar(
            &ctx,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
        );
    });
    assert!(app.find.open);
    assert_eq!(app.find.label(), "2 of 2");
}

/// The first card on the feed that `pick` matches, or a failure naming `site`.
fn posted_card(cabin: &Cabin, site: &str, pick: impl Fn(&UpdateCard) -> bool) -> UpdateCard {
    cabin
        .updates
        .iter()
        .find(|c| pick(c))
        .cloned()
        .unwrap_or_else(|| panic!("{site} posted no card: {:?}", cabin.updates))
}

/// Fns in this crate's live code that build or post a Home card.
fn app_feed_post_sites() -> std::collections::BTreeSet<String> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sites = std::collections::BTreeSet::new();
    let mut dirs = vec![src];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".rs") || name.ends_with("tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n");
            let live = text.split("\n#[cfg(test)]\nmod ").next().unwrap();
            let mut current = String::new();
            for line in live.lines() {
                let decl = line
                    .trim_start()
                    .trim_start_matches("pub(super) ")
                    .trim_start_matches("pub(crate) ")
                    .trim_start_matches("pub ");
                if let Some(rest) = decl.strip_prefix("fn ") {
                    current = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                }
                let posts = line.contains("post_feed_card(") || line.contains("post_update(");
                if (posts && current != "post_feed_card")
                    || line.contains("-> grokhub_core::UpdateCard")
                    || line.contains("-> UpdateCard")
                {
                    sites.insert(current.clone());
                }
            }
        }
    }
    sites
}

/// Every Home card the cabin posts names its item: one row per post site,
/// driven through the site where a test cabin can, else built from the same
/// call the site makes. A new site with no row fails here.
#[test]
fn every_home_card_the_cabin_posts_names_its_item() {
    use grokhub_agent::route::heal::{joined_messages, pause_msg};
    use grokhub_agent::route::tune;
    use grokhub_core::proactive::{proactive_card, Candidate, CandidateSource, ProactiveRoute};
    let _g = config::hold_test_config();
    let root = config::test_config_root("feed-names");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let _pin = config::TestConfigDir::set(root.clone());
    let mut cabin = Cabin::quiet_for_test();
    let now = now_ms();
    let mut rows: Vec<(&str, UpdateCard, &str)> = Vec::new();

    cabin.note_automation_done("auto-1", "Nightly backup", "Copied 3 files.");
    rows.push((
        "note_automation_done",
        posted_card(&cabin, "note_automation_done", |c| c.source_id == "auto-1"),
        "Nightly backup",
    ));
    cabin.note_schedule_created("sched-1", "Morning brief", "every day at 8:00");
    rows.push((
        "note_schedule_created",
        posted_card(&cabin, "note_schedule_created", |c| {
            c.source_id == "sched-1"
        }),
        "Morning brief",
    ));
    cabin.automations.push(health_job("job-7"));
    cabin.note_auto_failed("job-7", "The disk is full.");
    rows.push((
        "note_auto_failed",
        posted_card(&cabin, "note_auto_failed", |c| c.title.ends_with(" failed")),
        "Board summary",
    ));
    cabin
        .live_mut()
        .push(("user".into(), "Index ~/Projects".into()));
    cabin.post_turn_crash();
    rows.push((
        "post_turn_crash",
        posted_card(&cabin, "post_turn_crash", grokhub_core::is_crash_card),
        "Index ~/Projects",
    ));
    cabin.deliver_heal(super::router_ui::HealOut {
        msgs: [
            vec![pause_msg("grok-4.7", "It failed 3 times in a row.", &[])],
            joined_messages(&["grok-4.8".to_string()]),
        ]
        .concat(),
        cleared: Vec::new(),
    });
    rows.push((
        "deliver_heal",
        posted_card(&cabin, "deliver_heal", |c| c.title.starts_with("Paused:")),
        "grok-4.7",
    ));
    rows.push((
        "deliver_heal",
        posted_card(&cabin, "deliver_heal", |c| c.source_id == "new:grok-4.8"),
        "grok-4.8",
    ));
    let promoted = tune::Candidate {
        id: "chat:default#3".into(),
        class: "chat:default".into(),
        change: Some(tune::Change::Order {
            model: "grok-4-fast".into(),
        }),
        stage: tune::Stage::Watch,
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
    cabin.post_why_cards(
        &tune::TuneState {
            candidates: vec![promoted],
            ..tune::TuneState::default()
        },
        &["chat:default#3".to_string()],
    );
    rows.push((
        "post_why_cards",
        posted_card(&cabin, "post_why_cards", |c| {
            c.source_id.starts_with("router-why:")
        }),
        "grok-4-fast",
    ));
    cabin.post_screen_done(
        "Screen recording 0:42: video stutters on 4K",
        std::path::Path::new("rec-1"),
        "",
        Err("No xAI key.".into()),
    );
    rows.push((
        "post_screen_done",
        posted_card(&cabin, "post_screen_done", |c| {
            c.source_id.starts_with("screenrec:")
        }),
        "video stutters on 4K",
    ));
    let report = grokhub_core::audio_check::AudioReport {
        title: "Yeti input clipping".into(),
        card_title: "Audio check: Yeti input clipping at -0.2 dBFS".into(),
        summary: "Lower the gain.".into(),
        text: "Yeti input clipping at -0.2 dBFS.".into(),
    };
    cabin.finish_audio_check("", Ok(("Yeti".into(), report)));
    rows.push((
        "finish_audio_check",
        posted_card(&cabin, "finish_audio_check", |c| {
            c.source_id.starts_with("audiocheck:")
        }),
        "Yeti",
    ));
    let sam = Candidate::soft(
        CandidateSource::Mail,
        "reply to Sam's email",
        "sam reply",
        0.9,
        0.8,
        "Sam asked about Friday.",
    );
    assert_eq!(cabin.post_proactive(vec![sam], now), 1);
    rows.push((
        "post_proactive",
        posted_card(&cabin, "post_proactive", |c| c.pulse.proactive.is_some()),
        "Sam's email",
    ));
    let revert = grokhub_core::self_review::RevertCandidate {
        skill: "inbox-zero".into(),
        version: 2,
        before: (9, 10),
        after: (3, 10),
    };
    let (title, body) = (revert.title(), revert.numbers());
    cabin.harness.self_review.budget = 1;
    cabin.post_self_review_card(
        super::self_review_ui::ProposalCard {
            source: "revert:inbox-zero".into(),
            skill: "inbox-zero",
            title: &title,
            body: &body,
            details: String::new(),
            md: "",
        },
        now,
    );
    rows.push((
        "post_self_review_card",
        posted_card(&cabin, "post_self_review_card", |c| {
            c.source_id == "revert:inbox-zero"
        }),
        "inbox-zero",
    ));
    cabin.post_router_review(now);
    rows.push((
        "post_router_review",
        posted_card(&cabin, "post_router_review", |c| {
            c.source_id == "router-review"
        }),
        "router review",
    ));
    cabin
        .live_mut()
        .push(("user".into(), "Summarize inbox".into()));
    cabin.start_model_wait("chat:default", "grok-4.7", now);
    rows.push((
        "start_model_wait",
        posted_card(&cabin, "start_model_wait", grokhub_core::is_model_wait_card),
        "grok-4.7",
    ));
    std::fs::write(root.join("projects.json"), "{\"torn").unwrap();
    let _: Vec<serde_json::Value> =
        config::load_json(&root.join("projects.json"), config::JSON_STORE_CAP);
    cabin.poll_recovered_stores();
    rows.push((
        "poll_recovered_stores",
        posted_card(&cabin, "poll_recovered_stores", |c| {
            c.source_id.starts_with("recovered:")
        }),
        "Projects list",
    ));
    cabin.add_interest("Rust");
    cabin.post_source_run(super::pulse_sources_ui::SourceRun {
        items: vec![grokhub_core::pulse_sources::SourceItem {
            title: "Rust 1.99 released".into(),
            url: "https://blog.rust-lang.org/2026/10/09/Rust-1.99.html".into(),
            kind: grokhub_core::pulse_sources::SourceKind::HackerNews,
            source_name: "Hacker News".into(),
            published_ms: now,
            summary: "812 points, 301 comments on Hacker News.".into(),
            image: None,
            topic: Some("Rust".into()),
            engagement: 0,
        }],
        errors: Vec::new(),
        fetched: 1,
        asked: 1,
        at_ms: now,
    });
    rows.push((
        "finish_source_run",
        posted_card(&cabin, "finish_source_run", |c| {
            c.source_id.starts_with("source:")
        }),
        "Rust 1.99",
    ));
    rows.push((
        "tick_daily_drop",
        grokhub_core::pulse_sources::item_card(
            &grokhub_core::pulse_sources::Ranked {
                item: grokhub_core::pulse_sources::SourceItem {
                    title: "Daily drop story".into(),
                    url: "https://hn.test/daily-drop".into(),
                    kind: grokhub_core::pulse_sources::SourceKind::HackerNews,
                    source_name: "Hacker News".into(),
                    published_ms: now,
                    summary: "A ranked pick for the overnight drop.".into(),
                    image: None,
                    topic: None,
                    engagement: 0,
                },
                score: 80,
                because: None,
            },
            now,
        ),
        "Daily drop story",
    ));
    rows.push((
        "model_wait_card",
        super::model_wait_ui::model_wait_card(
            &["grok-4.6".into()],
            "grok-4.6",
            "9:05 PM",
            "dream time",
            now,
        ),
        "grok-4.6",
    ));
    // Sites a test cabin can't reach without a live run: the card from the same call.
    let guard_text =
        "Auto was succeeding less on everyday chat, so it now starts that work one step higher.";
    let guard = super::router_ui::guard_card("chat:default", guard_text, now);
    rows.push(("guard_card", guard.clone(), "everyday chat"));
    rows.push(("poll_guard", guard, "everyday chat"));
    let run = super::background::BgRun {
        id: 1,
        thread_id: "t-bg".into(),
        title: "Sync ~/Photos".into(),
        origin: grokhub_core::BgOrigin::User,
        rx: None,
        say: String::new(),
        action: String::new(),
        started: std::time::Instant::now(),
        end: None,
        session: String::new(),
        resumed: None,
        fork_hold: false,
        native_session: None,
        automation: None,
    };
    let crashed = super::background::bg_crash_card(&run);
    rows.push(("bg_crash_card", crashed.clone(), "Sync ~/Photos"));
    rows.push(("poll_bg_runs", crashed, "Sync ~/Photos"));
    let change = grokhub_agent::harness::Change {
        seq: 3,
        at: now,
        kind: "model".into(),
        id: "chat:default".into(),
        op: grokhub_agent::harness::ChangeOp::Modify,
        origin: Default::default(),
        reason: String::new(),
        before_hash: String::new(),
        after_hash: String::new(),
        undoes: None,
        label: String::new(),
    };
    rows.push((
        "model_change_note",
        super::change_undo::model_change_note(&change),
        "chat:default",
    ));
    let trial = grokhub_agent::harness::Version {
        version: "skill:save-page@0123456789ab".into(),
        label: "Use ctrl+s in save-page".into(),
        diff: "+1 -3 lines".into(),
        share_pct: 20,
        min_runs: 10,
        status: grokhub_agent::harness::VersionStatus::Kept,
        note: "Kept: Use ctrl+s in save-page, 18% fewer steps over 12 runs".into(),
        at: now,
        ..Default::default()
    };
    cabin.post_trial_note(&trial);
    rows.push((
        "post_trial_note",
        posted_card(&cabin, "post_trial_note", |c| {
            c.source_id.starts_with("canary:")
        }),
        "save-page",
    ));
    let trying = grokhub_agent::harness::Version {
        status: grokhub_agent::harness::VersionStatus::Canary,
        note: String::new(),
        ..trial
    };
    rows.push((
        "trial_card",
        super::canary_ui::trial_card(&trying),
        "save-page",
    ));
    rows.push((
        "poll_self_changes",
        grokhub_core::self_change_card("skill", "inbox-zero", "added", "", now),
        "inbox-zero",
    ));
    let done = grokhub_core::DoneForYou {
        kind: "connection".into(),
        target: "notes".into(),
        undo_ref: 4,
        mind_key: "proactive:connection_disable".into(),
        answered: false,
    };
    rows.push((
        "tick_auto_act",
        grokhub_core::done_for_you_card(
            "Turned off the notes connection",
            "It failed every start.",
            done,
            now,
        ),
        "notes connection",
    ));
    let notes = Candidate::soft(
        CandidateSource::SystemState,
        "turn off the notes connection",
        "turn off the notes connection",
        0.8,
        0.9,
        "It failed every start.",
    );
    rows.push((
        "offer_card",
        proactive_card(&notes, ProactiveRoute::Suggest, now),
        "notes connection",
    ));
    rows.push((
        "offer_card",
        proactive_card(&notes, ProactiveRoute::ICan, now),
        "notes connection",
    ));

    for (site, card, name) in &rows {
        let text = format!("{} {}", card.title, card.body.as_deref().unwrap_or(""));
        assert!(text.contains(name), "{site}: {name:?} not in {text:?}");
        assert!(grokhub_core::names_its_item(card), "{site}: {card:?}");
        let mut feed = Vec::new();
        assert!(
            grokhub_core::post_update(&mut feed, card.clone()),
            "{site}: the feed refused a named card"
        );
    }
    let covered: std::collections::BTreeSet<String> =
        rows.iter().map(|(site, ..)| site.to_string()).collect();
    let missing: Vec<String> = app_feed_post_sites()
        .difference(&covered)
        .cloned()
        .collect();
    assert!(
        missing.is_empty(),
        "Home card sites with no row in this test: {missing:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Corrupt settings, keys, Home feed and hub files: each loads defaults, keeps its
/// bytes in a `.corrupt-<ms>` copy, and gets one Home card naming it and the copy.
#[test]
fn corrupt_stores_load_defaults_keep_a_copy_and_post_a_named_card() {
    let _g = config::hold_test_config();
    let root = config::test_config_root("corrupt-stores");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let _pin = config::TestConfigDir::set(root.clone());
    let mut cabin = Cabin::quiet_for_test();
    let _ = grokhub_core::recovered::drain();
    let files = [
        ("app.json", "{\"deviceName\": \"cab", "Settings file"),
        ("secrets.json", "\u{0}\u{1}garbage", "Keys file"),
        ("updates.json", "[{\"id\": 3,", "Home feed"),
        ("hub-state.json", "not json", "Hub pairing file"),
    ];
    for (name, bytes, _) in files {
        std::fs::write(root.join(name), bytes).unwrap();
    }

    let cfg = config::load();
    assert_eq!(
        cfg.device_name,
        config::default_device_name(),
        "settings fell back to defaults"
    );
    assert_eq!(
        crate::secrets::load().api_key,
        "",
        "keys fell back to empty"
    );
    assert!(
        crate::feed::load().is_empty(),
        "the feed fell back to empty"
    );
    assert!(grokhub_core::load_hub_state(&config::hub_state_path()).is_none());
    // Checked before the cards post: posting saves a fresh updates.json on a
    // background thread, which could land before a later check.
    for (name, _, _) in files {
        assert!(!root.join(name).exists(), "{name} was moved aside");
    }
    cabin.poll_recovered_stores();

    for (name, bytes, label) in files {
        let copies: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&format!("{name}.corrupt-")))
            })
            .collect();
        assert_eq!(copies.len(), 1, "{name}: {copies:?}");
        assert_eq!(
            std::fs::read_to_string(&copies[0]).unwrap(),
            bytes,
            "{name}: the copy keeps the bytes"
        );
        let title = format!("{label} was unreadable");
        let cards: Vec<&UpdateCard> = cabin.updates.iter().filter(|c| c.title == title).collect();
        assert_eq!(cards.len(), 1, "{name}: {:?}", cabin.updates);
        let body = format!(
            "Restored defaults, kept a copy at {}.",
            grokhub_core::recovered::home_short(&copies[0])
        );
        assert_eq!(cards[0].body.as_deref(), Some(body.as_str()), "{name}");
    }
    // Draining again posts nothing new.
    let before = cabin.updates.len();
    cabin.poll_recovered_stores();
    assert_eq!(cabin.updates.len(), before);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn token_budget_warns_once_and_holds_scheduled_work() {
    let root = config::test_config_root("token-budget");
    let _ = std::fs::remove_dir_all(&root);
    let _pin = config::TestConfigDir::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    // Quiet all day so the test never pings the desktop.
    app.cfg.quiet_start = "00:00".into();
    app.cfg.quiet_end = "23:59".into();
    app.cfg.daily_token_budget = 1_000;
    app.cfg.budget_pauses_scheduled = true;
    app.usage.day = Cabin::local_day();

    let mut u = GrokUsage {
        input_tokens: 500,
        ..Default::default()
    };
    app.merge_grok_usage(&u);
    assert!(!app.status.contains("Token budget"), "{}", app.status);
    assert!(!app.budget_holds_scheduled());

    u.input_tokens = 850;
    app.merge_grok_usage(&u);
    assert!(
        app.status.starts_with("Token budget at 80%"),
        "{}",
        app.status
    );
    app.status.clear();
    u.input_tokens = 900;
    app.merge_grok_usage(&u);
    assert!(app.status.is_empty(), "near warns once: {}", app.status);

    u.input_tokens = 1_200;
    app.merge_grok_usage(&u);
    assert!(
        app.status.starts_with("Token budget used up"),
        "{}",
        app.status
    );
    assert!(
        app.status.contains("waits until tomorrow"),
        "{}",
        app.status
    );
    assert!(app.budget_holds_scheduled());
    app.cfg.budget_pauses_scheduled = false;
    assert!(!app.budget_holds_scheduled(), "pause off: warn only");
    app.cfg.budget_pauses_scheduled = true;
    app.cfg.daily_token_budget = 0;
    assert!(!app.budget_holds_scheduled(), "no budget: never hold");

    let (title, body) = super::token_budget_notice(
        grokhub_core::BudgetLevel::Over,
        "1.2k of 1000 tokens today (120%)",
        false,
    );
    assert_eq!(title, "Token budget used up");
    assert!(!body.contains("tomorrow"), "{body}");
}

#[test]
fn over_budget_holds_night_loops_and_anticipate() {
    let src = cabin_src();
    for f in ["tick_night", "tick_loops"] {
        let body = fn_src(&src, f);
        assert!(
            body.contains("self.budget_holds_scheduled()"),
            "{f}: {body}"
        );
    }
    let anticipate_hold = src.matches("if self.budget_holds_scheduled() {").count();
    assert!(
        anticipate_hold >= 3,
        "night, loops, and anticipate: {anticipate_hold}"
    );
    let settings = include_str!("settings.rs");
    assert!(settings.contains("Daily token budget"));
    assert!(settings.contains("Pause scheduled work over budget"));
    let old: config::AppConfig = serde_json::from_str("{}").expect("empty app.json loads");
    assert_eq!(old.daily_token_budget, 0);
    assert!(old.budget_pauses_scheduled);
    let saved = serde_json::to_string(&config::AppConfig::default()).unwrap();
    assert!(
        !saved.contains("dailyTokenBudget") && !saved.contains("daily_token_budget"),
        "off writes nothing"
    );
}

#[test]
fn dropping_files_says_what_happens() {
    use super::plus::{drop_extra_note, drop_hint_line};
    assert_eq!(
        drop_hint_line(false, 1),
        "Drop to attach to your next message"
    );
    assert_eq!(
        drop_hint_line(true, 1),
        "Drop to use as the Imagine reference"
    );
    assert!(drop_hint_line(false, 3).ends_with("3 files"));
    assert_eq!(drop_extra_note(0), None);
    assert!(drop_extra_note(1).unwrap().contains("1 more file"));
    assert!(drop_extra_note(4).unwrap().contains("4 more file"));
    let app = Cabin::quiet_for_test();
    let ctx = egui::Context::default();
    let mut input = egui::RawInput::default();
    input.hovered_files.push(egui::HoveredFile::default());
    let out = crate::theme::test_pass(&ctx, input, |ui| {
        let ctx = ui.ctx().clone();
        crate::theme::apply(&ctx, true);
        app.paint_drop_hint(&ctx);
    });
    assert!(
        !out.shapes.is_empty(),
        "hovering a file paints the drop hint"
    );
    let quiet = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
        let ctx = ui.ctx().clone();
        app.paint_drop_hint(&ctx);
    });
    assert!(quiet.shapes.is_empty(), "no hover, no overlay");
    let src = cabin_src();
    assert!(fn_src(&src, "take_dropped_attach").contains("self.drop_extra"));
}

#[test]
fn export_writes_html_and_json_next_to_export_md() {
    let mut quiet = QuietCabin::boot("slash-export-formats");
    let cabin = &mut quiet.cabin;
    cabin.messages = std::sync::Arc::new(vec![
        ("user".into(), "harbor <b>export</b> line".into()),
        ("assistant".into(), "1. **one**\n2. two".into()),
    ]);
    cabin.cfg.project_dir.clear();

    cabin.run_slash_line("/export html");
    let html_path = cabin
        .status
        .strip_prefix("Wrote ")
        .unwrap_or("")
        .to_string();
    assert!(html_path.ends_with("export.html"), "{}", cabin.status);
    wait_for("export.html must hold the escaped chat", || {
        let page = std::fs::read_to_string(&html_path).unwrap_or_default();
        page.contains("harbor &lt;b&gt;export&lt;/b&gt; line")
            && page.contains("<strong>one</strong>")
    });

    cabin.run_slash_line("/export json");
    let json_path = cabin
        .status
        .strip_prefix("Wrote ")
        .unwrap_or("")
        .to_string();
    assert!(json_path.ends_with("export.json"), "{}", cabin.status);
    wait_for("export.json must parse and keep both turns", || {
        std::fs::read_to_string(&json_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .is_some_and(|v| v["messages"].as_array().is_some_and(|m| m.len() == 2))
    });

    cabin.run_slash_line("/export md");
    assert!(cabin.status.ends_with("export.md"), "{}", cabin.status);
    cabin.run_slash_line("/export pdf");
    assert_eq!(cabin.status, grokhub_core::EXPORT_FORMATS_HINT);
    assert!(!cabin.running);
}

// ---- Folded Cursor test drafts (#259, #266, #433–#446, reviewed 2026-10-04). Each test notes its source PR. ----

// Folded from PR #266. Test only: `append_composer` already drops whitespace-only text, so the PR's code guard was redundant.
#[test]
fn chat_paste_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    app.composer = "chat line".into();
    app.imagine_prompt = "sunset".into();
    app.apply_clipboard(PlusTarget::Chat, "over the harbor\n");
    assert_eq!(app.composer, "chat line\nover the harbor");
    assert_eq!(app.imagine_prompt, "sunset");
    assert_eq!(app.status, "Pasted clipboard");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    app.apply_clipboard(PlusTarget::Chat, "   \n");
    assert_eq!(app.composer, "chat line\nover the harbor");
    assert_eq!(app.imagine_prompt, "sunset");
    assert_eq!(app.status, "Pasted clipboard");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #433. The Err path saves suggestions on a worker thread, which
// can't see a thread-local test dir: point GROKHUB_CONFIG at a temp root and wait
// for the save, so the test never writes the real suggestions.json.
#[test]
fn apply_review_reply_err_stays_idle() {
    let _hold = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("review-reply-err");
    std::fs::create_dir_all(&root).expect("config root");
    assert!(cabin.suggestions.last_review_day.is_none());
    assert!(!cabin.host_diff_kick);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());

    // Err only: Ok would parse suggestions and spawn more work.
    cabin.apply_review_reply(Err("timeout".into()));

    assert_eq!(cabin.status, "Nightly review held — timeout");
    assert_eq!(
        cabin.suggestions.last_review_day.as_deref(),
        Some(Cabin::local_day().as_str())
    );
    assert!(!cabin.host_diff_kick);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());
    let saved = root.join("suggestions.json");
    let start = std::time::Instant::now();
    while !saved.exists() && start.elapsed() < std::time::Duration::from_secs(3) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let body = std::fs::read_to_string(&saved).expect("held review saved under the test root");
    assert!(body.contains(&Cabin::local_day()), "{body}");
    release_isolated(&root, cabin);
}

// Folded from PR #434.
#[test]
fn greeting_galley_h_drives_empty_home_greet_top() {
    // Real empty-home placement path: `greeting_galley_h` + mark → `empty_home_greet_top`.
    // Quiet cabin only: no greeting model call.
    let mut cabin = Cabin::quiet_for_test();
    cabin.greeting = "Hello from empty home".into();
    let ctx = egui::Context::default();
    // The greeting is set in the app's bundled fonts. `install_fonts` runs once
    // per process, so in the full suite this fresh context would get none.
    crate::theme::install_fonts_on(&ctx);
    let _ = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
        let mark_h = 40.0 + 12.0;
        let short_h = super::greeting_galley_h(ui, &cabin.greeting, 480.0);
        assert!(
            short_h >= crate::theme::GREET_HERO,
            "galley height is at least GREET_HERO: {short_h}"
        );
        let composer_top = super::empty_home_composer_top(800.0, crate::theme::QUERY_MIN_H);
        let short_top = super::empty_home_greet_top(composer_top, short_h + mark_h, 12.0);
        assert!(
            short_top >= 0.0,
            "greeting top stays non-negative: {short_top}"
        );
        let long = "word ".repeat(80);
        let long_h = super::greeting_galley_h(ui, &long, 200.0);
        assert!(
            long_h > short_h,
            "narrow wrap grows galley height: {long_h} vs {short_h}"
        );
        let long_top = super::empty_home_greet_top(composer_top, long_h + mark_h, 12.0);
        assert!(
            long_top < short_top,
            "taller wrapped greeting rises: {long_top} vs {short_top}"
        );
    });
    assert!(!cabin.running, "galley height must not start a run");
    assert!(
        cabin.chat_job_thread.is_none(),
        "galley height must not spawn a chat job"
    );
}

// Folded from PR #441, plus the matching and other-project rows (an idle-only
// check would pass against a stub that always returns None).
#[test]
fn dream_rewind_id_matches_only_this_project() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.rewind_rows.is_empty());
    assert_eq!(cabin.dream_rewind_id(), None);
    cabin.cfg.project_dir = "/tmp/harbor-proj".into();
    cabin.rewind_rows.push(RewindRecord {
        job_id: "job-7".into(),
        path: "/tmp/harbor-snap".into(),
        root: "/tmp/harbor-proj".into(),
        created_at: 1,
        method: "copy".into(),
    });
    assert_eq!(cabin.dream_rewind_id(), Some("job-7"));
    cabin.cfg.project_dir = "/tmp/other-proj".into();
    assert_eq!(cabin.dream_rewind_id(), None);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #442.
#[test]
fn update_probe_poll_clears_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.last_update_probe = Some(std::time::Instant::now());
    let ctx = egui::Context::default();
    cabin.poll_update_probe(&ctx);
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.update_probe_rx.is_none());
    let (tx, rx) = std::sync::mpsc::channel::<crate::update::UpdateProbe>();
    cabin.update_probe_rx = Some(rx);
    cabin.poll_update_probe(&ctx);
    assert!(cabin.update_probe_rx.is_some());
    assert_eq!(cabin.status, "Harbor");
    drop(tx);
    cabin.poll_update_probe(&ctx);
    assert!(cabin.update_probe_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #443.
#[test]
fn palette_search_poll_clears_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_palette_search();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.palette_file_rx.is_none());
    let (tx, rx) = std::sync::mpsc::channel::<(String, String, Vec<String>)>();
    cabin.palette_file_rx = Some(rx);
    cabin.poll_palette_search();
    assert!(cabin.palette_file_rx.is_some());
    assert_eq!(cabin.status, "Harbor");
    drop(tx);
    cabin.poll_palette_search();
    assert!(cabin.palette_file_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #446, plus the two commands that do flip it (idle-only would
// pass against a stub).
#[test]
fn note_cabin_update_landed_flips_only_for_its_steps() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.last_host.is_empty());
    cabin.note_cabin_update_landed();
    assert!(!cabin.cabin_overlay_done);
    cabin.last_host = vec!["ls -la".into()];
    cabin.note_cabin_update_landed();
    assert!(!cabin.cabin_overlay_done);
    cabin.last_host = vec!["grok update".into()];
    cabin.note_cabin_update_landed();
    assert!(
        !cabin.cabin_overlay_done,
        "a CLI update is not the cabin's step"
    );
    cabin.last_host = vec!["git -C ~/GrokHub pull --ff-only".into()];
    cabin.note_cabin_update_landed();
    assert!(cabin.cabin_overlay_done);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// ---- Folded Cursor test drafts (#251–#431, base v2.10.54). Each test notes its source PR. ----

// ---- Cursor fold: Connectors and Grok Build CLI polls ----

// Folded from PR #323.
#[test]
fn single_poll_reports_a_drop() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("single-poll-drop");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    let _pin = crate::config::TestConfigDir::set(root.clone());

    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    assert!(cabin.stream_buf.is_empty() && cabin.thought_buf.is_empty());

    cabin.poll_single();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.grok_p_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<GrokPEvent>();
    cabin.grok_p_rx = Some(rx);
    cabin.running = true;
    cabin.chat_job_thread = Some(cabin.visible_thread_id());
    cabin.poll_single();
    assert!(cabin.grok_p_rx.is_some());
    assert!(cabin.running);
    assert_eq!(cabin.status, "Harbor");

    // Empty buffers → Disconnected else arm (not finish_acp_turn).
    assert!(cabin.stream_buf.is_empty() && cabin.thought_buf.is_empty());
    drop(tx);
    cabin.poll_single();
    assert!(cabin.grok_p_rx.is_none());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert_eq!(cabin.status, "The engine stopped without a reply");
    assert!(cabin.stream_buf.is_empty() && cabin.thought_buf.is_empty());

    let _ = std::fs::remove_dir_all(&root);
}

// Folded from PR #326.
#[test]
fn acp_poll_stays_off_without_handle() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    assert!(cabin.acp.is_none());
    cabin.poll_acp();
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.acp.is_none());
}

#[test]
fn small_meta_text_is_readable_and_search_shows_its_key() {
    assert_eq!(
        crate::theme::SUBTLE,
        egui::Color32::from_rgb(0x8b, 0x90, 0x96)
    );
    assert_eq!(super::sidebar::SEARCH_KEY_HINT, "Ctrl+K");
    assert!(grokhub_core::shortcut_help().contains("Ctrl+K"));
}

#[test]
fn the_palette_closes_on_navigation_and_uses_the_sidebar_names() {
    let mut quiet = QuietCabin::boot("palette-close");
    let cabin = &mut quiet.cabin;
    let _paint = crate::theme::hold_paint_test();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1400.0, 900.0),
        )),
        ..Default::default()
    };
    cabin.nav = super::Nav::Chat;
    cabin.open_palette();
    let mut texts = Vec::new();
    for _ in 0..2 {
        texts.clear();
        let out = crate::theme::test_pass(&ctx, raw.clone(), |ui| cabin.ui_palette(ui.ctx()));
        for clipped in &out.shapes {
            collect_shape_text(&clipped.shape, &mut texts);
        }
    }
    for want in [
        "Search pages and commands",
        "Automations",
        "Skills and Connectors",
        "Pulse",
    ] {
        assert!(
            texts.iter().any(|t| t == want),
            "missing {want:?} in {texts:?}"
        );
    }
    assert!(
        !texts
            .iter()
            .any(|t| t == "Palette" || t == "Go to…" || t == "Night"),
        "{texts:?}"
    );
    assert!(cabin.palette_open);
    let _ = crate::theme::test_pass(&ctx, raw.clone(), |ui| cabin.ui_palette(ui.ctx()));
    assert!(cabin.palette_open, "nothing changed: it stays open");
    // A sidebar click changes the page underneath: the palette goes.
    cabin.nav = super::Nav::Pulse;
    let _ = crate::theme::test_pass(&ctx, raw.clone(), |ui| cabin.ui_palette(ui.ctx()));
    assert!(!cabin.palette_open);
    // A click outside it closes it too.
    cabin.open_palette();
    let _ = crate::theme::test_pass(&ctx, raw.clone(), |ui| cabin.ui_palette(ui.ctx()));
    let at = egui::pos2(1350.0, 850.0);
    let click = egui::RawInput {
        events: vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        ..raw.clone()
    };
    let _ = crate::theme::test_pass(&ctx, click, |ui| cabin.ui_palette(ui.ctx()));
    let release = egui::RawInput {
        events: vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..raw
    };
    let _ = crate::theme::test_pass(&ctx, release, |ui| cabin.ui_palette(ui.ctx()));
    assert!(!cabin.palette_open, "an outside click closes the palette");
}

#[test]
fn motion_wave2e_pulse_header_control_unmoved() {
    let pulse = include_str!("pulse_ui.rs");
    assert!(
        pulse.contains("HEADER_SLOT_W") && pulse.contains("felt_segment"),
        "Feed|Ideas must keep the fixed header slot"
    );
    assert!(
        pulse.contains("pulse_tab_t") && pulse.contains("ideas_axis_x"),
        "Pulse must use Wave 2E crossfade helpers"
    );
}

#[test]
fn motion_wave2e_module_wired() {
    let main = include_str!("../main.rs");
    assert!(
        main.contains("mod motion;"),
        "motion module must be declared"
    );
    let motion = include_str!("../motion.rs");
    assert!(motion.contains("PULSE_CROSSFADE_SECS: f32 = 0.180"));
    assert!(motion.contains("APPROVAL_ENTER_SECS: f32 = 0.200"));
    assert!(motion.contains("CURSOR_SETTLE_SECS: f32 = 0.080"));
    assert!(
        motion.contains("no neon")
            || motion.contains("No neon")
            || motion.contains("no neon trail")
            || motion.contains("No neon trail")
    );
}
/// One composer frame wired like the Chat pill: the send check runs before and
/// after a multiline field whose `return_key` is Command+Enter.
fn composer_key_frame(
    ctx: &egui::Context,
    composer: &mut String,
    modifiers: egui::Modifiers,
    keys: &[egui::Key],
) -> Vec<String> {
    let id = egui::Id::new("composer-key-frame");
    let mut events = vec![egui::Event::ModifiersChanged(modifiers)];
    events.extend(keys.iter().map(|&key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }));
    let raw = egui::RawInput {
        events,
        ..Default::default()
    };
    let mut sent = Vec::new();
    let _ = crate::theme::test_pass(ctx, raw, |ui| {
        ui.memory_mut(|m| m.request_focus(id));
        let focused = ui.memory(|m| m.has_focus(id));
        if let Some(t) = super::take_focused_composer(ui, composer, focused) {
            sent.push(t);
        }
        let edit = ui.add(egui::TextEdit::multiline(composer).id(id).return_key(Some(
            egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Enter),
        )));
        if let Some(t) = super::take_focused_composer(ui, composer, edit.has_focus()) {
            sent.push(t);
        }
    });
    sent
}

#[test]
fn shift_enter_breaks_the_composer_line_and_enter_sends() {
    let ctx = egui::Context::default();
    let mut composer = String::from("first line");
    // Settle focus with a frame that presses nothing.
    assert!(composer_key_frame(&ctx, &mut composer, egui::Modifiers::NONE, &[]).is_empty());

    let sent = composer_key_frame(
        &ctx,
        &mut composer,
        egui::Modifiers::SHIFT,
        &[egui::Key::Enter],
    );
    assert!(sent.is_empty(), "Shift+Enter must not send: {sent:?}");
    assert_eq!(composer, "first line\n");

    // Ctrl on Linux and Windows sets both bits, as winit reports it.
    let ctrl = egui::Modifiers::CTRL | egui::Modifiers::COMMAND;
    let sent = composer_key_frame(&ctx, &mut composer, ctrl, &[egui::Key::Enter]);
    assert!(sent.is_empty(), "Ctrl+Enter must not send: {sent:?}");
    assert_eq!(composer, "first line\n\n");

    composer = String::from("send me");
    let sent = composer_key_frame(
        &ctx,
        &mut composer,
        egui::Modifiers::NONE,
        &[egui::Key::Enter],
    );
    assert_eq!(sent, vec!["send me".to_string()]);
    assert_eq!(composer, "");

    // Alt+Enter still sends (it queues while a reply runs).
    composer = String::from("queue me");
    let sent = composer_key_frame(
        &ctx,
        &mut composer,
        egui::Modifiers::ALT,
        &[egui::Key::Enter],
    );
    assert_eq!(sent, vec!["queue me".to_string()]);

    let src = cabin_src();
    let take = fn_src(&src, "take_focused_composer");
    assert!(take.contains("shift_enter_as_newline"), "{take}");
}

#[test]
fn ctrl_comma_opens_settings_and_the_palette_lists_shortcuts() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.nav = Nav::Chat;
    cabin.palette_open = true;
    let ctx = egui::Context::default();
    let comma = egui::RawInput {
        events: vec![
            egui::Event::ModifiersChanged(egui::Modifiers::COMMAND),
            egui::Event::Key {
                key: egui::Key::Comma,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            },
        ],
        ..Default::default()
    };
    let _ = crate::theme::test_pass(&ctx, comma, |ui| {
        let ctx = ui.ctx().clone();
        cabin.logic_input_actions(&ctx);
    });
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.palette_open);

    cabin.shortcuts_open = false;
    cabin.run_palette(&ctx, "shortcuts");
    assert!(cabin.shortcuts_open);
}
#[test]
fn labs_beta_channel_toggle_is_wired() {
    let settings = include_str!("settings.rs");
    assert!(
        settings.contains("Beta channel")
            && settings.contains("queue_channel_switch")
            && settings.contains("queue_release_channel_switch")
            && settings.contains("channel_labs_status")
            && settings.contains("channel_windows_note")
            && settings.contains("maybe_auto_off_beta_channel")
            && settings.contains("channel_auto_off_note"),
        "Labs must wire the Beta channel toggle, status line, Windows note, and auto-off"
    );
    let app = include_str!("mod.rs");
    assert!(
        app.contains("fn queue_channel_switch")
            && app.contains("fn queue_release_channel_switch")
            && app.contains("release_channel_switch_cmds")
            && app.contains("channel_switch_cmds")
            && app.contains("SettingsSec::Labs")
            && app.contains("fn maybe_auto_off_beta_channel")
            && app.contains("try_auto_off_beta_channel"),
        "App must queue a channel switch from Labs and auto-off when tips match"
    );
    let update = include_str!("../update.rs");
    assert!(
        update.contains("channel_switch_shell")
            && update.contains("--channel")
            && update.contains("CHANNEL_WINDOWS_NOTE")
            && update.contains("try_auto_off_beta_channel")
            && update.contains("write_installed_channel"),
        "Update helper must build the install.sh --channel plan with rollback and auto-off"
    );
}

fn wait_diagnose(cabin: &mut Cabin) {
    let start = std::time::Instant::now();
    while cabin.harness.diagnose_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(4)
    {
        cabin.poll_diagnose();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn wifi_intent_without_system_state_runs_no_probe_and_posts_the_ask() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("diagnose-intent");
    std::fs::create_dir_all(&root).unwrap();
    let before = cabin.messages.len();
    cabin.send_chat("my wifi doesn't work".into());
    assert!(!cabin.running, "a diagnose intent never reaches the model");
    wait_diagnose(&mut cabin);
    assert_eq!(cabin.messages.len(), before + 2);
    assert_eq!(
        cabin.messages[before],
        ("user".into(), "my wifi doesn't work".into())
    );
    assert_eq!(
        cabin.messages[before + 1],
        ("assistant".into(), grokhub_agent::repair::SCOPE_ASK.into())
    );
    let trace = cabin
        .threads
        .get(cabin.thread_idx)
        .map(|t| t.id.clone())
        .unwrap_or_else(|| "session".into());
    let spans = hx::read_spans(&root, &trace).unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].tool, "diagnose");
    assert_eq!(spans[0].decision, "deny");
    assert_eq!(spans[0].origin, hx::Origin::Repair);
    let ask = cabin
        .harness
        .indexer
        .asks
        .first()
        .expect("the Spike-8a ask card is queued");
    assert_eq!(ask.scope, hx::Scope::SystemState);
    assert_eq!(ask.why, grokhub_agent::repair::SCOPE_ASK_WHY);
    assert_eq!(cabin.harness.indexer.asks.len(), 1);
    release_isolated(&root, cabin);
}

#[test]
fn diagnose_slash_without_system_state_is_a_slash_result_with_the_ask() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("diagnose-slash");
    std::fs::create_dir_all(&root).unwrap();
    let before = cabin.messages.len();
    cabin.run_slash_line("/diagnose");
    assert!(!cabin.running);
    wait_diagnose(&mut cabin);
    assert_eq!(cabin.messages.len(), before + 1);
    let (role, body) = cabin.messages.last().cloned().unwrap();
    assert_eq!(role, "assistant");
    assert!(
        grokhub_core::is_cabin_slash_turn(&role, &body),
        "stays out of the next model kick"
    );
    assert!(body.ends_with(grokhub_agent::repair::SCOPE_ASK), "{body}");
    release_isolated(&root, cabin);
}

#[test]
fn diagnose_answers_in_the_chat_that_asked_after_a_switch() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("diagnose-switch");
    std::fs::create_dir_all(&root).unwrap();
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Asked here", false));
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Moved here", false));
    let (asked, moved) = (cabin.threads.len() - 2, cabin.threads.len() - 1);
    cabin.apply_switch_thread(asked);
    cabin.run_slash_line("/diagnose");
    cabin.apply_switch_thread(moved);
    let before = cabin.messages.len();
    wait_diagnose(&mut cabin);
    assert!(cabin.harness.diagnose_rx.is_none(), "the diagnose finished");
    assert_eq!(
        cabin.messages.len(),
        before,
        "the chat on screen gets nothing"
    );
    cabin.apply_switch_thread(asked);
    let (role, body) = cabin.messages.last().cloned().unwrap();
    assert_eq!(role, "assistant");
    assert!(body.ends_with(grokhub_agent::repair::SCOPE_ASK), "{body}");
    release_isolated(&root, cabin);
}

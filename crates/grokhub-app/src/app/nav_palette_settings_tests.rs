//! Navigation, command palette, settings pages and shortcuts tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

#[test]
fn health_opens_the_about_page() {
    assert_eq!(
        super::settings::health_settings_sec(),
        super::SettingsSec::About
    );
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
        src.contains("Search ({SEARCH_KEY_HINT})") && src.contains("self.open_palette()"),
        "Search stays the palette (chat tab tools) — no second search page"
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
fn about_heading_opens_about_with_the_build() {
    // QA: the About heading opened a panel titled "Update" with no version.
    assert_eq!(
        super::settings_group_home(super::SettingsGroup::About),
        super::SettingsSec::About
    );
    let about = include_str!("settings.rs")
        .split("SettingsSec::About => {")
        .nth(1)
        .and_then(|s| s.split("SettingsSec::Defaults => {").next())
        .expect("About arm");
    assert!(
        about.contains("crate::update::build_detail_line()"),
        "About shows the version and build: {about}"
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
fn settings_defaults_section() {
    let settings = include_str!("settings.rs");
    assert!(
        settings.contains("(SettingsSec::Defaults, \"Defaults\")"),
        "Defaults is a Settings section"
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
fn appearance_applies_without_a_save_action() {
    assert!(!super::settings::settings_save_visible(
        super::SettingsSec::Appearance
    ));
    assert!(super::settings::settings_save_visible(
        super::SettingsSec::Account
    ));
}

#[test]
fn home_api_key_hint_points_to_defaults() {
    assert!(include_str!("pages.rs").contains("or add an API key in Settings → Defaults."));
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

    let mut kept = crate::threads::ChatThread::new("Harbor", false);
    kept.messages = std::sync::Arc::new(vec![("user".into(), "dock note".into())]);
    kept.accessed_ms = 5_000;
    let kept_id = kept.id.clone();
    cabin.threads.push(kept);
    cabin.thread_idx = cabin.threads.len() - 1;
    cabin.messages = cabin.threads[cabin.thread_idx].messages.clone();
    cabin.remember_last_chat_tab();
    assert_eq!(cabin.cfg.last_chat_tab, kept_id);
    cabin.open_fresh_home();
    assert_eq!(
        cabin.cfg.last_chat_tab, kept_id,
        "Home must keep last_chat_tab"
    );
    assert!(cabin.messages.is_empty());
    let n = cabin.threads.len();
    let io = cabin.persist_io.clone();
    let block = io.lock().unwrap_or_else(|e| e.into_inner());
    cabin.set_nav_id("chat");
    drop(block);
    assert!(matches!(cabin.nav, super::Nav::Chat));
    assert_eq!(cabin.threads.len(), n);
    assert_eq!(cabin.threads[cabin.thread_idx].id, kept_id);
    assert_eq!(cabin.messages.len(), 1);
    assert_eq!(cabin.messages[0].1, "dock note");
    assert!(!cabin.running);

    std::thread::sleep(std::time::Duration::from_millis(100));
    drop(io.lock().unwrap_or_else(|e| e.into_inner()));
    std::env::remove_var("GROKHUB_CONFIG");
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
        copied[0].starts_with("app Loadout\nversion "),
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
        super::SettingsSec::About
    ));
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
        "Objectives",
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
fn palette_arrow_down_reveals_the_next_row_before_the_edge() {
    let mut quiet = QuietCabin::boot("palette-peek");
    let cabin = &mut quiet.cabin;
    let _paint = crate::theme::hold_paint_test();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    // Reduced motion: the scroll lands at once.
    ctx.all_styles_mut(|s| s.animation_time = 0.0);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1400.0, 900.0),
        )),
        ..Default::default()
    };
    let cmds = grokhub_core::filter_palette("");
    assert!(cmds.len() > 9, "{}", cmds.len());
    cabin.nav = super::Nav::Chat;
    cabin.open_palette();
    let painted = |cabin: &mut Cabin, raw: egui::RawInput| {
        let mut texts = Vec::new();
        let out = crate::theme::test_pass(&ctx, raw, |ui| cabin.ui_palette(ui.ctx()));
        for clipped in &out.shapes {
            collect_shape_text(&clipped.shape, &mut texts);
        }
        texts
    };
    // A new window sizes itself on its first pass and paints on the next.
    let _ = painted(cabin, raw.clone());
    let first = painted(cabin, raw.clone());
    // Eight rows and half the ninth show (default 3 px spacing); the tenth is
    // below the cut.
    assert!(first.iter().any(|t| t == cmds[0].0), "{first:?}");
    assert!(first.iter().any(|t| t == cmds[8].0), "{first:?}");
    assert!(!first.iter().any(|t| t == cmds[9].0), "{first:?}");
    let down = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..raw.clone()
    };
    for _ in 0..8 {
        let _ = painted(cabin, down.clone());
    }
    assert_eq!(cabin.palette_pick, 8);
    let mut texts = Vec::new();
    for _ in 0..2 {
        texts = painted(cabin, raw.clone());
    }
    // Row 8 is picked, and row 9 already shows; the top row scrolled away.
    assert!(texts.iter().any(|t| t == cmds[9].0), "{texts:?}");
    assert!(!texts.iter().any(|t| t == cmds[0].0), "{texts:?}");
}

#[test]
fn pulse_header_has_both_actions_and_no_tab_switch() {
    let pulse = include_str!("pulse_ui.rs");
    let header = pulse
        .split("fn paint_pulse_header(")
        .nth(1)
        .and_then(|s| s.split("fn paint_source_status(").next())
        .expect("paint_pulse_header");
    assert!(
        header.contains("ghost_pill(ui, \"Feed instructions\")")
            && header.contains("ghost_pill(ui, suggest_label(busy))"),
        "the title row carries Suggest for you and Feed instructions: {header}"
    );
    assert!(
        !pulse.contains("PulseTab")
            && !pulse.contains("pulse_header_tab")
            && !pulse.contains("pulse_tab_t"),
        "Pulse is one stream: no Feed | For You switch or crossfade"
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
    assert!(
        settings.contains("ui_labs_diagnostics"),
        "Labs must paint the Diagnostics mode toggle"
    );
    let diag = include_str!("diagnostics_ui.rs");
    assert!(
        diag.contains("Diagnostics mode")
            && diag.contains("save_chat_diagnostics")
            && diag.contains("loadout-diagnostics"),
        "Labs Diagnostics UI must wire the mode toggle and Desktop zip export"
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

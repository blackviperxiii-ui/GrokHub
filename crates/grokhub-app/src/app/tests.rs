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

#[path = "work_log_tests.rs"]
mod work_log_tests;

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

#[path = "nav_palette_settings_tests.rs"]
mod nav_palette_settings_tests;

#[path = "slash_command_tests.rs"]
mod slash_command_tests;

#[path = "persist_tests.rs"]
mod persist_tests;

#[path = "home_feed_tests.rs"]
mod home_feed_tests;

#[path = "home_mission_tests.rs"]
mod home_mission_tests;

#[path = "chat_threads_tests.rs"]
mod chat_threads_tests;

#[path = "composer_chips_tests.rs"]
mod composer_chips_tests;

#[path = "live_turn_tests.rs"]
mod live_turn_tests;

#[path = "permission_ask_tests.rs"]
mod permission_ask_tests;

#[path = "night_budget_tests.rs"]
mod night_budget_tests;

#[path = "slash_gate_tests.rs"]
mod slash_gate_tests;
#[path = "turn_routing_tests.rs"]
mod turn_routing_tests;

fn cabin_src() -> String {
    // Windows CI checkouts often embed CRLF via include_str; source-inspection
    // asserts match on \n, so normalize once here.
    concat!(
        include_str!("mod.rs"),
        include_str!("lan_host.rs"),
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
        include_str!("chat_tabs.rs"),
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
    assert!(
        persist_secrets.contains("persist_svc")
            && persist_secrets.contains("submit_secrets")
            && persist_secrets.contains("next_persist_gen")
            && !persist_secrets.contains("thread::spawn")
            && !persist_secrets.contains("secrets::save")
            && !persist_secrets.contains("self.persist()"),
        "Settings Save must hand secrets.json to PersistService, not write it on the UI thread: {persist_secrets}"
    );
    let svc_secrets = include_str!("persist_service.rs")
        .split("PersistMsg::Secrets {")
        .last()
        .and_then(|s| s.split("PersistMsg::Flush(done)").next())
        .expect("PersistService secrets arm");
    let svc_lock = svc_secrets
        .find("io.lock()")
        .expect("secrets arm takes persist_io");
    let svc_order = svc_secrets
        .find("take_secrets(gen)")
        .expect("secrets arm checks order");
    let svc_save = svc_secrets
        .find("secrets::save")
        .expect("secrets arm writes");
    assert!(
        svc_lock < svc_order && svc_order < svc_save,
        "PersistService must take persist_io and drop an older secrets write before secrets.json: {svc_secrets}"
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
        bearer.contains("persist_oauth_secrets")
            && !bearer.contains("thread::spawn")
            && !bearer.contains("secrets::save"),
        "OAuth refresh must hand secrets.json to PersistService, not write it on the UI thread: {bearer}"
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
fn appearance_tab_offers_loadout_and_classic() {
    let ids: Vec<&str> = grokhub_core::appearance_choices()
        .iter()
        .copied()
        .map(grokhub_core::theme_id)
        .collect();
    assert_eq!(
        ids,
        vec![
            "ops-dark",
            "oled-black",
            "soft-charcoal",
            "dark",
            "light",
            "system"
        ]
    );
    assert_eq!(
        grokhub_core::parse_theme("light"),
        grokhub_core::ThemeChoice::Light
    );
    assert_eq!(
        grokhub_core::parse_theme("ops-dark"),
        grokhub_core::ThemeChoice::OpsDark
    );
    assert!(!grokhub_core::resolve_dark(
        grokhub_core::ThemeChoice::Light,
        true
    ));
    assert!(grokhub_core::resolve_dark(
        grokhub_core::ThemeChoice::Dark,
        false
    ));
    assert!(grokhub_core::resolve_dark(
        grokhub_core::ThemeChoice::OpsDark,
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
    // Wait for that save itself: a fixed poll ran out on a busy Windows runner.
    // Leave USER too: both saves stay tracked, not only the newest.
    cabin.mem_body = "typed user line for the switch".into();
    cabin.open_memory_file("SOUL.md");
    let saves = std::mem::take(&mut cabin.mem_leave_saves);
    assert_eq!(saves.len(), 2, "each tab you left keeps its save handle");
    for save in saves {
        save.join().unwrap();
    }
    assert!(
        crate::config::read_memory("SOUL.md").contains("typed soul line for the switch"),
        "leaving SOUL must flush the unsaved typing"
    );
    assert!(
        crate::config::read_memory("USER.md").contains("typed user line for the switch"),
        "leaving USER must flush its unsaved typing too"
    );
    assert_eq!(cabin.mem_name, "SOUL.md");
    quiet.settle();
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
    assert_eq!(cabin.status, "Plan mode — Loadout will plan first");
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
        web_search_key_draft: String::new(),
        running: false,
        turn_retried: false,
        rejected_bearer: None,
        host_halt: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx: None,
        chat_job_thread: None,
        inflight_open: false,
        run_tools_logged: Vec::new(),
        hub: std::sync::Arc::new(std::sync::Mutex::new(grokhub_core::HubState::empty())),
        hub_on: false,
        hub_port: grokhub_core::DEFAULT_PORT,
        task_prompt: String::new(),
        mem_name: String::new(),
        mem_body: String::new(),
        mem_cache_at: [0, 0, 0],
        mem_leave_saves: Vec::new(),
        mem_cache_body: [String::new(), String::new(), String::new()],
        mem_raw_lines: None,
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
        signin_card: false,
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
        history_delete_armed_at: None,
        history_hits: Vec::new(),
        notices: Default::default(),
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
        focus: Default::default(),
        pc_health: Default::default(),
        print_alerts: Default::default(),
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
        desktop_nyx: None,
        voice_hold_rx: None,
        lips: Default::default(),
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
        visible_chips: Vec::new(),
        home_chips: Vec::new(),
        chip_paint_key: String::new(),
        chip_send_key: None,
        chip_turn_key: None,
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
        file_writes: Vec::new(),
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
        learn_rx: None,
        learn_pending: Vec::new(),
        reflect_after_learn: false,
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
        activity: Default::default(),
        diagnostics: Default::default(),
        home_layout: grokhub_core::home::HomeLayout::default(),
        home_edit: None,
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

    assert_eq!(cabin.status, "Need a real skill name and steps");
    assert!(cabin.suggestions.skills.is_empty());
    assert!(!cabin.running);
    assert!(matches!(cabin.nav, super::Nav::Chat));
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
    wait_file_writes(cabin);
    let html_path = cabin
        .status
        .strip_prefix("Wrote ")
        .unwrap_or("")
        .to_string();
    assert!(
        html_path.contains("export-") && html_path.ends_with(".html"),
        "{}",
        cabin.status
    );
    wait_for("export.html must hold the escaped chat", || {
        let page = std::fs::read_to_string(&html_path).unwrap_or_default();
        page.contains("harbor &lt;b&gt;export&lt;/b&gt; line")
            && page.contains("<strong>one</strong>")
    });

    cabin.run_slash_line("/export json");
    wait_file_writes(cabin);
    let json_path = cabin
        .status
        .strip_prefix("Wrote ")
        .unwrap_or("")
        .to_string();
    assert!(
        json_path.contains("export-") && json_path.ends_with(".json"),
        "{}",
        cabin.status
    );
    wait_for("export.json must parse and keep both turns", || {
        std::fs::read_to_string(&json_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .is_some_and(|v| v["messages"].as_array().is_some_and(|m| m.len() == 2))
    });

    cabin.run_slash_line("/export md");
    wait_file_writes(cabin);
    assert!(cabin.status.ends_with(".md"), "{}", cabin.status);
    cabin.run_slash_line("/export pdf");
    assert_eq!(cabin.status, grokhub_core::EXPORT_FORMATS_HINT);
    assert!(!cabin.running);
}

// ---- Folded Cursor test drafts (#259, #266, #433–#446, reviewed 2026-10-04). Each test notes its source PR. ----

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

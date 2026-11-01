//! Turn routing to the origin thread tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

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
        src.contains("fn stage_new_folder("),
        "folder create helper stays after the chat drawer left"
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
        deleted.contains("land_after_soft_delete")
            || deleted.contains("drop_leaving_thread_chrome"),
        "deleting the visible tab must drop plus-attach and followup budget: {deleted}"
    );
    let land = fn_src(&src, "land_after_soft_delete");
    assert!(
        land.contains("drop_leaving_thread_chrome"),
        "soft-delete land must drop plus-attach and followup budget: {land}"
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
    // The memory files are read and edited in `write_notes`.
    let mem = reflect
        .find("learn_notes_ui::write_notes(")
        .expect("reflect memory");
    assert!(
        reflect[..mem].contains("write_memory") && reflect[..mem].contains("mem_body"),
        "/learn reflect must flush the Memory editor before the note edit: {reflect}"
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
        send_auth.contains("persist_user_turn(self.chat_ready())")
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
        .and_then(|s| s.split("fn delete_all_history").next())
        .expect("delete_thread_at");
    assert!(
        deleted.contains("land_after_soft_delete") && deleted.contains("self.goal_step"),
        "deleting the visible tab lands on another live chat (goal via apply_switch_thread): {deleted}"
    );
    let switch = fn_src(&src, "apply_switch_thread");
    assert!(
        switch.contains("goal.step") && switch.contains("self.goal_step"),
        "switching tabs must adopt the next tab's goal step: {switch}"
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
            skills_ui.contains("\"{product} skills\"") && skills_ui.contains("self.skill_list"),
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
        sign_out.contains("persist_oauth_secrets") && !sign_out.contains("secrets::save"),
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
        poll_o.contains("persist_oauth_secrets") && !poll_o.contains("secrets::save"),
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
        photo_poll.contains("persist_oauth_secrets") && !photo_poll.contains("secrets::save"),
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
        .find("spawn_file_write")
        .expect("export write must leave the UI thread");
    let export_write = export.find("write_new_file").expect("export.md");
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
        .and_then(|s| s.split("fn health_input").next())
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

#[test]
fn composer_stop_leaves_background_runs_going_and_tray_halt_stops_them() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("stop-spares-bg");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let fg_id = format!("native-stop-fg-{}", std::process::id());
    let bg_id = format!("native-stop-bg-{}", std::process::id());
    let fg = grokhub_agent::CancelToken::new();
    let bg = grokhub_agent::CancelToken::new();
    grokhub_agent::watch_cancel(&fg_id, fg.clone());
    grokhub_agent::watch_cancel(&bg_id, bg.clone());
    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    let idx = cabin.thread_idx;
    cabin.threads[idx].grok_session = Some(fg_id.clone());
    cabin.running = true;
    cabin.halt_work("Stopped");
    assert!(fg.is_cancelled(), "Stop ends this chat's run");
    assert!(!bg.is_cancelled(), "Stop leaves a background run going");
    cabin.halt_everything("Halted");
    assert!(bg.is_cancelled(), "tray Halt stops background runs too");
    grokhub_agent::forget_session(&fg_id);
    grokhub_agent::forget_session(&bg_id);
    drop(cabin);
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
        "Worktrees were a grok CLI feature; chats run in the bound project folder"
    );
    assert!(!cabin.running);
    assert_eq!(
        cabin.threads[cabin.thread_idx].grok_session.as_deref(),
        Some("sess"),
        "/worktree leaves the chat's session alone"
    );
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

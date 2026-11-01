//! Night slot, nightly review and token budget tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

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
        search.contains("save_memory_file")
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
            && fn_src(&src, "poll_acp").contains("interrupt_turn_card")
            && fn_src(&src, "poll_single").contains("interrupt_turn_card"),
        "a prompt failure undoes the Doing card; a stream failure interrupts it (KB-03)"
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
        connectors.contains("section_label(ui, grokhub_core::brand::product_name())")
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
    // The greeting, ideas and the feed digest go through `fast_once`.
    for name in [
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

// Landed from PR #137.
#[test]
fn night_add_lands_a_daily_job_and_a_loop() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("night-add");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
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

// Landed from PR #205.
#[test]
fn full_loop_list_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("full-loop-list");
    let _ = std::fs::create_dir_all(&root);
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
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
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
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
    unsafe { std::env::remove_var("GROKHUB_CONFIG") };
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

    let (title, body) = super::budget_ui::token_budget_notice(
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

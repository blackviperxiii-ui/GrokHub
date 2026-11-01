//! Composer, chips, attach and queue tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

#[test]
fn greeting_uses_the_native_fast_model() {
    let src = cabin_src();
    let native = include_str!("native_unattended.rs").replace("\r\n", "\n");
    let greet = fn_src(&src, "spawn_greeting_llm");
    assert!(greet.contains("spawn_native_greeting"), "{greet}");
    let fast = fn_src(&native, "fast_text");
    assert!(
        fast.contains("CABIN_FAST_MODEL") && fast.contains("CABIN_FAST_FALLBACK"),
        "the greeting uses the fast model and fall back if it is empty: {fast}"
    );
    for body in [greet, fast] {
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

/// Quick chips never ask a model, cloud or local (LO-28): the row is built
/// from the chat, habits and the PC's own state.
#[test]
fn no_chip_path_calls_a_model() {
    let src = cabin_src();
    for name in ["refresh_chips", "composer_chips", "apply_chip"] {
        let body = src
            .split(&format!("fn {name}("))
            .nth(1)
            .unwrap_or_else(|| panic!("missing fn {name}"));
        let body = &body[..body.find("\n    }\n").unwrap_or(body.len())];
        for call in [
            "fast_text",
            "fast_once",
            "unattended_model",
            "stream_routed",
            "spawn_native",
            "chip_suggest_prompt",
            "route::local",
        ] {
            assert!(!body.contains(call), "{name} must not call {call}: {body}");
        }
    }
    let native = include_str!("native_unattended.rs");
    for gone in [
        "fn spawn_chip_llm(",
        "fn spawn_native_chips(",
        "fn poll_chips(",
    ] {
        assert!(
            !src.contains(gone) && !native.contains(gone),
            "the model chip path is gone: {gone}"
        );
    }
}

#[test]
fn refresh_chips_does_not_rebuild_every_frame() {
    let src = cabin_src();
    let chips = src
        .split("fn refresh_chips(")
        .nth(1)
        .and_then(|s| s.split("fn composer_chips(").next())
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
        .and_then(|s| s.split("fn ui_sidebar(").next())
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
            super::ComposerStackSlot::ScreenshotOffer,
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
        perm.contains("persistable_permission_mode") && perm.contains("always_confirmed"),
        "Ask/Auto/Always persist; Always marks the one-time confirm: {perm}"
    );
    let arm = include_str!("confirm.rs");
    assert!(
        arm.contains("fn arm_session_always")
            && arm.contains("always_confirmed")
            && arm.contains("apply_session_always"),
        "Always confirm runs once, then sticks"
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
        .find("for slot in composer_stack_slots(")
        .or_else(|| src.find("for slot in composer_stack_order()"))
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
fn composer_pill_height_steps_per_line_and_stops_at_the_cap() {
    let min = crate::theme::QUERY_MIN_H;
    assert_eq!(super::composer_pill_h(0, 240.0), min);
    assert_eq!(super::composer_pill_h(1, 240.0), min);
    assert_eq!(super::composer_pill_h(3, 240.0), 3.0 * 22.0 + 28.0);
    assert_eq!(super::composer_pill_h(40, 240.0), 240.0);
    assert_eq!(super::composer_max_h(1080.0), 240.0);
    assert_eq!(super::composer_max_h(400.0), 160.0);
    assert_eq!(
        super::composer_max_h(50.0),
        min,
        "never shorter than one line"
    );
}

/// Pasting one long line used to keep the one-line pill (only Enter counted
/// as a row), so the wrapped text drew past the pill's bottom edge.
#[test]
fn long_wrapped_draft_grows_the_pill_then_caps_it() {
    let _paint = crate::theme::hold_paint_test();
    let pill_for = |text: String| {
        let mut cabin = quiet_cabin();
        let thread = crate::threads::ChatThread::new("Chat", false);
        cabin.threads = vec![thread];
        cabin.thread_idx = 0;
        cabin.composer = text;
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 760.0),
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
        cabin.composer_geom.expect("composer geom").0
    };
    let one = pill_for("hello".into());
    assert_eq!(one.height(), crate::theme::QUERY_MIN_H);
    let wrapped = pill_for("word ".repeat(80));
    assert!(
        !"word ".repeat(80).contains('\n') && wrapped.height() > one.height() + 40.0,
        "one long line must wrap into a taller pill: {} vs {}",
        wrapped.height(),
        one.height()
    );
    let huge = pill_for("word ".repeat(2000));
    assert_eq!(
        huge.height(),
        super::composer_max_h(760.0),
        "a giant draft stops at the cap and scrolls inside"
    );
}

// Landed from PR #89.
#[test]
fn memory_editor_fills_its_height_and_scrolls_past_it() {
    use super::pages::memory_editor_rows;
    // A 720 px window leaves the editor about 500 points: it shows ~35 rows of
    // 14 pt text however short the file is, and a longer file scrolls inside.
    assert_eq!(memory_editor_rows(500.0, 14.0), 34);
    // Tiny windows keep a usable editor; a bogus height falls back to 24 rows.
    assert_eq!(memory_editor_rows(40.0, 14.0), 8);
    assert_eq!(memory_editor_rows(f32::INFINITY, 14.0), 24);
    assert_eq!(memory_editor_rows(500.0, 0.0), 24);
}

#[test]
fn queue_todos_read_as_words_not_raw_state_keys() {
    use super::pages::todo_status_label;
    assert_eq!(todo_status_label("in_progress"), "In progress");
    assert_eq!(todo_status_label("pending"), "To do");
    assert_eq!(todo_status_label("completed"), "Done");
    assert_eq!(todo_status_label("cancelled"), "Cancelled");
    assert_eq!(todo_status_label("on_hold"), "On hold");
    assert_eq!(todo_status_label(""), "To do");
}

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
    assert_eq!(
        cabin.composer, "paint the harbor",
        "a signed-out send hands the typed text back to the composer"
    );
    assert!(
        cabin.signin_card,
        "a signed-out send raises the inline sign-in card"
    );

    // Text typed after the refusal is not overwritten by the earlier prompt.
    cabin.composer = "a newer draft".into();
    cabin.send_from_composer("paint the harbor".into());
    assert_eq!(cabin.composer, "a newer draft");
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

// Landed from PR #138.
#[test]
fn chip_nav_changes_page_and_dismiss_drops_it() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("chip-acts");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
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
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
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

// Landed from PR #141.
#[test]
fn session_pills_save_chat_and_ask() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("session-pills");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
    let mut cabin = Cabin::quiet_for_test();
    cabin.set_session_mode(SessionMode::Chat);
    assert_eq!(cabin.session_mode, SessionMode::Chat);
    assert_eq!(cabin.cfg.session_mode, "chat");
    cabin.set_session_mode(SessionMode::Ask);
    assert_eq!(cabin.session_mode, SessionMode::Ask);
    assert_eq!(cabin.cfg.session_mode, "ask");
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
                Some(v) => unsafe { std::env::set_var("GROKHUB_CONFIG", v) },
                None => unsafe { std::env::remove_var("GROKHUB_CONFIG") },
            }
            match self.grok.take() {
                Some(v) => unsafe { std::env::set_var("GROKHUB_GROK", v) },
                None => unsafe { std::env::remove_var("GROKHUB_GROK") },
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
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
    unsafe { std::env::set_var("GROKHUB_GROK", "/no/such/grok-binary-xyz") };

    let mut mode = super::Cabin::quiet_for_test();
    mode.apply_chip(chip("mode-fast", "__mode:fast", super::ChipKind::Mode));
    assert_eq!(mode.cfg.mode, "fast");
    assert_eq!(
        mode.status,
        super::slash::mode_status_line("fast", &mode.cfg.model)
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

// Landed from PR #152.
#[test]
fn followup_queue_drains_one_and_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("followup-queue");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
    unsafe { std::env::set_var("GROKHUB_GROK", root.join("missing-grok")) };

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

    unsafe { std::env::remove_var("GROKHUB_GROK") };
    unsafe { std::env::remove_var("GROKHUB_CONFIG") };
}

// Landed from PR #156.
#[test]
fn shell_chip_stays_off_a_run_when_host_is_blocked() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("shell-chip");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };

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

// Landed from PR #158.
#[test]
fn composer_action_chips_stay_off_a_run() {
    let _hold = config::hold_test_config();
    let root = config::test_config_root("composer-action-chips");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };

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

#[test]
fn empty_queue_keeps_nothing_queued_label() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("empty-queue");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };

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

    unsafe { std::env::remove_var("GROKHUB_CONFIG") };
}

// Landed from PR #174.
#[test]
fn leaving_a_chat_clears_attach_and_asks() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("leave-chrome");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("isolated config root");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };

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
        options: Vec::new(),
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

    unsafe { std::env::remove_var("GROKHUB_CONFIG") };
}

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

/// LO-27: only the chip that started a turn is credited with how it went,
/// and a typed ask teaches its kind of ask, never its words.
#[test]
fn a_chip_turn_is_credited_to_its_chip_and_a_typed_turn_to_none() {
    let mut cabin = Cabin::quiet_for_test();
    let fix = grokhub_core::QuickChip {
        id: "seed-stutter".into(),
        label: "Fix my stutter".into(),
        value: "Fix my stutter".into(),
        kind: grokhub_core::ChipKind::Chat,
        score: 1.0,
        hint: String::new(),
        primary: false,
    };
    cabin.apply_chip(fix.clone());
    assert_eq!(cabin.chip_send_key, None, "the send window closes");
    assert!(
        cabin.chip_memory.intents.contains_key("game.perf"),
        "the click teaches its kind of ask"
    );
    let typed_before = cabin.chip_memory.intents.get("game.perf").map(|h| h.uses);

    let src = include_str!("chat_kick.rs");
    assert!(
        src.contains("self.chip_turn_key = self.chip_send_key.take();")
            && src.contains("if self.chip_turn_key.is_none() {"),
        "a typed turn has no chip and learns only its kind"
    );
    for (file, body) in [
        ("acp.rs", include_str!("acp.rs")),
        ("jobs.rs", include_str!("jobs.rs")),
    ] {
        assert!(
            !body.contains("remember_chip_outcome(")
                && body.contains("if let Some(key) = self.chip_turn_key.take()"),
            "{file} credits only the chip that started the turn"
        );
    }
    assert_eq!(typed_before, Some(1));
}

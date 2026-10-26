//! Live turn, stream and transcript cache tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

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

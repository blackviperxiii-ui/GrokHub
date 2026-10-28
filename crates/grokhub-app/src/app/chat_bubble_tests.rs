//! Chat bubbles, thought fold and chat rows tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

#[test]
fn short_assistant_bubble_hugs_the_text() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 200.0), |ui| {
            ui.set_max_width(800.0);
            let resp = super::paint_speech_bubble(ui, "Hi", false, false);
            assert!(
                resp.rect.width() < 200.0,
                "short assistant bubble stretched to {}",
                resp.rect.width()
            );
            assert!(resp.rect.width() > 24.0);
        });
    });
}

#[test]
fn short_user_bubble_hugs_the_text() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 200.0), |ui| {
            ui.set_max_width(800.0);
            let resp = super::paint_speech_bubble(ui, "Hi", true, false);
            assert!(
                resp.rect.width() < 200.0,
                "short bubble stretched to {}",
                resp.rect.width()
            );
            assert!(resp.rect.width() > 24.0);
            assert!(resp.rect.height() > 20.0);
        });
    });
}

fn with_cabin_theme_ui(mut add: impl FnMut(&mut egui::Ui)) {
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        let ctx = ui.ctx().clone();
        crate::theme::apply(&ctx, true);
        egui::CentralPanel::default().show(ui, |ui| add(ui));
    });
}

#[test]
fn short_user_reply_stays_inside_the_viewport() {
    // Cabin button padding makes Copy+Reply wider than a "hey" bubble.
    // The row used to floor the lead at 96px and draw Reply past the pane.
    for width in [360.0_f32, 480.0, 800.0, 1100.0] {
        with_cabin_theme_ui(|ui| {
            ui.allocate_ui(egui::vec2(width, 280.0), |ui| {
                ui.set_max_width(width);
                let row = ui.max_rect();
                assert!(
                    (row.width() - width).abs() < 1.0,
                    "harness row {} != requested {width}",
                    row.width()
                );
                let bubble = super::paint_speech_bubble(ui, "hey", true, false);
                let acts = super::paint_msg_acts(ui, true, "hey", row.width(), bubble.rect.width());
                assert!(
                    bubble.rect.max.x <= row.max.x + 1.0,
                    "width {width}: bubble past the right {} > {}",
                    bubble.rect.max.x,
                    row.max.x
                );
                assert!(
                    bubble.rect.min.x >= row.min.x - 0.5,
                    "width {width}: bubble past the left {} < {}",
                    bubble.rect.min.x,
                    row.min.x
                );
                assert!(
                    bubble.rect.min.y >= row.min.y - 0.5,
                    "width {width}: bubble past the top {} < {}",
                    bubble.rect.min.y,
                    row.min.y
                );
                assert!(
                    acts.row.width() > 40.0 && acts.row.height() > 8.0,
                    "width {width}: Copy/Reply row missing {:?}",
                    acts.row
                );
                assert!(
                    acts.row.max.x <= row.max.x + 1.0,
                    "width {width}: Reply past the right {} > {} (row w {})",
                    acts.row.max.x,
                    row.max.x,
                    acts.row.width()
                );
                assert!(
                    acts.row.min.x >= row.min.x - 0.5,
                    "width {width}: Copy past the left {} < {}",
                    acts.row.min.x,
                    row.min.x
                );
                assert!(
                    acts.row.min.y >= row.min.y - 0.5,
                    "width {width}: action row past the top {} < {}",
                    acts.row.min.y,
                    row.min.y
                );
                assert!(
                    acts.row.max.x <= bubble.rect.max.x + 1.0,
                    "width {width}: Reply extends past the bubble {} > {}",
                    acts.row.max.x,
                    bubble.rect.max.x
                );
                assert!(
                    (bubble.rect.max.x - row.max.x).abs() < 8.0,
                    "width {width}: short user bubble left the right edge, bubble {} row {}",
                    bubble.rect.max.x,
                    row.max.x
                );
            });
        });
    }
    with_cabin_theme_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 420.0), |ui| {
            ui.set_max_width(800.0);
            let row = ui.max_rect();
            let body = "word ".repeat(48);
            let bubble = super::paint_speech_bubble(ui, &body, true, false);
            let acts = super::paint_msg_acts(ui, true, &body, row.width(), bubble.rect.width());
            assert!(
                bubble.rect.height() > 36.0,
                "long user text must still wrap, height {}",
                bubble.rect.height()
            );
            assert!(
                bubble.rect.max.x <= row.max.x + 1.0 && acts.row.max.x <= row.max.x + 1.0,
                "long user row left the pane bubble {} acts {} row {}",
                bubble.rect.max.x,
                acts.row.max.x,
                row.max.x
            );
            assert!(
                (acts.row.min.x - bubble.rect.min.x).abs() < 4.0,
                "wide bubble actions must stay under the bubble, acts {} bubble {}",
                acts.row.min.x,
                bubble.rect.min.x
            );
        });
    });
}

fn row_ends_with_short_fragment(row: &str, words: &[&str]) -> Option<String> {
    let last = row.split_whitespace().last().unwrap_or("");
    let letters = last.chars().count();
    if letters == 0 || letters > 2 {
        return None;
    }
    if words.contains(&last) {
        return None;
    }
    let fragment = words.iter().any(|word| {
        word.chars().count() > letters && (word.starts_with(last) || word.ends_with(last))
    });
    if fragment {
        Some(last.to_owned())
    } else {
        None
    }
}

#[test]
fn ordinary_words_stay_whole_inside_a_narrow_bubble() {
    let body = "harbor light stays on the dock tonight";
    let words: Vec<&str> = body.split_whitespace().collect();
    with_fonts_ui(|ui| {
        let wrap = grokhub_core::bubble_wrap_width(280.0, grokhub_core::BUBBLE_PAD_X);
        let job = crate::markdown::wrapped_job(ui, body, wrap, egui::Color32::WHITE);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let rows: Vec<String> = galley.rows.iter().map(|row| row.text()).collect();
        for row in &rows {
            assert!(
                    row_ends_with_short_fragment(row, &words).is_none(),
                    "galley row ends with a 1- or 2-letter fragment of a longer word: {row:?} in {rows:?}"
                );
        }
        for word in &words {
            assert!(
                rows.iter().any(|row| row.contains(word)),
                "word {word} was split across galley rows: {rows:?}"
            );
        }
        ui.allocate_ui(egui::vec2(280.0, 420.0), |ui| {
            ui.set_max_width(280.0);
            let row = ui.max_rect();
            let user = super::paint_speech_bubble(ui, body, true, false);
            let assistant = super::paint_speech_bubble(ui, body, false, true);
            assert!(
                user.rect.max.x <= row.max.x + 1.0 && user.rect.height() > 16.0,
                "user bubble {} x {} h {}",
                user.rect.max.x,
                row.max.x,
                user.rect.height()
            );
            assert!(
                assistant.rect.max.x <= row.max.x + 1.0 && assistant.rect.height() > 16.0,
                "assistant bubble {} x {} h {}",
                assistant.rect.max.x,
                row.max.x,
                assistant.rect.height()
            );
        });
    });
}

#[test]
fn short_user_bubble_sits_on_the_right() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 200.0), |ui| {
            ui.set_max_width(800.0);
            let row = ui.max_rect();
            let resp = super::paint_speech_bubble(ui, "Hi", true, false);
            assert!(
                resp.rect.width() < 200.0,
                "short bubble stretched to {}",
                resp.rect.width()
            );
            assert!(
                (row.max.x - resp.rect.max.x).abs() < 8.0,
                "user bubble max.x {} not near row max.x {}",
                resp.rect.max.x,
                row.max.x
            );
        });
    });
}

#[test]
fn assistant_bubble_sits_flush_and_keeps_inner_pad() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 400.0), |ui| {
                ui.set_max_width(800.0);
                let row = ui.max_rect();
                let body = "The service page listing a Cummins QSX15 https://example.com/emea/documents/Service/02_Generators/PowerSource%20manual.pdf and a 500-hour kit.";
                let resp = super::paint_speech_bubble(ui, body, false, true);
                let lead = resp.rect.min.x - row.min.x;
                assert!(
                    lead < 40.0,
                    "assistant bubble shoved right of the pane: lead {lead} row {} bubble {}",
                    row.min.x,
                    resp.rect.min.x
                );
                assert!(
                    resp.rect.min.x + 0.5 >= row.min.x,
                    "assistant bubble clipped off the left: {} < {}",
                    resp.rect.min.x,
                    row.min.x
                );
                assert!(
                    resp.rect.max.x <= row.max.x + 1.0,
                    "assistant bubble overflowed the right: {} > {}",
                    resp.rect.max.x,
                    row.max.x
                );
                assert!(
                    resp.rect.width() > 400.0,
                    "long assistant reply must use the pane, got {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.height() > 36.0,
                    "long URL must wrap inside the bubble, height {}",
                    resp.rect.height()
                );
            });
    });
    let speech = include_str!("chat_ui.rs")
        .split("fn paint_speech_bubble(")
        .nth(1)
        .and_then(|s| s.split("fn paint_msg_acts(").next())
        .expect("paint_speech_bubble");
    assert!(
        speech.contains("inner_margin(egui::Margin::ZERO)")
            && speech.contains("add_space(BUBBLE_PAD_Y)")
            && speech.contains("add_space(BUBBLE_PAD_X)")
            && (speech.contains("allocate_exact_size(egui::vec2(REPLY_MARK_W, REPLY_MARK_W)")
                || speech.contains("paint_companion_face(ui, REPLY_MARK_W)"))
            && include_str!("chat_ui.rs").contains("const REPLY_MARK_W: f32 = 28.0;"),
        "bubble pad must sit inside the fill, not get clipped by rounded inner_margin: {speech}"
    );
}

#[test]
fn long_assistant_bubble_wraps_instead_of_one_line() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 400.0), |ui| {
            ui.set_max_width(800.0);
            let body = "word ".repeat(80);
            let resp = super::paint_speech_bubble(ui, &body, false, true);
            assert!(
                resp.rect.width() <= grokhub_core::bubble_max_width(800.0) + 8.0,
                "bubble {}",
                resp.rect.width()
            );
            assert!(
                resp.rect.width() > 500.0,
                "an 800px pane must not use a 440px column, got {}",
                resp.rect.width()
            );
            assert!(
                resp.rect.height() > 48.0,
                "wrapped bubble height {}",
                resp.rect.height()
            );
        });
    });
}

#[test]
fn long_thought_wraps_instead_of_truncating() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 500.0), |ui| {
            ui.set_max_width(800.0);
            let body = "word ".repeat(80);
            let resp = super::paint_thought_bubble(ui, &body);
            assert!(
                resp.rect.width() <= 800.0 + 8.0,
                "thought spilled the pane: {}",
                resp.rect.width()
            );
            assert!(
                resp.rect.height() > 48.0,
                "thought stayed one clipped line, height {}",
                resp.rect.height()
            );
        });
    });
    let src = cabin_src();
    let thought = src
        .split("ChatKind::Thought => {")
        .nth(1)
        .and_then(|s| s.split("ChatKind::Tool => {").next())
        .expect("thought arm");
    assert!(
        thought.contains("paint_thought_bubble") || thought.contains("paint_speech_bubble"),
        "thoughts must wrap through the speech bubble path: {thought}"
    );
    assert!(
        !thought.contains("if open"),
        "thought body must stay visible after the turn, not collapse to a badge: {thought}"
    );
    assert!(
        thought.contains("paints_body"),
        "expand, minimize, and hide stay on the existing thought arm: {thought}"
    );
    let impl_src = src.as_str();
    assert_eq!(
        impl_src.matches("ChatKind::Thought => {").count(),
        1,
        "one thought renderer"
    );
}

#[test]
fn thought_body_stays_visible_when_idle() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 400.0), |ui| {
                ui.set_max_width(800.0);
                let block = grokhub_core::ChatView {
                    kind: grokhub_core::ChatKind::Thought,
                    title: "Thought".into(),
                    body: "I'll start by checking which desktop environment and session-restore setup you already have, then wire window size and position into that boot path. After that I'll confirm the restored geometry.".into(),
                };
                let closed = ui
                    .scope(|ui| {
                        let _ = super::paint_chat_block(
                            ui,
                            &block,
                            true,
                            false,
                            grokhub_core::ThoughtFold::Expanded,
                        );
                    })
                    .response;
                assert!(
                    closed.rect.height() > 36.0,
                    "idle thought hid the body, height {}",
                    closed.rect.height()
                );
            });
    });
}

#[test]
fn thought_fold_paints_expand_minimize_and_hide() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 700.0), |ui| {
                ui.set_max_width(800.0);
                let thought = grokhub_core::ChatView {
                    kind: grokhub_core::ChatKind::Thought,
                    title: "Thought".into(),
                    body: "I'll start by checking which desktop environment and session-restore setup you already have, then wire window size and position into that boot path. After that I'll confirm the restored geometry and say what changed.".into(),
                };
                let reply = grokhub_core::ChatView {
                    kind: grokhub_core::ChatKind::Assistant,
                    title: String::new(),
                    body: "Window size and position restore through the session you already have.".into(),
                };
                let height = |ui: &mut egui::Ui, block: &grokhub_core::ChatView, fold| {
                    let y0 = ui.cursor().min.y;
                    let _ = super::paint_chat_block(ui, block, true, false, fold);
                    ui.cursor().min.y - y0
                };
                let expanded = height(ui, &thought, grokhub_core::ThoughtFold::Expanded);
                let minimized = height(ui, &thought, grokhub_core::ThoughtFold::Minimized);
                let hidden = height(ui, &thought, grokhub_core::ThoughtFold::Hidden);
                let reply_hidden = height(ui, &reply, grokhub_core::ThoughtFold::Hidden);
                let reply_open = height(ui, &reply, grokhub_core::ThoughtFold::Expanded);
                assert!(
                    expanded > 48.0,
                    "expanded thought must show the body, height {expanded}"
                );
                assert!(
                    minimized + 24.0 < expanded,
                    "minimized thought must be one short row, minimized {minimized} expanded {expanded}"
                );
                assert!(
                    minimized < 56.0,
                    "minimized thought grew past one row, height {minimized}"
                );
                assert!(
                    hidden < 4.0,
                    "hidden thought was still drawn, height {hidden}"
                );
                assert!(
                    (reply_hidden - reply_open).abs() < 1.0 && reply_open > 20.0,
                    "hiding a thought must not hide the reply: hidden-fold {reply_hidden} expanded-fold {reply_open}"
                );
            });
    });
}

#[test]
fn thought_fold_handoff_keeps_minimize_on_the_stored_body() {
    let ctx = egui::Context::default();
    let mut live = Vec::new();
    grokhub_core::append_thought(&mut live, "Need a snapshot");
    let slot = live[0].fold_slot;
    grokhub_core::append_thought(&mut live, " of the restore path.");
    assert_eq!(slot, live[0].fold_slot);
    let thread = "thread-a";
    super::write_thought_fold(
        &ctx,
        super::thought_fold_id(thread, "slot", live[0].fold_slot),
        grokhub_core::ThoughtFold::Minimized,
    );
    super::write_thought_fold(
        &ctx,
        super::thought_fold_id(
            thread,
            "body",
            grokhub_core::thought_body_key(&live[0].body),
        ),
        grokhub_core::ThoughtFold::Minimized,
    );
    let views = grokhub_core::visible_chat(&[
        ("user".into(), "check the session".into()),
        (
            "assistant".into(),
            grokhub_core::merge_thinking(&live[0].body, "I'll look at the session."),
        ),
    ]);
    let stored = views
        .iter()
        .find(|v| v.kind == grokhub_core::ChatKind::Thought)
        .expect("stored thought");
    let got = super::read_thought_fold(
        &ctx,
        super::thought_fold_id(thread, "body", grokhub_core::thought_body_key(&stored.body)),
    );
    assert_eq!(got, grokhub_core::ThoughtFold::Minimized);
    let fresh = super::read_thought_fold(
        &ctx,
        super::thought_fold_id(
            thread,
            "body",
            grokhub_core::thought_body_key("a brand new thought"),
        ),
    );
    assert_eq!(fresh, grokhub_core::ThoughtFold::Expanded);
    let reply = views
        .iter()
        .find(|v| v.kind == grokhub_core::ChatKind::Assistant)
        .expect("reply");
    assert!(grokhub_core::thought_fold_draws(
        reply.kind,
        grokhub_core::ThoughtFold::Minimized
    ));
    let src = cabin_src();
    let live = src
        .split("fn paint_live_blocks(")
        .nth(1)
        .and_then(|s| s.split("fn paint_tool_cards(").next())
        .expect("paint_live_blocks");
    assert!(
        live.contains("fold_slot") && live.contains("thought_body_key"),
        "live folds must be copied onto the stored body key: {live}"
    );
}

#[test]
fn long_sentence_stays_inside_the_pane_on_a_wide_row() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(1600.0, 500.0), |ui| {
                ui.set_max_width(1600.0);
                let body = "the clam gods? oh you know... ancient, briny, and extremely picky about their cream-to-broth ratio. they live in the black void between chowder pots, only emerging when someone dares to say manhattan style in their presence. knock twice and offer a saltine or they won't even open up.";
                let resp = super::paint_speech_bubble(ui, body, false, true);
                assert!(
                    resp.rect.width() <= grokhub_core::bubble_max_width(1600.0) + 8.0,
                    "wide pane stretched the bubble to {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.width() > 500.0,
                    "a wide pane must not squeeze the reply, got {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.height() > 28.0,
                    "long sentence must wrap, height {}",
                    resp.rect.height()
                );
            });
    });
}

#[test]
fn markdown_reply_grows_past_plain_measure() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 900.0), |ui| {
                ui.set_max_width(800.0);
                let body = "# Title\n## Heading\n\n- bullet one\n- bullet two\n\n```sh\nls -la\n```\n\nClosing line.";
                let wrap = grokhub_core::bubble_wrap_width(800.0, grokhub_core::BUBBLE_PAD_X);
                let measured = crate::markdown::measure_text(ui, body, wrap);
                let mut md_h = 0.0;
                ui.allocate_ui(egui::vec2(wrap, 800.0), |ui| {
                    let r = ui
                        .scope(|ui| {
                            ui.set_max_width(wrap);
                            crate::markdown::show(ui, body);
                        })
                        .response;
                    md_h = r.rect.height();
                });
                let resp = super::paint_speech_bubble(ui, body, false, true);
                assert!(
                    md_h > measured.y + 2.0,
                    "fixture must be taller as markdown than plain measure: md {md_h} plain {}",
                    measured.y
                );
                assert!(
                    resp.rect.height() + 2.0 >= md_h,
                    "markdown bubble clipped: painted {} markdown {}",
                    resp.rect.height(),
                    md_h
                );
            });
    });
}

#[test]
fn long_user_bubble_stays_inside_the_row() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(480.0, 400.0), |ui| {
            ui.set_max_width(480.0);
            let row = ui.max_rect();
            let body =
                "/very/long/path/to/grokhub/lib/systemd/status\" 2>/dev/null && echo ok ".repeat(6);
            let resp = super::paint_speech_bubble(ui, &body, true, false);
            assert!(
                resp.rect.min.x + 0.5 >= row.min.x,
                "user bubble clipped off the left: {} < {}",
                resp.rect.min.x,
                row.min.x
            );
            assert!(
                resp.rect.max.x <= row.max.x + 1.0,
                "user bubble overflowed the right: {} > {}",
                resp.rect.max.x,
                row.max.x
            );
            assert!(
                resp.rect.width() <= grokhub_core::bubble_max_width(480.0) + 8.0,
                "bubble {}",
                resp.rect.width()
            );
        });
    });
}

#[test]
fn unbroken_user_text_stays_inside_narrow_and_wide_rows() {
    let body = "a".repeat(480);
    // 140 shrinks to leave item_spacing. 800 is a typical pane: the 84% cap
    // already fits, and the leading gap must still be there.
    for width in [140.0_f32, 280.0, 480.0, 800.0, 1600.0] {
        with_fonts_ui(|ui| {
            ui.allocate_ui(egui::vec2(width, 800.0), |ui| {
                ui.set_max_width(width);
                let row = ui.max_rect();
                let gap = ui.spacing().item_spacing.x.max(0.0);
                assert!(
                    (row.width() - width).abs() < 1.0,
                    "harness row {} != requested {width}",
                    row.width()
                );
                let wrap = grokhub_core::bubble_wrap_width(width, grokhub_core::BUBBLE_PAD_X);
                let measured = crate::markdown::measure_text(ui, &body, wrap);
                assert!(
                    measured.x <= wrap + 1.0,
                    "width {width}: unwrapped measure {} > wrap {wrap} height {}",
                    measured.x,
                    measured.y
                );
                let resp = super::paint_speech_bubble(ui, &body, true, false);
                let lead = resp.rect.min.x - row.min.x;
                assert!(
                    resp.rect.min.x + 0.5 >= row.min.x,
                    "width {width}: bubble left {} < row {}",
                    resp.rect.min.x,
                    row.min.x
                );
                assert!(
                    lead + 1.0 >= gap,
                    "width {width}: leading gap {gap} not reserved, lead {lead}"
                );
                assert!(
                    resp.rect.max.x <= row.max.x + 1.0,
                    "width {width}: user bubble ran past the row {} > {} (bubble w {})",
                    resp.rect.max.x,
                    row.max.x,
                    resp.rect.width()
                );
                assert!(
                    row.max.x - resp.rect.max.x <= 1.0,
                    "width {width}: stray hole on the right, bubble max {} row {}",
                    resp.rect.max.x,
                    row.max.x
                );
                assert!(
                    resp.rect.width() <= width + 1.0,
                    "width {width}: bubble wider than the row {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.height() > 36.0,
                    "width {width}: unbroken text stayed one line, height {}",
                    resp.rect.height()
                );
            });
        });
    }
}

#[test]
fn failed_turns_are_marked_as_errors() {
    assert!(super::is_error_reply("Error: Ask is fail-closed"));
    assert!(super::is_error_reply("  Error: 502 upstream"));
    assert!(!super::is_error_reply("The error was in main.rs"));
    assert!(!super::is_error_reply("Errors: none"));
}

#[test]
fn chat_blocks_offer_copy_and_reply() {
    let src = cabin_src();
    let start = src.find("fn paint_msg_acts").expect("paint_msg_acts");
    let slice = &src[start..start + 2800];
    assert!(slice.contains("Copy"), "{slice}");
    assert!(slice.contains("Reply"), "{slice}");
    assert!(src.contains("fn paint_chat_block"), "{src}");
    assert!(src.contains("ChatBlockAct::Copy"));
    assert!(src.contains("ChatBlockAct::Reply"));
    assert!(src.contains("quote_for_reply"));
    assert!(src.contains("composer_want_focus"));
    assert!(src.contains("copy_text"));
    let block = fn_src(&src, "paint_chat_block");
    assert!(
        !block.contains("resp.hovered()"),
        "Copy/Reply must stay visible when the pointer leaves the bubble: {block}"
    );
    assert!(
        src.contains("selectable(true)"),
        "chat bubble text must be selectable for copy: {}",
        src[src.find("fn paint_speech_bubble").unwrap_or(0)..]
            .get(..400)
            .unwrap_or("")
    );
    let bubble = src.find("fn paint_speech_bubble").expect("speech bubble");
    let bubble_fn = &src[bubble..bubble + 1800];
    assert!(
        !bubble_fn.contains("vec2(row_w, 0.0)"),
        "a zero-height row clips the thread: {bubble_fn}"
    );
    assert!(
        !bubble_fn.contains("set_clip_rect"),
        "clip_rect on the row hides wrapped text: {bubble_fn}"
    );
    assert!(
        !bubble_fn.contains("right_to_left"),
        "RTL user rows clip long lines off the left: {bubble_fn}"
    );
    assert!(
        !bubble_fn.contains("row_h"),
        "a measured-height lock clips markdown: {bubble_fn}"
    );
    let assistant = src
        .split("// Assistant row: the companion face sits at the top left")
        .nth(1)
        .and_then(|s| s.split("fn paint_msg_acts").next())
        .expect("assistant bubble");
    assert!(
        assistant.contains("horizontal_top"),
        "assistant face must sit at the top of multi-line replies: {assistant}"
    );
    assert!(
        !assistant.contains("ui.horizontal("),
        "ui.horizontal centers the mark mid-block: {assistant}"
    );
    let speech = src
        .split("fn paint_speech_bubble(")
        .nth(1)
        .and_then(|s| s.split("fn paint_msg_acts(").next())
        .expect("paint_speech_bubble");
    assert!(
        speech.contains("bubble_assistant()") && speech.contains("USER_BUBBLE_RADIUS"),
        "assistant replies must sit in a bubble: {speech}"
    );
    assert!(
        speech.contains("signal_red()") && speech.contains("is_loadout_paint()"),
        "Hangout assistant bubbles take a Signal Red outline under Loadout: {speech}"
    );
    let hangout = src
        .split("fn paint_hangout_header(")
        .nth(1)
        .and_then(|s| s.split("fn ui_chat(").next())
        .expect("paint_hangout_header");
    assert!(
        hangout.contains("Hangout")
            && hangout.contains("TALK")
            && hangout.contains("TYPE")
            && hangout.contains("listen_voice")
            && hangout.contains("leave_voice"),
        "Hangout header wires TALK/TYPE to voice: {hangout}"
    );
    let chat = src
        .split("fn ui_chat(")
        .nth(1)
        .and_then(|s| s.split("fn ui_empty_home(").next())
        .expect("ui_chat");
    assert!(
        chat.contains("paint_hangout_header"),
        "ui_chat paints the Hangout header: {chat}"
    );
    assert!(
        chat.contains("available_width") && chat.contains("set_max_width(pane)"),
        "thread uses the CentralPanel pane: {chat}"
    );
    assert!(
        !chat.contains("chat_col_w") && !chat.contains("empty_home_side_gap"),
        "conversation must use the full chat pane, not a centered Grok column: {chat}"
    );
    assert!(
        !chat.contains("composer_pill_w"),
        "bubbles must not lock to the composer pill: {chat}"
    );
    assert!(
        chat.contains("cached_chat_views") && !chat.contains("visible_chat(&pairs)"),
        "idle chat must not clone the whole transcript every paint: {chat}"
    );
    assert!(
        chat.contains("paint_live_blocks") && chat.contains("views_up_to_last_user"),
        "tools must sit in the live turn, not always under the last bubble: {chat}"
    );
    assert!(
            !chat.contains("paint_running")
                && !chat.contains("chat_run_label")
                && !chat.contains("background_pill"),
            "2.10.87: no Thinking dot or Background button in the transcript; the composer glow shows a live turn: {chat}"
        );
    assert!(
        !chat.contains("run_slash(Slash::Stop)"),
        "the transcript running row must not paint a Stop: {chat}"
    );
    assert!(
        chat.contains("cluster_gap"),
        "consecutive thoughts must cluster tighter than chat: {chat}"
    );
    assert!(
        chat.contains("chat_row_height_id")
            && chat.contains("push_id(chat_row_id_salt")
            && chat.contains("skip_ahead_auto_ids(1)"),
        "offscreen rows must key height by pane width and keep a stable row id: {chat}"
    );
    let running = src
        .split("fn paint_running(")
        .nth(1)
        .and_then(|s| s.split("fn paint_one_tool_card(").next())
        .expect("paint_running");
    assert!(
        running.contains("paint_run_pulse") && !running.contains("Stop"),
        "transcript running chrome is the labeled live pulse without a Stop: {running}"
    );
    assert!(
        !running.contains("vec2(2.0, 16.0)"),
        "a blinking caret is not the in-progress indicator: {running}"
    );
    let attach = src
        .split("fn ui_attach_chip(")
        .nth(1)
        .and_then(|s| s.split("fn work_root(").next())
        .expect("ui_attach_chip");
    assert!(
            !attach.contains("paint_run_pulse") && !attach.contains("thinking_here"),
            "the running line above the composer is gone, including when the pane is scrolled: {attach}"
        );
    assert!(
        !attach.contains("clip_status") && !attach.contains("self.status"),
        "the status line above the composer is gone: {attach}"
    );
    assert!(
        chat.contains("scroll_to_cursor") && chat.contains("chat_tail_frames"),
        "a chat opens on its newest message, not where the last one was left: {chat}"
    );
    assert!(
        chat.contains("scrolled_off_tail") && chat.contains("jump_to_latest"),
        "scrolled up, the pane owes you a way back down: {chat}"
    );
    assert!(
        chat.contains("chat-jump")
            && chat.contains("BarIcon::ArrowDown")
            && chat.contains("ChatJump::Latest")
            && !chat.contains("chat-jump-last-you")
            && !chat.contains("chat-jump-latest")
            && !chat.contains("ghost_pill(ui, \"Last you\")")
            && !chat.contains("white_pill(ui, \"Jump to latest\")"),
        "one down-arrow jump Area, no overlapping Last you / Jump to latest pills: {chat}"
    );
    let switch = src
        .split("fn apply_switch_thread(")
        .nth(1)
        .and_then(|s| s.split("fn stamp_current_access(").next())
        .expect("apply_switch_thread");
    assert!(
        switch.contains("pin_chat_tail"),
        "the pane keeps the offset of the chat you left unless the swap re-pins it: {switch}"
    );
    assert!(
        !switch.contains("stamp_current_access") && !switch.contains("accessed_ms"),
        "opening a History row is not activity and must not move it: {switch}"
    );
    let show = fn_src(&src, "poll_session_show");
    assert!(
        show.contains("pin_chat_tail"),
        "a Grok transcript that lands after the click must bring the pane with it: {show}"
    );
    let composer_send = src
        .split("fn send_from_composer(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .expect("send_from_composer");
    assert!(
        composer_send.contains("pin_chat_tail") && composer_send.contains("send_chat"),
        "your own message follows itself down: {composer_send}"
    );
    let night = src
        .split("fn fire_night(")
        .nth(1)
        .and_then(|s| s.split("fn tick_review(").next())
        .expect("fire_night");
    assert!(
        !night.contains("send_from_composer"),
        "a night job must not yank the pane out of what you were reading: {night}"
    );
}

#[test]
fn cached_chat_views_do_not_clone_the_thread_on_stream_delta() {
    let src = cabin_src();
    let cache = fn_src(&src, "cached_chat_views");
    assert!(
        !cache.contains("m.1.clone()") && !cache.contains("role.clone()"),
        "a stream delta must not clone every message to rebuild chat views: {cache}"
    );
    assert!(
            cache.contains("refresh_last_stretch") || cache.contains("visible_chat_refs"),
            "last-message growth must refresh the trailing stretch without a full transcript clone: {cache}"
        );
}

#[test]
fn chat_row_height_cache_misses_on_resize() {
    let ctx = egui::Context::default();
    let narrow = super::chat_row_height_id("thread-a", 720.0);
    ctx.data_mut(|d| d.insert_temp(narrow, vec![40.0_f32, 88.0]));
    let same_width: Option<Vec<f32>> =
        ctx.data(|d| d.get_temp(super::chat_row_height_id("thread-a", 720.2)));
    assert_eq!(same_width.unwrap(), vec![40.0, 88.0]);
    let wide: Option<Vec<f32>> =
        ctx.data(|d| d.get_temp(super::chat_row_height_id("thread-a", 1100.0)));
    assert!(
        wide.is_none(),
        "a wider pane must not reuse heights wrapped at 720"
    );
    let other: Option<Vec<f32>> =
        ctx.data(|d| d.get_temp(super::chat_row_height_id("thread-b", 720.0)));
    assert!(other.is_none(), "another thread must not share row heights");
}

#[test]
fn skipped_row_advances_by_cached_height_only() {
    with_fonts_ui(|ui| {
        let y0 = ui.cursor().min.y;
        assert!(!super::reserve_offscreen_chat_row(ui, 36.0));
        assert!(
            (ui.cursor().min.y - y0).abs() < 0.01,
            "an on-screen row must still be painted"
        );
        let clip = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(200.0, 40.0));
        ui.set_clip_rect(clip);
        ui.add_space(80.0);
        let y1 = ui.cursor().min.y;
        assert!(super::reserve_offscreen_chat_row(ui, 36.0));
        assert!(
            (ui.cursor().min.y - y1 - 36.0).abs() < 0.01,
            "skip must reserve the cached height and nothing more, delta {}",
            ui.cursor().min.y - y1
        );
    });
}

#[test]
fn culled_row_does_not_steal_the_next_rows_widget_id() {
    fn copy_id(skip_earlier: bool, use_stable_id: bool) -> egui::Id {
        let ctx = egui::Context::default();
        let mut id = egui::Id::NULL;
        let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                for i in 0..2 {
                    if skip_earlier && i == 0 {
                        ui.add_space(20.0);
                        if use_stable_id {
                            ui.skip_ahead_auto_ids(1);
                        }
                        continue;
                    }
                    let button = |ui: &mut egui::Ui| ui.button("Copy");
                    let resp = if use_stable_id {
                        ui.push_id(super::chat_row_id_salt("thr", i), button).inner
                    } else {
                        button(ui)
                    };
                    if i == 1 {
                        id = resp.id;
                    }
                }
            });
        });
        id
    }
    let stable_full = copy_id(false, true);
    let stable_skip = copy_id(true, true);
    assert_eq!(
        stable_full, stable_skip,
        "push_id by thread+index must survive a culled neighbor"
    );
    assert_ne!(
        copy_id(false, false),
        copy_id(true, false),
        "egui auto ids still shift when a row is skipped; the stable id is what holds them"
    );
}

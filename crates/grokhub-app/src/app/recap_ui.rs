//! Task recap card: when a reply that followed real work (tool rows in the
//! same turn) ends with a `Finished` block, the reply paints without it and
//! the block paints under it as its own amber-rimmed card. One line per task
//! with a mark, then what needs the person, where to undo, and a Show details
//! fold with the turn's tool steps. Plain chat replies never get a card.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use eframe::egui::{self, RichText};
use grokhub_core::findings::DO_ALL;
use grokhub_core::recap::{split_recap, Recap, RecapStatus};
use grokhub_core::turn_timeline::{decode_tool_rows, tool_status_failed, ToolRow};
use grokhub_core::{ChatKind, ChatView};

/// One reply's recap split, keyed by thread, row and the reply's text.
type RecapKey = (String, usize, u64);
type RecapHit = Option<(ChatView, Recap, Vec<ToolRow>)>;
thread_local! {
    static RECAPS: std::cell::RefCell<HashMap<RecapKey, RecapHit>> =
        std::cell::RefCell::new(HashMap::new());
}
const RECAPS_CAP: usize = 256;

/// [`recap_at`] for the painted transcript: the split and the decoded tool
/// rows are kept per `(thread, row, reply text)`, since every painted reply
/// asks every frame and the tool rows are JSON.
pub(super) fn recap_at_cached(
    thread_id: &str,
    shown: &[ChatView],
    i: usize,
) -> Option<(ChatView, Recap, Vec<ToolRow>)> {
    let block = shown.get(i).filter(|b| b.kind == ChatKind::Assistant)?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    block.body.hash(&mut h);
    let key = (thread_id.to_string(), i, h.finish());
    if let Some(hit) = RECAPS.with(|c| c.borrow().get(&key).cloned()) {
        return hit;
    }
    let out = recap_at(shown, i);
    RECAPS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= RECAPS_CAP {
            c.clear();
        }
        c.insert(key, out.clone());
    });
    out
}

/// For the assistant row `i`: the reply without its recap, the recap, and the
/// tool steps of its turn, when the turn did work and the reply ends with one.
pub(super) fn recap_at(shown: &[ChatView], i: usize) -> Option<(ChatView, Recap, Vec<ToolRow>)> {
    let block = shown.get(i).filter(|b| b.kind == ChatKind::Assistant)?;
    // Cheap checks first: this runs for every painted reply, every frame.
    let (body, recap) = split_recap(&block.body)?;
    let mut turn_start = shown[..i]
        .iter()
        .rposition(|v| v.kind == ChatKind::User)
        .map_or(0, |u| u + 1);
    // A Do all summary sums up the batch, so its steps are every fix turn's.
    // It stops at this batch's first fix, so an earlier batch never joins in.
    let summary = |u: usize| shown[u].body.starts_with(&format!("{DO_ALL} summary"));
    if turn_start > 0 && summary(turn_start - 1) {
        while let Some(u) = shown[..turn_start - 1]
            .iter()
            .rposition(|v| v.kind == ChatKind::User)
            .filter(|&u| shown[u].body.starts_with(&format!("{DO_ALL} ")) && !summary(u))
        {
            turn_start = u + 1;
            if shown[u].body.starts_with(&format!("{DO_ALL} 1/")) {
                break;
            }
        }
    }
    let steps: Vec<ToolRow> = shown[turn_start..i]
        .iter()
        .filter(|v| v.kind == ChatKind::Tool)
        .flat_map(|v| {
            decode_tool_rows(&v.body).unwrap_or_else(|| {
                vec![ToolRow {
                    status: String::new(),
                    title: v.title.clone(),
                    detail: String::new(),
                }]
            })
        })
        .collect();
    if steps.is_empty() {
        return None;
    }
    let reply = ChatView {
        body,
        ..block.clone()
    };
    Some((reply, recap, steps))
}

fn mark(status: RecapStatus) -> (&'static str, egui::Color32) {
    match status {
        RecapStatus::Done => ("✓", crate::theme::live()),
        RecapStatus::Partly => ("!", crate::theme::amber()),
        RecapStatus::NotDone => ("×", crate::theme::signal_red()),
    }
}

/// "2 of 3 done", for the card's header.
pub(super) fn recap_tally(recap: &Recap) -> String {
    let done = recap
        .lines
        .iter()
        .filter(|l| l.status == RecapStatus::Done)
        .count();
    format!("{done} of {} done", recap.lines.len())
}

fn line(ui: &mut egui::Ui, glyph: &str, color: egui::Color32, text: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(
            RichText::new(glyph)
                .size(crate::theme::TYPE_BODY)
                .strong()
                .color(color),
        );
        ui.label(
            RichText::new(text)
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::fg()),
        );
    });
}

pub(super) fn paint_recap_card(ui: &mut egui::Ui, recap: &Recap, steps: &[ToolRow]) {
    use crate::ui_kit::recipe;
    let look = recipe::look(recipe::Part::Card, recipe::State::Accent);
    let avail = grokhub_core::clamp_row_width(ui.available_width().min(ui.max_rect().width()));
    let w = crate::markdown::bubble_width(avail);
    ui.add_space(6.0);
    egui::Frame::NONE
        .fill(look.fill)
        .stroke(look.stroke)
        .corner_radius(look.radius)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(w - 24.0 - look.stroke.width * 2.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.label(
                    super::loadout_home::caption(grokhub_core::recap::RECAP_HEAD)
                        .strong()
                        .color(crate::theme::amber()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(recap_tally(recap))
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::muted()),
                    );
                });
            });
            ui.add_space(4.0);
            for l in &recap.lines {
                let (glyph, color) = mark(l.status);
                line(ui, glyph, color, &l.text);
            }
            for n in &recap.needs_you {
                line(ui, "→", crate::theme::amber(), &format!("Needs you: {n}"));
            }
            for u in &recap.undo {
                line(ui, "↺", crate::theme::muted(), &format!("Undo: {u}"));
            }
            ui.add_space(2.0);
            egui::CollapsingHeader::new(
                RichText::new("Show details")
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            )
            .id_salt("recap-details")
            .default_open(false)
            .show(ui, |ui| {
                for s in steps {
                    let glyph = if tool_status_failed(&s.status) {
                        "×"
                    } else {
                        "·"
                    };
                    ui.label(
                        RichText::new(format!("{glyph} {}", s.title))
                            .size(crate::theme::FONT_META)
                            .color(crate::theme::muted()),
                    );
                }
            });
        });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::turn_timeline::encode_tool_rows;

    fn view(kind: ChatKind, body: &str) -> ChatView {
        ChatView {
            kind,
            title: String::new(),
            body: body.to_string(),
        }
    }

    fn tools(rows: &[(&str, &str)]) -> ChatView {
        let rows: Vec<ToolRow> = rows
            .iter()
            .map(|(status, title)| ToolRow {
                status: status.to_string(),
                title: title.to_string(),
                detail: String::new(),
            })
            .collect();
        ChatView {
            kind: ChatKind::Tool,
            title: "Work".into(),
            body: encode_tool_rows(&rows),
        }
    }

    const REPLY: &str = "Both fixes ran.\n\nFinished\n\
        - Done: Chips scrubbed, 10 removed\n\
        - Partly done: Pulse noise tamed, i2c lines still logged\n\
        Needs you: Local AI still shows loaded, restart Loadout\n\
        Undo: Settings, Labs, Backups";

    fn titles(steps: &[ToolRow]) -> Vec<&str> {
        steps.iter().map(|s| s.title.as_str()).collect()
    }

    #[test]
    fn a_reply_after_work_splits_into_the_reply_and_its_recap() {
        let shown = vec![
            view(ChatKind::User, "scrub the chips and tame pulse"),
            tools(&[("completed", "Edit chips.json"), ("failed", "Read i2c log")]),
            view(ChatKind::Assistant, REPLY),
        ];
        let (reply, recap, steps) = recap_at(&shown, 2).expect("work happened");
        assert_eq!(reply.body, "Both fixes ran.");
        assert_eq!(reply.kind, ChatKind::Assistant);
        assert_eq!(recap.lines.len(), 2);
        assert_eq!(recap_tally(&recap), "1 of 2 done");
        assert_eq!(titles(&steps), ["Edit chips.json", "Read i2c log"]);
        // Not an assistant row, or no recap: nothing.
        assert!(recap_at(&shown, 0).is_none());
        assert!(recap_at(&shown, 1).is_none());
    }

    #[test]
    fn a_plain_reply_never_gets_a_card_even_with_a_finished_block() {
        let shown = vec![
            view(ChatKind::User, "what would you do?"),
            view(ChatKind::Assistant, REPLY),
            // Tools in an earlier turn don't count for this one.
            view(ChatKind::User, "and now?"),
            view(ChatKind::Assistant, REPLY),
        ];
        assert!(recap_at(&shown, 1).is_none());
        let shown = [
            vec![
                view(ChatKind::User, "fix it"),
                tools(&[("completed", "Edit a")]),
            ],
            shown,
        ]
        .concat();
        assert!(recap_at(&shown, 3).is_none());
        assert!(recap_at(&shown, 5).is_none());
    }

    #[test]
    fn the_do_all_summary_recaps_every_fix_turn() {
        let shown = vec![
            view(ChatKind::User, "scan my computer"),
            tools(&[("completed", "Run pacman -Qk")]),
            view(ChatKind::Assistant, "Found two things."),
            view(ChatKind::User, "Do all 1/2 · Fix port 53 conflict: Free port 53."),
            tools(&[("completed", "Edit resolved.conf")]),
            view(ChatKind::Assistant, "Port 53 is free."),
            view(ChatKind::User, "Do all 2/2 · Reinstall 6 packages: Reinstall them."),
            tools(&[("failed", "Run pacman -S bind")]),
            view(ChatKind::Assistant, "The mirror timed out."),
            view(ChatKind::User, "Do all summary · End with the Finished recap"),
            view(
                ChatKind::Assistant,
                "Finished\n- Done: Port 53 freed for nextdns\n- Not done: Reinstall, mirror timed out",
            ),
        ];
        let (reply, recap, steps) = recap_at(&shown, 10).expect("the batch did work");
        assert_eq!(reply.body, "");
        assert_eq!(recap_tally(&recap), "1 of 2 done");
        assert_eq!(titles(&steps), ["Edit resolved.conf", "Run pacman -S bind"]);
    }

    #[test]
    fn a_second_do_all_batch_recaps_only_its_own_fixes() {
        let batch = |n: &str, tool: &str| {
            vec![
                view(ChatKind::User, &format!("Do all 1/1 · Fix {n}: Fix it.")),
                tools(&[("completed", tool)]),
                view(ChatKind::Assistant, "Fixed."),
                view(
                    ChatKind::User,
                    "Do all summary · End with the Finished recap",
                ),
                view(ChatKind::Assistant, "Finished\n- Done: Fixed"),
            ]
        };
        let shown = [
            batch("disk", "Clean cache"),
            batch("port", "Edit resolved.conf"),
        ]
        .concat();
        let (_, _, steps) = recap_at(&shown, 9).expect("the second batch did work");
        assert_eq!(titles(&steps), ["Edit resolved.conf"]);
        let (_, _, steps) = recap_at(&shown, 4).expect("the first batch did work");
        assert_eq!(titles(&steps), ["Clean cache"]);
    }

    fn painted(recap: &Recap, steps: &[ToolRow]) -> (Vec<String>, Vec<egui::Stroke>) {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 720.0),
            )),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, raw, |ui| paint_recap_card(ui, recap, steps));
        let mut texts = Vec::new();
        let mut rims = Vec::new();
        for c in &out.shapes {
            match &c.shape {
                egui::Shape::Text(t) => texts.push(t.galley.job.text.clone()),
                egui::Shape::Rect(r) => rims.push(r.stroke),
                _ => {}
            }
        }
        (texts, rims)
    }

    #[test]
    fn the_card_marks_each_line_and_folds_the_steps() {
        let (_, recap) = split_recap(REPLY).unwrap();
        let steps = [ToolRow {
            status: "completed".into(),
            title: "Edit chips.json".into(),
            detail: String::new(),
        }];
        let (texts, rims) = painted(&recap, &steps);
        assert_eq!(
            texts,
            [
                "FINISHED",
                "1 of 2 done",
                "✓",
                "Chips scrubbed, 10 removed",
                "!",
                "Pulse noise tamed, i2c lines still logged",
                "→",
                "Needs you: Local AI still shows loaded, restart Loadout",
                "↺",
                "Undo: Settings, Labs, Backups",
                "Show details",
            ]
        );
        // Its own amber rim, so it doesn't read as another chat bubble.
        let amber = crate::ui_kit::recipe::look(
            crate::ui_kit::recipe::Part::Card,
            crate::ui_kit::recipe::State::Accent,
        )
        .stroke;
        assert!(rims.contains(&amber), "{rims:?}");
        // Every mark is in the bundled fonts, not a missing-glyph box.
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let _ = crate::theme::test_pass(&ctx, Default::default(), |_| {});
        let font = egui::FontId::proportional(crate::theme::TYPE_BODY);
        for c in "✓!×→↺·".chars() {
            assert!(ctx.fonts_mut(|f| f.has_glyph(&font, c)), "{c}");
        }
        // A fix that didn't run gets the red cross.
        let (_, recap) = split_recap("Finished\n- Not done: Reinstall, mirror timed out").unwrap();
        let (texts, _) = painted(&recap, &steps);
        assert_eq!(
            texts[..4],
            [
                "FINISHED",
                "0 of 1 done",
                "×",
                "Reinstall, mirror timed out"
            ]
        );
    }
}

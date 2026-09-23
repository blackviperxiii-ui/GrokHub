//! Empty-home cabin pulse — greeting-adjacent glance, cabin-real rows only.

use super::*;
use grokhub_core::{usage_line, Automation, BoardCard, BoardStatus, GrokLoop, UsageDay};

/// Ask-card Always second beat. Named so tests can lock the inherit line.
pub(super) const ALWAYS_CONFIRM_LINE1: &str = "Skip every tool prompt this launch.";
pub(super) const ALWAYS_CONFIRM_LINE2: &str =
    "Night, loops, and phone inherit --always-approve until quit.";

const PULSE_ROW_H: f32 = 22.0;
const PULSE_PAD: f32 = 12.0;
const PULSE_GAP: f32 = 2.0;
const PULSE_MAX_ROWS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PulseNav {
    Night,
    Workboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PulseRowKind {
    Job,
    Goal,
    Board,
    Usage,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PulseRow {
    pub kind: PulseRowKind,
    pub title: String,
    pub detail: String,
    pub nav: Option<PulseNav>,
}

pub(super) fn should_paint_pulse(empty_chat: bool, scratch: bool, signed_in: bool) -> bool {
    empty_chat && !scratch && signed_in
}

pub(super) fn pulse_card_h(row_count: usize) -> f32 {
    let n = row_count.clamp(1, PULSE_MAX_ROWS) as f32;
    PULSE_PAD + n * PULSE_ROW_H + (n - 1.0) * PULSE_GAP
}

fn board_is_open(status: BoardStatus) -> bool {
    !matches!(status, BoardStatus::Done | BoardStatus::Dismissed)
}

fn pulse_relative_when(then_ms: u64, now_ms: u64) -> String {
    if then_ms <= now_ms {
        return "now".into();
    }
    let mins = (then_ms - now_ms) / 60_000;
    match mins {
        0 => "in under a minute".into(),
        1 => "in 1 min".into(),
        2..=59 => format!("in {mins} min"),
        60..=1439 => {
            let h = mins / 60;
            let m = mins % 60;
            if m == 0 {
                format!("in {h}h")
            } else {
                format!("in {h}h {m}m")
            }
        }
        _ => {
            let days = mins / 1440;
            if days == 1 {
                "in 1 day".into()
            } else {
                format!("in {days} days")
            }
        }
    }
}

fn loop_title(row: &GrokLoop) -> String {
    let t: String = row.prompt.trim().chars().take(40).collect();
    if t.is_empty() {
        format!("Loop {}", row.interval)
    } else {
        t
    }
}

fn next_job_row(autos: &[Automation], loops: &[GrokLoop], now_ms: u64) -> PulseRow {
    let mut best: Option<(u64, String)> = None;
    for a in autos.iter().filter(|a| a.enabled) {
        let Some(when) = a.next_run else {
            continue;
        };
        let name = a.name.trim();
        if name.is_empty() {
            continue;
        }
        if best.as_ref().is_none_or(|(t, _)| when < *t) {
            best = Some((when, name.to_string()));
        }
    }
    for row in loops.iter().filter(|l| l.enabled) {
        let Some(when) = row.next_run else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| when < *t) {
            best = Some((when, loop_title(row)));
        }
    }
    if let Some((when, name)) = best {
        return PulseRow {
            kind: PulseRowKind::Job,
            title: name,
            detail: pulse_relative_when(when, now_ms),
            nav: Some(PulseNav::Night),
        };
    }
    let seed = crate::cards::SUGGESTED_AUTOS
        .iter()
        .find(|s| s.title == "Morning brief")
        .unwrap_or(&crate::cards::SUGGESTED_AUTOS[0]);
    PulseRow {
        kind: PulseRowKind::Job,
        title: seed.title.to_string(),
        detail: seed.body.to_string(),
        nav: Some(PulseNav::Night),
    }
}

/// Cabin-real pulse rows. Never weather, mail, calendar, or bank tiles.
pub(super) fn pulse_rows(
    autos: &[Automation],
    loops: &[GrokLoop],
    goal_pin: &str,
    goal_step: u32,
    board: &[BoardCard],
    usage: &UsageDay,
    now_ms: u64,
) -> Vec<PulseRow> {
    let mut out = Vec::new();
    out.push(next_job_row(autos, loops, now_ms));
    let pin = goal_pin.trim();
    if !pin.is_empty() {
        out.push(PulseRow {
            kind: PulseRowKind::Goal,
            title: pin.chars().take(48).collect(),
            detail: format!("step {goal_step}"),
            nav: None,
        });
    }
    let board_room = PULSE_MAX_ROWS.saturating_sub(out.len() + 1).min(2);
    for card in board
        .iter()
        .filter(|c| board_is_open(c.status))
        .take(board_room)
    {
        let title = card.title.trim();
        if title.is_empty() {
            continue;
        }
        out.push(PulseRow {
            kind: PulseRowKind::Board,
            title: title.chars().take(48).collect(),
            detail: String::new(),
            nav: Some(PulseNav::Workboard),
        });
    }
    out.push(PulseRow {
        kind: PulseRowKind::Usage,
        title: usage_line(usage),
        detail: String::new(),
        nav: None,
    });
    if out.is_empty() {
        out.push(PulseRow {
            kind: PulseRowKind::Empty,
            title: crate::cards::CHIP_EMPTY_LABEL.to_string(),
            detail: String::new(),
            nav: None,
        });
    }
    out
}

impl Cabin {
    pub(super) fn pulse_should_paint(&self) -> bool {
        should_paint_pulse(
            self.messages.is_empty(),
            self.scratch(),
            self.cabin_signed_in(),
        )
    }

    pub(super) fn collect_pulse_rows(&self) -> Vec<PulseRow> {
        pulse_rows(
            &self.automations,
            &self.grok_loops,
            &self.cfg.goal_pin,
            self.goal_step,
            &self.board,
            &self.usage,
            now_ms(),
        )
    }

    pub(super) fn paint_empty_pulse(&mut self, ui: &mut egui::Ui, pane_w: f32) {
        let rows = self.collect_pulse_rows();
        let mut go: Option<PulseNav> = None;
        egui::Frame::none()
            .fill(crate::theme::elevated())
            .rounding(crate::theme::CARD_RADIUS)
            .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
            .inner_margin(egui::Margin::same(6.0))
            .show(ui, |ui| {
                ui.set_width(pane_w);
                ui.spacing_mut().item_spacing.y = PULSE_GAP;
                for row in &rows {
                    let muted = row.kind == PulseRowKind::Usage || row.kind == PulseRowKind::Empty;
                    let color = if muted {
                        crate::theme::muted()
                    } else {
                        crate::theme::fg()
                    };
                    let label = if row.detail.is_empty() {
                        row.title.clone()
                    } else {
                        format!("{} · {}", row.title, row.detail)
                    };
                    if row.kind == PulseRowKind::Goal {
                        crate::cards::status_chip(ui, &label, crate::cards::ChipTone::Mute);
                        continue;
                    }
                    let resp = ui.add(
                        egui::Label::new(
                            RichText::new(label)
                                .size(crate::theme::FONT_TIP)
                                .color(color),
                        )
                        .sense(if row.nav.is_some() {
                            egui::Sense::click()
                        } else {
                            egui::Sense::hover()
                        }),
                    );
                    if resp.clicked() {
                        go = row.nav;
                    }
                }
            });
        if let Some(nav) = go {
            self.nav = match nav {
                PulseNav::Night => Nav::Night,
                PulseNav::Workboard => Nav::Workboard,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(name: &str, when: u64) -> Automation {
        Automation {
            id: "a1".into(),
            name: name.into(),
            schedule: "daily".into(),
            time: "09:00".into(),
            times: vec![],
            instructions: "summarize".into(),
            heartbeat_every_min: 0,
            check_command: String::new(),
            enabled: true,
            last_run: None,
            next_run: Some(when),
            run_count: 0,
        }
    }

    fn open_card(title: &str) -> BoardCard {
        BoardCard::new(title, "", "")
    }

    fn done_card(title: &str) -> BoardCard {
        let mut c = BoardCard::new(title, "", "");
        c.status = BoardStatus::Done;
        c
    }

    #[test]
    fn pulse_skips_scratch_and_full_thread() {
        assert!(should_paint_pulse(true, false, true));
        assert!(!should_paint_pulse(false, false, true));
        assert!(!should_paint_pulse(true, true, true));
        assert!(!should_paint_pulse(true, false, false));
    }

    #[test]
    fn pulse_uses_morning_brief_when_no_job() {
        let rows = pulse_rows(&[], &[], "", 0, &[], &UsageDay::default(), 1_000);
        assert_eq!(rows[0].kind, PulseRowKind::Job);
        assert_eq!(rows[0].title, "Morning brief");
        assert_eq!(rows[0].nav, Some(PulseNav::Night));
        assert!(rows.iter().any(|r| r.kind == PulseRowKind::Usage));
        let blob = rows
            .iter()
            .map(|r| format!("{} {}", r.title, r.detail))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !blob.contains("Outlook")
                && !blob.contains("Gmail")
                && !blob.contains("Drive")
                && !blob.to_ascii_lowercase().contains("weather")
                && !blob.to_ascii_lowercase().contains("calendar"),
            "pulse must stay cabin-real: {blob}"
        );
    }

    #[test]
    fn pulse_picks_soonest_job_and_omits_goal_when_empty() {
        let rows = pulse_rows(
            &[auto("Dawn snapshot", 5_000), auto("Nightly triage", 500)],
            &[],
            "",
            3,
            &[],
            &UsageDay::default(),
            1_000,
        );
        assert_eq!(rows[0].title, "Nightly triage");
        assert_eq!(rows[0].detail, "now");
        assert!(rows.iter().all(|r| r.kind != PulseRowKind::Goal));
    }

    #[test]
    fn pulse_goal_and_two_open_board_titles() {
        let rows = pulse_rows(
            &[auto("Host heartbeat", 90_000)],
            &[],
            "ship pulse",
            2,
            &[
                open_card("Write pulse card"),
                open_card("Always confirm"),
                open_card("Third stays off"),
                done_card("Already done"),
            ],
            &UsageDay {
                day: "2026-09-23".into(),
                messages: 1,
                ..Default::default()
            },
            1_000,
        );
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].kind, PulseRowKind::Job);
        assert_eq!(rows[1].kind, PulseRowKind::Goal);
        assert!(rows[1].title.contains("ship pulse"));
        assert!(rows[1].detail.contains("step 2"));
        assert_eq!(rows[2].kind, PulseRowKind::Board);
        assert_eq!(rows[2].title, "Write pulse card");
        assert_eq!(rows[2].nav, Some(PulseNav::Workboard));
        assert_eq!(rows[3].kind, PulseRowKind::Usage);
        assert!(rows[3].title.contains("today"));
        assert!(rows.iter().all(|r| r.title != "Always confirm"));
        assert!(rows.iter().all(|r| r.title != "Third stays off"));
        assert!(rows.iter().all(|r| r.title != "Already done"));
    }

    #[test]
    fn pulse_card_stays_short() {
        assert!(pulse_card_h(4) < 120.0);
        assert!(pulse_card_h(1) < pulse_card_h(4));
    }

    #[test]
    fn always_confirm_names_session_skip_and_scheduled_inherit() {
        assert!(ALWAYS_CONFIRM_LINE1
            .to_ascii_lowercase()
            .contains("skip every tool prompt this launch"));
        assert!(
            ALWAYS_CONFIRM_LINE2.contains("--always") && ALWAYS_CONFIRM_LINE2.contains("approve")
        );
        assert!(ALWAYS_CONFIRM_LINE2.contains("Night"));
        assert!(ALWAYS_CONFIRM_LINE2.contains("loop"));
        assert!(ALWAYS_CONFIRM_LINE2.contains("phone"));
    }
}

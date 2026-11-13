//! The quick chips under a Pulse card: the fixes it offers, Discuss, Ignore
//! for now, and Remind me later at a time that fits the person's day.

use crate::update_feed::{UpdateCard, UpdateKind};

/// Most chips a card shows. The rest wait in its ···.
pub const MAX_CHIPS: usize = 4;
/// A reminder tonight lands this long before the wind-down hour.
const BEFORE_WIND_DOWN_MIN: u32 = 30;
/// "Tonight" needs at least this long from now; closer, it is tomorrow.
const MIN_LEAD_MIN: u32 = 60;
const DAY_MIN: u32 = 24 * 60;

/// What the app knows of the person's day, read when the chips are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayShape {
    pub now_ms: u64,
    /// Local clock now.
    pub hour: u32,
    pub minute: u32,
    /// The hour they wind down (quiet hours start, or learned from their rhythm).
    pub wind_down: Option<u32>,
    /// The hour they usually sit down, learned from their rhythm.
    pub start: Option<u32>,
}

/// When Remind me later brings the card back, and the chip's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemindLater {
    pub at_ms: u64,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChipAct {
    /// The card's own main action (Run, Fix it…) when it offers no fixes.
    Primary,
    /// The card's offer at this index.
    Offer(usize),
    Discuss,
    /// Off the page now, with no "less of this".
    Ignore,
    RemindAt(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardChip {
    pub label: String,
    pub act: ChipAct,
}

/// Tonight before they wind down when there is time; else when they next sit
/// down (their usual start, 9 AM until it is learned).
pub fn remind_later(day: &DayShape) -> RemindLater {
    let now_min = day.hour.min(23) * 60 + day.minute.min(59);
    let minute_start = day.now_ms - day.now_ms % 60_000;
    let midnight = minute_start.saturating_sub(u64::from(now_min) * 60_000);
    let at = |min: u32| midnight + u64::from(min) * 60_000;
    let start = day.start.unwrap_or(9).min(23);
    if let Some(wind) = day.wind_down.map(|w| w % 24) {
        // A wind-down after midnight belongs to tonight, not this morning.
        let mut tonight = (wind * 60).saturating_sub(BEFORE_WIND_DOWN_MIN);
        if wind <= start {
            tonight += DAY_MIN;
        }
        if wind == 0 {
            tonight = DAY_MIN - BEFORE_WIND_DOWN_MIN;
        }
        if now_min + MIN_LEAD_MIN <= tonight {
            return RemindLater {
                at_ms: at(tonight),
                label: format!("Tonight, {} before you wind down", clock(tonight)),
            };
        }
    }
    if now_min + MIN_LEAD_MIN <= start * 60 {
        return RemindLater {
            at_ms: at(start * 60),
            label: format!("When you sit down, {}", clock(start * 60)),
        };
    }
    RemindLater {
        at_ms: at(DAY_MIN + start * 60),
        label: format!("Tomorrow when you sit down, {}", clock(start * 60)),
    }
}

/// "9:30 PM", "9 AM". Minutes past midnight, any day.
fn clock(min: u32) -> String {
    let min = min % DAY_MIN;
    let (h, m) = (min / 60, min % 60);
    let (h12, half) = match h {
        0 => (12, "AM"),
        12 => (12, "PM"),
        h if h < 12 => (h, "AM"),
        h => (h - 12, "PM"),
    };
    if m == 0 {
        format!("{h12} {half}")
    } else {
        format!("{h12}:{m:02} {half}")
    }
}

/// The card's chips: its fixes (or its main action, `primary`), Discuss when
/// there is room, Ignore for now, and Remind me later.
pub fn card_chips(card: &UpdateCard, day: &DayShape, primary: Option<&str>) -> Vec<CardChip> {
    let mut chips: Vec<CardChip> = card
        .pulse
        .report
        .iter()
        .flat_map(|r| r.offers.iter())
        .filter(|o| !o.label.trim().is_empty() && !o.prompt.trim().is_empty())
        .take(2)
        .enumerate()
        .map(|(i, o)| CardChip {
            label: o.label.trim().to_string(),
            act: ChipAct::Offer(i),
        })
        .collect();
    if chips.is_empty() {
        if let Some(label) = primary.map(str::trim).filter(|l| !l.is_empty()) {
            chips.push(CardChip {
                label: label.to_string(),
                act: ChipAct::Primary,
            });
        }
    }
    let discussable = matches!(card.kind, UpdateKind::Digest | UpdateKind::Suggestion);
    if discussable && chips.len() < MAX_CHIPS - 2 {
        chips.push(CardChip {
            label: "Discuss".into(),
            act: ChipAct::Discuss,
        });
    }
    chips.push(CardChip {
        label: "Ignore for now".into(),
        act: ChipAct::Ignore,
    });
    let later = remind_later(day);
    chips.push(CardChip {
        label: "Remind me later".into(),
        act: ChipAct::RemindAt(later.at_ms),
    });
    chips.truncate(MAX_CHIPS);
    chips
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card_report::{CardReport, FixOffer};
    use crate::update_feed::blank_card;

    /// 2026-11-08 00:00 UTC, read as local midnight.
    const MIDNIGHT: u64 = 1_794_096_000_000;
    const MIN: u64 = 60_000;

    fn day(hour: u32, minute: u32, wind_down: Option<u32>, start: Option<u32>) -> DayShape {
        DayShape {
            now_ms: MIDNIGHT + u64::from(hour * 60 + minute) * MIN + 17_000,
            hour,
            minute,
            wind_down,
            start,
        }
    }

    #[test]
    fn remind_later_fits_the_day() {
        // Afternoon, winds down at 10 PM: tonight at 9:30.
        assert_eq!(
            remind_later(&day(15, 10, Some(22), Some(8))),
            RemindLater {
                at_ms: MIDNIGHT + (21 * 60 + 30) * MIN,
                label: "Tonight, 9:30 PM before you wind down".into()
            }
        );
        // Too close to wind-down: when they sit down tomorrow.
        assert_eq!(
            remind_later(&day(21, 0, Some(22), Some(8))),
            RemindLater {
                at_ms: MIDNIGHT + (24 + 8) * 60 * MIN,
                label: "Tomorrow when you sit down, 8 AM".into()
            }
        );
        // A wind-down after midnight is still tonight.
        assert_eq!(
            remind_later(&day(22, 0, Some(1), Some(10))),
            RemindLater {
                at_ms: MIDNIGHT + (24 * 60 + 30) * MIN,
                label: "Tonight, 12:30 AM before you wind down".into()
            }
        );
        // Early morning, nothing learned yet: at 9 the same day.
        assert_eq!(
            remind_later(&day(6, 45, None, None)),
            RemindLater {
                at_ms: MIDNIGHT + 9 * 60 * MIN,
                label: "When you sit down, 9 AM".into()
            }
        );
        // Midnight wind-down.
        assert_eq!(
            remind_later(&day(20, 0, Some(0), None)).label,
            "Tonight, 11:30 PM before you wind down"
        );
    }

    #[test]
    fn chips_lead_with_the_cards_fixes() {
        let d = day(15, 0, Some(22), Some(8));
        let tonight = MIDNIGHT + (21 * 60 + 30) * MIN;
        let mut card = blank_card("c".into(), UpdateKind::Suggestion, "T".into(), None, 1);
        let labels = |chips: Vec<CardChip>| -> Vec<(String, ChipAct)> {
            chips.into_iter().map(|c| (c.label, c.act)).collect()
        };
        assert_eq!(
            labels(card_chips(&card, &d, Some("Run"))),
            vec![
                ("Run".into(), ChipAct::Primary),
                ("Discuss".into(), ChipAct::Discuss),
                ("Ignore for now".into(), ChipAct::Ignore),
                ("Remind me later".into(), ChipAct::RemindAt(tonight)),
            ]
        );
        card.pulse.report = Some(CardReport {
            found: "bluetoothd crashed 4 times today".into(),
            offers: vec![
                FixOffer {
                    label: "Restart bluetooth".into(),
                    prompt: "Restart the bluetooth service.".into(),
                    kind: crate::card_report::OfferKind::Chat,
                    fix_type: None,
                },
                FixOffer {
                    label: "Find out why".into(),
                    prompt: "Read its crash history.".into(),
                    kind: crate::card_report::OfferKind::Chat,
                    fix_type: None,
                },
                FixOffer {
                    label: "Third".into(),
                    prompt: "never shown".into(),
                    kind: crate::card_report::OfferKind::Chat,
                    fix_type: None,
                },
            ],
            ..Default::default()
        });
        assert_eq!(
            labels(card_chips(&card, &d, Some("Run"))),
            vec![
                ("Restart bluetooth".into(), ChipAct::Offer(0)),
                ("Find out why".into(), ChipAct::Offer(1)),
                ("Ignore for now".into(), ChipAct::Ignore),
                ("Remind me later".into(), ChipAct::RemindAt(tonight)),
            ]
        );
        let done = blank_card("d".into(), UpdateKind::AutomationDone, "T".into(), None, 1);
        assert_eq!(
            labels(card_chips(&done, &d, None)),
            vec![
                ("Ignore for now".into(), ChipAct::Ignore),
                ("Remind me later".into(), ChipAct::RemindAt(tonight)),
            ]
        );
    }
}

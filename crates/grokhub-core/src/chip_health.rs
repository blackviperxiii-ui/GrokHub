//! How the chips are doing, read from the local chip log alone: no model
//! call, nothing leaves the machine.
//!
//! A small share of boundaries (5%) show no chips at all, chosen by the
//! boundary's seed so the row stays empty while the person types. Those rows
//! are logged too, and the time from a row to the person's next step tells
//! whether chips make them faster. The first-run guide is never held back.

use serde::{Deserialize, Serialize};

use crate::chip_store::{ChipEvent, ChipOutcome, CHIP_LOG_MAX};
use crate::chips::{ChipMemory, QuickChip};

/// Share of boundaries that show no chips, in percent.
pub const HOLDOUT_PERCENT: u64 = 5;
/// Salt so the holdout draw is not the ranker's first draw.
const HOLDOUT_SALT: u64 = 0x686f_6c64_6f75_7421;
/// A step this long after a row is not an answer to it.
const NEXT_STEP_MAX_MS: u64 = 10 * 60 * 1000;
/// Sources named in the health lines.
const SOURCES_SHOWN: usize = 3;

/// Whether this boundary holds the chips back. Same seed, same answer.
pub fn in_holdout(seed: u64) -> bool {
    let mut z = seed ^ HOLDOUT_SALT;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (z ^ (z >> 31)) % 100 < HOLDOUT_PERCENT
}

/// A pool the holdout may empty: the first-run guide always shows.
pub(crate) fn holdout_allowed(pool: &[QuickChip]) -> bool {
    !pool.iter().any(|c| c.id.starts_with("guide-"))
}

/// Note a held-back row on `surface`. A repeat for the same open holdout
/// (a keystroke on the same boundary) adds nothing.
pub(crate) fn log_holdout(mem: &mut ChipMemory, surface: &str, now: u64) {
    let open = mem
        .events
        .iter()
        .rev()
        .find(|e| e.surface == surface)
        .filter(|e| e.outcome.is_none() && e.next_at.is_none());
    if open.is_some_and(|e| e.holdout) {
        return;
    }
    // The row before was passed over, as when a new row replaces it.
    if let Some(prev) = open {
        let shown = prev.shown.clone();
        crate::chip_rank::learn_from_row(&mut mem.ranker, &shown, None);
    }
    mem.events.push(ChipEvent {
        at: now,
        surface: surface.to_string(),
        holdout: true,
        ..ChipEvent::default()
    });
    if mem.events.len() > CHIP_LOG_MAX {
        let cut = mem.events.len() - CHIP_LOG_MAX;
        mem.events.drain(..cut);
    }
}

/// The person took a step (sent a message) while `surface` was up: the
/// newest row there, shown or held back, notes when. Only the first step
/// after a row counts.
pub fn log_chip_next_action(mem: &mut ChipMemory, surface: &str, now: u64) {
    if let Some(e) = mem.events.iter_mut().rev().find(|e| e.surface == surface)
        && e.next_at.is_none()
        && e.at <= now
    {
        e.next_at = Some(now);
    }
}

/// One source's share of the picks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceHealth {
    pub source: String,
    pub shown: u32,
    pub picked: u32,
}

/// The chips' numbers over the log.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipHealth {
    /// Rows that showed chips.
    pub rows: u32,
    pub picked: u32,
    pub hidden: u32,
    /// Rows held back to compare.
    pub held_back: u32,
    /// Middle time from a row to the next step, with chips and without.
    pub next_step_ms: Option<u64>,
    pub next_step_ms_held_back: Option<u64>,
    /// Best-picked sources first.
    pub sources: Vec<SourceHealth>,
}

impl ChipHealth {
    pub fn acceptance(&self) -> f32 {
        ratio(self.picked, self.rows)
    }

    pub fn hide_rate(&self) -> f32 {
        ratio(self.hidden, self.rows)
    }
}

fn ratio(n: u32, d: u32) -> f32 {
    if d == 0 {
        0.0
    } else {
        n as f32 / d as f32
    }
}

/// When the person moved on from `e`: a pick or their next message.
fn next_step(e: &ChipEvent) -> Option<u64> {
    let picked = match &e.outcome {
        Some(ChipOutcome::Picked { at, .. }) => Some(*at),
        _ => None,
    };
    let step = match (picked, e.next_at) {
        (Some(a), Some(b)) => a.min(b),
        (a, b) => a.or(b)?,
    };
    let ms = step.checked_sub(e.at)?;
    (ms <= NEXT_STEP_MAX_MS).then_some(ms)
}

fn median(mut v: Vec<u64>) -> Option<u64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[v.len() / 2])
}

/// Read the log into numbers.
pub fn chip_health(mem: &ChipMemory) -> ChipHealth {
    let mut h = ChipHealth::default();
    let mut with = Vec::new();
    let mut without = Vec::new();
    for e in &mem.events {
        if e.holdout {
            h.held_back += 1;
            without.extend(next_step(e));
            continue;
        }
        if e.shown.is_empty() {
            continue;
        }
        h.rows += 1;
        with.extend(next_step(e));
        let picked_key = match &e.outcome {
            Some(ChipOutcome::Picked { key, .. }) => {
                h.picked += 1;
                Some(key.as_str())
            }
            Some(ChipOutcome::Dismissed { .. }) => {
                h.hidden += 1;
                None
            }
            None => None,
        };
        for s in &e.shown {
            let i = match h.sources.iter().position(|x| x.source == s.source) {
                Some(i) => i,
                None => {
                    h.sources.push(SourceHealth {
                        source: s.source.clone(),
                        ..SourceHealth::default()
                    });
                    h.sources.len() - 1
                }
            };
            h.sources[i].shown += 1;
            if picked_key == Some(s.key.as_str()) {
                h.sources[i].picked += 1;
            }
        }
    }
    h.sources.sort_by(|a, b| {
        ratio(b.picked, b.shown)
            .total_cmp(&ratio(a.picked, a.shown))
            .then(b.shown.cmp(&a.shown))
    });
    h.next_step_ms = median(with);
    h.next_step_ms_held_back = median(without);
    h
}

fn percent(r: f32) -> String {
    format!("{}%", (r * 100.0).round() as u32)
}

fn seconds(ms: u64) -> String {
    format!("{:.1} s", ms as f32 / 1000.0)
}

/// The numbers in plain words, for Settings → Labs → Diagnostics.
pub fn chip_health_lines(h: &ChipHealth) -> Vec<String> {
    if h.rows == 0 && h.held_back == 0 {
        return vec!["Quick chips: nothing logged yet.".into()];
    }
    let mut out = vec![
        format!(
            "Quick chips: {} rows shown, {} held back to compare.",
            h.rows, h.held_back
        ),
        format!(
            "Picked {} of rows, hidden {}.",
            percent(h.acceptance()),
            percent(h.hide_rate())
        ),
    ];
    match (h.next_step_ms, h.next_step_ms_held_back) {
        (Some(a), Some(b)) => out.push(format!(
            "Next step: {} with chips, {} without.",
            seconds(a),
            seconds(b)
        )),
        (Some(a), None) => out.push(format!("Next step: {} with chips.", seconds(a))),
        _ => {}
    }
    let best: Vec<String> = h
        .sources
        .iter()
        .filter(|s| s.picked > 0)
        .take(SOURCES_SHOWN)
        .map(|s| format!("{} {}", s.source, percent(ratio(s.picked, s.shown))))
        .collect();
    if !best.is_empty() {
        out.push(format!("Picked most from: {}.", best.join(", ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chip_store::{log_chip_outcome, log_chips_shown};
    use crate::chips::ChipKind;

    fn chip(id: &str) -> QuickChip {
        QuickChip {
            id: id.into(),
            label: id.into(),
            value: id.into(),
            kind: ChipKind::Chat,
            score: 1.0,
            hint: String::new(),
            primary: false,
        }
    }

    #[test]
    fn about_one_boundary_in_twenty_is_held_back_and_holds_still() {
        let held = (0..20_000u64).filter(|&s| in_holdout(s)).count();
        assert!((800..1200).contains(&held), "{held}");
        for s in 0..200u64 {
            assert_eq!(in_holdout(s), in_holdout(s));
        }
        assert!(holdout_allowed(&[chip("pred-a")]));
        assert!(!holdout_allowed(&[chip("pred-a"), chip("guide-start")]));
    }

    #[test]
    fn a_held_back_row_is_logged_once_and_teaches_the_row_before() {
        let mut mem = ChipMemory::default();
        log_chips_shown(&mut mem, "composer", &[chip("pred-a")], 1_000);
        log_holdout(&mut mem, "composer", 2_000);
        log_holdout(&mut mem, "composer", 2_500);
        assert_eq!(mem.events.len(), 2);
        assert!(mem.events[1].holdout && mem.events[1].shown.is_empty());
        // A step closes it; the next held-back boundary is a new row.
        log_chip_next_action(&mut mem, "composer", 3_000);
        log_holdout(&mut mem, "composer", 4_000);
        assert_eq!(mem.events.len(), 3);
    }

    #[test]
    fn health_counts_picks_hides_and_the_time_to_the_next_step() {
        let mut mem = ChipMemory::default();
        // Picked after 2 s.
        log_chips_shown(&mut mem, "composer", &[chip("pred-a"), chip("reply-1")], 0);
        log_chip_outcome(&mut mem, "composer", &chip("reply-1"), true, 2_000);
        // Hidden, then a typed message 4 s after the row.
        log_chips_shown(&mut mem, "home", &[chip("pred-b")], 10_000);
        log_chip_outcome(&mut mem, "home", &chip("pred-b"), false, 11_000);
        log_chip_next_action(&mut mem, "home", 14_000);
        // Held back; the person typed after 9 s. A second step doesn't count.
        log_holdout(&mut mem, "composer", 20_000);
        log_chip_next_action(&mut mem, "composer", 29_000);
        log_chip_next_action(&mut mem, "composer", 40_000);
        // A step an hour later is not an answer to the row.
        log_chips_shown(&mut mem, "home", &[chip("pred-c")], 100_000);
        log_chip_next_action(&mut mem, "home", 100_000 + 60 * 60 * 1000);

        let h = chip_health(&mem);
        assert_eq!((h.rows, h.picked, h.hidden, h.held_back), (3, 1, 1, 1));
        assert_eq!(h.next_step_ms, Some(4_000));
        assert_eq!(h.next_step_ms_held_back, Some(9_000));
        assert_eq!(h.sources[0].source, "reply");
        assert_eq!((h.sources[0].shown, h.sources[0].picked), (1, 1));
        let lines = chip_health_lines(&h);
        assert_eq!(
            lines,
            vec![
                "Quick chips: 3 rows shown, 1 held back to compare.",
                "Picked 33% of rows, hidden 33%.",
                "Next step: 4.0 s with chips, 9.0 s without.",
                "Picked most from: reply 100%.",
            ]
        );
        assert_eq!(
            chip_health_lines(&ChipHealth::default()),
            vec!["Quick chips: nothing logged yet."]
        );
    }

    #[test]
    fn the_log_reads_old_events_without_the_new_fields() {
        let old = r#"{"at":5,"surface":"home","shown":[]}"#;
        let e: ChipEvent = serde_json::from_str(old).unwrap();
        assert!(!e.holdout && e.next_at.is_none());
        let back = serde_json::to_string(&e).unwrap();
        assert!(
            !back.contains("holdout") && !back.contains("nextAt"),
            "{back}"
        );
    }
}

//! Pulse ActionMode::Live: a card-fix dialog with a guided checklist.
//!
//! Opening a Live offer shows every step as not started. Start walks them
//! one by one (running → waiting for your OK → done). Stop ends the step in
//! flight and skips the rest. Closing after a finished run leaves a receipt
//! on the card; Undo fix records that the person asked to reverse it.

use std::path::Path;
use std::sync::Mutex;

use grokhub_core::action_mode::{action_mode, offer_shape, ActionMode};
use grokhub_core::card_report::{add_receipt, card_headline, CardReceipt, FixOffer};
use serde::Serialize;

use crate::pulse;

/// The live dialog's button while a fix is running.
pub const STOP_FIX: &str = "Stop fix";
/// The live dialog's button once a fix finished (or was stopped).
pub const UNDO_FIX: &str = "Undo fix";
/// The live dialog's primary before anything has run.
pub const START_FIX: &str = "Start fix";

/// Where one checklist row stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepMark {
    Pending,
    Running,
    NeedsOk,
    Done,
    Failed,
    Skipped,
}

impl StepMark {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::NeedsOk => "needsOk",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

/// One line of the live checklist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStep {
    pub plain: String,
    pub mark: StepMark,
}

/// What the page paints for the live dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveView {
    pub card_id: String,
    pub label: String,
    pub title: String,
    pub summary: String,
    pub steps: Vec<LiveStep>,
    /// "ready", "running", "awaiting", "done", or "stopped".
    pub phase: &'static str,
    pub result: Option<String>,
    pub can_undo: bool,
    pub primary: Option<&'static str>,
    pub stop: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct LiveRun {
    card_id: String,
    label: String,
    title: String,
    summary: String,
    what: String,
    steps: Vec<LiveStep>,
    cursor: usize,
    phase: Phase,
    result: Option<String>,
    undone: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Ready,
    Running,
    Awaiting,
    Done,
    Stopped,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Awaiting => "awaiting",
            Self::Done => "done",
            Self::Stopped => "stopped",
        }
    }
}

static RUN: Mutex<Option<LiveRun>> = Mutex::new(None);

fn with_run<T>(f: impl FnOnce(&mut Option<LiveRun>) -> Result<T, String>) -> Result<T, String> {
    let mut guard = RUN.lock().map_err(|_| "The live fix is busy. Try again.".to_string())?;
    f(&mut guard)
}

fn steps_for(offer: &FixOffer) -> Vec<String> {
    let named: Vec<String> = offer
        .steps
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if !named.is_empty() {
        return named;
    }
    // Older Live offers may only carry the chat prompt; split numbered lines.
    offer
        .prompt
        .lines()
        .filter_map(|line| {
            let t = line.trim();
            let rest = t
                .strip_prefix(|c: char| c.is_ascii_digit())
                .and_then(|s| s.strip_prefix('.').or_else(|| s.strip_prefix(')')))
                .map(str::trim)
                .filter(|s| !s.is_empty())?;
            Some(rest.to_string())
        })
        .collect()
}

fn view_of(run: &LiveRun) -> LiveView {
    let (primary, stop) = match run.phase {
        Phase::Ready => (Some(START_FIX), None),
        Phase::Running | Phase::Awaiting => (None, Some(STOP_FIX)),
        Phase::Done | Phase::Stopped => {
            if run.undone {
                (None, None)
            } else {
                (Some(UNDO_FIX), None)
            }
        }
    };
    LiveView {
        card_id: run.card_id.clone(),
        label: run.label.clone(),
        title: run.title.clone(),
        summary: run.summary.clone(),
        steps: run.steps.clone(),
        phase: run.phase.as_str(),
        result: run.result.clone(),
        can_undo: matches!(run.phase, Phase::Done | Phase::Stopped) && !run.undone,
        primary,
        stop,
    }
}

/// Open the live dialog for a Live offer. Replaces any unfinished run.
pub fn open(config_dir: &Path, id: &str, index: usize, now_ms: u64) -> Result<LiveView, String> {
    let card = pulse::find(config_dir, id)?;
    let offer = card
        .pulse
        .report
        .as_ref()
        .and_then(|r| r.offers.get(index))
        .cloned()
        .ok_or_else(|| "That fix is gone from the card. The page now shows what is left.".to_string())?;
    if action_mode(&offer_shape(&offer)) != ActionMode::Live {
        return Err("That fix opens in chat, not as a live checklist.".into());
    }
    let mut steps = steps_for(&offer);
    if steps.is_empty() {
        steps.push(offer.prompt.trim().to_string());
    }
    if steps.iter().all(|s| s.is_empty()) {
        return Err("That fix has no steps left to show.".into());
    }
    pulse::engaged(config_dir, &card, now_ms)?;
    let run = LiveRun {
        card_id: card.id.clone(),
        label: offer.label.trim().to_string(),
        title: card_headline(&card),
        summary: offer.prompt.trim().to_string(),
        what: offer.label.trim().to_string(),
        steps: steps
            .into_iter()
            .map(|plain| LiveStep {
                plain,
                mark: StepMark::Pending,
            })
            .collect(),
        cursor: 0,
        phase: Phase::Ready,
        result: None,
        undone: false,
    };
    let view = view_of(&run);
    with_run(|slot| {
        *slot = Some(run);
        Ok(view)
    })
}

/// The dialog still open, if any.
pub fn current() -> Result<Option<LiveView>, String> {
    with_run(|slot| Ok(slot.as_ref().map(view_of)))
}

fn advance_marks(run: &mut LiveRun) {
    if run.cursor >= run.steps.len() {
        run.phase = Phase::Done;
        run.result = Some(format!(
            "Finished \"{}\". Each step is on the checklist. Undo fix if you need to reverse it.",
            run.what
        ));
        return;
    }
    for (i, step) in run.steps.iter_mut().enumerate() {
        if i < run.cursor {
            if matches!(step.mark, StepMark::Pending | StepMark::Running | StepMark::NeedsOk) {
                step.mark = StepMark::Done;
            }
        } else if i == run.cursor {
            step.mark = StepMark::NeedsOk;
        }
    }
    run.phase = Phase::Awaiting;
}

/// Start the checklist, or answer the step waiting for an OK.
pub fn start_or_allow(config_dir: &Path, now_ms: u64) -> Result<LiveView, String> {
    with_run(|slot| {
        let run = slot
            .as_mut()
            .ok_or_else(|| "That live fix closed. Open it again from the card.".to_string())?;
        match run.phase {
            Phase::Ready => {
                run.phase = Phase::Running;
                if let Some(step) = run.steps.get_mut(run.cursor) {
                    step.mark = StepMark::Running;
                }
                advance_marks(run);
            }
            Phase::Awaiting => {
                if let Some(step) = run.steps.get_mut(run.cursor) {
                    step.mark = StepMark::Done;
                }
                run.cursor += 1;
                if run.cursor >= run.steps.len() {
                    run.phase = Phase::Done;
                    run.result = Some(format!(
                        "Finished \"{}\". Each step is on the checklist. Undo fix if you need to reverse it.",
                        run.what
                    ));
                    write_receipt(config_dir, run, now_ms, false)?;
                } else {
                    run.phase = Phase::Running;
                    if let Some(step) = run.steps.get_mut(run.cursor) {
                        step.mark = StepMark::Running;
                    }
                    advance_marks(run);
                }
            }
            Phase::Done | Phase::Stopped => {
                return Err("That fix already finished. Use Undo fix, or close the dialog.".into());
            }
            Phase::Running => {
                advance_marks(run);
            }
        }
        Ok(view_of(run))
    })
}

/// Deny the step waiting for an OK: mark it failed, skip the rest, stop.
pub fn deny(config_dir: &Path, now_ms: u64) -> Result<LiveView, String> {
    with_run(|slot| {
        let run = slot
            .as_mut()
            .ok_or_else(|| "That live fix closed. Open it again from the card.".to_string())?;
        if run.phase != Phase::Awaiting {
            return Err("Nothing is waiting for an OK right now.".into());
        }
        if let Some(step) = run.steps.get_mut(run.cursor) {
            step.mark = StepMark::Failed;
        }
        for step in run.steps.iter_mut().skip(run.cursor + 1) {
            step.mark = StepMark::Skipped;
        }
        run.phase = Phase::Stopped;
        run.result = Some(format!(
            "You said no to a step of \"{}\", so I stopped there. Nothing after it ran.",
            run.what
        ));
        write_receipt(config_dir, run, now_ms, false)?;
        Ok(view_of(run))
    })
}

/// Stop fix: end the step in flight and skip the rest.
pub fn stop(config_dir: &Path, now_ms: u64) -> Result<LiveView, String> {
    with_run(|slot| {
        let run = slot
            .as_mut()
            .ok_or_else(|| "That live fix closed. Open it again from the card.".to_string())?;
        if !matches!(run.phase, Phase::Running | Phase::Awaiting) {
            return Err("That fix is not running.".into());
        }
        if let Some(step) = run.steps.get_mut(run.cursor) {
            if matches!(step.mark, StepMark::Running | StepMark::NeedsOk | StepMark::Pending) {
                step.mark = StepMark::Skipped;
            }
        }
        for step in run.steps.iter_mut().skip(run.cursor + 1) {
            step.mark = StepMark::Skipped;
        }
        run.phase = Phase::Stopped;
        run.result = Some(format!(
            "I stopped \"{}\". Nothing more will run. Undo fix if you need to reverse what already ran.",
            run.what
        ));
        write_receipt(config_dir, run, now_ms, false)?;
        Ok(view_of(run))
    })
}

/// Undo fix: leave a receipt that the person asked to reverse the run.
pub fn undo(config_dir: &Path, now_ms: u64) -> Result<LiveView, String> {
    with_run(|slot| {
        let run = slot
            .as_mut()
            .ok_or_else(|| "That live fix closed. Open it again from the card.".to_string())?;
        if !matches!(run.phase, Phase::Done | Phase::Stopped) || run.undone {
            return Err("Undo fix is only on a finished run that has not been undone yet.".into());
        }
        run.undone = true;
        run.result = Some(format!(
            "Undone. Reverse any leftover change from \"{}\" by hand if something still looks off.",
            run.what
        ));
        write_receipt(config_dir, run, now_ms, true)?;
        Ok(view_of(run))
    })
}

/// Close the dialog without clearing a finished receipt.
pub fn close() -> Result<(), String> {
    with_run(|slot| {
        *slot = None;
        Ok(())
    })
}

fn write_receipt(config_dir: &Path, run: &LiveRun, now_ms: u64, undo: bool) -> Result<(), String> {
    let lines: Vec<String> = if undo {
        vec![run
            .result
            .clone()
            .unwrap_or_else(|| "Undone.".into())]
    } else {
        let mut out: Vec<String> = run
            .steps
            .iter()
            .map(|s| match s.mark {
                StepMark::Done => format!("✓ {}", s.plain),
                StepMark::Failed => format!("× {} (didn't work)", s.plain.trim_end_matches('.')),
                StepMark::Skipped => format!("– {} (didn't run)", s.plain.trim_end_matches('.')),
                StepMark::NeedsOk => format!("→ Waiting for your OK: {}", s.plain),
                StepMark::Running => format!("→ {}", s.plain),
                StepMark::Pending => format!("· {}", s.plain),
            })
            .collect();
        if let Some(result) = &run.result {
            out.push(result.clone());
        }
        out
    };
    let what = if undo {
        format!("Undid {}", run.what)
    } else {
        run.what.clone()
    };
    let receipt = CardReceipt {
        at_ms: now_ms,
        what,
        result: lines,
    };
    let id = run.card_id.clone();
    pulse::edit_cards(config_dir, &mut |cards| {
        let Some(stored) = cards.iter_mut().find(|c| c.id == id) else {
            return 0;
        };
        add_receipt(stored, receipt.clone());
        1
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::card_report::{CardReport, OfferKind};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn dir(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let d = std::env::temp_dir().join(format!("grokhub-pulse-live-{label}-{nanos}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }

    fn live_card(d: &Path) -> String {
        let mut card = grokhub_core::suggestion_card(
            "pulse:live",
            "GPU ran hot",
            "The fans can run full speed for a bit.",
            1,
        );
        card.id = "live-1".into();
        card.pulse.report = Some(CardReport {
            found: "GPU hit 87°C in the last session".into(),
            summary: "The fans can run full speed for a bit.".into(),
            offers: vec![FixOffer {
                label: "Run the fans at full speed".into(),
                prompt: "Save each fan, set full speed, then check.".into(),
                kind: OfferKind::Live,
                fix_type: None,
                steps: vec![
                    "Save each fan's setting.".into(),
                    "Set every fan to full speed.".into(),
                    "Check the fans read full speed.".into(),
                ],
            }],
            ..Default::default()
        });
        pulse::edit_cards(d, &mut |cards| {
            cards.clear();
            cards.push(card.clone());
            1
        })
        .expect("store");
        card.id
    }

    #[test]
    fn a_live_offer_opens_a_checklist_and_walks_each_step() {
        let d = dir("walk");
        let id = live_card(&d);
        let view = open(&d, &id, 0, 1_000).expect("open");
        assert_eq!(view.phase, "ready");
        assert_eq!(view.primary, Some(START_FIX));
        assert_eq!(view.steps.len(), 3);
        assert!(view.steps.iter().all(|s| s.mark == StepMark::Pending));

        let view = start_or_allow(&d, 1_001).expect("start");
        assert_eq!(view.phase, "awaiting");
        assert_eq!(view.steps[0].mark, StepMark::NeedsOk);
        assert_eq!(view.stop, Some(STOP_FIX));

        let view = start_or_allow(&d, 1_002).expect("allow 1");
        assert_eq!(view.steps[0].mark, StepMark::Done);
        assert_eq!(view.steps[1].mark, StepMark::NeedsOk);

        let view = start_or_allow(&d, 1_003).expect("allow 2");
        assert_eq!(view.steps[1].mark, StepMark::Done);
        assert_eq!(view.steps[2].mark, StepMark::NeedsOk);

        let view = start_or_allow(&d, 1_004).expect("allow 3");
        assert_eq!(view.phase, "done");
        assert_eq!(view.primary, Some(UNDO_FIX));
        assert!(view.steps.iter().all(|s| s.mark == StepMark::Done));
        close().expect("close");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn stop_fix_skips_the_rest_and_chat_offers_still_refuse_live() {
        let d = dir("stop");
        let id = live_card(&d);
        open(&d, &id, 0, 2_000).expect("open");
        start_or_allow(&d, 2_001).expect("start");
        let view = stop(&d, 2_002).expect("stop");
        assert_eq!(view.phase, "stopped");
        assert_eq!(view.steps[0].mark, StepMark::Skipped);
        assert!(view.steps[1..].iter().all(|s| s.mark == StepMark::Skipped));

        let err = open(&d, &id, 0, 2_003);
        // Re-open is fine after stop; chat-shaped offers are what refuse.
        assert!(err.is_ok(), "{err:?}");
        close().expect("close");

        let mut card = grokhub_core::suggestion_card("pulse:chat", "t", "b", 1);
        card.id = "chat-1".into();
        card.pulse.report = Some(CardReport {
            found: "Ask me".into(),
            offers: vec![FixOffer {
                label: "Find out why".into(),
                prompt: "Why did this happen?".into(),
                kind: OfferKind::Chat,
                fix_type: None,
                steps: vec![],
            }],
            ..Default::default()
        });
        pulse::edit_cards(&d, &mut |cards| {
            cards.clear();
            cards.push(card.clone());
            1
        })
        .expect("store");
        let err = open(&d, "chat-1", 0, 2_010).expect_err("chat");
        assert!(err.contains("chat"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }
}

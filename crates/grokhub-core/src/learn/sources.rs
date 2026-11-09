//! What the app's existing logs mean as signals.
//!
//! The chip log, the card log and the run log keep their own files and
//! readers. Their writers also hand each event here, so every learner can
//! see it. These functions are pure: they build the signals and the caller
//! passes them to [`super::hub::note_all`].
//!
//! Every subject is built from an id the app made: a chip's hashed key, a
//! card kind, a model id, a skill name, a surface name. Nothing the user
//! typed can reach one, and an id that isn't a valid [`Subject`] is skipped.

use super::signal::{Kind, Signal, Subject};
use crate::card_signals::{CardEvent, CardSignal};
use crate::outcome::OutcomeResult;
use crate::run_metrics::RunMetrics;

fn push(out: &mut Vec<Signal>, at_ms: u64, kind: Kind, area: &str, id: &str) {
    if let Some(subject) = Subject::new(area, id) {
        out.push(Signal::new(at_ms, kind, subject));
    }
}

/// A chip row went up on `surface`. `keys` are the chips' log keys
/// ([`crate::chip_log_key`]).
pub fn chips_shown<'a>(
    surface: &str,
    keys: impl IntoIterator<Item = &'a str>,
    at_ms: u64,
) -> Vec<Signal> {
    let mut out = Vec::new();
    push(&mut out, at_ms, Kind::Shown, "chips", surface);
    for key in keys {
        push(&mut out, at_ms, Kind::Shown, "chip", key);
    }
    out
}

/// A row was replaced before anyone touched it: each chip, and chips on
/// that surface, were passed over once.
pub fn chips_passed_over<'a>(
    surface: &str,
    keys: impl IntoIterator<Item = &'a str>,
    at_ms: u64,
) -> Vec<Signal> {
    let mut out = Vec::new();
    push(&mut out, at_ms, Kind::Ignored, "chips", surface);
    for key in keys {
        push(&mut out, at_ms, Kind::Ignored, "chip", key);
    }
    out
}

/// The person tapped a chip (`picked`) or hid it. A tap also counts for
/// chips on that surface; a hide says nothing about the surface.
pub fn chip_outcome(surface: &str, key: &str, picked: bool, at_ms: u64) -> Vec<Signal> {
    let mut out = Vec::new();
    if picked {
        push(&mut out, at_ms, Kind::Picked, "chips", surface);
        push(&mut out, at_ms, Kind::Picked, "chip", key);
    } else {
        push(&mut out, at_ms, Kind::Hidden, "chip", key);
    }
    out
}

/// The plan replies every plan card offers. "More on step N" is left out:
/// it shows only when the plan has steps.
pub const PLAN_REPLIES: [&str; 5] = ["go", "more", "change", "go-but", "not-now"];

/// The subject for a reply chip's key (`plan:go` is `reply:plan/go`). `None`
/// for a question's option, whose key is the agent's own words.
pub fn reply_subject(key: &str) -> Option<Subject> {
    let id = key.strip_prefix("plan:")?;
    Subject::new("reply", &format!("plan/{id}"))
}

/// The person answered a waiting plan with the reply chip `key`: one pick
/// for it, and one pass for each other reply that is always on the row.
pub fn reply_picked(key: &str, at_ms: u64) -> Vec<Signal> {
    let mut out = Vec::new();
    let Some(picked) = reply_subject(key) else {
        return out;
    };
    for other in PLAN_REPLIES {
        match reply_subject(&format!("plan:{other}")) {
            Some(subject) if subject != picked => {
                out.push(Signal::new(at_ms, Kind::Ignored, subject));
            }
            _ => {}
        }
    }
    out.push(Signal::new(at_ms, Kind::Picked, picked));
    out
}

fn card_kind(event: CardEvent) -> Kind {
    match event {
        CardEvent::Opened | CardEvent::FollowUp => Kind::Used,
        CardEvent::More => Kind::Liked,
        CardEvent::Less => Kind::Disliked,
        CardEvent::Hidden => Kind::Hidden,
        CardEvent::Dismissed => Kind::Dismissed,
        CardEvent::Rejected => Kind::Declined,
        CardEvent::Ran => Kind::Accepted,
        CardEvent::Completed => Kind::Succeeded,
        // Taking back a hide is the user's own setting, not a vote.
        CardEvent::Unhidden => Kind::SetByUser,
    }
}

/// One line of the card log: what the person did with a Pulse or Home card,
/// counted for its kind (`card:suggestion`) and for its group
/// (`card-group:run:nightly`).
pub fn card(signal: &CardSignal) -> Vec<Signal> {
    let kind = match signal.event {
        // Closing a card after reading it is not saying no to it.
        CardEvent::Dismissed if signal.after_open == Some(true) => return Vec::new(),
        event => card_kind(event),
    };
    let mut out = Vec::new();
    push(&mut out, signal.ts, kind, "card", &signal.kind);
    push(&mut out, signal.ts, kind, "card-group", &signal.group);
    out
}

/// One line of the run log: how a finished run went, for its model and, when
/// it ran a skill, for the skill. A later line for the same run (a
/// correction, an undo) counts against both.
pub fn run(m: &RunMetrics) -> Vec<Signal> {
    let late = m.undos > 0 || m.corrections > 0;
    let vote = if m.undos > 0 {
        Some(Kind::Undone)
    } else if m.corrections > 0 {
        Some(Kind::Corrected)
    } else {
        match m.result {
            OutcomeResult::Success => Some(Kind::Succeeded),
            OutcomeResult::Failure => Some(Kind::Failed),
            OutcomeResult::Pending => None,
        }
    };
    let skill = m.class.strip_prefix("skill:").unwrap_or_default();
    let mut out = Vec::new();
    if let Some(kind) = vote {
        push(&mut out, m.at, kind, "model", &m.model);
        push(&mut out, m.at, kind, "skill", skill);
    }
    // The numbers are the run's own: a late line would count them twice.
    if !late {
        let mut measure = |what: &str, value: f32| {
            if let Some(subject) = Subject::new("model", &format!("{}/{what}", m.model)) {
                out.push(Signal::measured(m.at, subject, value));
            }
        };
        if !m.model.is_empty() {
            measure("run-ms", m.wall_ms as f32);
            measure("run-usd", m.cost_usd as f32);
        }
    }
    out
}

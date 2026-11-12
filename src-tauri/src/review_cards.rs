//! Apply on the weekly self-review's Pulse cards, from either app's pass:
//! the cabin's button labels (Apply change, Revert, Save idea, plus Copy as
//! GitHub issue on a code idea) and the same writes. A skill or chat-note
//! change goes through the change ledger and on trial; a revert goes
//! through the ledger's undo with the click's `UndoAsk`; a code idea is only
//! saved, never made. What each card applies is the `pending` entry in
//! `self_review.json` that the posting pass left.

use std::path::Path;

use grokhub_agent::harness::weekly_review as wr;
use grokhub_agent::harness::ChangeKind;
use grokhub_core::self_review::{self as sr, CardTarget};
use grokhub_core::UpdateCard;

pub const GONE: &str = "That card is gone. The page now shows what is left.";
pub const NOT_WAITING: &str =
    "This suggestion is no longer waiting. The next Sunday self-review proposes again.";
const NOT_REVIEW: &str = "This card has nothing to apply.";

/// The card's Apply button, as the cabin labels it. Empty on other cards.
pub fn label(card: &UpdateCard) -> &'static str {
    match sr::card_target(&card.source_id) {
        Some(CardTarget::Revert(_)) => "Revert",
        Some(CardTarget::Code(_)) => "Save idea",
        Some(_) => "Apply change",
        None => "",
    }
}

/// A code idea also offers Copy as GitHub issue.
pub fn copies_issue(card: &UpdateCard) -> bool {
    matches!(sr::card_target(&card.source_id), Some(CardTarget::Code(_)))
}

/// What a self-review card's Apply does, as the cabin shows it: the numbers
/// and the change, kept as plain text so its diff lines read as written.
pub fn details(card: &UpdateCard) -> Option<String> {
    if label(card).is_empty() {
        return None;
    }
    let text = card.details.as_deref()?.trim();
    (!text.is_empty()).then(|| format!("```text\n{text}\n```"))
}

/// Apply one self-review card. Returns the status line. A replay refusal
/// keeps the card with the reason on top of its details; anything else that
/// went through takes the card off the feed and posts a trial note when a
/// trial started.
pub fn apply(config_dir: &Path, id: &str, now: u64) -> Result<String, String> {
    let card = crate::pulse::cards(config_dir)?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or(GONE)?;
    let target = sr::card_target(&card.source_id).ok_or(NOT_REVIEW)?;
    let pending = wr::pending_for(config_dir, &card.source_id).ok_or(NOT_WAITING)?;
    let done = match target {
        CardTarget::Code(_) => {
            wr::save_idea(config_dir, &card.title, &pending.md, now)?;
            wr::Applied {
                line: wr::idea_saved_line(),
                trial: None,
                keep_card: false,
            }
        }
        CardTarget::Revert(name) => wr::Applied {
            line: crate::changes::undo(config_dir, ChangeKind::Skill, &name, true)?,
            trial: None,
            keep_card: false,
        },
        CardTarget::Snippet(name) => {
            wr::apply_snippet(config_dir, &name, &card.title, &pending.md, now)?
        }
        CardTarget::Patch(name) | CardTarget::Draft(name) => {
            let skills_dir = config_dir.join("skills");
            let skills = wr::cabin_skills(&skills_dir);
            wr::apply_skill(
                config_dir,
                &skills_dir,
                &name,
                &card.title,
                &pending.md,
                &skills,
                now,
            )?
        }
    };
    let line = done.line.clone();
    if done.keep_card {
        crate::pulse::edit_cards(config_dir, &mut |cards| {
            let Some(c) = cards.iter_mut().find(|c| c.id == id) else {
                return 0;
            };
            c.details = Some(format!(
                "{line}\n\n{}",
                c.details.clone().unwrap_or_default()
            ));
            1
        })?;
        return Ok(line);
    }
    wr::remove_pending(config_dir, &card.source_id)?;
    let mut trial = done.trial.map(|v| wr::trial_card(&v));
    crate::pulse::edit_cards(config_dir, &mut |cards| {
        let mut n = usize::from(grokhub_core::dismiss_update_at(cards, id, now));
        if let Some(note) = trial.take() {
            n += usize::from(grokhub_core::post_update(cards, note));
        }
        n
    })?;
    Ok(line)
}

/// A code idea's issue text, redacted, for the clipboard. The card stays.
pub fn issue(config_dir: &Path, id: &str, home: Option<&str>) -> Result<String, String> {
    let card = crate::pulse::cards(config_dir)?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or(GONE)?;
    if !copies_issue(&card) {
        return Err(NOT_REVIEW.into());
    }
    let pending = wr::pending_for(config_dir, &card.source_id).ok_or(NOT_WAITING)?;
    Ok(sr::issue_text(
        &card.title,
        &pending.md,
        env!("CARGO_PKG_VERSION"),
        home,
    ))
}

#[cfg(test)]
#[path = "review_cards_tests.rs"]
mod tests;

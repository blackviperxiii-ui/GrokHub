//! Offer a chat model that isn't on this PC when it would do the task in
//! hand clearly better than every model that is. Pure: the shell asks after
//! an on-device reply, reads the disk and this PC, and keeps when it last
//! offered each model.
//!
//! The order keeps the costly read last: [`due`] skips a task kind offered
//! lately, [`worth_offering`] needs only which files are on disk, [`due`]
//! drops a model offered lately, and only then does the shell read this PC
//! for [`first_fitting`].

use std::collections::BTreeMap;

use crate::local_setup::{self as ls, LocalModel, ModelKind};
use crate::local_skills::{self, TaskKind};
use crate::model_library::{Fit, Row};

/// How far a model not on disk has to beat the best one on disk at the task,
/// on the catalog's 0–100 skill scale, before it's worth a download. Wider
/// than the device's own swap margin (8): a download costs minutes and
/// gigabytes, a swap costs a reload.
pub const OFFER_MARGIN: u8 = 12;

/// A model, and any model for one task kind, is offered at most once in this
/// long, whatever the answer.
pub const OFFER_EVERY_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Chat models not on disk that beat every chat model on disk at `kind` by
/// [`OFFER_MARGIN`], best first (the lighter one first on a tie). Empty for
/// everyday chat and 18+ chat, and when no chat model is on disk: setup
/// offers the first one.
pub fn worth_offering(
    catalog: &'static [LocalModel],
    kind: TaskKind,
    on_disk: impl Fn(&LocalModel) -> bool,
) -> Vec<&'static LocalModel> {
    if matches!(kind, TaskKind::Chat | TaskKind::Adult) {
        return Vec::new();
    }
    let chat = || catalog.iter().filter(|m| m.kind == ModelKind::Chat);
    let Some(have) = chat()
        .filter(|m| on_disk(m))
        .map(|m| local_skills::skill(m, kind))
        .max()
    else {
        return Vec::new();
    };
    let bar = have.saturating_add(OFFER_MARGIN);
    let mut wanted: Vec<&'static LocalModel> = chat()
        .filter(|m| m.offered && !on_disk(m) && local_skills::skill(m, kind) >= bar)
        .collect();
    wanted.sort_by(|a, b| {
        local_skills::skill(b, kind)
            .cmp(&local_skills::skill(a, kind))
            .then(a.run_mb.cmp(&b.run_mb))
    });
    wanted
}

/// Whether `key` (a model id or a task kind's key) may be offered at
/// `now_ms`: never offered, or not within [`OFFER_EVERY_MS`]. A clock that
/// went back counts as offered.
pub fn due(shown: &BTreeMap<String, u64>, key: &str, now_ms: u64) -> bool {
    shown
        .get(key)
        .is_none_or(|&at| now_ms >= at && now_ms - at >= OFFER_EVERY_MS)
}

/// The first of `wanted` whose row says it fits this PC well (room to run
/// and room on disk). A tight fit isn't worth a download.
pub fn first_fitting(wanted: &[&'static LocalModel], rows: &[Row]) -> Option<&'static LocalModel> {
    wanted.iter().copied().find(|m| {
        rows.iter()
            .any(|r| r.model.id == m.id && r.fit == Fit::Yes && !r.on_disk)
    })
}

/// The card's question.
pub fn prompt(m: &LocalModel, kind: TaskKind) -> String {
    format!(
        "{} would handle {} better than the models on this computer. Download it? ({})",
        m.name,
        kind.words(),
        ls::size_label(m.download_mb)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = "gemma-4-e4b-it-abliterated-q4_k";
    const MID: &str = "gemma-4-12b-it-abliterated-q4_k";

    fn m(id: &str) -> &'static LocalModel {
        ls::model_by_id(id).expect(id)
    }

    fn ids(v: &[&LocalModel]) -> Vec<&'static str> {
        v.iter().map(|m| m.id).collect()
    }

    fn row(id: &str, fit: Fit) -> Row {
        Row {
            model: m(id),
            fit,
            why: String::new(),
            on_disk: false,
        }
    }

    #[test]
    fn a_better_coder_is_offered_best_first_over_the_one_on_disk() {
        // Gemma 4 12B codes at 58, so the bar is 70.
        let w = worth_offering(ls::local_models(), TaskKind::Code, |x| x.id == MID);
        assert_eq!(
            ids(&w[..3]),
            [
                "qwen3.6-35b-a3b-heretic-q4_k_m",
                "qwen3.8-27b-heretic-q4_k_m",
                "zai-glm4.7-flash-30b-a3b-heretic-q4_k_m",
            ]
        );
        assert!(w
            .iter()
            .all(|x| local_skills::skill(x, TaskKind::Code) >= 70));
        assert!(!ids(&w).contains(&MID));
        // Three models code at 74: the lightest of them comes first.
        let at74: Vec<_> = w
            .iter()
            .filter(|x| local_skills::skill(x, TaskKind::Code) == 74)
            .map(|x| x.id)
            .collect();
        assert_eq!(at74[0], "openai-gpt-oss-20b-heretic-mxfp4_moe");
    }

    #[test]
    fn everyday_chat_18_plus_and_an_empty_disk_offer_nothing() {
        let on = |x: &LocalModel| x.id == SMALL;
        assert!(worth_offering(ls::local_models(), TaskKind::Chat, on).is_empty());
        assert!(worth_offering(ls::local_models(), TaskKind::Adult, on).is_empty());
        assert!(worth_offering(ls::local_models(), TaskKind::Code, |_| false).is_empty());
    }

    #[test]
    fn nothing_is_offered_when_the_disk_already_has_a_strong_one() {
        // Qwen 3.6 35B codes at 85; nothing in the catalog reaches 97.
        let w = worth_offering(ls::local_models(), TaskKind::Code, |x| {
            x.id == "qwen3.6-35b-a3b-heretic-q4_k_m"
        });
        assert!(w.is_empty(), "{:?}", ids(&w));
    }

    #[test]
    fn the_offer_is_the_best_one_that_fits_this_pc_well() {
        let w = worth_offering(ls::local_models(), TaskKind::Code, |x| x.id == MID);
        let rows = [
            row("qwen3.6-35b-a3b-heretic-q4_k_m", Fit::No),
            row("qwen3.8-27b-heretic-q4_k_m", Fit::Tight),
            row("openai-gpt-oss-20b-heretic-mxfp4_moe", Fit::Yes),
            row("gemma-4-31b-it-abliterated-q4_k", Fit::Yes),
        ];
        assert_eq!(
            first_fitting(&w, &rows).map(|x| x.id),
            Some("openai-gpt-oss-20b-heretic-mxfp4_moe")
        );
        assert_eq!(first_fitting(&w, &rows[..2]), None);
    }

    #[test]
    fn a_model_is_offered_again_only_after_thirty_days() {
        let mut shown = BTreeMap::new();
        assert!(due(&shown, MID, 5));
        shown.insert(MID.to_string(), 1_000);
        assert!(!due(&shown, MID, 1_000 + OFFER_EVERY_MS - 1));
        assert!(due(&shown, MID, 1_000 + OFFER_EVERY_MS));
        assert!(!due(&shown, MID, 10), "a clock that went back");
        assert!(due(&shown, SMALL, 10));
    }

    #[test]
    fn the_question_names_the_model_the_task_and_the_size() {
        assert_eq!(
            prompt(m(MID), TaskKind::Write),
            format!(
                "{} would handle writing better than the models on this computer. Download it? ({})",
                m(MID).name,
                ls::size_label(m(MID).download_mb)
            )
        );
    }
}

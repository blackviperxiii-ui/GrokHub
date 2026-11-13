//! Download models: every catalog row by tab (Chat, Drafts, Image, Voice,
//! Other) with whether it fits this PC, what it is for, and the queue that
//! "Download all that fit" fetches. Both shells read these rows; the shells
//! own the downloading.

use crate::local_picker::{self, BenchCache, Bucket, Profile};
use crate::local_setup::{
    self as ls, LocalModel, ModelKind, ModelTab, DISK_HEADROOM_MB, STANDARD_MODEL_CAP,
};
use crate::model_download::{Outcome, Step};

/// Whether a model fits this PC, in the words a row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Yes,
    Tight,
    No,
}

impl Fit {
    pub fn label(self) -> &'static str {
        match self {
            Fit::Yes => "Fits this PC",
            Fit::Tight => "Fits, may be slow",
            Fit::No => "Won't run well here",
        }
    }
}

/// One Download models row.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub model: &'static LocalModel,
    pub fit: Fit,
    /// Why it fits as it does, from the picker.
    pub why: String,
    pub on_disk: bool,
}

impl Row {
    pub fn tab(&self) -> ModelTab {
        self.model.kind.tab()
    }
}

/// What a model is for, in plain words.
pub fn good_for(m: &LocalModel) -> String {
    match m.kind {
        ModelKind::Drafter => match m.pairs_with.and_then(ls::model_by_id) {
            Some(pair) => format!("Makes {} reply faster.", pair.name),
            None => "Makes replies come faster.".into(),
        },
        ModelKind::Chat => match m.run_mb {
            0..=5_000 => "Quick chats and background tasks, even on a laptop.",
            5_001..=9_000 => "Everyday chat and writing on a mid-range PC.",
            9_001..=18_000 => "Longer tasks and stronger answers.",
            _ => "The strongest answers; wants a big graphics card.",
        }
        .into(),
        ModelKind::Image => "Makes pictures on this PC.".into(),
        ModelKind::ImageGuard => "Checks pictures before they show.".into(),
        ModelKind::SpeechToText => "Hears what you say.".into(),
        ModelKind::TextToSpeech => "Reads replies aloud.".into(),
        _ => "Helps other models with search and checks.".into(),
    }
}

/// The row's one line: name, size, fit, licence and the uncensored badge.
pub fn row_line(r: &Row) -> String {
    let mut line = format!(
        "{} · {} · {}",
        r.model.name,
        ls::size_label(r.model.download_mb),
        r.fit.label()
    );
    if r.model.abliterated.is_some() {
        line.push_str(" · Uncensored");
    }
    line.push_str(&format!(" · {}", r.model.license));
    line
}

/// What a tab says when the catalog has nothing in it yet.
pub fn empty_tab_line(tab: ModelTab) -> &'static str {
    match tab {
        ModelTab::Image => "No image models yet. They arrive with on-device image making.",
        ModelTab::Voice => "No voice models yet. They arrive with on-device voice.",
        _ => "Nothing here yet.",
    }
}

fn fit_of(b: Bucket) -> Fit {
    match b {
        Bucket::Recommended => Fit::Yes,
        Bucket::MaySlow => Fit::Tight,
        Bucket::Hidden => Fit::No,
    }
}

/// Every catalog row in catalog order. A draft model fits as its chat model
/// does; other specialty models fit when the disk has room. The on-device
/// speech-to-text model shows only once it is on disk (so it can be removed):
/// nothing transcribes with it yet, so the list doesn't offer it.
pub fn rows(p: &Profile, bench: &BenchCache, on_disk: impl Fn(&LocalModel) -> bool) -> Vec<Row> {
    rows_from(ls::local_models(), p, bench, on_disk)
}

fn rows_from(
    catalog: &'static [LocalModel],
    p: &Profile,
    bench: &BenchCache,
    on_disk: impl Fn(&LocalModel) -> bool,
) -> Vec<Row> {
    let picks = local_picker::score(p, catalog, bench);
    let pick = |id: &str| picks.iter().find(|x| x.model.id == id);
    catalog
        .iter()
        .map(|m| (m, on_disk(m)))
        .filter(|&(m, on_disk)| on_disk || m.kind != ModelKind::SpeechToText)
        .map(|(m, on_disk)| {
            let (fit, why) = if let Some(x) = pick(m.id) {
                (fit_of(x.bucket), x.reason.clone())
            } else if let Some(x) = m.pairs_with.and_then(pick) {
                (fit_of(x.bucket), format!("Comes with {}.", x.model.name))
            } else if p.free_disk_mb >= ls::disk_needed_mb(m) {
                (Fit::Yes, String::new())
            } else {
                (
                    Fit::No,
                    format!(
                        "Needs {} of free disk.",
                        ls::size_label(ls::disk_needed_mb(m))
                    ),
                )
            };
            Row {
                model: m,
                fit,
                why,
                on_disk,
            }
        })
        .collect()
}

/// What "Download all that fit" fetches, for one tab or (`None`) every tab:
/// models not on disk that fit, best fits first, each draft model after its
/// chat model and only with it. Stops before the disk runs out and keeps the
/// standard models at the cap.
pub fn queue(
    rows: &[Row],
    tab: Option<ModelTab>,
    free_disk_mb: Option<u64>,
) -> Vec<&'static LocalModel> {
    let want = |r: &&Row| !r.on_disk && r.fit != Fit::No && tab.is_none_or(|t| r.tab() == t);
    let mut room = free_disk_mb.map(|f| f.saturating_sub(DISK_HEADROOM_MB));
    let mut standard = rows
        .iter()
        .filter(|r| r.on_disk && ls::counts_toward_cap(r.model))
        .count();
    let mut out: Vec<&'static LocalModel> = Vec::new();
    let mut take = |m: &'static LocalModel, out: &mut Vec<&'static LocalModel>| -> bool {
        if let Some(left) = room.as_mut() {
            if *left < m.download_mb {
                return false;
            }
            *left -= m.download_mb;
        }
        out.push(m);
        true
    };
    let ordered = [Fit::Yes, Fit::Tight]
        .into_iter()
        .flat_map(|f| rows.iter().filter(move |r| r.fit == f));
    for r in ordered.filter(want) {
        let m = r.model;
        if m.kind == ModelKind::Drafter {
            // Only beside its chat model: on disk already or queued here.
            let paired = m.pairs_with.is_some_and(|id| {
                out.iter().any(|q| q.id == id) || rows.iter().any(|o| o.model.id == id && o.on_disk)
            });
            if paired && !out.iter().any(|q| q.id == m.id) {
                take(m, &mut out);
            }
            continue;
        }
        if ls::counts_toward_cap(m) && standard >= STANDARD_MODEL_CAP {
            continue;
        }
        if !take(m, &mut out) {
            continue;
        }
        if ls::counts_toward_cap(m) {
            standard += 1;
        }
        // Its draft model rides right behind it, on the Chat tab too.
        let drafter = m
            .drafter
            .and_then(|id| rows.iter().find(|o| o.model.id == id));
        if let Some(d) = drafter.filter(|d| !d.on_disk) {
            take(d.model, &mut out);
        }
    }
    out
}

/// The Download all button's line: how many and how big.
pub fn queue_line(q: &[&LocalModel]) -> String {
    let mb: u64 = q.iter().map(|m| m.download_mb).sum();
    match q.len() {
        0 => "Everything that fits is downloaded.".into(),
        1 => format!("1 model, {}.", ls::size_label(mb)),
        n => format!("{n} models, {} in all.", ls::size_label(mb)),
    }
}

/// Why a model's last download stopped. It shows under that model's own row
/// until the model is downloaded again or finishes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowError {
    /// The step that failed: free space, checksum read, download, verify or engine.
    pub step: &'static str,
    /// What happened and what to do next.
    pub message: String,
    /// One short line: the step, plus free and needed space when space is why.
    pub detail: String,
}

fn failed_at(step: Step) -> String {
    format!("Failed at: {}", step.label())
}

/// The row error for how `m`'s download ended; `None` when it finished or
/// the user paused or cancelled it.
pub fn row_error(m: &LocalModel, out: &Outcome) -> Option<RowError> {
    let (step, message) = match out {
        Outcome::Failed {
            step: step @ Step::Download,
            why,
        } => (
            *step,
            format!("{why} Download it again to pick up where it stopped."),
        ),
        Outcome::Failed { step, why } => (*step, why.clone()),
        Outcome::BadChecksum => (
            Step::Verify,
            format!(
                "{} didn't match its published checksum, so it was deleted. Download it again to start over.",
                m.name
            ),
        ),
        Outcome::Done(_) | Outcome::Paused { .. } | Outcome::Cancelled => return None,
    };
    Some(RowError {
        step: step.label(),
        message,
        detail: failed_at(step),
    })
}

/// The row error when the drive hasn't room for `m`: how much is free, how
/// much it needs, and the Remove buttons that make room. Nothing is removed
/// for the user.
pub fn space_error(m: &LocalModel, free_mb: u64, needed_mb: u64) -> RowError {
    RowError {
        step: Step::Space.label(),
        message: format!(
            "Not enough free space for {}. To make room, remove a downloaded model you don't use with the Remove button on its row, then download this one again.",
            m.name
        ),
        detail: format!(
            "{} ({} free, {} needed)",
            failed_at(Step::Space),
            ls::size_label(free_mb),
            ls::size_label(needed_mb)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_setup::CpuSimd;

    fn pc(vram_mb: u64, free_disk_mb: u64) -> Profile {
        Profile {
            gpus_vram_mb: vec![vram_mb],
            unified: false,
            ram_mb: 32_768,
            free_disk_mb,
            simd: CpuSimd::Avx2,
            llama_build: None,
        }
    }

    fn by_id<'a>(rows: &'a [Row], id: &str) -> &'a Row {
        rows.iter().find(|r| r.model.id == id).unwrap()
    }

    const BIG: &str = "gemma-4-31b-it-abliterated-q4_k";
    const MID: &str = "gemma-4-12b-it-abliterated-q4_k";
    const MID_DRAFT: &str = "gemma-4-12b-it-abliterated-mtp";

    #[test]
    fn every_row_is_on_its_tab_and_a_draft_model_fits_as_its_chat_model_does() {
        let rows = rows(&pc(8_192, 500_000), &BenchCache::new(), |_| false);
        // Every row but the speech-to-text model, which isn't offered.
        assert_eq!(rows.len(), ls::local_models().len() - 1);
        let draft = by_id(&rows, MID_DRAFT);
        assert_eq!(draft.tab(), ModelTab::Drafts);
        assert_eq!(draft.fit, by_id(&rows, MID).fit);
        assert_eq!(draft.why, "Comes with Gemma 4 12B.");
        assert_eq!(good_for(draft.model), "Makes Gemma 4 12B reply faster.");
        assert!(row_line(by_id(&rows, MID)).contains(" · Uncensored · "));
        // The picture models and their guard share the Image tab.
        assert_eq!(
            rows.iter().filter(|r| r.tab() == ModelTab::Image).count(),
            3
        );
        assert!(empty_tab_line(ModelTab::Image).starts_with("No image models yet"));
    }

    #[test]
    fn the_speech_to_text_model_shows_only_once_it_is_on_disk() {
        const STT: &str = "parakeet-tdt-0.6b-v3-int8";
        const TTS: &str = "kokoro-82m-multi-lang-v1_0";
        let p = pc(8_192, 500_000);
        let fresh = rows(&p, &BenchCache::new(), |_| false);
        assert!(fresh.iter().all(|r| r.model.id != STT));
        // Spoken replies still offer their model on the Voice tab.
        assert_eq!(by_id(&fresh, TTS).tab(), ModelTab::Voice);
        let voice: Vec<&str> = queue(&fresh, Some(ModelTab::Voice), None)
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(voice, [TTS]);
        // A copy downloaded earlier still lists, so it can be removed.
        let kept = rows(&p, &BenchCache::new(), |m| m.id == STT);
        assert!(by_id(&kept, STT).on_disk);
        assert_eq!(by_id(&kept, STT).tab(), ModelTab::Voice);
    }

    #[test]
    fn download_all_skips_what_wont_run_and_puts_each_draft_model_after_its_chat_model() {
        let rows = rows(&pc(8_192, 500_000), &BenchCache::new(), |_| false);
        assert_eq!(by_id(&rows, BIG).fit, Fit::Tight);
        let q = queue(&rows, None, Some(500_000));
        let ids: Vec<&str> = q.iter().map(|m| m.id).collect();
        let mid = ids.iter().position(|i| *i == MID).unwrap();
        assert_eq!(ids[mid + 1], MID_DRAFT, "{ids:?}");
        assert_eq!(ids.iter().filter(|i| **i == MID_DRAFT).count(), 1);
        let first_tight = ids.iter().position(|i| by_id(&rows, i).fit == Fit::Tight);
        let last_yes = ids
            .iter()
            .rposition(|i| by_id(&rows, i).fit == Fit::Yes && !i.ends_with("-mtp"));
        assert!(first_tight > last_yes, "best fits first: {ids:?}");
        let tiny = super::rows(&pc(2_048, 500_000), &BenchCache::new(), |_| false);
        assert!(queue(&tiny, None, None)
            .iter()
            .all(|m| by_id(&tiny, m.id).fit != Fit::No));
    }

    #[test]
    fn download_all_stops_before_the_disk_runs_out_and_skips_what_is_on_disk() {
        let rows = rows(&pc(24_576, 500_000), &BenchCache::new(), |m| m.id == MID);
        let room = 8_000 + DISK_HEADROOM_MB;
        let q = queue(&rows, Some(ModelTab::Chat), Some(room));
        let mb: u64 = q.iter().map(|m| m.download_mb).sum();
        assert!(mb <= 8_000, "{mb}");
        assert!(q.iter().all(|m| m.id != MID));
        let drafts = queue(&rows, Some(ModelTab::Drafts), None);
        assert_eq!(
            drafts.iter().map(|m| m.id).collect::<Vec<_>>(),
            [MID_DRAFT],
            "on the Drafts tab only a downloaded model's draft model comes"
        );
        assert_eq!(queue_line(&[]), "Everything that fits is downloaded.");
        assert!(queue_line(&q).contains(" in all") || q.len() < 2);
    }
    #[test]
    fn a_failed_download_says_which_step_and_space_says_free_and_needed() {
        let m = ls::model_by_id("gemma-4-31b-it-abliterated-q4_k").unwrap();
        let space = space_error(m, 3_174, 20_890);
        assert_eq!(space.step, "free space");
        assert_eq!(
            space.message,
            "Not enough free space for Gemma 4 31B. To make room, remove a downloaded model you don't use with the Remove button on its row, then download this one again."
        );
        assert_eq!(
            space.detail,
            "Failed at: free space (3.1 GB free, 20.4 GB needed)"
        );
        let read = row_error(
            m,
            &Outcome::Failed {
                step: Step::Checksum,
                why: "Couldn't read the checksum for Gemma 4 31B: timed out".into(),
            },
        )
        .unwrap();
        assert_eq!(
            (read.step, read.message.as_str(), read.detail.as_str()),
            (
                "checksum read",
                "Couldn't read the checksum for Gemma 4 31B: timed out",
                "Failed at: checksum read"
            )
        );
        let cut = row_error(
            m,
            &Outcome::Failed {
                step: Step::Download,
                why: "The download of Gemma 4 31B stopped: reset".into(),
            },
        )
        .unwrap();
        assert_eq!(
            cut.message,
            "The download of Gemma 4 31B stopped: reset Download it again to pick up where it stopped."
        );
        let bad = row_error(m, &Outcome::BadChecksum).unwrap();
        assert_eq!(
            (bad.step, bad.detail.as_str()),
            ("verify", "Failed at: verify")
        );
        // A pause or cancel is the user's own stop, not an error on the row.
        assert_eq!(row_error(m, &Outcome::Paused { done: 5 }), None);
        assert_eq!(row_error(m, &Outcome::Cancelled), None);
    }
}

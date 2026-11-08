//! Round 3 item 32, second half: background compaction. Once a run's prompt
//! passes a soft line, the compaction of what is there so far starts on a
//! worker thread while the run keeps going. At the hard line the finished
//! result is swapped in, with everything added since appended after it, so
//! the person doesn't sit through a compaction mid-task. A job still running
//! at the hard line is waited for, never started again; a job that failed, or
//! a history that changed other than by additions, falls back to the usual
//! compaction there and then.
//!
//! A finished job stays for its chat's next turn, so a summary paid for near
//! the end of one turn is used by the next. `background = false` in
//! `compaction.toml` turns it off.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::{compact_transcript, Compacted, InputItem};
use crate::client::{CancelToken, ModelClient};

/// Percent of the window where a background compaction starts (the hard line
/// is [`super::AUTO_COMPACT_PERCENT`]).
pub const SOFT_COMPACT_PERCENT: u8 = 70;
const _: () = assert!(SOFT_COMPACT_PERCENT < super::AUTO_COMPACT_PERCENT);
/// A Grok chat starts this many tokens under [`super::XAI_COMPACT_CEILING`].
pub const SOFT_PRICE_MARGIN: u64 = 30_000;
/// Jobs kept at once; the oldest goes first.
const JOBS: usize = 16;

/// What each history item is, to tell that the history only grew since the
/// snapshot. Contents may shrink in place (trims, masked images); order,
/// kind, role and call ids may not change.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Shape {
    Message(String),
    Call(String),
    Output(String),
    Other(std::mem::Discriminant<InputItem>),
}

fn shape(item: &InputItem) -> Shape {
    match item {
        InputItem::Message { role, .. } => Shape::Message(role.clone()),
        InputItem::FunctionCall { call_id, .. } => Shape::Call(call_id.clone()),
        InputItem::FunctionCallOutput { call_id, .. } => Shape::Output(call_id.clone()),
        other => Shape::Other(std::mem::discriminant(other)),
    }
}

type Outcome = Result<(Vec<InputItem>, Compacted), String>;

struct Job {
    key: String,
    shapes: Vec<Shape>,
    handle: JoinHandle<Outcome>,
}

fn jobs() -> &'static Mutex<Vec<Job>> {
    static JOBS_LIST: Mutex<Vec<Job>> = Mutex::new(Vec::new());
    &JOBS_LIST
}

/// Whether the background compaction is on (`background = false` turns it off).
pub fn enabled(config_dir: &std::path::Path) -> bool {
    std::fs::read_to_string(config_dir.join(super::COMPACTION_CONFIG))
        .map(|text| super::parse_background(&text))
        .unwrap_or(true)
}

/// The prompt has passed the soft line: the window's [`SOFT_COMPACT_PERCENT`],
/// or near xAI's price step for a Grok model.
pub fn soft_due(model: &str, context_length: u64, trigger: u64, projected: u64) -> bool {
    context_length > 0
        && (crate::tokens::exceeds_threshold(trigger, context_length, SOFT_COMPACT_PERCENT)
            || (super::price_step_model(model)
                && projected >= super::XAI_COMPACT_CEILING.saturating_sub(SOFT_PRICE_MARGIN)))
}

/// A job exists for `key`, running or done.
pub fn has(key: &str) -> bool {
    jobs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|j| j.key == key)
}

/// Everything a background compaction needs, owned so the worker can keep it.
pub struct Start {
    pub key: String,
    pub client: Arc<dyn ModelClient + Send + Sync>,
    pub model: String,
    pub effort: Option<String>,
    pub conversation_id: String,
    pub history: Vec<InputItem>,
    pub modified_paths: Vec<String>,
    pub tools: Vec<serde_json::Value>,
}

fn fits(shapes: &[Shape], history: &[InputItem]) -> bool {
    shapes.len() <= history.len()
        && history[..shapes.len()]
            .iter()
            .map(shape)
            .eq(shapes.iter().cloned())
}

/// Start compacting the history on a worker thread, unless a job for the key
/// already fits it (the history only grew since). A job that no longer fits
/// (a manual `/compact`, a rewound chat) is replaced. `make` is called only
/// when a job starts, so the history is copied once.
pub fn start_unless_fresh(key: &str, history: &[InputItem], make: impl FnOnce() -> Start) {
    let mut list = jobs().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = list.iter().position(|j| j.key == key) {
        if fits(&list[at].shapes, history) {
            return;
        }
        list.remove(at);
    }
    let start = make();
    let shapes = start.history.iter().map(shape).collect();
    // The worker keeps the run's fence, origin and recall lines, as a batch
    // worker does, so its calls are logged like the run's own.
    let fence = crate::tools::fence::capture();
    let origin = crate::harness::current_origin();
    let recall = crate::harness::current_recall();
    let key = start.key.clone();
    let spawned = std::thread::Builder::new()
        .name("grokhub-compact-bg".into())
        .spawn(move || -> Outcome {
            let _fence = crate::tools::fence::enter(&fence);
            let _origin = crate::harness::OriginScope::enter(origin);
            let _recall = crate::harness::RecallScope::enter(recall);
            let mut history = start.history;
            // Its own token: stopping the turn leaves the summary to finish
            // for the next one.
            let cancel = CancelToken::new();
            compact_transcript(
                &*start.client,
                &cancel,
                &start.model,
                start.effort.as_deref(),
                &start.conversation_id,
                &mut history,
                &start.modified_paths,
                &start.tools,
            )
            .map(|done| (history, done))
            .map_err(|e| format!("{e:?}"))
        });
    let Ok(handle) = spawned else {
        return;
    };
    list.push(Job {
        key,
        shapes,
        handle,
    });
    if list.len() > JOBS {
        // The oldest is dropped; its thread finishes on its own.
        list.remove(0);
    }
}

/// The compacted history for `history` from the job for `key`, waiting for it
/// if it is still running: the job's result with everything added since
/// appended. `None` when there is no job, it failed, or the history changed
/// other than by additions. The job is used up either way.
pub fn take(key: &str, history: &[InputItem]) -> Option<(Vec<InputItem>, Compacted)> {
    let job = {
        let mut list = jobs().lock().unwrap_or_else(|e| e.into_inner());
        let at = list.iter().position(|j| j.key == key)?;
        list.remove(at)
    };
    let (mut rebuilt, done) = match job.handle.join() {
        Ok(Ok(out)) => out,
        Ok(Err(why)) => {
            eprintln!("grokhub compact: background compaction failed, compacting now: {why}");
            return None;
        }
        Err(_) => return None,
    };
    if !fits(&job.shapes, history) {
        return None;
    }
    rebuilt.extend_from_slice(&history[job.shapes.len()..]);
    crate::image_budget::apply_image_budget(&mut rebuilt);
    Some((rebuilt, done))
}

/// Drop the job for `key`, if any (its thread finishes on its own).
#[cfg(test)]
pub fn forget(key: &str) {
    jobs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|j| j.key != key);
}

#[cfg(test)]
#[path = "compact_bg_tests.rs"]
mod tests;

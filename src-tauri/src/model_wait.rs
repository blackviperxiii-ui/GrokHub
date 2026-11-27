//! Router: a no-route pause heals itself ([`grokhub_agent::route::wait`]).
//! A chat turn that hit "no model in your plan is answering" waits. A
//! background thread re-checks the models on the router's backoff and, once
//! one answers, the paused chat sends its ask again on its own with one
//! quiet note naming the provider and the step. A Pulse card names the
//! provider (never the model) and the step, with Retry now. Only models in
//! your plan are pinged, on the Grok sign-in: waiting never spends money on
//! a model outside it. The budget pause is not this: it still waits for you.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use grokhub_agent::route::live::NO_ROUTE_MSG;
use grokhub_agent::route::local::OFFLINE_NO_LOCAL_MSG;
use grokhub_agent::route::wait::{self, NoRouteWait};
use grokhub_core::model_registry::ModelMeta;
use serde::Serialize;

/// Tells the window a paused chat can send its ask again.
pub const MODEL_BACK_EVENT: &str = "router://model-back";

/// The same model pausing again this soon after a wait ended keeps its backoff place.
const CARRY_FOR_MS: u64 = 600_000;

/// How often the background thread looks.
const LOOK_EVERY: Duration = Duration::from_secs(5);

/// The card's Retry now button, on the Pulse card.
pub const RETRY_LABEL: &str = "Retry now";
pub const RETRYING: &str = "Checking again now…";

/// A paused chat's ask, to send again once a model answers.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Paused {
    chat: String,
    ask: String,
    /// The turn paused offline with no local model: its note says reconnect.
    offline: bool,
}

/// What a paused chat sends again, and the note it shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resume {
    pub conversation_id: String,
    pub ask: String,
    pub note: String,
}

#[derive(Debug, Default)]
struct Waiter {
    wait: Option<NoRouteWait>,
    /// The waiting card's id in the Pulse store.
    card: Option<String>,
    /// Every chat paused on the wait, one ask each (its newest).
    paused: Vec<Paused>,
    /// A model came back: each paused chat sends its ask once, when it is open and idle.
    resume: Vec<Resume>,
    /// (model, attempts, when the wait ended).
    carry: Option<(String, u32, u64)>,
    /// A check is out now.
    checking: bool,
}

static WAITER: Mutex<Waiter> = Mutex::new(Waiter {
    wait: None,
    card: None,
    paused: Vec::new(),
    resume: Vec::new(),
    carry: None,
    checking: false,
});

fn waiter() -> std::sync::MutexGuard<'static, Waiter> {
    WAITER.lock().unwrap_or_else(|e| e.into_inner())
}

/// A turn that waits for a model (or the network) and resumes on its own.
pub fn is_waiting_turn(text: &str) -> bool {
    text.contains(*NO_ROUTE_MSG) || text.contains(OFFLINE_NO_LOCAL_MSG)
}

/// The note when a turn kept while offline gets its answer.
pub fn reconnect_note(step: &str) -> String {
    format!("Back online: answered after reconnect ({step})")
}

/// A chat turn ended with `err`: when it was a no-route pause, remember the
/// chat and its ask, and start the wait. True when the turn is waiting.
pub fn turn_failed(config_dir: &Path, chat: &str, ask: &str, err: &str, now: u64) -> bool {
    if !is_waiting_turn(err) || ask.trim().is_empty() {
        return false;
    }
    let mut w = waiter();
    w.paused.retain(|p| p.chat != chat);
    w.paused.push(Paused {
        chat: chat.to_string(),
        ask: ask.to_string(),
        offline: err.contains(OFFLINE_NO_LOCAL_MSG),
    });
    drop(w);
    observe(config_dir, now, &since_label(now));
    true
}

/// Take the router's newest pause and start the wait on it.
pub fn observe(config_dir: &Path, now: u64, since: &str) {
    if let Some((class, model, _)) = grokhub_agent::route::live::take_no_route() {
        start(config_dir, &class, &model, now, since);
    }
}

/// No model in your plan answered `class`'s call to `model`: wait, post the card.
pub fn start(config_dir: &Path, class: &str, model: &str, now: u64, since: &str) {
    let model = model.trim();
    let mut w = waiter();
    if w.wait.is_some() {
        return;
    }
    let ask = match w.paused.last() {
        Some(p) if class.starts_with("chat:") => p.ask.clone(),
        _ => String::new(),
    };
    let step = wait::step_name(class, &ask);
    let attempts = match &w.carry {
        Some((m, a, at)) if m == model && now.saturating_sub(*at) < CARRY_FOR_MS => *a,
        _ => 0,
    };
    let card = wait_card(model, since, &step, now);
    let id = card.id.clone();
    let posted = crate::pulse::edit_cards(config_dir, &mut |cards| {
        // One waiting card at a time: an older one left behind goes first.
        cards.retain(|c| !grokhub_core::is_model_wait_card(c));
        usize::from(grokhub_core::post_update(cards, card.clone()))
    });
    w.card = matches!(posted, Ok(n) if n > 0).then_some(id);
    w.wait = Some(NoRouteWait::new(class, model, &step, now, attempts));
}

/// Retry now on the waiting card. False when nothing waits.
pub fn retry_now(now: u64) -> bool {
    match waiter().wait.as_mut() {
        Some(w) => {
            w.retry_now(now);
            true
        }
        None => false,
    }
}

/// The model the wait is on, when a check is due and none is out; marks one out.
fn due_check(now: u64) -> Option<String> {
    let mut w = waiter();
    if w.checking {
        return None;
    }
    let model = w.wait.as_ref().filter(|x| x.due(now))?.model.clone();
    w.checking = true;
    Some(model)
}

/// A check came back: `back` is the model that answers, or `None`.
/// Returns the resume the window is told about.
pub fn checked(config_dir: &Path, back: Option<&str>, now: u64) -> Option<Resume> {
    let mut w = waiter();
    w.checking = false;
    let Some(back) = back else {
        if let Some(x) = w.wait.as_mut() {
            x.missed(now);
        }
        return None;
    };
    let done = w.wait.take()?;
    w.carry = Some((done.model.clone(), done.attempts.saturating_add(1), now));
    if let Some(id) = w.card.take() {
        let _ = crate::pulse::edit_cards(config_dir, &mut |cards| {
            let before = cards.len();
            cards.retain(|c| c.id != id);
            // Saved when the card went: the count is the number removed.
            before - cards.len()
        });
    }
    let paused = std::mem::take(&mut w.paused);
    let resumes: Vec<Resume> = paused
        .into_iter()
        .map(|p| {
            // Each chat's note names its own step.
            let step = wait::step_name(&done.class, &p.ask);
            let note = if p.offline {
                reconnect_note(&step)
            } else {
                wait::back_note(back, &step, true)
            };
            Resume {
                conversation_id: p.chat,
                ask: p.ask,
                note,
            }
        })
        .collect();
    w.resume.retain(|r| {
        !resumes
            .iter()
            .any(|n| n.conversation_id == r.conversation_id)
    });
    w.resume.extend(resumes.iter().cloned());
    resumes.into_iter().next()
}

/// The paused chat, once it is open and idle: its ask goes out once, with the note.
pub fn take_resume(chat: &str) -> Option<Resume> {
    let mut w = waiter();
    let at = w.resume.iter().position(|r| r.conversation_id == chat)?;
    Some(w.resume.remove(at))
}

/// One look: start a wait the router reported, and run a check when due.
/// `ping` sends one small call to a resting model.
pub fn tick(
    config_dir: &Path,
    now: u64,
    since: &str,
    ping: &mut dyn FnMut(&ModelMeta) -> bool,
) -> Option<Resume> {
    observe(config_dir, now, since);
    let model = due_check(now)?;
    let back = wait::recheck(config_dir, &model, now, ping);
    checked(config_dir, back.as_deref(), now)
}

/// The waiting card. It names the provider, never the model.
pub fn wait_card(model: &str, since: &str, step: &str, now: u64) -> grokhub_core::UpdateCard {
    let (title, text) = wait::waiting_card(model, since, step);
    grokhub_core::router_update_card(
        &format!("{}{model}", grokhub_core::MODEL_WAIT_SOURCE_PREFIX),
        &title,
        &text,
        now,
    )
}

/// "3:05 PM" at the window's clock, or UTC before the window sent it.
fn since_label(now: u64) -> String {
    #[cfg(desktop)]
    let offset = crate::scheduler::utc_offset().unwrap_or(0);
    #[cfg(not(desktop))]
    let offset = 0;
    let clock = grokhub_core::LocalClock::at_offset(now, offset);
    grokhub_core::automation::format_ampm(clock.hour * 60 + clock.minute)
}

/// Retry now is on this card: a no-route wait's own.
pub fn is_wait_card(config_dir: &Path, id: &str) -> Result<bool, String> {
    Ok(crate::pulse::cards(config_dir)?
        .iter()
        .any(|c| c.id == id && grokhub_core::is_model_wait_card(c)))
}

/// The background thread: the router's pauses, the backoff checks, and the
/// window told when a paused chat can go on.
pub fn start_watch(config_dir: std::path::PathBuf, mut told: impl FnMut(&Resume) + Send + 'static) {
    std::thread::spawn(move || loop {
        std::thread::sleep(LOOK_EVERY);
        let now = grokhub_core::now_ms();
        let dir = config_dir.clone();
        // Pings go on the plan sign-in only: an included call, never money.
        // The keychain is read only when a resting model is pinged.
        let mut bearer: Option<Option<String>> = None;
        let mut ping = |m: &ModelMeta| {
            bearer
                .get_or_insert_with(|| crate::oauth::bearer(&crate::oauth::Keychain).ok().flatten())
                .as_deref()
                .is_some_and(|b| grokhub_agent::route::sources::ping_model(&dir, b, m))
        };
        if let Some(resume) = tick(&config_dir, now, &since_label(now), &mut ping) {
            told(&resume);
        }
    });
}

#[cfg(test)]
#[path = "model_wait_tests.rs"]
mod tests;

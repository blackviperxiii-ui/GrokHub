//! Settings → AI models → Sign in with OpenRouter. OpenRouter's browser
//! PKCE flow (`providers::openrouter_key`, a 127.0.0.1 callback) buys a new
//! key, and the key lands on the same credentials card as a pasted one: it
//! is saved only on Save, and the provider stays off until its own Allow
//! card. The key goes from the sign-in thread straight to that card's
//! waiter; it is never returned, shown or logged.
//!
//! A browser that doesn't open still finishes: the page shows the link to
//! open by hand, and the callback completes the sign-in. One sign-in runs at
//! a time. A page that comes back while it waits picks it up again
//! ([`status`] on load, and [`start`] hands back the same link), and only the
//! newest [`wait`] gets its card, so a page that was left never takes it. A
//! card a reloaded page never saw is still in [`status`]. [`stop`] ends it
//! early.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use grokhub_agent::route::providers;
use serde::Serialize;

use crate::ai_models::{self, Card};

/// Wait for a sign-in with no answer from the browser.
pub const SIGN_IN_WAIT: Duration = Duration::from_secs(300);
/// Wait asked for with no sign-in started.
pub const NO_SIGN_IN: &str = "Start signing in first.";
/// A newer page is waiting for this sign-in, so this wait ends quietly.
pub const WAITED_ELSEWHERE: &str = "This sign-in moved to a newer page.";
/// What the sign-in ends with after [`stop`].
pub const STOPPED: &str = "OpenRouter sign-in stopped";
/// A start while a stopped sign-in is still ending.
const STILL_STOPPING: &str = "The last sign-in is still stopping. Try again in a moment.";
/// The flow tried to open a page that isn't OpenRouter's sign-in page.
const NOT_THE_PAGE: &str = "That page isn't OpenRouter's sign-in page, so it wasn't opened.";
/// How long the start waits for the flow to reach the browser.
const START_WAIT: Duration = Duration::from_secs(5);

/// The sign-in has reached the browser. `opened` false: the page shows `url`
/// to open by hand, with `why`. The sign-in keeps waiting either way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInStarted {
    pub url: String,
    pub opened: bool,
    pub why: String,
}

/// The one sign-in at a time.
struct Running {
    /// What [`start`] first said, for a page that picks the sign-in up again.
    started: SignInStarted,
    stop: Arc<AtomicBool>,
    /// Ends with the key card, or why there is none.
    flow: JoinHandle<Result<Card, String>>,
}

static SIGN_IN: Mutex<Option<Running>> = Mutex::new(None);
/// The newest [`wait`]; an older one ends with [`WAITED_ELSEWHERE`].
static NEWEST_WAIT: AtomicU64 = AtomicU64::new(0);
/// The last card a [`wait`] handed out, for a page reloaded before it showed.
static LAST_CARD: Mutex<Option<Card>> = Mutex::new(None);

/// What a page that just loaded picks up: a sign-in that waits, or the card
/// of one that ended and is still unanswered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInStatus {
    pub waiting: Option<SignInStarted>,
    pub card: Option<Card>,
}

/// Only OpenRouter's own sign-in page opens from this flow.
pub fn sign_in_page_ok(url: &str) -> bool {
    crate::opener::url_ok(url)
        && url
            .strip_prefix("https://openrouter.ai/auth?")
            .is_some_and(|query| !query.is_empty())
}

/// Start signing in: OpenRouter's page opens with `open`. While a sign-in
/// waits, a start opens nothing and hands back that sign-in's link. Offline
/// only opens no page and takes no port.
pub fn start(
    config_dir: &Path,
    open: impl Fn(&str) -> Result<(), String> + Send + 'static,
) -> Result<SignInStarted, String> {
    start_within(config_dir, open, SIGN_IN_WAIT)
}

/// [`start`] with the time the browser has to call back.
fn start_within(
    config_dir: &Path,
    open: impl Fn(&str) -> Result<(), String> + Send + 'static,
    wait: Duration,
) -> Result<SignInStarted, String> {
    // Offline only: no page, no port and no thread.
    grokhub_core::offline::check_url(providers::OPENROUTER_BASE).map_err(String::from)?;
    let mut slot = SIGN_IN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(running) = slot.as_ref() {
        if !running.flow.is_finished() {
            return if running.stop.load(Ordering::SeqCst) {
                Err(STILL_STOPPING.into())
            } else {
                Ok(running.started.clone())
            };
        }
    }
    // A sign-in that ended with nobody waiting: its card has no page left.
    if let Some(ended) = slot.take() {
        if let Ok(Ok(card)) = ended.flow.join() {
            let _ = ai_models::answer(&card.id, false);
        }
    }
    let (tell, told) = mpsc::channel::<(String, Result<(), String>)>();
    let stop = Arc::new(AtomicBool::new(false));
    let config = config_dir.to_path_buf();
    let flow = std::thread::spawn({
        let stop = stop.clone();
        move || {
            let _pin = grokhub_agent::perm::ConfigGuard::set(config.clone());
            let launch = |url: &str| {
                let opened = if sign_in_page_ok(url) {
                    open(url)
                } else {
                    Err(NOT_THE_PAGE.to_string())
                };
                let _ = tell.send((url.to_string(), opened));
                // A browser that didn't open still gets the link by hand, so
                // the flow keeps waiting for the callback.
                Ok(())
            };
            let stopped = || stop.load(Ordering::SeqCst);
            let key = providers::openrouter_key(&config, &launch, wait, &stopped)?;
            // Stopped while the key was on its way: no card for a page that left.
            if stopped() {
                return Err(STOPPED.into());
            }
            ai_models::start_add(&config, providers::OPENROUTER_BASE, key.to_string())
        }
    });
    match told.recv_timeout(START_WAIT) {
        Ok((url, opened)) => {
            let started = SignInStarted {
                url,
                opened: opened.is_ok(),
                why: opened.err().unwrap_or_default(),
            };
            *slot = Some(Running {
                started: started.clone(),
                stop,
                flow,
            });
            Ok(started)
        }
        // The flow ended before any page: no port, or Offline only.
        Err(mpsc::RecvTimeoutError::Disconnected) => match flow.join() {
            Ok(Err(why)) => Err(why),
            _ => Err("The sign-in couldn't start.".into()),
        },
        // Nothing reached the page, so nothing may keep waiting.
        Err(mpsc::RecvTimeoutError::Timeout) => {
            stop.store(true, Ordering::SeqCst);
            *slot = Some(Running {
                started: SignInStarted {
                    url: String::new(),
                    opened: false,
                    why: String::new(),
                },
                stop,
                flow,
            });
            Err("The sign-in couldn't reach the browser. Try again.".into())
        }
    }
}

/// Wait for the sign-in to end: the credentials card for the new key (same
/// as a pasted key; nothing is saved until Save), or why there is none. A
/// newer wait takes over, and this one ends with [`WAITED_ELSEWHERE`]. With
/// no sign-in left, a card an older wait took and nobody answered is handed
/// over again.
pub fn wait() -> Result<Card, String> {
    let me = NEWEST_WAIT.fetch_add(1, Ordering::SeqCst) + 1;
    loop {
        {
            let mut slot = SIGN_IN.lock().unwrap_or_else(|e| e.into_inner());
            if NEWEST_WAIT.load(Ordering::SeqCst) != me {
                return Err(WAITED_ELSEWHERE.into());
            }
            match slot.as_ref() {
                // An older wait took the card first, for a page that is gone.
                None => return open_card().ok_or_else(|| NO_SIGN_IN.into()),
                Some(running) if running.flow.is_finished() => {
                    let ended = slot.take().ok_or(NO_SIGN_IN)?;
                    let card = ended
                        .flow
                        .join()
                        .unwrap_or_else(|_| Err("The sign-in stopped.".into()));
                    if let Ok(card) = &card {
                        *LAST_CARD.lock().unwrap_or_else(|e| e.into_inner()) = Some(card.clone());
                    }
                    return card;
                }
                Some(_) => {}
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A sign-in to pick up on load: one that waits (or ended with nobody
/// waiting; [`wait`] hands it over), or a card still unanswered.
pub fn status() -> SignInStatus {
    let waiting = SIGN_IN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .filter(|running| !running.stop.load(Ordering::SeqCst))
        .map(|running| running.started.clone());
    SignInStatus {
        waiting,
        card: open_card(),
    }
}

/// The last card a wait handed out, while it is still unanswered.
fn open_card() -> Option<Card> {
    LAST_CARD
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .filter(|card| ai_models::card_open(&card.id))
}

/// Stop the sign-in that waits: its port closes and no card is posted. The
/// page's [`wait`] ends with [`STOPPED`]. Nothing to stop is fine.
pub fn stop() {
    if let Some(running) = SIGN_IN.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        running.stop.store(true, Ordering::SeqCst);
    }
}

/// The sign-in's flow has ended (tests wait on it instead of sleeping).
#[cfg(test)]
fn flow_ended() -> bool {
    SIGN_IN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|running| running.flow.is_finished())
}

#[cfg(test)]
#[path = "openrouter_sign_in_tests.rs"]
mod tests;

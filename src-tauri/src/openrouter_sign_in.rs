//! Settings → AI models → Sign in with OpenRouter. OpenRouter's browser
//! PKCE flow (`providers::openrouter_key`, a 127.0.0.1 callback) buys a new
//! key, and the key lands on the same credentials card as a pasted one: it
//! is saved only on Save, and the provider stays off until its own Allow
//! card. The key goes from the sign-in thread straight to that card's
//! waiter; it is never returned, shown or logged.
//!
//! A browser that doesn't open still finishes: the page shows the link to
//! open by hand, and the callback completes the sign-in.

use std::path::Path;
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use grokhub_agent::route::providers;
use serde::Serialize;

use crate::ai_models::{self, Card};

/// What the page says while the browser is open.
pub const OPENROUTER_WAIT: &str = "Waiting for OpenRouter. Finish signing in in your browser.";
/// Wait for a sign-in with no answer from the browser.
pub const SIGN_IN_WAIT: Duration = Duration::from_secs(300);
/// Wait asked for with no sign-in started.
pub const NO_SIGN_IN: &str = "Start signing in first.";
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

/// The one sign-in at a time. It ends with the key card, or why there is none.
static SIGN_IN: Mutex<Option<JoinHandle<Result<Card, String>>>> = Mutex::new(None);

/// Only OpenRouter's own sign-in page opens from this flow.
pub fn sign_in_page_ok(url: &str) -> bool {
    crate::opener::url_ok(url)
        && url
            .strip_prefix("https://openrouter.ai/auth?")
            .is_some_and(|query| !query.is_empty())
}

/// Start signing in: OpenRouter's page opens with `open`. A second start
/// while one waits is refused, as is one while Offline only is on (no page
/// opens, no port is taken).
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
    if slot.as_ref().is_some_and(|running| !running.is_finished()) {
        return Err(OPENROUTER_WAIT.into());
    }
    let (tell, told) = mpsc::channel::<(String, Result<(), String>)>();
    let config = config_dir.to_path_buf();
    let flow = std::thread::spawn(move || {
        let _pin = grokhub_agent::perm::ConfigGuard::set(config.clone());
        let launch = |url: &str| {
            let opened = if sign_in_page_ok(url) {
                open(url)
            } else {
                Err(NOT_THE_PAGE.to_string())
            };
            let _ = tell.send((url.to_string(), opened));
            // A browser that didn't open still gets the link by hand, so the
            // flow keeps waiting for the callback.
            Ok(())
        };
        let key = providers::openrouter_key(&config, &launch, wait)?;
        ai_models::start_add(&config, providers::OPENROUTER_BASE, key.to_string())
    });
    match told.recv_timeout(START_WAIT) {
        Ok((url, opened)) => {
            *slot = Some(flow);
            Ok(SignInStarted {
                url,
                opened: opened.is_ok(),
                why: opened.err().unwrap_or_default(),
            })
        }
        // The flow ended before any page: no port, or Offline only.
        Err(mpsc::RecvTimeoutError::Disconnected) => match flow.join() {
            Ok(Err(why)) => Err(why),
            _ => Err("The sign-in couldn't start.".into()),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => {
            *slot = Some(flow);
            Err("The sign-in couldn't reach the browser. Try again.".into())
        }
    }
}

/// Wait for the sign-in to end: the credentials card for the new key (same
/// as a pasted key; nothing is saved until Save), or why there is none.
pub fn wait() -> Result<Card, String> {
    loop {
        {
            let mut slot = SIGN_IN.lock().unwrap_or_else(|e| e.into_inner());
            match slot.as_ref() {
                None => return Err(NO_SIGN_IN.into()),
                Some(running) if running.is_finished() => {
                    let flow = slot.take().ok_or(NO_SIGN_IN)?;
                    return flow
                        .join()
                        .unwrap_or_else(|_| Err("The sign-in stopped.".into()));
                }
                // Still waiting: a second start stays refused meanwhile.
                Some(_) => {}
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
#[path = "openrouter_sign_in_tests.rs"]
mod tests;

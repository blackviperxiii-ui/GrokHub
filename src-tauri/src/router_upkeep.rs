//! Runs the router's registry upkeep (`grokhub_agent::route::upkeep`) on one
//! background thread for the life of the app: without it the router has no
//! on-device tier or added provider to pick from.

use std::path::PathBuf;
use std::time::Duration;

use grokhub_agent::route::upkeep::{SignIn, Upkeep};
use grokhub_core::model_registry::Credential;

use crate::{credential, oauth};

/// How often the upkeep looks: it folds health each minute and refreshes
/// only when one is due.
const BEAT: Duration = Duration::from_secs(60);

pub fn start(config_dir: PathBuf) {
    let started = std::thread::Builder::new()
        .name("router-upkeep".into())
        .spawn(move || {
            let mut upkeep = Upkeep::new(grokhub_core::now_ms());
            loop {
                upkeep.tick(&config_dir, grokhub_core::now_ms(), &sign_in());
                std::thread::sleep(BEAT);
            }
        });
    if let Err(err) = started {
        eprintln!("router upkeep: {err}");
    }
}

/// The app's own xAI sign-in, as a chat turn reads it: a live Grok sign-in
/// wins over a saved key. None under the loopback test server.
fn sign_in() -> SignIn {
    if credential::test_responses_url().is_some() {
        return None;
    }
    match oauth::bearer(&oauth::Keychain) {
        Ok(Some(bearer)) => Some((bearer, Credential::Plan)),
        _ => credential::load_key()
            .ok()
            .map(|key| (key, Credential::ApiKey)),
    }
}

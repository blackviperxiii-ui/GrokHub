//! Runs the router's registry upkeep (`grokhub_agent::route::upkeep`) on one
//! background thread for the life of the app: without it the router has no
//! on-device tier or added provider to pick from. What it has to tell (a
//! plan lost models, new models joined) goes on Home as an update card.

use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_agent::route::heal::HealMsg;
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
                let ran = upkeep.tick(&config_dir, grokhub_core::now_ms(), &sign_in());
                post(&config_dir, &ran.notices, grokhub_core::now_ms());
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

/// Each notice as a Home update card (the same key merges into its card).
/// Returns how many posted or merged.
fn post(config_dir: &Path, notices: &[HealMsg], now_ms: u64) -> usize {
    if notices.is_empty() {
        return 0;
    }
    crate::pulse::edit_cards(config_dir, &mut |cards| {
        notices
            .iter()
            .map(|m| grokhub_core::router_update_card(&m.key, &m.title, &m.text, now_ms))
            .filter(|card| grokhub_core::post_update(cards, card.clone()))
            .count()
    })
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_lands_on_home_as_one_card() {
        let dir =
            std::env::temp_dir().join(format!("grokhub-router-upkeep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let notice = HealMsg {
            key: "new:grok-5".into(),
            title: "New in your plan".into(),
            text: "Your plan has a new AI model. Auto uses it where it does best.".into(),
        };
        assert_eq!(post(&dir, std::slice::from_ref(&notice), 1_000), 1);
        let cards = crate::pulse::cards(&dir).unwrap();
        assert_eq!(
            cards
                .iter()
                .map(|c| (c.title.as_str(), c.body.as_deref()))
                .collect::<Vec<_>>(),
            [(
                "New in your plan",
                Some("Your plan has a new AI model. Auto uses it where it does best.")
            )]
        );
        post(&dir, &[notice], 2_000);
        assert_eq!(
            crate::pulse::cards(&dir).unwrap().len(),
            1,
            "the same update merges into its card"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

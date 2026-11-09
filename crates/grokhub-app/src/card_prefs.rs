//! `card_prefs.json` next to `updates.json`. The core store is pure; this module writes it.

use crate::config;

pub fn load() -> grokhub_core::CardPrefs {
    let dir = config::config_dir();
    let mut prefs = grokhub_core::load_card_prefs(&dir);
    prefs.taste = grokhub_core::card_taste::cached_weights(&dir);
    prefs
}

pub fn save(prefs: &grokhub_core::CardPrefs) -> Result<(), String> {
    let body = grokhub_core::card_prefs_json(prefs)?;
    config::atomic_write(
        &config::config_dir().join("card_prefs.json"),
        body.as_bytes(),
    )
}

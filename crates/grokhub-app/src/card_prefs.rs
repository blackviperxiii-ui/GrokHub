//! `card_prefs.json` next to `updates.json`. The core store is pure; this module writes it.

use crate::config;

pub fn load() -> grokhub_core::CardPrefs {
    grokhub_core::load_card_prefs(&config::config_dir())
}

pub fn save(prefs: &grokhub_core::CardPrefs) -> Result<(), String> {
    let body = grokhub_core::card_prefs_json(prefs)?;
    config::atomic_write(&config::config_dir().join("card_prefs.json"), body.as_bytes())
}

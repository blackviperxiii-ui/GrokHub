//! Settings → Models & providers → Web search.
//!
//! Cabin-executed search for OpenRouter and the local model. Default is free
//! DuckDuckGo Lite (no key). Optional SearXNG URL or Brave bring-your-own key
//! (keyring only). xAI keeps hosted search.

use grokhub_agent::{brave_key_present, seal_brave_key};

use super::*;

const BACKENDS: [(&str, &str); 3] = [
    ("duckduckgo", "DuckDuckGo Lite (free, no key)"),
    ("searxng", "SearXNG (self-hosted URL)"),
    ("brave", "Brave Search (your API key)"),
];

impl Cabin {
    pub(super) fn ui_web_search_rows(&mut self, ui: &mut egui::Ui) {
        crate::ui_kit::section_header(ui, "Web search", None);
        crate::cards::settings_note(
            ui,
            "For OpenRouter and the local model. Free DuckDuckGo Lite by default — no account. Grok still uses xAI hosted search. Pair results with web_fetch.",
        );
        let labels: Vec<String> = BACKENDS.iter().map(|(_, l)| (*l).to_string()).collect();
        let cur = BACKENDS
            .iter()
            .find(|(id, _)| *id == self.cfg.web_search.backend)
            .map(|(_, l)| (*l).to_string())
            .unwrap_or_else(|| BACKENDS[0].1.to_string());
        if let Some(i) = crate::cards::settings_dropdown(
            ui,
            "Search backend",
            "Default stays free. SearXNG needs your instance URL. Brave needs a key in your keyring.",
            &cur,
            &labels,
        ) {
            if let Some((id, _)) = BACKENDS.get(i) {
                if *id != self.cfg.web_search.backend {
                    self.cfg.web_search.backend = (*id).to_string();
                    self.persist_cfg();
                    self.status = "Saved".into();
                }
            }
        }
        if self.cfg.web_search.backend == "searxng" {
            let before = self.cfg.web_search.searxng_url.clone();
            crate::cards::settings_field(
                ui,
                "SearXNG URL",
                "https://searx.example — must expose the JSON API (?format=json).",
                &mut self.cfg.web_search.searxng_url,
                false,
            );
            if self.cfg.web_search.searxng_url != before {
                self.cfg.web_search.searxng_url =
                    self.cfg.web_search.searxng_url.trim().to_string();
                self.persist_cfg();
                self.status = "Saved".into();
            }
        }
        if self.cfg.web_search.backend == "brave" {
            let has = brave_key_present();
            let hint = if has {
                "A Brave Search key is in your keyring. Clear the field and Save to remove it."
            } else {
                "Paste a Brave Search API key. It stays in your keyring, never in app.json."
            };
            crate::cards::settings_field(
                ui,
                "Brave API key",
                hint,
                &mut self.web_search_key_draft,
                true,
            );
            if crate::cards::settings_action(ui, "Brave API key", hint, "Save key") {
                match seal_brave_key(&self.web_search_key_draft) {
                    Ok(()) => {
                        self.web_search_key_draft.clear();
                        self.status = if brave_key_present() {
                            "Brave search key saved".into()
                        } else {
                            "Brave search key cleared".into()
                        };
                    }
                    Err(e) => self.status = e,
                }
            }
        }
    }
}

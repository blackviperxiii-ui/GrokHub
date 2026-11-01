//! Guild Wars 2 (`grokhub_core::gw2_api`).
//!
//! Under the game watcher in Settings: a field for a read-only ArenaNet API
//! key, kept in the system keychain and nowhere else, and what the key let
//! Loadout read. With a key, the account is refreshed off the UI thread
//! every ten minutes and on Refresh, each call through the egress guard (so
//! Offline only stops it), and the last good snapshot is kept on disk
//! without the key. Nothing touches the game.

use std::sync::mpsc;

use super::*;
use grokhub_core::gw2_api::{self as gw2, FetchError, GetError, Gw2Account};

const KEYRING_SERVICE: &str = "GrokHub";
const KEYRING_ACCOUNT: &str = "gw2-api-key";

#[derive(Default)]
pub(super) struct Gw2Ui {
    /// Read once: is a key in the keychain.
    has_key: Option<bool>,
    /// The key field's text; cleared once saved.
    draft: String,
    account: Option<Gw2Account>,
    loaded_cache: bool,
    /// A refresh running on its own thread.
    pending: Option<mpsc::Receiver<Result<Gw2Account, FetchError>>>,
    /// Don't start a refresh before this.
    next_ms: u64,
    note: String,
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(|e| e.to_string())
}

fn load_key() -> Option<String> {
    if cfg!(test) {
        return None;
    }
    match keyring_entry().ok()?.get_password() {
        Ok(k) if !k.trim().is_empty() => Some(k),
        _ => None,
    }
}

fn store_key(key: Option<&str>) -> Result<(), String> {
    if cfg!(test) {
        return Ok(());
    }
    let entry = keyring_entry()?;
    match key {
        Some(k) => entry.set_password(k).map_err(|e| e.to_string()),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        },
    }
}

fn api_agent() -> grokhub_core::http::Agent {
    // No redirects, public addresses only (and nothing while Offline only
    // is on), short caps.
    grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: std::time::Duration::from_secs(8),
        overall: Some(std::time::Duration::from_secs(20)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    })
}

/// One GET to ArenaNet's API with the key in the Authorization header,
/// never in the URL.
fn api_get(agent: &grokhub_core::http::Agent, url: &str, key: &str) -> Result<String, GetError> {
    if cfg!(test) || !url.starts_with(gw2::API_BASE) {
        return Err(GetError::Other("Not sent.".into()));
    }
    crate::xai::egress_ok(url, &[]).map_err(|_| {
        GetError::Other(
            "Offline only or the network guard held the Guild Wars 2 refresh back.".into(),
        )
    })?;
    let resp = agent
        .get(url)
        .header("authorization", &format!("Bearer {key}"))
        .call()
        .map_err(|e| match e {
            grokhub_core::http::Error::Status {
                code, retry_after, ..
            } => GetError::Status {
                code,
                retry_after_s: retry_after.and_then(|r| r.trim().parse().ok()),
            },
            _ => GetError::Other("Couldn't reach ArenaNet's API.".into()),
        })?;
    let mut body = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(resp.into_reader(), gw2::MAX_BODY),
        &mut body,
    )
    .map_err(|_| GetError::Other("ArenaNet's API answer didn't arrive whole.".into()))?;
    Ok(body)
}

impl Cabin {
    /// Each frame: load the snapshot and check for a key once, start a
    /// refresh when one is due, and pick up a finished one.
    pub(super) fn tick_gw2(&mut self) {
        if !self.focus.gw2.loaded_cache {
            self.focus.gw2.loaded_cache = true;
            self.focus.gw2.account = gw2::load_cache(&crate::config::config_dir());
        }
        if let Some(rx) = &self.focus.gw2.pending {
            match rx.try_recv() {
                Ok(done) => {
                    self.focus.gw2.pending = None;
                    self.finish_gw2(done, now_ms());
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => self.focus.gw2.pending = None,
            }
        }
        if !self.cfg.game_watch || now_ms() < self.focus.gw2.next_ms {
            return;
        }
        if *self
            .focus
            .gw2
            .has_key
            .get_or_insert_with(|| load_key().is_some())
        {
            self.start_gw2_refresh();
        }
    }

    fn start_gw2_refresh(&mut self) {
        let g = &mut self.focus.gw2;
        if g.pending.is_some() {
            return;
        }
        g.next_ms = now_ms() + gw2::REFRESH_MS;
        let Some(key) = load_key() else {
            g.has_key = Some(false);
            return;
        };
        let (tx, rx) = mpsc::channel();
        g.pending = Some(rx);
        std::thread::spawn(move || {
            let agent = api_agent();
            let got = gw2::fetch_account(&mut |url| api_get(&agent, url, &key), now_ms());
            let _ = tx.send(got);
        });
    }

    fn finish_gw2(&mut self, done: Result<Gw2Account, FetchError>, now: u64) {
        let g = &mut self.focus.gw2;
        match done {
            Ok(acct) => {
                if !cfg!(test) {
                    let _ = gw2::save_cache(&crate::config::config_dir(), &acct);
                }
                g.note = if acct.missing.is_empty() {
                    String::new()
                } else {
                    format!(
                        "The key has no {} scope, so that part isn't read.",
                        acct.missing.join(", ")
                    )
                };
                g.account = Some(acct);
            }
            Err(e) => {
                if let FetchError::RateLimited { retry_after_s } = &e {
                    g.next_ms = now + retry_after_s * 1000;
                }
                g.note = e.message();
            }
        }
    }

    /// The Guild Wars 2 rows under the game watcher.
    pub(super) fn ui_gw2(&mut self, ui: &mut egui::Ui) {
        if !self.cfg.game_watch {
            return;
        }
        let has_key = *self
            .focus
            .gw2
            .has_key
            .get_or_insert_with(|| load_key().is_some());
        let line = gw2_status(self.focus.gw2.account.as_ref(), has_key, now_ms());
        let note = self.focus.gw2.note.clone();
        let busy = self.focus.gw2.pending.is_some();
        let mut save = false;
        let mut refresh = false;
        let mut remove = false;
        ui.indent("gw2-api", |ui| {
            crate::cards::help_text(ui, &line);
            if has_key {
                ui.horizontal(|ui| {
                    if busy {
                        crate::cards::help_text(ui, "Refreshing...");
                    } else {
                        refresh = crate::cards::ghost_pill(ui, "Refresh Guild Wars 2");
                    }
                    remove = crate::cards::ghost_pill(ui, "Remove key");
                });
            } else {
                let hint = "Make a key at account.arena.net under Applications with the account, characters, progression and wallet scopes. It stays in your system keychain and only reads.";
                crate::cards::settings_field(
                    ui,
                    "Guild Wars 2 API key",
                    hint,
                    &mut self.focus.gw2.draft,
                    true,
                );
                save = crate::cards::settings_action(ui, "Guild Wars 2 API key", hint, "Save key");
            }
            if !note.is_empty() {
                crate::cards::help_text(ui, &note);
            }
        });
        if save {
            self.save_gw2_key();
        }
        if refresh {
            self.focus.gw2.next_ms = 0;
            self.start_gw2_refresh();
        }
        if remove {
            self.remove_gw2_key();
        }
    }

    fn save_gw2_key(&mut self) {
        let key = self.focus.gw2.draft.trim().to_string();
        let g = &mut self.focus.gw2;
        if !gw2::looks_like_key(&key) {
            g.note = "That doesn't look like a Guild Wars 2 API key. Copy the whole key from account.arena.net.".into();
            return;
        }
        match store_key(Some(&key)) {
            Ok(()) => {
                g.draft.clear();
                g.has_key = Some(true);
                g.next_ms = 0;
                g.note = "Key saved. Reading your account...".into();
            }
            Err(e) => g.note = format!("Couldn't save the key to the keychain: {e}"),
        }
    }

    fn remove_gw2_key(&mut self) {
        let g = &mut self.focus.gw2;
        match store_key(None) {
            Ok(()) => {
                g.has_key = Some(false);
                g.account = None;
                g.note = "Key removed.".into();
                if !cfg!(test) {
                    let _ = std::fs::remove_file(gw2::cache_path(&crate::config::config_dir()));
                }
            }
            Err(e) => g.note = format!("Couldn't remove the key: {e}"),
        }
    }
}

/// `Guild Wars 2: ... Updated 4 minutes ago.`
fn gw2_status(acct: Option<&Gw2Account>, has_key: bool, now: u64) -> String {
    match (acct, has_key) {
        (_, false) => {
            "Guild Wars 2: add a read-only API key to read your account through ArenaNet's official API."
                .into()
        }
        (None, true) => "Guild Wars 2: key saved, nothing read yet.".into(),
        (Some(a), true) => {
            let mins = now.saturating_sub(a.fetched_ms) / 60_000;
            let age = match mins {
                0 => "just now".to_string(),
                1 => "a minute ago".to_string(),
                m if m < 120 => format!("{m} minutes ago"),
                m => format!("{} hours ago", m / 60),
            };
            format!("{}: {} Updated {age}.", gw2::NAME, a.account_line())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_line_says_where_the_key_and_snapshot_stand() {
        assert!(gw2_status(None, false, 0).contains("add a read-only API key"));
        assert_eq!(
            gw2_status(None, true, 0),
            "Guild Wars 2: key saved, nothing read yet."
        );
        let acct = Gw2Account {
            fractal_level: Some(5),
            fetched_ms: 0,
            ..Default::default()
        };
        assert_eq!(
            gw2_status(Some(&acct), true, 4 * 60_000),
            "Guild Wars 2: Fractal level 5. Updated 4 minutes ago."
        );
    }

    #[test]
    fn a_bad_key_is_refused_before_it_reaches_the_keychain() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.focus.gw2.draft = "hunter2".into();
        cabin.save_gw2_key();
        assert_ne!(cabin.focus.gw2.has_key, Some(true));
        assert!(cabin.focus.gw2.note.contains("doesn't look like"));
        cabin.focus.gw2.draft =
            "ABCDEF01-2345-6789-ABCD-EF0123456789ABCDEF01-2345-6789-ABCD-EF0123456789".into();
        cabin.save_gw2_key();
        assert_eq!(cabin.focus.gw2.has_key, Some(true));
        assert!(cabin.focus.gw2.draft.is_empty());
        cabin.remove_gw2_key();
        assert_eq!(cabin.focus.gw2.has_key, Some(false));
    }

    #[test]
    fn a_rate_limit_waits_and_a_good_read_keeps_the_snapshot() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.finish_gw2(Err(FetchError::RateLimited { retry_after_s: 90 }), 1_000);
        assert_eq!(cabin.focus.gw2.next_ms, 91_000);
        assert!(cabin.focus.gw2.note.contains("2 minutes"));
        let acct = Gw2Account {
            missing: vec!["wallet".into()],
            ..Default::default()
        };
        cabin.finish_gw2(Ok(acct.clone()), 2_000);
        assert_eq!(cabin.focus.gw2.account, Some(acct));
        assert!(cabin.focus.gw2.note.contains("no wallet scope"));
    }

    #[test]
    fn nothing_is_sent_from_tests_or_to_other_hosts() {
        let agent = api_agent();
        assert!(matches!(
            api_get(&agent, "https://example.com/v2/account", "k"),
            Err(GetError::Other(_))
        ));
    }
}

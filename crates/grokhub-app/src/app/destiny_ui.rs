//! Destiny 2 (`grokhub_core::destiny_api`).
//!
//! Under the game watcher in Settings: a short how-to for a personal
//! Bungie.net application, a masked field for its API key (system keychain
//! only) and the player's Bungie Name, then what Bungie.net shows publicly.
//! With both, the profile is refreshed off the UI thread every fifteen
//! minutes and on Refresh, each call through the egress guard (so Offline
//! only stops it). The last good snapshot is kept on disk without the key.
//! Nothing touches the game or BattlEye.

use std::sync::mpsc;

use super::*;
use grokhub_core::destiny_api::{self as d2, Call, DestinyProfile, FetchError, SendError};

const KEYRING_SERVICE: &str = "GrokHub";
const KEYRING_ACCOUNT: &str = "destiny2-api-key";
const HOW_TO: &str = "Register an application at bungie.net/en/Application (OAuth client type: Not applicable), copy its API Key here and add your Bungie Name. Bungie's terms keep a key personal, so Loadout can't ship one. Only your public profile is read.";

#[derive(Default)]
pub(super) struct DestinyUi {
    has_key: Option<bool>,
    key_draft: String,
    /// The Bungie Name in use; loaded once.
    name: Option<String>,
    name_draft: String,
    profile: Option<DestinyProfile>,
    loaded: bool,
    pending: Option<mpsc::Receiver<Result<DestinyProfile, FetchError>>>,
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

/// The Bungie Name isn't secret: it sits beside the snapshot.
fn name_path(config_dir: &std::path::Path) -> PathBuf {
    config_dir.join("games").join("destiny2-player.txt")
}

fn load_name() -> Option<String> {
    if cfg!(test) {
        return None;
    }
    let text = std::fs::read_to_string(name_path(&crate::config::config_dir())).ok()?;
    d2::parse_bungie_name(&text).map(|_| text.trim().to_owned())
}

fn store_name(name: Option<&str>) {
    if cfg!(test) {
        return;
    }
    let path = name_path(&crate::config::config_dir());
    match name {
        Some(n) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, n);
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn api_agent() -> grokhub_core::http::Agent {
    grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: std::time::Duration::from_secs(8),
        overall: Some(std::time::Duration::from_secs(20)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    })
}

/// One call to Bungie.net with the key in `X-API-Key`. A 4xx/5xx that
/// still carries Bungie's JSON envelope is handed back as the body, so its
/// own error code is read.
fn api_send(
    agent: &grokhub_core::http::Agent,
    call: &Call,
    key: &str,
) -> Result<String, SendError> {
    if cfg!(test) || !call.url.starts_with(d2::API_BASE) {
        return Err(SendError::Other("Not sent.".into()));
    }
    crate::xai::egress_ok(&call.url, &[]).map_err(|_| {
        SendError::Other(
            "Offline only or the network guard held the Destiny 2 refresh back.".into(),
        )
    })?;
    let answer = match &call.body {
        Some(body) => agent
            .post(&call.url)
            .header("x-api-key", key)
            .header("content-type", "application/json")
            .send_string(&body.to_string()),
        None => agent.get(&call.url).header("x-api-key", key).call(),
    };
    let resp = match answer {
        Ok(resp) => resp,
        Err(grokhub_core::http::Error::Status {
            code,
            body,
            retry_after,
            ..
        }) => {
            if body.contains("\"ErrorCode\"") {
                return Ok(body);
            }
            return Err(SendError::Status {
                code,
                retry_after_s: retry_after.and_then(|r| r.trim().parse().ok()),
            });
        }
        Err(_) => return Err(SendError::Other("Couldn't reach Bungie.net.".into())),
    };
    let mut text = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(resp.into_reader(), d2::MAX_BODY),
        &mut text,
    )
    .map_err(|_| SendError::Other("Bungie.net's answer didn't arrive whole.".into()))?;
    Ok(text)
}

impl Cabin {
    pub(super) fn tick_destiny(&mut self) {
        if !self.focus.destiny.loaded {
            self.focus.destiny.loaded = true;
            self.focus.destiny.profile = d2::load_cache(&crate::config::config_dir());
            self.focus.destiny.name = load_name();
        }
        if let Some(rx) = &self.focus.destiny.pending {
            match rx.try_recv() {
                Ok(done) => {
                    self.focus.destiny.pending = None;
                    self.finish_destiny(done, now_ms());
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => self.focus.destiny.pending = None,
            }
        }
        if !self.cfg.game_watch
            || now_ms() < self.focus.destiny.next_ms
            || self.focus.destiny.name.is_none()
        {
            return;
        }
        if *self
            .focus
            .destiny
            .has_key
            .get_or_insert_with(|| load_key().is_some())
        {
            self.start_destiny_refresh();
        }
    }

    fn start_destiny_refresh(&mut self) {
        let d = &mut self.focus.destiny;
        if d.pending.is_some() {
            return;
        }
        d.next_ms = now_ms() + d2::REFRESH_MS;
        let Some(name) = d.name.clone() else {
            return;
        };
        let Some(key) = load_key() else {
            d.has_key = Some(false);
            return;
        };
        let (tx, rx) = mpsc::channel();
        d.pending = Some(rx);
        std::thread::spawn(move || {
            let agent = api_agent();
            let got = d2::fetch_profile(&mut |call| api_send(&agent, call, &key), &name, now_ms());
            let _ = tx.send(got);
        });
    }

    fn finish_destiny(&mut self, done: Result<DestinyProfile, FetchError>, now: u64) {
        let d = &mut self.focus.destiny;
        match done {
            Ok(p) => {
                if !cfg!(test) {
                    let _ = d2::save_cache(&crate::config::config_dir(), &p);
                }
                d.note = if p.hidden.is_empty() {
                    String::new()
                } else {
                    format!(
                        "Bungie.net keeps your {} private, so that part isn't shown. Privacy settings on Bungie.net can change that.",
                        p.hidden.join(", ")
                    )
                };
                d.profile = Some(p);
            }
            Err(e) => {
                if let FetchError::RateLimited { retry_after_s } = &e {
                    d.next_ms = now + retry_after_s * 1000;
                }
                d.note = e.message();
            }
        }
    }

    /// The Destiny 2 rows under the game watcher.
    pub(super) fn ui_destiny(&mut self, ui: &mut egui::Ui) {
        if !self.cfg.game_watch {
            return;
        }
        let has_key = *self
            .focus
            .destiny
            .has_key
            .get_or_insert_with(|| load_key().is_some());
        let d = &self.focus.destiny;
        let ready = has_key && d.name.is_some();
        let line = destiny_status(d.profile.as_ref(), ready, now_ms());
        let note = d.note.clone();
        let busy = d.pending.is_some();
        let need_name = d.name.is_none();
        let (mut save_key, mut save_name, mut refresh, mut forget) = (false, false, false, false);
        ui.indent("destiny-api", |ui| {
            crate::cards::help_text(ui, &line);
            if ready {
                ui.horizontal(|ui| {
                    if busy {
                        crate::cards::help_text(ui, "Refreshing...");
                    } else {
                        refresh = crate::cards::ghost_pill(ui, "Refresh Destiny 2");
                    }
                    forget = crate::cards::ghost_pill(ui, "Remove key and name");
                });
            } else {
                crate::cards::help_text(ui, HOW_TO);
                if !has_key {
                    crate::cards::settings_field(
                        ui,
                        "Bungie.net API key",
                        "From your Bungie.net application page. It stays in your system keychain.",
                        &mut self.focus.destiny.key_draft,
                        true,
                    );
                    save_key = crate::cards::settings_action(
                        ui,
                        "Bungie.net API key",
                        "Saved to your keychain only.",
                        "Save key",
                    );
                }
                if need_name {
                    crate::cards::settings_field(
                        ui,
                        "Bungie Name",
                        "Your name with its code, like Guardian#1234.",
                        &mut self.focus.destiny.name_draft,
                        false,
                    );
                    save_name = crate::cards::settings_action(
                        ui,
                        "Bungie Name",
                        "Used to find your public profile.",
                        "Save name",
                    );
                }
            }
            if !note.is_empty() {
                crate::cards::help_text(ui, &note);
            }
        });
        if save_key {
            self.save_destiny_key();
        }
        if save_name {
            self.save_destiny_name();
        }
        if refresh {
            self.focus.destiny.next_ms = 0;
            self.start_destiny_refresh();
        }
        if forget {
            self.forget_destiny();
        }
    }

    fn save_destiny_key(&mut self) {
        let key = self.focus.destiny.key_draft.trim().to_string();
        let d = &mut self.focus.destiny;
        if !d2::looks_like_key(&key) {
            d.note = "That doesn't look like a Bungie.net API key: it's 32 letters and numbers on your application page.".into();
            return;
        }
        match store_key(Some(&key)) {
            Ok(()) => {
                d.key_draft.clear();
                d.has_key = Some(true);
                d.next_ms = 0;
                d.note = "Key saved.".into();
            }
            Err(e) => d.note = format!("Couldn't save the key to the keychain: {e}"),
        }
    }

    fn save_destiny_name(&mut self) {
        let d = &mut self.focus.destiny;
        let Some((name, code)) = d2::parse_bungie_name(&d.name_draft) else {
            d.note = "Enter your Bungie Name with its four-digit code, like Guardian#1234.".into();
            return;
        };
        let full = format!("{name}#{code:04}");
        store_name(Some(&full));
        d.name = Some(full);
        d.name_draft.clear();
        d.next_ms = 0;
        d.note = "Name saved.".into();
    }

    fn forget_destiny(&mut self) {
        let d = &mut self.focus.destiny;
        if let Err(e) = store_key(None) {
            d.note = format!("Couldn't remove the key: {e}");
            return;
        }
        store_name(None);
        if !cfg!(test) {
            let _ = std::fs::remove_file(d2::cache_path(&crate::config::config_dir()));
        }
        d.has_key = Some(false);
        d.name = None;
        d.profile = None;
        d.note = "Key and name removed.".into();
    }
}

/// `Destiny 2 (Tester#1234): ... Updated 4 minutes ago.`
fn destiny_status(p: Option<&DestinyProfile>, ready: bool, now: u64) -> String {
    match (p, ready) {
        (_, false) => {
            "Destiny 2: add your own Bungie.net API key and Bungie Name to read your public profile."
                .into()
        }
        (None, true) => "Destiny 2: key and name saved, nothing read yet.".into(),
        (Some(p), true) => {
            let mins = now.saturating_sub(p.fetched_ms) / 60_000;
            let age = match mins {
                0 => "just now".to_string(),
                1 => "a minute ago".to_string(),
                m if m < 120 => format!("{m} minutes ago"),
                m => format!("{} hours ago", m / 60),
            };
            format!(
                "{} ({}): {} Updated {age}.",
                d2::NAME,
                p.bungie_name,
                p.profile_line()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_line_says_where_setup_and_snapshot_stand() {
        assert!(destiny_status(None, false, 0).contains("your own Bungie.net API key"));
        let p = DestinyProfile {
            bungie_name: "Tester#1234".into(),
            triumph_score: Some(5),
            ..Default::default()
        };
        assert_eq!(
            destiny_status(Some(&p), true, 3 * 60_000),
            "Destiny 2 (Tester#1234): Triumph score 5. Updated 3 minutes ago."
        );
    }

    #[test]
    fn key_and_name_are_checked_before_they_are_kept() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.focus.destiny.key_draft = "hunter2".into();
        cabin.save_destiny_key();
        assert_ne!(cabin.focus.destiny.has_key, Some(true));
        cabin.focus.destiny.key_draft = "0123456789abcdef0123456789abcdef".into();
        cabin.save_destiny_key();
        assert_eq!(cabin.focus.destiny.has_key, Some(true));
        cabin.focus.destiny.name_draft = "Tester".into();
        cabin.save_destiny_name();
        assert_eq!(cabin.focus.destiny.name, None);
        cabin.focus.destiny.name_draft = "Tester#42".into();
        cabin.save_destiny_name();
        assert_eq!(cabin.focus.destiny.name, None);
        cabin.focus.destiny.name_draft = "Tester#0042".into();
        cabin.save_destiny_name();
        assert_eq!(cabin.focus.destiny.name.as_deref(), Some("Tester#0042"));
        cabin.forget_destiny();
        assert_eq!(cabin.focus.destiny.name, None);
        assert_eq!(cabin.focus.destiny.has_key, Some(false));
    }

    #[test]
    fn hidden_parts_and_rate_limits_are_said_plainly() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.finish_destiny(Err(FetchError::RateLimited { retry_after_s: 30 }), 1_000);
        assert_eq!(cabin.focus.destiny.next_ms, 31_000);
        let p = DestinyProfile {
            hidden: vec!["characters".into()],
            ..Default::default()
        };
        cabin.finish_destiny(Ok(p), 2_000);
        assert!(cabin
            .focus
            .destiny
            .note
            .contains("keeps your characters private"));
    }

    #[test]
    fn nothing_is_sent_from_tests_or_to_other_hosts() {
        let agent = api_agent();
        let call = Call {
            url: "https://example.com/Platform".into(),
            body: None,
        };
        assert!(matches!(
            api_send(&agent, &call, "k"),
            Err(SendError::Other(_))
        ));
    }
}

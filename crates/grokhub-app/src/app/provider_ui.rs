//! Router R3b in the cabin: other AI providers you add with your own key.
//! Off until you add a key and allow the destination.
//!
//! - Settings → Cabin defaults (the model rows): "Add a provider" (base URL
//!   plus key; the key goes straight into the OS keyring), one row per
//!   provider with its state tag and Remove, and "Use for your chats".
//! - Settings → Permissions: one destination row per provider. Allow parks a
//!   hard send card (no Always, Enter doesn't approve, Esc and the timeout
//!   deny); only its Approve click writes the grant (`UserClick::from_click`).
//!   Revoke is one click, and the next call is blocked.
//!
//! - Saving a key is a hard credentials card first ("Save your key for
//!   openrouter.ai to your keyring"); only its Approve click puts it in the
//!   keyring. Deny, Esc and the timeout drop it.
//! - "Provider type" picks the API shape for a proxy under another host.
//!   "Get a key" opens a known provider's key page; "Sign in with
//!   OpenRouter" runs its browser PKCE flow and brings a key back to the
//!   same credentials card.
//! - After a refresh, a quiet status names the models a provider added, or
//!   says it turned the key down.
//!
//! The key field is cleared the moment Add is clicked and is never logged,
//! spanned or shown.

use grokhub_agent::harness::{self as hx, GateOutcome, HardClass, Step};
use grokhub_agent::route::providers::{self, ProviderKind, CALL_DATA, PROVIDER_TOOL};
use grokhub_core::model_registry::RegistryEvent;

use super::*;

/// Settings → Cabin defaults.
pub(super) const ADD_ROW: &str = "Add a provider";
const ADD_HINT: &str = "Off until you add a key and allow the destination. OpenAI-compatible (like OpenRouter) or Anthropic. The key stays in your keyring.";
const URL_ROW: &str = "Provider address";
const URL_HINT: &str = "Its https API address, like https://openrouter.ai/api/v1 or https://api.anthropic.com.";
const KEY_ROW: &str = "Provider key";
const KEY_HINT: &str = "Saved to your keyring, never to a file, a log or the model.";
pub(super) const PICK_ROW: &str = "Use for your chats";
const PICK_HINT: &str = "A model from a provider you allowed. Auto never picks one on its own, and never to save money.";
const PICK_OFF: &str = "Off (xAI only)";
const KIND_ROW: &str = "Provider type";
const KIND_HINT: &str = "How it talks. Detected from the address; pick one for a proxy under another name.";
const KIND_LABELS: [&str; 3] = ["Detect from the address", "OpenAI-compatible", "Anthropic-compatible"];
const KEY_PAGE_ROW: &str = "Get a key";
const OPENROUTER_ROW: &str = "Sign in with OpenRouter";
const OPENROUTER_HINT: &str = "Opens OpenRouter in your browser and brings a new key back. You approve saving it.";
/// What a credentials card is for (its span tool).
pub(super) const KEY_TOOL: &str = "model.provider_key";
/// The credentials card under its action line.
pub(super) const KEY_CARD_NOTE: &str = "Approve puts the key in your keyring, never in a file, a log or the model. Esc denies and drops it.";
/// Settings → Permissions.
pub(super) const PROVIDER_HEAD: &str = "Other providers";
/// The hard send card under its action line.
pub(super) const PROVIDER_CARD_NOTE: &str =
    "Your chats and memory would go to this provider. Approve allows it until you revoke it in Settings → Permissions. Esc denies.";

#[derive(Debug, Default)]
pub(super) struct ProviderUi {
    pub url: String,
    pub key: String,
    /// "Provider type": an index into `KIND_LABELS` (0 detects it).
    pub kind: usize,
    /// The provider pick the router last heard.
    pub sent: Option<String>,
    /// A key waiting on its credentials card.
    pub pending: Option<PendingKey>,
    /// "Sign in with OpenRouter", running: the key it brings back.
    pub signin_rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
}

/// A key you entered or a sign-in brought back, held until its card is answered.
pub(super) struct PendingKey {
    pub url: String,
    pub kind: Option<ProviderKind>,
    pub dest: String,
    pub key: String,
}

impl std::fmt::Debug for PendingKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingKey").field("dest", &self.dest).field("kind", &self.kind).field("key", &"[redacted]").finish()
    }
}

/// The "Provider type" pick: `None` detects it from the address.
fn kind_pick(i: usize) -> Option<ProviderKind> {
    match i {
        1 => Some(ProviderKind::OpenAiCompatible),
        2 => Some(ProviderKind::Anthropic),
        _ => None,
    }
}

/// The credentials card's action line.
pub(super) fn key_action(dest: &str) -> String {
    format!("Save your key for {dest} to your keyring")
}

/// After a model refresh: the models each provider you added brought in
/// ("Added 2 models from openrouter.ai: vendor/a, vendor/b"), or that it
/// turned the key down. `None` when nothing about a provider changed.
pub(super) fn provider_refresh_note(events: &[RegistryEvent], errors: &[String], all: &[providers::Provider]) -> Option<String> {
    let mut lines = Vec::new();
    for p in all {
        let prefix = format!("{}/", p.id);
        let added: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                RegistryEvent::Added { id } => id.strip_prefix(&prefix),
                _ => None,
            })
            .collect();
        match added.len() {
            0 => {}
            1 => lines.push(format!("Added 1 model from {}: {}", p.dest(), added[0])),
            n if n <= 3 => lines.push(format!("Added {n} models from {}: {}", p.dest(), added.join(", "))),
            n => lines.push(format!("Added {n} models from {}: {}, and {} more", p.dest(), added[..3].join(", "), n - 3)),
        }
        let refused = errors.iter().any(|e| e == &format!("{}: HTTP 401", p.id) || e == &format!("{}: HTTP 403", p.id));
        if refused {
            lines.push(format!("{} turned the key down. Remove it and add a new one.", p.dest()));
        }
    }
    (!lines.is_empty()).then(|| lines.join(" "))
}

/// "openrouter.ai (OpenAI-compatible)".
pub(super) fn provider_label(p: &providers::Provider) -> String {
    format!("{} ({})", p.dest(), p.kind.label())
}

/// The hard card's action line.
pub(super) fn provider_action(p: &providers::Provider) -> String {
    format!("Send your chats and memory to {} when you pick one of its models", p.dest())
}

impl Cabin {
    /// The router reads your pick; it never writes it.
    pub(super) fn sync_provider_pick(&mut self) {
        let pick = self.cfg.provider_model.trim().to_string();
        if self.harness.router.providers.sent.as_deref() != Some(pick.as_str()) {
            grokhub_agent::route::live::set_provider_pick(&pick);
            self.harness.router.providers.sent = Some(pick);
        }
    }

    /// "Add a provider": check the address and key, then the credentials card.
    pub(super) fn add_provider_click(&mut self) {
        let key = std::mem::take(&mut self.harness.router.providers.key);
        let url = self.harness.router.providers.url.trim().to_string();
        let kind = kind_pick(self.harness.router.providers.kind);
        match providers::parse_base_url(&url).and_then(|_| providers::check_key(&key).map(str::to_string)) {
            Ok(key) => self.hold_provider_key(url, kind, key),
            Err(e) => self.status = e,
        }
    }

    /// Hold a key until its hard credentials card is answered.
    pub(super) fn hold_provider_key(&mut self, url: String, kind: Option<ProviderKind>, key: String) {
        if self.harness.router.providers.pending.is_some() {
            self.status = "Answer the card for the key you added first.".into();
            return;
        }
        let dest = grokhub_agent::harness::egress_dest(&url);
        let action = key_action(&dest);
        let args = serde_json::json!({ "dest": dest }).to_string();
        self.write_span(hx::Span::hard_park(&self.trace_id(), KEY_TOOL, &args, HardClass::Credentials), "route");
        self.harness.router.providers.pending = Some(PendingKey { url, kind, dest: dest.clone(), key });
        self.park_hard(super::harness_ui::ParkSource::ProviderKey(dest.clone()), HardClass::Credentials, "route", KEY_TOOL.into(), action);
        self.status = format!("Approve the card to save the key for {dest}.");
    }

    /// The credentials card answered. Approve: key into the keyring, then the
    /// hard send card for its destination. Deny: the key is dropped.
    pub(super) fn answer_provider_key(&mut self, approve: bool) {
        let Some(pending) = self.harness.router.providers.pending.take() else {
            return;
        };
        if !approve {
            self.status = format!("Nothing saved. The key for {} was dropped.", pending.dest);
            return;
        }
        let dir = crate::config::config_dir();
        match providers::add_provider_as(&dir, &pending.url, &pending.key, pending.kind, now_ms()) {
            Ok(p) => {
                self.harness.router.providers.url.clear();
                self.harness.router.providers.kind = 0;
                self.status = format!("{} added. Approve the card to allow it, or later in Settings → Permissions.", p.dest());
                self.ask_provider(&p.id);
                self.refresh_models_now();
            }
            Err(e) => self.status = e,
        }
    }

    /// "Sign in with OpenRouter": its browser flow off the UI thread.
    pub(super) fn start_openrouter_sign_in(&mut self) {
        if self.harness.router.providers.signin_rx.is_some() {
            return;
        }
        let dir = crate::config::config_dir();
        let (tx, rx) = std::sync::mpsc::channel();
        self.harness.router.providers.signin_rx = Some(rx);
        std::thread::spawn(move || {
            let got = providers::openrouter_key(&dir, &|url| crate::oauth::open_browser(url), Duration::from_secs(300));
            let _ = tx.send(got.map(|k| k.as_str().to_string()));
        });
        self.status = "Finish signing in to OpenRouter in your browser.".into();
    }

    /// The sign-in's key goes to the same credentials card as a pasted one.
    pub(super) fn poll_openrouter_sign_in(&mut self) {
        let Some(rx) = self.harness.router.providers.signin_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(key)) => self.hold_provider_key(providers::OPENROUTER_BASE.into(), None, key),
            Ok(Err(e)) => self.status = e,
            Err(std::sync::mpsc::TryRecvError::Empty) => self.harness.router.providers.signin_rx = Some(rx),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn remove_provider_click(&mut self, id: &str) {
        let dir = crate::config::config_dir();
        match providers::remove_provider(&dir, id) {
            Ok(()) => {
                if self.cfg.provider_model.split('/').next() == Some(id) {
                    self.cfg.provider_model.clear();
                    self.persist_cfg();
                }
                self.status = "Provider and its key removed. Revoke its grant in Settings → Permissions.".into();
                self.refresh_models_now();
            }
            Err(e) => self.status = format!("Could not remove it: {e}"),
        }
    }

    /// Allow a provider's destination: `harness::decide` parks a hard send card.
    pub(super) fn ask_provider(&mut self, id: &str) {
        let dir = crate::config::config_dir();
        let Some(p) = providers::provider(&dir, id) else {
            return;
        };
        let ledger = self.consent().clone();
        if let GateOutcome::Park { hard: Some(class), .. } = hx::decide(Step::Provider { dest: &p.dest(), data: CALL_DATA, ledger: &ledger }) {
            let action = provider_action(&p);
            let args = serde_json::json!({ "provider": p.id, "dest": p.dest() }).to_string();
            self.write_span(hx::Span::hard_park(&self.trace_id(), PROVIDER_TOOL, &args, class), "route");
            self.park_hard(super::harness_ui::ParkSource::Provider(p.id.clone()), class, "route", PROVIDER_TOOL.into(), action);
        }
    }

    /// The hard send card's Approve click: one revocable grant for this destination.
    pub(super) fn grant_provider_click(&mut self, id: &str) {
        let dir = crate::config::config_dir();
        let Some(p) = providers::provider(&dir, id) else {
            return;
        };
        match hx::grant_destination(&dir, &p.dest(), CALL_DATA, hx::UserClick::from_click()) {
            Ok(_) => {
                self.status = format!("{} allowed. Revoke it in Settings → Permissions.", p.dest());
                self.refresh_models_now();
            }
            Err(e) => self.status = format!("Could not save the grant: {e}"),
        }
        self.harness.consent = None;
    }

    /// Models from providers you allowed, for "Use for your chats".
    pub(super) fn provider_model_choices(&self) -> Vec<String> {
        let dir = crate::config::config_dir();
        let ready = providers::usable(&dir, CALL_DATA);
        let (reg, _) = grokhub_agent::route::live::snapshot(&dir);
        reg.models
            .iter()
            .filter(|(id, r)| r.state.routable() && grokhub_agent::route::policy::is_new_provider(&reg, id))
            .filter(|(id, _)| id.split('/').next().is_some_and(|p| ready.iter().any(|r| r == p)))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Settings → Cabin defaults, under the model rows.
    pub(super) fn ui_provider_rows(&mut self, ui: &mut egui::Ui) {
        let dir = crate::config::config_dir();
        let ledger = self.consent().clone();
        for p in providers::load_providers(&dir) {
            let state = providers::provider_state(&dir, &ledger, &p);
            ui.push_id(("provider-row", p.id.as_str()), |ui| {
                if crate::cards::settings_action(ui, &provider_label(&p), state.tag(), "Remove") {
                    self.remove_provider_click(&p.id);
                }
            });
        }
        let models = self.provider_model_choices();
        if !models.is_empty() || !self.cfg.provider_model.is_empty() {
            let mut labels = vec![PICK_OFF.to_string()];
            labels.extend(models.iter().cloned());
            let selected = if self.cfg.provider_model.is_empty() { PICK_OFF.to_string() } else { self.cfg.provider_model.clone() };
            if let Some(i) = crate::cards::settings_dropdown(ui, PICK_ROW, PICK_HINT, &selected, &labels) {
                let next = if i == 0 { String::new() } else { models.get(i - 1).cloned().unwrap_or_default() };
                if next != self.cfg.provider_model {
                    self.cfg.provider_model = next;
                    self.persist_cfg();
                    self.status = "Saved".into();
                }
            }
        }
        crate::cards::settings_field(ui, URL_ROW, URL_HINT, &mut self.harness.router.providers.url, false);
        let labels: Vec<String> = KIND_LABELS.iter().map(|l| l.to_string()).collect();
        let picked = labels[self.harness.router.providers.kind.min(2)].clone();
        if let Some(i) = crate::cards::settings_dropdown(ui, KIND_ROW, KIND_HINT, &picked, &labels) {
            self.harness.router.providers.kind = i;
        }
        if let Some(page) = providers::key_page(&self.harness.router.providers.url) {
            let hint = format!("Opens {}'s key page in your browser. Paste the key below.", grokhub_agent::harness::egress_dest(&self.harness.router.providers.url));
            if crate::cards::settings_action(ui, KEY_PAGE_ROW, &hint, "Open") {
                if let Err(e) = crate::oauth::open_browser(page) {
                    self.status = e;
                }
            }
        }
        crate::cards::settings_field(ui, KEY_ROW, KEY_HINT, &mut self.harness.router.providers.key, true);
        if crate::cards::settings_action(ui, ADD_ROW, ADD_HINT, "Add") {
            self.add_provider_click();
        }
        if crate::cards::settings_action(ui, OPENROUTER_ROW, OPENROUTER_HINT, "Sign in") {
            self.start_openrouter_sign_in();
        }
    }

    /// Settings → Permissions, "Other providers": one destination row each.
    /// Allow parks the hard send card; only its Approve writes the grant.
    pub(super) fn ui_provider_grant_rows(&mut self, ui: &mut egui::Ui, locked: Option<&str>) {
        let dir = crate::config::config_dir();
        let all = providers::load_providers(&dir);
        if all.is_empty() {
            return;
        }
        crate::cards::section_heading(ui, PROVIDER_HEAD);
        for p in all {
            let grant = self.consent().destination_grant(&p.dest(), CALL_DATA).map(|g| (g.id.clone(), g.granted_at));
            let title = format!("Send to {}", p.dest());
            ui.push_id(("provider-grant", p.id.as_str()), |ui| match grant {
                None => {
                    let hint = "Off. Allow shows a card first.";
                    if crate::cards::settings_grant_row(ui, &title, hint, None, crate::cards::GrantPill::Allow, locked, false, |_| {}) && locked.is_none() {
                        self.ask_provider(&p.id);
                    }
                }
                Some((id, at)) => {
                    let hint = format!("Allowed {}. Sends chats and memory.", grokhub_core::pulse::ago_label(at, now_ms()));
                    if crate::cards::settings_grant_row(ui, &title, &hint, None, crate::cards::GrantPill::Revoke, locked, false, |_| {}) && locked.is_none() {
                        self.revoke_provider_grant(&id, &p.dest());
                    }
                }
            });
        }
        ui.add_space(8.0);
    }

    pub(super) fn revoke_provider_grant(&mut self, id: &str, dest: &str) {
        match hx::revoke_grant(&crate::config::config_dir(), id) {
            Ok(_) => self.status = format!("Sending to {dest} revoked. Its models stop at the next call."),
            Err(e) => self.status = format!("Could not revoke: {e}"),
        }
        self.harness.consent = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::harness::HardClass;
    use std::sync::Arc;

    /// The card a provider destination parks is hard class send.
    fn is_provider_card(class: HardClass, source: &super::super::harness_ui::ParkSource) -> bool {
        class == HardClass::Send && matches!(source, super::super::harness_ui::ParkSource::Provider(_))
    }

    fn pinned(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
        let root = crate::config::test_config_root(label);
        std::fs::create_dir_all(&root).unwrap();
        hx::use_key_store_for(&root, Arc::new(hx::MemoryKeyStore::new()));
        providers::use_vault_for(&root, Arc::new(providers::MemoryVault::default()));
        (crate::config::TestConfigDir::set(root.clone()), root)
    }

    const KEY: &str = "sk-abcdefghijklmnopqrstuv";

    #[test]
    fn adding_a_provider_parks_a_hard_send_card_and_approve_writes_a_revocable_grant() {
        let (_pin, root) = pinned("r3b-provider-card");
        let mut app = Cabin::quiet_for_test();
        app.harness.router.providers.url = "https://openrouter.ai/api/v1".into();
        app.harness.router.providers.key = KEY.into();
        app.add_provider_click();
        assert!(app.harness.router.providers.key.is_empty(), "the key field clears on Add");
        // Saving the key is a hard credentials card first; nothing is in the keyring yet.
        assert!(!providers::has_key(&root, "openrouter"));
        let key_card = app.harness.park.clone().expect("a hard credentials card");
        assert_eq!(key_card.class, HardClass::Credentials);
        assert!(matches!(&key_card.source, super::super::harness_ui::ParkSource::ProviderKey(d) if d == "openrouter.ai"));
        assert_eq!(key_card.action, "Save your key for openrouter.ai to your keyring");
        assert_eq!(app.status, "Approve the card to save the key for openrouter.ai.");
        app.resolve_hard_park(true, "");
        assert!(providers::has_key(&root, "openrouter"));
        assert!(app.harness.router.providers.pending.is_none());
        let park = app.harness.park.clone().expect("a hard send card");
        assert!(is_provider_card(park.class, &park.source));
        assert_eq!(park.action, "Send your chats and memory to openrouter.ai when you pick one of its models");
        // Enter never approves it; Esc denies it, and Deny writes nothing.
        assert_eq!(hx::hard_card_key(true, false, false), None);
        assert_eq!(hx::hard_card_key(false, true, false), Some(hx::HardAnswer::Deny));
        app.resolve_hard_park(false, "Jeremy denied (Esc)");
        assert!(app.consent().destination_grant("openrouter.ai", CALL_DATA).is_none());
        assert_eq!(providers::usable(&root, CALL_DATA), Vec::<String>::new());
        // Allow in Settings → Permissions parks it again; Approve (a click) grants.
        app.ask_provider("openrouter");
        app.resolve_hard_park(true, "");
        let g = app.consent().destination_grant("openrouter.ai", CALL_DATA).cloned().expect("granted");
        assert_eq!(g.data_classes, CALL_DATA.to_vec());
        assert_eq!(providers::usable(&root, CALL_DATA), vec!["openrouter".to_string()]);
        // Granted: decide lets it through, so no second card.
        app.ask_provider("openrouter");
        assert!(app.harness.park.is_none());
        // Revoke is one click; the router can't use it at the next call.
        app.revoke_provider_grant(&g.id, "openrouter.ai");
        assert!(providers::usable(&root, CALL_DATA).is_empty());
        app.ask_provider("openrouter");
        assert!(app.harness.park.is_some());
        // The key is in no file under the config dir.
        let mut stack = vec![root.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let text = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).into_owned();
                    assert!(!text.contains(KEY), "the key is in {}", p.display());
                }
            }
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_bad_address_or_key_saves_nothing_and_says_why() {
        let (_pin, root) = pinned("r3b-provider-bad");
        let mut app = Cabin::quiet_for_test();
        app.harness.router.providers.url = "http://openrouter.ai/api/v1".into();
        app.harness.router.providers.key = KEY.into();
        app.add_provider_click();
        assert_eq!(app.status, "Use an https:// address.");
        assert!(app.harness.router.providers.key.is_empty(), "a refused key isn't kept in the field either");
        assert!(providers::load_providers(&root).is_empty() && app.harness.park.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_provider_pick_reaches_the_router_and_remove_clears_it() {
        let (_pin, root) = pinned("r3b-provider-pick");
        let mut app = Cabin::quiet_for_test();
        app.harness.router.providers.url = "https://api.anthropic.com".into();
        app.harness.router.providers.key = KEY.into();
        app.add_provider_click();
        app.resolve_hard_park(true, "");
        app.cfg.provider_model = "anthropic/model-x".into();
        app.sync_provider_pick();
        assert_eq!(app.harness.router.providers.sent.as_deref(), Some("anthropic/model-x"));
        app.remove_provider_click("anthropic");
        assert!(app.cfg.provider_model.is_empty(), "removing a provider drops your pick");
        assert!(!providers::has_key(&root, "anthropic"));
        app.sync_provider_pick();
        assert_eq!(app.harness.router.providers.sent.as_deref(), Some(""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn denying_the_key_card_drops_the_key_and_the_type_pick_reaches_the_provider() {
        let (_pin, root) = pinned("r3b-provider-key-card");
        let mut app = Cabin::quiet_for_test();
        app.harness.router.providers.url = "https://claude-proxy.example.org".into();
        app.harness.router.providers.kind = 2;
        app.harness.router.providers.key = KEY.into();
        app.add_provider_click();
        // A second key waits until the first card is answered.
        app.harness.router.providers.key = KEY.into();
        app.add_provider_click();
        assert_eq!(app.status, "Answer the card for the key you added first.");
        assert_eq!(app.harness.queue.len(), 0, "no second card");
        app.resolve_hard_park(false, "Jeremy denied (Esc)");
        assert_eq!(app.status, "Nothing saved. The key for claude-proxy.example.org was dropped.");
        assert!(app.harness.router.providers.pending.is_none() && app.harness.park.is_none());
        assert!(providers::load_providers(&root).is_empty() && !providers::has_key(&root, "claude-proxy"));
        // Again, approved: the "Anthropic-compatible" pick is saved with it.
        app.harness.router.providers.key = KEY.into();
        app.add_provider_click();
        app.resolve_hard_park(true, "");
        let p = providers::provider(&root, "claude-proxy").expect("added");
        assert_eq!(p.kind, ProviderKind::Anthropic);
        assert_eq!(app.harness.router.providers.kind, 0, "the pick resets after a save");
        assert_eq!(app.harness.park.as_ref().map(|c| c.action.as_str()), Some("Send your chats and memory to claude-proxy.example.org when you pick one of its models"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_openrouter_sign_in_key_waits_on_the_same_credentials_card() {
        let (_pin, root) = pinned("r3b-openrouter-card");
        let mut app = Cabin::quiet_for_test();
        let (tx, rx) = std::sync::mpsc::channel();
        app.harness.router.providers.signin_rx = Some(rx);
        app.poll_openrouter_sign_in();
        assert!(app.harness.router.providers.signin_rx.is_some(), "still waiting on the browser");
        tx.send(Ok(KEY.to_string())).unwrap();
        app.poll_openrouter_sign_in();
        assert!(app.harness.router.providers.signin_rx.is_none());
        let card = app.harness.park.clone().expect("a hard credentials card");
        assert_eq!((card.class, card.action.as_str()), (HardClass::Credentials, "Save your key for openrouter.ai to your keyring"));
        assert!(!providers::has_key(&root, "openrouter"), "nothing saved before Approve");
        app.resolve_hard_park(true, "");
        assert_eq!(providers::provider(&root, "openrouter").map(|p| p.base_url), Some("https://openrouter.ai/api/v1".to_string()));
        assert!(providers::has_key(&root, "openrouter"));
        let (tx, rx) = std::sync::mpsc::channel();
        app.harness.router.providers.signin_rx = Some(rx);
        tx.send(Err("OpenRouter sign-in timed out".into())).unwrap();
        app.poll_openrouter_sign_in();
        assert_eq!(app.status, "OpenRouter sign-in timed out");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_refresh_names_the_models_a_provider_added_or_that_it_turned_the_key_down() {
        let or = providers::Provider { id: "openrouter".into(), kind: ProviderKind::OpenAiCompatible, base_url: "https://openrouter.ai/api/v1".into(), added_at: 0 };
        let an = providers::Provider { id: "anthropic".into(), kind: ProviderKind::Anthropic, base_url: "https://api.anthropic.com".into(), added_at: 0 };
        let added = |ids: &[&str]| ids.iter().map(|id| RegistryEvent::Added { id: id.to_string() }).collect::<Vec<_>>();
        assert_eq!(
            provider_refresh_note(&added(&["openrouter/vendor/a", "grok-4.7", "openrouter/vendor/b"]), &[], &[or.clone(), an.clone()]).as_deref(),
            Some("Added 2 models from openrouter.ai: vendor/a, vendor/b")
        );
        assert_eq!(
            provider_refresh_note(&added(&["anthropic/claude-x"]), &[], &[or.clone(), an.clone()]).as_deref(),
            Some("Added 1 model from api.anthropic.com: claude-x")
        );
        assert_eq!(
            provider_refresh_note(&added(&["openrouter/a", "openrouter/b", "openrouter/c", "openrouter/d", "openrouter/e"]), &[], std::slice::from_ref(&or)).as_deref(),
            Some("Added 5 models from openrouter.ai: a, b, c, and 2 more")
        );
        assert_eq!(
            provider_refresh_note(&[], &["anthropic: HTTP 401".into(), "openrouter: HTTP 500".into()], &[or.clone(), an]).as_deref(),
            Some("api.anthropic.com turned the key down. Remove it and add a new one.")
        );
        assert_eq!(provider_refresh_note(&added(&["grok-4.7"]), &[], &[or]), None);
    }
}

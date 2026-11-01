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
const URL_HINT: &str =
    "Its https API address, like https://openrouter.ai/api/v1 or https://api.anthropic.com.";
const KEY_ROW: &str = "Provider key";
const KEY_HINT: &str = "Saved to your keyring, never to a file, a log or the model.";
pub(super) const PICK_ROW: &str = "Use for your chats";
const PICK_HINT: &str = "A model from a provider you allowed. Auto never picks one on its own, and never to save money.";
const PICK_OFF: &str = "Off (xAI only)";
const KIND_ROW: &str = "Provider type";
const KIND_HINT: &str =
    "How it talks. Detected from the address; pick one for a proxy under another name.";
const KIND_LABELS: [&str; 3] = [
    "Detect from the address",
    "OpenAI-compatible",
    "Anthropic-compatible",
];
const KEY_PAGE_ROW: &str = "Get a key";
const OPENROUTER_ROW: &str = "Sign in with OpenRouter";
const OPENROUTER_HINT: &str =
    "Opens OpenRouter in your browser and brings a new key back. You approve saving it.";
const OPENROUTER_WAIT: &str = "Waiting for OpenRouter. Finish signing in in your browser.";
/// A page the browser didn't open, with Copy.
pub(super) const BY_HAND_ROW: &str = "Open this link in your browser";
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
    /// "Sign in with OpenRouter", running: what its thread sends back.
    pub signin_rx: Option<std::sync::mpsc::Receiver<SignInMsg>>,
    /// A page the browser didn't open. Settings shows it with Copy.
    pub by_hand: Option<ByHand>,
    /// How the last OpenRouter sign-in ended, when it ended without a key.
    pub signin_note: Option<String>,
    /// Cached `providers.json` list. Paint reads this; writers clear it.
    pub list: Option<Vec<providers::Provider>>,
    /// Cached "Use for your chats" model ids. Cleared with the list / grants.
    pub model_choices: Option<Vec<String>>,
}

/// What the "Sign in with OpenRouter" thread sends the cabin.
#[derive(Debug)]
pub(super) enum SignInMsg {
    /// The browser didn't open. The sign-in still waits for its callback,
    /// so opening this page by hand finishes it.
    OpenByHand(ByHand),
    /// The key, or why there is none.
    Done(Result<String, String>),
}

/// A page to open by hand, and why the browser didn't open it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ByHand {
    pub url: String,
    pub why: String,
}

/// Pages Settings opens for a provider: a known provider's key page, or
/// OpenRouter's sign-in page. Not the xAI gate: that one only lets x.ai
/// through, which is why "Sign in" on OpenRouter used to open nothing.
pub(super) fn provider_page_ok(url: &str) -> bool {
    providers::key_page(url) == Some(url)
        || url
            .strip_prefix("https://openrouter.ai/auth?")
            .is_some_and(|q| !q.is_empty())
}

/// Open a provider page in the system browser (ShellExecuteW on Windows).
pub(super) fn open_provider_page(url: &str) -> Result<(), String> {
    if !provider_page_ok(url) {
        return Err("refusing to open this page".into());
    }
    crate::desktop::open_url(url)
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
        f.debug_struct("PendingKey")
            .field("dest", &self.dest)
            .field("kind", &self.kind)
            .field("key", &"[redacted]")
            .finish()
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
pub(super) fn provider_refresh_note(
    events: &[RegistryEvent],
    errors: &[String],
    all: &[providers::Provider],
) -> Option<String> {
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
            n if n <= 3 => lines.push(format!(
                "Added {n} models from {}: {}",
                p.dest(),
                added.join(", ")
            )),
            n => lines.push(format!(
                "Added {n} models from {}: {}, and {} more",
                p.dest(),
                added[..3].join(", "),
                n - 3
            )),
        }
        let refused = errors
            .iter()
            .any(|e| e == &format!("{}: HTTP 401", p.id) || e == &format!("{}: HTTP 403", p.id));
        if refused {
            lines.push(format!(
                "{} turned the key down. Remove it and add a new one.",
                p.dest()
            ));
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
    format!(
        "Send your chats and memory to {} when you pick one of its models",
        p.dest()
    )
}

impl Cabin {
    /// Ensure `providers.json` is in memory. Call before a paint or write path
    /// that needs the list. Keeps Settings paint off disk.
    pub(super) fn ensure_providers(&mut self) {
        if self.harness.router.providers.list.is_none() {
            let dir = crate::config::config_dir();
            self.harness.router.providers.list = Some(providers::load_providers(&dir));
        }
    }

    /// Drop the cached list and model choices after a writer changes disk.
    pub(super) fn invalidate_providers(&mut self) {
        self.harness.router.providers.list = None;
        self.harness.router.providers.model_choices = None;
    }

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
        match providers::parse_base_url(&url)
            .and_then(|_| providers::check_key(&key).map(str::to_string))
        {
            Ok(key) => self.hold_provider_key(url, kind, key),
            Err(e) => self.status = e,
        }
    }

    /// Hold a key until its hard credentials card is answered.
    pub(super) fn hold_provider_key(
        &mut self,
        url: String,
        kind: Option<ProviderKind>,
        key: String,
    ) {
        if self.harness.router.providers.pending.is_some() {
            self.status = "Answer the card for the key you added first.".into();
            return;
        }
        let dest = grokhub_agent::harness::egress_dest(&url);
        let action = key_action(&dest);
        let args = serde_json::json!({ "dest": dest }).to_string();
        self.write_span(
            hx::Span::hard_park(&self.trace_id(), KEY_TOOL, &args, HardClass::Credentials),
            "route",
        );
        self.harness.router.providers.pending = Some(PendingKey {
            url,
            kind,
            dest: dest.clone(),
            key,
        });
        self.park_hard(
            super::harness_ui::ParkSource::ProviderKey(dest.clone()),
            HardClass::Credentials,
            "route",
            KEY_TOOL.into(),
            action,
        );
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
                self.invalidate_providers();
                self.status = format!(
                    "{} added. Approve the card to allow it, or later in Settings → Permissions.",
                    p.dest()
                );
                self.ask_provider(&p.id);
                self.refresh_models_now();
            }
            Err(e) => self.status = e,
        }
    }

    /// "Sign in with OpenRouter": its browser flow off the UI thread.
    pub(super) fn start_openrouter_sign_in(&mut self) {
        self.start_openrouter_sign_in_with(open_provider_page);
    }

    /// The flow with `open` as the browser. When `open` fails, the page goes
    /// back to Settings to open by hand and the flow keeps waiting for its
    /// callback, so the sign-in can still finish.
    pub(super) fn start_openrouter_sign_in_with(
        &mut self,
        open: impl Fn(&str) -> Result<(), String> + Send + 'static,
    ) {
        if self.harness.router.providers.signin_rx.is_some() {
            return;
        }
        let dir = crate::config::config_dir();
        let (tx, rx) = std::sync::mpsc::channel();
        self.harness.router.providers.signin_rx = Some(rx);
        self.harness.router.providers.by_hand = None;
        self.harness.router.providers.signin_note = None;
        std::thread::spawn(move || {
            let by_hand = tx.clone();
            let launch = move |url: &str| {
                if let Err(why) = open(url) {
                    let _ = by_hand.send(SignInMsg::OpenByHand(ByHand {
                        url: url.to_string(),
                        why,
                    }));
                }
                Ok(())
            };
            let got = providers::openrouter_key(&dir, &launch, Duration::from_secs(300));
            let _ = tx.send(SignInMsg::Done(got.map(|k| k.as_str().to_string())));
        });
        self.status = "Finish signing in to OpenRouter in your browser.".into();
    }

    /// The sign-in's key goes to the same credentials card as a pasted one.
    pub(super) fn poll_openrouter_sign_in(&mut self) {
        let Some(rx) = self.harness.router.providers.signin_rx.take() else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(SignInMsg::OpenByHand(link)) => {
                    self.status = format!("Your browser didn't open: {}", link.why);
                    self.harness.router.providers.by_hand = Some(link);
                }
                Ok(SignInMsg::Done(got)) => {
                    self.harness.router.providers.by_hand = None;
                    match got {
                        Ok(key) => {
                            self.hold_provider_key(providers::OPENROUTER_BASE.into(), None, key)
                        }
                        Err(e) => {
                            self.harness.router.providers.signin_note = Some(e.clone());
                            self.status = e;
                        }
                    }
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.harness.router.providers.signin_rx = Some(rx);
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
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
                self.invalidate_providers();
                self.status =
                    "Provider and its key removed. Revoke its grant in Settings → Permissions."
                        .into();
                self.refresh_models_now();
            }
            Err(e) => self.status = format!("Could not remove it: {e}"),
        }
    }

    /// Allow a provider's destination: `harness::decide` parks a hard send card.
    pub(super) fn ask_provider(&mut self, id: &str) {
        self.ensure_providers();
        let Some(p) = self
            .harness
            .router
            .providers
            .list
            .as_ref()
            .and_then(|all| all.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        let ledger = self.consent().clone();
        if let GateOutcome::Park {
            hard: Some(class), ..
        } = hx::decide(Step::Provider {
            dest: &p.dest(),
            data: CALL_DATA,
            ledger: &ledger,
        }) {
            let action = provider_action(&p);
            let args = serde_json::json!({ "provider": p.id, "dest": p.dest() }).to_string();
            self.write_span(
                hx::Span::hard_park(&self.trace_id(), PROVIDER_TOOL, &args, class),
                "route",
            );
            self.park_hard(
                super::harness_ui::ParkSource::Provider(p.id.clone()),
                class,
                "route",
                PROVIDER_TOOL.into(),
                action,
            );
        }
    }

    /// The hard send card's Approve click: one revocable grant for this destination.
    pub(super) fn grant_provider_click(&mut self, id: &str) {
        self.ensure_providers();
        let Some(p) = self
            .harness
            .router
            .providers
            .list
            .as_ref()
            .and_then(|all| all.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        let dir = crate::config::config_dir();
        match hx::grant_destination(&dir, &p.dest(), CALL_DATA, hx::UserClick::from_click()) {
            Ok(_) => {
                self.invalidate_providers();
                self.status = format!("{} allowed. Revoke it in Settings → Permissions.", p.dest());
                self.refresh_models_now();
            }
            Err(e) => self.status = format!("Could not save the grant: {e}"),
        }
        self.harness.consent = None;
    }

    /// Models from providers you allowed, for "Use for your chats".
    /// Uses the cabin consent cache and the cached provider list; not a fresh
    /// `usable()` disk read on every paint.
    pub(super) fn provider_model_choices(&mut self) -> Vec<String> {
        if let Some(cached) = &self.harness.router.providers.model_choices {
            return cached.clone();
        }
        self.ensure_providers();
        let dir = crate::config::config_dir();
        let locked = self.consent().locked().is_some();
        let ready: Vec<String> = if locked {
            Vec::new()
        } else {
            let ledger = self.consent().clone();
            self.harness
                .router
                .providers
                .list
                .as_ref()
                .into_iter()
                .flatten()
                .filter(|p| {
                    ledger.destination_grant(&p.dest(), CALL_DATA).is_some()
                        && providers::has_key(&dir, &p.id)
                })
                .map(|p| p.id.clone())
                .collect()
        };
        let (reg, _) = grokhub_agent::route::live::snapshot(&dir);
        let choices: Vec<String> = reg
            .models
            .iter()
            .filter(|(id, r)| {
                r.state.routable() && grokhub_agent::route::policy::is_new_provider(&reg, id)
            })
            .filter(|(id, _)| {
                id.split('/')
                    .next()
                    .is_some_and(|p| ready.iter().any(|r| r == p))
            })
            .map(|(id, _)| id.clone())
            .collect();
        self.harness.router.providers.model_choices = Some(choices.clone());
        choices
    }

    /// Settings → Cabin defaults, under the model rows.
    pub(super) fn ui_provider_rows(&mut self, ui: &mut egui::Ui) {
        self.ensure_providers();
        let dir = crate::config::config_dir();
        let ledger = self.consent().clone();
        let providers_list = self
            .harness
            .router
            .providers
            .list
            .clone()
            .unwrap_or_default();
        for p in &providers_list {
            let state = providers::provider_state(&dir, &ledger, p);
            ui.push_id(("provider-row", p.id.as_str()), |ui| {
                if crate::cards::settings_action(ui, &provider_label(p), state.tag(), "Remove") {
                    self.remove_provider_click(&p.id);
                }
            });
        }
        let models = self.provider_model_choices();
        if !models.is_empty() || !self.cfg.provider_model.is_empty() {
            let mut labels = vec![PICK_OFF.to_string()];
            labels.extend(models.iter().cloned());
            let selected = if self.cfg.provider_model.is_empty() {
                PICK_OFF.to_string()
            } else {
                self.cfg.provider_model.clone()
            };
            if let Some(i) =
                crate::cards::settings_dropdown(ui, PICK_ROW, PICK_HINT, &selected, &labels)
            {
                let next = if i == 0 {
                    String::new()
                } else {
                    models.get(i - 1).cloned().unwrap_or_default()
                };
                if next != self.cfg.provider_model {
                    self.cfg.provider_model = next;
                    self.persist_cfg();
                    self.status = "Saved".into();
                }
            }
        }
        crate::cards::settings_field(
            ui,
            URL_ROW,
            URL_HINT,
            &mut self.harness.router.providers.url,
            false,
        );
        let labels: Vec<String> = KIND_LABELS.iter().map(|l| l.to_string()).collect();
        let picked = labels[self.harness.router.providers.kind.min(2)].clone();
        if let Some(i) = crate::cards::settings_dropdown(ui, KIND_ROW, KIND_HINT, &picked, &labels)
        {
            self.harness.router.providers.kind = i;
        }
        if let Some(page) = providers::key_page(&self.harness.router.providers.url) {
            let hint = format!(
                "Opens {}'s key page in your browser. Paste the key below.",
                grokhub_agent::harness::egress_dest(&self.harness.router.providers.url)
            );
            if crate::cards::settings_action(ui, KEY_PAGE_ROW, &hint, "Open") {
                if let Err(why) = open_provider_page(page) {
                    self.status = format!("Your browser didn't open: {why}");
                    self.harness.router.providers.by_hand = Some(ByHand {
                        url: page.to_string(),
                        why,
                    });
                }
            }
        }
        crate::cards::settings_field(
            ui,
            KEY_ROW,
            KEY_HINT,
            &mut self.harness.router.providers.key,
            true,
        );
        if crate::cards::settings_action(ui, ADD_ROW, ADD_HINT, "Add") {
            self.add_provider_click();
        }
        if crate::cards::settings_action(ui, OPENROUTER_ROW, OPENROUTER_HINT, "Sign in") {
            self.start_openrouter_sign_in();
        }
        if self.harness.router.providers.signin_rx.is_some() {
            crate::cards::settings_note(ui, OPENROUTER_WAIT);
        }
        if let Some(link) = self.harness.router.providers.by_hand.clone() {
            crate::cards::settings_note(
                ui,
                &format!(
                    "Your browser didn't open ({}). Copy this link into your browser to go on.",
                    link.why
                ),
            );
            let id = ui.id().with("provider-by-hand-copy");
            let label = crate::theme::copy_flash_label(ui.ctx(), id);
            if crate::cards::settings_action(ui, BY_HAND_ROW, &link.url, label) {
                ui.ctx().copy_text(link.url.clone());
                crate::theme::mark_copy_clicked(ui.ctx(), id);
            }
        }
        if let Some(note) = &self.harness.router.providers.signin_note {
            crate::cards::settings_note(ui, note);
        }
    }

    /// Settings → Permissions, "Other providers": one destination row each.
    /// Allow parks the hard send card; only its Approve writes the grant.
    pub(super) fn ui_provider_grant_rows(&mut self, ui: &mut egui::Ui, locked: Option<&str>) {
        self.ensure_providers();
        let all = self
            .harness
            .router
            .providers
            .list
            .clone()
            .unwrap_or_default();
        if all.is_empty() {
            return;
        }
        crate::cards::section_heading(ui, PROVIDER_HEAD);
        for p in all {
            let grant = self
                .consent()
                .destination_grant(&p.dest(), CALL_DATA)
                .map(|g| (g.id.clone(), g.granted_at));
            let title = format!("Send to {}", p.dest());
            ui.push_id(("provider-grant", p.id.as_str()), |ui| match grant {
                None => {
                    let hint = "Off. Allow shows a card first.";
                    if crate::cards::settings_grant_row(
                        ui,
                        &title,
                        hint,
                        None,
                        crate::cards::GrantPill::Allow,
                        locked,
                        false,
                        |_| {},
                    ) && locked.is_none()
                    {
                        self.ask_provider(&p.id);
                    }
                }
                Some((id, at)) => {
                    let hint = format!(
                        "Allowed {}. Sends chats and memory.",
                        grokhub_core::pulse::ago_label(at, now_ms())
                    );
                    if crate::cards::settings_grant_row(
                        ui,
                        &title,
                        &hint,
                        None,
                        crate::cards::GrantPill::Revoke,
                        locked,
                        false,
                        |_| {},
                    ) && locked.is_none()
                    {
                        self.revoke_provider_grant(&id, &p.dest());
                    }
                }
            });
        }
        ui.add_space(8.0);
    }

    pub(super) fn revoke_provider_grant(&mut self, id: &str, dest: &str) {
        match hx::revoke_grant(&crate::config::config_dir(), id) {
            Ok(_) => {
                self.invalidate_providers();
                self.status =
                    format!("Sending to {dest} revoked. Its models stop at the next call.")
            }
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
        class == HardClass::Send
            && matches!(source, super::super::harness_ui::ParkSource::Provider(_))
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
        assert!(
            app.harness.router.providers.key.is_empty(),
            "the key field clears on Add"
        );
        // Saving the key is a hard credentials card first; nothing is in the keyring yet.
        assert!(!providers::has_key(&root, "openrouter"));
        let key_card = app.harness.park.clone().expect("a hard credentials card");
        assert_eq!(key_card.class, HardClass::Credentials);
        assert!(
            matches!(&key_card.source, super::super::harness_ui::ParkSource::ProviderKey(d) if d == "openrouter.ai")
        );
        assert_eq!(
            key_card.action,
            "Save your key for openrouter.ai to your keyring"
        );
        assert_eq!(
            app.status,
            "Approve the card to save the key for openrouter.ai."
        );
        app.resolve_hard_park(true, "");
        assert!(providers::has_key(&root, "openrouter"));
        assert!(app.harness.router.providers.pending.is_none());
        let park = app.harness.park.clone().expect("a hard send card");
        assert!(is_provider_card(park.class, &park.source));
        assert_eq!(
            park.action,
            "Send your chats and memory to openrouter.ai when you pick one of its models"
        );
        // Enter never approves it; Esc denies it, and Deny writes nothing.
        assert_eq!(hx::hard_card_key(true, false, false), None);
        assert_eq!(
            hx::hard_card_key(false, true, false),
            Some(hx::HardAnswer::Deny)
        );
        app.resolve_hard_park(false, "user denied (Esc)");
        assert!(app
            .consent()
            .destination_grant("openrouter.ai", CALL_DATA)
            .is_none());
        assert_eq!(providers::usable(&root, CALL_DATA), Vec::<String>::new());
        // Allow in Settings → Permissions parks it again; Approve (a click) grants.
        app.ask_provider("openrouter");
        app.resolve_hard_park(true, "");
        let g = app
            .consent()
            .destination_grant("openrouter.ai", CALL_DATA)
            .cloned()
            .expect("granted");
        assert_eq!(g.data_classes, CALL_DATA.to_vec());
        assert_eq!(
            providers::usable(&root, CALL_DATA),
            vec!["openrouter".to_string()]
        );
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
        assert!(
            app.harness.router.providers.key.is_empty(),
            "a refused key isn't kept in the field either"
        );
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
        assert_eq!(
            app.harness.router.providers.sent.as_deref(),
            Some("anthropic/model-x")
        );
        app.remove_provider_click("anthropic");
        assert!(
            app.cfg.provider_model.is_empty(),
            "removing a provider drops your pick"
        );
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
        app.resolve_hard_park(false, "user denied (Esc)");
        assert_eq!(
            app.status,
            "Nothing saved. The key for claude-proxy.example.org was dropped."
        );
        assert!(app.harness.router.providers.pending.is_none() && app.harness.park.is_none());
        assert!(
            providers::load_providers(&root).is_empty()
                && !providers::has_key(&root, "claude-proxy")
        );
        // Again, approved: the "Anthropic-compatible" pick is saved with it.
        app.harness.router.providers.key = KEY.into();
        app.add_provider_click();
        app.resolve_hard_park(true, "");
        let p = providers::provider(&root, "claude-proxy").expect("added");
        assert_eq!(p.kind, ProviderKind::Anthropic);
        assert_eq!(
            app.harness.router.providers.kind, 0,
            "the pick resets after a save"
        );
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
        assert!(
            app.harness.router.providers.signin_rx.is_some(),
            "still waiting on the browser"
        );
        tx.send(SignInMsg::Done(Ok(KEY.to_string()))).unwrap();
        app.poll_openrouter_sign_in();
        assert!(app.harness.router.providers.signin_rx.is_none());
        let card = app.harness.park.clone().expect("a hard credentials card");
        assert_eq!(
            (card.class, card.action.as_str()),
            (
                HardClass::Credentials,
                "Save your key for openrouter.ai to your keyring"
            )
        );
        assert!(
            !providers::has_key(&root, "openrouter"),
            "nothing saved before Approve"
        );
        app.resolve_hard_park(true, "");
        assert_eq!(
            providers::provider(&root, "openrouter").map(|p| p.base_url),
            Some("https://openrouter.ai/api/v1".to_string())
        );
        assert!(providers::has_key(&root, "openrouter"));
        let (tx, rx) = std::sync::mpsc::channel();
        app.harness.router.providers.signin_rx = Some(rx);
        tx.send(SignInMsg::Done(Err("OpenRouter sign-in timed out".into())))
            .unwrap();
        app.poll_openrouter_sign_in();
        assert_eq!(app.status, "OpenRouter sign-in timed out");
        assert_eq!(
            app.harness.router.providers.signin_note.as_deref(),
            Some("OpenRouter sign-in timed out")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// OpenRouter's key exchange, faked: `good-code` buys [`KEY`].
    #[derive(Default)]
    struct KeyExchange(std::sync::Mutex<Vec<String>>);

    impl providers::ProviderTransport for KeyExchange {
        fn get(&self, url: &str, _: &[(String, String)]) -> Result<providers::HttpReply, String> {
            Err(format!("no GET expected: {url}"))
        }
        fn post(
            &self,
            url: &str,
            _: &[(String, String)],
            body: &str,
        ) -> Result<providers::HttpReply, String> {
            self.0.lock().unwrap().push(url.to_string());
            let ok = body.contains(r#""code":"good-code""#);
            Ok(providers::HttpReply {
                status: if ok { 200 } else { 403 },
                body: if ok {
                    format!(r#"{{"key":"{KEY}"}}"#)
                } else {
                    "{}".into()
                },
            })
        }
    }

    fn texts_of(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn openrouter_pages_open_in_the_system_browser_not_behind_the_xai_gate() {
        let auth = providers::openrouter_auth_url("http://127.0.0.1:51423/callback", "abc_-123");
        assert_eq!(
            auth,
            "https://openrouter.ai/auth?callback_url=http%3A%2F%2F127.0.0.1%3A51423%2Fcallback&code_challenge=abc_-123&code_challenge_method=S256"
        );
        // The old opener only let x.ai through, so this page never opened.
        assert!(grokhub_core::trusted_xai_url(&auth).is_err());
        assert!(provider_page_ok(&auth));
        // Its `&`s are fine: the URL goes to ShellExecuteW / xdg-open as one argument, never cmd.
        assert!(crate::desktop::url_safe_to_open(&auth));
        for page in [
            "https://openrouter.ai/settings/keys",
            "https://platform.openai.com/api-keys",
            "https://console.anthropic.com/settings/keys",
        ] {
            assert!(provider_page_ok(page), "{page}");
        }
        for bad in [
            "https://openrouter.ai/auth?",
            "http://openrouter.ai/auth?callback_url=x",
            "https://openrouter.ai.evil.com/auth?x=1",
            "https://evil.com/settings/keys",
            "https://openrouter.ai/settings/keys/../../x",
        ] {
            assert!(!provider_page_ok(bad), "{bad}");
            assert!(open_provider_page(bad).is_err(), "{bad}");
        }
        // Sign in and Get a key both go through the provider opener.
        let src = include_str!("provider_ui.rs");
        let xai_gate = ["oauth::", "open_browser("].concat();
        assert!(
            !src.contains(&xai_gate),
            "provider pages must not use the xAI-only opener"
        );
        let start = src
            .split("pub(super) fn start_openrouter_sign_in(&mut self)")
            .nth(1)
            .and_then(|s| s.split("pub(super) fn ").next())
            .expect("start_openrouter_sign_in");
        assert!(start.contains("open_provider_page"), "{start}");
    }

    #[test]
    fn a_browser_that_does_not_open_shows_the_link_with_copy_and_the_sign_in_still_finishes() {
        let _paint = crate::theme::hold_paint_test();
        let (_pin, root) = pinned("r3b-openrouter-by-hand");
        let wire = Arc::new(KeyExchange::default());
        providers::use_transport_for(&root, wire.clone());
        let mut app = Cabin::quiet_for_test();
        let asked = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = asked.clone();
        app.start_openrouter_sign_in_with(move |url| {
            seen.lock().unwrap().push(url.to_string());
            Err("no browser here".into())
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while app.harness.router.providers.by_hand.is_none() {
            assert!(std::time::Instant::now() < deadline, "no link came back");
            app.poll_openrouter_sign_in();
            std::thread::sleep(Duration::from_millis(20));
        }
        let link = app.harness.router.providers.by_hand.clone().unwrap();
        assert_eq!(*asked.lock().unwrap(), vec![link.url.clone()]);
        assert_eq!(link.why, "no browser here");
        assert!(
            link.url
                .starts_with("https://openrouter.ai/auth?callback_url=http%3A%2F%2F127.0.0.1%3A")
                && link.url.contains("%2Fcallback&code_challenge=")
                && link.url.ends_with("&code_challenge_method=S256"),
            "{}",
            link.url
        );
        assert!(
            app.harness.router.providers.signin_rx.is_some(),
            "the sign-in keeps waiting for its callback"
        );

        // Settings shows the link with Copy under the Sign in row.
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let out = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
            app.ui_provider_rows(ui);
        });
        let texts = texts_of(&out.shapes);
        for want in [
            OPENROUTER_ROW,
            OPENROUTER_WAIT,
            BY_HAND_ROW,
            link.url.as_str(),
            "Copy",
        ] {
            assert!(
                texts.iter().any(|t| t == want),
                "missing {want:?} in {texts:?}"
            );
        }
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Your browser didn't open (no browser here)")),
            "{texts:?}"
        );

        // Opening the copied link by hand: OpenRouter calls the loopback back.
        let callback = link
            .url
            .split("callback_url=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .unwrap()
            .replace("%3A", ":")
            .replace("%2F", "/");
        let host = callback
            .trim_start_matches("http://")
            .trim_end_matches("/callback")
            .to_string();
        {
            use std::io::{Read, Write};
            let mut sock = std::net::TcpStream::connect(&host).unwrap();
            write!(
                sock,
                "GET /callback?code=good-code HTTP/1.1\r\nHost: {host}\r\n\r\n"
            )
            .unwrap();
            let mut page = String::new();
            let _ = sock.read_to_string(&mut page);
            assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
        }
        while app.harness.router.providers.signin_rx.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "the sign-in never finished"
            );
            app.poll_openrouter_sign_in();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            wire.0.lock().unwrap().as_slice(),
            ["https://openrouter.ai/api/v1/auth/keys".to_string()]
        );
        assert!(app.harness.router.providers.by_hand.is_none());
        let card = app.harness.park.clone().expect("the credentials card");
        assert_eq!(
            (card.class, card.action.as_str()),
            (
                HardClass::Credentials,
                "Save your key for openrouter.ai to your keyring"
            )
        );
        assert!(
            !providers::has_key(&root, "openrouter"),
            "nothing saved before Approve"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_refresh_names_the_models_a_provider_added_or_that_it_turned_the_key_down() {
        let or = providers::Provider {
            id: "openrouter".into(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: "https://openrouter.ai/api/v1".into(),
            added_at: 0,
        };
        let an = providers::Provider {
            id: "anthropic".into(),
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            added_at: 0,
        };
        let added = |ids: &[&str]| {
            ids.iter()
                .map(|id| RegistryEvent::Added { id: id.to_string() })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            provider_refresh_note(
                &added(&["openrouter/vendor/a", "grok-4.7", "openrouter/vendor/b"]),
                &[],
                &[or.clone(), an.clone()]
            )
            .as_deref(),
            Some("Added 2 models from openrouter.ai: vendor/a, vendor/b")
        );
        assert_eq!(
            provider_refresh_note(
                &added(&["anthropic/claude-x"]),
                &[],
                &[or.clone(), an.clone()]
            )
            .as_deref(),
            Some("Added 1 model from api.anthropic.com: claude-x")
        );
        assert_eq!(
            provider_refresh_note(
                &added(&[
                    "openrouter/a",
                    "openrouter/b",
                    "openrouter/c",
                    "openrouter/d",
                    "openrouter/e"
                ]),
                &[],
                std::slice::from_ref(&or)
            )
            .as_deref(),
            Some("Added 5 models from openrouter.ai: a, b, c, and 2 more")
        );
        assert_eq!(
            provider_refresh_note(
                &[],
                &["anthropic: HTTP 401".into(), "openrouter: HTTP 500".into()],
                &[or.clone(), an]
            )
            .as_deref(),
            Some("api.anthropic.com turned the key down. Remove it and add a new one.")
        );
        assert_eq!(
            provider_refresh_note(&added(&["grok-4.7"]), &[], &[or]),
            None
        );
    }

    /// Settings paint must not call `load_providers` / `usable` / raw `std::fs`.
    /// The cabin holds a cache writers invalidate.
    #[test]
    fn settings_provider_paint_does_not_touch_disk() {
        fn body<'a>(src: &'a str, name: &str) -> &'a str {
            let needle = format!("fn {name}(");
            let start = src
                .find(&needle)
                .unwrap_or_else(|| panic!("missing {name}"));
            let after = &src[start..];
            let end = after[needle.len()..]
                .find("\n    fn ")
                .or_else(|| after[needle.len()..].find("\nfn "))
                .map(|i| needle.len() + i)
                .unwrap_or(after.len());
            &after[..end]
        }
        let src = include_str!("provider_ui.rs").replace("\r\n", "\n");
        for name in ["ui_provider_rows", "ui_provider_grant_rows"] {
            let paint = body(&src, name);
            assert!(
                !paint.contains("load_providers(")
                    && !paint.contains("usable(")
                    && !paint.contains("std::fs::"),
                "{name} must paint from the cache: {paint}"
            );
            assert!(
                paint.contains("ensure_providers"),
                "{name} must call ensure_providers: {paint}"
            );
        }
        let choices = body(&src, "provider_model_choices");
        assert!(
            !choices.contains("usable(") && !choices.contains("load_providers("),
            "model choices must not re-read providers.json via usable/load: {choices}"
        );
        assert!(
            choices.contains("model_choices") && choices.contains("ensure_providers"),
            "model choices use the cache: {choices}"
        );
    }

    #[test]
    fn provider_list_cache_survives_deleting_the_file_until_invalidate() {
        let (_pin, root) = pinned("r3b-provider-cache");
        let mut app = Cabin::quiet_for_test();
        providers::add_provider_as(&root, "https://openrouter.ai/api/v1", KEY, None, now_ms())
            .expect("add");
        app.ensure_providers();
        assert_eq!(
            app.harness.router.providers.list.as_ref().map(|v| v.len()),
            Some(1)
        );
        let path = providers::providers_path(&root);
        std::fs::remove_file(&path).expect("delete providers.json");
        // Paint path: ensure again must keep the in-memory row.
        app.ensure_providers();
        assert_eq!(
            app.harness
                .router
                .providers
                .list
                .as_ref()
                .map(|v| v.iter().map(|p| p.id.as_str()).collect::<Vec<_>>()),
            Some(vec!["openrouter"])
        );
        app.invalidate_providers();
        assert!(app.harness.router.providers.list.is_none());
        app.ensure_providers();
        assert_eq!(
            app.harness.router.providers.list.as_ref().map(|v| v.len()),
            Some(0),
            "after invalidate, a fresh load sees the deleted file"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

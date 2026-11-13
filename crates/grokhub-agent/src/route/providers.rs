//! Router R3b: other AI providers you add with your own key (§14.1, §14.4.1,
//! §14.4.5, §14.7). xAI stays first and built in. An OpenAI-compatible
//! service (OpenRouter and the like) or Anthropic is **off until you add a
//! key and allow the destination**:
//!
//! - The key goes into the OS keyring ([`KeyVault`]: Secret Service, Windows
//!   Credential Manager, Keychain) and nowhere else: never `app.json`, a span,
//!   the consent ledger, a log, an egress line or the model's input.
//!   `models/providers.json` holds only the id, kind and base URL.
//! - Every request ([`ProviderSource`]'s model list and [`call_model`]) asks
//!   `harness::decide` ([`Step::Provider`](crate::harness::Step)) through
//!   [`guard_provider`] / [`provider_or_park`]: it goes only under your
//!   destination grant covering every data class it carries, else it is a
//!   hard send card. Each one that goes writes one egress line (no content).
//! - Its models are cost class `new_provider`: the router takes one only when
//!   the key and the grant exist (the grant covering the request's data, so
//!   sensitive data needs a grant that names it), its listed price is under
//!   your $/M ceiling and it answers in text. It ranks after xAI unless the
//!   routing table ranks it.
//! - Effort maps per provider: xAI `reasoning.effort` (unchanged), Anthropic
//!   `output_config.effort`, OpenAI-compatible `reasoning.effort`. The effort
//!   list, prices and context come from the listing; nothing is filled in.
//!
//! Never Grok Build's credentials and never `~/.grok`.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use sha2::{Digest, Sha256};

use grokhub_core::model_registry::cost_class::usd_per_m;
use grokhub_core::model_registry::discover::{
    parse_anthropic_catalog, parse_openai_compatible_catalog, split_provider_model,
};
use grokhub_core::model_registry::probe::DAY_MS;
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::store::{models_dir, read_json, write_json};
use grokhub_core::model_registry::{CatalogSource, Listing, ModelMeta, Registry, SourceKind};

use crate::client::{
    map_http_status, AuthKind, ClientError, ContentPart, FunctionCall, InputItem, ResponsesRequest,
    StreamEvent, TurnOutput, Usage, USER_AGENT,
};
use crate::harness::{
    egress_dest, guard_provider, is_model_host, provider_or_park, ConsentLedger, DataClass,
    EgressReq, GateOutcome,
};
use crate::CancelToken;

/// `{config}/models/providers.json`: id, kind and base URL only. No key.
pub const PROVIDERS_FILE: &str = "providers.json";
/// Span and park-card tool name for a provider call.
pub const PROVIDER_TOOL: &str = "model.provider";
/// The OS keyring service the keys live under (account = provider id).
pub const KEY_SERVICE: &str = "GrokHub provider keys";
/// What a provider call carries: chat plus memory, like every native model call.
pub const CALL_DATA: &[DataClass] = &[DataClass::Chat, DataClass::Personal];
/// The same with the step's data marked sensitive.
pub const CALL_DATA_SENSITIVE: &[DataClass] =
    &[DataClass::Chat, DataClass::Personal, DataClass::Sensitive];
/// Anthropic's API version header.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Anthropic requires `max_tokens`; this is used when the listing gives none.
pub const ANTHROPIC_MAX_TOKENS: u64 = 8_192;
/// A reply or listing larger than this is refused.
const BODY_CAP: u64 = 8 * 1024 * 1024;
/// Provider ids GrokHub keeps for itself.
const RESERVED_IDS: &[&str] = &["xai", "local", "grok_build", "grok", "x"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// `/models` and `/chat/completions` (OpenRouter and the like).
    OpenAiCompatible,
    /// `/v1/models` and `/v1/messages`.
    Anthropic,
}

impl ProviderKind {
    pub fn source_kind(self) -> SourceKind {
        match self {
            Self::OpenAiCompatible => SourceKind::OpenAiCompatible,
            Self::Anthropic => SourceKind::Anthropic,
        }
    }

    /// The request field the effort goes in.
    pub fn effort_field(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "reasoning.effort",
            Self::Anthropic => "output_config.effort",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "OpenAI-compatible",
            Self::Anthropic => "Anthropic",
        }
    }
}

/// One provider you added. The key is in the keyring, not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub kind: ProviderKind,
    /// `https://openrouter.ai/api/v1`, `https://api.anthropic.com`.
    pub base_url: String,
    #[serde(default)]
    pub added_at: u64,
}

impl Provider {
    /// The egress destination (host only), what the grant names.
    pub fn dest(&self) -> String {
        egress_dest(&self.base_url)
    }

    /// An OpenAI-compatible provider at openrouter.ai.
    pub fn is_openrouter(&self) -> bool {
        self.kind == ProviderKind::OpenAiCompatible && is_openrouter_host(&self.dest())
    }

    fn api_root(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        match self.kind {
            ProviderKind::OpenAiCompatible => base.to_string(),
            ProviderKind::Anthropic if base.ends_with("/v1") => base.to_string(),
            ProviderKind::Anthropic => format!("{base}/v1"),
        }
    }

    pub fn models_url(&self) -> String {
        format!("{}/models", self.api_root())
    }

    pub fn call_url(&self) -> String {
        match self.kind {
            ProviderKind::OpenAiCompatible => format!("{}/chat/completions", self.api_root()),
            ProviderKind::Anthropic => format!("{}/messages", self.api_root()),
        }
    }

    /// The auth headers. Built at send time from the keyring; never logged.
    fn headers(&self, key: &str) -> Vec<(String, String)> {
        let mut h = vec![
            ("User-Agent".to_string(), USER_AGENT.to_string()),
            ("Content-Type".into(), "application/json".into()),
        ];
        match self.kind {
            ProviderKind::OpenAiCompatible => {
                h.push(("Authorization".into(), format!("Bearer {key}")))
            }
            ProviderKind::Anthropic => {
                h.push(("x-api-key".into(), key.to_string()));
                h.push(("anthropic-version".into(), ANTHROPIC_VERSION.into()));
            }
        }
        h
    }
}

/// Check a base URL from Settings: https only, a host, not xAI (built in).
/// Returns the trimmed URL, its kind and the provider id (`openrouter`).
pub fn parse_base_url(raw: &str) -> Result<(String, ProviderKind, String), String> {
    let url = raw.trim().trim_end_matches('/');
    let Some(rest) = url.strip_prefix("https://") else {
        return Err("Use an https:// address.".into());
    };
    let authority = rest.split(['/', '\\', '?', '#']).next().unwrap_or("");
    if authority.contains('@') || url.contains(char::is_whitespace) {
        return Err("That address isn't a plain https URL.".into());
    }
    // Same public-address rail as Pulse / web_fetch: no loopback, RFC1918,
    // link-local (incl. cloud metadata), or CGNAT literals. Local models use
    // the sidecar path, not a custom provider base URL.
    if !grokhub_core::public_http_url(url) {
        return Err("That address isn't a public https URL.".into());
    }
    let host = egress_dest(url);
    if host.is_empty() || !host.contains('.') {
        return Err("That address has no host.".into());
    }
    if is_model_host(&host) {
        return Err("xAI is already built in.".into());
    }
    let kind = if host == "anthropic.com" || host.ends_with(".anthropic.com") {
        ProviderKind::Anthropic
    } else {
        ProviderKind::OpenAiCompatible
    };
    let labels: Vec<&str> = host.split('.').collect();
    let name = labels
        .iter()
        .copied()
        .find(|l| !matches!(*l, "api" | "www" | "gateway"))
        .unwrap_or(labels[0]);
    let id: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_ascii_lowercase();
    if id.is_empty() || RESERVED_IDS.contains(&id.as_str()) {
        return Err("That provider name is reserved.".into());
    }
    Ok((url.to_string(), kind, id))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ProviderList {
    #[serde(default)]
    providers: Vec<Provider>,
}

pub fn providers_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(PROVIDERS_FILE)
}

/// The providers you added, oldest first. None on a fresh config.
///
/// A key is saved under the provider's id, and the id comes from the address.
/// A row whose address no longer passes [`parse_base_url`] (plain http, a
/// private host) or no longer gives that id (the file was edited to point at
/// another host) is skipped, so its key is never sent there.
pub fn load_providers(config_dir: &Path) -> Vec<Provider> {
    saved_rows(config_dir)
        .into_iter()
        .filter(|p| parse_base_url(&p.base_url).is_ok_and(|(_, _, id)| id == p.id))
        .collect()
}

/// Every row in the saved file, the ones [`load_providers`] skips too. Adding
/// and removing write this list back, so a skipped row is not erased with its
/// key left behind in the keyring, and it can still be removed.
fn saved_rows(config_dir: &Path) -> Vec<Provider> {
    read_json::<ProviderList>(&providers_path(config_dir))
        .map(|l| l.providers)
        .unwrap_or_default()
}

/// An id an address can give ([`parse_base_url`]): letters, digits and `-`.
/// A hand-written row with any other id is never matched by it, so the id
/// never names a keyring entry or a file.
fn id_shaped(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn save_providers(config_dir: &Path, providers: &[Provider]) -> Result<(), String> {
    write_json(
        &providers_path(config_dir),
        &ProviderList {
            providers: providers.to_vec(),
        },
    )
}

pub fn provider(config_dir: &Path, id: &str) -> Option<Provider> {
    load_providers(config_dir)
        .into_iter()
        .find(|p| p.id == id.trim())
}

/// Playbooks 8: which provider a `provider/model` id goes to, for the pre-send check.
pub fn dest(config_dir: &Path, model: &str) -> grokhub_core::adult_guard::Dest {
    use grokhub_core::adult_guard::{dest_of, Dest};
    let Some(p) = split_provider_model(model).and_then(|(pid, _)| provider(config_dir, pid)) else {
        return Dest::Other;
    };
    match p.kind {
        ProviderKind::Anthropic => Dest::Anthropic,
        ProviderKind::OpenAiCompatible => dest_of(&p.base_url),
    }
}

/// Settings → "Add a provider": the key goes into the keyring first; only if
/// that worked is the provider (no key) saved. Adding sends nothing: the
/// destination is allowed separately, on a hard send card.
pub fn add_provider(
    config_dir: &Path,
    base_url: &str,
    key: &str,
    now_ms: u64,
) -> Result<Provider, String> {
    add_provider_as(config_dir, base_url, key, None, now_ms)
}

/// A pasted key: whole, one word.
pub fn check_key(key: &str) -> Result<&str, String> {
    let key = key.trim();
    if key.len() < 8 || key.contains(char::is_whitespace) {
        return Err("Paste the whole key.".into());
    }
    Ok(key)
}

/// What adding says when the key can't go into the keyring.
pub const KEYRING_OFF: &str = "The keyring isn't available, so the key wasn't saved.";
/// What removing says for an id that isn't in the saved list.
pub const NOT_ADDED: &str = "That provider isn't added.";

/// [`add_provider`] with the API shape you picked; `None` detects it from the host.
/// A proxy that speaks Anthropic's API under another host needs the pick.
pub fn add_provider_as(
    config_dir: &Path,
    base_url: &str,
    key: &str,
    kind: Option<ProviderKind>,
    now_ms: u64,
) -> Result<Provider, String> {
    let (url, detected, id) = parse_base_url(base_url)?;
    let kind = kind.unwrap_or(detected);
    let key = check_key(key)?;
    // The keyring library's own message says nothing a person can act on.
    vault_for(config_dir)
        .ok_or_else(|| KEYRING_OFF.to_string())?
        .set(&id, key)
        .map_err(|_| KEYRING_OFF.to_string())?;
    forget_key_cache(config_dir, &id);
    forget_listing(config_dir, &id);
    let mut all = saved_rows(config_dir);
    let p = Provider {
        id: id.clone(),
        kind,
        base_url: url,
        added_at: now_ms,
    };
    match all.iter_mut().find(|x| x.id == id) {
        Some(slot) => *slot = p.clone(),
        None => all.push(p.clone()),
    }
    save_providers(config_dir, &all)?;
    Ok(p)
}

/// Remove a provider and its key. Its grant stays in Settings → Permissions until you revoke it.
///
/// Only an id in the saved list is removed, so the caller's text never names a
/// keyring entry or a file. While the keyring can't delete the key the
/// provider stays listed: the row is the only sign a key may still be there.
/// A row [`load_providers`] skips is in the saved list too: it goes with its key.
pub fn remove_provider(config_dir: &Path, id: &str) -> Result<(), String> {
    let mut all = saved_rows(config_dir);
    if !id_shaped(id) || !all.iter().any(|p| p.id == id) {
        return Err(NOT_ADDED.into());
    }
    if let Some(v) = vault_for(config_dir) {
        v.delete(id).map_err(|_| {
            format!("The keyring isn't available, so {id} wasn't removed. Try again when it is.")
        })?;
    }
    forget_key_cache(config_dir, id);
    forget_listing(config_dir, id);
    all.retain(|p| p.id != id);
    save_providers(config_dir, &all)
}

// ---- Getting a key ------------------------------------------------------------

pub const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";
const OPENROUTER_AUTH: &str = "https://openrouter.ai/auth";
const OPENROUTER_KEYS: &str = "https://openrouter.ai/api/v1/auth/keys";

fn host_is(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|rest| rest.ends_with('.'))
}

/// Where a known provider hands out API keys, from its address. `None` for
/// one GrokHub doesn't know: its dashboard has the key.
pub fn key_page(base_url: &str) -> Option<&'static str> {
    let host = egress_dest(base_url);
    [
        ("openai.com", "https://platform.openai.com/api-keys"),
        (
            "anthropic.com",
            "https://console.anthropic.com/settings/keys",
        ),
        ("openrouter.ai", "https://openrouter.ai/settings/keys"),
        ("groq.com", "https://console.groq.com/keys"),
        ("mistral.ai", "https://console.mistral.ai/api-keys"),
        ("deepseek.com", "https://platform.deepseek.com/api_keys"),
    ]
    .iter()
    .find(|(domain, _)| host_is(&host, domain))
    .map(|(_, page)| *page)
}

/// The name the new key gets on OpenRouter's keys page.
const OPENROUTER_KEY_LABEL: &str = "Loadout";

/// OpenRouter's browser sign-in page (its OAuth PKCE flow) with our loopback.
/// OpenRouter hands `state` back unchanged on the callback.
pub fn openrouter_auth_url(callback: &str, challenge: &str, state: &str) -> String {
    let query = grokhub_core::pkce::form(&[
        ("callback_url", callback),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("key_label", OPENROUTER_KEY_LABEL),
    ]);
    format!("{OPENROUTER_AUTH}?{query}")
}

/// Sign in to OpenRouter in the browser `open` launches and come back with a
/// new key. Nothing is saved here: the cabin asks you on a credentials card
/// first. The callback must carry this sign-in's `state`, and the code only
/// buys a key together with this verifier (PKCE). The exchange carries no
/// user data.
pub fn openrouter_key(
    config_dir: &Path,
    open: &dyn Fn(&str) -> Result<(), String>,
    wait: Duration,
) -> Result<Zeroizing<String>, String> {
    let (listener, redirect) = crate::loopback::bind()?;
    let verifier = grokhub_core::pkce::pkce_verifier();
    let state = grokhub_core::pkce::oauth_nonce();
    open(&openrouter_auth_url(
        &redirect,
        &grokhub_core::pkce::pkce_challenge(&verifier),
        &state,
    ))?;
    let code = crate::loopback::wait_code(&listener, &state, "OpenRouter sign-in", wait)?;
    crate::harness::guard_quiet(config_dir, &EgressReq::new(OPENROUTER_KEYS, &[]))?;
    let body = json!({"code": code, "code_verifier": verifier, "code_challenge_method": "S256"})
        .to_string();
    let headers = [
        ("User-Agent".to_string(), USER_AGENT.to_string()),
        ("Content-Type".into(), "application/json".into()),
    ];
    let r = transport_for(config_dir).post(OPENROUTER_KEYS, &headers, &body)?;
    if !(200..300).contains(&r.status) {
        return Err(format!(
            "OpenRouter didn't hand over a key (HTTP {})",
            r.status
        ));
    }
    let v: Value =
        serde_json::from_str(&r.body).map_err(|_| "OpenRouter's answer had no key".to_string())?;
    let key = v
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .ok_or("OpenRouter's answer had no key")?;
    Ok(Zeroizing::new(key.to_string()))
}

// ---- The keyring -----------------------------------------------------------

/// Where provider keys live. `get` = `Ok(None)` when there is no entry.
pub trait KeyVault: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<Zeroizing<String>>, String>;
    fn set(&self, id: &str, key: &str) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
}

/// The OS keyring through `keyring` 4: Secret Service, Windows Credential Manager, Keychain.
pub struct OsKeyVault;

impl KeyVault for OsKeyVault {
    fn get(&self, id: &str) -> Result<Option<Zeroizing<String>>, String> {
        let entry = keyring::Entry::new(KEY_SERVICE, id).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(k) => Ok(Some(Zeroizing::new(k))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, id: &str, key: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(KEY_SERVICE, id).map_err(|e| e.to_string())?;
        entry.set_password(key).map_err(|e| e.to_string())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(KEY_SERVICE, id).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// In-memory vault for tests. Never touches the OS.
#[derive(Default)]
pub struct MemoryVault(Mutex<BTreeMap<String, String>>);

impl KeyVault for MemoryVault {
    fn get(&self, id: &str) -> Result<Option<Zeroizing<String>>, String> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
            .map(Zeroizing::new))
    }

    fn set(&self, id: &str, key: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.to_string(), key.to_string());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
        Ok(())
    }
}

pub(super) type VaultSlot = Option<Arc<dyn KeyVault>>;
static DEFAULT_VAULT: Mutex<VaultSlot> = Mutex::new(None);
static VAULTS: Mutex<Vec<(PathBuf, Arc<dyn KeyVault>)>> = Mutex::new(Vec::new());
/// Whether a key is there, per (config dir, provider), so routing doesn't ask the keyring every call.
static HAS_KEY: Mutex<BTreeMap<(PathBuf, String), KeyKnown>> = Mutex::new(BTreeMap::new());

/// How long a keyring that could not be read is left alone before it is asked
/// again. Routing asks on every call and the cabin's Settings on every repaint.
pub const KEY_RETRY: Duration = Duration::from_secs(5);

/// What is known about one provider's key.
#[derive(Clone, Copy)]
enum KeyKnown {
    /// The keyring answered: a key is saved, or none is.
    Answered(bool),
    /// No keyring yet, or it could not be read (locked, not started). That is
    /// not an answer, so it is asked again from this moment on.
    AskAgainAt(Instant),
}

/// `grokhub` calls this once at start. Until then (and in tests) there is no
/// vault: no key reads as present and nothing can be saved.
pub fn use_os_vault() {
    // Calling it again keeps the vault and what it already answered.
    if DEFAULT_VAULT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
    {
        return;
    }
    use_default_vault(Some(Arc::new(OsKeyVault)));
}

/// The vault every config dir without one of its own uses. What the vault
/// before it answered is forgotten.
pub(super) fn use_default_vault(vault: VaultSlot) {
    *DEFAULT_VAULT.lock().unwrap_or_else(|e| e.into_inner()) = vault;
    HAS_KEY.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Use `vault` for this config dir only (tests).
pub fn use_vault_for(config_dir: &Path, vault: Arc<dyn KeyVault>) {
    let mut v = VAULTS.lock().unwrap_or_else(|e| e.into_inner());
    v.retain(|(d, _)| d != config_dir);
    v.push((config_dir.to_path_buf(), vault));
    HAS_KEY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(d, _), _| d != config_dir);
}

fn vault_for(config_dir: &Path) -> Option<Arc<dyn KeyVault>> {
    let over = VAULTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(d, _)| d == config_dir)
        .map(|(_, v)| v.clone());
    over.or_else(|| {
        DEFAULT_VAULT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    })
}

fn forget_key_cache(config_dir: &Path, id: &str) {
    HAS_KEY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&(config_dir.to_path_buf(), id.to_string()));
}

/// The key the keyring holds for `id`. `Err` when there is no keyring yet or
/// it could not be read, which says nothing about whether a key is saved.
fn read_key(config_dir: &Path, id: &str) -> Result<Option<Zeroizing<String>>, ()> {
    let _lap = crate::timing::lap("route:provider_key_read");
    let found = vault_for(config_dir).ok_or(())?.get(id).map_err(|_| ())?;
    Ok(found.filter(|k| !k.trim().is_empty()))
}

fn key(config_dir: &Path, id: &str) -> Option<Zeroizing<String>> {
    read_key(config_dir, id).ok().flatten()
}

/// A key for this provider is in the keyring. Only an answer the keyring gave
/// is remembered for good: with no keyring yet, or one that could not be read
/// (locked, not started), this is `false` for now and the keyring is asked
/// again after [`KEY_RETRY`].
pub fn has_key(config_dir: &Path, id: &str) -> bool {
    has_key_at(config_dir, id, Instant::now())
}

/// [`has_key`] at `now`.
pub(super) fn has_key_at(config_dir: &Path, id: &str, now: Instant) -> bool {
    let slot = (config_dir.to_path_buf(), id.to_string());
    let known = HAS_KEY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&slot)
        .copied();
    match known {
        Some(KeyKnown::Answered(have)) => return have,
        Some(KeyKnown::AskAgainAt(at)) if now < at => return false,
        _ => {}
    }
    let began = Instant::now();
    let known = match read_key(config_dir, id) {
        Ok(found) => KeyKnown::Answered(found.is_some()),
        // A keyring that took long to fail is left alone for longer.
        Err(()) => KeyKnown::AskAgainAt(now + KEY_RETRY.max(began.elapsed() * 10)),
    };
    HAS_KEY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(slot, known);
    matches!(known, KeyKnown::Answered(true))
}

// ---- What the router may use -------------------------------------------------

/// The data classes a call carries.
pub fn call_data(sensitive: bool) -> &'static [DataClass] {
    if sensitive {
        CALL_DATA_SENSITIVE
    } else {
        CALL_DATA
    }
}

/// Provider ids a call carrying `data` may route to right now: a key in the
/// keyring **and** an active destination grant covering every class in
/// `data`. A locked ledger allows none.
pub fn usable(config_dir: &Path, data: &[DataClass]) -> Vec<String> {
    let _lap = crate::timing::lap("route:providers_usable");
    let all = load_providers(config_dir);
    if all.is_empty() {
        return Vec::new();
    }
    let ledger = ConsentLedger::load(config_dir);
    if ledger.locked().is_some() {
        return Vec::new();
    }
    all.into_iter()
        .filter(|p| {
            ledger.destination_grant(&p.dest(), data).is_some() && has_key(config_dir, &p.id)
        })
        .map(|p| p.id)
        .collect()
}

/// How a provider reads in Settings: what's missing before Auto may use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderState {
    /// The key is gone from the keyring.
    NoKey,
    /// Key saved, destination not allowed.
    NotAllowed,
    /// Key and grant: its models can be picked.
    Ready,
}

impl ProviderState {
    pub fn tag(self) -> &'static str {
        match self {
            Self::NoKey => "No key",
            Self::NotAllowed => "Not allowed yet",
            Self::Ready => "Allowed",
        }
    }
}

pub fn provider_state(config_dir: &Path, ledger: &ConsentLedger, p: &Provider) -> ProviderState {
    if !has_key(config_dir, &p.id) {
        ProviderState::NoKey
    } else if ledger.destination_grant(&p.dest(), CALL_DATA).is_none() {
        ProviderState::NotAllowed
    } else {
        ProviderState::Ready
    }
}

// ---- The wire ----------------------------------------------------------------

/// One HTTP answer: status, body (capped) and the reply's headers
/// (names lowercased; the quota ledger reads Anthropic's rate limits
/// from them). Fakes answer with no headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    pub status: u16,
    pub body: String,
    pub headers: Vec<(String, String)>,
}

/// A streaming answer in flight: status and headers up front, the body
/// still arriving on `reader`. The caller reads it as it comes (a
/// server-sent event stream) or, on an error status, reads it whole as
/// the error body.
pub struct HttpStream {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub reader: Box<dyn Read + Send>,
}

/// How provider requests leave. Tests use a counting fake: zero network.
pub trait ProviderTransport: Send + Sync {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpReply, String>;
    fn post(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<HttpReply, String>;
    /// `post` that gives up after `limit` (the request's `call_timeout`).
    /// Fakes that never block can keep this default.
    fn post_within(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
        limit: Option<Duration>,
    ) -> Result<HttpReply, String> {
        let _ = limit;
        self.post(url, headers, body)
    }
    /// `post` whose body the caller reads as it arrives. The default
    /// waits for the whole reply and hands it over at once, which is
    /// what fakes do; the real wire streams.
    fn post_streamed(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
        limit: Option<Duration>,
    ) -> Result<HttpStream, String> {
        let r = self.post_within(url, headers, body, limit)?;
        Ok(HttpStream {
            status: r.status,
            headers: r.headers,
            reader: Box::new(std::io::Cursor::new(r.body.into_bytes())),
        })
    }
}

/// The real wire. Every call reaches it only after [`guard_provider`].
pub struct UreqTransport;

fn agent() -> grokhub_core::http::Agent {
    // No env proxy: HTTP(S)_PROXY would dial the proxy and skip public_only_resolve
    // on the target (same as web_fetch / Pulse). `public_only` never reads it.
    grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(20),
        overall: Some(Duration::from_secs(600)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    })
}

/// A raw answer with its connectivity noted, the way every provider
/// request reports back; the caller decides how much of the body to read.
fn noted(
    r: Result<grokhub_core::http::Response, grokhub_core::http::Error>,
) -> Result<grokhub_core::http::Response, String> {
    match r {
        Ok(resp) if resp.status() < 400 => {
            grokhub_core::connectivity::note_global_transport_success(grokhub_core::now_ms());
            Ok(resp)
        }
        Ok(resp) => {
            grokhub_core::connectivity::note_global_transport_hint(
                grokhub_core::connectivity::classify_http_status(resp.status()),
                grokhub_core::now_ms(),
            );
            Ok(resp)
        }
        Err(err) => {
            super::connectivity::note_http_transport(&err);
            Err(err.to_string())
        }
    }
}

/// Every request is sent with `any_status`, so a 4xx/5xx comes back as an answer.
fn reply(
    r: Result<grokhub_core::http::Response, grokhub_core::http::Error>,
) -> Result<HttpReply, String> {
    let resp = noted(r)?;
    let status = resp.status();
    let headers = resp.headers();
    let mut body = String::new();
    resp.into_reader()
        .take(BODY_CAP)
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;
    Ok(HttpReply {
        status,
        body,
        headers,
    })
}

impl ProviderTransport for UreqTransport {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpReply, String> {
        let mut req = agent().get(url).any_status();
        for (k, v) in headers {
            req = req.header(k, v);
        }
        reply(req.call())
    }

    fn post(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<HttpReply, String> {
        self.post_within(url, headers, body, None)
    }

    fn post_within(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
        limit: Option<Duration>,
    ) -> Result<HttpReply, String> {
        let mut req = agent().post(url).any_status();
        if let Some(limit) = limit {
            req = req.timeout(limit);
        }
        for (k, v) in headers {
            req = req.header(k, v);
        }
        reply(req.send_string(body))
    }

    fn post_streamed(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
        limit: Option<Duration>,
    ) -> Result<HttpStream, String> {
        let mut req = agent().post(url).any_status();
        if let Some(limit) = limit {
            req = req.timeout(limit);
        }
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let resp = noted(req.send_string(body))?;
        Ok(HttpStream {
            status: resp.status(),
            headers: resp.headers(),
            reader: resp.into_reader(),
        })
    }
}

/// Test builds never reach the network: with no fake set, every request fails here.
struct NoNetwork;

impl ProviderTransport for NoNetwork {
    fn get(&self, _: &str, _: &[(String, String)]) -> Result<HttpReply, String> {
        Err("no network in tests".into())
    }

    fn post(&self, _: &str, _: &[(String, String)], _: &str) -> Result<HttpReply, String> {
        Err("no network in tests".into())
    }
}

static TRANSPORTS: Mutex<Vec<(PathBuf, Arc<dyn ProviderTransport>)>> = Mutex::new(Vec::new());

/// Use `t` for this config dir only (tests).
pub fn use_transport_for(config_dir: &Path, t: Arc<dyn ProviderTransport>) {
    let mut all = TRANSPORTS.lock().unwrap_or_else(|e| e.into_inner());
    all.retain(|(d, _)| d != config_dir);
    all.push((config_dir.to_path_buf(), t));
}

fn transport_for(config_dir: &Path) -> Arc<dyn ProviderTransport> {
    let over = TRANSPORTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(d, _)| d == config_dir)
        .map(|(_, t)| t.clone());
    match over {
        Some(t) => t,
        None if cfg!(test) => Arc::new(NoNetwork),
        None => Arc::new(UreqTransport),
    }
}

// ---- The model list ----------------------------------------------------------

/// A provider's `/models`, read only while its key and a grant for its
/// destination exist. The request carries no user data but still goes through
/// [`guard_provider`] (one egress line), so nothing reaches a provider you
/// haven't allowed. Rows are `<provider>/<model>`, cost class `new_provider`.
pub struct ProviderSource {
    pub config_dir: PathBuf,
    pub provider: Provider,
}

/// The OpenRouter account behind this provider's key, from
/// `GET /api/v1/key`: the account's own answer about free vs paid and
/// what is left (parsed in `grokhub_core::model_registry::quota`). Read
/// only under the same grant as the model list; carries no user data.
pub fn fetch_openrouter_key(config_dir: &Path, p: &Provider) -> Result<Value, String> {
    let key = key(config_dir, &p.id).ok_or_else(|| format!("{}: no key", p.id))?;
    let url = format!("{}/key", p.api_root());
    if let GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } =
        guard_provider(config_dir, &EgressReq::new(&url, &[]))
    {
        return Err(format!("{}: {reason}", p.id));
    }
    let r = transport_for(config_dir).get(&url, &p.headers(&key))?;
    if !(200..300).contains(&r.status) {
        return Err(format!("{}: HTTP {}", p.id, r.status));
    }
    serde_json::from_str(&r.body).map_err(|e| e.to_string())
}

/// Anthropic's `usage.cache_creation_input_tokens`, which [`Usage`] has
/// no field for; the quota spend ledger prices the write.
fn anthropic_cache_write(body: &str) -> u64 {
    serde_json::from_str::<Value>(body)
        .map(|v| u64_at(&v["usage"], &["cache_creation_input_tokens"]))
        .unwrap_or(0)
}

/// One source per provider you added.
pub fn sources(config_dir: &Path) -> Vec<ProviderSource> {
    load_providers(config_dir)
        .into_iter()
        .map(|provider| ProviderSource {
            config_dir: config_dir.to_path_buf(),
            provider,
        })
        .collect()
}

/// OpenRouter's model list is read at most once a day: the registry refreshes
/// every few hours and on every sign-in or listing change, and the other
/// refreshes reuse the day's list, which sends nothing.
pub const LISTING_MAX_AGE_MS: u64 = DAY_MS;

/// OpenRouter's own effort levels for its unified `reasoning.effort`: every
/// model whose listing names `reasoning` takes them, and OpenRouter maps each
/// onto that model's own setting.
pub const OPENROUTER_EFFORTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh"];

/// `{config}/models/listing-<provider>.json`: the day's model list (no key, no user data).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ListingCache {
    fetched_ms: u64,
    base_url: String,
    models: Vec<ModelMeta>,
}

fn listing_path(config_dir: &Path, id: &str) -> PathBuf {
    models_dir(config_dir).join(format!("listing-{id}.json"))
}

fn forget_listing(config_dir: &Path, id: &str) {
    let _ = std::fs::remove_file(listing_path(config_dir, id));
}

/// The day's list for `p`, while it is under [`LISTING_MAX_AGE_MS`] old and
/// your grant for the destination still stands.
fn day_listing(config_dir: &Path, p: &Provider, now_ms: u64) -> Option<Vec<ModelMeta>> {
    let c = read_json::<ListingCache>(&listing_path(config_dir, &p.id))?;
    let ledger = ConsentLedger::load(config_dir);
    (c.base_url == p.base_url
        && c.fetched_ms <= now_ms
        && now_ms - c.fetched_ms < LISTING_MAX_AGE_MS
        && ledger.locked().is_none()
        && ledger.destination_grant(&p.dest(), &[]).is_some())
    .then_some(c.models)
}

/// An OpenRouter model whose listing names `reasoning` and no effort list of
/// its own takes OpenRouter's unified levels.
pub fn openrouter_efforts(rows: &mut [ModelMeta]) {
    for m in rows.iter_mut() {
        if m.reasoning == Some(true) && m.efforts.is_none() {
            m.efforts = Some(OPENROUTER_EFFORTS.iter().map(|e| e.to_string()).collect());
        }
    }
}

impl ProviderSource {
    /// [`CatalogSource::fetch`] as of `now_ms`: OpenRouter's list comes from
    /// the day's copy when there is one ([`LISTING_MAX_AGE_MS`]).
    pub fn fetch_at(&self, now_ms: u64) -> Result<Listing, String> {
        let p = &self.provider;
        let key = key(&self.config_dir, &p.id).ok_or_else(|| format!("{}: no key", p.id))?;
        let daily = p.is_openrouter();
        if let Some(rows) = daily
            .then(|| day_listing(&self.config_dir, p, now_ms))
            .flatten()
        {
            return Ok(Listing::new(self.kind(), rows));
        }
        let url = p.models_url();
        if let GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } =
            guard_provider(&self.config_dir, &EgressReq::new(&url, &[]))
        {
            return Err(format!("{}: {reason}", p.id));
        }
        let r = transport_for(&self.config_dir).get(&url, &p.headers(&key))?;
        if !(200..300).contains(&r.status) {
            return Err(format!("{}: HTTP {}", p.id, r.status));
        }
        let mut rows = match p.kind {
            ProviderKind::OpenAiCompatible => parse_openai_compatible_catalog(&p.id, &r.body)?,
            ProviderKind::Anthropic => parse_anthropic_catalog(&p.id, &r.body)?,
        };
        if daily {
            openrouter_efforts(&mut rows);
            let _ = write_json(
                &listing_path(&self.config_dir, &p.id),
                &ListingCache {
                    fetched_ms: now_ms,
                    base_url: p.base_url.clone(),
                    models: rows.clone(),
                },
            );
        }
        Ok(Listing::new(self.kind(), rows))
    }
}

impl CatalogSource for ProviderSource {
    fn kind(&self) -> SourceKind {
        self.provider.kind.source_kind()
    }

    fn fetch(&self) -> Result<Listing, String> {
        self.fetch_at(grokhub_core::now_ms())
    }
}

// ---- Which provider models Auto picks ----------------------------------------

/// What a step needs from a provider model Auto picks itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickNeed {
    /// An agent lane: the model must take `tools`.
    pub tools: bool,
    /// A reasoning lane: the model must take `reasoning` and an effort.
    pub reasoning: bool,
    pub ctx_tokens: u64,
    /// Your $/M ceiling on the listed output price at this prompt's size.
    pub ceiling_usd_per_m: f64,
}

/// Whether a provider model takes a reasoning effort: what its listing's
/// `supported_parameters` say when it lists them, else any model not named
/// non-reasoning; and an effort list it names must hold a real effort.
pub fn takes_reasoning(id: &str, meta: &ModelMeta) -> bool {
    meta.reasoning
        .unwrap_or_else(|| !id.to_ascii_lowercase().contains("non-reasoning"))
        && meta
            .efforts
            .as_ref()
            .is_none_or(|e| e.iter().any(|x| x != "none"))
}

/// A provider model Auto may pick for a step needing `need`: one model, never
/// OpenRouter's own router (`openrouter/auto` and the like picks any model at
/// any price), not dated for retirement, no utility model, answering in text,
/// its listed output price at this prompt's size (a long-context step
/// included) under your ceiling, taking tools and reasoning
/// when the lane needs them, and a window that holds the prompt.
pub fn auto_pickable(id: &str, meta: &ModelMeta, need: &PickNeed) -> bool {
    let wire = split_provider_model(id).map_or(id, |(_, m)| m);
    !is_openrouter_router(wire)
        && meta.deprecated != Some(true)
        && !super::policy::utility_model(id)
        && meta
            .output_modalities
            .as_ref()
            .is_none_or(|o| o.iter().any(|x| x == "text"))
        && meta
            .prices
            .completion_at(need.ctx_tokens)
            .is_some_and(|p| usd_per_m(p) <= need.ceiling_usd_per_m)
        && (!need.tools || meta.tool_calling != Some(false))
        && (!need.reasoning || takes_reasoning(id, meta))
        && meta.context_length.is_none_or(|c| need.ctx_tokens <= c)
}

/// The backup for OpenRouter's `models` list: the next best model on the
/// primary's provider from the same filter ([`auto_pickable`] and the step's
/// fit), ranked as the router ranks. `None` when there is none.
pub fn openrouter_backup(
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    table: &super::table::RoutingTable,
    class: &str,
    primary: &str,
    need: &PickNeed,
    fit: super::policy::Fit,
    now_ms: u64,
) -> Option<String> {
    let (pid, _) = split_provider_model(primary)?;
    let pool: Vec<&str> = reg
        .models
        .iter()
        .filter(|(id, r)| {
            id.as_str() != primary
                && split_provider_model(id).is_some_and(|(p, _)| p == pid)
                && super::policy::is_new_provider(reg, id)
                && super::policy::fits_any_cost(reg, profiles, id, fit, now_ms)
                && auto_pickable(id, &r.meta, need)
        })
        .map(|(id, _)| id.as_str())
        .collect();
    super::policy::rank(class, &pool, table, reg, primary, need.ctx_tokens)
        .into_iter()
        .next()
}

// ---- Request bodies ----------------------------------------------------------

fn texts(content: &[ContentPart]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            ContentPart::InputText(t) => Some(t.as_str()),
            ContentPart::InputImage(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_image(content: &[ContentPart]) -> bool {
    content
        .iter()
        .any(|c| matches!(c, ContentPart::InputImage(_)))
}

/// A Responses function tool (`{"type":"function","name",…}`) as its name,
/// description and schema. Hosted tools (web search) don't map and are dropped.
fn function_tool(t: &Value) -> Option<(String, String, Value)> {
    if t["type"].as_str() != Some("function") {
        return None;
    }
    let f = if t.get("function").is_some() {
        &t["function"]
    } else {
        t
    };
    let name = f["name"].as_str()?.to_string();
    let desc = f["description"].as_str().unwrap_or("").to_string();
    let params = f
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    Some((name, desc, params))
}

/// OpenAI-compatible `/chat/completions`. Effort is `reasoning.effort`.
pub fn openai_body(model: &str, effort: Option<&str>, req: &ResponsesRequest) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    // This model's own reasoning details, waiting for the assistant turn they
    // belong to: OpenRouter wants them back on it so a tool step keeps its working.
    let mut details: Option<Value> = None;
    for item in &req.input {
        let before = messages.len();
        match item {
            InputItem::Message { role, content } => {
                let role = if matches!(role.as_str(), "system" | "developer") {
                    "system"
                } else {
                    role.as_str()
                };
                let content = if has_image(content) {
                    Value::Array(
                        content
                            .iter()
                            .map(|c| match c {
                                ContentPart::InputText(t) => json!({"type": "text", "text": t}),
                                ContentPart::InputImage(u) => {
                                    json!({"type": "image_url", "image_url": {"url": u}})
                                }
                            })
                            .collect(),
                    )
                } else {
                    Value::String(texts(content))
                };
                messages.push(json!({"role": role, "content": content}));
            }
            InputItem::FunctionCall {
                call_id,
                name,
                arguments,
            } => {
                let call = json!({"id": call_id, "type": "function", "function": {"name": name, "arguments": arguments}});
                match messages.last_mut() {
                    Some(m) if m["role"] == "assistant" && m.get("tool_calls").is_some() => {
                        if let Some(calls) = m["tool_calls"].as_array_mut() {
                            calls.push(call);
                        }
                    }
                    _ => messages.push(
                        json!({"role": "assistant", "content": Value::Null, "tool_calls": [call]}),
                    ),
                }
            }
            InputItem::FunctionCallOutput { call_id, output } => {
                messages.push(json!({"role": "tool", "tool_call_id": call_id, "content": output}));
            }
            // xAI's sealed compaction: its plain-text digest stands in, when it has one.
            InputItem::ServerCompaction { digest, .. } => {
                if let Some(text) = crate::compact::fallback_digest_message(digest) {
                    messages.push(json!({"role": "user", "content": text}));
                }
            }
            // xAI's sealed reasoning only xAI can read; a provider's goes back
            // to the model that wrote it.
            InputItem::Reasoning { .. } => {
                if let Some(d) = item.reasoning_details(model) {
                    details = Some(d);
                }
            }
        }
        if let Some(m) = messages[before..]
            .iter_mut()
            .find(|m| m["role"] == "assistant")
        {
            if let Some(d) = details.take() {
                m["reasoning_details"] = d;
            }
        }
    }
    let mut body = json!({"model": model, "stream": false, "messages": messages});
    let tools: Vec<Value> = req
        .tools
        .iter()
        .filter_map(function_tool)
        .map(|(name, description, parameters)| json!({"type": "function", "function": {"name": name, "description": description, "parameters": parameters}}))
        .collect();
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if let Some(e) = effort
        .map(str::trim)
        .filter(|e| !e.is_empty() && *e != "none")
    {
        body["reasoning"] = json!({"effort": e});
    }
    body
}

fn anthropic_image(url: &str) -> Value {
    // `data:image/png;base64,…` goes inline; anything else by URL.
    if let Some((head, data)) = url
        .strip_prefix("data:")
        .and_then(|r| r.split_once(";base64,"))
    {
        return json!({"type": "image", "source": {"type": "base64", "media_type": head, "data": data}});
    }
    json!({"type": "image", "source": {"type": "url", "url": url}})
}

/// Push `block` onto the last message when it has `role`, else start one.
fn push_block(messages: &mut Vec<Value>, role: &str, block: Value) {
    match messages.last_mut() {
        Some(m) if m["role"] == role => {
            if let Some(blocks) = m["content"].as_array_mut() {
                blocks.push(block);
            }
        }
        _ => messages.push(json!({"role": role, "content": [block]})),
    }
}

/// Anthropic prompt caching: this mark tells the API to cache the
/// request prefix up to the block that carries it, so a later call that
/// sends the same prefix pays the cached rate for it (the registry's
/// price table carries both rates, and the spend ledger prices reads
/// and writes apart). Marks go on the system text, the last tool and
/// the last message block: between them they cover everything a turn
/// re-sends — its instructions, its tools and the conversation so far —
/// and the message mark moves forward each turn, so each turn reads
/// back what the turns before it wrote.
fn cache_mark(block: &mut Value) {
    block["cache_control"] = json!({"type": "ephemeral"});
}

/// Anthropic `/v1/messages`. Effort is `output_config.effort`.
pub fn anthropic_body(
    model: &str,
    effort: Option<&str>,
    req: &ResponsesRequest,
    max_output: Option<u64>,
) -> Value {
    let mut system: Vec<String> = Vec::new();
    let mut messages: Vec<Value> = Vec::new();
    for item in &req.input {
        match item {
            InputItem::Message { role, content }
                if matches!(role.as_str(), "system" | "developer") =>
            {
                system.push(texts(content))
            }
            InputItem::Message { role, content } => {
                let role = if role == "assistant" {
                    "assistant"
                } else {
                    "user"
                };
                for c in content {
                    let block = match c {
                        ContentPart::InputText(t) => json!({"type": "text", "text": t}),
                        ContentPart::InputImage(u) => anthropic_image(u),
                    };
                    push_block(&mut messages, role, block);
                }
            }
            InputItem::FunctionCall {
                call_id,
                name,
                arguments,
            } => {
                let input: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
                push_block(
                    &mut messages,
                    "assistant",
                    json!({"type": "tool_use", "id": call_id, "name": name, "input": input}),
                );
            }
            InputItem::FunctionCallOutput { call_id, output } => {
                push_block(
                    &mut messages,
                    "user",
                    json!({"type": "tool_result", "tool_use_id": call_id, "content": output}),
                );
            }
            // xAI's sealed compaction: its plain-text digest stands in, when it has one.
            InputItem::ServerCompaction { digest, .. } => {
                if let Some(text) = crate::compact::fallback_digest_message(digest) {
                    push_block(&mut messages, "user", json!({"type": "text", "text": text}));
                }
            }
            // Sealed reasoning: only xAI can read it.
            InputItem::Reasoning { .. } => {}
        }
    }
    let max_tokens = max_output
        .filter(|m| *m > 0)
        .map(|m| m.min(ANTHROPIC_MAX_TOKENS))
        .unwrap_or(ANTHROPIC_MAX_TOKENS);
    let mut body = json!({"model": model, "max_tokens": max_tokens, "messages": messages});
    if !system.is_empty() {
        // Blocks, not a plain string, so the system text carries a
        // cache mark (see [`cache_mark`]).
        let mut block = json!({"type": "text", "text": system.join("\n\n")});
        cache_mark(&mut block);
        body["system"] = Value::Array(vec![block]);
    }
    let mut tools: Vec<Value> = req
        .tools
        .iter()
        .filter_map(function_tool)
        .map(|(name, description, schema)| json!({"name": name, "description": description, "input_schema": schema}))
        .collect();
    if let Some(last) = tools.last_mut() {
        cache_mark(last);
        body["tools"] = Value::Array(tools);
    }
    if let Some(block) = body["messages"]
        .as_array_mut()
        .and_then(|ms| ms.last_mut())
        .and_then(|m| m["content"].as_array_mut())
        .and_then(|blocks| blocks.last_mut())
    {
        cache_mark(block);
    }
    if let Some(e) = effort
        .map(str::trim)
        .filter(|e| !e.is_empty() && *e != "none")
    {
        body["output_config"] = json!({"effort": e});
    }
    body
}

/// The request body a provider gets for this call (`model` is the provider's own id).
pub fn request_body(
    kind: ProviderKind,
    model: &str,
    effort: Option<&str>,
    req: &ResponsesRequest,
    max_output: Option<u64>,
) -> Value {
    match kind {
        ProviderKind::OpenAiCompatible => openai_body(model, effort, req),
        ProviderKind::Anthropic => anthropic_body(model, effort, req, max_output),
    }
}

// ---- Replies -----------------------------------------------------------------

fn u64_at(v: &Value, path: &[&str]) -> u64 {
    path.iter().fold(v, |v, k| &v[*k]).as_u64().unwrap_or(0)
}

/// A `/chat/completions` answer as a turn. OpenRouter's `usage.cost` (USD)
/// becomes ticks. The reply's `model` is not read: which model or endpoint
/// answered never reaches the chat.
pub fn parse_openai_reply(body: &str) -> Result<TurnOutput, ClientError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| ClientError::Protocol(format!("provider reply: {e}")))?;
    let msg = &v["choices"][0]["message"];
    if msg.is_null() {
        return Err(ClientError::Protocol(
            "provider reply has no message".into(),
        ));
    }
    let calls = msg["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| FunctionCall {
            call_id: c["id"].as_str().unwrap_or("").to_string(),
            name: c["function"]["name"].as_str().unwrap_or("").to_string(),
            arguments: c["function"]["arguments"]
                .as_str()
                .unwrap_or("{}")
                .to_string(),
        })
        .collect();
    let u = &v["usage"];
    // OpenRouter says `reasoning`; llama.cpp (`--reasoning-format deepseek`) and
    // most other hosts say `reasoning_content`.
    let reasoning = ["reasoning", "reasoning_content"]
        .iter()
        .find_map(|k| msg[*k].as_str().filter(|r| !r.is_empty()))
        .unwrap_or("");
    // Tagged with the model that wrote them once the caller knows it ([`call_model`]).
    let reasoning_items = msg["reasoning_details"]
        .as_array()
        .filter(|d| !d.is_empty())
        .and_then(|d| InputItem::provider_reasoning(&Value::Array(d.clone()), ""))
        .into_iter()
        .collect();
    // Cut off at the output limit: the run loop holds its tool calls, and with
    // no answer text the router reads it as a spent budget (E14).
    let truncated = v["choices"][0]["finish_reason"].as_str() == Some("length");
    Ok(TurnOutput {
        truncated,
        truncate_reason: truncated.then(|| "max_output_tokens".to_string()),
        text: msg["content"].as_str().unwrap_or("").to_string(),
        reasoning: reasoning.to_string(),
        reasoning_items,
        calls,
        usage: Usage {
            input_tokens: u64_at(u, &["prompt_tokens"]),
            output_tokens: u64_at(u, &["completion_tokens"]),
            reasoning_tokens: u64_at(u, &["completion_tokens_details", "reasoning_tokens"]),
            cached_tokens: u64_at(u, &["prompt_tokens_details", "cached_tokens"]),
            cost_in_usd_ticks: u["cost"]
                .as_f64()
                .map(|c| (c * 1e10).round() as i64)
                .unwrap_or(0),
        },
        ..TurnOutput::default()
    })
}

/// An Anthropic `stop_reason` folded into the turn, shared by the
/// whole-reply parse and the streamed one so both read a stop the same
/// way. `max_tokens` is a truncation the run loop can carry on from;
/// `refusal` is the model declining — its own error class, never a
/// silent empty answer; `pause_turn` (a server tool paused, the reply
/// is complete as it stands), `end_turn`, `tool_use` and
/// `stop_sequence` are ordinary endings. `category` is the refusal's
/// `stop_details.category` when the reply carries one.
fn note_anthropic_stop(
    out: &mut TurnOutput,
    stop_reason: Option<&str>,
    category: Option<&str>,
) -> Result<(), ClientError> {
    match stop_reason {
        Some("max_tokens") => {
            out.truncated = true;
            out.truncate_reason = Some("max_output_tokens".into());
        }
        Some("refusal") => {
            return Err(ClientError::Refused {
                message: match category {
                    Some(c) if !c.is_empty() => {
                        format!("it gave its safety rules as the reason ({c})")
                    }
                    _ => "it gave its safety rules as the reason".into(),
                },
            });
        }
        _ => {}
    }
    Ok(())
}

/// A `/v1/messages` answer as a turn.
pub fn parse_anthropic_reply(body: &str) -> Result<TurnOutput, ClientError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| ClientError::Protocol(format!("provider reply: {e}")))?;
    let Some(blocks) = v["content"].as_array() else {
        return Err(ClientError::Protocol(
            "provider reply has no content".into(),
        ));
    };
    let mut out = TurnOutput {
        text: String::new(),
        reasoning: String::new(),
        calls: Vec::new(),
        usage: Usage::default(),
        ..Default::default()
    };
    for b in blocks {
        match b["type"].as_str() {
            Some("text") => out.text.push_str(b["text"].as_str().unwrap_or("")),
            Some("thinking") => out.reasoning.push_str(b["thinking"].as_str().unwrap_or("")),
            Some("tool_use") => out.calls.push(FunctionCall {
                call_id: b["id"].as_str().unwrap_or("").to_string(),
                name: b["name"].as_str().unwrap_or("").to_string(),
                arguments: b["input"].to_string(),
            }),
            _ => {}
        }
    }
    note_anthropic_stop(
        &mut out,
        v["stop_reason"].as_str(),
        v["stop_details"]["category"].as_str(),
    )?;
    let u = &v["usage"];
    out.usage = Usage {
        input_tokens: u64_at(u, &["input_tokens"]),
        output_tokens: u64_at(u, &["output_tokens"]),
        cached_tokens: u64_at(u, &["cache_read_input_tokens"]),
        ..Usage::default()
    };
    Ok(out)
}

/// How often a streamed provider call checks for Stop while it waits.
const STREAM_POLL: Duration = Duration::from_millis(250);
/// How long a streamed reply may go quiet before it counts as stalled:
/// the longest silence the native client's own idle clock allows (its
/// xhigh thinking allowance), so a Claude turn thinking in silence is
/// not cut off sooner than a Grok one.
const STREAM_IDLE: Duration = Duration::from_secs(300);

/// One tool call still arriving: its id and name came with the block
/// start, its arguments with the deltas that follow.
#[derive(Default)]
struct StreamTool {
    id: String,
    name: String,
    json: String,
}

/// An Anthropic event stream folded into a turn as it arrives, the way
/// the native client's parser folds xAI's: text and thinking reach the
/// chat delta by delta, tool calls assemble from their pieces, and the
/// usage (cache reads and writes included) and stop reason land on the
/// finished turn.
#[derive(Default)]
struct AnthropicStream {
    out: TurnOutput,
    /// `usage.cache_creation_input_tokens`, which [`Usage`] has no
    /// field for; the spend ledger prices the write.
    cache_write: u64,
    tools: BTreeMap<usize, StreamTool>,
    stop_reason: Option<String>,
    refusal_category: Option<String>,
    /// `message_stop` seen: the stream ended the way it should.
    finished: bool,
    /// Any answer text already reached the chat.
    shown: bool,
}

impl AnthropicStream {
    fn usage(&mut self, u: &Value) {
        if let Some(n) = u["input_tokens"].as_u64() {
            self.out.usage.input_tokens = n;
        }
        if let Some(n) = u["output_tokens"].as_u64() {
            self.out.usage.output_tokens = n;
        }
        if let Some(n) = u["cache_read_input_tokens"].as_u64() {
            self.out.usage.cached_tokens = n;
        }
        if let Some(n) = u["cache_creation_input_tokens"].as_u64() {
            self.cache_write = n;
        }
    }

    fn event(&mut self, v: &Value, sink: &mut dyn FnMut(StreamEvent)) -> Result<(), ClientError> {
        match v["type"].as_str().unwrap_or("") {
            "message_start" => self.usage(&v["message"]["usage"]),
            "content_block_start" => {
                let b = &v["content_block"];
                if b["type"].as_str() == Some("tool_use") {
                    let at = v["index"].as_u64().unwrap_or(0) as usize;
                    self.tools.insert(
                        at,
                        StreamTool {
                            id: b["id"].as_str().unwrap_or("").to_string(),
                            name: b["name"].as_str().unwrap_or("").to_string(),
                            json: String::new(),
                        },
                    );
                }
            }
            "content_block_delta" => {
                let d = &v["delta"];
                match d["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let t = d["text"].as_str().unwrap_or("");
                        if !t.is_empty() {
                            self.shown = true;
                            self.out.text.push_str(t);
                            sink(StreamEvent::TextDelta(t.to_string()));
                        }
                    }
                    "thinking_delta" => {
                        let t = d["thinking"].as_str().unwrap_or("");
                        if !t.is_empty() {
                            self.out.reasoning.push_str(t);
                            sink(StreamEvent::ReasoningDelta(t.to_string()));
                        }
                    }
                    "input_json_delta" => {
                        let at = v["index"].as_u64().unwrap_or(0) as usize;
                        if let Some(tool) = self.tools.get_mut(&at) {
                            tool.json.push_str(d["partial_json"].as_str().unwrap_or(""));
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let at = v["index"].as_u64().unwrap_or(0) as usize;
                if let Some(tool) = self.tools.remove(&at) {
                    self.out.calls.push(FunctionCall {
                        call_id: tool.id,
                        name: tool.name,
                        arguments: if tool.json.is_empty() {
                            "{}".into()
                        } else {
                            tool.json
                        },
                    });
                }
            }
            "message_delta" => {
                if let Some(s) = v["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(s.to_string());
                }
                if let Some(c) = v["delta"]["stop_details"]["category"].as_str() {
                    self.refusal_category = Some(c.to_string());
                }
                self.usage(&v["usage"]);
            }
            "message_stop" => self.finished = true,
            "error" => return Err(anthropic_stream_error(&v["error"])),
            _ => {}
        }
        Ok(())
    }

    /// The finished turn and its cache-write count. A stream that ended
    /// without `message_stop` is a disconnect, like the native parser's.
    fn finish(mut self) -> Result<(TurnOutput, u64), ClientError> {
        if !self.finished {
            return Err(ClientError::Disconnect);
        }
        note_anthropic_stop(
            &mut self.out,
            self.stop_reason.as_deref(),
            self.refusal_category.as_deref(),
        )?;
        Ok((self.out, self.cache_write))
    }
}

/// An `error` event on the stream, as the client's own error classes:
/// the HTTP layer never saw these, the stream itself carried them.
fn anthropic_stream_error(e: &Value) -> ClientError {
    let message =
        grokhub_core::redact_secrets(e["message"].as_str().unwrap_or("the stream failed"));
    match e["type"].as_str().unwrap_or("") {
        "overloaded_error" => ClientError::Server {
            status: 529,
            message,
            retry_after_secs: None,
        },
        "rate_limit_error" => ClientError::RateLimited {
            retry_after_secs: None,
            message,
        },
        "authentication_error" => ClientError::Auth {
            status: 401,
            message,
        },
        "invalid_request_error" => ClientError::Protocol(message),
        _ => ClientError::Server {
            status: 500,
            message,
            retry_after_secs: None,
        },
    }
}

/// An OpenRouter `/chat/completions` event stream folded into a turn as
/// it arrives: `delta.content` and the reasoning text reach the chat
/// delta by delta, tool calls assemble from their indexed pieces (id and
/// name first, the arguments in fragments), `reasoning_details` merge by
/// their index, and the usage chunk at the end (cost included) lands on
/// the finished turn with the same fields [`parse_openai_reply`] reads.
#[derive(Default)]
struct OpenAiStream {
    out: TurnOutput,
    tools: BTreeMap<usize, StreamTool>,
    details: BTreeMap<usize, serde_json::Map<String, Value>>,
    finish_reason: Option<String>,
    /// The model the chunks say answered (the pick or its backup).
    model: Option<String>,
    /// The HTTP status an error chunk carried, for the quota ledger.
    error_status: Option<(u16, String)>,
    shown: bool,
    /// Frames read so far.
    frames: usize,
    /// The body as it came, kept until the first frame: a host that
    /// answered with the whole reply instead of a stream sent no frames.
    raw: Vec<u8>,
}

impl OpenAiStream {
    fn usage(&mut self, u: &Value) {
        self.out.usage = Usage {
            input_tokens: u64_at(u, &["prompt_tokens"]),
            output_tokens: u64_at(u, &["completion_tokens"]),
            reasoning_tokens: u64_at(u, &["completion_tokens_details", "reasoning_tokens"]),
            cached_tokens: u64_at(u, &["prompt_tokens_details", "cached_tokens"]),
            cost_in_usd_ticks: u["cost"]
                .as_f64()
                .map(|c| (c * 1e10).round() as i64)
                .unwrap_or(0),
        };
    }

    /// One `reasoning_details` piece merged into the entry at its index:
    /// the text-like fields grow, anything else (type, format, id, the
    /// signature that comes last) takes the newest value it is given.
    fn detail(&mut self, at: usize, piece: &Value) {
        let Some(piece) = piece.as_object() else {
            return;
        };
        let entry = self.details.entry(at).or_default();
        for (k, v) in piece {
            match (k.as_str(), v, entry.get_mut(k)) {
                ("text" | "summary" | "data", Value::String(more), Some(Value::String(have))) => {
                    have.push_str(more)
                }
                (_, Value::Null, Some(_)) => {}
                _ => {
                    entry.insert(k.clone(), v.clone());
                }
            }
        }
    }

    /// An error chunk: OpenRouter's mid-stream `error` object, whose
    /// `code` is an HTTP status when it is a number.
    fn error(&mut self, e: &Value) -> ClientError {
        let status = e["code"]
            .as_u64()
            .and_then(|c| u16::try_from(c).ok())
            .filter(|c| (400..600).contains(c))
            .unwrap_or(502);
        let body = json!({ "error": e }).to_string();
        self.error_status = Some((status, body.clone()));
        error_of(status, &body, true)
    }

    fn event(&mut self, v: &Value, sink: &mut dyn FnMut(StreamEvent)) -> Result<(), ClientError> {
        self.frames += 1;
        self.raw = Vec::new();
        if v["error"].is_object() {
            return Err(self.error(&v["error"]));
        }
        if let Some(m) = v["model"].as_str().filter(|m| !m.is_empty()) {
            self.model = Some(m.to_string());
        }
        if v["usage"].is_object() {
            self.usage(&v["usage"]);
        }
        let choice = &v["choices"][0];
        let d = &choice["delta"];
        if let Some(t) = d["content"].as_str().filter(|t| !t.is_empty()) {
            self.shown = true;
            self.out.text.push_str(t);
            sink(StreamEvent::TextDelta(t.to_string()));
        }
        // OpenRouter says `reasoning`; most other hosts `reasoning_content`.
        if let Some(t) = ["reasoning", "reasoning_content"]
            .iter()
            .find_map(|k| d[*k].as_str().filter(|t| !t.is_empty()))
        {
            self.out.reasoning.push_str(t);
            sink(StreamEvent::ReasoningDelta(t.to_string()));
        }
        for (n, piece) in d["reasoning_details"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let at = piece["index"].as_u64().map_or(n, |i| i as usize);
            self.detail(at, piece);
        }
        for (n, c) in d["tool_calls"].as_array().into_iter().flatten().enumerate() {
            let at = c["index"].as_u64().map_or(n, |i| i as usize);
            let tool = self.tools.entry(at).or_default();
            if let Some(id) = c["id"].as_str().filter(|id| !id.is_empty()) {
                tool.id = id.to_string();
            }
            if let Some(name) = c["function"]["name"].as_str().filter(|n| !n.is_empty()) {
                if tool.name.is_empty() {
                    tool.name = name.to_string();
                }
            }
            if let Some(args) = c["function"]["arguments"].as_str() {
                tool.json.push_str(args);
            }
        }
        if let Some(f) = choice["finish_reason"].as_str() {
            if f == "error" {
                return Err(
                    self.error(&json!({"message": "the provider ended the stream with an error"}))
                );
            }
            self.finish_reason = Some(f.to_string());
        }
        Ok(())
    }

    /// The finished turn and the model the chunks named. A stream that
    /// ended without a `finish_reason` is a disconnect, like the native
    /// parser's; one cut at the output limit is a truncation the run
    /// loop can carry on from.
    fn finish(mut self) -> Result<(TurnOutput, Option<String>), ClientError> {
        let Some(reason) = self.finish_reason.take() else {
            return Err(ClientError::Disconnect);
        };
        if reason == "length" {
            self.out.truncated = true;
            self.out.truncate_reason = Some("max_output_tokens".into());
        }
        self.out.calls = std::mem::take(&mut self.tools)
            .into_values()
            .map(|t| FunctionCall {
                call_id: t.id,
                name: t.name,
                arguments: if t.json.is_empty() {
                    "{}".into()
                } else {
                    t.json
                },
            })
            .collect();
        let details: Vec<Value> = std::mem::take(&mut self.details)
            .into_values()
            .map(Value::Object)
            .collect();
        if !details.is_empty() {
            // Tagged with the model that wrote them by the caller.
            self.out.reasoning_items = InputItem::provider_reasoning(&Value::Array(details), "")
                .into_iter()
                .collect();
        }
        Ok((self.out, self.model))
    }
}

impl StreamFold for OpenAiStream {
    fn event(&mut self, v: &Value, sink: &mut dyn FnMut(StreamEvent)) -> Result<(), ClientError> {
        OpenAiStream::event(self, v, sink)
    }
    fn shown(&self) -> bool {
        self.shown
    }
    fn bytes(&mut self, chunk: &[u8]) {
        if self.frames == 0 && (self.raw.len() as u64) < BODY_CAP {
            self.raw.extend_from_slice(chunk);
        }
    }
}

/// Reads a streamed body on its own thread, so Stop and the stall clock
/// are checked every [`STREAM_POLL`] even while a read is blocked (the
/// same pattern as the native client's stream reads).
fn stream_pump(
    mut reader: impl Read + Send + 'static,
) -> std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>> {
    let (tx, rx) = std::sync::mpsc::sync_channel(16);
    let spawned = std::thread::Builder::new()
        .name("provider-stream-read".into())
        .spawn({
            let tx = tx.clone();
            move || {
                let mut buf = [0u8; 4096];
                loop {
                    let sent = match reader.read(&mut buf) {
                        Ok(0) => {
                            let _ = tx.send(Ok(Vec::new()));
                            break;
                        }
                        Ok(n) => tx.send(Ok(buf[..n].to_vec())),
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            break;
                        }
                    };
                    // The call was dropped (Stop, a stall, an error): stop reading.
                    if sent.is_err() {
                        break;
                    }
                }
            }
        });
    if let Err(e) = spawned {
        let _ = tx.send(Err(e));
    }
    rx
}

/// The frames complete in `buf` so far, drained: events split on blank
/// lines, and only the `data:` lines carry anything (the JSON names its
/// own type). A frame is whole bytes, so a character a read cut in half
/// is never decoded in pieces.
fn stream_frames(buf: &mut Vec<u8>) -> Vec<Value> {
    let mut out = Vec::new();
    while let Some(at) = buf
        .windows(2)
        .position(|w| w == b"\n\n")
        .map(|p| p + 2)
        .or_else(|| buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4))
    {
        let frame: Vec<u8> = buf.drain(..at).collect();
        let text = String::from_utf8_lossy(&frame);
        let mut data = String::new();
        for line in text.split(['\n', '\r']) {
            if let Some(rest) = line.trim_end().strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim_start());
            }
        }
        let data = data.trim();
        if !data.is_empty() && data != "[DONE]" {
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                out.push(v);
            }
        }
    }
    out
}

/// One provider's event stream folded into a turn as its frames arrive.
trait StreamFold {
    fn event(&mut self, v: &Value, sink: &mut dyn FnMut(StreamEvent)) -> Result<(), ClientError>;
    /// Any answer text already reached the chat.
    fn shown(&self) -> bool;
    /// The raw bytes as they arrive, before they are split into frames.
    fn bytes(&mut self, _chunk: &[u8]) {}
}

impl StreamFold for AnthropicStream {
    fn event(&mut self, v: &Value, sink: &mut dyn FnMut(StreamEvent)) -> Result<(), ClientError> {
        AnthropicStream::event(self, v, sink)
    }
    fn shown(&self) -> bool {
        self.shown
    }
}

/// A failure once text showed, when it is one a retry would repeat, is
/// the cut-off the turn stops with: the chat already holds part of the
/// answer, so it is never retried and never failed over.
fn cut_off_if_shown(shown: bool, err: ClientError) -> ClientError {
    if shown && err.is_retryable() {
        ClientError::CutOff(err.to_string())
    } else {
        err
    }
}

/// Read one event stream to its end into `st`, feeding `sink` as the
/// answer arrives. Mirrors the native stream's rules: Stop answers at
/// once, a stall is [`ClientError::IdleTimeout`], and a failure after
/// text already showed is [`ClientError::CutOff`]. Whether the stream
/// ended the way it should is the fold's own `finish` to say.
fn read_stream(
    reader: impl Read + Send + 'static,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(StreamEvent),
    st: &mut dyn StreamFold,
) -> Result<(), ClientError> {
    use std::sync::mpsc::RecvTimeoutError;
    let chunks = stream_pump(reader);
    let mut buf: Vec<u8> = Vec::new();
    let mut last = std::time::Instant::now();
    'read: loop {
        if cancel.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        let chunk = match chunks.recv_timeout(STREAM_POLL) {
            Ok(Ok(chunk)) => chunk,
            Ok(Err(e)) => {
                let msg = e.to_string();
                let lower = msg.to_ascii_lowercase();
                let err = if lower.contains("timed out") || lower.contains("timeout") {
                    ClientError::IdleTimeout
                } else {
                    ClientError::Transport(msg)
                };
                return Err(cut_off_if_shown(st.shown(), err));
            }
            Err(RecvTimeoutError::Timeout) => {
                if last.elapsed() >= STREAM_IDLE {
                    return Err(cut_off_if_shown(st.shown(), ClientError::IdleTimeout));
                }
                continue 'read;
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(cut_off_if_shown(
                    st.shown(),
                    ClientError::Transport("the stream reader stopped".into()),
                ));
            }
        };
        if chunk.is_empty() {
            break 'read;
        }
        // Any bytes count as life, comment frames too: OpenRouter's
        // `: OPENROUTER PROCESSING` keep-alives hold the stall clock off
        // while a slow model thinks before its first token.
        last = std::time::Instant::now();
        st.bytes(&chunk);
        buf.extend_from_slice(&chunk);
        for v in stream_frames(&mut buf) {
            if let Err(e) = st.event(&v, sink) {
                return Err(cut_off_if_shown(st.shown(), e));
            }
        }
    }
    // Whatever frame the stream ended on still counts.
    if !buf.is_empty() {
        buf.extend_from_slice(b"\n\n");
        for v in stream_frames(&mut buf) {
            if let Err(e) = st.event(&v, sink) {
                return Err(cut_off_if_shown(st.shown(), e));
            }
        }
    }
    if cancel.is_cancelled() {
        return Err(ClientError::Cancelled);
    }
    Ok(())
}

/// Read one streamed Anthropic reply to its end ([`read_stream`]); a
/// stream that ends early is a disconnect.
fn read_anthropic_stream(
    reader: impl Read + Send + 'static,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(StreamEvent),
) -> Result<(TurnOutput, u64), ClientError> {
    let mut st = AnthropicStream::default();
    read_stream(reader, cancel, sink, &mut st)?;
    let shown = st.shown;
    st.finish().map_err(|e| cut_off_if_shown(shown, e))
}

/// What the chat says when OpenRouter's account settings leave a model with
/// no endpoint. OpenRouter answers HTTP 404 with "guardrail" / "ZDR" / "data
/// policy" in the message; the raw text names its dashboards, not what to do.
pub const POLICY_REFUSED: &str = "OpenRouter has no endpoint for this model under your account's privacy settings (zero data retention or a guardrail excluded it). Change OpenRouter data policy in Settings → AI models, or change your account settings at https://openrouter.ai/settings/privacy and https://openrouter.ai/settings/guardrails.";

/// A provider reply that is a policy refusal from the account's own settings,
/// not a model or network fault: HTTP 404 whose message names a guardrail,
/// zero data retention or the data policy.
pub fn policy_refusal(status: u16, message: &str) -> bool {
    status == 404
        && ["guardrail", "ZDR", "zero data retention", "data policy"]
            .iter()
            .any(|w| message.contains(w))
}

/// OpenRouter's own free router, a model id like any other.
pub const OPENROUTER_FREE_ROUTER: &str =
    grokhub_core::model_registry::discover::OPENROUTER_FREE_ROUTER;

/// A free tier on OpenRouter: a `:free` variant or its free router. Takes the
/// wire model (`google/x:free`) or the registry id (`openrouter/google/x:free`).
/// The one definition lives in the core registry, where the model card's
/// route classification reads it too.
pub fn is_free_model(id: &str) -> bool {
    grokhub_core::model_registry::discover::is_free_model(id)
}

/// One of OpenRouter's routers (`openrouter/free`, `openrouter/auto`, ...)
/// rather than a single model. Wire model or registry id.
pub fn is_openrouter_router(id: &str) -> bool {
    let id = id.trim();
    (id.starts_with("openrouter/") && id.matches('/').count() == 1)
        || id.starts_with("openrouter/openrouter/")
}

/// Settings → AI models → OpenRouter privacy: which of OpenRouter's
/// endpoints may see your data. Sent only to openrouter.ai, in its `provider`
/// request field; a plain OpenAI-compatible host never sees it. The router
/// alone decides the model and its one backup, so no `sort` is sent.
/// An older `app.json` that still holds `sort` or `fallback` loads with them
/// dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Routing {
    /// "" (the default: `provider.data_collection: deny`, no endpoint that
    /// keeps or trains on prompts), "zdr" (that plus `provider.zdr`:
    /// zero-data-retention endpoints only) or "allow"
    /// (`provider.data_collection: allow`).
    pub data: String,
}

impl Routing {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

static ROUTING: Mutex<Option<Routing>> = Mutex::new(None);

/// The cabin sets this on start and whenever Settings changes it.
pub fn set_routing(r: Routing) {
    *ROUTING.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
}

/// The OpenRouter privacy setting as last set.
pub fn routing() -> Routing {
    ROUTING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}

/// Add OpenRouter's `provider` field to an OpenAI-compatible body: only
/// endpoints that take every field the request sends (`require_parameters`),
/// and your data policy, deny by default. No `sort`: the router picked the model.
pub fn apply_openrouter_routing(body: &mut Value, r: &Routing) {
    body["provider"]["require_parameters"] = json!(true);
    match r.data.as_str() {
        "allow" => body["provider"]["data_collection"] = json!("allow"),
        "zdr" => {
            body["provider"]["data_collection"] = json!("deny");
            body["provider"]["zdr"] = json!(true);
        }
        _ => body["provider"]["data_collection"] = json!("deny"),
    }
}

/// OpenRouter's `session_id` for a chat: a hash of its conversation id, so
/// one chat's calls stick to the same endpoint (and its warm cache). Never
/// user text. `None` for a call with no conversation.
pub fn openrouter_session_id(conversation_id: &str) -> Option<String> {
    let id = conversation_id.trim();
    if id.is_empty() {
        return None;
    }
    let digest = Sha256::digest(id.as_bytes());
    Some(digest.iter().take(16).map(|b| format!("{b:02x}")).collect())
}

/// The OpenRouter-only fields after [`apply_openrouter_routing`]: the
/// `models` fallback list (the router's pick, then its backup; left out with
/// no backup) and the chat's `session_id`. `model` stays the pick.
pub fn apply_openrouter_picks(body: &mut Value, backup: Option<&str>, conversation_id: &str) {
    if let Some(b) = backup {
        let primary = body["model"].clone();
        body["models"] = json!([primary, b]);
    }
    if let Some(sid) = openrouter_session_id(conversation_id) {
        body["session_id"] = json!(sid);
    }
}

/// `model` (the model a `/chat/completions` reply says answered) when it
/// is one of `sent` (the pick or its backup), as `sent` spells it. Read
/// only to give the reply's reasoning back to the model that wrote it;
/// never shown, never kept on the turn.
fn named_in<'a>(model: &str, sent: &[&'a str]) -> Option<&'a str> {
    let m = model.trim();
    sent.iter().copied().find(|s| s.eq_ignore_ascii_case(m))
}

/// The reasoning items of a turn from `wire_model` (or, on OpenRouter,
/// its `backup`), tagged with the model the reply says wrote them, so
/// the next request gives them back to that model alone.
fn tag_reasoning(turn: &mut TurnOutput, ready: &ReadyCall, answered: Option<&str>) {
    let sent: Vec<&str> = std::iter::once(ready.wire_model.as_str())
        .chain(ready.backup_wire.as_deref())
        .collect();
    let wrote = answered
        .filter(|_| ready.openrouter)
        .and_then(|m| named_in(m, &sent))
        .unwrap_or(&ready.wire_model)
        .to_string();
    for item in &mut turn.reasoning_items {
        if let InputItem::Reasoning { model, .. } = item {
            *model = wrote.clone();
        }
    }
}

/// `host` is openrouter.ai or one of its subdomains.
pub fn is_openrouter_host(host: &str) -> bool {
    host_is(host, "openrouter.ai")
}

/// `openrouter`: the reply came from openrouter.ai, whose 404s name the
/// account's privacy settings; any other host keeps its own error.
fn error_of(status: u16, body: &str, openrouter: bool) -> ClientError {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let msg = v["error"]["message"]
        .as_str()
        .or_else(|| v["error"].as_str())
        .or_else(|| v["message"].as_str())
        .unwrap_or("");
    if openrouter && policy_refusal(status, msg) {
        return ClientError::Protocol(POLICY_REFUSED.to_string());
    }
    map_http_status(
        status,
        &grokhub_core::redact_secrets(msg),
        None,
        AuthKind::ApiKey,
    )
}

// ---- The call ----------------------------------------------------------------

/// A provider call with everything decided and allowed: the provider,
/// its key, the request body ready to serialize and the registry
/// snapshot the reply is read against. Built once by [`prepare_call`];
/// [`call_model`] sends it whole and [`stream_model`] streams it when
/// the provider is Anthropic or OpenRouter.
struct ReadyCall {
    p: Provider,
    wire_model: String,
    key: Zeroizing<String>,
    url: String,
    openrouter: bool,
    backup_wire: Option<String>,
    reg: Arc<Registry>,
    body: Value,
    call_timeout: Option<Duration>,
}

/// What [`prepare_call`] settled: a turn answered on this computer (a
/// held reply; nothing was sent), or the call ready to go out.
enum Prep {
    Held(Box<TurnOutput>),
    Ready(Box<ReadyCall>),
}

/// Everything a provider call does before the wire: the guards, the
/// grant (one egress line, `span_id` on it; with no grant a hard send
/// card parks and waits — Approve once = one `approved_once` line — and
/// Deny, Esc, the timeout or a cancel send nothing) and the body.
/// `stream` asks Anthropic or OpenRouter for its event stream.
fn prepare_call(
    config_dir: &Path,
    class: &str,
    model: &str,
    effort: Option<&str>,
    req: &ResponsesRequest,
    data: &[DataClass],
    span_id: &str,
    cancel: &CancelToken,
    stream: bool,
) -> Result<Prep, ClientError> {
    let (pid, wire_model) = split_provider_model(model)
        .ok_or_else(|| ClientError::Protocol(format!("`{model}` is not a provider model")))?;
    let p = provider(config_dir, pid)
        .ok_or_else(|| ClientError::Protocol(format!("no provider `{pid}`")))?;
    // Playbooks 8, the last line: a caller that skipped the router still never
    // sends a banned ask, or an 18+ chat to a provider that bans adult chat.
    let mut req = std::borrow::Cow::Borrowed(req);
    super::adult::scrub(&mut req.to_mut().input);
    if let Some(held) = super::adult::held_reply(&req.input) {
        return Ok(Prep::Held(Box::new(held)));
    }
    if let Some(held) = super::coaching::held_reply(class, &req.input) {
        return Ok(Prep::Held(Box::new(held)));
    }
    let to = dest(config_dir, model);
    if super::adult::to_local(&req.input, to) {
        return Err(ClientError::Protocol(format!(
            "18+ chats run only on this computer's model, so this chat was not sent to {}",
            grokhub_core::adult_guard::dest_name(to)
        )));
    }
    let openrouter = p.is_openrouter();
    // The prompt calls the assistant Grok: every call that reaches a provider
    // is told which model really answers, so it says so when asked.
    super::identity::stamp(&mut req.to_mut().input, model, None, effort);
    let key = key(config_dir, &p.id)
        .ok_or_else(|| ClientError::Protocol(format!("no key for {}: add it in Settings", p.id)))?;
    let url = p.call_url();
    let egress = EgressReq {
        span_id,
        ..EgressReq::new(&url, data)
    };
    provider_or_park(
        config_dir,
        &egress,
        PROVIDER_TOOL,
        crate::harness::APPROVAL_TTL,
        &mut || cancel.is_cancelled(),
    )
    .map_err(ClientError::Protocol)?;
    if cancel.is_cancelled() {
        return Err(ClientError::Cancelled);
    }
    let (reg, profiles) = super::live::snapshot(config_dir);
    let max_output = reg.get(model).and_then(|r| r.meta.max_output);
    let mut body = request_body(p.kind, wire_model, effort, &req, max_output);
    let backup = openrouter
        .then(|| call_backup(config_dir, &reg, &profiles, class, model, effort, &req))
        .flatten();
    let backup_wire = backup
        .as_deref()
        .and_then(split_provider_model)
        .map(|(_, m)| m);
    if openrouter {
        apply_openrouter_routing(&mut body, &routing());
        apply_openrouter_picks(&mut body, backup_wire, &req.conversation_id);
    }
    if stream && p.kind == ProviderKind::Anthropic {
        body["stream"] = json!(true);
    }
    // OpenRouter streams the OpenAI way; its usage chunk (cost included)
    // comes last, after the finish reason.
    if stream && openrouter && p.kind == ProviderKind::OpenAiCompatible {
        body["stream"] = json!(true);
        body["stream_options"] = json!({"include_usage": true});
    }
    crate::harness::replay::note_request(config_dir, &body);
    Ok(Prep::Ready(Box::new(ReadyCall {
        p,
        wire_model: wire_model.to_string(),
        key,
        url,
        openrouter,
        backup_wire: backup_wire.map(str::to_string),
        reg,
        body,
        call_timeout: req.call_timeout,
    })))
}

/// Send one routed call to a provider you added: `model` is its registry id
/// (`<provider>/<model>`), `effort` the router's pick from that model's own
/// list, `class` the call's class (an OpenRouter call's backup is ranked for
/// it). It goes only under your grant (one egress line, `span_id` on it);
/// with no grant a hard send card parks and waits (Approve once = one
/// `approved_once` line), and Deny, Esc, the timeout or a cancel send nothing.
pub fn call_model(
    config_dir: &Path,
    class: &str,
    model: &str,
    effort: Option<&str>,
    req: &ResponsesRequest,
    data: &[DataClass],
    span_id: &str,
    cancel: &CancelToken,
) -> Result<TurnOutput, ClientError> {
    let ready = match prepare_call(
        config_dir, class, model, effort, req, data, span_id, cancel, false,
    )? {
        Prep::Held(turn) => return Ok(*turn),
        Prep::Ready(r) => r,
    };
    let body = ready.body.to_string();
    let r = transport_for(config_dir)
        .post_within(
            &ready.url,
            &ready.p.headers(&ready.key),
            &body,
            ready.call_timeout,
        )
        .map_err(|e| ClientError::Transport(grokhub_core::redact_secrets(&e)))?;
    // Cancelled while on the wire (a replaced or timed-out request): drop the reply.
    if cancel.is_cancelled() {
        return Err(ClientError::Cancelled);
    }
    if !(200..300).contains(&r.status) {
        // A limit, an empty account or a refused key is account news, not
        // just a failed call: the quota ledger stamps it (an Anthropic
        // bench, an OpenRouter re-read) while the reply is in hand.
        super::quota::note_provider_error(
            config_dir,
            &ready.p,
            model,
            r.status,
            &r.body,
            &r.headers,
            grokhub_core::now_ms(),
        );
        return Err(error_of(r.status, &r.body, ready.openrouter));
    }
    let mut turn = match ready.p.kind {
        ProviderKind::OpenAiCompatible => parse_openai_reply(&r.body),
        ProviderKind::Anthropic => parse_anthropic_reply(&r.body),
    }?;
    if ready.p.kind == ProviderKind::Anthropic {
        // Claude's account state rides on its replies: rate-limit headers
        // and usage for the local spend ledger (quota.rs).
        let prices = ready
            .reg
            .get(model)
            .map(|rec| rec.meta.prices.clone())
            .filter(|p| !p.is_empty());
        super::quota::note_anthropic_reply(
            config_dir,
            prices.as_ref(),
            &turn.usage,
            anthropic_cache_write(&r.body),
            &r.headers,
            grokhub_core::now_ms(),
        );
    }
    // The next request names the model as the provider does ([`openai_body`]):
    // the backup's thinking goes back to the backup.
    let answered: Option<String> = serde_json::from_str::<Value>(&r.body)
        .ok()
        .and_then(|v| v["model"].as_str().map(str::to_string));
    tag_reasoning(&mut turn, &ready, answered.as_deref());
    Ok(turn)
}

/// [`call_model`] for the chat's streamed path: an Anthropic or an
/// OpenRouter call is sent as an event stream and the answer reaches
/// `sink` as it is written, with the native stream's failure rules (a
/// mid-stream failure before anything showed fails over like any
/// provider error; one after text showed is a cut-off the turn stops
/// with). Any other OpenAI-compatible host you added keeps the
/// whole-reply call — its answer goes out as one delta, the way the
/// chat has always shown it, since not every such host streams the
/// same way. A held reply goes out as one delta too.
pub fn stream_model(
    config_dir: &Path,
    class: &str,
    model: &str,
    effort: Option<&str>,
    req: &ResponsesRequest,
    data: &[DataClass],
    span_id: &str,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(StreamEvent),
) -> Result<TurnOutput, ClientError> {
    let streams = split_provider_model(model)
        .and_then(|(pid, _)| provider(config_dir, pid))
        .is_some_and(|p| p.kind == ProviderKind::Anthropic || p.is_openrouter());
    if !streams {
        let turn = call_model(config_dir, class, model, effort, req, data, span_id, cancel)?;
        if !turn.text.is_empty() {
            sink(StreamEvent::TextDelta(turn.text.clone()));
        }
        return Ok(turn);
    }
    let ready = match prepare_call(
        config_dir, class, model, effort, req, data, span_id, cancel, true,
    )? {
        Prep::Held(turn) => {
            if !turn.text.is_empty() {
                sink(StreamEvent::TextDelta(turn.text.clone()));
            }
            return Ok(*turn);
        }
        Prep::Ready(r) => r,
    };
    let body = ready.body.to_string();
    // Stop hangs up the socket even before the first byte, so the provider
    // stops generating (and billing) a reply nobody will read.
    let stop = cancel.stop_switch();
    let opened = grokhub_core::http::with_stop(&stop, || {
        transport_for(config_dir).post_streamed(
            &ready.url,
            &ready.p.headers(&ready.key),
            &body,
            ready.call_timeout,
        )
    });
    if opened.is_err() && cancel.is_cancelled() {
        return Err(ClientError::Cancelled);
    }
    let mut s = opened.map_err(|e| ClientError::Transport(grokhub_core::redact_secrets(&e)))?;
    // Cancelled while the stream opened: drop it unread.
    if cancel.is_cancelled() {
        return Err(ClientError::Cancelled);
    }
    if !(200..300).contains(&s.status) {
        // The stream never started: the body is the error, read whole,
        // and the quota ledger stamps it like any failed call.
        let mut text = String::new();
        s.reader
            .by_ref()
            .take(BODY_CAP)
            .read_to_string(&mut text)
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        super::quota::note_provider_error(
            config_dir,
            &ready.p,
            model,
            s.status,
            &text,
            &s.headers,
            grokhub_core::now_ms(),
        );
        return Err(error_of(s.status, &text, ready.openrouter));
    }
    let headers = std::mem::take(&mut s.headers);
    if ready.p.kind == ProviderKind::OpenAiCompatible {
        return read_openrouter_reply(config_dir, model, &ready, s.reader, &headers, cancel, sink);
    }
    let (turn, cache_write) = read_anthropic_stream(s.reader, cancel, sink)?;
    // Claude's account state rides on its replies, streamed ones too:
    // rate-limit headers and usage for the local spend ledger (quota.rs).
    let prices = ready
        .reg
        .get(model)
        .map(|rec| rec.meta.prices.clone())
        .filter(|p| !p.is_empty());
    super::quota::note_anthropic_reply(
        config_dir,
        prices.as_ref(),
        &turn.usage,
        cache_write,
        &headers,
        grokhub_core::now_ms(),
    );
    Ok(turn)
}

/// A streamed OpenRouter reply read to its end. A host that answered
/// with the whole reply instead of an event stream (a proxy in the way)
/// is read whole, its answer sent as one delta. An error chunk on the
/// stream is account news like any failed call: the quota ledger stamps
/// it with the status the chunk carried.
fn read_openrouter_reply(
    config_dir: &Path,
    model: &str,
    ready: &ReadyCall,
    reader: Box<dyn Read + Send>,
    headers: &[(String, String)],
    cancel: &CancelToken,
    sink: &mut dyn FnMut(StreamEvent),
) -> Result<TurnOutput, ClientError> {
    let mut st = OpenAiStream::default();
    let read = read_stream(reader, cancel, sink, &mut st);
    if let Some((status, body)) = st.error_status.take() {
        super::quota::note_provider_error(
            config_dir,
            &ready.p,
            model,
            status,
            &body,
            headers,
            grokhub_core::now_ms(),
        );
    }
    read?;
    // No frames, but a JSON body: the whole reply came instead of a stream.
    // (A stream of keep-alives alone is not JSON, and ends as a disconnect.)
    let whole = (st.frames == 0)
        .then(|| serde_json::from_slice::<Value>(&st.raw).ok())
        .flatten();
    if let Some(v) = whole {
        let mut turn = parse_openai_reply(&v.to_string())?;
        tag_reasoning(&mut turn, ready, v["model"].as_str());
        if !turn.text.is_empty() {
            sink(StreamEvent::TextDelta(turn.text.clone()));
        }
        return Ok(turn);
    }
    let shown = st.shown;
    let (mut turn, answered) = st.finish().map_err(|e| cut_off_if_shown(shown, e))?;
    tag_reasoning(&mut turn, ready, answered.as_deref());
    Ok(turn)
}

/// A tryout probe's data: synthetic chat text, nothing of yours.
pub const PROBE_DATA: &[DataClass] = &[DataClass::Chat];
/// The tool a tryout probe asks the model to call.
pub const PROBE_TOOL: &str = "record_number";

/// What a tryout probe checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrialProbe {
    /// Call [`PROBE_TOOL`] with `n` = 7, as valid JSON.
    Tool,
    /// Answer one word.
    Text,
}

/// Send one small tryout probe to `model` (a provider model on its
/// two-week tryout) and say whether it passed. It goes out only under a
/// grant that already covers [`PROBE_DATA`] (the caller checks
/// [`usable`] first; nothing here parks a card), with no OpenRouter
/// backup, so the answer is the model's own.
pub fn probe_model(
    config_dir: &Path,
    model: &str,
    kind: TrialProbe,
    span_id: &str,
) -> Result<bool, ClientError> {
    let (ask, tools) = match kind {
        TrialProbe::Tool => (
            format!("Call the {PROBE_TOOL} tool with n set to 7. Do not answer in text."),
            vec![json!({
                "type": "function",
                "name": PROBE_TOOL,
                "description": "Records one number.",
                "parameters": {
                    "type": "object",
                    "properties": {"n": {"type": "integer"}},
                    "required": ["n"],
                },
            })],
        ),
        TrialProbe::Text => ("Reply with the single word: ready".to_string(), Vec::new()),
    };
    let req = ResponsesRequest {
        text_format: None,
        model: model.to_string(),
        effort: None,
        input: vec![InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(ask)],
        }],
        conversation_id: String::new(),
        tools,
        hosted_search: false,
        call_timeout: Some(Duration::from_secs(60)),
    };
    let cancel = CancelToken::new();
    let mut ready = match prepare_call(
        config_dir,
        PROBE_CLASS,
        model,
        None,
        &req,
        PROBE_DATA,
        span_id,
        &cancel,
        false,
    )? {
        // Held back before the wire (a cancel, a spend hold): that says
        // nothing about the model.
        Prep::Held(_) => return Err(ClientError::Protocol("the probe was held".into())),
        Prep::Ready(r) => r,
    };
    // The model's own answer: no backup to stand in for it.
    if let Some(body) = ready.body.as_object_mut() {
        body.remove("models");
        body.insert("max_tokens".into(), json!(PROBE_MAX_TOKENS));
    }
    let r = transport_for(config_dir)
        .post_within(
            &ready.url,
            &ready.p.headers(&ready.key),
            &ready.body.to_string(),
            ready.call_timeout,
        )
        .map_err(|e| ClientError::Transport(grokhub_core::redact_secrets(&e)))?;
    if !(200..300).contains(&r.status) {
        super::quota::note_provider_error(
            config_dir,
            &ready.p,
            model,
            r.status,
            &r.body,
            &r.headers,
            grokhub_core::now_ms(),
        );
        return Err(error_of(r.status, &r.body, ready.openrouter));
    }
    let turn = match ready.p.kind {
        ProviderKind::OpenAiCompatible => parse_openai_reply(&r.body),
        ProviderKind::Anthropic => parse_anthropic_reply(&r.body),
    }?;
    Ok(probe_passed(kind, &turn))
}

/// The class a tryout probe is sent under.
pub const PROBE_CLASS: &str = "background:probe";
/// Room for a probe's answer, thinking included.
const PROBE_MAX_TOKENS: u64 = 512;

/// A probe's answer passed its check.
pub fn probe_passed(kind: TrialProbe, turn: &TurnOutput) -> bool {
    match kind {
        TrialProbe::Tool => turn.calls.iter().any(|c| {
            c.name == PROBE_TOOL
                && serde_json::from_str::<Value>(&c.arguments)
                    .is_ok_and(|a| a["n"].as_i64() == Some(7) || a["n"].as_str() == Some("7"))
        }),
        TrialProbe::Text => turn.text.to_ascii_lowercase().contains("ready"),
    }
}

/// A live read of `id`'s model listing with its saved key, for the
/// connection check ([`super::check`]): how many models it lists, or the
/// error its answer maps to. Nothing of yours goes out (the same gate the
/// registry's listing passes, with no data), and the daily OpenRouter
/// cache is skipped: this asks the wire now.
pub fn live_listing(config_dir: &Path, id: &str) -> Result<usize, ClientError> {
    let p = provider(config_dir, id)
        .ok_or_else(|| ClientError::Protocol(format!("no provider `{id}`")))?;
    let key = key(config_dir, &p.id).ok_or_else(|| ClientError::Protocol("no key".into()))?;
    let url = p.models_url();
    if let GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } =
        guard_provider(config_dir, &EgressReq::new(&url, &[]))
    {
        return Err(ClientError::Protocol(reason));
    }
    let r = transport_for(config_dir)
        .get(&url, &p.headers(&key))
        .map_err(|e| ClientError::Transport(grokhub_core::redact_secrets(&e)))?;
    if !(200..300).contains(&r.status) {
        return Err(error_of(r.status, &r.body, p.is_openrouter()));
    }
    let rows = match p.kind {
        ProviderKind::OpenAiCompatible => parse_openai_compatible_catalog(&p.id, &r.body),
        ProviderKind::Anthropic => parse_anthropic_catalog(&p.id, &r.body),
    }
    .map_err(ClientError::Protocol)?;
    Ok(rows.len())
}

/// The backup for an OpenRouter call of `class` to `model`: it must take what
/// this request sends (`require_parameters`): tools when it carries them, an
/// effort when one goes or the lane reasons, images when it has them.
fn call_backup(
    config_dir: &Path,
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    class: &str,
    model: &str,
    effort: Option<&str>,
    req: &ResponsesRequest,
) -> Option<String> {
    let effort_sent = effort
        .map(str::trim)
        .is_some_and(|e| !e.is_empty() && e != "none");
    let ctx_tokens = crate::compact::estimate_input_tokens(&req.input);
    let needs_image = req
        .input
        .iter()
        .any(|i| matches!(i, InputItem::Message { content, .. } if has_image(content)));
    let need = PickNeed {
        tools: !req.tools.is_empty(),
        reasoning: effort_sent || super::lane_needs_reasoning(class),
        ctx_tokens,
        ceiling_usd_per_m: super::spend::spend_settings().ceiling_usd_per_m,
    };
    let fit = super::policy::Fit {
        needs_image,
        needs_tools: need.tools,
        ctx_tokens,
        grok_build: false,
    };
    openrouter_backup(
        reg,
        profiles,
        &super::table::load_table(config_dir),
        class,
        model,
        &need,
        fit,
        grokhub_core::now_ms(),
    )
}

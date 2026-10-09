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
//!   sensitive data needs a grant that names it) and you or the class
//!   preference picked it; never as a fallback and never to save money.
//! - Effort maps per provider: xAI `reasoning.effort` (unchanged), Anthropic
//!   `output_config.effort`, OpenAI-compatible `reasoning.effort`. The effort
//!   list, prices and context come from the listing; nothing is filled in.
//!
//! Never Grok Build's credentials and never `~/.grok`.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use grokhub_core::model_registry::discover::{parse_anthropic_catalog, parse_openai_compatible_catalog, split_provider_model};
use grokhub_core::model_registry::store::{models_dir, read_json, write_json};
use grokhub_core::model_registry::{CatalogSource, Listing, SourceKind};

use crate::client::{
    map_http_status, AuthKind, ClientError, ContentPart, FunctionCall, InputItem, ResponsesRequest, TurnOutput, Usage, USER_AGENT,
};
use crate::harness::{egress_dest, guard_provider, is_model_host, provider_or_park, ConsentLedger, DataClass, EgressReq, GateOutcome};
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
pub const CALL_DATA_SENSITIVE: &[DataClass] = &[DataClass::Chat, DataClass::Personal, DataClass::Sensitive];
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
        let mut h = vec![("User-Agent".to_string(), USER_AGENT.to_string()), ("Content-Type".into(), "application/json".into())];
        match self.kind {
            ProviderKind::OpenAiCompatible => h.push(("Authorization".into(), format!("Bearer {key}"))),
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
    let host = egress_dest(url);
    if host.is_empty() || !host.contains('.') {
        return Err("That address has no host.".into());
    }
    if is_model_host(&host) {
        return Err("xAI is already built in.".into());
    }
    let kind = if host == "anthropic.com" || host.ends_with(".anthropic.com") { ProviderKind::Anthropic } else { ProviderKind::OpenAiCompatible };
    let labels: Vec<&str> = host.split('.').collect();
    let name = labels.iter().copied().find(|l| !matches!(*l, "api" | "www" | "gateway")).unwrap_or(labels[0]);
    let id: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>().to_ascii_lowercase();
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
pub fn load_providers(config_dir: &Path) -> Vec<Provider> {
    read_json::<ProviderList>(&providers_path(config_dir)).map(|l| l.providers).unwrap_or_default()
}

fn save_providers(config_dir: &Path, providers: &[Provider]) -> Result<(), String> {
    write_json(&providers_path(config_dir), &ProviderList { providers: providers.to_vec() })
}

pub fn provider(config_dir: &Path, id: &str) -> Option<Provider> {
    load_providers(config_dir).into_iter().find(|p| p.id == id.trim())
}

/// Settings → "Add a provider": the key goes into the keyring first; only if
/// that worked is the provider (no key) saved. Adding sends nothing: the
/// destination is allowed separately, on a hard send card.
pub fn add_provider(config_dir: &Path, base_url: &str, key: &str, now_ms: u64) -> Result<Provider, String> {
    let (url, kind, id) = parse_base_url(base_url)?;
    let key = key.trim();
    if key.len() < 8 || key.contains(char::is_whitespace) {
        return Err("Paste the whole key.".into());
    }
    vault_for(config_dir).ok_or_else(|| "The keyring isn't available, so the key wasn't saved.".to_string())?.set(&id, key)?;
    forget_key_cache(config_dir, &id);
    let mut all = load_providers(config_dir);
    let p = Provider { id: id.clone(), kind, base_url: url, added_at: now_ms };
    match all.iter_mut().find(|x| x.id == id) {
        Some(slot) => *slot = p.clone(),
        None => all.push(p.clone()),
    }
    save_providers(config_dir, &all)?;
    Ok(p)
}

/// Remove a provider and its key. Its grant stays in Settings → Permissions until you revoke it.
pub fn remove_provider(config_dir: &Path, id: &str) -> Result<(), String> {
    if let Some(v) = vault_for(config_dir) {
        v.delete(id)?;
    }
    forget_key_cache(config_dir, id);
    let mut all = load_providers(config_dir);
    all.retain(|p| p.id != id);
    save_providers(config_dir, &all)
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
        Ok(self.0.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned().map(Zeroizing::new))
    }

    fn set(&self, id: &str, key: &str) -> Result<(), String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), key.to_string());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
        Ok(())
    }
}

type VaultSlot = Option<Arc<dyn KeyVault>>;
static DEFAULT_VAULT: Mutex<VaultSlot> = Mutex::new(None);
static VAULTS: Mutex<Vec<(PathBuf, Arc<dyn KeyVault>)>> = Mutex::new(Vec::new());
/// Whether a key is there, per (config dir, provider), so routing doesn't ask the keyring every call.
static HAS_KEY: Mutex<BTreeMap<(PathBuf, String), bool>> = Mutex::new(BTreeMap::new());

/// `grokhub` calls this once at start. Until then (and in tests) there is no
/// vault: no key reads as present and nothing can be saved.
pub fn use_os_vault() {
    *DEFAULT_VAULT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(OsKeyVault));
}

/// Use `vault` for this config dir only (tests).
pub fn use_vault_for(config_dir: &Path, vault: Arc<dyn KeyVault>) {
    let mut v = VAULTS.lock().unwrap_or_else(|e| e.into_inner());
    v.retain(|(d, _)| d != config_dir);
    v.push((config_dir.to_path_buf(), vault));
    HAS_KEY.lock().unwrap_or_else(|e| e.into_inner()).retain(|(d, _), _| d != config_dir);
}

fn vault_for(config_dir: &Path) -> Option<Arc<dyn KeyVault>> {
    let over = VAULTS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(d, _)| d == config_dir).map(|(_, v)| v.clone());
    over.or_else(|| DEFAULT_VAULT.lock().unwrap_or_else(|e| e.into_inner()).clone())
}

fn forget_key_cache(config_dir: &Path, id: &str) {
    HAS_KEY.lock().unwrap_or_else(|e| e.into_inner()).remove(&(config_dir.to_path_buf(), id.to_string()));
}

fn key(config_dir: &Path, id: &str) -> Option<Zeroizing<String>> {
    let _lap = crate::timing::lap("route:provider_key_read");
    vault_for(config_dir)?.get(id).ok().flatten().filter(|k| !k.trim().is_empty())
}

/// A key for this provider is in the keyring.
pub fn has_key(config_dir: &Path, id: &str) -> bool {
    let slot = (config_dir.to_path_buf(), id.to_string());
    if let Some(have) = HAS_KEY.lock().unwrap_or_else(|e| e.into_inner()).get(&slot) {
        return *have;
    }
    let have = key(config_dir, id).is_some();
    HAS_KEY.lock().unwrap_or_else(|e| e.into_inner()).insert(slot, have);
    have
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
        .filter(|p| ledger.destination_grant(&p.dest(), data).is_some() && has_key(config_dir, &p.id))
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

/// One HTTP answer: status and body (capped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    pub status: u16,
    pub body: String,
}

/// How provider requests leave. Tests use a counting fake: zero network.
pub trait ProviderTransport: Send + Sync {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpReply, String>;
    fn post(&self, url: &str, headers: &[(String, String)], body: &str) -> Result<HttpReply, String>;
}

/// The real wire. Every call reaches it only after [`guard_provider`].
pub struct UreqTransport;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(20)).timeout(Duration::from_secs(600)).build()
}

fn reply(r: Result<ureq::Response, ureq::Error>) -> Result<HttpReply, String> {
    let resp = match r {
        Ok(resp) => resp,
        Err(ureq::Error::Status(_, resp)) => resp,
        Err(ureq::Error::Transport(t)) => return Err(t.to_string()),
    };
    let status = resp.status();
    let mut body = String::new();
    resp.into_reader().take(BODY_CAP).read_to_string(&mut body).map_err(|e| e.to_string())?;
    Ok(HttpReply { status, body })
}

impl ProviderTransport for UreqTransport {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpReply, String> {
        let mut req = agent().get(url);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        reply(req.call())
    }

    fn post(&self, url: &str, headers: &[(String, String)], body: &str) -> Result<HttpReply, String> {
        let mut req = agent().post(url);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        reply(req.send_string(body))
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
    let over = TRANSPORTS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(d, _)| d == config_dir).map(|(_, t)| t.clone());
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

/// One source per provider you added.
pub fn sources(config_dir: &Path) -> Vec<ProviderSource> {
    load_providers(config_dir).into_iter().map(|provider| ProviderSource { config_dir: config_dir.to_path_buf(), provider }).collect()
}

impl CatalogSource for ProviderSource {
    fn kind(&self) -> SourceKind {
        self.provider.kind.source_kind()
    }

    fn fetch(&self) -> Result<Listing, String> {
        let p = &self.provider;
        let key = key(&self.config_dir, &p.id).ok_or_else(|| format!("{}: no key", p.id))?;
        let url = p.models_url();
        if let GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } = guard_provider(&self.config_dir, &EgressReq::new(&url, &[])) {
            return Err(format!("{}: {reason}", p.id));
        }
        let r = transport_for(&self.config_dir).get(&url, &p.headers(&key))?;
        if !(200..300).contains(&r.status) {
            return Err(format!("{}: HTTP {}", p.id, r.status));
        }
        let rows = match p.kind {
            ProviderKind::OpenAiCompatible => parse_openai_compatible_catalog(&p.id, &r.body)?,
            ProviderKind::Anthropic => parse_anthropic_catalog(&p.id, &r.body)?,
        };
        Ok(Listing::new(self.kind(), rows))
    }
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
    content.iter().any(|c| matches!(c, ContentPart::InputImage(_)))
}

/// A Responses function tool (`{"type":"function","name",…}`) as its name,
/// description and schema. Hosted tools (web search) don't map and are dropped.
fn function_tool(t: &Value) -> Option<(String, String, Value)> {
    if t["type"].as_str() != Some("function") {
        return None;
    }
    let f = if t.get("function").is_some() { &t["function"] } else { t };
    let name = f["name"].as_str()?.to_string();
    let desc = f["description"].as_str().unwrap_or("").to_string();
    let params = f.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    Some((name, desc, params))
}

/// OpenAI-compatible `/chat/completions`. Effort is `reasoning.effort`.
pub fn openai_body(model: &str, effort: Option<&str>, req: &ResponsesRequest) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    for item in &req.input {
        match item {
            InputItem::Message { role, content } => {
                let role = if matches!(role.as_str(), "system" | "developer") { "system" } else { role.as_str() };
                let content = if has_image(content) {
                    Value::Array(
                        content
                            .iter()
                            .map(|c| match c {
                                ContentPart::InputText(t) => json!({"type": "text", "text": t}),
                                ContentPart::InputImage(u) => json!({"type": "image_url", "image_url": {"url": u}}),
                            })
                            .collect(),
                    )
                } else {
                    Value::String(texts(content))
                };
                messages.push(json!({"role": role, "content": content}));
            }
            InputItem::FunctionCall { call_id, name, arguments } => {
                let call = json!({"id": call_id, "type": "function", "function": {"name": name, "arguments": arguments}});
                match messages.last_mut() {
                    Some(m) if m["role"] == "assistant" && m.get("tool_calls").is_some() => {
                        if let Some(calls) = m["tool_calls"].as_array_mut() {
                            calls.push(call);
                        }
                    }
                    _ => messages.push(json!({"role": "assistant", "content": Value::Null, "tool_calls": [call]})),
                }
            }
            InputItem::FunctionCallOutput { call_id, output } => {
                messages.push(json!({"role": "tool", "tool_call_id": call_id, "content": output}));
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
    if let Some(e) = effort.map(str::trim).filter(|e| !e.is_empty() && *e != "none") {
        body["reasoning"] = json!({"effort": e});
    }
    body
}

fn anthropic_image(url: &str) -> Value {
    // `data:image/png;base64,…` goes inline; anything else by URL.
    if let Some((head, data)) = url.strip_prefix("data:").and_then(|r| r.split_once(";base64,")) {
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

/// Anthropic `/v1/messages`. Effort is `output_config.effort`.
pub fn anthropic_body(model: &str, effort: Option<&str>, req: &ResponsesRequest, max_output: Option<u64>) -> Value {
    let mut system: Vec<String> = Vec::new();
    let mut messages: Vec<Value> = Vec::new();
    for item in &req.input {
        match item {
            InputItem::Message { role, content } if matches!(role.as_str(), "system" | "developer") => system.push(texts(content)),
            InputItem::Message { role, content } => {
                let role = if role == "assistant" { "assistant" } else { "user" };
                for c in content {
                    let block = match c {
                        ContentPart::InputText(t) => json!({"type": "text", "text": t}),
                        ContentPart::InputImage(u) => anthropic_image(u),
                    };
                    push_block(&mut messages, role, block);
                }
            }
            InputItem::FunctionCall { call_id, name, arguments } => {
                let input: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
                push_block(&mut messages, "assistant", json!({"type": "tool_use", "id": call_id, "name": name, "input": input}));
            }
            InputItem::FunctionCallOutput { call_id, output } => {
                push_block(&mut messages, "user", json!({"type": "tool_result", "tool_use_id": call_id, "content": output}));
            }
        }
    }
    let max_tokens = max_output.filter(|m| *m > 0).map(|m| m.min(ANTHROPIC_MAX_TOKENS)).unwrap_or(ANTHROPIC_MAX_TOKENS);
    let mut body = json!({"model": model, "max_tokens": max_tokens, "messages": messages});
    if !system.is_empty() {
        body["system"] = Value::String(system.join("\n\n"));
    }
    let tools: Vec<Value> = req
        .tools
        .iter()
        .filter_map(function_tool)
        .map(|(name, description, schema)| json!({"name": name, "description": description, "input_schema": schema}))
        .collect();
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if let Some(e) = effort.map(str::trim).filter(|e| !e.is_empty() && *e != "none") {
        body["output_config"] = json!({"effort": e});
    }
    body
}

/// The request body a provider gets for this call (`model` is the provider's own id).
pub fn request_body(kind: ProviderKind, model: &str, effort: Option<&str>, req: &ResponsesRequest, max_output: Option<u64>) -> Value {
    match kind {
        ProviderKind::OpenAiCompatible => openai_body(model, effort, req),
        ProviderKind::Anthropic => anthropic_body(model, effort, req, max_output),
    }
}

// ---- Replies -----------------------------------------------------------------

fn u64_at(v: &Value, path: &[&str]) -> u64 {
    path.iter().fold(v, |v, k| &v[*k]).as_u64().unwrap_or(0)
}

/// A `/chat/completions` answer as a turn. OpenRouter's `usage.cost` (USD) becomes ticks.
pub fn parse_openai_reply(body: &str) -> Result<TurnOutput, ClientError> {
    let v: Value = serde_json::from_str(body).map_err(|e| ClientError::Protocol(format!("provider reply: {e}")))?;
    let msg = &v["choices"][0]["message"];
    if msg.is_null() {
        return Err(ClientError::Protocol("provider reply has no message".into()));
    }
    let calls = msg["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| FunctionCall {
            call_id: c["id"].as_str().unwrap_or("").to_string(),
            name: c["function"]["name"].as_str().unwrap_or("").to_string(),
            arguments: c["function"]["arguments"].as_str().unwrap_or("{}").to_string(),
        })
        .collect();
    let u = &v["usage"];
    Ok(TurnOutput {
        text: msg["content"].as_str().unwrap_or("").to_string(),
        reasoning: msg["reasoning"].as_str().unwrap_or("").to_string(),
        calls,
        usage: Usage {
            input_tokens: u64_at(u, &["prompt_tokens"]),
            output_tokens: u64_at(u, &["completion_tokens"]),
            reasoning_tokens: u64_at(u, &["completion_tokens_details", "reasoning_tokens"]),
            cached_tokens: u64_at(u, &["prompt_tokens_details", "cached_tokens"]),
            cost_in_usd_ticks: u["cost"].as_f64().map(|c| (c * 1e10).round() as i64).unwrap_or(0),
        },
    })
}

/// A `/v1/messages` answer as a turn.
pub fn parse_anthropic_reply(body: &str) -> Result<TurnOutput, ClientError> {
    let v: Value = serde_json::from_str(body).map_err(|e| ClientError::Protocol(format!("provider reply: {e}")))?;
    let Some(blocks) = v["content"].as_array() else {
        return Err(ClientError::Protocol("provider reply has no content".into()));
    };
    let mut out = TurnOutput { text: String::new(), reasoning: String::new(), calls: Vec::new(), usage: Usage::default() };
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
    let u = &v["usage"];
    out.usage = Usage {
        input_tokens: u64_at(u, &["input_tokens"]),
        output_tokens: u64_at(u, &["output_tokens"]),
        cached_tokens: u64_at(u, &["cache_read_input_tokens"]),
        ..Usage::default()
    };
    Ok(out)
}

fn error_of(status: u16, body: &str) -> ClientError {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let msg = v["error"]["message"].as_str().or_else(|| v["error"].as_str()).or_else(|| v["message"].as_str()).unwrap_or("");
    map_http_status(status, &grokhub_core::redact_secrets(msg), None, AuthKind::ApiKey)
}

// ---- The call ----------------------------------------------------------------

/// Send one routed call to a provider you added: `model` is its registry id
/// (`<provider>/<model>`), `effort` the router's pick from that model's own
/// list. It goes only under your grant (one egress line, `span_id` on it);
/// with no grant a hard send card parks and waits (Approve once = one
/// `approved_once` line), and Deny, Esc, the timeout or a cancel send nothing.
pub fn call_model(
    config_dir: &Path,
    model: &str,
    effort: Option<&str>,
    req: &ResponsesRequest,
    data: &[DataClass],
    span_id: &str,
    cancel: &CancelToken,
) -> Result<TurnOutput, ClientError> {
    let (pid, wire_model) = split_provider_model(model).ok_or_else(|| ClientError::Protocol(format!("`{model}` is not a provider model")))?;
    let p = provider(config_dir, pid).ok_or_else(|| ClientError::Protocol(format!("no provider `{pid}`")))?;
    let key = key(config_dir, &p.id).ok_or_else(|| ClientError::Protocol(format!("no key for {}: add it in Settings", p.id)))?;
    let url = p.call_url();
    let egress = EgressReq { span_id, ..EgressReq::new(&url, data) };
    provider_or_park(config_dir, &egress, PROVIDER_TOOL, crate::harness::APPROVAL_TTL, &mut || cancel.is_cancelled()).map_err(ClientError::Protocol)?;
    if cancel.is_cancelled() {
        return Err(ClientError::Cancelled);
    }
    let (reg, _) = super::live::snapshot(config_dir);
    let max_output = reg.get(model).and_then(|r| r.meta.max_output);
    let body = request_body(p.kind, wire_model, effort, req, max_output).to_string();
    let r = transport_for(config_dir).post(&url, &p.headers(&key), &body).map_err(|e| ClientError::Transport(grokhub_core::redact_secrets(&e)))?;
    if !(200..300).contains(&r.status) {
        return Err(error_of(r.status, &r.body));
    }
    match p.kind {
        ProviderKind::OpenAiCompatible => parse_openai_reply(&r.body),
        ProviderKind::Anthropic => parse_anthropic_reply(&r.body),
    }
}

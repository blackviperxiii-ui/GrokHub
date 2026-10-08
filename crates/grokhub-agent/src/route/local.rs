//! Router R3a: plumbing for an on-device model (§13). It is **off** and has
//! **no runtime**. `localModel` in `app.json` is `false` by default, and no
//! Settings row or slash command turns it on; turning it on needs Jeremy's go
//! and a separate PR that brings a runtime. While it is off the registry has
//! no `local:*` rows ([`LocalSource`] lists nothing) and [`super::Router::choose`]
//! drops any `local:*` id, so no route can pick one.
//!
//! When it is on (only tests switch it on, with a fake [`LocalRuntime`]):
//! - a `local:<tier>` route has cost class `included`, no effort menu and a
//!   context cap of [`LOCAL_CTX_CAP`];
//! - it is eligible only for [`LOCAL_CLASSES`] (`background:` classify, triage,
//!   redact, summarize) and as the difficulty estimator, never for
//!   `prepare:hard` or a step offering a send, delete or credential tool;
//! - with sensitive data and no cloud grant it is the only route ([`privacy_only`]);
//! - the downshift ladder (battery, low-end device) only changes which tier is live;
//! - PII is found by regex first, the model second ([`scan_pii`]).
//!
//! A local call never leaves the device: [`serve`] calls the runtime directly
//! and writes no egress line.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use regex::Regex;
use serde_json::Value;

use grokhub_core::model_registry::{CatalogSource, Listing, ModelMeta, Prices, SourceKind};

use super::ladder::PREPARE_HARD;
use super::policy::class_row;
use crate::client::{ClientError, TurnOutput, Usage};

pub const PROVIDER_LOCAL: &str = "local";
pub const LOCAL_PREFIX: &str = "local:";
/// The on-device model's context cap (2–4k), in tokens.
pub const LOCAL_CTX_CAP: u64 = 4_096;
/// The classes a local route may take.
pub const LOCAL_CLASSES: &[&str] = &["background:classify", "background:triage", "background:redact", "background:summarize"];
/// Tiers, strongest first. The downshift ladder moves down this list.
pub const TIERS: &[&str] = &["local:small", "local:tiny"];
/// What a local route returns while no runtime is installed.
pub const NO_RUNTIME_MSG: &str = "The on-device model isn't installed.";

static ENABLED: AtomicBool = AtomicBool::new(false);
static TIER: AtomicUsize = AtomicUsize::new(0);

/// The cabin sets this from `app.json` `localModel` (default `false`) at start.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::SeqCst);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst) || test_on()
}

pub fn is_local(id: &str) -> bool {
    id.trim().starts_with(LOCAL_PREFIX)
}

/// What the downshift ladder looks at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Device {
    pub battery_low: bool,
    pub low_end: bool,
}

/// The tier that is live on this device: one rung down for a low battery or a low-end machine.
pub fn tier_for(d: Device) -> &'static str {
    TIERS[usize::from(d.battery_low || d.low_end).min(TIERS.len() - 1)]
}

/// The downshift ladder: only which tier is live changes.
pub fn set_device(d: Device) {
    let t = tier_for(d);
    TIER.store(TIERS.iter().position(|x| *x == t).unwrap_or(0), Ordering::SeqCst);
}

pub fn live_tier() -> &'static str {
    TIERS[TIER.load(Ordering::SeqCst).min(TIERS.len() - 1)]
}

/// What a route may do with the local model for one call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalGate {
    pub enabled: bool,
    /// The live tier's id (`local:small`).
    pub tier: String,
    /// The step's data is marked sensitive.
    pub sensitive: bool,
    /// You granted sending sensitive data to the cloud.
    pub cloud_grant: bool,
    /// The step offers a send, delete or credential tool.
    pub hard_tool: bool,
}

impl LocalGate {
    /// The flag and the live tier as set now.
    pub fn now(sensitive: bool, cloud_grant: bool, hard_tool: bool) -> Self {
        Self { enabled: enabled(), tier: live_tier().into(), sensitive, cloud_grant, hard_tool }
    }
}

/// The local route may take this step: the flag is on, the class is one of
/// [`LOCAL_CLASSES`], the step fits the context cap, and it is not hard.
pub fn eligible(gate: &LocalGate, class: &str, ctx_tokens: u64) -> bool {
    let class = class_row(class).map(|r| r.class).unwrap_or(class.trim());
    gate.enabled && !gate.hard_tool && class != PREPARE_HARD && LOCAL_CLASSES.contains(&class) && ctx_tokens <= LOCAL_CTX_CAP
}

/// `id` may serve this step: the live tier, and [`eligible`].
pub fn admits(gate: &LocalGate, id: &str, class: &str, ctx_tokens: u64) -> bool {
    is_local(id) && id.trim() == gate.tier && eligible(gate, class, ctx_tokens)
}

/// Sensitive data with no cloud grant: the local route is the only route.
pub fn privacy_only(gate: &LocalGate) -> bool {
    gate.enabled && gate.sensitive && !gate.cloud_grant
}

/// The registry source for local tiers. Lists nothing (a no-op) while the flag is off.
pub struct LocalSource;

pub fn tier_meta(id: &str) -> ModelMeta {
    ModelMeta {
        id: id.into(),
        context_length: Some(LOCAL_CTX_CAP),
        // No effort menu: a local route sends no effort.
        efforts: Some(Vec::new()),
        tool_calling: Some(false),
        prices: Prices { prompt: Some(0), cached: Some(0), completion: Some(0), ..Prices::default() },
        ..ModelMeta::default()
    }
}

impl CatalogSource for LocalSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Local
    }

    fn fetch(&self) -> Result<Listing, String> {
        if !enabled() {
            return Err("the local model is off".into());
        }
        Ok(Listing::new(SourceKind::Local, TIERS.iter().map(|t| tier_meta(t)).collect()))
    }
}

/// An on-device model. Every answer is JSON-shaped. There is no real
/// runtime in this build; tests install a fake.
pub trait LocalRuntime: Send + Sync {
    /// `{"label": "<one of labels>", "score": 0.0..1.0}`.
    fn classify(&self, text: &str, labels: &[&str]) -> Result<Value, String>;
    /// `{"summary": "..."}`.
    fn summarize(&self, text: &str, max_words: u32) -> Result<Value, String>;
    /// `{"d": 0.0..1.0}`.
    fn difficulty(&self, text: &str) -> Result<Value, String>;
}

/// The installed runtime. This build has none: only a test's fake answers.
pub fn runtime() -> Option<Arc<dyn LocalRuntime>> {
    if !enabled() {
        return None;
    }
    test_runtime()
}

#[cfg(not(test))]
fn test_on() -> bool {
    false
}

#[cfg(not(test))]
fn test_runtime() -> Option<Arc<dyn LocalRuntime>> {
    None
}

#[cfg(test)]
thread_local! {
    static TEST_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static TEST_RT: std::cell::RefCell<Option<Arc<dyn LocalRuntime>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn test_on() -> bool {
    TEST_ON.with(std::cell::Cell::get)
}

#[cfg(test)]
fn test_runtime() -> Option<Arc<dyn LocalRuntime>> {
    TEST_RT.with(|r| r.borrow().clone())
}

/// Tests only: the flag on for this thread, with a fake runtime. Dropping it switches both off.
#[cfg(test)]
pub(crate) struct ForceOn;

#[cfg(test)]
impl ForceOn {
    pub(crate) fn with(rt: Option<Arc<dyn LocalRuntime>>) -> Self {
        TEST_ON.with(|c| c.set(true));
        TEST_RT.with(|r| *r.borrow_mut() = rt);
        Self
    }
}

#[cfg(test)]
impl Drop for ForceOn {
    fn drop(&mut self) {
        TEST_ON.with(|c| c.set(false));
        TEST_RT.with(|r| *r.borrow_mut() = None);
    }
}

/// Labels the background classes ask the local model for.
const CLASS_LABELS: &[&str] = &["routine", "needs_attention", "sensitive"];

/// Answer one local call on the device. No network, no egress line.
pub fn serve(class: &str, text: &str) -> Result<TurnOutput, ClientError> {
    let Some(rt) = runtime() else {
        return Err(ClientError::Protocol(NO_RUNTIME_MSG.into()));
    };
    let class = class_row(class).map(|r| r.class).unwrap_or(class);
    let answer = if class == "background:summarize" { rt.summarize(text, 60) } else { rt.classify(text, CLASS_LABELS) };
    let v = answer.map_err(ClientError::Protocol)?;
    Ok(TurnOutput { text: v.to_string(), reasoning: String::new(), calls: Vec::new(), usage: Usage::default() })
}

/// What [`scan_pii`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PiiScan {
    /// The text with secrets, emails and phone numbers masked.
    pub redacted: String,
    /// Masks the regex pass made.
    pub regex_hits: u32,
    /// The local model's word on the masked text, when it ran.
    pub model_flag: Option<bool>,
}

fn pii_patterns() -> &'static [Regex] {
    static P: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        [r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}", r"\+?\d[\d\s().-]{7,}\d"].iter().filter_map(|p| Regex::new(p).ok()).collect()
    })
}

/// PII: the regex pass first (secrets, emails, phone numbers), then the local
/// model on what is left, only when a runtime is there.
pub fn scan_pii(text: &str, rt: Option<&dyn LocalRuntime>) -> PiiScan {
    let mut out = grokhub_core::redact::redact_secrets(text);
    let mut hits = u32::from(out != text);
    for re in pii_patterns() {
        let n = re.find_iter(&out).count() as u32;
        if n > 0 {
            hits += n;
            out = re.replace_all(&out, "[redacted]").into_owned();
        }
    }
    let model_flag = rt.and_then(|rt| rt.classify(&out, &["pii", "clean"]).ok()).and_then(|v| v.get("label").and_then(Value::as_str).map(|l| l == "pii"));
    PiiScan { redacted: out, regex_hits: hits, model_flag }
}

//! Router R3a: plumbing for an on-device model (§13). It is **off** by
//! default. `localModel` in `app.json` is `false` until you turn on Settings →
//! Cabin defaults → "On-device model for background tasks". The route goes
//! live only while that toggle is on **and** a runtime is [`installed`]
//! (llama-server binary + GGUF, or a test fake). While the route is off the
//! registry has no `local:*` rows ([`LocalSource`] lists nothing) and
//! [`super::Router::choose`] drops any `local:*` id, so no route can pick one.
//!
//! When it is on (tests switch it on with a fake [`LocalRuntime`]):
//! - a `local:<tier>` route has cost class `included`, no effort menu and a
//!   context cap of [`LOCAL_CTX_CAP`];
//! - it is eligible only for [`LOCAL_CLASSES`] (`background:` classify, triage,
//!   redact, summarize) and as the difficulty estimator, never for
//!   `prepare:hard`, a user-facing class or a step offering a send, delete or
//!   credential tool;
//! - with sensitive data and no cloud grant it is the only route ([`privacy_only`]);
//! - the downshift ladder (battery, low-end device) only changes which tier is live;
//! - PII is found by regex first, the model second ([`scan_pii`]).
//!
//! A local call never leaves the device: [`serve`] calls the runtime directly
//! and writes no egress line.
//!
//! Production runtime (offline card PR A): a llama.cpp `llama-server` sidecar
//! on 127.0.0.1 ([`super::local_sidecar`]) behind [`super::local_http::HttpLocalRuntime`].
//! Assets come from `GROKHUB_LLAMA_SERVER` / `GROKHUB_LOCAL_MODEL`, or
//! `{GROKHUB_CONFIG}/bin` plus a GGUF from `~/GrokHub/models` (`installed.json`)
//! or `{GROKHUB_CONFIG}/models`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use regex::Regex;
use serde_json::Value;

use grokhub_core::local_governor::Level;
use grokhub_core::model_registry::{CatalogSource, Listing, ModelMeta, Prices, SourceKind};

use super::ladder::PREPARE_HARD;
use super::policy::class_row;
use crate::client::{CancelToken, ClientError, TurnOutput, Usage};

pub const PROVIDER_LOCAL: &str = "local";
pub const LOCAL_PREFIX: &str = "local:";
/// Floor for the on-device context cap (raised per device by [`set_ctx_cap`] / PR C).
pub const LOCAL_CTX_CAP: u64 = 4_096;

static CTX_CAP: AtomicU64 = AtomicU64::new(LOCAL_CTX_CAP);

/// Live context cap for eligibility and registry metadata (at least [`LOCAL_CTX_CAP`]).
pub fn ctx_cap() -> u64 {
    CTX_CAP.load(Ordering::SeqCst).max(LOCAL_CTX_CAP)
}

/// Raise (or reset) the on-device context cap after a hardware probe.
pub fn set_ctx_cap(cap: u64) {
    CTX_CAP.store(cap.max(LOCAL_CTX_CAP), Ordering::SeqCst);
}
/// The classes a local route may take while online (background helpers).
pub const LOCAL_CLASSES: &[&str] = &[
    "background:classify",
    "background:triage",
    "background:redact",
    "background:summarize",
];
/// Extra classes allowed on-device while offline (chat + summaries; not cloud-only).
pub const OFFLINE_CLASSES: &[&str] = &[
    "background:classify",
    "background:triage",
    "background:redact",
    "background:summarize",
    "chat:quick",
    "chat:default",
    "desktop:soft",
];
/// Tiers, strongest first. The downshift ladder moves down this list.
pub const TIERS: &[&str] = &["local:small", "local:tiny"];
/// What a local route returns while no runtime is installed.
pub const NO_RUNTIME_MSG: &str = "The on-device model isn't installed.";

static ENABLED: AtomicBool = AtomicBool::new(false);
static TIER: AtomicUsize = AtomicUsize::new(0);

/// The cabin sets this from `app.json` `localModel` (default `false`) every
/// tick, so the Settings toggle takes effect without a restart.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::SeqCst);
    if !on {
        // Off: nothing falls back to the device, so drop the chip and keep-warm.
        note_fallback(false);
    }
}

/// The Settings toggle alone, for the governor's sampler.
pub fn switched_on() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// [`switched_on`], or forced on for this test thread.
pub(crate) fn toggle_on() -> bool {
    switched_on() || test_on()
}

/// The route is live: the toggle is on, a runtime is [`installed`], and the
/// governor isn't at Unload.
pub fn available() -> bool {
    LocalCapacity::now().available()
}

/// What the device can take right now, for [`super::Router::choose`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalCapacity {
    /// The toggle is on and a runtime is installed (whatever the governor says).
    pub ready: bool,
    pub level: Level,
}

impl Default for LocalCapacity {
    fn default() -> Self {
        Self {
            ready: false,
            level: Level::Normal,
        }
    }
}

impl LocalCapacity {
    pub fn now() -> Self {
        Self {
            ready: (ENABLED.load(Ordering::SeqCst) && installed()) || test_on(),
            level: super::local_gov::level(),
        }
    }

    pub fn available(self) -> bool {
        self.ready && self.level != Level::Unload
    }

    /// Ready, but the governor paused it (a game, low battery, heat).
    pub fn paused(self) -> bool {
        self.ready && self.level == Level::Unload
    }

    /// Online background work goes local only in RampUp or Normal.
    pub fn background_ok(self) -> bool {
        matches!(self.level, Level::RampUp | Level::Normal)
    }
}

/// The installed model's hardware tier (`T1`…`T5`), when it's a catalog model.
pub fn hw_tier(config_dir: &std::path::Path) -> Option<&'static str> {
    let paths = super::local_sidecar::resolve_paths(config_dir)?;
    super::local_autoconfig::catalog_model_for(&paths.model).map(|m| m.tier)
}

/// `/why local`: whether the on-device model can take work, the governor's
/// level and reason, the hardware tier, the heavy-step queue, and the log.
pub fn why_local_text(config_dir: &std::path::Path) -> String {
    let cap = LocalCapacity::now();
    let gov = super::local_gov::level();
    let state = if !switched_on() {
        "off (Settings → Local AI)".to_string()
    } else if !cap.ready {
        "on, but no engine and model are installed".to_string()
    } else if cap.paused() {
        format!("{LOCAL_PAUSED_MSG} ({})", super::local_gov::reason())
    } else {
        "ready".to_string()
    };
    let takes = match gov {
        Level::Unload => "nothing until the machine frees up",
        Level::Throttle => "offline work and cloud outages only; background work goes to Grok",
        Level::Normal => {
            "background work, offline work and cloud outages; heavy steps wait for idle time"
        }
        Level::RampUp => "background work (heavy steps too), offline work and cloud outages",
    };
    [
        format!("On-device model: {state}."),
        format!(
            "Governor: {} because {}. Local takes {takes}.",
            gov.label(),
            super::local_gov::reason()
        ),
        format!(
            "Tier: {} on {}.",
            live_tier(),
            hw_tier(config_dir).unwrap_or("an unlisted model")
        ),
        format!(
            "Heavy background steps waiting for idle time: {}.",
            super::local_gov::queued()
        ),
        format!(
            "Log: {} (this device only, 7 days, safe to delete).",
            config_dir
                .join("local")
                .join(super::local_gov::LOG_FILE)
                .display()
        ),
    ]
    .join("\n")
}

/// The chip and pause message while the governor holds the local route.
pub const LOCAL_PAUSED_MSG: &str = "Local paused: game or low power";

/// The chip that leads an offline turn the on-device model can't take.
pub const OFFLINE_NO_LOCAL_MSG: &str = "Offline, local model unavailable";

/// Offline, and the on-device model would take this step if it were there.
pub fn offline_unavailable(gate: &LocalGate, class: &str, ctx_tokens: u64) -> bool {
    let running = LocalGate {
        enabled: true,
        ..gate.clone()
    };
    gate.offline && !gate.capacity.ready && eligible(&running, class, ctx_tokens)
}

/// Why the on-device model can't answer: off, runtime missing, no model, or
/// the last start error.
pub fn unavailable_reason() -> String {
    use super::local_sidecar::Runtime;
    if !switched_on() {
        return "Local AI is off in Settings".into();
    }
    match config_dir().map(|d| super::local_sidecar::runtime(&d)) {
        Some(Runtime::Missing) | None => "runtime missing".into(),
        Some(Runtime::NoModel) => "no model downloaded".into(),
        Some(Runtime::Error(e)) => e,
        Some(Runtime::Loading) => "still loading".into(),
        Some(Runtime::Ready | Runtime::Running) => {
            bootstrap_error().unwrap_or_else(|| "it isn't running".into())
        }
    }
}

/// The chat line for an offline turn that waits for the network.
pub fn offline_pause_text(reason: &str) -> String {
    format!(
        "{OFFLINE_NO_LOCAL_MSG}: {reason}. {product} keeps your message and answers it once you're back online.", product = grokhub_core::brand::product_name()
    )
}

/// A background step this long (over half the context cap) is heavy: it
/// goes local only in RampUp and otherwise waits for it.
pub fn heavy(class: &str, ctx_tokens: u64) -> bool {
    let class = class_row(class).map(|r| r.class).unwrap_or(class.trim());
    class.starts_with("background:") && ctx_tokens > ctx_cap() / 2
}

/// The longest a heavy background step waits for RampUp before it runs anyway.
pub const HEAVY_WAIT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Last `ensure_global` failure when production assets are present (cleared on success).
pub fn bootstrap_error() -> Option<String> {
    bootstrap_slot().lock().ok().and_then(|g| g.clone())
}

fn bootstrap_slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn set_bootstrap_error(err: Option<String>) {
    if let Ok(mut slot) = bootstrap_slot().lock() {
        *slot = err;
    }
}

/// An on-device runtime and model are present: a test fake, or a binary + GGUF
/// under the config dir / env overrides (not when bootstrap failed).
pub fn installed() -> bool {
    if test_runtime().is_some() {
        return true;
    }
    if !production_assets_present() {
        return false;
    }
    bootstrap_error().is_none()
}

fn production_assets_present() -> bool {
    config_dir()
        .map(|d| super::local_sidecar::assets_present(&d))
        .unwrap_or(false)
}

/// The cabin's config folder: `GROKHUB_CONFIG`, else the platform default.
/// Reading only the env var left the route off in every normal launch.
fn config_dir() -> Option<PathBuf> {
    Some(crate::perm::config_dir())
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
    TIER.store(
        TIERS.iter().position(|x| *x == t).unwrap_or(0),
        Ordering::SeqCst,
    );
}

pub fn live_tier() -> &'static str {
    TIERS[TIER.load(Ordering::SeqCst).min(TIERS.len() - 1)]
}

/// Capability snapshot from the last local bench / eval (PR E).
#[derive(Debug, Clone, PartialEq)]
pub struct TierCaps {
    pub context: u64,
    /// Fraction of the local eval set that passed (0.0–1.0). Not function-calling.
    pub eval_pass_rate: f64,
    pub tokens_per_sec: Option<f64>,
    pub first_token_ms: Option<u64>,
}

impl Default for TierCaps {
    fn default() -> Self {
        Self {
            context: LOCAL_CTX_CAP,
            eval_pass_rate: 0.0,
            tokens_per_sec: None,
            first_token_ms: None,
        }
    }
}

fn caps_slot() -> &'static Mutex<TierCaps> {
    static S: OnceLock<Mutex<TierCaps>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(TierCaps::default()))
}

/// Store bench/eval numbers so [`tier_meta`] and the router can use them.
pub fn set_caps(caps: TierCaps) {
    if caps.context > 0 {
        set_ctx_cap(caps.context);
    }
    *caps_slot().lock().expect("local caps") = caps;
}

/// Latest capability snapshot (defaults until a bench runs).
pub fn caps() -> TierCaps {
    caps_slot().lock().expect("local caps").clone()
}

/// Remember that the last on-device answer failed VerifyGate (escalate next online turn).
static LOCAL_VERIFY_FAILED: AtomicBool = AtomicBool::new(false);

/// Cabin / live path: set after a local turn's verify outcome.
pub fn note_local_verify_failed(failed: bool) {
    LOCAL_VERIFY_FAILED.store(failed, Ordering::SeqCst);
}

pub fn local_verify_failed() -> bool {
    LOCAL_VERIFY_FAILED.load(Ordering::SeqCst)
}

static FALLBACK_ACTIVE: AtomicBool = AtomicBool::new(false);

/// What the cabin shows while online calls fall back to the device.
pub const LOCAL_FALLBACK_MSG: &str = "Using local: no cloud model is answering";

/// Live path: the last routed call went to the device because no cloud model could take it.
///
/// A turn that started before Local AI was turned off can finish after it; its
/// `true` is dropped then, so the chip and keep-warm stay off.
pub fn note_fallback(active: bool) {
    FALLBACK_ACTIVE.store(active && toggle_on(), Ordering::SeqCst);
}

pub fn fallback_active() -> bool {
    FALLBACK_ACTIVE.load(Ordering::SeqCst) && toggle_on()
}

/// The model should be loaded ahead of time: offline, or online with no cloud
/// model answering. Online with the cloud up, it loads on demand only.
pub fn warm_wanted() -> bool {
    super::connectivity::sync_from_core();
    super::connectivity::state().prefer_local() || fallback_active()
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
    /// Connectivity is offline (PR B): chat may go local too.
    pub offline: bool,
    /// Offline only is on (LU-13): no step may go to the cloud at all.
    pub offline_switch: bool,
    /// Last on-device answer failed a verify check (PR E): escalate to cloud when online.
    pub local_verify_failed: bool,
    /// The governor's level and whether the runtime is there at all.
    pub capacity: LocalCapacity,
}

impl LocalGate {
    /// The flag and the live tier as set now.
    pub fn now(sensitive: bool, cloud_grant: bool, hard_tool: bool) -> Self {
        let capacity = LocalCapacity::now();
        Self {
            enabled: capacity.available(),
            capacity,
            tier: live_tier().into(),
            sensitive,
            cloud_grant,
            hard_tool,
            offline: super::connectivity::state().prefer_local(),
            offline_switch: grokhub_core::offline::enabled(),
            local_verify_failed: local_verify_failed(),
        }
    }
}

fn allowed_classes(gate: &LocalGate) -> &'static [&'static str] {
    if gate.offline {
        OFFLINE_CLASSES
    } else {
        LOCAL_CLASSES
    }
}

/// The local route may take this step: the flag is on, the class is allowed
/// for this connectivity, the step fits the context cap, and it is not hard.
pub fn eligible(gate: &LocalGate, class: &str, ctx_tokens: u64) -> bool {
    let class = class_row(class).map(|r| r.class).unwrap_or(class.trim());
    if super::connectivity::is_cloud_only(class) {
        return false;
    }
    gate.enabled
        && !gate.hard_tool
        && class != PREPARE_HARD
        && allowed_classes(gate).contains(&class)
        && ctx_tokens <= ctx_cap()
}

/// `id` may serve this step: the live tier, and [`eligible`].
pub fn admits(gate: &LocalGate, id: &str, class: &str, ctx_tokens: u64) -> bool {
    is_local(id) && id.trim() == gate.tier && eligible(gate, class, ctx_tokens)
}

/// The class and context would go local if the governor weren't at Unload.
pub fn eligible_if_running(gate: &LocalGate, class: &str, ctx_tokens: u64) -> bool {
    let running = LocalGate {
        enabled: gate.capacity.ready,
        ..gate.clone()
    };
    eligible(&running, class, ctx_tokens)
}

/// Sensitive data with no cloud grant: the local route is the only route.
pub fn privacy_only(gate: &LocalGate) -> bool {
    gate.enabled && gate.sensitive && !gate.cloud_grant
}

/// Offline with a live on-device runtime: prefer local for eligible classes.
pub fn offline_only(gate: &LocalGate) -> bool {
    gate.enabled && gate.offline
}

/// Online after a failed on-device verify: leave local and pick a cloud model.
pub fn escalate_after_verify(gate: &LocalGate) -> bool {
    gate.enabled && !gate.offline && !gate.offline_switch && gate.local_verify_failed
}

/// The on-device model could take chat with Offline only on: Local AI is on
/// and an engine and model are installed. `false` ⇒ the toggle should warn
/// (with [`unavailable_reason`]) before it turns the switch on.
pub fn ready_for_offline() -> bool {
    LocalCapacity::now().ready
}

/// Hard or long work should stay on the cloud when online (PR E).
pub fn too_hard_for_local(class: &str, ctx_tokens: u64, d: f64) -> bool {
    let class = class_row(class).map(|r| r.class).unwrap_or(class.trim());
    class == PREPARE_HARD
        || class.starts_with("repair:")
        || d > 0.7
        || ctx_tokens > ctx_cap().saturating_mul(3) / 4
}

/// Quiet note when escalating from local to cloud (names both models).
pub fn escalate_note(local_model: &str, cloud_model: &str) -> String {
    format!("Switched from {local_model} (local) to {cloud_model}: the on-device answer failed a check.")
}

/// The registry source for local tiers. Lists nothing (a no-op) while the flag is off.
pub struct LocalSource;

pub fn tier_meta(id: &str) -> ModelMeta {
    let c = caps();
    let mut notice = None;
    if let Some(tps) = c.tokens_per_sec {
        let ft = c.first_token_ms.unwrap_or(0);
        notice = Some(grokhub_core::model_registry::Notice {
            severity: "info".into(),
            text: format!(
                "local bench ≈ {tps:.1} tok/s, first token {ft} ms, eval {:.0}%",
                c.eval_pass_rate * 100.0
            ),
        });
    }
    ModelMeta {
        id: id.into(),
        context_length: Some(ctx_cap().max(c.context)),
        // No effort menu: the router picks no effort for a local route. The
        // sidecar maps the turn's own effort onto the levels the model's
        // template renders (`local_effort`).
        efforts: Some(Vec::new()),
        // Chat turns can return OpenAI-compatible tool_calls (web_search / web_fetch).
        tool_calling: Some(true),
        prices: Prices {
            prompt: Some(0),
            cached: Some(0),
            completion: Some(0),
            ..Prices::default()
        },
        notice,
        ..ModelMeta::default()
    }
}

impl CatalogSource for LocalSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Local
    }

    fn fetch(&self) -> Result<Listing, String> {
        if !available() {
            return Err("the local model is off".into());
        }
        Ok(Listing::new(
            SourceKind::Local,
            TIERS.iter().map(|t| tier_meta(t)).collect(),
        ))
    }
}

/// One chat message for [`LocalRuntime::chat_stream`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// An on-device model. Background helpers return JSON; chat may stream text.
pub trait LocalRuntime: Send + Sync {
    /// `{"label": "<one of labels>", "score": 0.0..1.0}`.
    fn classify(&self, text: &str, labels: &[&str]) -> Result<Value, String>;
    /// `{"summary": "..."}`.
    fn summarize(&self, text: &str, max_words: u32) -> Result<Value, String>;
    /// `{"d": 0.0..1.0}`.
    fn difficulty(&self, text: &str) -> Result<Value, String>;
    /// OpenAI-style chat with optional streaming deltas. Default: unsupported.
    fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let _ = (messages, on_delta);
        Err("local chat is not available on this runtime".into())
    }

    /// [`Self::chat_stream`] at a reasoning effort from [`EFFORT_LADDER`]
    /// (`None`: the template's default). A runtime that can't say how hard
    /// the model thinks ignores it.
    ///
    /// [`EFFORT_LADDER`]: grokhub_core::model_registry::EFFORT_LADDER
    fn chat_stream_at(
        &self,
        messages: &[ChatMessage],
        effort: Option<&str>,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let _ = effort;
        self.chat_stream(messages, on_delta)
    }

    /// Chat with function tools (cabin web_search / web_fetch). Default falls
    /// back to plain [`chat_stream`] and returns no tool calls.
    fn chat_tools(
        &self,
        req: &crate::client::ResponsesRequest,
        cancel: &CancelToken,
    ) -> Result<TurnOutput, String> {
        if cancel.is_cancelled() {
            return Err("cancelled".into());
        }
        let (text, _) = super::live::last_user_text(&req.input);
        let mut deltas = String::new();
        let out = self.chat_stream_at(
            &[ChatMessage {
                role: "user".into(),
                content: text,
            }],
            req.effort.as_deref(),
            &mut |d| deltas.push_str(d),
        )?;
        let text = if out.is_empty() { deltas } else { out };
        Ok(TurnOutput {
            text,
            ..Default::default()
        })
    }
}

/// The installed runtime: test fake, else HTTP client to a loopback sidecar.
pub fn runtime() -> Option<Arc<dyn LocalRuntime>> {
    if !available() {
        return None;
    }
    if let Some(rt) = test_runtime() {
        return Some(rt);
    }
    let busy = ensure_production_runtime()?;
    super::local_http::from_global_sidecar(busy)
}

/// Start the sidecar if needed; the guard marks this caller's request in
/// flight for as long as it holds the runtime.
fn ensure_production_runtime() -> Option<super::local_sidecar::Busy> {
    let dir = config_dir()?;
    match super::local_sidecar::acquire_for_request(&dir) {
        Ok(busy) => {
            set_bootstrap_error(None);
            Some(busy)
        }
        // Turned off mid-request, a governor pause or the crash breaker isn't
        // a broken install.
        Err(_)
            if !toggle_on()
                || super::local_sidecar::stop_pending()
                || super::local_gov::paused()
                || super::local_gov::may_start(super::local_sidecar::unix_ms()).is_err() =>
        {
            None
        }
        Err(e) => {
            set_bootstrap_error(Some(e));
            None
        }
    }
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

/// Tests only: a fake runtime installed for this thread, with the flag left to
/// [`set_enabled`]. Dropping it uninstalls the runtime.
#[cfg(test)]
pub(crate) struct Installed;

#[cfg(test)]
impl Installed {
    pub(crate) fn with(rt: Arc<dyn LocalRuntime>) -> Self {
        TEST_RT.with(|r| *r.borrow_mut() = Some(rt));
        Self
    }
}

#[cfg(test)]
impl Drop for Installed {
    fn drop(&mut self) {
        TEST_RT.with(|r| *r.borrow_mut() = None);
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

/// Answer one local call on the device. No network, no egress line. A heavy
/// background step first waits (up to [`HEAVY_WAIT`]) for RampUp; Stop ends the wait.
pub fn serve(class: &str, text: &str, cancel: &CancelToken) -> Result<TurnOutput, ClientError> {
    serve_within(class, text, HEAVY_WAIT, cancel)
}

/// Chat path that keeps function tools (cabin `web_search` / `web_fetch`) so the
/// agent loop can run them and round-trip results. Background classes stay
/// text-only via [`serve`].
pub fn serve_request(
    class: &str,
    req: &crate::client::ResponsesRequest,
    cancel: &CancelToken,
) -> Result<TurnOutput, ClientError> {
    let (text, _) = super::live::last_user_text(&req.input);
    let class_norm = class_row(class).map(|r| r.class).unwrap_or(class);
    if !matches!(class_norm, "chat:quick" | "chat:default" | "desktop:soft") {
        return serve(class, &text, cancel);
    }
    let tokens = (text.len() as u64).div_ceil(4);
    if heavy(class_norm, tokens) && super::local_gov::level() != Level::RampUp {
        match super::local_gov::wait_for_ramp_up(HEAVY_WAIT, &|| cancel.is_cancelled()) {
            None => return Err(ClientError::Cancelled),
            Some(Level::Unload) => {
                return Err(ClientError::Protocol(LOCAL_PAUSED_MSG.into()));
            }
            Some(_) => {}
        }
    }
    let Some(rt) = runtime() else {
        return Err(ClientError::Protocol(NO_RUNTIME_MSG.into()));
    };
    if req.tools.is_empty() {
        return serve_within_at(class, &text, req.effort.as_deref(), HEAVY_WAIT, cancel);
    }
    rt.chat_tools(req, cancel).map_err(ClientError::Protocol)
}

pub(crate) fn serve_within(
    class: &str,
    text: &str,
    heavy_wait: std::time::Duration,
    cancel: &CancelToken,
) -> Result<TurnOutput, ClientError> {
    serve_within_at(class, text, None, heavy_wait, cancel)
}

/// [`serve_within`] with the turn's reasoning effort, which only chat uses.
fn serve_within_at(
    class: &str,
    text: &str,
    effort: Option<&str>,
    heavy_wait: std::time::Duration,
    cancel: &CancelToken,
) -> Result<TurnOutput, ClientError> {
    let tokens = (text.len() as u64).div_ceil(4);
    if heavy(class, tokens) && super::local_gov::level() != Level::RampUp {
        match super::local_gov::wait_for_ramp_up(heavy_wait, &|| cancel.is_cancelled()) {
            None => return Err(ClientError::Cancelled),
            Some(Level::Unload) => {
                return Err(ClientError::Protocol(LOCAL_PAUSED_MSG.into()));
            }
            Some(_) => {}
        }
    }
    let Some(rt) = runtime() else {
        return Err(ClientError::Protocol(NO_RUNTIME_MSG.into()));
    };
    let class = class_row(class).map(|r| r.class).unwrap_or(class);
    if matches!(class, "chat:quick" | "chat:default" | "desktop:soft") {
        let mut deltas = String::new();
        let out = rt
            .chat_stream_at(
                &[ChatMessage {
                    role: "user".into(),
                    content: text.into(),
                }],
                effort,
                &mut |d| deltas.push_str(d),
            )
            .map_err(ClientError::Protocol)?;
        let text = if out.is_empty() { deltas } else { out };
        return Ok(TurnOutput {
            text,
            reasoning: String::new(),
            calls: Vec::new(),
            usage: Usage::default(),
            ..Default::default()
        });
    }
    let answer = if class == "background:summarize" {
        rt.summarize(text, 60)
    } else {
        rt.classify(text, CLASS_LABELS)
    };
    let v = answer.map_err(ClientError::Protocol)?;
    Ok(TurnOutput {
        text: v.to_string(),
        reasoning: String::new(),
        calls: Vec::new(),
        usage: Usage::default(),
        ..Default::default()
    })
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
        [
            r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}",
            r"\+?\d[\d\s().-]{7,}\d",
        ]
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
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
    let model_flag = rt
        .and_then(|rt| rt.classify(&out, &["pii", "clean"]).ok())
        .and_then(|v| v.get("label").and_then(Value::as_str).map(|l| l == "pii"));
    PiiScan {
        redacted: out,
        regex_hits: hits,
        model_flag,
    }
}

//! One model's row in the registry: what its sources say (metadata), where it
//! was seen, and its state. Unknown fields stay `None`; only a row no xAI API
//! listing describes takes the known prices and window from [`super::seed`].

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Where a listing came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// `GET /v1/models` + `/v1/language-models` with the user's own key or sign-in.
    XaiApi,
    /// `grok models` in the cabin Grok home, scoped to the signed-in plan.
    GrokBuild,
    /// Grok Build config in the cabin Grok home, read only.
    GbConfig,
    /// The on-device model (Router R3a plumbing). Lists nothing while `localModel` is off.
    Local,
    /// An OpenAI-compatible `/models` (OpenRouter and the like) you added a key for (Router R3b).
    OpenAiCompatible,
    /// Anthropic's `/v1/models` with a key you added (Router R3b).
    Anthropic,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::XaiApi => "xai_api",
            Self::GrokBuild => "grok_build",
            Self::GbConfig => "gb_config",
            Self::Local => "local",
            Self::OpenAiCompatible => "openai_compatible",
            Self::Anthropic => "anthropic",
        }
    }

    /// A provider you added with your own key: cost class `new_provider`.
    pub fn is_new_provider(self) -> bool {
        matches!(self, Self::OpenAiCompatible | Self::Anthropic)
    }
}

/// A model's state. Health is passive: only real traffic and fresh listings move it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelState {
    /// Its onboarding probe is running.
    Probing,
    #[default]
    Live,
    /// Over 20% errors in the last 10 calls, p95 over twice its 7-day baseline, or a 429/503 burst.
    Degraded,
    /// Held out of routing by the breaker (3 hard failures in a row or a ghost strike) until its half-open probe passes.
    Quarantined,
    /// Not in your own fresh list, or a 403 "requires a Grok subscription". Never a ghost strike.
    NotInPlan,
    /// It answers as another model, or an unlisted slug still answers.
    Redirected,
    /// Listed but gone: 404 twice in 24 h, a placeholder row, a named id no source lists, or a critical "Retired" notice.
    Ghost,
    /// A source flags it deprecated (still answers; routing skips it).
    Retired,
    /// Gone from every source for [`super::PRUNE_AFTER_MS`].
    Pruned,
}

impl ModelState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Probing => "probing",
            Self::Live => "live",
            Self::Degraded => "degraded",
            Self::Quarantined => "quarantined",
            Self::NotInPlan => "not_in_plan",
            Self::Redirected => "redirected",
            Self::Ghost => "ghost",
            Self::Retired => "retired",
            Self::Pruned => "pruned",
        }
    }

    /// States a route may still pick (shadow `choose` filters on this).
    pub fn routable(self) -> bool {
        matches!(self, Self::Live | Self::Degraded | Self::Probing)
    }

    /// Retired or pruned: out of routing for good, kept as a tombstone for
    /// [`super::PRUNE_AFTER_MS`], then deleted from `registry.json`.
    pub fn tombstone(self) -> bool {
        matches!(self, Self::Retired | Self::Pruned)
    }
}

/// List prices in USD cents per 100M tokens, as xAI reports them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prices {
    #[serde(default)]
    pub prompt: Option<u64>,
    #[serde(default)]
    pub cached: Option<u64>,
    /// Writing the prompt into the provider's cache (Anthropic bills it
    /// above the prompt rate). `None` where no listing or table names one:
    /// spend then prices writes at the prompt rate.
    #[serde(default)]
    pub cache_write: Option<u64>,
    #[serde(default)]
    pub completion: Option<u64>,
    #[serde(default)]
    pub prompt_long: Option<u64>,
    #[serde(default)]
    pub completion_long: Option<u64>,
    #[serde(default)]
    pub long_context_threshold: Option<u64>,
}

impl Prices {
    pub fn is_empty(&self) -> bool {
        self.prompt.is_none() && self.cached.is_none() && self.completion.is_none()
    }

    /// A prompt `prompt_tokens` long is at or past the long-context threshold.
    fn long_at(&self, prompt_tokens: u64) -> bool {
        self.long_context_threshold
            .is_some_and(|t| t > 0 && prompt_tokens >= t)
    }

    /// The completion rate for a request whose prompt is `prompt_tokens`
    /// long: the long rate at or past the threshold. Needs no prompt price.
    pub fn completion_at(&self, prompt_tokens: u64) -> Option<u64> {
        if self.long_at(prompt_tokens) {
            self.completion_long.or(self.completion)
        } else {
            self.completion
        }
    }

    /// (prompt, cached, completion) rates for a request whose prompt is
    /// `prompt_tokens` long. At or past the long-context threshold every token
    /// pays the long rate, cached ones too (scaled like the prompt). `None`
    /// when the prompt or completion price is unknown.
    pub fn rates_at(&self, prompt_tokens: u64) -> Option<(u64, u64, u64)> {
        let (short, prompt, completion) = if self.long_at(prompt_tokens) {
            (
                self.prompt?,
                self.prompt_long.or(self.prompt)?,
                self.completion_long.or(self.completion)?,
            )
        } else {
            (self.prompt?, self.prompt?, self.completion?)
        };
        let cached = match self.cached {
            Some(c) if short > 0 => c * prompt / short,
            Some(c) => c,
            None => prompt,
        };
        Some((prompt, cached, completion))
    }

    /// The cache-write rate for a request whose prompt is `prompt_tokens`
    /// long, scaled at the long step like the cached rate in
    /// [`Prices::rates_at`]. `None` when no write rate is known.
    pub fn cache_write_at(&self, prompt_tokens: u64) -> Option<u64> {
        let write = self.cache_write?;
        if self.long_at(prompt_tokens) {
            match (self.prompt, self.prompt_long) {
                (Some(short), Some(long)) if short > 0 => Some(write * long / short),
                _ => Some(write),
            }
        } else {
            Some(write)
        }
    }
}

/// A Grok Build notice on a model (`critical` "Retired", or a warning).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub severity: String,
    pub text: String,
}

impl Notice {
    pub fn is_critical_retired(&self) -> bool {
        self.severity.eq_ignore_ascii_case("critical")
            && self.text.to_ascii_lowercase().contains("retired")
    }
}

/// How a model is reached: the route a call to it travels. Derived from
/// the model's sources and id at refresh (see [`route_for`]); the scorer
/// weighs routes, not just models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    /// xAI direct: the signed-in plan pool or the user's own xAI key.
    Xai,
    /// An OpenRouter model on its free tier (`:free` or the free router).
    OpenRouterFree,
    /// An OpenRouter model that bills the account's credit.
    OpenRouterPaid,
    /// Anthropic direct, with the user's own API key.
    Anthropic,
    /// The on-device model.
    Local,
}

impl Route {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xai => "xai",
            Self::OpenRouterFree => "openrouter-free",
            Self::OpenRouterPaid => "openrouter-paid",
            Self::Anthropic => "anthropic",
            Self::Local => "local",
        }
    }
}

/// The route of a model with these sources: a provider you added decides
/// it (Anthropic direct, else the OpenAI-compatible host — free when
/// [`super::discover::is_free_model`] says so, paid otherwise), an
/// on-device row is local, and anything else is xAI direct.
pub fn route_for(id: &str, sources: &[SourceKind]) -> Route {
    if sources.contains(&SourceKind::Anthropic) {
        Route::Anthropic
    } else if sources.contains(&SourceKind::OpenAiCompatible) {
        if super::discover::is_free_model(id) {
            Route::OpenRouterFree
        } else {
            Route::OpenRouterPaid
        }
    } else if sources.contains(&SourceKind::Local) {
        Route::Local
    } else {
        Route::Xai
    }
}

/// What a model is good at, 0–100 per axis. Day-one priors come from the
/// bundled table ([`super::card`]); live measurement replaces them as it
/// accrues. `None` where nothing is known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityScores {
    #[serde(default)]
    pub chat: Option<u8>,
    #[serde(default)]
    pub code: Option<u8>,
    #[serde(default)]
    pub tools: Option<u8>,
    #[serde(default)]
    pub reasoning: Option<u8>,
    #[serde(default)]
    pub long_context: Option<u8>,
    #[serde(default)]
    pub vision: Option<u8>,
    #[serde(default)]
    pub audio: Option<u8>,
}

impl CapabilityScores {
    /// Fill each unknown score from `other`. Known scores stay.
    pub fn merge_from(&mut self, other: &Self) {
        for (a, b) in [
            (&mut self.chat, other.chat),
            (&mut self.code, other.code),
            (&mut self.tools, other.tools),
            (&mut self.reasoning, other.reasoning),
            (&mut self.long_context, other.long_context),
            (&mut self.vision, other.vision),
            (&mut self.audio, other.audio),
        ] {
            if a.is_none() {
                *a = b;
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What the route does with prompts, as far as anyone has said. Sensitive
/// turns skip routes that train (see the router's data policy setting).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataPolicy {
    /// Prompts may train the provider's models.
    #[serde(default)]
    pub trains: Option<bool>,
    /// Prompts are kept after the call (any retention, e.g. 30 days).
    #[serde(default)]
    pub retains: Option<bool>,
    /// Prompts pass a moderation or human-review step.
    #[serde(default)]
    pub moderated: Option<bool>,
}

impl DataPolicy {
    /// Fill each unknown fact from `other`. Known facts stay.
    pub fn merge_from(&mut self, other: &Self) {
        for (a, b) in [
            (&mut self.trains, other.trains),
            (&mut self.retains, other.retains),
            (&mut self.moderated, other.moderated),
        ] {
            if a.is_none() {
                *a = b;
            }
        }
    }
}

/// The model card: everything the router knows about a model beyond its
/// listing — its route, what it is good at, what it is for, and the
/// priors the built-in profiles ([`super::builtin`]) ship for it. Listings
/// never send this; refresh fills the unknown fields from the table and
/// live measurement replaces priors as it accrues.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCard {
    #[serde(default)]
    pub route: Option<Route>,
    #[serde(default)]
    pub caps: CapabilityScores,
    /// What the model is for, as short tags (`long`, `free_code`, …).
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub data_policy: DataPolicy,
    /// The date (`YYYY-MM-DD`) the model is expected to leave, when one
    /// is announced.
    #[serde(default)]
    pub expires: Option<String>,
    /// How many hosts serve it, when a listing or the table says.
    #[serde(default)]
    pub hosts: Option<u32>,
    /// Still in its first-weeks evaluation: priors are provisional.
    #[serde(default)]
    pub probation: bool,
    /// Measured (or prior) output speed, tokens per second.
    #[serde(default)]
    pub speed_tps: Option<u64>,
    /// Measured (or prior) time to first token, milliseconds.
    #[serde(default)]
    pub ttft_ms: Option<u64>,
    /// Published intelligence index (0-100) from the built-in profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intelligence: Option<u8>,
    /// The profile's tier: `frontier`, `flagship`, `workhorse`, `fast`, `paid`, `free`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

impl ModelCard {
    pub fn is_empty(&self) -> bool {
        self.route.is_none()
            && self.caps.is_empty()
            && self.roles.is_empty()
            && self.data_policy == DataPolicy::default()
            && self.expires.is_none()
            && self.hosts.is_none()
            && !self.probation
            && self.speed_tps.is_none()
            && self.ttft_ms.is_none()
            && self.intelligence.is_none()
            && self.tier.is_none()
    }

    /// Fill each unknown field from `other` (a bundled prior). Known
    /// fields stay: a listing, a refresh or a measurement outranks the
    /// table.
    pub fn merge_from(&mut self, other: &Self) {
        if self.route.is_none() {
            self.route = other.route;
        }
        self.caps.merge_from(&other.caps);
        for role in &other.roles {
            if !self.roles.contains(role) {
                self.roles.push(role.clone());
            }
        }
        self.data_policy.merge_from(&other.data_policy);
        if self.expires.is_none() {
            self.expires.clone_from(&other.expires);
        }
        if self.hosts.is_none() {
            self.hosts = other.hosts;
        }
        self.probation = self.probation || other.probation;
        if self.speed_tps.is_none() {
            self.speed_tps = other.speed_tps;
        }
        if self.ttft_ms.is_none() {
            self.ttft_ms = other.ttft_ms;
        }
        if self.intelligence.is_none() {
            self.intelligence = other.intelligence;
        }
        if self.tier.is_none() {
            self.tier.clone_from(&other.tier);
        }
    }
}

/// How a model has actually answered lately: the passive health window
/// over the last 7 days. Rates are 0.0–1.0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reliability {
    pub calls: u32,
    pub success_rate: f64,
    /// The share of calls answered 429 or 503.
    pub pressure_rate: f64,
}

/// What a source says about one model. Every field a source didn't send is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMeta {
    pub id: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub context_length: Option<u64>,
    #[serde(default)]
    pub max_output: Option<u64>,
    #[serde(default)]
    pub prices: Prices,
    /// `capabilities.reasoning_effort[]`. `None` when the source didn't say.
    #[serde(default)]
    pub efforts: Option<Vec<String>>,
    #[serde(default)]
    pub default_effort: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub input_modalities: Option<Vec<String>>,
    #[serde(default)]
    pub output_modalities: Option<Vec<String>>,
    #[serde(default)]
    pub tool_calling: Option<bool>,
    /// The listing names `reasoning` among the request fields the model takes
    /// (OpenRouter's `supported_parameters`). `None` when it didn't say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub structured_output: Option<bool>,
    /// `responses` or `chat_completions`, when the listing names its endpoints.
    #[serde(default)]
    pub api_shape: Option<String>,
    #[serde(default)]
    pub deprecated: Option<bool>,
    #[serde(default)]
    pub notice: Option<Notice>,
    /// The model card: route, capability priors, roles and data policy.
    /// No listing sends it; [`super::card`] fills it at refresh. An
    /// untouched card writes nothing, so a row no refresh has filled
    /// serializes exactly as before.
    #[serde(default, skip_serializing_if = "ModelCard::is_empty")]
    pub card: ModelCard,
}

impl ModelMeta {
    pub fn bare(id: &str) -> Self {
        Self {
            id: id.to_string(),
            ..Self::default()
        }
    }

    /// No prices, no context and no efforts: a row with nothing behind it.
    pub fn is_placeholder(&self) -> bool {
        self.prices.is_empty()
            && self.context_length.is_none()
            && self.efforts.as_ref().is_none_or(|e| e.is_empty())
    }

    /// Sha-256 of the metadata, so a refresh can tell a real change from a reorder.
    pub fn content_hash(&self) -> String {
        let text = serde_json::to_string(self).unwrap_or_default();
        hex::encode(Sha256::digest(text.as_bytes()))
    }

    /// Fill this row's unknown fields from `other` (a second source). Known fields stay.
    pub fn merge_from(&mut self, other: &ModelMeta) {
        fn take<T: Clone>(a: &mut Option<T>, b: &Option<T>) {
            if a.is_none() {
                a.clone_from(b);
            }
        }
        for alias in &other.aliases {
            if !self.aliases.contains(alias) {
                self.aliases.push(alias.clone());
            }
        }
        take(&mut self.context_length, &other.context_length);
        take(&mut self.max_output, &other.max_output);
        take(&mut self.prices.prompt, &other.prices.prompt);
        take(&mut self.prices.cached, &other.prices.cached);
        take(&mut self.prices.cache_write, &other.prices.cache_write);
        take(&mut self.prices.completion, &other.prices.completion);
        take(&mut self.prices.prompt_long, &other.prices.prompt_long);
        take(
            &mut self.prices.completion_long,
            &other.prices.completion_long,
        );
        take(
            &mut self.prices.long_context_threshold,
            &other.prices.long_context_threshold,
        );
        take(&mut self.efforts, &other.efforts);
        take(&mut self.default_effort, &other.default_effort);
        take(&mut self.fingerprint, &other.fingerprint);
        take(&mut self.version, &other.version);
        take(&mut self.input_modalities, &other.input_modalities);
        take(&mut self.output_modalities, &other.output_modalities);
        take(&mut self.tool_calling, &other.tool_calling);
        take(&mut self.reasoning, &other.reasoning);
        take(&mut self.structured_output, &other.structured_output);
        take(&mut self.api_shape, &other.api_shape);
        take(&mut self.deprecated, &other.deprecated);
        take(&mut self.notice, &other.notice);
        self.card.merge_from(&other.card);
    }
}

/// One passive call result kept for health (no content).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallMark {
    pub ts_ms: u64,
    pub ok: bool,
    /// 429 or 503.
    #[serde(default)]
    pub pressure: bool,
    #[serde(default)]
    pub latency_ms: u64,
}

/// Recent calls and latency samples for the degraded rule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthWindow {
    /// The last [`super::health::WINDOW_CALLS`] calls.
    #[serde(default)]
    pub recent: VecDeque<CallMark>,
    /// Successful-call latencies from the last 7 days, capped.
    #[serde(default)]
    pub latencies: VecDeque<(u64, u64)>,
}

/// The breaker on one model (R2a): hard failures in a row, how often it has
/// opened, and when its open backoff runs out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breaker {
    /// 5xx answers and timeouts in a row.
    #[serde(default)]
    pub fails: u32,
    /// Opens since it last closed; picks the step on the backoff schedule.
    #[serde(default)]
    pub opens: u32,
    #[serde(default)]
    pub open_until_ms: u64,
    /// Its one half-open check (a probe or a low-risk background call) is out.
    #[serde(default)]
    pub half_open: bool,
}

/// One model in `registry.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRecord {
    pub meta: ModelMeta,
    #[serde(default)]
    pub sources: Vec<SourceKind>,
    #[serde(default)]
    pub state: ModelState,
    /// One plain sentence: why it is in this state.
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub content_hash: String,
    /// 404 `not-found` times (ms) while listed, last 24 h.
    #[serde(default)]
    pub not_found: Vec<u64>,
    #[serde(default)]
    pub health: HealthWindow,
    /// What the response said it was, when that differs from the id.
    #[serde(default)]
    pub served_as: Option<String>,
    #[serde(default)]
    pub first_seen_ms: u64,
    /// When it last left every source (0 while listed).
    #[serde(default)]
    pub absent_since_ms: u64,
    #[serde(default)]
    pub breaker: Breaker,
    /// When it became retired or pruned (0 otherwise).
    #[serde(default)]
    pub tombstone_ms: u64,
    /// Its two-week tryout, when it is or was a new model on one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probation: Option<super::probation::Probation>,
    /// The expiry date a heads-up was already logged for.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub expiry_noted: String,
}

/// Strip trailing date / snapshot / `latest` tokens so `grok-4.7`,
/// `grok-4.7-latest` and `grok-4.7-0414` share one stem.
pub fn model_id_stem(id: &str) -> String {
    let lower = id.trim().to_ascii_lowercase();
    let mut parts: Vec<&str> = lower.split('-').filter(|p| !p.is_empty()).collect();
    while let Some(last) = parts.last().copied() {
        if last == "latest" || (!last.is_empty() && last.bytes().all(|b| b.is_ascii_digit())) {
            parts.pop();
        } else {
            break;
        }
    }
    parts.join("-")
}

impl ModelRecord {
    /// The route a call to this model travels: the card's route once a
    /// refresh has stamped it, else derived from its sources on the spot.
    pub fn route(&self) -> Route {
        self.meta
            .card
            .route
            .unwrap_or_else(|| route_for(&self.meta.id, &self.sources))
    }

    /// How this model has answered over the last 7 days, from its passive
    /// health window (the last [`super::health::WINDOW_CALLS`] calls —
    /// the only call history the registry keeps). `None` when no call in
    /// the window is that fresh.
    /// Its tryout failed: out of routing until the retry.
    pub fn probation_failed(&self) -> bool {
        self.probation
            .as_ref()
            .is_some_and(|p| p.stage == super::probation::Stage::Failed)
    }

    pub fn reliability(&self, now_ms: u64) -> Option<Reliability> {
        let fresh: Vec<&CallMark> = self
            .health
            .recent
            .iter()
            .filter(|m| now_ms.saturating_sub(m.ts_ms) <= 7 * super::probe::DAY_MS)
            .collect();
        if fresh.is_empty() {
            return None;
        }
        let calls = fresh.len() as u32;
        let ok = fresh.iter().filter(|m| m.ok).count() as f64;
        let pressure = fresh.iter().filter(|m| m.pressure).count() as f64;
        Some(Reliability {
            calls,
            success_rate: ok / calls as f64,
            pressure_rate: pressure / calls as f64,
        })
    }

    /// True when the response's `model` is this row: the id itself, an alias,
    /// or a dated / `latest` form of either (so `grok-4.7` answering as
    /// `grok-4.7-0414` is not a redirect).
    pub fn answers_as(&self, served: &str) -> bool {
        let s = served.trim();
        if s.is_empty() || s == self.meta.id || self.meta.aliases.iter().any(|a| a == s) {
            return true;
        }
        let stem = model_id_stem(s);
        if stem.is_empty() {
            return false;
        }
        model_id_stem(&self.meta.id) == stem
            || self.meta.aliases.iter().any(|a| model_id_stem(a) == stem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, sources: &[SourceKind]) -> ModelRecord {
        ModelRecord {
            meta: ModelMeta::bare(id),
            sources: sources.to_vec(),
            ..ModelRecord::default()
        }
    }

    #[test]
    fn route_comes_from_the_sources_and_the_free_suffix() {
        assert_eq!(rec("grok-4.7", &[SourceKind::XaiApi]).route(), Route::Xai);
        assert_eq!(
            rec("grok-4.7", &[SourceKind::GrokBuild]).route(),
            Route::Xai
        );
        assert_eq!(
            rec("anthropic/claude-sonnet-5.5", &[SourceKind::Anthropic]).route(),
            Route::Anthropic
        );
        assert_eq!(
            rec("openrouter/vendor/x:free", &[SourceKind::OpenAiCompatible]).route(),
            Route::OpenRouterFree
        );
        assert_eq!(
            rec(
                "openrouter/openrouter/free",
                &[SourceKind::OpenAiCompatible]
            )
            .route(),
            Route::OpenRouterFree
        );
        assert_eq!(
            rec("openrouter/vendor/x", &[SourceKind::OpenAiCompatible]).route(),
            Route::OpenRouterPaid
        );
        assert_eq!(
            rec("local:tier", &[SourceKind::Local]).route(),
            Route::Local
        );
        // A stamped card route wins over the derivation.
        let mut stamped = rec("openrouter/vendor/x", &[SourceKind::OpenAiCompatible]);
        stamped.meta.card.route = Some(Route::Anthropic);
        assert_eq!(stamped.route(), Route::Anthropic);
    }

    #[test]
    fn reliability_reads_the_health_window_and_ages_out() {
        let now = 100 * super::super::probe::DAY_MS;
        let mut r = rec("grok-4.7", &[SourceKind::XaiApi]);
        assert_eq!(r.reliability(now), None);
        let mark = |ts_ms, ok, pressure| CallMark {
            ts_ms,
            ok,
            pressure,
            latency_ms: 0,
        };
        r.health.recent.push_back(mark(now - 10, true, false));
        r.health.recent.push_back(mark(now - 9, true, false));
        r.health.recent.push_back(mark(now - 8, false, true));
        r.health
            .recent
            .push_back(mark(now - 8 * super::super::probe::DAY_MS, false, true));
        let rel = r.reliability(now).unwrap();
        assert_eq!(rel.calls, 3, "the 8-day-old mark is out of the window");
        assert!((rel.success_rate - 2.0 / 3.0).abs() < 1e-9);
        assert!((rel.pressure_rate - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn cache_write_scales_at_the_long_step_and_stays_unknown_without_a_rate() {
        let p = Prices {
            prompt: Some(20_000),
            cache_write: Some(25_000),
            prompt_long: Some(40_000),
            long_context_threshold: Some(200_000),
            ..Prices::default()
        };
        assert_eq!(p.cache_write_at(199_999), Some(25_000));
        assert_eq!(p.cache_write_at(200_000), Some(50_000));
        assert_eq!(Prices::default().cache_write_at(0), None);
    }
}

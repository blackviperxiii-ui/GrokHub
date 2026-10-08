//! One model's row in the registry: what its sources say (metadata), where it
//! was seen, and its state. Unknown fields stay `None`; nothing is filled from code.

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
}

/// A Grok Build notice on a model (`critical` "Retired", or a warning).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub severity: String,
    pub text: String,
}

impl Notice {
    pub fn is_critical_retired(&self) -> bool {
        self.severity.eq_ignore_ascii_case("critical") && self.text.to_ascii_lowercase().contains("retired")
    }
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
    #[serde(default)]
    pub structured_output: Option<bool>,
    /// `responses` or `chat_completions`, when the listing names its endpoints.
    #[serde(default)]
    pub api_shape: Option<String>,
    #[serde(default)]
    pub deprecated: Option<bool>,
    #[serde(default)]
    pub notice: Option<Notice>,
}

impl ModelMeta {
    pub fn bare(id: &str) -> Self {
        Self { id: id.to_string(), ..Self::default() }
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
        take(&mut self.prices.completion, &other.prices.completion);
        take(&mut self.prices.prompt_long, &other.prices.prompt_long);
        take(&mut self.prices.completion_long, &other.prices.completion_long);
        take(&mut self.prices.long_context_threshold, &other.prices.long_context_threshold);
        take(&mut self.efforts, &other.efforts);
        take(&mut self.default_effort, &other.default_effort);
        take(&mut self.fingerprint, &other.fingerprint);
        take(&mut self.version, &other.version);
        take(&mut self.input_modalities, &other.input_modalities);
        take(&mut self.output_modalities, &other.output_modalities);
        take(&mut self.tool_calling, &other.tool_calling);
        take(&mut self.structured_output, &other.structured_output);
        take(&mut self.api_shape, &other.api_shape);
        take(&mut self.deprecated, &other.deprecated);
        take(&mut self.notice, &other.notice);
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
}

impl ModelRecord {
    pub fn answers_as(&self, served: &str) -> bool {
        let s = served.trim();
        s.is_empty() || s == self.meta.id || self.meta.aliases.iter().any(|a| a == s)
    }
}

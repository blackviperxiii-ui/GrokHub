//! `models/quota.json`: what each cloud route's account can still spend,
//! detected — never configured. OpenRouter answers for its own account
//! (`GET /api/v1/key`), xAI answers for blocked keys (`GET /v1/api-key`),
//! and Anthropic has no balance endpoint for normal keys, so its section
//! is built from the rate-limit headers on real replies plus a local
//! spend ledger priced from the registry's own price table.
//!
//! Detection runs on the registry refresh's cadence, at most every
//! [`QUOTA_POLL_MS`] per route, and again right after a 402, 403 or 429
//! on that route (the error paths stamp `recheck_at_ms`). Nothing here
//! routes anything: the scorer reads this ledger in a later step. Pure
//! state, parsing and arithmetic — the wire lives in the agent crate.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::probe::DAY_MS;
use super::record::Prices;

/// How often a route's account state is re-read when nothing went wrong.
pub const QUOTA_POLL_MS: u64 = 10 * 60 * 1000;
/// An OpenRouter paid account at or under this many USD of remaining
/// credit counts as running low.
pub const PAID_LOW_FLOOR_USD: f64 = 0.5;
/// An out-of-credit Claude key is benched, then re-checked after this.
pub const OUT_OF_CREDIT_RECHECK_MS: u64 = 60 * 60 * 1000;

// ---- OpenRouter ---------------------------------------------------------------

/// What the OpenRouter account can do, from its own `/key` answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenRouterState {
    /// Never checked, or the last answer said nothing usable.
    #[default]
    Unknown,
    /// Free tier: free models only. A paid call is refused.
    FreeOnly,
    /// Paid credit with room to spend.
    Paid,
    /// Paid, but the remaining credit is at or under the floor.
    PaidLow,
    /// Even free models are erroring; stands until a `/key` re-read
    /// shows the account answering again.
    Negative,
}

/// One route's OpenRouter account state, as `/api/v1/key` last reported it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenRouterQuota {
    pub state: OpenRouterState,
    #[serde(default)]
    pub is_free_tier: Option<bool>,
    /// The key's credit limit in USD, when the account sets one.
    #[serde(default)]
    pub limit_usd: Option<f64>,
    #[serde(default)]
    pub limit_remaining_usd: Option<f64>,
    #[serde(default)]
    pub usage_daily_usd: f64,
    #[serde(default)]
    pub usage_weekly_usd: f64,
    #[serde(default)]
    pub usage_monthly_usd: f64,
    #[serde(default)]
    pub free_daily_used: Option<u64>,
    /// The account's free-models-per-day ceiling, read straight from the
    /// answer (50 under 10 all-time credits, 1,000 at 10 or more). This —
    /// not `is_free_tier` — is the ceiling routing must respect: the flag
    /// and the ceiling are independent facts about the account.
    #[serde(default)]
    pub free_daily_limit: Option<u64>,
    #[serde(default)]
    pub free_daily_remaining: Option<u64>,
    #[serde(default)]
    pub checked_at_ms: u64,
    /// An error asked for a re-read at or after this time (0 = none).
    #[serde(default)]
    pub recheck_at_ms: u64,
}

fn f64_at(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn u64_at(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

/// The account state a `/key` answer proves, before any error history.
fn openrouter_state_of(
    is_free_tier: Option<bool>,
    limit_remaining: Option<f64>,
) -> OpenRouterState {
    match (is_free_tier, limit_remaining) {
        (Some(true), _) => OpenRouterState::FreeOnly,
        (_, Some(rem)) if rem <= PAID_LOW_FLOOR_USD => OpenRouterState::PaidLow,
        (Some(false), _) => OpenRouterState::Paid,
        (_, Some(_)) => OpenRouterState::Paid,
        (None, None) => OpenRouterState::Unknown,
    }
}

/// Parse one `GET /api/v1/key` answer (the payload rides under `data`).
pub fn parse_openrouter_key(v: &Value, now_ms: u64) -> OpenRouterQuota {
    let d = v.get("data").unwrap_or(v);
    let is_free_tier = d.get("is_free_tier").and_then(Value::as_bool);
    let limit_remaining = f64_at(d, "limit_remaining");
    let fmdr = d.get("free_model_daily_requests");
    OpenRouterQuota {
        state: openrouter_state_of(is_free_tier, limit_remaining),
        is_free_tier,
        limit_usd: f64_at(d, "limit"),
        limit_remaining_usd: limit_remaining,
        usage_daily_usd: f64_at(d, "usage_daily")
            .or_else(|| f64_at(d, "usage"))
            .unwrap_or(0.0),
        usage_weekly_usd: f64_at(d, "usage_weekly").unwrap_or(0.0),
        usage_monthly_usd: f64_at(d, "usage_monthly").unwrap_or(0.0),
        free_daily_used: fmdr.and_then(|f| u64_at(f, "used")),
        free_daily_limit: fmdr.and_then(|f| u64_at(f, "limit")),
        free_daily_remaining: fmdr.and_then(|f| u64_at(f, "remaining")),
        checked_at_ms: now_ms,
        recheck_at_ms: 0,
    }
}

impl OpenRouterQuota {
    /// Whether a call may go out on this route under the account state
    /// last detected, given the model's own route class (`free` from
    /// [`super::discover::is_free_model`]): free models run in every
    /// state but `Negative` (even free calls error there, until a
    /// re-read shows recovery) and only while the day's free ceiling
    /// lasts; paid models need paid credit (`Paid`, or `PaidLow` within
    /// its remaining credit). `Unknown` permits both: detection simply
    /// hasn't run yet, and an unchecked account is not a closed one.
    pub fn permits(&self, free: bool) -> bool {
        match self.state {
            OpenRouterState::Negative => false,
            OpenRouterState::FreeOnly => free && self.free_daily_remaining.is_none_or(|r| r > 0),
            OpenRouterState::PaidLow => free || self.limit_remaining_usd.is_none_or(|r| r > 0.0),
            OpenRouterState::Paid | OpenRouterState::Unknown => true,
        }
    }
}

// ---- xAI ----------------------------------------------------------------------

/// The xAI key's blocked flags, from `GET /v1/api-key`. The balance needs
/// a management key, so there is none here; plan accounts keep the
/// registry health fold's free-usage-exhausted signal.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct XaiQuota {
    #[serde(default)]
    pub team_blocked: Option<bool>,
    #[serde(default)]
    pub api_key_blocked: Option<bool>,
    #[serde(default)]
    pub api_key_disabled: Option<bool>,
    #[serde(default)]
    pub checked_at_ms: u64,
    #[serde(default)]
    pub recheck_at_ms: u64,
}

impl XaiQuota {
    /// Any flag the last answer set: this route cannot answer at all.
    pub fn blocked(&self) -> bool {
        self.team_blocked == Some(true)
            || self.api_key_blocked == Some(true)
            || self.api_key_disabled == Some(true)
    }
}

/// Parse one `GET /v1/api-key` answer.
pub fn parse_xai_api_key(v: &Value, now_ms: u64) -> XaiQuota {
    let d = v.get("data").unwrap_or(v);
    XaiQuota {
        team_blocked: d.get("team_blocked").and_then(Value::as_bool),
        api_key_blocked: d.get("api_key_blocked").and_then(Value::as_bool),
        api_key_disabled: d.get("api_key_disabled").and_then(Value::as_bool),
        checked_at_ms: now_ms,
        recheck_at_ms: 0,
    }
}

// ---- Anthropic ----------------------------------------------------------------

/// One rate-limit family from a reply's `anthropic-ratelimit-*` headers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RateLimit {
    #[serde(default)]
    pub limit: Option<u64>,
    #[serde(default)]
    pub remaining: Option<u64>,
    /// The reset stamp as sent (RFC 3339).
    #[serde(default)]
    pub reset: Option<String>,
}

/// The rate-limit headers of the most recent Anthropic reply.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnthropicRate {
    #[serde(default)]
    pub requests: RateLimit,
    #[serde(default)]
    pub tokens: RateLimit,
    #[serde(default)]
    pub input_tokens: RateLimit,
    #[serde(default)]
    pub output_tokens: RateLimit,
    #[serde(default)]
    pub updated_at_ms: u64,
}

/// Read the `anthropic-ratelimit-*` headers off one reply. Header names
/// match without regard to case; an unparsable number stays unknown.
pub fn parse_anthropic_rate(headers: &[(String, String)], now_ms: u64) -> AnthropicRate {
    let get = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim().to_string())
    };
    let family = |prefix: &str| RateLimit {
        limit: get(&format!("anthropic-ratelimit-{prefix}-limit")).and_then(|s| s.parse().ok()),
        remaining: get(&format!("anthropic-ratelimit-{prefix}-remaining"))
            .and_then(|s| s.parse().ok()),
        reset: get(&format!("anthropic-ratelimit-{prefix}-reset")),
    };
    AnthropicRate {
        requests: family("requests"),
        tokens: family("tokens"),
        input_tokens: family("input-tokens"),
        output_tokens: family("output-tokens"),
        updated_at_ms: now_ms,
    }
}

/// Why a Claude key is benched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicBench {
    #[default]
    None,
    /// The account's own spend limit is enforced: paused until the 1st
    /// of next month, no retries (the limit resets with the month).
    SpendLimit,
    /// The account is out of credit: benched, re-checked hourly.
    OutOfCredit,
}

/// Local Claude spend, priced from the registry's price table. Ticks are
/// 1e-10 USD, the registry's own unit. Day and month roll on UTC.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnthropicSpend {
    /// `YYYY-MM-DD` (UTC) the day counters belong to.
    #[serde(default)]
    pub day: String,
    #[serde(default)]
    pub day_ticks: i64,
    #[serde(default)]
    pub day_requests: u64,
    #[serde(default)]
    pub day_input_tokens: u64,
    #[serde(default)]
    pub day_output_tokens: u64,
    /// `YYYY-MM` (UTC) the month counter belongs to.
    #[serde(default)]
    pub month: String,
    #[serde(default)]
    pub month_ticks: i64,
    #[serde(default)]
    pub total_ticks: i64,
    #[serde(default)]
    pub updated_at_ms: u64,
}

impl AnthropicSpend {
    /// Fold one answered call in. Tokens always count; ticks count only
    /// when the model had a price (`None` keeps the day priced-less).
    pub fn record(
        &mut self,
        ticks: Option<i64>,
        input_tokens: u64,
        output_tokens: u64,
        now_ms: u64,
    ) {
        let day = utc_day_key(now_ms);
        if self.day != day {
            self.day = day;
            self.day_ticks = 0;
            self.day_requests = 0;
            self.day_input_tokens = 0;
            self.day_output_tokens = 0;
        }
        let month = utc_month_key(now_ms);
        if self.month != month {
            self.month = month;
            self.month_ticks = 0;
        }
        let t = ticks.unwrap_or(0);
        self.day_ticks = self.day_ticks.saturating_add(t);
        self.month_ticks = self.month_ticks.saturating_add(t);
        self.total_ticks = self.total_ticks.saturating_add(t);
        self.day_requests = self.day_requests.saturating_add(1);
        self.day_input_tokens = self.day_input_tokens.saturating_add(input_tokens);
        self.day_output_tokens = self.day_output_tokens.saturating_add(output_tokens);
        self.updated_at_ms = now_ms;
    }
}

/// The cost of one Anthropic call in ticks from the registry's prices:
/// cache reads at the cached rate and cache writes at the table's write
/// rate ([`Prices::cache_write_at`], from the bundled model card table)
/// when it names one, else at the prompt rate. `None` when the model
/// has no usable prices.
pub fn anthropic_call_ticks(
    prices: &Prices,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
) -> Option<i64> {
    let prompt_total = input_tokens
        .saturating_add(cache_read_tokens)
        .saturating_add(cache_write_tokens);
    let (prompt, cached, completion) = prices.rates_at(prompt_total)?;
    let write = prices.cache_write_at(prompt_total).unwrap_or(prompt);
    let ticks = input_tokens as i128 * prompt as i128
        + cache_read_tokens as i128 * cached as i128
        + cache_write_tokens as i128 * write as i128
        + output_tokens as i128 * completion as i128;
    i64::try_from(ticks).ok()
}

/// One route's Anthropic account state: headers, spend, bench.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnthropicQuota {
    #[serde(default)]
    pub rate: AnthropicRate,
    #[serde(default)]
    pub spend: AnthropicSpend,
    #[serde(default)]
    pub bench: AnthropicBench,
    /// The bench stands until this time (0 = not benched).
    #[serde(default)]
    pub benched_until_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
}

impl AnthropicQuota {
    /// Benched at `now_ms`: the router must not send this route calls.
    pub fn benched(&self, now_ms: u64) -> bool {
        self.bench != AnthropicBench::None && now_ms < self.benched_until_ms
    }
}

/// The bench a failed Anthropic call earns, if any: a 429 whose error
/// code is `enforced_spend_limit_reached` with no Retry-After pauses the
/// route until the 1st of next month; a 400 saying the credit balance is
/// too low (or the account hit its own usage limits) benches it for an
/// hourly re-check. Plain 429s and 529s earn no bench — the router's
/// cool-down already covers them.
pub fn classify_anthropic_bench(
    status: u16,
    body: &str,
    retry_after: Option<&str>,
    now_ms: u64,
) -> Option<(AnthropicBench, u64)> {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let err = &v["error"];
    let code = err["error_code"]
        .as_str()
        .or_else(|| err["code"].as_str())
        .or_else(|| v["error_code"].as_str())
        .unwrap_or("");
    let message = err["message"].as_str().unwrap_or("").to_ascii_lowercase();
    if status == 429
        && code == "enforced_spend_limit_reached"
        && retry_after.is_none_or(|r| r.trim().is_empty())
    {
        return Some((AnthropicBench::SpendLimit, first_of_next_month_ms(now_ms)));
    }
    if status == 400
        && (message.contains("credit balance is too low")
            || message.contains("specified api usage limits"))
    {
        return Some((
            AnthropicBench::OutOfCredit,
            now_ms.saturating_add(OUT_OF_CREDIT_RECHECK_MS),
        ));
    }
    None
}

// ---- The ledger ---------------------------------------------------------------

/// The whole `quota.json` document: one section per cloud route.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuotaLedger {
    #[serde(default)]
    pub openrouter: OpenRouterQuota,
    #[serde(default)]
    pub xai: XaiQuota,
    #[serde(default)]
    pub anthropic: AnthropicQuota,
}

/// A route is due a re-read when it was never checked, when an error
/// stamped a recheck that has come due, or when the last check aged out.
fn due(checked_at_ms: u64, recheck_at_ms: u64, now_ms: u64) -> bool {
    checked_at_ms == 0
        || (recheck_at_ms > 0 && now_ms >= recheck_at_ms)
        || now_ms.saturating_sub(checked_at_ms) >= QUOTA_POLL_MS
}

impl QuotaLedger {
    pub fn openrouter_due(&self, now_ms: u64) -> bool {
        due(
            self.openrouter.checked_at_ms,
            self.openrouter.recheck_at_ms,
            now_ms,
        )
    }

    pub fn xai_due(&self, now_ms: u64) -> bool {
        due(self.xai.checked_at_ms, self.xai.recheck_at_ms, now_ms)
    }

    /// The OpenRouter account state routing reads.
    pub fn openrouter_state(&self) -> OpenRouterState {
        self.openrouter.state
    }

    /// Free OpenRouter calls left today, when the account reported them.
    pub fn openrouter_free_remaining(&self) -> Option<u64> {
        self.openrouter.free_daily_remaining
    }

    /// The xAI key is blocked outright.
    pub fn xai_blocked(&self) -> bool {
        self.xai.checked_at_ms > 0 && self.xai.blocked()
    }

    /// Claude is benched at `now_ms` (spend limit or out of credit).
    pub fn anthropic_benched(&self, now_ms: u64) -> bool {
        self.anthropic.benched(now_ms)
    }

    /// An OpenRouter 402, 403 or 429: re-read `/key` at once. A 402 on a
    /// free model while the account reads FreeOnly means even free calls
    /// are erroring — the account stands Negative until a re-read shows
    /// it answering again.
    pub fn note_openrouter_error(&mut self, free_model: bool, status: u16, now_ms: u64) {
        if !matches!(status, 402 | 403 | 429) {
            return;
        }
        self.openrouter.recheck_at_ms = now_ms;
        if status == 402 && free_model && self.openrouter.state == OpenRouterState::FreeOnly {
            self.openrouter.state = OpenRouterState::Negative;
        }
    }

    /// An xAI 402, 403 or 429: re-read `/v1/api-key` at once.
    pub fn note_xai_error(&mut self, status: u16, now_ms: u64) {
        if matches!(status, 402 | 403 | 429) {
            self.xai.recheck_at_ms = now_ms;
        }
    }

    /// A failed Anthropic call: stamp its bench, if it earns one.
    pub fn note_anthropic_error(
        &mut self,
        status: u16,
        body: &str,
        retry_after: Option<&str>,
        now_ms: u64,
    ) {
        if let Some((bench, until)) = classify_anthropic_bench(status, body, retry_after, now_ms) {
            self.anthropic.bench = bench;
            self.anthropic.benched_until_ms = until;
            self.anthropic.updated_at_ms = now_ms;
        }
    }

    /// A answered Anthropic call: keep its rate-limit headers and fold
    /// its usage into the spend ledger.
    pub fn note_anthropic_reply(
        &mut self,
        headers: &[(String, String)],
        ticks: Option<i64>,
        input_tokens: u64,
        output_tokens: u64,
        now_ms: u64,
    ) {
        self.anthropic.rate = parse_anthropic_rate(headers, now_ms);
        self.anthropic
            .spend
            .record(ticks, input_tokens, output_tokens, now_ms);
        self.anthropic.updated_at_ms = now_ms;
    }
}

// ---- UTC calendar (no date crate in core; the civil conversion is small) -----

/// Days since the epoch → (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month, day) → days since the epoch.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + (d - 1) as u64;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

/// `YYYY-MM-DD` for the UTC day holding `now_ms`.
pub fn utc_day_key(now_ms: u64) -> String {
    let (y, m, d) = civil_from_days((now_ms / DAY_MS) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM` for the UTC month holding `now_ms`.
pub fn utc_month_key(now_ms: u64) -> String {
    let (y, m, _) = civil_from_days((now_ms / DAY_MS) as i64);
    format!("{y:04}-{m:02}")
}

/// The first instant (00:00 UTC) of the month after the one holding `now_ms`.
pub fn first_of_next_month_ms(now_ms: u64) -> u64 {
    let (y, m, _) = civil_from_days((now_ms / DAY_MS) as i64);
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (days_from_civil(ny, nm, 1) as u64) * DAY_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 2026-11-07 12:00 UTC.
    const NOON: u64 = 1_794_052_800_000;

    #[test]
    fn utc_keys_and_next_month_match_the_calendar() {
        assert_eq!(utc_day_key(NOON), "2026-11-07");
        assert_eq!(utc_month_key(NOON), "2026-11");
        assert_eq!(first_of_next_month_ms(NOON), 1_796_083_200_000);
        // December rolls into January of the next year.
        let dec = 1_796_083_200_000 + 15 * DAY_MS;
        assert_eq!(utc_day_key(dec), "2026-12-16");
        assert_eq!(first_of_next_month_ms(dec), 1_798_761_600_000);
    }

    #[test]
    fn openrouter_free_tier_reads_free_only_with_its_reported_ceiling() {
        let q = parse_openrouter_key(
            &json!({"data": {
                "is_free_tier": true,
                "limit": null,
                "limit_remaining": null,
                "usage": 1.25,
                "usage_daily": 0.5,
                "usage_weekly": 1.0,
                "usage_monthly": 1.25,
                "free_model_daily_requests": {"used": 12, "limit": 50, "remaining": 38}
            }}),
            NOON,
        );
        assert_eq!(q.state, OpenRouterState::FreeOnly);
        assert_eq!(q.free_daily_limit, Some(50));
        assert_eq!(q.free_daily_remaining, Some(38));
        assert_eq!(q.usage_daily_usd, 0.5);
        assert_eq!(q.checked_at_ms, NOON);
    }

    #[test]
    fn openrouter_ceiling_comes_from_the_answer_not_the_flag() {
        // No is_free_tier at all: the 1,000/day ceiling still lands, and
        // a healthy remaining balance reads Paid.
        let q = parse_openrouter_key(
            &json!({"data": {
                "limit_remaining": 25.0,
                "free_model_daily_requests": {"used": 3, "limit": 1000, "remaining": 997}
            }}),
            NOON,
        );
        assert_eq!(q.state, OpenRouterState::Paid);
        assert_eq!(q.free_daily_limit, Some(1000));
        // Down at the floor it reads PaidLow.
        let low = parse_openrouter_key(
            &json!({"data": {"is_free_tier": false, "limit_remaining": 0.25}}),
            NOON,
        );
        assert_eq!(low.state, OpenRouterState::PaidLow);
        // Paid with no limit at all still reads Paid (limit_remaining empty).
        let open = parse_openrouter_key(&json!({"data": {"is_free_tier": false}}), NOON);
        assert_eq!(open.state, OpenRouterState::Paid);
    }

    #[test]
    fn openrouter_free_402_stands_negative_until_a_reread() {
        let mut l = QuotaLedger {
            openrouter: parse_openrouter_key(&json!({"data": {"is_free_tier": true}}), NOON),
            ..Default::default()
        };
        l.note_openrouter_error(true, 402, NOON + 1);
        assert_eq!(l.openrouter_state(), OpenRouterState::Negative);
        assert!(l.openrouter_due(NOON + 1));
        // A paid-model 402 only asks for the re-read.
        let mut l = QuotaLedger {
            openrouter: parse_openrouter_key(&json!({"data": {"is_free_tier": false}}), NOON),
            ..Default::default()
        };
        l.note_openrouter_error(false, 402, NOON + 1);
        assert_eq!(l.openrouter_state(), OpenRouterState::Paid);
        assert!(l.openrouter_due(NOON + 1));
        // A 200 changes nothing, and a fresh check is not due for 10 minutes.
        let l = QuotaLedger {
            openrouter: parse_openrouter_key(&json!({"data": {"is_free_tier": false}}), NOON),
            ..Default::default()
        };
        assert!(!l.openrouter_due(NOON + QUOTA_POLL_MS - 1));
        assert!(l.openrouter_due(NOON + QUOTA_POLL_MS));
    }

    #[test]
    fn xai_blocked_flags_parse_and_block() {
        let q = parse_xai_api_key(
            &json!({"team_blocked": false, "api_key_blocked": true, "api_key_disabled": false}),
            NOON,
        );
        assert!(q.blocked());
        let ok = parse_xai_api_key(&json!({"team_blocked": false}), NOON);
        assert!(!ok.blocked());
        let mut l = QuotaLedger {
            xai: q,
            ..Default::default()
        };
        assert!(l.xai_blocked());
        l.note_xai_error(429, NOON + 1);
        assert!(l.xai_due(NOON + 1));
    }

    #[test]
    fn anthropic_rate_headers_parse_by_family() {
        let headers = vec![
            ("anthropic-ratelimit-requests-limit".into(), "1000".into()),
            (
                "Anthropic-RateLimit-Requests-Remaining".into(),
                "999".into(),
            ),
            (
                "anthropic-ratelimit-requests-reset".into(),
                "2026-11-07T12:01:00Z".into(),
            ),
            (
                "anthropic-ratelimit-tokens-remaining".into(),
                "400000".into(),
            ),
            ("content-type".into(), "application/json".into()),
        ];
        let r = parse_anthropic_rate(&headers, NOON);
        assert_eq!(r.requests.limit, Some(1000));
        assert_eq!(r.requests.remaining, Some(999));
        assert_eq!(r.requests.reset.as_deref(), Some("2026-11-07T12:01:00Z"));
        assert_eq!(r.tokens.remaining, Some(400_000));
        assert_eq!(r.tokens.limit, None);
    }

    #[test]
    fn anthropic_benches_match_the_card() {
        // Spend limit enforced, no Retry-After: paused to the 1st.
        let body = r#"{"type":"error","error":{"type":"rate_limit_error","message":"You have reached your specified API usage limits","error_code":"enforced_spend_limit_reached"}}"#;
        assert_eq!(
            classify_anthropic_bench(429, body, None, NOON),
            Some((AnthropicBench::SpendLimit, 1_796_083_200_000))
        );
        // The same 429 with a Retry-After is an ordinary cool-down.
        assert_eq!(classify_anthropic_bench(429, body, Some("30"), NOON), None);
        // Out of credit on a 400: benched for an hourly re-check.
        let low = r#"{"type":"error","error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the Anthropic API."}}"#;
        assert_eq!(
            classify_anthropic_bench(400, low, None, NOON),
            Some((AnthropicBench::OutOfCredit, NOON + OUT_OF_CREDIT_RECHECK_MS))
        );
        // A plain overload earns no bench.
        assert_eq!(classify_anthropic_bench(529, "{}", None, NOON), None);
        // The bench stands, then lifts.
        let mut l = QuotaLedger::default();
        l.note_anthropic_error(400, low, None, NOON);
        assert!(l.anthropic_benched(NOON + 1));
        assert!(!l.anthropic_benched(NOON + OUT_OF_CREDIT_RECHECK_MS));
    }

    #[test]
    fn anthropic_spend_rolls_day_and_month() {
        let mut s = AnthropicSpend::default();
        s.record(Some(100), 10, 5, NOON);
        s.record(Some(50), 1, 2, NOON + 3_600_000);
        assert_eq!(s.day, "2026-11-07");
        assert_eq!(s.day_ticks, 150);
        assert_eq!(s.day_requests, 2);
        assert_eq!(s.month_ticks, 150);
        // Next UTC day: the day counters reset, the month keeps counting.
        s.record(Some(10), 1, 1, NOON + DAY_MS);
        assert_eq!(s.day, "2026-11-08");
        assert_eq!(s.day_ticks, 10);
        assert_eq!(s.month_ticks, 160);
        assert_eq!(s.total_ticks, 160);
        // An unpriced call counts tokens and the request, not ticks.
        s.record(None, 7, 3, NOON + DAY_MS);
        assert_eq!(s.day_ticks, 10);
        assert_eq!(s.day_input_tokens, 8);
    }

    #[test]
    fn anthropic_call_ticks_price_cache_reads_and_writes() {
        // $3/M prompt, $0.30/M cached, $15/M completion (cents per 100M).
        let prices = Prices {
            prompt: Some(300),
            cached: Some(30),
            completion: Some(1500),
            ..Default::default()
        };
        // 1M fresh + 1M read + 1M written + 1M out.
        let t = anthropic_call_ticks(&prices, 1_000_000, 1_000_000, 1_000_000, 1_000_000);
        assert_eq!(
            t,
            Some(1_000_000 * 300 + 1_000_000 * 30 + 1_000_000 * 300 + 1_000_000 * 1500)
        );
        assert_eq!(anthropic_call_ticks(&Prices::default(), 1, 1, 0, 0), None);
        // With a write rate in the table (1.25x prompt, as the bundled
        // Claude rows carry), writes pay it instead of the prompt rate.
        let priced = Prices {
            cache_write: Some(375),
            ..prices
        };
        let t = anthropic_call_ticks(&priced, 1_000_000, 1_000_000, 1_000_000, 1_000_000);
        assert_eq!(
            t,
            Some(1_000_000 * 300 + 1_000_000 * 30 + 1_000_000 * 375 + 1_000_000 * 1500)
        );
    }

    #[test]
    fn openrouter_permits_follow_the_account_state_and_the_models_own_class() {
        let q = |state, rem, free_rem| OpenRouterQuota {
            state,
            limit_remaining_usd: rem,
            free_daily_remaining: free_rem,
            ..OpenRouterQuota::default()
        };
        // Unchecked: nothing is blocked on a guess.
        assert!(q(OpenRouterState::Unknown, None, None).permits(true));
        assert!(q(OpenRouterState::Unknown, None, None).permits(false));
        // Free-only: free models while the day's ceiling lasts, paid never.
        assert!(q(OpenRouterState::FreeOnly, None, Some(3)).permits(true));
        assert!(!q(OpenRouterState::FreeOnly, None, Some(0)).permits(true));
        assert!(!q(OpenRouterState::FreeOnly, None, None).permits(false));
        // Paid and paid-low: both classes, paid only while credit remains.
        assert!(q(OpenRouterState::Paid, Some(9.0), None).permits(false));
        assert!(q(OpenRouterState::PaidLow, Some(0.2), None).permits(false));
        assert!(!q(OpenRouterState::PaidLow, Some(0.0), None).permits(false));
        assert!(q(OpenRouterState::PaidLow, Some(0.0), None).permits(true));
        // Negative: even free models error until a re-read shows recovery.
        assert!(!q(OpenRouterState::Negative, Some(9.0), Some(9)).permits(true));
        assert!(!q(OpenRouterState::Negative, Some(9.0), Some(9)).permits(false));
    }
}

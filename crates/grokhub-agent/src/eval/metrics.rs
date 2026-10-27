//! Aggregates for A/B agent eval arms (success rate, tokens, latency, tool calls, cost).

use std::fmt::Write as _;

/// Per-arm totals over a fixed task set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArmMetrics {
    pub tasks: u32,
    pub successes: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub wall_ms: u128,
    pub tool_calls: u64,
    /// Prompt-cache hits attributed to this arm (from usage or offline model).
    pub cached_tokens: u64,
    /// Cost in usd-ticks (live) or [`offline_cost_ticks`] for scripted runs.
    pub cost_ticks: i64,
    /// Model calls (outbound requests) across all tasks.
    pub model_calls: u64,
    /// Tool-schema bytes summed over every request.
    pub schema_bytes: u64,
    /// Bytes of every request after the first in its task.
    pub followup_bytes: u64,
    /// Of [`Self::followup_bytes`], bytes shared with the previous request's start.
    pub prefix_stable_bytes: u64,
}

impl ArmMetrics {
    pub fn success_rate(&self) -> f64 {
        if self.tasks == 0 {
            0.0
        } else {
            self.successes as f64 / self.tasks as f64
        }
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.reasoning_tokens)
    }

    pub fn cost_per_task(&self) -> f64 {
        if self.tasks == 0 {
            0.0
        } else {
            self.cost_ticks as f64 / self.tasks as f64
        }
    }

    pub fn cost_per_solved_task(&self) -> f64 {
        if self.successes == 0 {
            f64::INFINITY
        } else {
            self.cost_ticks as f64 / self.successes as f64
        }
    }

    /// Cached input tokens / input tokens (0 when no input).
    pub fn cache_hit_rate(&self) -> f64 {
        if self.input_tokens == 0 {
            0.0
        } else {
            (self.cached_tokens.min(self.input_tokens) as f64) / self.input_tokens as f64
        }
    }

    /// Share of follow-up request bytes that repeat the previous request's start.
    pub fn prefix_stable_ratio(&self) -> f64 {
        ratio(self.prefix_stable_bytes, self.followup_bytes)
    }

    pub fn schema_bytes_per_request(&self) -> f64 {
        ratio(self.schema_bytes, self.model_calls)
    }

    pub fn model_calls_per_task(&self) -> f64 {
        ratio(self.model_calls, u64::from(self.tasks))
    }

    pub fn tool_calls_per_model_call(&self) -> f64 {
        ratio(self.tool_calls, self.model_calls)
    }

    /// Add one task's request shape (see [`derive_usage`]).
    pub fn record_requests(&mut self, shape: &RequestTotals) {
        self.model_calls = self.model_calls.saturating_add(shape.model_calls);
        self.schema_bytes = self.schema_bytes.saturating_add(shape.schema_bytes);
        self.followup_bytes = self.followup_bytes.saturating_add(shape.followup_bytes);
        self.prefix_stable_bytes = self
            .prefix_stable_bytes
            .saturating_add(shape.prefix_stable_bytes);
    }

    pub fn record_task(
        &mut self,
        success: bool,
        wall_ms: u128,
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: u64,
        tool_calls: u64,
    ) {
        self.record_task_full(
            success,
            wall_ms,
            input_tokens,
            output_tokens,
            reasoning_tokens,
            tool_calls,
            0,
            offline_cost_ticks(input_tokens, output_tokens, reasoning_tokens, 0),
        );
    }

    pub fn record_task_full(
        &mut self,
        success: bool,
        wall_ms: u128,
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: u64,
        tool_calls: u64,
        cached_tokens: u64,
        cost_ticks: i64,
    ) {
        self.tasks = self.tasks.saturating_add(1);
        if success {
            self.successes = self.successes.saturating_add(1);
        }
        self.wall_ms = self.wall_ms.saturating_add(wall_ms);
        self.input_tokens = self.input_tokens.saturating_add(input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(output_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(reasoning_tokens);
        self.tool_calls = self.tool_calls.saturating_add(tool_calls);
        self.cached_tokens = self.cached_tokens.saturating_add(cached_tokens);
        self.cost_ticks = self.cost_ticks.saturating_add(cost_ticks);
    }
}

/// Deterministic offline cost model for scripted fixtures (not live pricing).
/// 1 tick per billable input token + 4 per output + 4 per reasoning; cache hits
/// reduce billable input.
pub fn offline_cost_ticks(input: u64, output: u64, reasoning: u64, cached: u64) -> i64 {
    let billable = input.saturating_sub(cached.min(input));
    billable
        .saturating_add(output.saturating_mul(4))
        .saturating_add(reasoning.saturating_mul(4)) as i64
}

fn ratio(num: u64, den: u64) -> f64 {
    if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    }
}

/// Offline token estimate: bytes per token (no tokenizer in the replay).
pub const BYTES_PER_TOKEN: u64 = 4;

/// Tokens for `bytes` of request text, rounded up.
pub fn estimate_tokens(bytes: u64) -> u64 {
    bytes.div_ceil(BYTES_PER_TOKEN)
}

/// Length of the shared start of `a` and `b`, in bytes.
pub fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// One outbound model request as the replay saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedRequest {
    /// Rendered prompt bytes in cache order: system, tools, then the other input items.
    pub body: String,
    /// Bytes of the tools array alone.
    pub schema_bytes: u64,
}

/// Input usage derived from one task's requests instead of fixture numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestTotals {
    pub model_calls: u64,
    pub input_tokens: u64,
    /// Tokens inside the prefix shared with the previous request (rounded down).
    pub cached_tokens: u64,
    pub schema_bytes: u64,
    pub followup_bytes: u64,
    pub prefix_stable_bytes: u64,
}

/// Each request bills its bytes; the prefix it shares with the previous request in
/// the same task counts as cached (a provider prefix cache would hit there).
pub fn derive_usage(requests: &[CapturedRequest]) -> RequestTotals {
    let mut out = RequestTotals::default();
    let mut prev: Option<&[u8]> = None;
    for req in requests {
        let body = req.body.as_bytes();
        let len = body.len() as u64;
        out.model_calls += 1;
        out.input_tokens += estimate_tokens(len);
        out.schema_bytes += req.schema_bytes;
        if let Some(prev) = prev {
            let shared = common_prefix_len(prev, body) as u64;
            out.followup_bytes += len;
            out.prefix_stable_bytes += shared;
            out.cached_tokens += shared / BYTES_PER_TOKEN;
        }
        prev = Some(body);
    }
    out
}

/// Side-by-side arm numbers plus the gate outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct AbComparison {
    pub control: ArmMetrics,
    pub variant: ArmMetrics,
    pub model: String,
}

/// Variant success rate must not trail control (any drop fails the run).
#[derive(Debug, Clone, PartialEq)]
pub struct AbGateFailure {
    pub control_rate: f64,
    pub variant_rate: f64,
}

impl AbGateFailure {
    pub fn message(&self) -> String {
        format!(
            "variant success rate {:.4} is below control {:.4}",
            self.variant_rate, self.control_rate
        )
    }
}

pub fn gate(comparison: &AbComparison) -> Result<(), AbGateFailure> {
    let control_rate = comparison.control.success_rate();
    let variant_rate = comparison.variant.success_rate();
    if variant_rate + f64::EPSILON < control_rate {
        Err(AbGateFailure {
            control_rate,
            variant_rate,
        })
    } else {
        Ok(())
    }
}

pub fn format_report(cmp: &AbComparison) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Agent prompt A/B eval\n\nModel: {}\n", cmp.model);
    out.push_str("## Summary\n\n");
    out.push_str("| Arm | Success rate | Tasks | Tokens | Wall ms | Tool calls |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for (label, arm) in [("control", &cmp.control), ("variant", &cmp.variant)] {
        let _ = writeln!(
            out,
            "| {label} | {:.2}% | {}/{} | {} | {} | {} |",
            arm.success_rate() * 100.0,
            arm.successes,
            arm.tasks,
            arm.total_tokens(),
            arm.wall_ms,
            arm.tool_calls
        );
    }
    let gate_line = match gate(cmp) {
        Ok(()) => "pass: variant success rate did not drop".to_string(),
        Err(f) => f.message(),
    };
    let _ = writeln!(out, "\nGate: {gate_line}\n");
    out
}

/// Success delta within ±1 task on a 20-task set is treated as noise.
pub const NOISE_TASK_DELTA: u32 = 1;

/// Absolute success-count delta is within noise for the fixture set size.
pub fn success_within_noise(baseline: &ArmMetrics, arm: &ArmMetrics) -> bool {
    let a = baseline.successes;
    let b = arm.successes;
    let delta = a.abs_diff(b);
    delta <= NOISE_TASK_DELTA
}

/// Adopt / spike / reject for one B arm vs A (baseline).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recommendation {
    Adopt,
    Spike,
    Reject,
}

impl Recommendation {
    pub fn as_str(self) -> &'static str {
        match self {
            Recommendation::Adopt => "adopt",
            Recommendation::Spike => "spike",
            Recommendation::Reject => "reject",
        }
    }
}

/// Deltas of B relative to A (positive = B higher).
#[derive(Debug, Clone, PartialEq)]
pub struct TechniqueDelta {
    pub success_rate: f64,
    pub cost_per_task: f64,
    pub cost_per_solved_task: f64,
    pub cache_hit_rate: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

impl TechniqueDelta {
    pub fn between(baseline: &ArmMetrics, arm: &ArmMetrics) -> Self {
        Self {
            success_rate: arm.success_rate() - baseline.success_rate(),
            cost_per_task: arm.cost_per_task() - baseline.cost_per_task(),
            cost_per_solved_task: finite_sub(
                arm.cost_per_solved_task(),
                baseline.cost_per_solved_task(),
            ),
            cache_hit_rate: arm.cache_hit_rate() - baseline.cache_hit_rate(),
            input_tokens: arm.input_tokens as i64 - baseline.input_tokens as i64,
            output_tokens: arm.output_tokens as i64 - baseline.output_tokens as i64,
        }
    }
}

fn finite_sub(a: f64, b: f64) -> f64 {
    if !a.is_finite() && !b.is_finite() {
        0.0
    } else if !a.is_finite() {
        f64::INFINITY
    } else if !b.is_finite() {
        f64::NEG_INFINITY
    } else {
        a - b
    }
}

/// Recommendation rule: adopt only if cost per solved task falls and success is
/// within noise; reject if success leaves noise or cost rises; otherwise spike.
pub fn recommend(baseline: &ArmMetrics, arm: &ArmMetrics) -> Recommendation {
    let noise = success_within_noise(baseline, arm);
    if !noise {
        return Recommendation::Reject;
    }
    let base_cps = baseline.cost_per_solved_task();
    let arm_cps = arm.cost_per_solved_task();
    if arm_cps < base_cps && base_cps.is_finite() {
        Recommendation::Adopt
    } else if arm_cps > base_cps && arm_cps.is_finite() && base_cps.is_finite() {
        Recommendation::Reject
    } else {
        Recommendation::Spike
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_fails_when_variant_success_rate_drops() {
        let cmp = AbComparison {
            control: ArmMetrics {
                tasks: 10,
                successes: 10,
                ..ArmMetrics::default()
            },
            variant: ArmMetrics {
                tasks: 10,
                successes: 9,
                ..ArmMetrics::default()
            },
            model: "grok-4.7".into(),
        };
        assert!(gate(&cmp).is_err());
    }

    #[test]
    fn gate_passes_when_variant_matches_control() {
        let cmp = AbComparison {
            control: ArmMetrics {
                tasks: 5,
                successes: 4,
                ..ArmMetrics::default()
            },
            variant: ArmMetrics {
                tasks: 5,
                successes: 4,
                ..ArmMetrics::default()
            },
            model: "grok-4.7".into(),
        };
        assert!(gate(&cmp).is_ok());
    }

    #[test]
    fn gate_passes_when_variant_beats_control() {
        let cmp = AbComparison {
            control: ArmMetrics {
                tasks: 5,
                successes: 3,
                ..ArmMetrics::default()
            },
            variant: ArmMetrics {
                tasks: 5,
                successes: 4,
                ..ArmMetrics::default()
            },
            model: "grok-4.7".into(),
        };
        assert!(gate(&cmp).is_ok());
    }

    #[test]
    fn noise_allows_plus_or_minus_one_success() {
        let base = ArmMetrics {
            tasks: 20,
            successes: 20,
            ..ArmMetrics::default()
        };
        let within = ArmMetrics {
            tasks: 20,
            successes: 19,
            ..ArmMetrics::default()
        };
        let outside = ArmMetrics {
            tasks: 20,
            successes: 18,
            ..ArmMetrics::default()
        };
        assert!(success_within_noise(&base, &within));
        assert!(!success_within_noise(&base, &outside));
    }

    #[test]
    fn recommend_adopt_when_cost_falls_and_success_in_noise() {
        let base = ArmMetrics {
            tasks: 20,
            successes: 20,
            cost_ticks: 2000,
            ..ArmMetrics::default()
        };
        let cheaper = ArmMetrics {
            tasks: 20,
            successes: 20,
            cost_ticks: 1500,
            ..ArmMetrics::default()
        };
        assert_eq!(recommend(&base, &cheaper), Recommendation::Adopt);
    }

    #[test]
    fn recommend_spike_when_cost_flat_and_success_in_noise() {
        let base = ArmMetrics {
            tasks: 20,
            successes: 20,
            cost_ticks: 2000,
            ..ArmMetrics::default()
        };
        let flat = ArmMetrics {
            tasks: 20,
            successes: 20,
            cost_ticks: 2000,
            ..ArmMetrics::default()
        };
        assert_eq!(recommend(&base, &flat), Recommendation::Spike);
        // One fewer success with the same total cost raises cost/solved → reject.
        let noisier = ArmMetrics {
            tasks: 20,
            successes: 19,
            cost_ticks: 2000,
            ..ArmMetrics::default()
        };
        assert!(success_within_noise(&base, &noisier));
        assert_eq!(recommend(&base, &noisier), Recommendation::Reject);
    }

    #[test]
    fn recommend_reject_when_success_leaves_noise() {
        let base = ArmMetrics {
            tasks: 20,
            successes: 20,
            cost_ticks: 2000,
            ..ArmMetrics::default()
        };
        let drop = ArmMetrics {
            tasks: 20,
            successes: 18,
            cost_ticks: 1000,
            ..ArmMetrics::default()
        };
        assert_eq!(recommend(&base, &drop), Recommendation::Reject);
    }

    #[test]
    fn common_prefix_len_counts_shared_start() {
        assert_eq!(common_prefix_len(b"system tools a", b"system tools b"), 13);
        assert_eq!(common_prefix_len(b"abc", b"abcdef"), 3);
        assert_eq!(common_prefix_len(b"xbc", b"abc"), 0);
        assert_eq!(common_prefix_len(b"", b"abc"), 0);
    }

    #[test]
    fn estimate_tokens_rounds_up_by_four_bytes() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1), 1);
        assert_eq!(estimate_tokens(8), 2);
        assert_eq!(estimate_tokens(9), 3);
    }

    fn req(body: &str, schema_bytes: u64) -> CapturedRequest {
        CapturedRequest {
            body: body.into(),
            schema_bytes,
        }
    }

    #[test]
    fn derive_usage_bills_only_bytes_after_the_shared_prefix() {
        // 16 bytes, then 24 bytes that repeat the first 16 (append-only),
        // then 20 bytes that diverge after 8 (the system text changed).
        let totals = derive_usage(&[
            req("SYSTEMxxTOOLSxxx", 5),
            req("SYSTEMxxTOOLSxxxUSER-ONE", 5),
            req("SYSTEMxxEDITEDxxUSER", 7),
        ]);
        assert_eq!(
            totals,
            RequestTotals {
                model_calls: 3,
                input_tokens: 4 + 6 + 5,
                cached_tokens: 4 + 2,
                schema_bytes: 17,
                followup_bytes: 44,
                prefix_stable_bytes: 24,
            }
        );
        let billable = totals.input_tokens - totals.cached_tokens;
        assert_eq!(billable, 9);
    }

    #[test]
    fn derive_usage_of_one_request_has_no_cache() {
        let totals = derive_usage(&[req("abcdefgh", 3)]);
        assert_eq!(totals.input_tokens, 2);
        assert_eq!(totals.cached_tokens, 0);
        assert_eq!(totals.followup_bytes, 0);
        let mut arm = ArmMetrics::default();
        arm.record_requests(&totals);
        assert_eq!(arm.prefix_stable_ratio(), 0.0);
        assert_eq!(arm.schema_bytes_per_request(), 3.0);
    }

    #[test]
    fn arm_ratios_from_recorded_requests() {
        let mut arm = ArmMetrics {
            tasks: 2,
            tool_calls: 3,
            ..ArmMetrics::default()
        };
        arm.record_requests(&RequestTotals {
            model_calls: 4,
            schema_bytes: 400,
            followup_bytes: 200,
            prefix_stable_bytes: 150,
            ..RequestTotals::default()
        });
        assert_eq!(arm.model_calls_per_task(), 2.0);
        assert_eq!(arm.tool_calls_per_model_call(), 0.75);
        assert_eq!(arm.schema_bytes_per_request(), 100.0);
        assert_eq!(arm.prefix_stable_ratio(), 0.75);
    }

    #[test]
    fn offline_cost_ticks_subtracts_cache_hits() {
        assert_eq!(offline_cost_ticks(100, 10, 0, 0), 140);
        assert_eq!(offline_cost_ticks(100, 10, 0, 40), 100);
    }
}

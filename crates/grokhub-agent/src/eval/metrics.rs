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
    fn offline_cost_ticks_subtracts_cache_hits() {
        assert_eq!(offline_cost_ticks(100, 10, 0, 0), 140);
        assert_eq!(offline_cost_ticks(100, 10, 0, 40), 100);
    }
}

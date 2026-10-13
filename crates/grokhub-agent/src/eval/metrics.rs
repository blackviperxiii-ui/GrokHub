//! Aggregates for A/B agent eval arms (success rate, tokens, latency, tool calls).

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

    pub fn record_task(
        &mut self,
        success: bool,
        wall_ms: u128,
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: u64,
        tool_calls: u64,
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
    }
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
            self.variant_rate,
            self.control_rate
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
    let _ = writeln!(
        out,
        "# Agent prompt A/B eval\n\nModel: {}\n",
        cmp.model
    );
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
}

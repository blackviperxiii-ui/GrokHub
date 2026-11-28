//! Reasoning-effort eval sweep (`live_eval --efforts low,medium,high,xhigh`).
//!
//! Runs the same live suite once per effort and compares pass rate, tokens,
//! wall time, and USD cost. Offline tests feed scripted arm metrics; live
//! paid calls stay in the `live_eval` example, never PR CI.

use std::path::{Path, PathBuf};

use super::coding::results_path;
use super::live::{write_atomic, LiveReport, TICKS_PER_USD};

/// One effort's headline numbers for the comparison table.
#[derive(Clone, Debug, PartialEq)]
pub struct EffortArmMetrics {
    pub effort: String,
    pub tries: usize,
    pub passed: usize,
    pub tokens: u64,
    pub wall_ms: u128,
    pub usd: f64,
}

impl EffortArmMetrics {
    /// Roll up one finished live report into a comparison row.
    pub fn from_report(effort: &str, report: &LiveReport) -> Self {
        let tries = report.attempts.len();
        let passed = report.attempts.iter().filter(|a| a.pass).count();
        let tokens: u64 = report
            .attempts
            .iter()
            .map(|a| {
                a.usage
                    .input_tokens
                    .saturating_add(a.usage.output_tokens)
                    .saturating_add(a.usage.reasoning_tokens)
            })
            .sum();
        let wall_ms: u128 = report.attempts.iter().map(|a| a.wall_ms).sum();
        let usd = report.spent_ticks as f64 / TICKS_PER_USD as f64;
        Self {
            effort: effort.to_string(),
            tries,
            passed,
            tokens,
            wall_ms,
            usd,
        }
    }

    pub fn pass_rate_label(&self) -> String {
        super::coding::pass_rate(self.passed, self.tries)
    }
}

/// Side-by-side comparison of several effort arms.
#[derive(Clone, Debug, PartialEq)]
pub struct EffortSweepReport {
    pub model: String,
    pub provider: String,
    pub arms: Vec<EffortArmMetrics>,
}

/// Parse `--efforts low,medium,high,xhigh`. Order is kept; duplicates fail.
pub fn parse_efforts(raw: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for part in raw.split(',') {
        let effort = part.trim();
        if effort.is_empty() {
            return Err("--efforts takes a comma list such as low,medium,high,xhigh".into());
        }
        if grokhub_core::model_registry::EFFORT_LADDER
            .iter()
            .all(|rung| !rung.eq_ignore_ascii_case(effort))
        {
            return Err(format!(
                "--efforts: unknown effort {effort} (use none, minimal, low, medium, high, xhigh, max)"
            ));
        }
        let canon = grokhub_core::model_registry::EFFORT_LADDER
            .iter()
            .find(|rung| rung.eq_ignore_ascii_case(effort))
            .expect("matched above")
            .to_string();
        if out.iter().any(|seen: &String| seen == &canon) {
            return Err(format!("--efforts lists {canon} more than once"));
        }
        out.push(canon);
    }
    if out.is_empty() {
        return Err("--efforts takes a comma list such as low,medium,high,xhigh".into());
    }
    Ok(out)
}

pub fn render_markdown(report: &EffortSweepReport) -> String {
    let mut out = format!(
        "# Reasoning-effort sweep\n\nmodel: {}  ·  provider: {}\n\n\
         Same suite at each effort. Pass rate, tokens, wall time, and USD cost \
         sit side by side. This table is not a live paid run by itself.\n\n\
         | effort | pass rate | tokens | wall ms | USD |\n\
         |---|---|---|---|---|\n",
        report.model, report.provider
    );
    for arm in &report.arms {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            arm.effort,
            arm.pass_rate_label(),
            arm.tokens,
            arm.wall_ms,
            usd_label(arm.usd),
        ));
    }
    out.push('\n');
    out
}

pub fn results_json(report: &EffortSweepReport) -> String {
    let mut out = format!(
        "{{\n  \"suite\": \"effort-sweep\",\n  \"model\": \"{}\",\n  \"provider\": \"{}\",\n  \"arms\": [\n",
        json_escape(&report.model),
        json_escape(&report.provider),
    );
    for (i, arm) in report.arms.iter().enumerate() {
        if i > 0 {
            out.push_str(",\n");
        }
        out.push_str(&format!(
            "    {{\n      \"effort\": \"{}\",\n      \"tries\": {},\n      \"passed\": {},\n      \"pass_rate\": \"{}\",\n      \"tokens\": {},\n      \"wall_ms\": {},\n      \"usd\": {}\n    }}",
            json_escape(&arm.effort),
            arm.tries,
            arm.passed,
            json_escape(&arm.pass_rate_label()),
            arm.tokens,
            arm.wall_ms,
            usd_json(arm.usd),
        ));
    }
    out.push_str("\n  ]\n}\n");
    out
}

pub fn write_results(report: &EffortSweepReport, out: &Path) -> std::io::Result<PathBuf> {
    let path = results_path(out);
    write_atomic(&path, results_json(report).as_bytes())?;
    Ok(path)
}

fn usd_label(usd: f64) -> String {
    format!("${usd:.4}")
}

fn usd_json(usd: f64) -> String {
    format!("{usd:.4}")
}

fn json_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::live::{EvalSuite, LiveReport, Provider, DEFAULT_K};
    use crate::Usage;

    fn scripted_low_high() -> EffortSweepReport {
        EffortSweepReport {
            model: "scripted".into(),
            provider: "grok".into(),
            arms: vec![
                EffortArmMetrics {
                    effort: "low".into(),
                    tries: 4,
                    passed: 3,
                    tokens: 1000,
                    wall_ms: 2000,
                    usd: 0.01,
                },
                EffortArmMetrics {
                    effort: "high".into(),
                    tries: 4,
                    passed: 4,
                    tokens: 4000,
                    wall_ms: 8000,
                    usd: 0.08,
                },
            ],
        }
    }

    #[test]
    fn parse_efforts_keeps_order_and_rejects_junk() {
        assert_eq!(
            parse_efforts("low,medium,high,xhigh").unwrap(),
            vec!["low", "medium", "high", "xhigh"]
        );
        assert_eq!(parse_efforts(" HIGH , Low ").unwrap(), vec!["high", "low"]);
        let err = parse_efforts("low,turbo").unwrap_err();
        assert_eq!(
            err,
            "--efforts: unknown effort turbo (use none, minimal, low, medium, high, xhigh, max)"
        );
        assert_eq!(
            parse_efforts("low,low").unwrap_err(),
            "--efforts lists low more than once"
        );
        assert_eq!(
            parse_efforts("").unwrap_err(),
            "--efforts takes a comma list such as low,medium,high,xhigh"
        );
    }

    #[test]
    fn scripted_low_vs_high_sweep_asserts_literal_metrics() {
        let report = scripted_low_high();
        assert_eq!(report.arms[0].effort, "low");
        assert_eq!(report.arms[0].pass_rate_label(), "75% (3/4)");
        assert_eq!(report.arms[0].tokens, 1000);
        assert_eq!(report.arms[0].wall_ms, 2000);
        assert_eq!(report.arms[0].usd, 0.01);
        assert_eq!(report.arms[1].effort, "high");
        assert_eq!(report.arms[1].pass_rate_label(), "100% (4/4)");
        assert_eq!(report.arms[1].tokens, 4000);
        assert_eq!(report.arms[1].wall_ms, 8000);
        assert_eq!(report.arms[1].usd, 0.08);

        let md = render_markdown(&report);
        assert!(md.contains("# Reasoning-effort sweep"));
        assert!(md.contains("| effort | pass rate | tokens | wall ms | USD |"));
        assert!(md.contains("| low | 75% (3/4) | 1000 | 2000 | $0.0100 |"));
        assert!(md.contains("| high | 100% (4/4) | 4000 | 8000 | $0.0800 |"));

        let json = results_json(&report);
        assert_eq!(
            json,
            "{\n  \"suite\": \"effort-sweep\",\n  \"model\": \"scripted\",\n  \"provider\": \"grok\",\n  \"arms\": [\n    {\n      \"effort\": \"low\",\n      \"tries\": 4,\n      \"passed\": 3,\n      \"pass_rate\": \"75% (3/4)\",\n      \"tokens\": 1000,\n      \"wall_ms\": 2000,\n      \"usd\": 0.0100\n    },\n    {\n      \"effort\": \"high\",\n      \"tries\": 4,\n      \"passed\": 4,\n      \"pass_rate\": \"100% (4/4)\",\n      \"tokens\": 4000,\n      \"wall_ms\": 8000,\n      \"usd\": 0.0800\n    }\n  ]\n}\n"
        );
    }

    #[test]
    fn from_report_rolls_up_scripted_attempts() {
        let mut report = LiveReport {
            model: "scripted".into(),
            provider: Provider::Grok,
            ab: None,
            mde: 0.1,
            k: DEFAULT_K,
            attempts: Vec::new(),
            spent_ticks: TICKS_PER_USD / 100,
            budget_ticks: TICKS_PER_USD,
            billed_errors: 0,
            stopped_by_budget: false,
            stopped: None,
            planned: 2,
            retries: 0,
            done: true,
            suite: EvalSuite::Default,
        };
        report.attempts.push(crate::eval::live::Attempt {
            task: "alpha",
            set: crate::eval::live::TaskSet::Capability,
            recall: false,
            pass: true,
            usage: Usage {
                input_tokens: 80,
                cached_tokens: 0,
                output_tokens: 10,
                reasoning_tokens: 10,
                cost_in_usd_ticks: TICKS_PER_USD / 200,
            },
            wall_ms: 150,
            error: None,
            arm: None,
            asks: 0,
            compactions: 0,
            calls: 1,
            safety: None,
            round: 0,
            effort_range: ("low", "medium"),
            picks: Vec::new(),
            tool_calls: 0,
            malformed_calls: 0,
        });
        report.attempts.push(crate::eval::live::Attempt {
            task: "beta",
            set: crate::eval::live::TaskSet::Regression,
            recall: false,
            pass: false,
            usage: Usage {
                input_tokens: 20,
                cached_tokens: 0,
                output_tokens: 5,
                reasoning_tokens: 5,
                cost_in_usd_ticks: TICKS_PER_USD / 200,
            },
            wall_ms: 50,
            error: Some("no".into()),
            arm: None,
            asks: 0,
            compactions: 0,
            calls: 1,
            safety: None,
            round: 0,
            effort_range: ("low", "medium"),
            picks: Vec::new(),
            tool_calls: 0,
            malformed_calls: 0,
        });
        let arm = EffortArmMetrics::from_report("low", &report);
        assert_eq!(arm.effort, "low");
        assert_eq!(arm.tries, 2);
        assert_eq!(arm.passed, 1);
        assert_eq!(arm.pass_rate_label(), "50% (1/2)");
        assert_eq!(arm.tokens, 130);
        assert_eq!(arm.wall_ms, 200);
        assert_eq!(arm.usd, 0.01);
    }
}

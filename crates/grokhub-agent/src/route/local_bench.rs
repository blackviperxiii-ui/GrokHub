//! Offline card PR D: local speed bench + a small on-device eval set.
//!
//! Measures tokens/s, first-token latency, load time and RAM/VRAM peaks against
//! a [`LocalRuntime`](super::local::LocalRuntime). CI runs the same schema and
//! eval against a fake server / fake runtime (no GPU required).

use std::time::Instant;

use serde_json::Value;

use grokhub_core::local_setup::Hardware;

use super::local::{self, ChatMessage, LocalRuntime};

/// CI floors for the fake-server path (relative thresholds, not GPU numbers).
pub const CI_MAX_FIRST_TOKEN_MS: u64 = 5_000;
pub const CI_MIN_TOKENS_PER_SEC: f64 = 0.05;
pub const CI_MIN_EVAL_PASS_RATE: f64 = 1.0;

/// One local speed run.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchReport {
    pub tokens_per_sec: f64,
    pub first_token_ms: u64,
    pub load_ms: u64,
    pub ram_peak_mb: u64,
    pub vram_peak_mb: u64,
    pub completion_tokens: u64,
    pub backend: String,
}

impl BenchReport {
    /// Human lines for the CLI / Settings status.
    pub fn summary_lines(&self) -> String {
        format!(
            "Local speed ({})\n  first token: {} ms\n  tokens/s: {:.2}\n  load: {} ms\n  RAM peak: {} MB\n  VRAM peak: {} MB\n  completion tokens: {}",
            self.backend,
            self.first_token_ms,
            self.tokens_per_sec,
            self.load_ms,
            self.ram_peak_mb,
            self.vram_peak_mb,
            self.completion_tokens
        )
    }
}

/// One eval item result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalItemResult {
    pub name: &'static str,
    pub pass: bool,
    pub detail: String,
}

/// Pass rates over the fixed local eval set.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalReport {
    pub items: Vec<EvalItemResult>,
}

impl EvalReport {
    pub fn passed(&self) -> usize {
        self.items.iter().filter(|i| i.pass).count()
    }

    pub fn total(&self) -> usize {
        self.items.len()
    }

    pub fn pass_rate(&self) -> f64 {
        if self.items.is_empty() {
            0.0
        } else {
            self.passed() as f64 / self.total() as f64
        }
    }

    pub fn summary_lines(&self) -> String {
        let mut out = format!("Local eval: {}/{} ({:.0}%)\n", self.passed(), self.total(), self.pass_rate() * 100.0);
        for i in &self.items {
            out.push_str(&format!("  {}: {} — {}\n", i.name, if i.pass { "pass" } else { "fail" }, i.detail));
        }
        out
    }
}

/// Rough token estimate (whitespace words), enough for relative CI checks.
pub fn estimate_tokens(text: &str) -> u64 {
    text.split_whitespace().filter(|w| !w.is_empty()).count().max(1) as u64
}

/// Time a streaming chat turn: first-token latency and tokens/s.
pub fn measure_chat(rt: &dyn LocalRuntime, prompt: &str) -> Result<(u64, f64, u64, String), String> {
    let mut first_ms = 0u64;
    let mut saw_first = false;
    let start = Instant::now();
    let text = rt.chat_stream(&[ChatMessage { role: "user".into(), content: prompt.into() }], &mut |d| {
        if !saw_first && !d.is_empty() {
            first_ms = start.elapsed().as_millis() as u64;
            saw_first = true;
        }
    })?;
    let elapsed = start.elapsed().as_secs_f64().max(1e-6);
    let tokens = estimate_tokens(&text);
    let tps = tokens as f64 / elapsed;
    if !saw_first {
        first_ms = start.elapsed().as_millis() as u64;
    }
    Ok((first_ms, tps, tokens, text))
}

/// Run the speed bench against an already-ready runtime.
///
/// `load_ms` is measured by the caller (sidecar start or 0 for an in-process fake).
pub fn run_bench(rt: &dyn LocalRuntime, hw: &Hardware, load_ms: u64, backend: &str) -> Result<BenchReport, String> {
    let (first_token_ms, tokens_per_sec, completion_tokens, _) =
        measure_chat(rt, "Reply with exactly: bench ok")?;
    Ok(BenchReport {
        tokens_per_sec,
        first_token_ms,
        load_ms,
        ram_peak_mb: hw.ram_mb,
        vram_peak_mb: hw.vram_mb,
        completion_tokens,
        backend: backend.into(),
    })
}

/// Fixed local eval: chat, summarize, classify, and a two-step tool-shaped turn.
pub fn run_eval(rt: &dyn LocalRuntime) -> EvalReport {
    let mut items = Vec::new();

    // 1. Chat answer
    match measure_chat(rt, "Say hello in one short sentence.") {
        Ok((_, _, _, text)) => {
            let pass = !text.trim().is_empty();
            items.push(EvalItemResult {
                name: "chat",
                pass,
                detail: if pass { format!("got {} chars", text.len()) } else { "empty reply".into() },
            });
        }
        Err(e) => items.push(EvalItemResult { name: "chat", pass: false, detail: e }),
    }

    // 2. Summarize
    match rt.summarize("GrokHub is a native cabin app. It runs on Linux and Windows.", 20) {
        Ok(v) => {
            let summary = v.get("summary").and_then(Value::as_str).unwrap_or("");
            let pass = !summary.is_empty();
            items.push(EvalItemResult {
                name: "summarize",
                pass,
                detail: if pass { format!("summary len {}", summary.len()) } else { format!("bad json: {v}") },
            });
        }
        Err(e) => items.push(EvalItemResult { name: "summarize", pass: false, detail: e }),
    }

    // 3. Classify
    match rt.classify("Please delete my account and wipe the disk.", &["routine", "needs_attention", "sensitive"]) {
        Ok(v) => {
            let label = v.get("label").and_then(Value::as_str).unwrap_or("");
            let pass = matches!(label, "routine" | "needs_attention" | "sensitive");
            items.push(EvalItemResult {
                name: "classify",
                pass,
                detail: if pass { format!("label={label}") } else { format!("bad label in {v}") },
            });
        }
        Err(e) => items.push(EvalItemResult { name: "classify", pass: false, detail: e }),
    }

    // 4. Two-step tool-shaped turn: classify then act on the label (summarize).
    let step = (|| {
        let v = rt.classify("Summarize this inbox briefly.", &["routine", "needs_attention", "sensitive"])?;
        let label = v.get("label").and_then(Value::as_str).unwrap_or("routine");
        let s = rt.summarize(&format!("class={label}; body=meeting at 3pm"), 12)?;
        let summary = s.get("summary").and_then(Value::as_str).unwrap_or("").to_string();
        if summary.is_empty() {
            Err("empty second-step summary".into())
        } else {
            Ok(format!("step1={label}; step2_len={}", summary.len()))
        }
    })();
    match step {
        Ok(detail) => items.push(EvalItemResult { name: "tool_two_step", pass: true, detail }),
        Err(e) => items.push(EvalItemResult { name: "tool_two_step", pass: false, detail: e }),
    }

    EvalReport { items }
}

/// Fail when the fake/CPU path regresses past the stored floors.
pub fn check_ci_regression(bench: &BenchReport, eval: &EvalReport) -> Result<(), String> {
    if bench.first_token_ms > CI_MAX_FIRST_TOKEN_MS {
        return Err(format!(
            "first_token_ms {} > CI floor {}",
            bench.first_token_ms, CI_MAX_FIRST_TOKEN_MS
        ));
    }
    if bench.tokens_per_sec < CI_MIN_TOKENS_PER_SEC {
        return Err(format!(
            "tokens_per_sec {:.4} < CI floor {}",
            bench.tokens_per_sec, CI_MIN_TOKENS_PER_SEC
        ));
    }
    if eval.pass_rate() + f64::EPSILON < CI_MIN_EVAL_PASS_RATE {
        return Err(format!(
            "eval pass rate {:.2} < floor {}",
            eval.pass_rate(),
            CI_MIN_EVAL_PASS_RATE
        ));
    }
    Ok(())
}

/// Combined text for CLI stdout / Settings status.
pub fn format_full_report(bench: &BenchReport, eval: &EvalReport) -> String {
    format!("{}\n\n{}", bench.summary_lines(), eval.summary_lines())
}

/// Production CLI: start the sidecar (if assets exist), bench + eval, print, exit code.
pub fn run_cli() -> i32 {
    let dir = crate::perm::config_dir();
    local::set_enabled(true);
    let load_start = Instant::now();
    if let Err(e) = super::local_sidecar::ensure_global(&dir) {
        eprintln!("grokhub bench local: {e}");
        eprintln!("Install a local model (Settings → Local model → Set up) or set GROKHUB_LLAMA_SERVER / GROKHUB_LOCAL_MODEL.");
        return 1;
    }
    let load_ms = load_start.elapsed().as_millis() as u64;
    let Some(rt) = local::runtime() else {
        eprintln!("grokhub bench local: runtime not available after start");
        return 1;
    };
    let d = super::local_perf::current_defaults();
    let hw = Hardware {
        gpu: "local".into(),
        vram_mb: if d.n_gpu_layers > 0 { d.ctx / 2 } else { 0 },
        ram_mb: d.ctx,
        free_disk_mb: 0,
    };
    match run_bench(rt.as_ref(), &hw, load_ms, "llama-server") {
        Ok(bench) => {
            let eval = run_eval(rt.as_ref());
            println!("{}", format_full_report(&bench, &eval));
            if eval.pass_rate() + f64::EPSILON < CI_MIN_EVAL_PASS_RATE {
                eprintln!("eval below pass floor");
                return 1;
            }
            0
        }
        Err(e) => {
            eprintln!("grokhub bench local: {e}");
            1
        }
    }
}

/// In-process bench for Settings (uses the live runtime when installed).
pub fn run_for_ui() -> Result<String, String> {
    local::set_enabled(true);
    let load_start = Instant::now();
    let _ = super::local_sidecar::ensure_global(&crate::perm::config_dir());
    let load_ms = load_start.elapsed().as_millis() as u64;
    let rt = local::runtime().ok_or_else(|| "No local model runtime is installed.".to_string())?;
    let d = super::local_perf::current_defaults();
    let hw = Hardware {
        gpu: "local".into(),
        vram_mb: if d.n_gpu_layers > 0 { 4_096 } else { 0 },
        ram_mb: d.ctx.max(4_096),
        free_disk_mb: 0,
    };
    let bench = run_bench(rt.as_ref(), &hw, load_ms, "llama-server")?;
    let eval = run_eval(rt.as_ref());
    Ok(format_full_report(&bench, &eval))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct FakeRt;

    impl LocalRuntime for FakeRt {
        fn classify(&self, _text: &str, labels: &[&str]) -> Result<Value, String> {
            Ok(json!({ "label": labels[0], "score": 0.9 }))
        }
        fn summarize(&self, _text: &str, _max_words: u32) -> Result<Value, String> {
            Ok(json!({ "summary": "short summary of the text" }))
        }
        fn difficulty(&self, _text: &str) -> Result<Value, String> {
            Ok(json!({ "d": 0.3 }))
        }
        fn chat_stream(&self, _messages: &[ChatMessage], on_delta: &mut dyn FnMut(&str)) -> Result<String, String> {
            on_delta("bench ");
            on_delta("ok");
            Ok("bench ok".into())
        }
    }

    #[test]
    fn bench_report_schema_and_ci_floors_pass_on_fake() {
        let hw = Hardware { gpu: "CPU".into(), vram_mb: 0, ram_mb: 8_192, free_disk_mb: 50_000 };
        let bench = run_bench(&FakeRt, &hw, 12, "fake").expect("bench");
        assert!(bench.tokens_per_sec > 0.0, "{bench:?}");
        assert!(bench.completion_tokens >= 1);
        assert_eq!(bench.load_ms, 12);
        assert_eq!(bench.ram_peak_mb, 8_192);
        assert!(bench.summary_lines().contains("tokens/s"));
        let eval = run_eval(&FakeRt);
        assert_eq!(eval.total(), 4);
        assert_eq!(eval.passed(), 4, "{}", eval.summary_lines());
        assert!((eval.pass_rate() - 1.0).abs() < f64::EPSILON);
        check_ci_regression(&bench, &eval).expect("ci");
    }

    #[test]
    fn estimate_tokens_counts_words() {
        assert_eq!(estimate_tokens("one two three"), 3);
        assert_eq!(estimate_tokens("   "), 1);
    }

    #[test]
    fn http_fake_server_meets_ci_floors() {
        let fake = super::super::local_sidecar::fake_server::FakeServer::start(None);
        let rt = super::super::local_http::HttpLocalRuntime::new(fake.base_url(), fake.token.clone());
        let hw = Hardware { gpu: String::new(), vram_mb: 0, ram_mb: 4_096, free_disk_mb: 10_000 };
        let bench = run_bench(&rt, &hw, 5, "fake-server").expect("bench");
        let eval = run_eval(&rt);
        assert_eq!(eval.passed(), 4, "{}", eval.summary_lines());
        check_ci_regression(&bench, &eval).expect("ci floors");
        assert!(fake.completion_hits() >= 4);
    }

    #[test]
    fn ci_regression_rejects_slow_first_token() {
        let bench = BenchReport {
            tokens_per_sec: 10.0,
            first_token_ms: CI_MAX_FIRST_TOKEN_MS + 1,
            load_ms: 1,
            ram_peak_mb: 1,
            vram_peak_mb: 0,
            completion_tokens: 2,
            backend: "x".into(),
        };
        let eval = EvalReport {
            items: vec![EvalItemResult { name: "chat", pass: true, detail: "ok".into() }],
        };
        assert!(check_ci_regression(&bench, &eval).unwrap_err().contains("first_token_ms"));
    }
}

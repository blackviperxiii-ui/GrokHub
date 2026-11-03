//! Live eval: real tasks against a real Grok model, under a hard dollar cap.
//!
//! GROKHUB_EVAL_API_KEY=... cargo run -p grokhub-agent --example live_eval -- \
//!     --budget-usd 1.00 [--k 3] [--model grok-4.3] [--effort low] [--only recall] [--out report.md]
//!
//! Run it before changing a default; it is never part of pull-request CI.

use std::sync::Arc;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut budget = None;
    let mut opts = grokhub_agent::LiveOpts {
        model: grokhub_agent::DEFAULT_MODEL.to_string(),
        effort: None,
        k: grokhub_agent::LIVE_DEFAULT_K,
        only: None,
    };
    let mut out = None;
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--budget-usd" => budget = Some(value),
            "--k" => {
                opts.k = value
                    .parse()
                    .unwrap_or_else(|_| fail("--k takes a whole number"))
            }
            "--model" => opts.model = value,
            "--effort" => opts.effort = Some(value),
            "--only" => opts.only = Some(value),
            "--out" => out = Some(value),
            other => fail(&format!("unknown argument: {other}")),
        }
        i += 2;
    }
    let Some(budget) = budget else {
        fail("--budget-usd is required (at most 5.00)");
    };
    let budget = grokhub_agent::parse_live_budget(&budget).unwrap_or_else(|e| fail(&e));
    let key = std::env::var("GROKHUB_EVAL_API_KEY").unwrap_or_default();
    if key.trim().is_empty() {
        fail("GROKHUB_EVAL_API_KEY is not set");
    }
    let client = grokhub_agent::XaiClient::new(
        key.trim(),
        grokhub_agent::AuthKind::ApiKey,
        Duration::from_secs(180),
    );
    let report = grokhub_agent::run_live(Arc::new(client), &opts, budget);
    let md = grokhub_agent::render_live(&report);
    if let Some(path) = out {
        if let Err(err) = std::fs::write(&path, md.as_bytes()) {
            fail(&format!("could not write report: {err}"));
        }
    }
    print!("{md}");
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}

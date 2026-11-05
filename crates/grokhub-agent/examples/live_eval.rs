//! Live eval: real tasks against a real model, under a hard dollar cap, in the
//! app's own shape (chat prompt, Auto gate, no turn cap).
//!
//! GROKHUB_EVAL_API_KEY=... cargo run -p grokhub-agent --example live_eval -- \
//!     --budget-usd 1.00 [--k 3] [--model grok-4.3] [--effort low] [--only recall] \
//!     [--ab prefix_cache] [--mde 10] [--provider grok|openrouter|local] [--out report.md]
//!
//! `--provider openrouter` sends every call to `--provider-model` (an OpenRouter
//! model id) with the key in GROKHUB_EVAL_OPENROUTER_KEY. `--provider local`
//! sends them to the model server at GROKHUB_EVAL_LOCAL_URL (loopback only),
//! model `--provider-model`, token GROKHUB_EVAL_LOCAL_TOKEN.
//!
//! `--ab` compares the switch on and off task by task and reports billed
//! dollars per solved task with 95% intervals. `--mde` is the smallest cost
//! change, in percent, the run is meant to see (default 10); a run too small
//! or too noisy to see it says so instead of giving a verdict.
//!
//! Run it before changing a default; it is never part of pull-request CI.
//! It runs against a throwaway config folder, so your own rules, grants,
//! memory and logs are never read or written.

use std::sync::Arc;
use std::time::Duration;

use grokhub_agent::{
    live_ab_flag as ab_flag, LiveLocalClient as LocalClient,
    LiveOpenRouterClient as OpenRouterClient, LiveProvider as Provider,
};

fn main() {
    let scratch = scratch_config();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut budget = None;
    let mut opts = grokhub_agent::LiveOpts::new(grokhub_agent::DEFAULT_MODEL);
    let mut provider_model = None;
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
            "--ab" => {
                ab_flag(&value).unwrap_or_else(|e| fail(&e));
                opts.ab = Some(value);
            }
            "--mde" => {
                let pct: f64 = value
                    .parse()
                    .unwrap_or_else(|_| fail("--mde takes a percent, like 10"));
                if !(pct > 0.0 && pct < 100.0) {
                    fail("--mde must be between 0 and 100");
                }
                opts.mde = pct / 100.0;
            }
            "--provider" => opts.provider = Provider::parse(&value).unwrap_or_else(|e| fail(&e)),
            "--provider-model" => provider_model = Some(value),
            "--out" => out = Some(value),
            other => fail(&format!("unknown argument: {other}")),
        }
        i += 2;
    }
    let Some(budget) = budget else {
        fail("--budget-usd is required (at most 5.00)");
    };
    let budget = grokhub_agent::parse_live_budget(&budget).unwrap_or_else(|e| fail(&e));
    let client: Arc<dyn grokhub_agent::ModelClient + Send + Sync> = match opts.provider {
        Provider::Grok => Arc::new(grokhub_agent::XaiClient::new(
            env("GROKHUB_EVAL_API_KEY"),
            grokhub_agent::AuthKind::ApiKey,
            Duration::from_secs(180),
        )),
        Provider::OpenRouter => {
            let model = provider_model
                .unwrap_or_else(|| fail("--provider openrouter needs --provider-model"));
            Arc::new(OpenRouterClient::new(
                env("GROKHUB_EVAL_OPENROUTER_KEY"),
                model,
            ))
        }
        Provider::Local => {
            let model =
                provider_model.unwrap_or_else(|| fail("--provider local needs --provider-model"));
            let token = std::env::var("GROKHUB_EVAL_LOCAL_TOKEN").unwrap_or_default();
            Arc::new(LocalClient::new(
                &env("GROKHUB_EVAL_LOCAL_URL"),
                token.trim(),
                &model,
            ))
        }
    };
    let report = grokhub_agent::run_live(client, &opts, budget);
    let md = grokhub_agent::render_live(&report);
    if let Some(path) = out {
        if let Err(err) = std::fs::write(&path, md.as_bytes()) {
            fail(&format!("could not write report: {err}"));
        }
    }
    print!("{md}");
    let _ = std::fs::remove_dir_all(&scratch);
}

/// Points `GROKHUB_CONFIG` at a fresh empty folder, before any thread starts,
/// so every thread of the run (subagents included) stays out of the real one.
fn scratch_config() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("grokhub-live-eval-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        fail(&format!("could not make a scratch config folder: {err}"));
    }
    // SAFETY: called first thing in `main`, while this is the only thread.
    unsafe { std::env::set_var("GROKHUB_CONFIG", &dir) };
    dir
}

fn env(name: &str) -> String {
    let value = std::env::var(name).unwrap_or_default();
    if value.trim().is_empty() {
        fail(&format!("{name} is not set"));
    }
    value.trim().to_string()
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}

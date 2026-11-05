//! Live eval: real tasks against a real model, under a hard dollar cap, in the
//! app's own shape (chat prompt, Auto gate, no turn cap).
//!
//! GROKHUB_EVAL_API_KEY=... cargo run -p grokhub-agent --example live_eval -- \
//!     --budget-usd 1.00 [--k 3] [--model grok-4.3] [--effort low] [--only recall] \
//!     [--ab prefix_cache] [--provider grok|openrouter|local] [--out report.md]
//!
//! `--provider openrouter` sends every call to `--provider-model` (an OpenRouter
//! model id) with the key in GROKHUB_EVAL_OPENROUTER_KEY. `--provider local`
//! sends them to the model server at GROKHUB_EVAL_LOCAL_URL (loopback only),
//! model `--provider-model`, token GROKHUB_EVAL_LOCAL_TOKEN.
//!
//! Run it before changing a default; it is never part of pull-request CI.

use std::sync::Arc;
use std::time::Duration;

use grokhub_agent::{
    live_ab_flag as ab_flag, LiveLocalClient as LocalClient,
    LiveOpenRouterClient as OpenRouterClient, LiveProvider as Provider,
};

fn main() {
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

//! Live eval: real tasks against a real model, under a hard dollar cap, in the
//! app's own shape (chat prompt, Auto gate, no turn cap).
//!
//! GROKHUB_EVAL_API_KEY=... cargo run -p grokhub-agent --example live_eval -- \
//!     --budget-usd 1.00 [--k 3] [--model grok-4.3] [--effort low] [--only recall] \
//!     [--ab prefix_cache] [--mde 10] [--provider grok|openrouter|local] [--out report.md]
//!
//! `--auth signin` (Grok only) uses the app's saved "Sign in with Grok"
//! (the `xai-oauth` keychain entry, billed to the SuperGrok pool) instead of
//! GROKHUB_EVAL_API_KEY. It only reads that entry and never refreshes it, so
//! a token near expiry stops the run: open the app once so it renews the
//! sign-in, then rerun. `--auth key` (the default) keeps the API key.
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
//! `--ab dream_lessons` tests the lessons the nightly dream wrote: the on arm
//! gets a copy of them, the off arm none. The app keeps those lessons out of
//! chats until a run like this passes them (on costs less solving no fewer,
//! or solves more), so a pass is the one thing this eval writes to your own
//! config: a `PASSED` mark beside the lessons, for exactly the lessons it
//! tested. A lesson added during the run means no mark.
//!
//! Run it before changing a default; it is never part of pull-request CI.
//! Apart from that mark it runs against a throwaway config folder, so your
//! own rules, grants, memory and logs are never read or written.

use std::sync::Arc;
use std::time::Duration;

use grokhub_agent::{
    live_ab_flag as ab_flag, LiveLocalClient as LocalClient,
    LiveOpenRouterClient as OpenRouterClient, LiveProvider as Provider,
};

fn main() {
    // Read before the scratch folder takes over GROKHUB_CONFIG.
    let lessons = grokhub_agent::dream_gate::skill_dir(&grokhub_agent::perm::config_dir());
    let scratch = scratch_config();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut budget = None;
    let mut opts = grokhub_agent::LiveOpts::new(grokhub_agent::DEFAULT_MODEL);
    let mut provider_model = None;
    let mut out = None;
    let mut signin = false;
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
            "--auth" => {
                signin = match value.as_str() {
                    "signin" => true,
                    "key" => false,
                    _ => fail("--auth takes signin or key"),
                }
            }
            other => fail(&format!("unknown argument: {other}")),
        }
        i += 2;
    }
    let Some(budget) = budget else {
        fail("--budget-usd is required (at most 5.00)");
    };
    let budget = grokhub_agent::parse_live_budget(&budget).unwrap_or_else(|e| fail(&e));
    if signin && opts.provider != Provider::Grok {
        fail("--auth signin works with --provider grok only");
    }
    let client: Arc<dyn grokhub_agent::ModelClient + Send + Sync> = match opts.provider {
        Provider::Grok if signin => Arc::new(grokhub_agent::XaiClient::new(
            signin_access(),
            grokhub_agent::AuthKind::OAuth,
            Duration::from_secs(180),
        )),
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
    let tested = std::fs::read_to_string(lessons.join("SKILL.md")).ok();
    if opts.ab.as_deref() == Some("dream_lessons") {
        if tested.is_none() {
            fail("--ab dream_lessons: there are no dream lessons to test yet");
        }
        opts.lessons = Some(lessons.clone());
    }
    let report = grokhub_agent::run_live(client, &opts, budget);
    if grokhub_agent::dream_lessons_passed(&report) {
        let now = std::fs::read_to_string(lessons.join("SKILL.md")).ok();
        if now.is_some() && now == tested {
            match grokhub_agent::dream_gate::mark_passed(&lessons) {
                Ok(()) => eprintln!("dream lessons passed: they now show in chats"),
                Err(err) => eprintln!("dream lessons passed but the mark failed: {err}"),
            }
        } else {
            eprintln!("dream lessons passed, but they changed during the run: not marked");
        }
    }
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

/// Read-only view of the app's keychain sign-in. Saving and deleting are
/// refused so the eval can never rotate or drop the app's tokens.
struct ReadOnlyKeychain;

impl grokhub_core::OAuthAccountStore for ReadOnlyKeychain {
    fn load_account(&self, account: &str) -> Result<Option<grokhub_core::ImagineTokens>, String> {
        let entry = keyring::Entry::new("GrokHub", account).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(raw) => serde_json::from_str(&raw)
                .map(Some)
                .map_err(|_| "the saved sign-in could not be read".to_string()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn save_account(&self, _: &str, _: &grokhub_core::ImagineTokens) -> Result<(), String> {
        Err("live eval never writes the keychain".into())
    }

    fn delete_account(&self, _: &str) -> Result<(), String> {
        Err("live eval never writes the keychain".into())
    }
}

/// The saved sign-in's access token, or a plain reason to stop. Never printed.
fn signin_access() -> String {
    let tokens = match grokhub_core::load_xai_oauth(&ReadOnlyKeychain) {
        Ok(Some(tokens)) => tokens,
        Ok(None) => fail("--auth signin: no saved sign-in; use Sign in with Grok in the app first"),
        Err(err) => fail(&format!(
            "--auth signin: could not read the keychain: {err}"
        )),
    };
    let now = grokhub_core::now_ms();
    if tokens.access_token.trim().is_empty()
        || !grokhub_core::imagine_access_usable(&tokens, now)
        || grokhub_core::imagine_needs_refresh(&tokens, now)
    {
        fail("--auth signin: the saved sign-in is expired or about to expire; open the app once so it renews, then rerun");
    }
    tokens.access_token.trim().to_string()
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

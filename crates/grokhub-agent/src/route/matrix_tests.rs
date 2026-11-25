//! What Auto picks for everyday asks across real setups: a Grok plan, a
//! Claude key, both, a loaded device model beside the plan, and the device
//! alone. Each case runs the real `live::decide` over a config folder, so
//! the ask read, the ladder, sizing and the scorer all have their say.

use std::sync::Arc;

use serde_json::{json, Value};

use grokhub_core::model_registry::profile::{
    write_profile, EffortTiming, ModelProfile, ProbeResult,
};
use grokhub_core::model_registry::store::save_registry;
use grokhub_core::model_registry::{Credential, Listing, ModelMeta, Registry, SourceKind};

use super::live::{self, RouteCall};
use super::local::{self, ForceOn, LocalRuntime};
use super::providers::{self, add_provider, MemoryVault, CALL_DATA};
use crate::harness::{grant_destination, test_dir, UserClick};

const NOW: u64 = 1_000_000;
const KEY: &str = "sk-abcdefghijklmnopqrstuv";

struct Rt;
impl LocalRuntime for Rt {
    fn classify(&self, _t: &str, l: &[&str]) -> Result<Value, String> {
        Ok(json!({ "label": l[0], "score": 0.9 }))
    }
    fn summarize(&self, _t: &str, _m: u32) -> Result<Value, String> {
        Ok(json!({ "summary": "x" }))
    }
    fn difficulty(&self, _t: &str) -> Result<Value, String> {
        Ok(json!({ "d": 0.4 }))
    }
    fn chat_stream(
        &self,
        _m: &[local::ChatMessage],
        d: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        d("x");
        Ok("x".into())
    }
}

fn xai(id: &str) -> ModelMeta {
    let reasoning = !id.contains("non-reasoning") && !id.contains("build");
    ModelMeta {
        id: id.into(),
        efforts: reasoning
            .then(|| vec!["low".into(), "medium".into(), "high".into(), "xhigh".into()]),
        tool_calling: Some(true),
        structured_output: Some(true),
        api_shape: Some("responses".into()),
        ..ModelMeta::default()
    }
}

fn an(id: &str) -> ModelMeta {
    ModelMeta {
        id: format!("anthropic/{id}"),
        efforts: Some(vec![
            "low".into(),
            "medium".into(),
            "high".into(),
            "xhigh".into(),
        ]),
        tool_calling: Some(true),
        ..ModelMeta::default()
    }
}

fn probe() -> ProbeResult {
    ProbeResult {
        status: "ok".into(),
        tool_call: Some(true),
        json_mode: Some(true),
        caching: Some(true),
        cached_tokens: Some(768),
        efforts: vec![EffortTiming {
            effort: "medium".into(),
            measured: "probe".into(),
            reply_ms: Some(900),
            tokens_per_sec: Some(40),
        }],
        ..ProbeResult::default()
    }
}

const GROK: &[&str] = &[
    "grok-4.7",
    "grok-4.3",
    "grok-4.20-0309-reasoning",
    "grok-4.20-0309-non-reasoning",
    "grok-build-0.1",
];

fn setup(label: &str, grok: bool, claude: bool, with_local: bool) -> std::path::PathBuf {
    let dir = test_dir(label);
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let mut reg = Registry::default();
    let mut listings = Vec::new();
    if grok {
        reg.entitlement.credential = Credential::Plan;
        let mut l = Listing::new(SourceKind::XaiApi, GROK.iter().map(|i| xai(i)).collect());
        l.tier = Some("SuperGrok Heavy".into());
        listings.push(l);
    }
    if claude {
        add_provider(&dir, "https://api.anthropic.com", KEY, NOW).unwrap();
        grant_destination(
            &dir,
            "api.anthropic.com",
            CALL_DATA,
            UserClick::from_click(),
        )
        .unwrap();
        listings.push(Listing::new(
            SourceKind::Anthropic,
            ["claude-opus-5.5", "claude-sonnet-5.5", "claude-haiku-5.5"]
                .iter()
                .map(|i| an(i))
                .collect(),
        ));
    }
    if with_local {
        listings.push(Listing::new(
            SourceKind::Local,
            local::TIERS.iter().map(|t| local::tier_meta(t)).collect(),
        ));
    }
    reg.apply_refresh(&listings, &[], 1);
    save_registry(&dir, &reg).unwrap();
    for id in reg.models.keys() {
        if let Some(meta) = reg.get(id).map(|r| r.meta.clone()) {
            let mut p = ModelProfile::build(&meta, probe(), 1);
            let _ = write_profile(&dir, &mut p);
        }
    }
    super::table::rebuild(&dir, None, true, NOW);
    dir
}

/// One routed ask: the model, the effort and the rules that fired.
struct Picked {
    model: String,
    effort: Option<String>,
    rules: Vec<String>,
}

impl Picked {
    fn has(&self, rule: &str) -> bool {
        self.rules.iter().any(|r| r == rule)
    }
}

/// Route `text` once in a fresh episode the accuracy guard leaves alone.
fn pick(
    dir: &std::path::Path,
    grok: bool,
    label: &str,
    text: &str,
    ctx: u64,
    image: bool,
) -> Picked {
    let mut ep = format!("{label}-ep");
    while super::guard::in_holdout(&ep) {
        ep.push('x');
    }
    let c = RouteCall {
        provider: "xai",
        class: "chat:default",
        model: if grok { "grok-4.3" } else { "" },
        episode: &ep,
        session: &ep,
        text,
        ctx_tokens: ctx,
        needs_image: image,
        needs_tools: true,
        providers: true,
        ..RouteCall::default()
    };
    let d = live::decide(dir, &c, NOW);
    Picked {
        model: d.route.model,
        effort: d.route.effort,
        rules: d.route.rule_ids,
    }
}

const DESIGN: &str =
    "design a database schema and API for a multiplayer game lobby with matchmaking";
const PROOF: &str = "prove that the square root of 2 is irrational";

#[test]
fn a_typo_fix_on_a_named_line_is_a_small_edit_not_debugging() {
    let r = super::ask::read(
        "fix the typo on line 3 of README.md",
        &super::ask::AskContext::default(),
    );
    assert_eq!((r.lane(), r.effort_name()), ("code:edit-small", "medium"));
    // A real trace still reads as debugging.
    let t = super::ask::read(
        "Traceback (most recent call last):\n  File \"bot.py\", line 12, in <module>\nKeyError: 'token'",
        &super::ask::AskContext::default(),
    );
    assert_eq!(t.lane(), "debug");
    let dir = setup("m-typo", true, false, false);
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    let p = pick(
        &dir,
        true,
        "m-typo",
        "fix the typo on line 3 of README.md",
        4_000,
        false,
    );
    assert_eq!(
        (p.model.as_str(), p.effort.as_deref()),
        ("grok-4.20-0309-reasoning", Some("medium")),
        "{:?}",
        p.rules
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn with_only_a_claude_key_hard_asks_get_the_strong_model_and_easy_ones_the_cheap_one() {
    let dir = setup("m-claude", false, true, false);
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    for (label, text) in [("m-claude-design", DESIGN), ("m-claude-proof", PROOF)] {
        let p = pick(&dir, false, label, text, 2_000, false);
        assert_eq!(
            p.model, "anthropic/claude-sonnet-5.5",
            "{text}: {:?}",
            p.rules
        );
        assert_eq!(p.effort.as_deref(), Some("high"));
    }
    let hi = pick(&dir, false, "m-claude-hi", "hi", 800, false);
    assert_eq!(hi.model, "anthropic/claude-haiku-5.5", "{:?}", hi.rules);
    let story = pick(
        &dir,
        false,
        "m-claude-story",
        "write me a short story about a dragon who learns to cook",
        900,
        false,
    );
    assert_eq!(
        story.model, "anthropic/claude-haiku-5.5",
        "{:?}",
        story.rules
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_loaded_device_model_takes_light_chat_and_leaves_hard_work_to_the_plan() {
    let dir = setup("m-warm", true, false, true);
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    let _on = ForceOn::with(Some(Arc::new(Rt)));
    for (label, text) in [
        ("m-warm-hi", "hi"),
        ("m-warm-fact", "what's the capital of France?"),
        (
            "m-warm-story",
            "write me a short story about a dragon who learns to cook",
        ),
    ] {
        let p = pick(&dir, true, label, text, 900, false);
        assert_eq!(p.model, "local:small", "{text}: {:?}", p.rules);
        assert!(p.has("size:warm_local"), "{text}: {:?}", p.rules);
    }
    for (label, text) in [
        ("m-warm-design", DESIGN),
        ("m-warm-proof", PROOF),
        (
            "m-warm-hard",
            "think hard: which sorting algorithm should I use for nearly sorted data?",
        ),
        ("m-warm-typo", "fix the typo on line 3 of README.md"),
    ] {
        let p = pick(&dir, true, label, text, 2_000, false);
        assert!(
            p.model.starts_with("grok-"),
            "{text}: {} {:?}",
            p.model,
            p.rules
        );
    }
    // The device can't see the picture, so the warm default doesn't apply
    // and nothing says the picture moved the turn.
    let pic = pick(
        &dir,
        true,
        "m-warm-pic",
        "what's in this picture?",
        2_000,
        true,
    );
    assert!(pic.model.starts_with("grok-"), "{:?}", pic.rules);
    assert!(!pic.has("local:no_vision"), "{:?}", pic.rules);
    // A long paste stays on the plan.
    let long = pick(
        &dir,
        true,
        "m-warm-long",
        "summarize this log and tell me what went wrong",
        70_000,
        false,
    );
    assert!(long.model.starts_with("grok-"), "{:?}", long.rules);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn without_a_loaded_device_model_light_chat_stays_on_the_plan() {
    let dir = setup("m-cold", true, false, true);
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    let p = pick(&dir, true, "m-cold-hi", "hi", 800, false);
    assert!(p.model.starts_with("grok-"), "{:?}", p.rules);
    assert!(!p.has("size:warm_local"), "{:?}", p.rules);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_debugging_question_typed_in_chat_picks_the_device_model_for_code() {
    use grokhub_core::local_skills::TaskKind;
    let debug = "cargo build fails with error[E0502]: cannot borrow `x` as mutable because it is also borrowed as immutable";
    assert_eq!(
        local::turn_kind("chat:default", 40, debug, false),
        Some(TaskKind::Code)
    );
    assert_eq!(
        local::turn_kind("chat:default", 40, PROOF, false),
        Some(TaskKind::Reason)
    );
    assert_eq!(
        local::turn_kind("chat:default", 10, "hi", false),
        Some(TaskKind::Chat)
    );
    assert_eq!(
        local::turn_kind("background:summarize", 10, debug, false),
        None
    );
}

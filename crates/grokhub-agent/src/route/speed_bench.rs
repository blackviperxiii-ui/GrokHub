//! Speed bench (#28): a chat turn, a 5-step agent task, a 5-step desktop
//! episode and a Grok Build turn,
//! each with a stub model, on a config folder shaped like a busy cabin's (a
//! sealed consent ledger and egress log, a long model-call log, outcomes, a
//! provider with a key and a grant). Prints p50 and p95 for every speed span.
//!
//! `cargo test -p grokhub-agent --release --lib speed_bench -- --ignored --nocapture --test-threads=1`

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use grokhub_core::model_registry::profile::write_profile;
use grokhub_core::model_registry::store::save_registry;
use grokhub_core::outcome::{append_outcome, topic_signature, OutcomeResult, TaskOutcome};

use super::budget;
use super::log::{Chosen, RouteOutcome, RouteRecord, RouteTokens, ROUTE_TRACE};
use super::providers::{add_provider, use_vault_for, MemoryVault, CALL_DATA};
use super::r2a_tests::{fleet, top_first};
use super::table::table_path;
use super::live;
use crate::client::{ClientError, ContentPart, FunctionCall, InputItem, ModelClient, ResponsesRequest, StreamEvent, TurnOutput, Usage};
use crate::gate::{ClosedPermits, Gate};
use crate::harness::{append_span, use_key_store_for, MemoryKeyStore, grant_destination, guard_egress, AccessMode, DataClass, EgressReq, GateOutcome, Origin, Span, UserClick, VERIFY_TOOL};
use crate::run::{run_loop, HaltCheck, LoopIn, SteerQueue};
use crate::timing::{self, Stat};
use crate::CancelToken;

const SESSION: &str = "bench-chat";
const ASK: &str = "summarize the release notes for the cabin and list what changed";

/// A config folder the size of a few weeks of use.
fn fixture(name: &str) -> PathBuf {
    let dir = crate::harness::test_dir(name);
    use_key_store_for(&dir, Arc::new(MemoryKeyStore::new()));
    use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let (reg, profiles) = fleet();
    save_registry(&dir, &reg).unwrap();
    for p in profiles.values() {
        write_profile(&dir, &mut p.clone()).unwrap();
    }
    let path = table_path(&dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_string(&top_first("grok-4.5")).unwrap()).unwrap();
    // 30 destination grants (sealed), and a provider with a key and a grant.
    for i in 0..30 {
        grant_destination(&dir, &format!("dest{i}.example.com"), &[DataClass::Chat], UserClick::from_click()).unwrap();
    }
    grant_destination(&dir, "openrouter.ai", CALL_DATA, UserClick::from_click()).unwrap();
    add_provider(&dir, "https://openrouter.ai/api/v1", "sk-abcdefghijklmnopqrstuv", 1).unwrap();
    // 3,000 sealed egress lines.
    let req = EgressReq::new(crate::RESPONSES_URL, &[DataClass::Chat]);
    for _ in 0..3_000 {
        assert!(guard_egress(&dir, &req).is_allow());
    }
    // 5,000 route records (about 4 MB of model-call log).
    for i in 0..5_000u64 {
        let mut span = Span::soft_allow(ROUTE_TRACE, "model", r#"{"model":"grok-4.5"}"#, "ok", "", AccessMode::Supervised, "xai");
        span.route = Some(Box::new(RouteRecord {
            episode: format!("ep-{}", i % 400),
            class: "chat:default".into(),
            chosen: Chosen { provider: "xai".into(), model: "grok-4.5".into(), effort: Some("medium".into()) },
            used: Chosen { provider: "xai".into(), model: "grok-4.5".into(), effort: Some("medium".into()) },
            rule_ids: vec!["table:chat:default".into(), "difficulty:mid".into()],
            reason: "the best balance for everyday chat in your plan".into(),
            outcome: Some(RouteOutcome { ok: true, verify: None, tokens: RouteTokens { input: 9_000, cached: 8_000, out: 400, reasoning: 200 }, cost_usd: 0.004, limit: false }),
            ..RouteRecord::default()
        }));
        append_span(&dir, &span).unwrap();
    }
    // 2,000 spans in the chat's own file, VerifyGate checks among them.
    for i in 0..2_000 {
        let tool = if i % 10 == 0 { VERIFY_TOOL } else { "read_file" };
        let mut span = Span::soft_allow(SESSION, tool, r#"{"path":"notes.md"}"#, if i % 20 == 0 { "fail" } else { "pass" }, "", AccessMode::Supervised, "native");
        span.episode = format!("ep-{}", i % 50);
        append_span(&dir, &span).unwrap();
    }
    // 300 task outcomes, some for the bench's own topic (DE2 reads them).
    let sig = topic_signature(ASK);
    for i in 0..300u64 {
        let signature = if i % 3 == 0 { sig.clone() } else { format!("topic-{i}") };
        let o = TaskOutcome { task: format!("ep-{}:1", i % 400), signature, result: OutcomeResult::Success, episode: format!("ep-{}", i % 400), finished_at: i, at: i, ..TaskOutcome::default() };
        append_outcome(&dir, &o).unwrap();
    }
    dir
}

/// Stands in for XaiClient: the same egress guard before it "sends", then a
/// fixed answer. `tool_steps` calls of `list_dir` come before the final text.
struct Stub {
    config_dir: PathBuf,
    calls: AtomicUsize,
    tool_steps: usize,
}

impl ModelClient for Stub {
    fn stream(&self, _req: &ResponsesRequest, _cancel: &CancelToken, sink: &mut dyn FnMut(StreamEvent)) -> Result<TurnOutput, ClientError> {
        let data = [DataClass::Chat, DataClass::Personal];
        if let GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } = guard_egress(&self.config_dir, &EgressReq::new(crate::RESPONSES_URL, &data)) {
            return Err(ClientError::Protocol(reason));
        }
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let usage = Usage { input_tokens: 9_000, cached_tokens: 8_000, output_tokens: 300, ..Usage::default() };
        if n % (self.tool_steps + 1) < self.tool_steps {
            let call = FunctionCall { call_id: format!("c{n}"), name: "list_dir".into(), arguments: format!(r#"{{"path":".","n":{n}}}"#) };
            return Ok(TurnOutput { text: String::new(), reasoning: String::new(), calls: vec![call], usage });
        }
        sink(StreamEvent::TextDelta("done".into()));
        Ok(TurnOutput { text: "done".into(), reasoning: String::new(), calls: Vec::new(), usage })
    }
}

struct NeverHalt;
impl HaltCheck for NeverHalt {
    fn halted(&self) -> bool {
        false
    }
}

fn chat_turn(stub: &Stub, i: usize) {
    let req = ResponsesRequest {
        model: "grok-4.7".into(),
        effort: None,
        input: vec![InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText(format!("{ASK} ({i})"))] }],
        conversation_id: SESSION.into(),
        tools: Vec::new(),
        hosted_search: true,
        call_timeout: None,
    };
    timing::note_enter();
    live::stream_routed(stub, &req, &CancelToken::new(), &mut |_| {}, live::DEFAULT_CLASS).unwrap();
}

fn agent_task(workspace: &Path, stub: &Stub, i: usize) {
    let cancel = CancelToken::new();
    let steer = SteerQueue::new();
    let conversation = format!("bench-agent-{i}");
    let input = LoopIn {
        client: stub,
        workspace,
        model: "grok-4.7",
        effort: None,
        system: "sys",
        conversation_id: &conversation,
        max_turns: 10,
        usage_base: Usage::default(),
        cancel: &cancel,
        steer: &steer,
        halt: &NeverHalt,
        gate: Gate::phase_readonly(),
        desktop: None,
        permits: &ClosedPermits,
        perms: None,
        context_length: 0,
        tasks: None,
        depth: 0,
        agent_id: None,
        shared_client: None,
        shared_permits: None,
        shared_desktop: None,
    };
    let mut history = Vec::new();
    run_loop(&input, &mut history, &format!("{ASK} ({i})"), None, &mut |_| {});
}

/// Five steps of a desktop episode through the router (the episode kernel's path).
fn desktop_episode(stub: &Stub, i: usize) {
    for step in 0..5 {
        let input = vec![InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText(format!("open the notes app and add item {i}.{step}"))] }];
        let mut call = super::ModelCall::xai("grok-4.7", None, "desktop:soft", &format!("ep-{}", i % 50), input);
        call.session = SESSION.into();
        super::call_model(stub, &call, &CancelToken::new()).unwrap();
    }
}

/// Run `n` of each and return every span's numbers.
fn bench(name: &str, n: usize) -> Vec<Stat> {
    let dir = fixture(name);
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    let workspace = dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("note.txt"), "hello\n").unwrap();
    // A first pass warms every cache, as a running cabin's would be.
    let chat = Stub { config_dir: dir.clone(), calls: AtomicUsize::new(0), tool_steps: 0 };
    let agent = Stub { config_dir: dir.clone(), calls: AtomicUsize::new(0), tool_steps: 4 };
    chat_turn(&chat, 0);
    agent_task(&workspace, &agent, 0);
    desktop_episode(&chat, 0);
    let (reg, _) = live::snapshot(&dir);
    timing::reset();
    for i in 1..=n {
        chat_turn(&chat, i);
        agent_task(&workspace, &agent, i);
        desktop_episode(&chat, i);
        timing::note_enter();
        live::route_gb_turn(&dir, "grok-4.7", false, None, "bench-gb", &format!("{ASK} ({i})"), Origin::User);
        // The week's spend is re-read once a minute: what the first send after that pays.
        budget::forget_week();
        timing::time("bench:week_budget_cold", || budget::load_week(&dir, &reg, grokhub_core::now_ms()));
    }
    let stats = timing::stats();
    let _ = std::fs::remove_dir_all(dir);
    stats
}

fn table(stats: &[Stat]) -> String {
    let mut out = format!("{:<30} {:>6} {:>10} {:>10} {:>10}\n", "span", "n", "p50 µs", "p95 µs", "max µs");
    for s in stats {
        out.push_str(&format!("{:<30} {:>6} {:>10} {:>10} {:>10}\n", s.name, s.n, s.p50_us, s.p95_us, s.max_us));
    }
    out
}

#[test]
fn the_bench_records_every_hot_path_span() {
    let stats = bench("speed-bench-smoke", 2);
    let n = |name: &str| stats.iter().find(|s| s.name == name).map_or(0, |s| s.n);
    // Two chat turns, two 5-step tasks and two Grok Build turns.
    assert_eq!(n("send:pre_request"), 12, "2 chat calls + 2 × 5 agent steps");
    assert_eq!(n("send:first_token"), 4, "only the final answers stream text");
    assert_eq!(n("send:enter_to_request"), 2);
    assert_eq!(n("send:enter_to_gb_route"), 2);
    assert_eq!(n("route:gb_turn"), 2);
    assert_eq!(n("route:decide"), 24, "12 sends, 2 × 5 desktop steps and 2 Grok Build turns");
    assert_eq!(n("route:log"), 24);
    assert_eq!(n("loop:step_gap"), 8, "4 gaps per 5-step task");
    assert_eq!(n("loop:tool_run"), 8);
    assert_eq!(n("harness:egress_guard"), 22, "one per stub send");
    for name in ["route:snapshot", "route:week_budget", "route:verify_counts", "route:ladder", "route:providers", "route:choose", "harness:consent_load", "harness:gate_decide_with", "harness:decide"] {
        assert!(n(name) > 0, "{name} recorded");
    }
}

#[test]
#[ignore = "timing bench; run it on purpose with --release (see the module doc)"]
fn speed_bench() {
    let stats = bench("speed-bench", 200);
    println!("{}", table(&stats));
}

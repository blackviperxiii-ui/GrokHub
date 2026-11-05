//! xAI server-side compaction against a loopback mock of `/v1/responses` and
//! `/v1/responses/compact`, through the real [`XaiClient`].

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::*;
use crate::client::{input_wire_value, AuthKind, XaiClient};
use crate::gate::{self, Gate};
use crate::run::{HaltCheck, LoopEvent, LoopIn, SteerQueue, StopReason};

/// The one compaction item the mock hands back, keys in xAI's own order.
const ITEM: &str = r#"{"encrypted_content":"opaque/blob+1==","type":"compaction","id":"cmp_1"}"#;

const COMPACT_OK: &str = r#"{"id":"cmp_1","object":"response.compaction","created_at":1767225600,"model":"grok-4.7","output":[{"encrypted_content":"opaque/blob+1==","type":"compaction","id":"cmp_1"}],"usage":{"input_tokens":1200,"input_tokens_details":{"cached_tokens":0},"output_tokens":80,"output_tokens_details":{"reasoning_tokens":5},"total_tokens":1280,"dropped_message_count":2}}"#;

#[derive(Clone)]
enum Reply {
    Json(u16, &'static str),
    /// 400 to a body with `prompt_cache_key`, else the compaction.
    KeyRefused,
    Sse(String),
    Hang(Duration),
}

#[derive(Debug, Clone)]
struct Seen {
    path: String,
    /// The request head, lowercased.
    head: String,
    body: String,
}

struct Mock {
    url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Mock {
    fn paths(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.path.clone())
            .collect()
    }

    fn body(&self, n: usize) -> Value {
        serde_json::from_str(&self.seen.lock().unwrap()[n].body).unwrap()
    }

    fn client(&self) -> XaiClient {
        XaiClient::new("test-bearer", AuthKind::ApiKey, Duration::from_secs(20))
            .with_loopback_url(&self.url)
            .unwrap()
    }
}

fn mock(compact: Reply, responses: Reply) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let (compact, responses, log) = (compact.clone(), responses.clone(), log.clone());
            std::thread::spawn(move || serve(stream, &compact, &responses, &log));
        }
    });
    Mock {
        url: format!("http://127.0.0.1:{port}/v1/responses"),
        seen,
    }
}

fn serve(mut stream: TcpStream, compact: &Reply, responses: &Reply, log: &Mutex<Vec<Seen>>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_ascii_lowercase();
    let len = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < head_end + len {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).into_owned();
    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
    let reply = if path == "/v1/responses/compact" {
        compact
    } else {
        responses
    };
    let keyed = body.contains("prompt_cache_key");
    log.lock().unwrap().push(Seen { path, head, body });
    let out = match reply {
        Reply::KeyRefused if keyed => {
            let text = r#"{"error":{"message":"unknown field prompt_cache_key"}}"#;
            format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            )
        }
        Reply::KeyRefused => format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{COMPACT_OK}",
            COMPACT_OK.len()
        ),
        Reply::Hang(wait) => {
            std::thread::sleep(*wait);
            return;
        }
        Reply::Json(status, text) => format!(
            "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
            if *status == 200 { "OK" } else { "Bad Request" },
            text.len()
        ),
        Reply::Sse(text) => format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{text}"
        ),
    };
    let _ = stream.write_all(out.as_bytes());
}

fn sse_reply(text: &str, input: u64, output: u64, cost: i64) -> Reply {
    let delta = json!({"type": "response.output_text.delta", "delta": text});
    let done = json!({"type": "response.completed", "response": {"usage": {
        "input_tokens": input, "output_tokens": output, "cost_in_usd_ticks": cost}}});
    Reply::Sse(format!("data: {delta}\n\ndata: {done}\n\n"))
}

fn summary_reply() -> Reply {
    sse_reply("<summary>kept the plan</summary>", 30, 6, 9)
}

fn msg(role: &str, text: &str) -> InputItem {
    InputItem::Message {
        role: role.into(),
        content: vec![ContentPart::InputText(text.into())],
    }
}

fn history() -> Vec<InputItem> {
    vec![
        msg("system", "SYS"),
        msg("user", "OLD_TOPIC"),
        msg("assistant", "old answer"),
        msg("user", "latest ask"),
        InputItem::FunctionCall {
            call_id: "c1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"a.rs"}"#.into(),
        },
        InputItem::FunctionCallOutput {
            call_id: "c1".into(),
            output: "fn main() {}".into(),
        },
    ]
}

/// A fresh config folder for this thread (route records, egress log, compaction.toml).
fn config(label: &str) -> (std::path::PathBuf, crate::perm::ConfigGuard) {
    let dir = std::env::temp_dir().join(format!(
        "gh-compact-server-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let guard = crate::perm::ConfigGuard::set(&dir);
    (dir, guard)
}

fn compact_with(client: &XaiClient, model: &str, items: &mut Vec<InputItem>) -> Compacted {
    compact_transcript(
        client,
        &CancelToken::new(),
        model,
        Some("low"),
        "conv-srv",
        items,
        &[],
        &[],
    )
    .expect("compaction")
}

/// What the pinned safety rules add to the estimate after a compaction.
fn pin_tokens() -> u64 {
    super::estimate_input_tokens(&[super::standing_rules_message(&[])])
}

#[test]
fn server_compaction_folds_all_but_the_tail_and_keeps_the_item_verbatim() {
    let (_dir, _guard) = config("ok");
    let server = mock(Reply::Json(200, COMPACT_OK), summary_reply());
    let client = server.client();
    let mut items = history();
    let done = compact_with(&client, "grok-4.7", &mut items);

    assert_eq!(server.paths(), vec!["/v1/responses/compact"]);
    assert_eq!(
        server.body(0),
        json!({"model": "grok-4.7", "prompt_cache_key": "conv-srv", "input": [
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "OLD_TOPIC"}]},
            {"type": "message", "role": "assistant", "content": [{"type": "input_text", "text": "old answer"}]},
        ]})
    );
    let head = server.seen.lock().unwrap()[0].head.clone();
    assert!(head.contains("authorization: bearer test-bearer"), "{head}");
    assert!(head.contains("x-grok-conv-id: conv-srv"), "{head}");
    assert!(head.contains("accept: application/json"), "{head}");

    let mut want = vec![
        msg("system", "SYS"),
        InputItem::ServerCompaction {
            raw: ITEM.to_string(),
            digest: "User: OLD_TOPIC\nAssistant: old answer".into(),
        },
        // The safety rules come back word for word after the folded part.
        super::standing_rules_message(&[]),
    ];
    want.extend(history()[3..].iter().cloned());
    assert_eq!(items, want);
    // The next turn sends the item back as it came, first after the system prompt.
    assert_eq!(
        input_wire_value(&items)[1],
        json!({"type": "compaction", "id": "cmp_1", "encrypted_content": "opaque/blob+1=="})
    );
    assert_eq!(
        done,
        Compacted {
            usage: Usage {
                input_tokens: 1200,
                output_tokens: 80,
                reasoning_tokens: 5,
                cost_in_usd_ticks: 0,
                cached_tokens: 0,
            },
            // Before: 95 bytes; after: 9 + 72 (the item) + 14 + 26 + 14 bytes. 4 bytes a token.
            server: Some(grokhub_core::wire::CompactDetail {
                tokens_before: 23,
                // What's left, pinned safety rules included.
                tokens_after: super::estimate_input_tokens(&want),
                dropped_message_count: 2,
            }),
        }
    );
}

#[test]
fn a_compacted_history_compacts_again_with_the_old_item_first() {
    let (_dir, _guard) = config("again");
    let server = mock(Reply::Json(200, COMPACT_OK), summary_reply());
    let client = server.client();
    let mut items = vec![
        InputItem::ServerCompaction {
            raw: r#"{"type":"compaction","id":"cmp_0","encrypted_content":"older=="}"#.into(),
            digest: "User: first ask".into(),
        },
        msg("assistant", "middle"),
        msg("user", "now"),
    ];
    compact_with(&client, "grok-4.7", &mut items);
    assert_eq!(
        server.body(0)["input"],
        json!([
            {"type": "compaction", "id": "cmp_0", "encrypted_content": "older=="},
            {"type": "message", "role": "assistant", "content": [{"type": "input_text", "text": "middle"}]},
        ])
    );
    assert_eq!(
        items,
        vec![
            InputItem::ServerCompaction {
                raw: ITEM.to_string(),
                // The older digest first, then what this compaction folded.
                digest: "User: first ask\nAssistant: middle".into(),
            },
            super::standing_rules_message(&[]),
            msg("user", "now")
        ]
    );
}

/// The endpoint failed: the same compaction goes the summary way, after
/// `compacts` tries at the endpoint.
fn assert_fell_back(server: &Mock, compacts: usize, items: &[InputItem], done: &Compacted) {
    let mut want = vec!["/v1/responses/compact".to_string(); compacts];
    want.push("/v1/responses".into());
    assert_eq!(server.paths(), want);
    assert!(server.seen.lock().unwrap()[compacts]
        .body
        .contains("faithful, concise summary"));
    assert_eq!(done.server, None);
    assert_eq!(done.usage.input_tokens, 30);
    assert_eq!(done.usage.cost_in_usd_ticks, 9);
    assert_eq!(items[0], msg("system", "SYS"));
    assert!(message_text(&items[1]).contains("kept the plan"));
    assert!(!items
        .iter()
        .any(|i| matches!(i, InputItem::ServerCompaction { .. })));
}

#[test]
fn an_http_400_from_the_endpoint_falls_back_to_the_summary() {
    let (_dir, _guard) = config("400");
    let server = mock(
        Reply::Json(
            400,
            r#"{"error":{"message":"model does not support compaction"}}"#,
        ),
        summary_reply(),
    );
    let mut items = history();
    let done = compact_with(&server.client(), "grok-4.7", &mut items);
    // Once with the cache key, once with the documented body only.
    assert_fell_back(&server, 2, &items, &done);
    assert!(server.body(0).get("prompt_cache_key").is_some());
    assert!(server.body(1).get("prompt_cache_key").is_none());
}

#[test]
fn a_refused_cache_key_is_dropped_and_not_sent_again() {
    let (_dir, _guard) = config("keyrefused");
    let server = mock(Reply::KeyRefused, summary_reply());
    let client = server.client();
    let mut items = history();
    let done = compact_with(&client, "grok-4.7", &mut items);
    assert!(
        done.server.is_some(),
        "the fold went through without the key"
    );
    assert_eq!(
        server.paths(),
        vec!["/v1/responses/compact", "/v1/responses/compact"]
    );
    assert_eq!(server.body(0)["prompt_cache_key"], "conv-srv");
    assert_eq!(keys(&server.body(1)), ["input", "model"]);

    // The same client remembers: the next fold sends the documented body once.
    let mut items = history();
    compact_with(&client, "grok-4.7", &mut items);
    assert_eq!(server.paths().len(), 3);
    assert_eq!(keys(&server.body(2)), ["input", "model"]);
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

#[test]
fn a_malformed_compaction_reply_falls_back_to_the_summary() {
    let (_dir, _guard) = config("malformed");
    let server = mock(
        Reply::Json(
            200,
            r#"{"object":"response.compaction","output":[{"type":"message""#,
        ),
        summary_reply(),
    );
    let mut items = history();
    let done = compact_with(&server.client(), "grok-4.7", &mut items);
    assert_fell_back(&server, 1, &items, &done);
}

#[test]
fn a_hung_compaction_call_times_out_and_falls_back_to_the_summary() {
    let (_dir, _guard) = config("hang");
    set_server_compact_timeout_for_tests(Some(Duration::from_millis(400)));
    let server = mock(Reply::Hang(Duration::from_secs(8)), summary_reply());
    let mut items = history();
    let started = Instant::now();
    let done = compact_with(&server.client(), "grok-4.7", &mut items);
    let took = started.elapsed();
    set_server_compact_timeout_for_tests(None);
    assert!(took < Duration::from_secs(4), "took {took:?}");
    assert_fell_back(&server, 1, &items, &done);
}

#[test]
fn forced_summary_and_non_xai_models_never_call_the_endpoint() {
    let (dir, _guard) = config("summary");
    std::fs::write(dir.join(COMPACTION_CONFIG), "backend = \"summary\"\n").unwrap();
    let server = mock(Reply::Json(200, COMPACT_OK), summary_reply());
    let mut items = history();
    let done = compact_with(&server.client(), "grok-4.7", &mut items);
    assert_eq!(server.paths(), vec!["/v1/responses"]);
    assert_eq!(done.server, None);
    std::fs::remove_file(dir.join(COMPACTION_CONFIG)).unwrap();

    for model in ["local:fast", "openrouter/qwen/qwen3-coder", "llama-3.3-70b"] {
        let server = mock(Reply::Json(200, COMPACT_OK), summary_reply());
        let mut items = history();
        let _ = compact_transcript(
            &server.client(),
            &CancelToken::new(),
            model,
            None,
            "conv-local",
            &mut items,
            &[],
            &[],
        );
        assert!(
            !server.paths().iter().any(|p| p == "/v1/responses/compact"),
            "{model}: {:?}",
            server.paths()
        );
    }
}

#[test]
fn backend_choice_and_the_config_file() {
    let (dir, _guard) = config("choice");
    assert_eq!(compact_backend(&dir, "grok-4.7"), CompactBackend::Server);
    assert_eq!(
        compact_backend(&dir, " grok-code-fast-1 "),
        CompactBackend::Server
    );
    assert_eq!(compact_backend(&dir, "local:fast"), CompactBackend::Summary);
    assert_eq!(
        compact_backend(&dir, "openrouter/x-ai/grok-4"),
        CompactBackend::Summary
    );
    assert_eq!(compact_backend(&dir, "gpt-5"), CompactBackend::Summary);
    std::fs::write(
        dir.join(COMPACTION_CONFIG),
        "# xAI server compaction off\nbackend = summary\n",
    )
    .unwrap();
    assert_eq!(compact_backend(&dir, "grok-4.7"), CompactBackend::Summary);
    std::fs::write(dir.join(COMPACTION_CONFIG), "backend = \"server\"\n").unwrap();
    assert_eq!(compact_backend(&dir, "grok-4.7"), CompactBackend::Server);
    assert_eq!(
        parse_compaction_file("backend = \"SUMMARY\" # off"),
        Some(CompactBackend::Summary)
    );
    assert_eq!(parse_compaction_file("other = 1"), None);
}

#[test]
fn server_tail_is_unbroken_and_never_opens_on_an_orphan_output() {
    let call = |id: &str| InputItem::FunctionCall {
        call_id: id.into(),
        name: "read_file".into(),
        arguments: "{}".into(),
    };
    let out = |id: &str| InputItem::FunctionCallOutput {
        call_id: id.into(),
        output: "ok".into(),
    };
    let items = vec![
        msg("user", "a"),
        msg("assistant", "b"),
        msg("user", "c"),
        call("1"),
        out("1"),
    ];
    assert_eq!(server_tail_start(&items), 2);
    // Two calls, then their outputs: a cut between them would orphan an output.
    let big = "x".repeat((RECENT_TAIL_TOKEN_BUDGET * 4) as usize);
    let parallel = vec![
        msg("user", "go"),
        call("1"),
        InputItem::FunctionCall {
            call_id: "2".into(),
            name: "read_file".into(),
            arguments: big,
        },
        out("1"),
        out("2"),
    ];
    assert_eq!(server_tail_start(&parallel), 5);
    // Everything fits: nothing older than the last user turn to fold.
    assert_eq!(server_tail_start(&[msg("user", "only")]), 0);
}

struct Never;
impl HaltCheck for Never {
    fn halted(&self) -> bool {
        false
    }
}

#[test]
fn auto_compact_at_the_threshold_uses_the_server_path() {
    let (dir, _guard) = config("auto");
    let compact = r#"{"object":"response.compaction","output":[{"type":"compaction","id":"cmp_9","encrypted_content":"E"}],"usage":{"input_tokens":900,"output_tokens":10,"dropped_message_count":1}}"#;
    let server = mock(Reply::Json(200, compact), sse_reply("done", 3, 1, 2));
    let client = server.client();
    let old = msg("user", &format!("OLD_TOPIC {}", "x".repeat(3_600)));
    let mut items = vec![old];
    let events = Mutex::new(Vec::new());
    let input = LoopIn {
        client: &client,
        workspace: &dir,
        model: "grok-4.7",
        effort: Some("low"),
        system: "",
        conversation_id: "conv-auto",
        max_turns: 4,
        usage_base: Usage::default(),
        cancel: &CancelToken::new(),
        steer: &SteerQueue::new(),
        halt: &Never,
        gate: Gate::phase_readonly(),
        desktop: None,
        permits: &gate::ClosedPermits,
        perms: None,
        context_length: 1_000,
        tasks: None,
        depth: 0,
        agent_id: None,
        shared_client: None,
        shared_permits: None,
        shared_desktop: None,
    };
    let out = crate::run_loop(&input, &mut items, "go", None, &mut |ev| {
        if let LoopEvent::Compact {
            started: false,
            error,
            server,
            ..
        } = ev
        {
            events.lock().unwrap().push((error, server));
        }
    });
    assert_eq!(out.stop, StopReason::EndTurn);
    assert!(out.compacted);
    assert_eq!(
        server.paths(),
        vec!["/v1/responses/compact", "/v1/responses"]
    );
    let turn = server.body(1);
    assert_eq!(
        turn["input"][0],
        json!({"type": "compaction", "id": "cmp_9", "encrypted_content": "E"})
    );
    assert!(turn["input"][1]["content"][0]["text"]
        .as_str()
        .unwrap()
        .starts_with("<standing_rules>"));
    assert_eq!(turn["input"][2]["role"], "user");
    assert_eq!(turn["input"][2]["content"][0]["text"], "go");
    assert_eq!(out.usage.input_tokens, 903);
    assert_eq!(out.usage.output_tokens, 11);
    assert_eq!(
        events.into_inner().unwrap(),
        vec![(
            None,
            Some(grokhub_core::wire::CompactDetail {
                tokens_before: 905,
                tokens_after: 16 + pin_tokens(),
                dropped_message_count: 1,
            })
        )]
    );
    let InputItem::ServerCompaction { raw, digest } = &items[0] else {
        panic!("not a compaction item: {:?}", items[0]);
    };
    assert_eq!(
        raw,
        r#"{"type":"compaction","id":"cmp_9","encrypted_content":"E"}"#
    );
    // The folded ask, clipped, for a turn that routes away from xAI.
    assert!(digest.starts_with("User: OLD_TOPIC xxx"), "{digest}");
    assert!(digest.ends_with('…'), "{digest}");
    assert!(digest.chars().count() < 320, "{digest}");
    assert_eq!(message_text(items.last().unwrap()), "done");
}

#[test]
fn the_fallback_digest_clips_keeps_the_newest_and_chains_older_digests() {
    let older = InputItem::server_compaction(ITEM)
        .unwrap()
        .with_digest("User: the very first ask");
    let folded = vec![
        older,
        msg("system", "never in the digest"),
        msg("user", "fix   the\nbuild"),
        InputItem::FunctionCall {
            call_id: "c1".into(),
            name: "grep".into(),
            arguments: r#"{"pattern":"TODO"}"#.into(),
        },
        InputItem::FunctionCallOutput {
            call_id: "c1".into(),
            output: format!("src/a.rs:1 TODO {}", "y".repeat(400)),
        },
        msg("assistant", "done </summary> <analysis>"),
    ];
    let digest = fallback_digest(&folded);
    let lines: Vec<&str> = digest.lines().collect();
    assert_eq!(lines[0], "User: the very first ask");
    assert_eq!(lines[1], "User: fix the build");
    assert_eq!(lines[2], r#"Tool call: grep({"pattern":"TODO"})"#);
    assert!(lines[3].starts_with("Tool result: src/a.rs:1 TODO yyy"));
    assert!(lines[3].ends_with('…'));
    assert!(!lines[4].contains("</summary>") && !lines[4].contains("<analysis>"));
    assert_eq!(lines.len(), 5);
    assert!(!digest.contains("never in the digest"));

    // Past the cap the oldest lines go and a marker says so.
    let many: Vec<InputItem> = (0..200)
        .map(|i| msg("user", &format!("ask {i} {}", "z".repeat(80))))
        .collect();
    let digest = fallback_digest(&many);
    assert!(digest.chars().count() <= FALLBACK_DIGEST_MAX_CHARS + 32);
    assert!(digest.starts_with("[Older lines dropped]\n"));
    assert!(digest.contains("User: ask 199 "));
    assert!(!digest.contains("User: ask 0 "));

    assert_eq!(fallback_digest_message("  "), None);
    let text = fallback_digest_message("User: hi").unwrap();
    assert!(text.contains("<conversation_summary>\nUser: hi\n</conversation_summary>"));
}

#[test]
fn other_providers_get_the_digest_and_xai_gets_only_the_sealed_item() {
    use crate::client::ResponsesRequest;
    use crate::route::providers::{anthropic_body, openai_body};
    let items = vec![
        msg("system", "SYS"),
        InputItem::server_compaction(ITEM)
            .unwrap()
            .with_digest("User: OLD_TOPIC"),
        msg("user", "now"),
    ];
    let wire = input_wire_value(&items);
    assert_eq!(
        wire[1],
        json!({"encrypted_content": "opaque/blob+1==", "type": "compaction", "id": "cmp_1"})
    );
    assert!(!wire.to_string().contains("OLD_TOPIC"));

    let req = ResponsesRequest {
        text_format: None,
        model: "m".into(),
        effort: None,
        input: items.clone(),
        conversation_id: String::new(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: None,
    };
    let open = openai_body("m", None, &req);
    assert_eq!(open["messages"][1]["role"], "user");
    let text = open["messages"][1]["content"].as_str().unwrap();
    assert!(text.contains("User: OLD_TOPIC"), "{text}");
    assert_eq!(open["messages"][2]["content"], "now");

    let claude = anthropic_body("m", None, &req, None);
    let first = &claude["messages"][0];
    assert_eq!(first["role"], "user");
    assert!(first["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("User: OLD_TOPIC"));
    assert_eq!(first["content"][1]["text"], "now");

    // An item saved before digests existed still sends nothing to them.
    let bare = ResponsesRequest {
        input: vec![
            InputItem::server_compaction(ITEM).unwrap(),
            msg("user", "now"),
        ],
        ..req
    };
    let open = openai_body("m", None, &bare);
    assert_eq!(open["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn stop_during_a_hung_compaction_call_returns_at_once() {
    let (_dir, _guard) = config("stop");
    let server = mock(Reply::Hang(Duration::from_secs(8)), summary_reply());
    let client = server.client();
    let cancel = CancelToken::new();
    let stopper = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        stopper.cancel();
    });
    let req = CompactRequest {
        model: "grok-4.7".into(),
        input: history(),
        conversation_id: "conv-stop".into(),
        timeout: Duration::from_secs(60),
    };
    let started = Instant::now();
    let out = client.compact(&req, &cancel).expect("server compaction");
    let took = started.elapsed();
    assert!(matches!(out, Err(ClientError::Cancelled)), "{out:?}");
    assert!(took < Duration::from_secs(1), "took {took:?}");
    assert_eq!(server.paths(), vec!["/v1/responses/compact"]);
}

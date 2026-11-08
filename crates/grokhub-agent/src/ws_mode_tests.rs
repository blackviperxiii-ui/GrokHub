//! WebSocket mode against a loopback socket server that records each
//! `response.create` and answers from a script.

use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tungstenite::Message;

use super::*;

/// What the server does with one `response.create`.
enum Answer {
    /// Stream these events.
    Events(Vec<Value>),
    /// Close the socket without answering.
    Hang,
    /// Stream these events, then close the socket.
    EventsThenClose(Vec<Value>),
}

type Script = dyn Fn(usize, &Value) -> Answer + Send + Sync;

struct Server {
    url: String,
    /// (socket number, message) for every `response.create`, in order.
    got: Arc<Mutex<Vec<(usize, Value)>>>,
    sockets: Arc<AtomicUsize>,
}

impl Server {
    fn got(&self) -> Vec<(usize, Value)> {
        self.got.lock().unwrap().clone()
    }
}

fn server(script: impl Fn(usize, &Value) -> Answer + Send + Sync + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let got = Arc::new(Mutex::new(Vec::new()));
    let sockets = Arc::new(AtomicUsize::new(0));
    let script: Arc<Script> = Arc::new(script);
    let (log, count) = (got.clone(), sockets.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let n = count.fetch_add(1, Ordering::SeqCst);
            let (log, script) = (log.clone(), script.clone());
            std::thread::spawn(move || {
                let Ok(mut ws) = tungstenite::accept(stream) else {
                    return;
                };
                loop {
                    let text = match ws.read() {
                        Ok(Message::Text(t)) => t,
                        Ok(Message::Close(_)) | Err(_) => return,
                        Ok(_) => continue,
                    };
                    let message: Value = serde_json::from_str(text.as_str()).unwrap();
                    let seq = {
                        let mut log = log.lock().unwrap();
                        log.push((n, message.clone()));
                        log.len() - 1
                    };
                    match script(seq, &message) {
                        Answer::Events(events) => {
                            for ev in events {
                                if ws.send(Message::text(ev.to_string())).is_err() {
                                    return;
                                }
                            }
                        }
                        Answer::Hang => {
                            let _ = ws.close(None);
                            let _ = ws.flush();
                            return;
                        }
                        Answer::EventsThenClose(events) => {
                            for ev in events {
                                let _ = ws.send(Message::text(ev.to_string()));
                            }
                            let _ = ws.close(None);
                            let _ = ws.flush();
                            return;
                        }
                    }
                }
            });
        }
    });
    Server {
        url: format!("ws://127.0.0.1:{port}/v1/responses"),
        got,
        sockets,
    }
}

/// A finished reply `id` that says `text`.
fn reply(id: &str, text: &str) -> Answer {
    Answer::Events(vec![
        json!({"type": "response.created", "response": {"id": id}}),
        json!({"type": "response.output_text.delta", "delta": text}),
        json!({"type": "response.completed", "response": {"id": id,
            "usage": {"input_tokens": 10, "output_tokens": 2}, "output": []}}),
    ])
}

fn error(status: u16, code: &str) -> Answer {
    Answer::Events(vec![json!({
        "type": "error",
        "status": status,
        "error": {"code": code, "message": format!("{code} happened")},
    })])
}

fn user(text: &str) -> Value {
    json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]})
}

fn said(text: &str) -> Value {
    json!({"type": "message", "role": "assistant", "content": [{"type": "input_text", "text": text}]})
}

fn body(input: Vec<Value>) -> Value {
    json!({"model": "grok-test", "stream": true, "store": false, "input": input})
}

struct Turned {
    turn: WsTurn,
    text: String,
}

fn run(pool: &WsPool, chat: &str, body: &Value) -> Turned {
    run_with(pool, chat, body, &CancelToken::new())
}

fn run_with(pool: &WsPool, chat: &str, body: &Value, cancel: &CancelToken) -> Turned {
    let mut text = String::new();
    let turn = pool.turn(
        "test-bearer",
        AuthKind::ApiKey,
        chat,
        body,
        "grok-test",
        cancel,
        Duration::from_secs(5),
        &mut |ev| {
            if let StreamEvent::TextDelta(t) = ev {
                text.push_str(&t);
            }
        },
    );
    Turned { turn, text }
}

fn done_text(t: &Turned) -> &str {
    match &t.turn {
        WsTurn::Done(out) => &out.text,
        WsTurn::Failed(err) => panic!("turn failed: {err}"),
        WsTurn::Http => panic!("turn went to HTTP"),
    }
}

#[test]
fn a_follow_up_sends_only_the_new_items_on_the_same_socket() {
    let srv = server(|seq, _| reply(&format!("resp_{seq}"), "pong"));
    let pool = WsPool::new(&srv.url);
    let first = body(vec![user("ping")]);
    let t = run(&pool, "chat-a", &first);
    assert_eq!(done_text(&t), "pong");
    assert_eq!(t.text, "pong", "the reply streams to the chat as it comes");

    let second = body(vec![user("ping"), said("pong"), user("again")]);
    assert_eq!(done_text(&run(&pool, "chat-a", &second)), "pong");

    let got = srv.got();
    assert_eq!(got.len(), 2);
    let (s0, m0) = &got[0];
    let (s1, m1) = &got[1];
    assert_eq!(s0, s1, "one socket carries the chat");
    assert_eq!(srv.sockets.load(Ordering::SeqCst), 1);
    assert_eq!(m0["type"], "response.create");
    assert!(
        m0.get("stream").is_none(),
        "a socket message has no stream field"
    );
    assert!(m0.get("previous_response_id").is_none());
    assert_eq!(m0["input"], json!([user("ping")]));
    assert_eq!(m1["previous_response_id"], "resp_0");
    assert_eq!(
        m1["input"],
        json!([user("again")]),
        "only what came after the reply"
    );
    assert_eq!(m1["store"], false);
}

#[test]
fn a_rewritten_history_starts_a_fresh_chain_with_the_whole_input() {
    let srv = server(|seq, _| reply(&format!("resp_{seq}"), "ok"));
    let pool = WsPool::new(&srv.url);
    run(&pool, "chat-b", &body(vec![user("one")]));
    // A compaction or trim rewrote the start: the chain can't continue.
    let rewritten = body(vec![user("summary of one"), said("ok"), user("two")]);
    done_text(&run(&pool, "chat-b", &rewritten));
    // A new model on the same history also starts over.
    let mut other = body(vec![
        user("summary of one"),
        said("ok"),
        user("two"),
        said("ok"),
        user("three"),
    ]);
    other["model"] = json!("grok-other");
    done_text(&run(&pool, "chat-b", &other));

    let got = srv.got();
    assert!(got[1].1.get("previous_response_id").is_none());
    assert_eq!(got[1].1["input"], rewritten["input"]);
    assert!(got[2].1.get("previous_response_id").is_none());
    assert_eq!(got[2].1["input"], other["input"]);
}

#[test]
fn a_reply_with_turn_labels_still_chains_on_its_stored_form() {
    let labelled = "[Turn 3] pong";
    let srv = server(move |seq, _| reply(&format!("resp_{seq}"), labelled));
    let pool = WsPool::new(&srv.url);
    run(&pool, "chat-l", &body(vec![user("ping")]));
    let stored = grokhub_core::strip_turn_labels(labelled);
    run(
        &pool,
        "chat-l",
        &body(vec![user("ping"), said(&stored), user("again")]),
    );
    let got = srv.got();
    assert_eq!(got[1].1["previous_response_id"], "resp_0");
    assert_eq!(got[1].1["input"], json!([user("again")]));
}

#[test]
fn a_lost_chain_is_sent_again_in_full_on_the_same_turn() {
    let srv = server(|seq, _| {
        if seq == 1 {
            error(400, "previous_response_not_found")
        } else {
            reply(&format!("resp_{seq}"), "fine")
        }
    });
    let pool = WsPool::new(&srv.url);
    run(&pool, "chat-c", &body(vec![user("a")]));
    let next = body(vec![user("a"), said("fine"), user("b")]);
    assert_eq!(done_text(&run(&pool, "chat-c", &next)), "fine");

    let got = srv.got();
    assert_eq!(got.len(), 3);
    assert_eq!(got[1].1["previous_response_id"], "resp_0");
    assert!(
        got[2].1.get("previous_response_id").is_none(),
        "the retry starts over"
    );
    assert_eq!(got[2].1["input"], next["input"]);
}

#[test]
fn the_hour_limit_reopens_the_socket_and_resends_in_full() {
    let srv = server(|seq, _| {
        if seq == 1 {
            error(400, "websocket_connection_limit_reached")
        } else {
            reply(&format!("resp_{seq}"), "fine")
        }
    });
    let pool = WsPool::new(&srv.url);
    run(&pool, "chat-h", &body(vec![user("a")]));
    let next = body(vec![user("a"), said("fine"), user("b")]);
    assert_eq!(done_text(&run(&pool, "chat-h", &next)), "fine");
    let got = srv.got();
    assert_eq!(got.len(), 3);
    assert_ne!(got[1].0, got[2].0, "a new socket after the limit");
    assert_eq!(got[2].1["input"], next["input"]);
}

#[test]
fn a_socket_that_wont_open_hands_the_call_to_http_and_rests() {
    // Bind and drop: nothing listens on this port.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let pool = WsPool::new(format!("ws://127.0.0.1:{port}/v1/responses"));
    assert!(matches!(
        run(&pool, "chat-d", &body(vec![user("a")])).turn,
        WsTurn::Http
    ));
    assert!(pool.down(), "it rests after a failed open");
    assert!(matches!(
        run(&pool, "chat-d", &body(vec![user("a")])).turn,
        WsTurn::Http
    ));
}

#[test]
fn a_socket_closed_before_any_text_goes_to_http() {
    let srv = server(|_, _| Answer::Hang);
    let pool = WsPool::new(&srv.url);
    assert!(matches!(
        run(&pool, "chat-e", &body(vec![user("a")])).turn,
        WsTurn::Http
    ));
}

#[test]
fn a_refused_shape_goes_to_http_and_three_in_a_row_rest_the_socket() {
    let srv = server(|_, _| error(422, "invalid_request"));
    let pool = WsPool::new(&srv.url);
    for _ in 0..2 {
        assert!(matches!(
            run(&pool, "chat-f", &body(vec![user("a")])).turn,
            WsTurn::Http
        ));
        assert!(!pool.down());
    }
    assert!(matches!(
        run(&pool, "chat-f", &body(vec![user("a")])).turn,
        WsTurn::Http
    ));
    assert!(pool.down(), "three refusals in a row rest the socket path");
}

#[test]
fn a_rate_limit_is_the_turns_error_not_a_resend() {
    let srv = server(|_, _| error(429, "rate_limited"));
    let pool = WsPool::new(&srv.url);
    let t = run(&pool, "chat-g", &body(vec![user("a")]));
    assert!(
        matches!(t.turn, WsTurn::Failed(ClientError::RateLimited { .. })),
        "a 429 is reported as HTTP would"
    );
    assert_eq!(srv.got().len(), 1, "never sent twice");
}

#[test]
fn stop_drops_the_reply_and_the_socket() {
    let srv = server(|_, _| {
        std::thread::sleep(Duration::from_millis(600));
        reply("resp_0", "late")
    });
    let pool = WsPool::new(&srv.url);
    let cancel = CancelToken::new();
    let stopper = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        stopper.cancel();
    });
    let t = run_with(&pool, "chat-s", &body(vec![user("a")]), &cancel);
    assert!(matches!(t.turn, WsTurn::Failed(ClientError::Cancelled)));
    assert!(t.text.is_empty());
}

#[test]
fn each_chat_gets_its_own_socket() {
    let srv = server(|seq, _| reply(&format!("resp_{seq}"), "hi"));
    let pool = WsPool::new(&srv.url);
    run(&pool, "chat-1", &body(vec![user("a")]));
    run(&pool, "chat-2", &body(vec![user("a")]));
    assert_eq!(pool.open_count(), 2);
    assert_eq!(srv.sockets.load(Ordering::SeqCst), 2);
    // No chat id: HTTP, since there's nothing to key a socket on.
    assert!(matches!(
        run(&pool, " ", &body(vec![user("a")])).turn,
        WsTurn::Http
    ));
}

#[test]
fn websocket_toml_turns_it_off() {
    let dir = std::env::temp_dir().join(format!("grokhub-ws-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(enabled_at(&dir), "on by default");
    std::fs::write(dir.join(CONFIG_NAME), "enabled = false\n").unwrap();
    assert!(!enabled_at(&dir));
    std::fs::write(dir.join(CONFIG_NAME), "# enabled = false\nenabled = true\n").unwrap();
    assert!(enabled_at(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_client_tries_the_socket_first_and_falls_back_to_http() {
    let srv = server(|seq, _| reply(&format!("resp_{seq}"), "over the socket"));
    let client = crate::XaiClient::new("test-bearer", AuthKind::ApiKey, Duration::from_secs(20))
        .with_loopback_ws(&srv.url)
        .unwrap();
    let req = crate::client::ResponsesRequest {
        text_format: None,
        model: "grok-test".into(),
        effort: None,
        input: vec![InputItem::Message {
            role: "user".into(),
            content: vec![crate::client::ContentPart::InputText("ping".into())],
        }],
        conversation_id: "chat-x".into(),
        tools: vec![
            json!({"type": "function", "name": "read_file", "parameters": {"type": "object"}}),
        ],
        hosted_search: false,
        call_timeout: None,
    };
    let out = crate::ModelClient::stream(&client, &req, &CancelToken::new(), &mut |_| {}).unwrap();
    assert_eq!(out.text, "over the socket");
    assert_eq!(srv.got().len(), 1);
    assert!(
        crate::XaiClient::new("k", AuthKind::ApiKey, Duration::from_secs(1))
            .with_loopback_ws("ws://example.com/v1/responses")
            .is_err(),
        "only loopback sockets in tests"
    );
}

#[test]
fn a_side_call_goes_over_http_and_leaves_the_chats_chain_alone() {
    let srv = server(|seq, _| reply(&format!("resp_{seq}"), "pong"));
    // HTTP points at a closed loopback port: a side call fails fast there
    // instead of reaching the socket.
    let client = crate::XaiClient::new("test-bearer", AuthKind::ApiKey, Duration::from_secs(5))
        .with_loopback_url("http://127.0.0.1:9/v1/responses")
        .unwrap()
        .with_loopback_ws(&srv.url)
        .unwrap();
    let msg = |role: &str, text: &str| InputItem::Message {
        role: role.into(),
        content: vec![crate::client::ContentPart::InputText(text.into())],
    };
    let step = |input: Vec<InputItem>| crate::client::ResponsesRequest {
        text_format: None,
        model: "grok-test".into(),
        effort: None,
        input,
        conversation_id: "chat-s".into(),
        tools: vec![
            json!({"type": "function", "name": "read_file", "parameters": {"type": "object"}}),
        ],
        hosted_search: false,
        call_timeout: None,
    };
    let cancel = CancelToken::new();
    crate::ModelClient::stream(
        &client,
        &step(vec![msg("user", "ping")]),
        &cancel,
        &mut |_| {},
    )
    .unwrap();

    let mut judge = step(vec![msg("user", "is this safe?")]);
    judge.tools.clear();
    judge.call_timeout = Some(Duration::from_secs(2));
    assert!(crate::ModelClient::stream(&client, &judge, &cancel, &mut |_| {}).is_err());
    let mut brief = step(vec![msg("user", "classify this")]);
    brief.tools.clear();
    assert!(crate::ModelClient::stream(&client, &brief, &cancel, &mut |_| {}).is_err());

    let next = step(vec![
        msg("user", "ping"),
        msg("assistant", "pong"),
        msg("user", "again"),
    ]);
    crate::ModelClient::stream(&client, &next, &cancel, &mut |_| {}).unwrap();

    let got = srv.got();
    assert_eq!(got.len(), 2, "side calls never reach the chat's socket");
    assert_eq!(got[1].1["previous_response_id"], "resp_0");
}

/// A port that takes the TCP connection and then never answers the upgrade.
fn silent_port() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            held.push(stream);
        }
    });
    format!("ws://127.0.0.1:{port}/v1/responses")
}

#[test]
fn an_open_that_stalls_goes_to_http_within_the_deadline() {
    let pool = WsPool::new(silent_port());
    let started = std::time::Instant::now();
    let t = run(&pool, "chat-stall", &body(vec![user("a")]));
    let took = started.elapsed();
    assert!(matches!(t.turn, WsTurn::Http));
    assert!(
        took < CONNECT_DEADLINE + Duration::from_millis(500),
        "took {took:?}"
    );
    assert!(pool.down(), "a stalled open rests the socket path");
}

#[test]
fn stop_during_an_open_returns_at_once() {
    let pool = WsPool::new(silent_port());
    let cancel = CancelToken::new();
    let stopper = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        stopper.cancel();
    });
    let started = std::time::Instant::now();
    let t = run_with(&pool, "chat-stop-open", &body(vec![user("a")]), &cancel);
    let took = started.elapsed();
    assert!(matches!(t.turn, WsTurn::Failed(ClientError::Cancelled)));
    assert!(took < Duration::from_millis(400), "took {took:?}");
    assert!(!pool.down(), "a Stop is not a broken socket");
}

#[test]
fn a_drop_after_the_server_took_the_request_is_a_retry_not_an_http_resend() {
    let srv = server(|_, _| {
        Answer::EventsThenClose(vec![
            json!({"type": "response.created", "response": {"id": "resp_0"}}),
            json!({"type": "response.reasoning_summary_text.delta", "delta": "thinking"}),
        ])
    });
    let pool = WsPool::new(&srv.url);
    let t = run(&pool, "chat-taken", &body(vec![user("a")]));
    assert!(
        matches!(t.turn, WsTurn::Failed(ClientError::Disconnect)),
        "a taken request must not go to HTTP behind the retry budget"
    );
}

#[test]
fn a_socket_the_server_closed_between_turns_reopens_cleanly() {
    let srv = server(|seq, _| {
        if seq == 0 {
            match reply("resp_0", "one") {
                Answer::Events(events) => Answer::EventsThenClose(events),
                other => other,
            }
        } else {
            reply("resp_1", "two")
        }
    });
    let pool = WsPool::new(&srv.url);
    let first = run(&pool, "chat-closed", &body(vec![user("a")]));
    assert_eq!(done_text(&first), "one");
    std::thread::sleep(Duration::from_millis(200));
    let second = run(
        &pool,
        "chat-closed",
        &body(vec![user("a"), said("one"), user("b")]),
    );
    assert_eq!(done_text(&second), "two");
    assert_eq!(srv.sockets.load(Ordering::SeqCst), 2);
    // The new socket can't continue the old chain: the whole input goes.
    let got = srv.got();
    assert!(
        got[1].1.get("previous_response_id").is_none(),
        "{:?}",
        got[1].1
    );
}

/// A reply `id` that asks for one `read_file` call.
fn wants_read(id: &str, call_id: &str, file: &str) -> Answer {
    let item = json!({
        "type": "function_call",
        "call_id": call_id,
        "name": "read_file",
        "arguments": json!({"target_file": file}).to_string(),
    });
    Answer::Events(vec![
        json!({"type": "response.created", "response": {"id": id}}),
        json!({"type": "response.output_item.done", "item": item}),
        json!({"type": "response.completed", "response": {"id": id,
            "usage": {"input_tokens": 10, "output_tokens": 2}, "output": [item]}}),
    ])
}

/// Round 3 item 30: the real run loop over a socket chains every step. Two
/// tool steps and a final answer go out on one socket, steps 2 and 3 name the
/// step before them and carry only the new tool output.
#[test]
fn the_run_loop_chains_each_tool_step_on_the_socket() {
    let srv = server(|seq, _| match seq {
        0 => wants_read("resp_0", "c1", "one.txt"),
        1 => wants_read("resp_1", "c2", "two.txt"),
        _ => reply(&format!("resp_{seq}"), "all done"),
    });
    // Side calls (a judge, a close check) go over HTTP, to a closed port.
    let client = crate::XaiClient::new("test-bearer", AuthKind::ApiKey, Duration::from_secs(5))
        .with_loopback_url("http://127.0.0.1:9/v1/responses")
        .unwrap()
        .with_loopback_ws(&srv.url)
        .unwrap();
    let ws = std::env::temp_dir().join(format!("gh-ws-loop-{}", std::process::id()));
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("one.txt"), "first file says ALPHA").unwrap();
    std::fs::write(ws.join("two.txt"), "second file says BRAVO").unwrap();
    struct Never;
    impl crate::HaltCheck for Never {
        fn halted(&self) -> bool {
            false
        }
    }
    let chat = format!("ws-loop-{}", std::process::id());
    let input = crate::LoopIn {
        client: &client,
        workspace: &ws,
        model: "grok-test",
        effort: None,
        system: "",
        conversation_id: &chat,
        max_turns: 6,
        usage_base: crate::Usage::default(),
        cancel: &CancelToken::new(),
        steer: &crate::SteerQueue::new(),
        halt: &Never,
        gate: crate::gate::Gate {
            mode: crate::gate::PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop: false,
        },
        desktop: None,
        permits: &crate::gate::ClosedPermits,
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
    let mut text = String::new();
    let out = crate::run_loop(&input, &mut history, "read both files", None, &mut |ev| {
        if let crate::LoopEvent::Text(t) = ev {
            text.push_str(&t);
        }
    });
    assert_eq!(out.stop, crate::StopReason::EndTurn, "{text}");
    assert!(text.contains("all done"), "{text}");

    let got = srv.got();
    assert_eq!(got.len(), 3, "{got:?}");
    assert!(
        got.iter().all(|(socket, _)| *socket == got[0].0),
        "one socket"
    );
    assert!(got[0].1.get("previous_response_id").is_none());
    for (step, prev, call, word) in [(1, "resp_0", "c1", "ALPHA"), (2, "resp_1", "c2", "BRAVO")] {
        let m = &got[step].1;
        assert_eq!(m["previous_response_id"], prev, "step {step}: {m}");
        let items = m["input"].as_array().unwrap();
        assert!(
            items.iter().all(|i| i["type"] == "function_call_output"),
            "step {step} carries only the new tool output: {m}"
        );
        assert_eq!(items.len(), 1, "step {step}: {m}");
        assert_eq!(items[0]["call_id"], call);
        assert!(items[0]["output"].to_string().contains(word), "{m}");
    }
    let _ = std::fs::remove_dir_all(&ws);
}

use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::client::{AuthKind, ModelClient, StreamEvent, XaiClient};

fn tmp(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("gh-batch-{tag}-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn msg(role: &str, text: &str) -> InputItem {
    InputItem::Message {
        role: role.into(),
        content: vec![ContentPart::InputText(text.into())],
    }
}

fn ask(text: &str) -> ResponsesRequest {
    ResponsesRequest {
        model: "grok-4.3".into(),
        effort: Some("low".into()),
        input: vec![msg("system", "Reply with JSON."), msg("user", text)],
        conversation_id: "background:dream".into(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: None,
        text_format: None,
    }
}

fn policy(dir: &Path) -> BatchPolicy {
    BatchPolicy {
        config_dir: dir.to_path_buf(),
        max_wait: Duration::from_secs(60),
        poll: Duration::from_millis(1),
    }
}

/// A Responses-shaped result row for `id`.
fn answer(id: &str, text: &str) -> Value {
    json!({
        "batch_request_id": id,
        "batch_result": {"response": {"responses": {
            "status": "completed",
            "output": [
                {"type": "reasoning", "encrypted_content": "c2VhbGVk", "summary": []},
                {"type": "message", "role": "assistant",
                 "content": [{"type": "output_text", "text": text}]}
            ],
            "usage": {"input_tokens": 120, "output_tokens": 9,
                      "input_tokens_details": {"cached_tokens": 100},
                      "output_tokens_details": {"reasoning_tokens": 4},
                      "cost_in_usd_ticks": 800}
        }}}
    })
}

fn settled(success: u64) -> BatchState {
    BatchState {
        num_requests: 1,
        num_success: success,
        ..BatchState::default()
    }
}

fn pending() -> BatchState {
    BatchState {
        num_requests: 1,
        num_pending: 1,
        ..BatchState::default()
    }
}

/// A scripted Batch API: each status read pops the next state (the last repeats).
#[derive(Default)]
struct Fake {
    calls: RefCell<Vec<String>>,
    states: RefCell<Vec<BatchState>>,
    cancel_state: RefCell<BatchState>,
    add_err: RefCell<Option<ClientError>>,
    reply: RefCell<Option<Value>>,
    /// The request id `add` saw (results answer it).
    added: RefCell<String>,
}

impl Fake {
    fn answering(states: Vec<BatchState>, text: &str) -> Self {
        let fake = Fake::default();
        *fake.states.borrow_mut() = states;
        *fake.reply.borrow_mut() = Some(json!(text));
        fake
    }

    fn called(&self, prefix: &str) -> usize {
        self.calls
            .borrow()
            .iter()
            .filter(|c| c.starts_with(prefix))
            .count()
    }
}

impl BatchApi for Fake {
    fn create(&self, name: &str) -> Result<String, ClientError> {
        self.calls.borrow_mut().push(format!("create {name}"));
        Ok("batch_1".into())
    }
    fn add(&self, batch_id: &str, request_id: &str, body: Value) -> Result<(), ClientError> {
        self.calls.borrow_mut().push(format!("add {batch_id}"));
        assert!(
            body.get("stream").is_none(),
            "a batch request is not streamed"
        );
        *self.added.borrow_mut() = request_id.to_string();
        match self.add_err.borrow_mut().take() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    fn get(&self, batch_id: &str) -> Result<BatchState, ClientError> {
        self.calls.borrow_mut().push(format!("get {batch_id}"));
        let mut states = self.states.borrow_mut();
        Ok(if states.len() > 1 {
            states.remove(0)
        } else {
            states.first().cloned().unwrap_or_default()
        })
    }
    fn results(&self, batch_id: &str) -> Result<Vec<Value>, ClientError> {
        self.calls.borrow_mut().push(format!("results {batch_id}"));
        let text = self.reply.borrow().clone().unwrap_or(json!(""));
        let id = self.added.borrow().clone();
        Ok(match text {
            Value::String(t) => vec![answer("someone-else", "no"), answer(&id, &t)],
            other => vec![other],
        })
    }
    fn cancel(&self, batch_id: &str) -> Result<BatchState, ClientError> {
        self.calls.borrow_mut().push(format!("cancel {batch_id}"));
        Ok(self.cancel_state.borrow().clone())
    }
}

fn no_sleep(_: Duration, cancel: &CancelToken) -> bool {
    !cancel.is_cancelled()
}

fn go(api: &dyn BatchApi, dir: &Path, req: &ResponsesRequest, now: &dyn Fn() -> u64) -> Deferred {
    run(api, &policy(dir), req, &CancelToken::new(), now, &no_sleep)
}

#[test]
fn only_a_plain_text_ask_to_a_batchable_grok_model_qualifies() {
    assert!(eligible(&ask("hi")));
    let mut searched = ask("hi");
    searched.hosted_search = true;
    assert!(eligible(&searched), "hosted search runs on xAI's side");
    let mut tooled = ask("hi");
    tooled.tools = vec![json!({"type": "function", "name": "read_file"})];
    assert!(
        !eligible(&tooled),
        "a tool round trip would be a batch per step"
    );
    let mut image = ask("hi");
    image.input.push(InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputImage("data:image/png;base64,YQ==".into())],
    });
    assert!(!eligible(&image));
    for model in ["grok-build-0.1", "gpt-5", "grok-imagine-image-2.0"] {
        let mut other = ask("hi");
        other.model = model.into();
        assert!(!eligible(&other), "{model}");
    }
    let mut empty = ask("hi");
    empty.input.clear();
    assert!(!eligible(&empty));
}

#[test]
fn the_batch_body_is_the_responses_body_without_streaming() {
    let body = batch_body(&ask("hello"));
    assert!(body.get("stream").is_none());
    assert_eq!(body["model"], "grok-4.3");
    assert_eq!(body["reasoning"]["effort"], "low");
    assert_eq!(body["prompt_cache_key"], "background:dream");
    assert_eq!(body["input"][1]["content"][0]["text"], "hello");
}

#[test]
fn a_settled_batch_answers_and_its_job_is_forgotten() {
    let dir = tmp("ok");
    let fake = Fake::answering(vec![pending(), pending(), settled(1)], "{\"lessons\":[]}");
    let Deferred::Done(turn) = go(&fake, &dir, &ask("dream"), &|| 1_000) else {
        panic!("expected an answer");
    };
    assert_eq!(turn.text, "{\"lessons\":[]}");
    assert_eq!(turn.usage.input_tokens, 120);
    assert_eq!(turn.usage.cached_tokens, 100);
    assert_eq!(turn.usage.reasoning_tokens, 4);
    assert_eq!(turn.usage.cost_in_usd_ticks, 800);
    assert_eq!(turn.reasoning_items.len(), 1);
    assert!(!turn.truncated);
    assert_eq!(fake.called("create"), 1);
    assert_eq!(fake.called("get"), 3);
    assert_eq!(fake.called("cancel"), 0);
    assert!(load(&dir).jobs.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn past_the_wait_the_batch_is_cancelled_before_the_normal_call() {
    let dir = tmp("late");
    let fake = Fake::answering(vec![pending()], "late");
    let clock = Cell::new(1_000u64);
    let now = || {
        clock.set(clock.get() + 30_000);
        clock.get()
    };
    let Deferred::Fallback(why) = go(&fake, &dir, &ask("dream"), &now) else {
        panic!("expected a fallback");
    };
    assert_eq!(why, "batch took too long");
    assert_eq!(fake.called("cancel"), 1);
    assert_eq!(fake.called("results"), 0, "nothing finished, nothing read");
    assert!(load(&dir).jobs.is_empty());
    assert!(load(&dir).cooldown.is_empty(), "slow is not broken");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_answer_that_lands_as_it_is_cancelled_is_still_used() {
    let dir = tmp("race");
    let fake = Fake::answering(vec![pending()], "just in time");
    *fake.cancel_state.borrow_mut() = settled(1);
    let clock = Cell::new(0u64);
    let now = || {
        clock.set(clock.get() + 61_000);
        clock.get()
    };
    let Deferred::Done(turn) = go(&fake, &dir, &ask("dream"), &now) else {
        panic!("expected the answer");
    };
    assert_eq!(turn.text, "just in time");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_model_xai_cannot_batch_rests_a_week_and_others_still_try() {
    let dir = tmp("unsupported");
    let fake = Fake::answering(vec![settled(1)], "x");
    *fake.add_err.borrow_mut() = Some(ClientError::Server {
        status: 400,
        message: "model grok-4.3 is not supported for batch processing".into(),
        retry_after_secs: None,
    });
    assert!(matches!(
        go(&fake, &dir, &ask("dream"), &|| 1_000),
        Deferred::Fallback(_)
    ));
    assert_eq!(fake.called("cancel"), 1, "the empty batch goes");
    assert_eq!(
        load(&dir).cooldown.get("grok-4.3"),
        Some(&(1_000 + WEEK_MS))
    );
    // The same model waits; it never reaches create.
    let again = Fake::answering(vec![settled(1)], "x");
    assert!(matches!(
        go(&again, &dir, &ask("dream"), &|| 2_000),
        Deferred::Fallback(_)
    ));
    assert_eq!(again.called("create"), 0);
    // Another model still goes.
    let mut other = ask("dream");
    other.model = "grok-4.7".into();
    let third = Fake::answering(vec![settled(1)], "ok");
    assert!(matches!(
        go(&third, &dir, &other, &|| 3_000),
        Deferred::Done(_)
    ));
    // A week on, the first model is tried again.
    let later = Fake::answering(vec![settled(1)], "ok");
    assert!(matches!(
        go(&later, &dir, &ask("dream"), &|| 1_000 + WEEK_MS + 1),
        Deferred::Done(_)
    ));
    assert!(load(&dir).cooldown.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_key_rests_a_day_and_other_failures_an_hour() {
    let auth = ClientError::Auth {
        status: 403,
        message: "no".into(),
    };
    assert_eq!(cooldown_for(&auth, "grok-4.3"), ("*".into(), DAY_MS));
    let net = ClientError::Transport("reset".into());
    assert_eq!(cooldown_for(&net, "grok-4.3"), ("*".into(), HOUR_MS));
}

#[test]
fn the_same_ask_picks_its_batch_back_up_and_a_left_behind_one_is_cancelled() {
    let dir = tmp("resume");
    let req = ask("dream");
    let body = batch_body(&req);
    let key = key_of("grok-4.3", &body);
    let now = 10 * DAY_MS;
    save(
        &dir,
        &Store {
            jobs: vec![
                Job {
                    batch_id: "batch_mine".into(),
                    request_id: format!("loadout-{key}"),
                    key: key.clone(),
                    model: "grok-4.3".into(),
                    submitted_ms: now - 60_000,
                },
                Job {
                    batch_id: "batch_old".into(),
                    request_id: "loadout-old".into(),
                    key: "old".into(),
                    model: "grok-4.3".into(),
                    submitted_ms: now - DAY_MS,
                },
            ],
            cooldown: BTreeMap::new(),
        },
    );
    let fake = Fake::answering(vec![settled(1)], "resumed");
    *fake.added.borrow_mut() = format!("loadout-{key}");
    // A wide wait, so the minute-old job is still in its window.
    let wide = BatchPolicy {
        max_wait: MAX_WAIT,
        ..policy(&dir)
    };
    let Deferred::Done(turn) = run(&fake, &wide, &req, &CancelToken::new(), &|| now, &no_sleep)
    else {
        panic!("expected the resumed answer");
    };
    assert_eq!(turn.text, "resumed");
    assert_eq!(fake.called("create"), 0);
    assert_eq!(
        *fake.calls.borrow(),
        vec!["cancel batch_old", "get batch_mine", "results batch_mine"]
    );
    assert!(load(&dir).jobs.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_cancelled_pass_cancels_its_batch() {
    let dir = tmp("cancel");
    let fake = Fake::answering(vec![pending()], "x");
    let cancel = CancelToken::new();
    let stop = |_: Duration, c: &CancelToken| {
        c.cancel();
        false
    };
    let out = run(
        &fake,
        &policy(&dir),
        &ask("dream"),
        &cancel,
        &|| 1_000,
        &stop,
    );
    assert!(matches!(out, Deferred::Cancelled));
    assert_eq!(fake.called("cancel"), 1);
    assert!(load(&dir).jobs.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_reply_in_an_unknown_shape_rests_batching_a_week() {
    let dir = tmp("shape");
    let fake = Fake::answering(vec![settled(1)], "x");
    *fake.reply.borrow_mut() = Some(json!({
        "batch_request_id": "whatever",
        "batch_result": {"response": {"mystery": {"text": "?"}}}
    }));
    // Results don't name our request: missing, no cooldown.
    assert!(matches!(
        go(&fake, &dir, &ask("dream"), &|| 1_000),
        Deferred::Fallback(_)
    ));
    assert!(load(&dir).cooldown.is_empty());
    let row = json!({
        "batch_request_id": "r",
        "batch_result": {"response": {"mystery": {"text": "?"}}}
    });
    assert_eq!(parse_result(&row, "grok-4.3"), Err(ResultError::Unreadable));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn results_read_both_reply_shapes_and_failures() {
    // The Chat Completions shape from xAI's reference.
    let chat = json!({
        "batch_request_id": "r",
        "batch_result": {"response": {"chat_get_completion": {
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "303"},
                         "finish_reason": "length"}],
            "usage": {"prompt_tokens": 706, "completion_tokens": 22,
                      "prompt_tokens_details": {"cached_tokens": 679},
                      "completion_tokens_details": {"reasoning_tokens": 99},
                      "cost_in_usd_ticks": 1234}
        }}}
    });
    let turn = parse_result(&chat, "grok-4").unwrap();
    assert_eq!(turn.text, "303");
    assert!(turn.truncated);
    assert_eq!(turn.usage.input_tokens, 706);
    assert_eq!(turn.usage.cached_tokens, 679);
    assert_eq!(turn.usage.reasoning_tokens, 99);
    assert_eq!(turn.usage.cost_in_usd_ticks, 1234);
    // The Responses shape, cut short.
    let cut = json!({
        "batch_request_id": "r",
        "batch_result": {"response": {"responses": {
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "output": [], "output_text": "partial", "usage": {}
        }}}
    });
    let turn = parse_result(&cut, "grok-4.3").unwrap();
    assert_eq!(turn.text, "partial");
    assert_eq!(turn.truncate_reason.as_deref(), Some("max_output_tokens"));
    // A failed request.
    let failed = json!({"batch_request_id": "r", "error_message": "bad input"});
    assert_eq!(
        parse_result(&failed, "grok-4.3"),
        Err(ResultError::Failed("bad input".into()))
    );
}

#[test]
fn the_setting_turns_batching_off() {
    let dir = tmp("setting");
    assert!(enabled_at(&dir));
    fs::write(dir.join("app.json"), "{\"batch_background\":false}").unwrap();
    assert!(!enabled_at(&dir));
    let client = XaiClient::new("k", AuthKind::ApiKey, Duration::from_secs(1)).deferrable(&dir);
    assert!(!client.defers());
    fs::write(dir.join("app.json"), "{\"batch_background\":true}").unwrap();
    assert!(
        XaiClient::new("k", AuthKind::ApiKey, Duration::from_secs(1))
            .deferrable(&dir)
            .defers()
    );
    assert!(
        !XaiClient::new("t", AuthKind::OAuth, Duration::from_secs(1))
            .deferrable(&dir)
            .defers(),
        "the SuperGrok pool has no batch price"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn batch_urls_are_the_api_root_and_one_batch_id_with_a_known_tail() {
    use crate::client::batch_url;
    for ok in [
        "https://api.x.ai/v1/batches",
        "https://api.x.ai/v1/batches/batch_1934e8b5-f3dc",
        "https://api.x.ai/v1/batches/batch_1/requests",
        "https://api.x.ai/v1/batches/batch_1/results?limit=10",
        "https://api.x.ai/v1/batches/batch_1:cancel",
    ] {
        assert!(batch_url(ok), "{ok}");
    }
    for bad in [
        "http://api.x.ai/v1/batches",
        "https://api.x.ai/v1/batchesx",
        "https://api.x.ai/v1/batches/",
        "https://api.x.ai/v1/batches/../files",
        "https://api.x.ai/v1/batches/b/other",
        "https://api.x.ai/v1/batches/b/results?url=https://evil",
        "https://api.x.ai/v1/batches/b%2F/results",
        "https://evil.example/v1/batches",
    ] {
        assert!(!batch_url(bad), "{bad}");
    }
}

// The real client against a loopback Batch API.

struct Mock {
    url: String,
    seen: Arc<Mutex<Vec<(String, String)>>>,
}

fn mock(cancel_body: &'static str) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let log = log.clone();
            std::thread::spawn(move || serve(stream, &log, cancel_body));
        }
    });
    Mock {
        url: format!("http://127.0.0.1:{port}/v1/responses"),
        seen,
    }
}

fn serve(mut stream: TcpStream, log: &Mutex<Vec<(String, String)>>, cancel_body: &str) {
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
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let len = head
        .to_ascii_lowercase()
        .lines()
        .find_map(|l| l.strip_prefix("content-length:").map(str::to_string))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < head_end + len {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).into_owned();
    let mut words = head.split_whitespace();
    let line = format!(
        "{} {}",
        words.next().unwrap_or(""),
        words.next().unwrap_or("")
    );
    log.lock().unwrap().push((line.clone(), body.clone()));
    let request_id = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v.pointer("/batch_requests/0/batch_request_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    if !request_id.is_empty() {
        *LAST_ID.lock().unwrap() = request_id;
    }
    let reply = match line.as_str() {
        "POST /v1/batches" => json!({"batch_id": "batch_7", "state": {}}).to_string(),
        "POST /v1/batches/batch_7/requests" => String::new(),
        "GET /v1/batches/batch_7" => {
            json!({"batch_id": "batch_7", "state": {"num_requests": 1, "num_pending": 0, "num_success": 1}}).to_string()
        }
        "GET /v1/batches/batch_7/results?limit=10" => {
            let id = LAST_ID.lock().unwrap().clone();
            json!({"results": [answer(&id, "from the batch")], "pagination_token": null}).to_string()
        }
        "POST /v1/batches/batch_7:cancel" => cancel_body.to_string(),
        "POST /v1/responses" => {
            let done = json!({"type": "response.completed", "response": {"usage": {}}});
            let delta = json!({"type": "response.output_text.delta", "delta": "live"});
            let sse = format!("data: {delta}\n\ndata: {done}\n\n");
            let out = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{sse}"
            );
            let _ = stream.write_all(out.as_bytes());
            return;
        }
        _ => "{}".into(),
    };
    let out = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
        reply.len()
    );
    let _ = stream.write_all(out.as_bytes());
}

static LAST_ID: Mutex<String> = Mutex::new(String::new());

#[test]
fn a_deferrable_client_answers_a_night_pass_through_the_batch_api() {
    let dir = tmp("e2e");
    let server = mock("{}");
    let client = XaiClient::new("test-key", AuthKind::ApiKey, Duration::from_secs(10))
        .with_loopback_url(&server.url)
        .unwrap()
        .deferrable(&dir);
    assert!(client.defers());
    let mut streamed = String::new();
    let turn = client
        .stream(&ask("dream"), &CancelToken::new(), &mut |ev| {
            if let StreamEvent::TextDelta(t) = ev {
                streamed.push_str(&t);
            }
        })
        .unwrap();
    assert_eq!(turn.text, "from the batch");
    assert_eq!(streamed, "from the batch", "the sink still sees the reply");
    let seen = server.seen.lock().unwrap().clone();
    let lines: Vec<&str> = seen.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(
        lines,
        vec![
            "POST /v1/batches",
            "POST /v1/batches/batch_7/requests",
            "GET /v1/batches/batch_7",
            "GET /v1/batches/batch_7/results?limit=10",
        ]
    );
    let added: Value = serde_json::from_str(&seen[1].1).unwrap();
    let request = &added["batch_requests"][0]["batch_request"]["responses"];
    assert_eq!(request["model"], "grok-4.3");
    assert!(request.get("stream").is_none());
    assert!(load(&dir).jobs.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_plain_client_and_a_tool_turn_never_touch_the_batch_api() {
    let dir = tmp("plain");
    let server = mock("{}");
    let plain = XaiClient::new("test-key", AuthKind::ApiKey, Duration::from_secs(2))
        .with_loopback_url(&server.url)
        .unwrap();
    let live = plain.stream(&ask("chat"), &CancelToken::new(), &mut |_| {});
    assert_eq!(live.unwrap().text, "live");
    let deferrable = XaiClient::new("test-key", AuthKind::ApiKey, Duration::from_secs(2))
        .with_loopback_url(&server.url)
        .unwrap()
        .deferrable(&dir);
    let mut tooled = ask("agent step");
    tooled.tools = vec![json!({"type": "function", "name": "read_file"})];
    let live = deferrable.stream(&tooled, &CancelToken::new(), &mut |_| {});
    assert_eq!(live.unwrap().text, "live");
    let seen = server.seen.lock().unwrap().clone();
    assert!(
        seen.iter().all(|(line, _)| line == "POST /v1/responses"),
        "{seen:?}"
    );
    assert_eq!(seen.len(), 2);
    let _ = fs::remove_dir_all(&dir);
}

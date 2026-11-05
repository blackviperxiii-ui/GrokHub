use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};

use super::*;
use crate::client::{ClientError, ContentPart, ResponsesRequest, StreamEvent, TurnOutput};
use crate::gate::{self, Gate};
use crate::run::{HaltCheck, LoopIn, SteerQueue};
use crate::FunctionCall;

fn text(role: &str, body: &str) -> InputItem {
    InputItem::Message {
        role: role.into(),
        content: vec![ContentPart::InputText(body.into())],
    }
}

fn body(item: &InputItem) -> String {
    super::super::message_text(item)
}

fn is_summary_request(req: &ResponsesRequest) -> bool {
    req.input.iter().any(|item| {
        body(item).contains("<summary_request>") || body(item).contains("faithful, concise summary")
    })
}

/// Answers a summary request with a summary (after `gate` lets it go, when
/// set) and every other request from `chat`, in order.
struct Router {
    summaries: AtomicUsize,
    fail_summary: bool,
    gate: Mutex<Option<mpsc::Receiver<()>>>,
    chat: Mutex<Vec<TurnOutput>>,
}

impl Router {
    fn new(chat: Vec<TurnOutput>) -> Self {
        Self {
            summaries: AtomicUsize::new(0),
            fail_summary: false,
            gate: Mutex::new(None),
            chat: Mutex::new(chat),
        }
    }
}

impl ModelClient for Router {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        if is_summary_request(req) {
            self.summaries.fetch_add(1, Ordering::SeqCst);
            if let Some(rx) = self.gate.lock().unwrap().take() {
                let _ = rx.recv();
            }
            if self.fail_summary {
                return Err(ClientError::Protocol("summary failed".into()));
            }
            let text =
                "<summary>\n1. Primary Request and Intent: BG_SUMMARY\n</summary>".to_string();
            sink(StreamEvent::TextDelta(text.clone()));
            return Ok(TurnOutput {
                text,
                ..Default::default()
            });
        }
        let mut chat = self.chat.lock().unwrap();
        let mut turn = if chat.is_empty() {
            TurnOutput {
                text: "done".into(),
                ..Default::default()
            }
        } else {
            chat.remove(0)
        };
        // The provider's own prompt count, which the triggers read.
        turn.usage.input_tokens = super::super::estimate_input_tokens(&req.input);
        Ok(turn)
    }
}

fn start_for(key: &str, client: &Arc<Router>, history: &[InputItem]) {
    let client: Arc<dyn ModelClient + Send + Sync> = client.clone();
    start_unless_fresh(key, history, || Start {
        key: key.to_string(),
        client,
        model: "test/model".into(),
        effort: None,
        conversation_id: key.to_string(),
        history: history.to_vec(),
        modified_paths: Vec::new(),
        tools: Vec::new(),
    });
}

fn call(id: &str) -> InputItem {
    InputItem::FunctionCall {
        call_id: id.into(),
        name: "read_file".into(),
        arguments: r#"{"path":"a.txt"}"#.into(),
    }
}

fn out(id: &str) -> InputItem {
    InputItem::FunctionCallOutput {
        call_id: id.into(),
        output: "contents".into(),
    }
}

#[test]
fn the_soft_line_sits_under_the_hard_one() {
    assert!(!soft_due("test/model", 100_000, 69_999, 0));
    assert!(soft_due("test/model", 100_000, 70_000, 0));
    assert!(!soft_due("test/model", 0, 99_999, 0));
    // A Grok chat starts before the price step's ceiling.
    let ceiling = super::super::XAI_COMPACT_CEILING;
    assert!(!soft_due(
        "grok-4.3",
        1_000_000,
        0,
        ceiling - SOFT_PRICE_MARGIN - 1
    ));
    assert!(soft_due(
        "grok-4.3",
        1_000_000,
        0,
        ceiling - SOFT_PRICE_MARGIN
    ));
    assert!(super::super::parse_background(""));
    assert!(super::super::parse_background("backend = \"summary\""));
    assert!(!super::super::parse_background("background = false"));
}

#[test]
fn a_running_job_is_waited_for_and_what_came_after_is_kept() {
    let key = "bg-wait";
    forget(key);
    let (tx, rx) = mpsc::channel();
    let client = Arc::new(Router::new(Vec::new()));
    *client.gate.lock().unwrap() = Some(rx);
    let mut history = vec![text("user", "port the parser"), call("c1"), out("c1")];
    start_for(key, &client, &history);
    assert!(has(key));
    // The run goes on while the summary is written.
    history.push(call("c2"));
    history.push(out("c2"));
    // A fresh job is not started again.
    start_for(key, &client, &history);
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        tx.send(()).unwrap();
    });
    let (rebuilt, _) = take(key, &history).expect("the background result");
    release.join().unwrap();
    assert_eq!(client.summaries.load(Ordering::SeqCst), 1);
    assert!(body(&rebuilt[0]).contains("BG_SUMMARY"), "{:?}", rebuilt[0]);
    // The two items added after the snapshot come last, in order.
    assert_eq!(&rebuilt[rebuilt.len() - 2..], &history[3..]);
    assert!(!has(key), "a job is used once");
}

#[test]
fn a_history_that_changed_other_than_by_growing_is_not_swapped() {
    let key = "bg-changed";
    forget(key);
    let client = Arc::new(Router::new(Vec::new()));
    let history = vec![text("user", "a"), call("c1"), out("c1"), text("user", "b")];
    start_for(key, &client, &history);
    let rewound = vec![text("user", "a"), text("user", "different")];
    assert!(take(key, &rewound).is_none());
    assert!(!has(key));
    // A stale job is replaced by a new start rather than kept.
    start_for(key, &client, &history);
    start_for(key, &client, &rewound);
    let (rebuilt, _) = take(key, &rewound).expect("the replacement job");
    assert!(body(&rebuilt[0]).contains("BG_SUMMARY"));
    // The first and the replacement ran; the dropped one may still be going.
    assert!(client.summaries.load(Ordering::SeqCst) >= 2);
}

#[test]
fn a_failed_job_falls_back() {
    let key = "bg-failed";
    forget(key);
    let mut router = Router::new(Vec::new());
    router.fail_summary = true;
    let client = Arc::new(router);
    let history = vec![text("user", "a"), call("c1"), out("c1")];
    start_for(key, &client, &history);
    assert!(take(key, &history).is_none());
    assert!(!has(key));
}

struct Never;
impl HaltCheck for Never {
    fn halted(&self) -> bool {
        false
    }
}

#[test]
fn the_run_starts_at_the_soft_line_and_swaps_at_the_hard_one() {
    let dir = std::env::temp_dir().join(format!("gh-compact-bg-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("big.txt"), "y".repeat(800)).unwrap();
    let limit = 1_000u64;
    // 75% full: past the soft line, under the hard one.
    let old = format!(
        "OLD_TOPIC {}",
        "x".repeat((750 * crate::tokens::BYTES_PER_TOKEN) as usize)
    );
    let mut history = vec![text("user", &old)];
    let client = Arc::new(Router::new(vec![TurnOutput {
        calls: vec![FunctionCall {
            call_id: "r1".into(),
            name: "read_file".into(),
            arguments: r#"{"target_file":"big.txt"}"#.into(),
        }],
        ..Default::default()
    }]));
    let key = crate::prefix_guard::run_key("bg-loop", 1, None);
    forget(&key);
    let input = LoopIn {
        client: &*client,
        workspace: &dir,
        model: "test/model",
        effort: None,
        system: "",
        conversation_id: "bg-loop",
        max_turns: 6,
        usage_base: crate::client::Usage::default(),
        cancel: &CancelToken::new(),
        steer: &SteerQueue::new(),
        halt: &Never,
        gate: Gate::phase_readonly(),
        desktop: None,
        permits: &gate::ClosedPermits,
        perms: None,
        context_length: limit,
        tasks: None,
        depth: 1,
        agent_id: None,
        shared_client: Some(client.clone()),
        shared_permits: None,
        shared_desktop: None,
    };
    let done = crate::run_loop(&input, &mut history, "go", None, &mut |_| {});
    assert!(done.compacted, "{:?}", done.stop);
    // One summary, written in the background and swapped in at the hard line.
    assert_eq!(client.summaries.load(Ordering::SeqCst), 1);
    assert!(history.iter().any(|i| body(i).contains("BG_SUMMARY")));
    assert!(!history.iter().any(|i| body(i).contains("OLD_TOPIC")));
    // The read made after the snapshot is still there.
    assert!(history
        .iter()
        .any(|i| matches!(i, InputItem::FunctionCallOutput { call_id, .. } if call_id == "r1")));
    assert!(!has(&key));
    let _ = std::fs::remove_dir_all(dir);
}

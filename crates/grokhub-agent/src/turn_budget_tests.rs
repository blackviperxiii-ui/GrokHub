use std::sync::Mutex;

use grokhub_core::run_metrics::append_run;

use super::*;
use crate::client::{
    CancelToken, ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest, StreamEvent,
    TurnOutput,
};
use crate::gate::{self, Gate};
use crate::run::{HaltCheck, LoopIn, SteerQueue};
use crate::FunctionCall;

fn ok(class: &str, steps: u32) -> (String, u32) {
    (class.to_string(), steps)
}

fn run(id: &str, class: &str, steps: u32, result: OutcomeResult, at: u64) -> RunMetrics {
    RunMetrics {
        run: id.into(),
        class: class.into(),
        result,
        steps,
        at,
        finished_at: at,
        ..Default::default()
    }
}

#[test]
fn the_budget_is_the_ninetieth_percentile_with_a_floor() {
    assert_eq!(p90(Vec::new()), None);
    assert_eq!(p90((1..=10).collect()), Some(9));
    assert_eq!(p90((1..=20).collect()), Some(18));
    assert_eq!(p90(vec![7]), Some(7));
    let kind: Vec<_> = (0..10).map(|i| ok("topic:invoices", 20 + i)).collect();
    assert_eq!(learned(&kind, "topic:invoices"), Some(28));
    // Short tasks still get room to work.
    let quick: Vec<_> = (0..10).map(|_| ok("topic:hello", 2)).collect();
    assert_eq!(learned(&quick, "topic:hello"), Some(FLOOR));
}

#[test]
fn a_kind_with_few_runs_borrows_from_all_and_too_little_history_learns_nothing() {
    let mut runs: Vec<_> = (0..4).map(|_| ok("topic:invoices", 60)).collect();
    // Four of its own: too few, so every run counts.
    runs.extend((0..6).map(|_| ok("topic:other", 15)));
    assert_eq!(learned(&runs, "topic:invoices"), Some(60));
    assert_eq!(learned(&runs[..9], "topic:invoices"), None);
    // An ask with no topic words uses every run.
    assert_eq!(learned(&runs, ""), Some(60));
}

#[test]
fn only_successful_runs_count_and_the_newest_line_of_a_run_wins() {
    let lines = vec![
        run("a", "topic:x", 5, OutcomeResult::Success, 1),
        // Undone later: the newer line says failure.
        run("a", "topic:x", 5, OutcomeResult::Failure, 2),
        run("b", "topic:x", 9, OutcomeResult::Pending, 1),
        run("c", "topic:x", 0, OutcomeResult::Success, 1),
        run("d", "topic:x", 4, OutcomeResult::Success, 1),
    ];
    assert_eq!(successes(lines), vec![ok("topic:x", 4)]);
}

#[test]
fn each_redirect_comes_once() {
    let mut budget = TurnBudget::new(Some(12));
    assert_eq!(budget.after_steps(11), None);
    let note = budget.after_steps(2).expect("the redirect");
    assert!(note.contains("within 12 tool steps"), "{note}");
    assert!(note.contains("has taken 13"), "{note}");
    assert_eq!(budget.after_steps(40), None);
    // No learned budget: no step redirect, ever.
    let mut none = TurnBudget::new(None);
    assert_eq!(none.after_steps(500), None);
}

/// One reply of `calls` reads, then a plain answer.
struct Reads {
    turns: Mutex<Vec<TurnOutput>>,
}

impl ModelClient for Reads {
    fn stream(
        &self,
        _req: &ResponsesRequest,
        _cancel: &CancelToken,
        _sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let mut turns = self.turns.lock().unwrap();
        Ok(if turns.is_empty() {
            TurnOutput {
                text: "done".into(),
                ..Default::default()
            }
        } else {
            turns.remove(0)
        })
    }
}

struct Never;
impl HaltCheck for Never {
    fn halted(&self) -> bool {
        false
    }
}

fn redirects(history: &[InputItem]) -> usize {
    history
        .iter()
        .filter(|item| match item {
            InputItem::Message { role, content } if role == "user" => content.iter().any(
                |p| matches!(p, ContentPart::InputText(t) if t.contains("Redirect: tasks like this one")),
            ),
            _ => false,
        })
        .count()
}

fn run_with(dir: &Path, ask: &str, reads: usize) -> Vec<InputItem> {
    let calls = (0..reads)
        .map(|i| FunctionCall {
            call_id: format!("r{i}"),
            name: "read_file".into(),
            arguments: r#"{"target_file":"a.txt"}"#.into(),
        })
        .collect();
    let client = Reads {
        turns: Mutex::new(vec![
            TurnOutput {
                calls,
                ..Default::default()
            },
            TurnOutput {
                calls: vec![FunctionCall {
                    call_id: "last".into(),
                    name: "read_file".into(),
                    arguments: r#"{"target_file":"a.txt"}"#.into(),
                }],
                ..Default::default()
            },
        ]),
    };
    let conv = format!("turn-budget-{}-{reads}", std::process::id());
    let input = LoopIn {
        client: &client,
        workspace: dir,
        model: "test/model",
        effort: None,
        system: "",
        conversation_id: &conv,
        max_turns: 0,
        usage_base: crate::client::Usage::default(),
        cancel: &CancelToken::new(),
        steer: &SteerQueue::new(),
        halt: &Never,
        gate: Gate::phase_readonly(),
        desktop: None,
        permits: &gate::ClosedPermits,
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
    crate::run_loop(&input, &mut history, ask, None, &mut |_| {});
    history
}

#[test]
fn a_run_past_its_learned_budget_gets_one_redirect() {
    let dir = std::env::temp_dir().join(format!("gh-turn-budget-{}", std::process::id()));
    let config = dir.join("config");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(dir.join("a.txt"), "hello").unwrap();
    let _guard = crate::perm::ConfigGuard::set(&config);
    let ask = "tidy the invoice spreadsheet";
    let class = topic_signature(ask);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    for i in 0..10 {
        let m = run(&format!("old:{i}"), &class, 3, OutcomeResult::Success, now);
        append_run(&config, &m, 90).unwrap();
    }
    assert_eq!(for_ask(&config, ask), Some(FLOOR));
    // Under the budget: no note.
    assert_eq!(redirects(&run_with(&dir, ask, 5)), 0);
    // Past it: one note, though the run goes on past it.
    let history = run_with(&dir, ask, FLOOR as usize);
    assert_eq!(redirects(&history), 1);
    assert!(history
        .iter()
        .any(|i| matches!(i, InputItem::FunctionCallOutput { call_id, .. } if call_id == "last")));
    let _ = std::fs::remove_dir_all(dir);
}

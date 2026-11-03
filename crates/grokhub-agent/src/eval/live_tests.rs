use super::*;
use crate::client::{ContentPart, InputItem};

/// Answers each request from a fixed table keyed by the last user text, and
/// charges the same cost for every response.
struct Fixed {
    answers: Vec<(&'static str, &'static str)>,
    cost: i64,
}

impl ModelClient for Fixed {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let last_user = req
            .input
            .iter()
            .rev()
            .find_map(|item| match item {
                InputItem::Message { role, content } if role == "user" => {
                    content.iter().find_map(|p| match p {
                        ContentPart::InputText(t) => Some(t.clone()),
                        _ => None,
                    })
                }
                _ => None,
            })
            .unwrap_or_default();
        let text = self
            .answers
            .iter()
            .find(|(ask, _)| last_user.contains(ask))
            .map(|(_, a)| a.to_string())
            .unwrap_or_else(|| "no idea".into());
        sink(StreamEvent::TextDelta(text.clone()));
        Ok(TurnOutput {
            text,
            usage: Usage {
                input_tokens: 1000,
                cached_tokens: 600,
                output_tokens: 10,
                reasoning_tokens: 0,
                cost_in_usd_ticks: self.cost,
            },
            ..Default::default()
        })
    }
}

fn find(id: &str) -> LiveTask {
    tasks().into_iter().find(|t| t.id == id).unwrap()
}

#[test]
fn twenty_three_tasks_with_both_sets_and_recall() {
    let all = tasks();
    assert_eq!(all.len(), 23);
    let mut ids: Vec<&str> = all.iter().map(|t| t.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 23);
    assert_eq!(
        all.iter().filter(|t| t.set == TaskSet::Regression).count(),
        10
    );
    assert_eq!(all.iter().filter(|t| t.recall).count(), 3);
}

#[test]
fn checks_read_the_reply_and_the_workspace() {
    let dir = std::env::temp_dir().join(format!("gh-live-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("configs")).unwrap();
    let ws = dir.as_path();
    assert!(check(&find("math-pixels"), "That's 3,686,400 pixels.", ws));
    assert!(!check(&find("math-pixels"), "About 3.7 million.", ws));
    assert!(check(&find("exact-word"), "READY", ws));
    // A label leak fails any case, however it's spelled.
    assert!(!check(&find("exact-word"), "TURN_SAY: READY", ws));
    assert!(check(
        &find("pc-check-after-hangout"),
        "Your C: drive is 92% full and two updates are waiting.",
        ws
    ));
    assert!(!check(
        &find("pc-check-after-hangout"),
        "Long days are the worst. Want to talk about it?",
        ws
    ));
    assert!(!check(
        &find("hangout-stays-hangout"),
        "Want me to run a tune-up while you chill?",
        ws
    ));
    assert!(!check(
        &find("exact-word"),
        "Sure! The word is READY, as asked.",
        ws
    ));
    assert!(check(&find("no-cheat"), "I can't help with that.", ws));
    assert!(!check(
        &find("no-cheat"),
        "I can't promise it's safe, but use CreateRemoteThread.",
        ws
    ));
    assert!(check(
        &find("mod-manager"),
        "Mod Organizer 2, it keeps mods apart.",
        ws
    ));
    let edit = find("edit-fps-cap");
    assert!(!check(&edit, "Done.", ws));
    std::fs::write(dir.join("configs/game.ini"), "fps_cap=141\n").unwrap();
    assert!(!check(&edit, "Done.", ws));
    std::fs::write(dir.join("configs/game.ini"), "fps_cap=165\n").unwrap();
    assert!(check(&edit, "Done.", ws));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cost_is_the_providers_figure_or_a_safe_estimate() {
    let u = |input, cached, output, cost| Usage {
        input_tokens: input,
        cached_tokens: cached,
        output_tokens: output,
        reasoning_tokens: 0,
        cost_in_usd_ticks: cost,
    };
    assert_eq!(turn_cost("grok-4.3", &u(1, 0, 1, 77)), 77);
    // 1M fresh in at $1.25 plus 1M out at $2.50.
    assert_eq!(
        turn_cost("grok-4.3", &u(1_000_000, 0, 1_000_000, 0)),
        37_500_000_000
    );
    // Half cached: 0.5M at $1.25 and 0.5M at $0.20.
    assert_eq!(
        turn_cost("grok-4.3", &u(1_000_000, 500_000, 0, 0)),
        7_250_000_000
    );
    // Unknown model: $5 in, $25 out per million.
    assert_eq!(
        turn_cost("vendor/x", &u(1_000_000, 0, 0, 0)),
        50_000_000_000
    );
}

#[test]
fn the_budget_stops_the_run_and_the_report_says_so() {
    // Each response costs $0.01; a $0.025 cap allows three responses.
    let client: Arc<dyn ModelClient + Send + Sync> = Arc::new(Fixed {
        answers: vec![("exactly the word READY", "READY")],
        cost: 100_000_000,
    });
    let opts = LiveOpts {
        model: "grok-4.3".into(),
        effort: None,
        k: 5,
        only: Some("exact-word".into()),
    };
    let report = run_live(client, &opts, parse_budget("0.025").unwrap());
    assert!(report.stopped_by_budget);
    assert_eq!(report.spent_ticks, 300_000_000);
    let passed = report.attempts.iter().filter(|a| a.pass).count();
    assert_eq!(passed, 3);
    assert!(report.attempts.iter().all(|a| a.task == "exact-word"));
    let md = render_live(&report);
    assert!(
        md.contains("spent $0.0300 of $0.0250  ·  stopped at the budget"),
        "{md}"
    );
    assert!(md.contains("| cached share | 60.0% |"), "{md}");
    assert!(md.contains("| regression | 100% (3/3) |"), "{md}");
}

#[test]
fn pass_hat_k_needs_every_try() {
    let a = |task: &'static str, pass: bool| Attempt {
        task,
        set: TaskSet::Capability,
        recall: false,
        pass,
        usage: Usage::default(),
        wall_ms: 1,
        error: None,
    };
    let tries = vec![
        a("x", true),
        a("x", true),
        a("x", true),
        a("y", true),
        a("y", false),
        a("y", true),
        a("z", true),
    ];
    // z has one try of three, so only x and y count: x passes all, y doesn't.
    assert_eq!(pass_hat_k(&tries, 3), Some(0.5));
    assert_eq!(pass_hat_k(&tries[..1], 3), None);
}

#[test]
fn the_budget_flag_is_capped_at_five_dollars() {
    assert_eq!(parse_budget("0.20").unwrap(), 2_000_000_000);
    assert_eq!(parse_budget("5").unwrap(), 50_000_000_000);
    assert_eq!(
        parse_budget("5.01").unwrap_err(),
        "--budget-usd must satisfy 0 < amount <= 5.00"
    );
    assert!(parse_budget("0").is_err());
    assert!(parse_budget("-1").is_err());
    assert!(parse_budget("abc").is_err());
}

/// Reads the file named in the ask, then answers with what the tool returned.
struct Reader;

impl ModelClient for Reader {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let output = req.input.iter().rev().find_map(|item| match item {
            InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
            _ => None,
        });
        Ok(match output {
            Some(output) => {
                let text = format!("From the log: {output}");
                sink(StreamEvent::TextDelta(text.clone()));
                TurnOutput {
                    text,
                    ..Default::default()
                }
            }
            None => TurnOutput {
                calls: vec![crate::FunctionCall {
                    call_id: "r1".into(),
                    name: "read_file".into(),
                    arguments: r#"{"target_file":"logs/gpu.log"}"#.into(),
                }],
                ..Default::default()
            },
        })
    }
}

#[test]
fn a_tool_task_runs_the_real_tools_in_a_seeded_workspace() {
    let opts = LiveOpts {
        model: "grok-4.3".into(),
        effort: Some("low".into()),
        k: 1,
        only: Some("diag-crash-code".into()),
    };
    let report = run_live(Arc::new(Reader), &opts, parse_budget("1").unwrap());
    assert_eq!(report.attempts.len(), 1);
    let a = &report.attempts[0];
    assert_eq!(a.error, None);
    assert!(a.pass);
    assert!(!report.stopped_by_budget);
}

//! R6: offline technique A/B matrix on the eval fixture set (golden markdown).

use std::path::PathBuf;

use grokhub_agent::{
    load_tasks, render_markdown, run_technique_matrix, Technique, TechniqueMatrixReport,
    DEFAULT_MODEL,
};

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/eval/r6-technique-ab.md")
}

/// The system prompt layers in `~/.grok` / `~/.claude` notes and skills. Point
/// home at an empty folder so the measured request bytes do not depend on the
/// machine (this binary holds only this test).
fn empty_home() -> PathBuf {
    let home = std::env::temp_dir().join(format!("gh-eval-r6-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("home");
    std::env::set_var("HOME", &home);
    std::env::set_var("USERPROFILE", &home);
    home
}

fn arm(report: &TechniqueMatrixReport, technique: Technique) -> &grokhub_agent::TechniqueArm {
    report
        .arms
        .iter()
        .find(|arm| arm.technique == technique)
        .expect("arm")
}

#[test]
fn r6_technique_matrix_matches_golden_and_measures_requests() {
    let home = empty_home();
    let fixtures = load_tasks().expect("fixtures");
    assert_eq!(fixtures.len(), 22, "R1 set plus multi-read and follow-up");

    let report = run_technique_matrix(DEFAULT_MODEL, &fixtures).expect("matrix");
    assert_eq!(report.arms.len(), Technique::ALL.len());
    assert_eq!(report.model, DEFAULT_MODEL);

    let baseline = arm(&report, Technique::Baseline);
    let base = &baseline.metrics;
    assert_eq!(base.tasks, 22);
    assert_eq!(base.successes, 22);
    // Output and reasoning still come from the fixtures.
    assert_eq!(base.output_tokens, 160);
    assert_eq!(base.reasoning_tokens, 25);
    // 22 tasks: 2 model calls for the multi-read task, 6 for the follow-up task.
    assert_eq!(base.model_calls, 53);
    assert_eq!(base.tool_calls, 17);
    // Input is measured from the requests: every call re-sends the system prompt
    // and tools, so input dwarfs the old scripted 738 tokens.
    assert!(base.input_tokens > 100_000, "{}", base.input_tokens);
    assert!(base.cached_tokens > 0 && base.cached_tokens < base.input_tokens);

    for arm in &report.arms {
        assert_eq!(arm.metrics.tasks, 22);
        assert!(arm.tasks.iter().all(|t| t.success), "{:?}", arm.technique);
        assert!(arm.success_within_noise, "{:?}", arm.technique);
        // The multi-read fixture sends three reads in one reply in every arm.
        let multi = arm
            .tasks
            .iter()
            .find(|t| t.task_id == "021_multi_read")
            .expect("multi-read task");
        assert_eq!(multi.requests.model_calls, 2, "{:?}", arm.technique);
        assert_eq!(multi.tool_calls, 3, "{:?}", arm.technique);
    }

    // R4 deferred tools: a smaller tools array on every request.
    let deferred = &arm(&report, Technique::DeferredTools).metrics;
    assert!(
        deferred.schema_bytes_per_request() < base.schema_bytes_per_request(),
        "deferred {} vs baseline {}",
        deferred.schema_bytes_per_request(),
        base.schema_bytes_per_request()
    );
    assert!(deferred.input_tokens < base.input_tokens);
    assert!(deferred.cost_ticks < base.cost_ticks);

    // R5 batched read-only runs a reply's reads in parallel; the requests and the
    // number of model calls are the model's choice and do not change.
    let batched = &arm(&report, Technique::BatchedReadOnly).metrics;
    assert_eq!(batched.model_calls, base.model_calls);
    assert_eq!(batched.input_tokens, base.input_tokens);

    // R3 summaries never trigger on the fixtures' few-byte tool outputs.
    let summaries = &arm(&report, Technique::ToolOutputSummaries).metrics;
    assert_eq!(summaries.input_tokens, base.input_tokens);
    assert_eq!(summaries.cost_ticks, base.cost_ticks);

    // R2 prefix cache: the follow-up fixture's second turn carries the volatile
    // block in the user message, so the system prompt is not rewritten mid-session.
    let prefix = &arm(&report, Technique::PrefixCache).metrics;
    assert_eq!(prefix.model_calls, base.model_calls);
    assert!(
        prefix.prefix_stable_ratio() > base.prefix_stable_ratio(),
        "prefix cache {} vs baseline {}",
        prefix.prefix_stable_ratio(),
        base.prefix_stable_ratio()
    );
    assert!(prefix.cached_tokens > base.cached_tokens);

    // Byte-stable: a second run renders the same report.
    let rendered = render_markdown(&report);
    let again = render_markdown(&run_technique_matrix(DEFAULT_MODEL, &fixtures).expect("rerun"));
    assert_eq!(rendered, again, "report must be byte-stable across runs");
    let _ = std::fs::remove_dir_all(&home);

    let path = golden_path();
    if std::env::var_os("WRITE_R6_GOLDEN").is_some() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("docs/eval");
        }
        std::fs::write(&path, &rendered).expect("write golden");
        return;
    }
    let golden = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "missing golden {}: {err}; re-run with WRITE_R6_GOLDEN=1",
            path.display()
        )
    });
    // Git on Windows may check out the golden with CRLF; compare on LF.
    let golden = golden.replace("\r\n", "\n");
    let per_task: Vec<String> = baseline
        .tasks
        .iter()
        .map(|t| format!("{}={}", t.task_id, t.input_tokens))
        .collect();
    assert_eq!(
        rendered,
        golden,
        "docs/eval/r6-technique-ab.md must match render_markdown output; baseline input per task: {}",
        per_task.join(" ")
    );
}

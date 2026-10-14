//! R6: offline technique A/B matrix on the R1 fixture set (golden markdown).

use std::path::PathBuf;

use grokhub_agent::{
    load_tasks, render_markdown, run_technique_matrix, Recommendation, Technique, DEFAULT_MODEL,
};

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/eval/r6-technique-ab.md")
}

#[test]
fn r6_technique_matrix_matches_golden_and_baseline() {
    let fixtures = load_tasks().expect("fixtures");
    assert_eq!(fixtures.len(), 20, "R1 fixture set is 20 tasks");

    let report = run_technique_matrix(DEFAULT_MODEL, &fixtures).expect("matrix");
    assert_eq!(report.arms.len(), Technique::ALL.len());
    assert_eq!(report.model, DEFAULT_MODEL);

    let baseline = report
        .arms
        .iter()
        .find(|arm| arm.technique == Technique::Baseline)
        .expect("baseline arm");
    assert_eq!(baseline.metrics.tasks, 20);
    assert_eq!(baseline.metrics.successes, 20);
    assert_eq!(baseline.metrics.input_tokens, 738);
    assert_eq!(baseline.metrics.output_tokens, 140);
    assert_eq!(baseline.metrics.reasoning_tokens, 25);
    assert!(
        baseline.metrics.cost_ticks > 0,
        "baseline cost must be non-zero"
    );
    assert!(
        baseline.metrics.cost_per_task().is_finite(),
        "cost per task finite"
    );
    assert!(
        baseline.metrics.cost_per_solved_task().is_finite(),
        "cost per solved finite"
    );

    for arm in &report.arms {
        assert_eq!(arm.metrics.tasks, 20);
        assert!(arm.tasks.iter().all(|t| t.success), "{:?}", arm.technique);
        assert!(arm.metrics.cost_per_task().is_finite());
        assert!(arm.metrics.cost_per_solved_task().is_finite());
        if arm.technique == Technique::Baseline {
            continue;
        }
        assert!(
            arm.success_within_noise,
            "{:?} success should stay within noise",
            arm.technique
        );
        // Offline scripted usage is identical across arms → flat cost → spike.
        assert_eq!(
            arm.recommendation,
            Recommendation::Spike,
            "{:?} should spike until instrumented cost wins land",
            arm.technique
        );
        assert_eq!(arm.metrics.input_tokens, baseline.metrics.input_tokens);
        assert_eq!(arm.metrics.output_tokens, baseline.metrics.output_tokens);
        assert_eq!(arm.metrics.cost_ticks, baseline.metrics.cost_ticks);
    }

    let rendered = render_markdown(&report);
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
    assert_eq!(
        rendered, golden,
        "docs/eval/r6-technique-ab.md must match render_markdown output"
    );
}

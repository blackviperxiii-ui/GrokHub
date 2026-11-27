use super::*;

const SAMPLE: &str = r#"{
  "verdict": "needs-attention",
  "summary": "One risky unwrap in the new parser.",
  "findings": [{
    "severity": "high",
    "title": "unwrap on user input",
    "body": "parse_review unwraps the JSON object.",
    "file": "src/review.rs",
    "line_start": 12,
    "line_end": 18,
    "confidence": 0.91,
    "recommendation": "Return ReviewOutput::Markdown on parse failure.",
    "next_steps": ["Add a unit test for malformed JSON"]
  }, {
    "severity": "info",
    "title": "Naming",
    "body": "The helper name is fine.",
    "file": "src/lib.rs",
    "line_start": 3,
    "line_end": 3,
    "confidence": 0.4,
    "recommendation": "Leave as is.",
    "next_steps": []
  }]
}"#;

#[test]
fn parses_the_code_tab_review_schema() {
    let ReviewOutput::Structured(review) = parse_review(SAMPLE) else {
        panic!("expected structured review");
    };
    assert_eq!(review.verdict, Verdict::NeedsAttention);
    assert_eq!(review.summary, "One risky unwrap in the new parser.");
    assert_eq!(review.findings.len(), 2);
    assert_eq!(review.findings[0].severity, Severity::High);
    assert_eq!(review.findings[0].title, "unwrap on user input");
    assert_eq!(review.findings[0].file, "src/review.rs");
    assert_eq!(review.findings[0].line_start, 12);
    assert_eq!(review.findings[0].line_end, 18);
    assert!((review.findings[0].confidence - 0.91).abs() < f64::EPSILON);
    assert_eq!(
        review.findings[0].recommendation,
        "Return ReviewOutput::Markdown on parse failure."
    );
    assert_eq!(
        review.findings[0].next_steps,
        vec!["Add a unit test for malformed JSON"]
    );
}

#[test]
fn keeps_malformed_json_as_markdown() {
    let text = "Looks good overall.\nNot JSON {";
    assert_eq!(
        parse_review(text),
        ReviewOutput::Markdown {
            text: text.to_string()
        }
    );
}

#[test]
fn keeps_json_that_is_missing_verdict_as_markdown() {
    let text = r#"{"summary":"ok","findings":[]}"#;
    assert_eq!(
        parse_review(text),
        ReviewOutput::Markdown {
            text: text.to_string()
        }
    );
}

#[test]
fn reads_json_inside_a_fence_or_prose() {
    let wrapped = format!("Here is the review:\n```json\n{SAMPLE}\n```\n");
    assert!(matches!(
        parse_review(&wrapped),
        ReviewOutput::Structured(_)
    ));
    let approve = r#"prefix {"verdict":"approve","summary":"Ship it.","findings":[]} suffix"#;
    let ReviewOutput::Structured(review) = parse_review(approve) else {
        panic!("expected structured review");
    };
    assert_eq!(review.verdict, Verdict::Approve);
    assert!(review.findings.is_empty());
}

#[test]
fn maps_critical_severity_and_clamps_confidence() {
    let text = r#"{
      "verdict": "needs-attention",
      "summary": "Secret in the diff.",
      "findings": [{
        "severity": "critical",
        "title": "Token",
        "body": "A live token is in the patch.",
        "file": "src/a.rs",
        "line_start": 0,
        "line_end": 4,
        "confidence": 1.4,
        "recommendation": "Remove it.",
        "next_steps": []
      }]
    }"#;
    let ReviewOutput::Structured(review) = parse_review(text) else {
        panic!("expected structured review");
    };
    assert_eq!(review.findings[0].severity, Severity::High);
    assert_eq!(review.findings[0].line_start, 1);
    assert_eq!(review.findings[0].confidence, 1.0);
}

#[test]
fn filters_findings_by_severity() {
    let ReviewOutput::Structured(review) = parse_review(SAMPLE) else {
        panic!("expected structured review");
    };
    let high = filter_findings(&review, Some(Severity::High));
    assert_eq!(high.len(), 1);
    assert_eq!(high[0].title, "unwrap on user input");
    let info = filter_findings(&review, Some(Severity::Info));
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].file, "src/lib.rs");
    assert_eq!(filter_findings(&review, None).len(), 2);
    assert!(filter_findings(&review, Some(Severity::Low)).is_empty());
}

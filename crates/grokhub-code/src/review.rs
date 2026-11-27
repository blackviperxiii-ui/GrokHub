//! Structured Code tab review output: a verdict, a summary, and clickable
//! findings. The Code tab asks the native engine for JSON in this shape.
//! A reply that is not that JSON is kept as markdown so the review is never
//! dropped silently.

use serde::{Deserialize, Serialize};

/// The JSON object the Code tab asks the model to return.
pub const REVIEW_JSON_SCHEMA: &str = r#"{
  "verdict": "approve" | "needs-attention",
  "summary": string,
  "findings": [{
    "severity": "high" | "medium" | "low" | "info",
    "title": string,
    "body": string,
    "file": string,
    "line_start": number,
    "line_end": number,
    "confidence": number,
    "recommendation": string,
    "next_steps": string[]
  }]
}"#;

/// Shown when the reply is not structured JSON.
pub const MARKDOWN_FALLBACK: &str =
    "The model did not return structured JSON. Showing the reply as markdown.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Approve,
    NeedsAttention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    High,
    Medium,
    Low,
    Info,
}

impl Severity {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "critical" | "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            "info" => Some(Self::Info),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Info => "info",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    pub title: String,
    pub body: String,
    pub file: String,
    pub line_start: u32,
    pub line_end: u32,
    pub confidence: f64,
    pub recommendation: String,
    #[serde(default)]
    pub next_steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredReview {
    pub verdict: Verdict,
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewOutput {
    Structured(StructuredReview),
    Markdown { text: String },
}

/// Parse a model reply into structured review JSON, or keep the text as
/// markdown when it is not that JSON.
pub fn parse_review(text: &str) -> ReviewOutput {
    match parse_structured(text) {
        Some(review) => ReviewOutput::Structured(review),
        None => ReviewOutput::Markdown {
            text: text.to_string(),
        },
    }
}

/// Findings whose severity matches `want`. `None` keeps every finding.
pub fn filter_findings(review: &StructuredReview, want: Option<Severity>) -> Vec<&Finding> {
    review
        .findings
        .iter()
        .filter(|item| want.is_none_or(|sev| item.severity == sev))
        .collect()
}

fn parse_structured(text: &str) -> Option<StructuredReview> {
    let value = extract_object(text)?;
    let verdict = parse_verdict(value.get("verdict")?.as_str()?)?;
    let summary = value.get("summary")?.as_str()?.to_string();
    let findings = match value.get("findings") {
        None => Vec::new(),
        Some(items) => items
            .as_array()?
            .iter()
            .map(parse_finding)
            .collect::<Option<Vec<_>>>()?,
    };
    Some(StructuredReview {
        verdict,
        summary,
        findings,
    })
}

fn parse_verdict(raw: &str) -> Option<Verdict> {
    match raw.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "approve" => Some(Verdict::Approve),
        "needs-attention" | "needsattention" => Some(Verdict::NeedsAttention),
        _ => None,
    }
}

fn parse_finding(value: &serde_json::Value) -> Option<Finding> {
    let severity = Severity::parse(value.get("severity")?.as_str()?)?;
    let title = value.get("title")?.as_str()?.to_string();
    let body = value.get("body")?.as_str()?.to_string();
    let file = value.get("file")?.as_str()?.to_string();
    let line_start = int_field(value, "line_start")?.max(1);
    let line_end = int_field(value, "line_end").unwrap_or(line_start).max(line_start);
    let confidence = value
        .get("confidence")?
        .as_f64()
        .or_else(|| value.get("confidence")?.as_u64().map(|n| n as f64))?;
    let recommendation = value.get("recommendation")?.as_str()?.to_string();
    let next_steps = match value.get("next_steps") {
        None => Vec::new(),
        Some(steps) => steps
            .as_array()?
            .iter()
            .map(|step| step.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()?,
    };
    Some(Finding {
        severity,
        title,
        body,
        file,
        line_start,
        line_end,
        confidence: confidence.clamp(0.0, 1.0),
        recommendation,
        next_steps,
    })
}

fn int_field(value: &serde_json::Value, key: &str) -> Option<u32> {
    let n = value.get(key)?;
    n.as_u64()
        .or_else(|| n.as_i64().and_then(|v| u64::try_from(v).ok()))
        .and_then(|v| u32::try_from(v).ok())
}

fn extract_object(text: &str) -> Option<serde_json::Value> {
    let trimmed = text.trim();
    let body = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|rest| rest.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if value.is_object() {
            return Some(value);
        }
    }
    let start = body.find('{')?;
    let end = body.rfind('}')?;
    if end < start {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(body.get(start..=end)?).ok()?;
    value.is_object().then_some(value)
}

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;

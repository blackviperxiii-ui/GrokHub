//! VerifyGate (Spike-3b): at episode end a checker in a fresh context gets
//! only the goal text and the final observation (window list, screenshot
//! hash, or a file check). It never sees the worker's reasoning, its
//! transcript, or the episode view. It answers `{"verdict":"ok"}` or a reject
//! with a reason (strict JSON on Grok; the old `VERIFY_OK` / `REJECT:` text is
//! still read). The call goes through the router as `background:judge` at a
//! constant low effort.

use sha2::{Digest, Sha256};

use crate::client::{ClientError, ContentPart, InputItem, ModelClient, TextFormat};
use crate::route::{
    call_model, CallTokens, ModelCall, BACKGROUND_EFFORT, CLASS_EPISODE, CLASS_JUDGE,
};
use crate::tools::ToolOutput;
use crate::CancelToken;

/// The checker's only instructions.
pub static JUDGE_SYSTEM: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    grokhub_core::brand::filled("You are {product}'s VerifyGate. You did not do this task and you do not see how it was done. \
You get the goal and the final state of the desktop. Reply with JSON only: \
{\"verdict\":\"ok\",\"reason\":\"\"} when the final state shows the goal is met, \
otherwise {\"verdict\":\"reject\",\"reason\":\"one short reason\"}.")
});

/// The checker's reply schema, sent when a Grok model answers.
pub const VERIFY_FORMAT: TextFormat = TextFormat {
    name: "verify_gate_verdict",
    schema: r#"{"type":"object","properties":{"verdict":{"type":"string","enum":["ok","reject"]},"reason":{"type":"string"}},"required":["verdict","reason"],"additionalProperties":false}"#,
};

/// The checker's last try, after two calls that could not run, goes up the
/// route ladder: it routes as the worker's class (`episode:step`, at the
/// worker's effort) instead of `background:judge`, which never climbs. The
/// router still picks its model and effort. Its input is still only the goal
/// and the observation.
pub const ESCALATED_CLASS: &str = CLASS_EPISODE;
/// The reject reason when the checker still could not run after that.
pub const CHECKER_UNAVAILABLE: &str = "checker unavailable";

/// How much of an observation's text the checker gets.
pub const OBSERVATION_CAP: usize = 4_000;

/// What the desktop looks like after a step: the observation tool's text
/// (window list or geometry), a hash of its screenshot, and an optional
/// file check. The image itself stays with the worker.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observation {
    pub text: String,
    pub image: Option<String>,
    /// sha256 of the screenshot (or of the text when there is none), hex.
    pub hash: String,
    pub file_check: Option<String>,
}

impl Observation {
    pub fn from_output(out: &ToolOutput) -> Self {
        let mut h = Sha256::new();
        h.update(
            out.image_data_url
                .as_deref()
                .unwrap_or(&out.text)
                .as_bytes(),
        );
        let hash = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        Self {
            text: out.text.clone(),
            image: out.image_data_url.clone(),
            hash,
            file_check: None,
        }
    }

    /// The text form the worker and the checker read.
    pub fn line(&self) -> String {
        let text: String = self.text.chars().take(OBSERVATION_CAP).collect();
        let mut out = format!("{text}\nscreenshot sha256: {}", self.hash);
        if let Some(check) = &self.file_check {
            out.push_str(&format!("\nfile check: {check}"));
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Reject(String),
}

pub fn parse_verdict(text: &str) -> Verdict {
    if let Some(verdict) = parse_json_verdict(text) {
        return verdict;
    }
    if grokhub_core::verify::has_verify_ok(text) {
        return Verdict::Ok;
    }
    let t = text.trim();
    let reason = t.strip_prefix("REJECT:").unwrap_or(t).trim();
    let reason: String = reason.chars().take(200).collect();
    Verdict::Reject(if reason.is_empty() {
        "the checker gave no reason".into()
    } else {
        reason
    })
}

/// The JSON reply, also when wrapped in a fence or prose. `None` when there is
/// no object with a known verdict, so the text reader takes over.
fn parse_json_verdict(text: &str) -> Option<Verdict> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let value: serde_json::Value = serde_json::from_str(text.get(start..=end)?).ok()?;
    let verdict = value.get("verdict")?.as_str()?.trim().to_ascii_lowercase();
    let reason: String = value
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .chars()
        .take(200)
        .collect();
    match verdict.as_str() {
        "ok" => Some(Verdict::Ok),
        "reject" => Some(Verdict::Reject(if reason.is_empty() {
            "the checker gave no reason".into()
        } else {
            reason
        })),
        _ => None,
    }
}

/// The checker's call: its own system line, then the goal and the final
/// observation. Nothing else goes in.
pub fn verify_call(model: &str, goal: &str, obs: &Observation, conversation_id: &str) -> ModelCall {
    verify_call_at(
        model,
        CLASS_JUDGE,
        Some(BACKGROUND_EFFORT),
        goal,
        obs,
        conversation_id,
    )
}

/// [`verify_call`] routed as `class` at `effort` (the last try uses
/// [`ESCALATED_CLASS`] and the worker's effort).
pub fn verify_call_at(
    model: &str,
    class: &str,
    effort: Option<&str>,
    goal: &str,
    obs: &Observation,
    conversation_id: &str,
) -> ModelCall {
    let input = vec![
        InputItem::Message {
            role: "system".into(),
            content: vec![ContentPart::InputText((*JUDGE_SYSTEM).into())],
        },
        InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(format!(
                "Goal:\n{goal}\n\nFinal observation:\n{}",
                obs.line()
            ))],
        },
    ];
    ModelCall::xai(model, effort, class, conversation_id, input).with_text_format(VERIFY_FORMAT)
}

pub fn verify_gate(
    client: &dyn ModelClient,
    call: &ModelCall,
    cancel: &CancelToken,
) -> Result<(Verdict, CallTokens), ClientError> {
    let routed = call_model(client, call, cancel)?;
    Ok((parse_verdict(&routed.out.text), routed.tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_verdicts_parse_with_or_without_a_fence() {
        assert_eq!(
            parse_verdict(r#"{"verdict":"ok","reason":""}"#),
            Verdict::Ok
        );
        assert_eq!(
            parse_verdict(
                "```json\n{\"verdict\":\"reject\",\"reason\":\"the file is still there\"}\n```"
            ),
            Verdict::Reject("the file is still there".into())
        );
        assert_eq!(
            parse_verdict(r#"{"verdict":"reject","reason":"  "}"#),
            Verdict::Reject("the checker gave no reason".into())
        );
        // An unknown verdict falls through to the text reader, which rejects.
        assert!(matches!(
            parse_verdict(r#"{"verdict":"maybe"}"#),
            Verdict::Reject(_)
        ));
    }

    #[test]
    fn the_checker_call_asks_for_the_strict_verdict_schema() {
        let obs = Observation::from_output(&ToolOutput::ok("windows: Settings"));
        let call = verify_call("grok-4.3", "open settings", &obs, "ep");
        assert_eq!(call.text_format, Some(VERIFY_FORMAT));
        assert_eq!(call.request().text_format, Some(VERIFY_FORMAT));
    }

    #[test]
    fn verdicts_need_verify_ok_and_keep_the_reason() {
        assert_eq!(parse_verdict("VERIFY_OK"), Verdict::Ok);
        assert_eq!(
            parse_verdict("REJECT: the file is still there"),
            Verdict::Reject("the file is still there".into())
        );
        assert_eq!(
            parse_verdict("looks fine"),
            Verdict::Reject("looks fine".into())
        );
        assert_eq!(
            parse_verdict("  "),
            Verdict::Reject("the checker gave no reason".into())
        );
    }

    #[test]
    fn observation_hashes_the_screenshot_not_the_text() {
        let mut out = ToolOutput::ok("windows: Settings");
        let a = Observation::from_output(&out);
        assert_eq!(a.hash.len(), 64);
        out.image_data_url = Some("data:image/png;base64,AAAA".into());
        let b = Observation::from_output(&out);
        assert_ne!(a.hash, b.hash);
        assert!(
            b.line()
                .starts_with("windows: Settings\nscreenshot sha256: "),
            "{}",
            b.line()
        );
        assert!(
            !b.line().contains("base64"),
            "the checker never gets the image"
        );
    }
}

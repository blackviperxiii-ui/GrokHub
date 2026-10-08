//! VerifyGate (Spike-3b): at episode end a checker in a fresh context gets
//! only the goal text and the final observation (window list, screenshot
//! hash, or a file check). It never sees the worker's reasoning, its
//! transcript, or the episode view. It answers `VERIFY_OK` or a reject with
//! a reason. The call goes through the router as `background:judge` at a
//! constant low effort.

use sha2::{Digest, Sha256};

use crate::client::{ClientError, ContentPart, InputItem, ModelClient};
use crate::route::{call_model, CallTokens, ModelCall, BACKGROUND_EFFORT, CLASS_JUDGE};
use crate::tools::ToolOutput;
use crate::CancelToken;

/// The checker's only instructions.
pub const JUDGE_SYSTEM: &str = "You are GrokHub's VerifyGate. You did not do this task and you do not see how it was done. \
You get the goal and the final state of the desktop. Reply with exactly VERIFY_OK when the final state shows the goal is met. \
Otherwise reply REJECT: followed by one short reason.";

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
        h.update(out.image_data_url.as_deref().unwrap_or(&out.text).as_bytes());
        let hash = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        Self { text: out.text.clone(), image: out.image_data_url.clone(), hash, file_check: None }
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
    if grokhub_core::verify::has_verify_ok(text) {
        return Verdict::Ok;
    }
    let t = text.trim();
    let reason = t.strip_prefix("REJECT:").unwrap_or(t).trim();
    let reason: String = reason.chars().take(200).collect();
    Verdict::Reject(if reason.is_empty() { "the checker gave no reason".into() } else { reason })
}

/// The checker's call: its own system line, then the goal and the final
/// observation. Nothing else goes in.
pub fn verify_call(model: &str, goal: &str, obs: &Observation, conversation_id: &str) -> ModelCall {
    let input = vec![
        InputItem::Message { role: "system".into(), content: vec![ContentPart::InputText(JUDGE_SYSTEM.into())] },
        InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(format!("Goal:\n{goal}\n\nFinal observation:\n{}", obs.line()))],
        },
    ];
    ModelCall::xai(model, Some(BACKGROUND_EFFORT), CLASS_JUDGE, conversation_id, input)
}

pub fn verify_gate(
    client: &dyn ModelClient,
    model: &str,
    goal: &str,
    obs: &Observation,
    conversation_id: &str,
    cancel: &CancelToken,
) -> Result<(Verdict, CallTokens), ClientError> {
    let routed = call_model(client, &verify_call(model, goal, obs, conversation_id), cancel)?;
    Ok((parse_verdict(&routed.out.text), routed.tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_need_verify_ok_and_keep_the_reason() {
        assert_eq!(parse_verdict("VERIFY_OK"), Verdict::Ok);
        assert_eq!(parse_verdict("REJECT: the file is still there"), Verdict::Reject("the file is still there".into()));
        assert_eq!(parse_verdict("looks fine"), Verdict::Reject("looks fine".into()));
        assert_eq!(parse_verdict("  "), Verdict::Reject("the checker gave no reason".into()));
    }

    #[test]
    fn observation_hashes_the_screenshot_not_the_text() {
        let mut out = ToolOutput::ok("windows: Settings");
        let a = Observation::from_output(&out);
        assert_eq!(a.hash.len(), 64);
        out.image_data_url = Some("data:image/png;base64,AAAA".into());
        let b = Observation::from_output(&out);
        assert_ne!(a.hash, b.hash);
        assert!(b.line().starts_with("windows: Settings\nscreenshot sha256: "), "{}", b.line());
        assert!(!b.line().contains("base64"), "the checker never gets the image");
    }
}

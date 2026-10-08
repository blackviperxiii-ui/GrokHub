//! Read-only signal hooks the router uses. Spike-3b (#548 VerifyGate), Spike-4c
//! (#552 span `origin`) and Spike-7 (#556 outcome records) are all in beta, so
//! each hook has a real source here. Tests use fakes of the traits. A missing
//! signal is `None` (written as `null`), never a guess.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use grokhub_core::outcome::{read_outcomes, OutcomeResult};

use crate::harness::{span_path, Origin, Span, SpanTail, VERIFY_TOOL};

/// What VerifyGate said about an episode step. The checker's reason text stays on its span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifySignal {
    Ok,
    Reject,
}

impl VerifySignal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Reject => "reject",
        }
    }
}

pub trait VerifySource {
    /// The `step`-th VerifyGate check of `episode` (1-based), or its latest when `step` is 0.
    fn verdict(&self, episode: &str, step: u32) -> Option<VerifySignal>;
    /// A real source (#548). A stub says `false`, and self-tuning refuses to run on it.
    fn ready(&self) -> bool {
        true
    }
}

pub trait OriginSource {
    fn origin(&self, span: &Span) -> Option<Origin>;
}

pub trait OutcomeSource {
    fn outcome(&self, span_id: &str) -> Option<OutcomeResult>;
    /// A real source (#556). A stub says `false`, and self-tuning refuses to run on it.
    fn ready(&self) -> bool {
        true
    }
}

/// A hook with no source behind it: every answer is `None`.
pub struct StubSource;

impl VerifySource for StubSource {
    fn verdict(&self, _episode: &str, _step: u32) -> Option<VerifySignal> {
        None
    }

    fn ready(&self) -> bool {
        false
    }
}

impl OutcomeSource for StubSource {
    fn outcome(&self, _span_id: &str) -> Option<OutcomeResult> {
        None
    }

    fn ready(&self) -> bool {
        false
    }
}

/// Spans VerifyGate checks scanned per lookup.
pub const VERIFY_SCAN_LINES: usize = 2_000;

/// VerifyGate's `verify_script` spans in a chat's span file (#548).
pub struct SpanVerifySource {
    pub config_dir: PathBuf,
    pub session: String,
}

/// Chat span files followed at once for VerifyGate lookups.
const VERIFY_TAILS: usize = 32;

/// Each followed chat span file keeps only its decided checks: `(episode, verdict)`.
static TAILS: Mutex<Vec<SpanTail<(String, VerifySignal)>>> = Mutex::new(Vec::new());

fn keep_check(s: Span) -> Option<(String, VerifySignal)> {
    if s.tool != VERIFY_TOOL {
        return None;
    }
    match s.result.as_str() {
        "pass" => Some((s.episode, VerifySignal::Ok)),
        "fail" => Some((s.episode, VerifySignal::Reject)),
        _ => None,
    }
}

impl SpanVerifySource {
    /// Every decided VerifyGate check of `episode`, oldest first, from the
    /// newest [`VERIFY_SCAN_LINES`] lines of the chat's span file (followed
    /// in memory, so a step parses only the lines added since the last one).
    pub fn checks(&self, episode: &str) -> Vec<VerifySignal> {
        if episode.is_empty() || self.session.is_empty() {
            return Vec::new();
        }
        let path = span_path(&self.config_dir, &self.session);
        let mut tails = TAILS.lock().unwrap_or_else(|e| e.into_inner());
        let at = match tails.iter().position(|t| t.path() == path) {
            Some(i) => i,
            None => {
                if tails.len() >= VERIFY_TAILS {
                    tails.remove(0);
                }
                tails.push(SpanTail::new(path, VERIFY_SCAN_LINES, keep_check));
                tails.len() - 1
            }
        };
        let tail = &mut tails[at];
        tail.refresh();
        tail.newest(VERIFY_SCAN_LINES).into_iter().filter(|(ep, _)| ep == episode).map(|(_, v)| v).collect()
    }
}

impl VerifySource for SpanVerifySource {
    fn verdict(&self, episode: &str, step: u32) -> Option<VerifySignal> {
        let checks = self.checks(episode);
        if step == 0 { checks.last() } else { checks.get(step as usize - 1) }.copied()
    }
}

/// The span's own `origin` field (#552 puts one on every span).
pub struct SpanOriginSource;

impl OriginSource for SpanOriginSource {
    fn origin(&self, span: &Span) -> Option<Origin> {
        Some(span.origin)
    }
}

/// `outcomes.jsonl` (#556): the task outcome whose `span_ids` holds this span.
pub struct LedgerOutcomeSource {
    pub config_dir: PathBuf,
}

impl LedgerOutcomeSource {
    pub fn new(config_dir: &Path) -> Self {
        Self { config_dir: config_dir.to_path_buf() }
    }
}

impl OutcomeSource for LedgerOutcomeSource {
    fn outcome(&self, span_id: &str) -> Option<OutcomeResult> {
        read_outcomes(&self.config_dir).into_iter().rev().find(|o| o.span_ids.iter().any(|s| s == span_id)).map(|o| o.result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{append_span, test_dir};
    use grokhub_core::outcome::TaskOutcome;

    #[test]
    fn real_sources_read_verify_spans_origin_and_outcomes() {
        let dir = test_dir("route-signals");
        let mut pass = Span::deny("chat-1", VERIFY_TOOL, "{}", "", "soft");
        pass.result = "fail".into();
        pass.episode = "ep-1".into();
        append_span(&dir, &pass).unwrap();
        pass.result = "pass".into();
        append_span(&dir, &pass).unwrap();
        let v = SpanVerifySource { config_dir: dir.clone(), session: "chat-1".into() };
        assert_eq!(v.verdict("ep-1", 1), Some(VerifySignal::Reject));
        assert_eq!(v.verdict("ep-1", 0), Some(VerifySignal::Ok));
        assert_eq!(v.verdict("ep-1", 3), None);
        assert_eq!(v.verdict("ep-2", 0), None);
        assert_eq!(v.checks("ep-1"), vec![VerifySignal::Reject, VerifySignal::Ok]);
        let mut s = Span::deny("chat-1", "x", "{}", "", "soft");
        s.origin = Origin::Automation;
        assert_eq!(SpanOriginSource.origin(&s), Some(Origin::Automation));
        let line = TaskOutcome { task: "t".into(), result: OutcomeResult::Success, span_ids: vec!["chat-1:5".into()], ..TaskOutcome::default() };
        let path = grokhub_core::outcome::outcomes_path(&dir);
        std::fs::write(&path, format!("{}\n", serde_json::to_string(&line).unwrap())).unwrap();
        let o = LedgerOutcomeSource::new(&dir);
        assert_eq!(o.outcome("chat-1:5"), Some(OutcomeResult::Success));
        assert_eq!(o.outcome("chat-1:6"), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}

//! Signals: what happened, as ids and numbers only.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Signals that may wait in memory. One more drops the oldest.
pub const RING: usize = 256;
/// Waiting signals that make a write due.
pub const FLUSH_AT: usize = 64;
/// The longest a signal waits before it is written.
pub const FLUSH_MS: u64 = 30_000;
/// The longest subject, in bytes.
pub const SUBJECT_MAX: usize = 64;

/// What a signal is about: `area:id`, such as `setting:textSize`,
/// `chip:3fa9c1` or `model:grok-code`.
///
/// Letters, digits and `. _ - / :` only, so a sentence can't be a subject.
/// The id must already be an id: callers never pass words the user typed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Subject(String);

impl Subject {
    pub fn new(area: &str, id: &str) -> Option<Self> {
        if area.is_empty() || id.is_empty() || area.contains(':') {
            return None;
        }
        Self::parse(&format!("{area}:{id}"))
    }

    pub fn parse(text: &str) -> Option<Self> {
        let (area, id) = text.split_once(':')?;
        let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':');
        (!area.is_empty() && !id.is_empty() && text.len() <= SUBJECT_MAX && text.chars().all(ok))
            .then(|| Self(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part before the first colon.
    pub fn area(&self) -> &str {
        self.0.split_once(':').map_or("", |(area, _)| area)
    }
}

impl TryFrom<String> for Subject {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        Self::parse(&text).ok_or_else(|| format!("not a subject: {text}"))
    }
}

impl From<Subject> for String {
    fn from(s: Subject) -> String {
        s.0
    }
}

/// What happened to the subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Put in front of the user. Counts nothing on its own.
    Shown,
    Used,
    Picked,
    Accepted,
    Liked,
    Succeeded,
    /// Shown and passed over.
    Ignored,
    Hidden,
    Dismissed,
    Declined,
    Disliked,
    Failed,
    /// The user said the app got it wrong.
    Corrected,
    /// The user took back something the app did.
    Undone,
    /// The user set it by hand.
    SetByUser,
    /// A number the app measured about itself, in [`Signal::value`].
    Measured,
}

impl Kind {
    /// Whether this counts for the subject (`true`), against it (`false`),
    /// or neither.
    pub fn vote(self) -> Option<bool> {
        match self {
            Kind::Used | Kind::Picked | Kind::Accepted | Kind::Liked | Kind::Succeeded => {
                Some(true)
            }
            Kind::Ignored
            | Kind::Hidden
            | Kind::Dismissed
            | Kind::Declined
            | Kind::Disliked
            | Kind::Failed
            | Kind::Corrected
            | Kind::Undone => Some(false),
            Kind::Shown | Kind::SetByUser | Kind::Measured => None,
        }
    }
}

fn one() -> f32 {
    1.0
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

/// One line of the stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    /// When, in Unix milliseconds.
    #[serde(rename = "t")]
    pub at_ms: u64,
    #[serde(rename = "k")]
    pub kind: Kind,
    #[serde(rename = "s")]
    pub subject: Subject,
    /// 1 unless the kind is [`Kind::Measured`].
    #[serde(rename = "v", default = "one", skip_serializing_if = "is_one")]
    pub value: f32,
}

impl Signal {
    pub fn new(at_ms: u64, kind: Kind, subject: Subject) -> Self {
        Self {
            at_ms,
            kind,
            subject,
            value: 1.0,
        }
    }

    /// A number the app measured: milliseconds, tokens, a count.
    pub fn measured(at_ms: u64, subject: Subject, value: f32) -> Self {
        Self {
            at_ms,
            kind: Kind::Measured,
            subject,
            value,
        }
    }
}

/// The signals that wait in memory for the next write.
#[derive(Debug, Default)]
pub struct Stream {
    waiting: VecDeque<Signal>,
    dropped: u64,
    flushed_ms: u64,
}

impl Stream {
    pub fn record(&mut self, signal: Signal) {
        if self.waiting.len() == RING {
            self.waiting.pop_front();
            self.dropped += 1;
        }
        self.waiting.push_back(signal);
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// A write is due: enough signals wait, or one has waited long enough.
    pub fn due(&self, now_ms: u64) -> bool {
        match self.waiting.len() {
            0 => false,
            n if n >= FLUSH_AT => true,
            _ => now_ms.saturating_sub(self.flushed_ms) >= FLUSH_MS,
        }
    }

    /// Hands over everything that waits, oldest first.
    pub fn drain(&mut self, now_ms: u64) -> Vec<Signal> {
        self.flushed_ms = now_ms;
        self.waiting.drain(..).collect()
    }
}

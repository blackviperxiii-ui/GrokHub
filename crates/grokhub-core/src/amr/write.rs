//! M1 writer: one remembered line becomes one node. Ids come from the text,
//! so the same line remembered twice (by reflect, an insight, or the user) is
//! one node, not two.

use sha2::{Digest, Sha256};

use super::schema::{NodeDraft, NodeType, Sensitivity};
use super::store::AmrStore;
use super::AmrError;
use crate::redact::redact_secrets;

/// `<prefix>-<12 hex>` from the SHA-256 of `parts` joined by NUL.
pub(super) fn hashed_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            hasher.update([0u8]);
        }
        hasher.update(part.as_bytes());
    }
    let digest = hex::encode(hasher.finalize());
    format!("{prefix}-{}", &digest[..12])
}

/// `mem-<12 hex>` for a remembered line. Case and runs of whitespace don't
/// change it, and the hash is taken after secrets are redacted.
pub fn line_id(text: &str) -> String {
    let redacted = redact_secrets(text);
    let normalized = redacted
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    hashed_id("mem", &[&normalized])
}

/// A line with an email, phone, card, SSN or street address is personal and
/// gets sealed. Everything else stays plain markdown.
pub fn sensitivity_for(text: &str) -> Sensitivity {
    if crate::pii::redact_pii(text).1 > 0 {
        Sensitivity::Personal
    } else {
        Sensitivity::Plain
    }
}

/// `preference` when the line reads like one (the same test that sends a fact
/// to USER.md in legacy), else `fact`.
pub fn node_type_for(text: &str) -> NodeType {
    if crate::learning::looks_like_user_pref(text) {
        NodeType::Preference
    } else {
        NodeType::Fact
    }
}

/// One line to remember.
#[derive(Debug, Clone, PartialEq)]
pub struct LineWrite<'a> {
    pub text: &'a str,
    /// `chat:<thread_id>`, `user`, `learning_state`, or `chips`.
    pub source: &'a str,
    pub tags: Vec<String>,
    pub confidence: f32,
    /// An explicit user remember lifts a tombstone on the same line. Reflect
    /// and insights don't, so a forgotten line stays forgotten.
    pub revive: bool,
}

/// What [`remember_line`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remembered {
    /// A new node file was written.
    New(String),
    /// The line was already a node. Nothing was written (unless a tombstone
    /// was lifted).
    Known(String),
}

impl Remembered {
    pub fn id(&self) -> &str {
        match self {
            Self::New(id) | Self::Known(id) => id,
        }
    }
}

/// Write `w.text` as one node through [`AmrStore::remember`], which redacts
/// secrets and seals personal lines (or returns [`AmrError::Paused`] and
/// writes nothing). A blank line is [`AmrError::BadFrontmatter`].
pub fn remember_line(
    store: &AmrStore,
    w: &LineWrite<'_>,
    now_ms: u64,
) -> Result<Remembered, AmrError> {
    let text = w.text.trim();
    if text.is_empty() || text.contains(['\n', '\r']) {
        return Err(AmrError::BadFrontmatter("body".into()));
    }
    let id = line_id(text);
    let stamp = crate::oauth::unix_ms_to_rfc3339(now_ms);
    let draft = NodeDraft {
        id: id.clone(),
        node_type: node_type_for(text),
        created: stamp.clone(),
        updated: stamp,
        source: w.source.to_string(),
        confidence: w.confidence,
        tags: w.tags.clone(),
        body: format!("{text}\n"),
        sensitivity: sensitivity_for(text),
        consent_ref: String::new(),
    };
    match store.remember(&draft) {
        Ok(id) => Ok(Remembered::New(id.as_str().to_string())),
        Err(AmrError::DuplicateId(id)) => {
            if w.revive {
                store.revive(&id)?;
            }
            Ok(Remembered::Known(id))
        }
        Err(err) => Err(err),
    }
}

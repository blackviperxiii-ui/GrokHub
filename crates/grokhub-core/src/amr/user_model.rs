//! Spike-5a view, edit and forget for the learned user model, plus the
//! reflect diff. `/memory` and Settings → Permissions read [`memory_rows`];
//! every row says why GrokHub thinks it ([`SourceLink`]).
//!
//! Edit writes a user-authored node that supersedes the old one. Forget is a
//! tombstone, leaves `index.sqlite`, is noted in `dreams/forget-<date>.md`
//! (id only, never the text), and [`strip_forgotten`] keeps it out of the
//! next hub snapshot. Only a [`UserForget`] (the user's click or typing) can
//! forget here; a forget the agent starts is a hard delete card in the
//! harness. Reflect returns a diff for the user to see; it writes nothing.

use std::fs::{self, OpenOptions};
use std::io::Write;

use crate::hub_sync::HubMemoryFile;

use super::schema::{EdgeRel, Node, NodeDraft, NodeType, Sensitivity};
use super::store::AmrStore;
use super::write::line_id;
use super::AmrError;

/// Lowest confidence reflect proposes for USER.md.
pub const REFLECT_MIN_CONFIDENCE: f32 = 0.7;

/// A background guess. Recall skips it until [`approve_node`].
pub const PROPOSED_TAG: &str = "proposed";

/// Set when a person approves a proposed line.
const APPROVED_TAG: &str = "approved";

/// One approve or reject, with the confidence the line had.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApprovalSample {
    pub confidence: f32,
    pub approved: bool,
}

/// Approved live rows, and rejected rows that are still tagged `proposed`.
/// Forgetting a line that was already approved is not a rejection.
pub fn approval_samples(store: &AmrStore) -> Vec<ApprovalSample> {
    let mut out = Vec::new();
    for node in store.load_live().nodes {
        let approved = node.tags.iter().any(|tag| tag == APPROVED_TAG);
        let proposed = node.tags.iter().any(|tag| tag == PROPOSED_TAG);
        if approved && !proposed {
            out.push(ApprovalSample {
                confidence: node.confidence,
                approved: true,
            });
        }
    }
    for node in store.load_forgotten().nodes {
        if node.tags.iter().any(|tag| tag == PROPOSED_TAG) {
            out.push(ApprovalSample {
                confidence: node.confidence,
                approved: false,
            });
        }
    }
    out
}

/// Lowest approved confidence that sits strictly above every rejection.
/// No such approval, or no history, keeps [`REFLECT_MIN_CONFIDENCE`].
pub fn calibrate_reflect_min(samples: &[ApprovalSample]) -> f32 {
    let mut max_rejected = f32::NEG_INFINITY;
    let mut approved = Vec::new();
    for sample in samples {
        if !sample.confidence.is_finite() {
            continue;
        }
        if sample.approved {
            approved.push(sample.confidence);
        } else if sample.confidence > max_rejected {
            max_rejected = sample.confidence;
        }
    }
    approved.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    approved
        .into_iter()
        .find(|confidence| *confidence > max_rejected)
        .unwrap_or(REFLECT_MIN_CONFIDENCE)
}

/// The morning-summary line for the threshold [`calibrate_reflect_min`] chose.
pub fn reflect_threshold_log(threshold: f32) -> String {
    format!("reflect_min_confidence {threshold:.2}")
}

/// Where a node came from, parsed from its `source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLink {
    /// `chat:<thread>#<turn>` (old notes may have no turn).
    Chat { thread: String, turn: Option<u32> },
    /// `span:<session>:<ms>` or another span ref.
    Span(String),
    /// `pulse:<date>` or `pulse:card:<id>`.
    Pulse(String),
    /// `scope:<grant>`: read under a consent grant.
    Scope(String),
    /// Typed or edited by the user.
    User,
    /// An older source (`learning_state`, `chips`, …).
    Other(String),
}

impl SourceLink {
    pub fn parse(source: &str) -> Self {
        let source = source.trim();
        if let Some(rest) = source.strip_prefix("chat:") {
            return match rest.rsplit_once('#') {
                Some((thread, turn)) => match turn.parse() {
                    Ok(turn) => Self::Chat {
                        thread: thread.to_string(),
                        turn: Some(turn),
                    },
                    Err(_) => Self::Chat {
                        thread: rest.to_string(),
                        turn: None,
                    },
                },
                None => Self::Chat {
                    thread: rest.to_string(),
                    turn: None,
                },
            };
        }
        if let Some(rest) = source.strip_prefix("span:") {
            return Self::Span(rest.to_string());
        }
        if let Some(rest) = source.strip_prefix("pulse:") {
            return Self::Pulse(rest.to_string());
        }
        if let Some(rest) = source.strip_prefix("scope:") {
            return Self::Scope(rest.to_string());
        }
        if source == "user" {
            return Self::User;
        }
        Self::Other(source.to_string())
    }

    /// The "why I think this" words.
    pub fn label(&self) -> String {
        match self {
            Self::Chat {
                turn: Some(turn), ..
            } => format!("you said it in chat (turn {turn})"),
            Self::Chat { turn: None, .. } => "you said it in chat".into(),
            Self::Span(_) => "from what you approved, denied or undid".into(),
            Self::Pulse(_) => "from a card you reacted to".into(),
            Self::Scope(grant) => format!("read under your grant {grant}"),
            Self::User => "you wrote it".into(),
            Self::Other(source) => format!("imported from {source}"),
        }
    }

    /// Link target the cabin opens: `chat:`, `span:` or `pulse:` as stored.
    pub fn target(&self) -> String {
        match self {
            Self::Chat {
                thread,
                turn: Some(turn),
            } => format!("chat:{thread}#{turn}"),
            Self::Chat { thread, turn: None } => format!("chat:{thread}"),
            Self::Span(rest) => format!("span:{rest}"),
            Self::Pulse(rest) => format!("pulse:{rest}"),
            Self::Scope(grant) => format!("scope:{grant}"),
            Self::User => "user".into(),
            Self::Other(source) => source.clone(),
        }
    }
}

/// One learned row for `/memory` and Settings → Permissions.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryRow {
    pub id: String,
    pub node_type: NodeType,
    /// First body line.
    pub text: String,
    pub source: SourceLink,
    pub confidence: f32,
    pub sensitivity: Sensitivity,
    pub updated: String,
    /// Waiting in the approval queue. Recall skips it.
    pub proposed: bool,
    /// Claims this row corrects or contradicts, in edge order.
    pub beside: Vec<String>,
}

impl MemoryRow {
    /// `- <text> · why: [<label>](<target>) · <updated>`.
    /// A correction adds `· corrects: …`. A proposal adds `· waiting for approval`.
    pub fn line(&self) -> String {
        let mut out = format!(
            "- {} · why: [{}]({}) · {}",
            self.text,
            self.source.label(),
            self.source.target(),
            self.updated
        );
        if !self.beside.is_empty() {
            out.push_str(" · corrects: ");
            out.push_str(&self.beside.join("; "));
        }
        if self.proposed {
            out.push_str(" · waiting for approval");
        }
        out
    }
}

/// Learned user-model rows (fact, preference, routine, need, mind prior),
/// newest first. Forgotten and superseded nodes are left out; sealed ones
/// show only while they open. The count is sealed nodes that stayed shut.
pub fn memory_rows(store: &AmrStore) -> (Vec<MemoryRow>, usize) {
    let loaded = store.load_live();
    let superseded = store.superseded_ids();
    let edges = store.edges().unwrap_or_default();
    let text_of = |id: &str| -> String {
        loaded
            .nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| first_line(&n.body))
            .unwrap_or_default()
    };
    let mut rows: Vec<MemoryRow> = loaded
        .nodes
        .iter()
        .filter(|n| n.node_type.is_user_model() && !superseded.contains(&n.id))
        .map(|n| {
            let mut beside = Vec::new();
            for edge in edges.iter().filter(|e| {
                e.from == n.id && matches!(e.rel, EdgeRel::Supersedes | EdgeRel::Contradicts)
            }) {
                let line = text_of(&edge.to);
                if !line.is_empty() && !beside.contains(&line) {
                    beside.push(line);
                }
            }
            MemoryRow {
                text: first_line(&n.body),
                source: SourceLink::parse(&n.source),
                id: n.id.clone(),
                node_type: n.node_type,
                confidence: n.confidence,
                sensitivity: n.sensitivity,
                updated: n.updated.clone(),
                proposed: n.tags.iter().any(|t| t == PROPOSED_TAG),
                beside,
            }
        })
        .collect();
    rows.sort_by(|a, b| b.updated.cmp(&a.updated).then(a.id.cmp(&b.id)));
    (rows, loaded.locked)
}

/// `/memory` in chat: one line per row, or a plain empty line.
pub fn memory_text(store: &AmrStore) -> String {
    let (rows, locked) = memory_rows(store);
    let mut out: Vec<String> = rows.iter().map(MemoryRow::line).collect();
    if out.is_empty() {
        out.push("Nothing learned yet.".into());
    }
    if locked > 0 {
        out.push(format!("{locked} private notes are locked."));
    }
    out.join("\n")
}

/// Proof the user asked to forget: their click on Forget or their typing.
/// A forget the agent starts goes through the harness as a hard delete card
/// and never builds one of these.
#[derive(Debug)]
pub struct UserForget(());

impl UserForget {
    pub fn from_click() -> Self {
        Self(())
    }

    pub fn from_typing() -> Self {
        Self(())
    }
}

/// Edit a learned row: a new node in the user's words (`source: user`,
/// confidence 1) that supersedes the old one. Returns the new id.
pub fn edit_node(store: &AmrStore, id: &str, text: &str, now_ms: u64) -> Result<String, AmrError> {
    if store.is_scratch() {
        return Err(AmrError::Scratch);
    }
    let text = text.trim();
    if text.is_empty() || text.contains(['\n', '\r']) {
        return Err(AmrError::BadFrontmatter("body".into()));
    }
    let loaded = store.load_live();
    let superseded = store.superseded_ids();
    let old = loaded
        .nodes
        .into_iter()
        .find(|n| n.id == id && !superseded.contains(&n.id))
        .ok_or_else(|| AmrError::MissingNode(id.to_string()))?;
    let stamp = crate::oauth::unix_ms_to_rfc3339(now_ms);
    let draft = NodeDraft {
        id: line_id(text),
        node_type: old.node_type,
        created: stamp.clone(),
        updated: stamp,
        source: "user".into(),
        confidence: 1.0,
        tags: vec!["edited".into()],
        body: format!("{text}\n"),
        sensitivity: if old.sensitivity.sealed() {
            old.sensitivity
        } else {
            super::write::sensitivity_for(text)
        },
        consent_ref: String::new(),
    };
    let new_id = match store.remember(&draft) {
        Ok(new_id) => new_id.as_str().to_string(),
        Err(AmrError::DuplicateId(new_id)) => {
            store.revive(&new_id)?;
            new_id
        }
        Err(err) => return Err(err),
    };
    if new_id != id {
        store.link(&new_id, id, EdgeRel::Supersedes)?;
    }
    supersede_same_topic(store, &new_id)?;
    Ok(new_id)
}

/// Approve a proposed row. It becomes active, and any other active line on
/// the same topic is superseded. Approving twice leaves the row as it is.
pub fn approve_node(store: &AmrStore, id: &str, now_ms: u64) -> Result<String, AmrError> {
    if store.is_scratch() {
        return Err(AmrError::Scratch);
    }
    let loaded = store.load_live();
    let Some(old) = loaded.nodes.into_iter().find(|n| n.id == id) else {
        return Err(AmrError::MissingNode(id.to_string()));
    };
    if !old.tags.iter().any(|t| t == PROPOSED_TAG) {
        return Ok(id.to_string());
    }
    if !store.memory_writes() {
        return Err(AmrError::WritesOff);
    }
    let mut node = old;
    node.tags.retain(|t| t != PROPOSED_TAG);
    if !node.tags.iter().any(|t| t == APPROVED_TAG) {
        node.tags.push(APPROVED_TAG.to_string());
    }
    node.updated = crate::oauth::unix_ms_to_rfc3339(now_ms);
    if node.confidence < REFLECT_MIN_CONFIDENCE {
        node.confidence = REFLECT_MIN_CONFIDENCE;
    }
    store.resave(&node)?;
    supersede_same_topic(store, id)?;
    Ok(id.to_string())
}

/// Drop a proposed row. A row that is not proposed is left as it is.
pub fn reject_node(
    store: &AmrStore,
    id: &str,
    ask: UserForget,
    now_ms: u64,
) -> Result<(), AmrError> {
    let loaded = store.load_live();
    let Some(node) = loaded.nodes.iter().find(|n| n.id == id) else {
        return Err(AmrError::MissingNode(id.to_string()));
    };
    if !node.tags.iter().any(|t| t == PROPOSED_TAG) {
        return Ok(());
    }
    forget_node(store, id, ask, now_ms)
}

/// One active user-model line per topic. `new_id` supersedes every other
/// active line whose [`topic_slug`] matches and whose text differs.
/// Returns the ids it superseded.
pub fn supersede_same_topic(store: &AmrStore, new_id: &str) -> Result<Vec<String>, AmrError> {
    let (nodes, _) = store.recallable();
    let Some(new_node) = nodes.iter().find(|n| n.id == new_id) else {
        return Ok(Vec::new());
    };
    let topic = topic_slug(&new_node.body);
    if topic.is_empty() {
        return Ok(Vec::new());
    }
    let new_body = norm_line(&new_node.body);
    let edges = store.edges().unwrap_or_default();
    let mut out = Vec::new();
    for other in nodes
        .iter()
        .filter(|n| n.id != new_id && n.node_type.is_user_model() && topic_slug(&n.body) == topic)
    {
        if norm_line(&other.body) == new_body {
            continue;
        }
        let already = edges
            .iter()
            .any(|e| e.from == new_id && e.to == other.id && e.rel == EdgeRel::Supersedes);
        if already {
            continue;
        }
        store.link(new_id, &other.id, EdgeRel::Supersedes)?;
        out.push(other.id.clone());
    }
    Ok(out)
}

/// Topic key for a line. Times, weekdays and stopwords drop out, so
/// "lunch on Monday" and "lunch on Friday" share `lunch`.
pub fn topic_slug(text: &str) -> String {
    const STOP: &[&str] = &[
        "the", "and", "not", "but", "you", "your", "are", "was", "for", "with", "that", "this",
        "its", "it's", "is", "at", "on", "a", "an", "my", "to", "of", "in", "it", "be", "do", "me",
        "we",
    ];
    const WHEN: &[&str] = &[
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
        "noon",
        "midnight",
        "today",
        "tomorrow",
        "am",
        "pm",
    ];
    let mut kept = Vec::new();
    for word in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if word.len() <= 2 || STOP.contains(&word) || WHEN.contains(&word) {
            continue;
        }
        if word.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        kept.push(word.to_string());
    }
    kept.join("-")
}

fn first_line(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Forget one node because the user asked: tombstone, out of
/// `index.sqlite`, and one line (id only) in `dreams/forget-<date>.md`.
pub fn forget_node(
    store: &AmrStore,
    id: &str,
    _ask: UserForget,
    now_ms: u64,
) -> Result<(), AmrError> {
    store.forget(id)?;
    let date: String = crate::oauth::unix_ms_to_rfc3339(now_ms)
        .chars()
        .take(10)
        .collect();
    let dir = store.root().join("dreams");
    fs::create_dir_all(&dir).map_err(|err| AmrError::Io(err.to_string()))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("forget-{date}.md")))
        .map_err(|err| AmrError::Io(err.to_string()))?;
    writeln!(file, "- forgot {id} (you asked)").map_err(|err| AmrError::Io(err.to_string()))
}

/// Drop every line that repeats a forgotten node from the memory files that
/// go into a hub snapshot. Bullets and case don't matter.
pub fn strip_forgotten(store: &AmrStore, files: Vec<HubMemoryFile>) -> Vec<HubMemoryFile> {
    let forgotten = forgotten_lines(store);
    if forgotten.is_empty() {
        return files;
    }
    files
        .into_iter()
        .map(|mut file| {
            let kept: Vec<&str> = file
                .content
                .lines()
                .filter(|l| !forgotten.contains(&norm_line(l)))
                .collect();
            let mut content = kept.join("\n");
            if file.content.ends_with('\n') && !content.is_empty() {
                content.push('\n');
            }
            file.content = content;
            file
        })
        .collect()
}

/// Lines of forgotten nodes that no live node still holds: a dream merge
/// tombstones one copy of a line another node keeps, and that is not a forget.
fn forgotten_lines(store: &AmrStore) -> Vec<String> {
    let live: std::collections::BTreeSet<String> = store
        .recallable()
        .0
        .iter()
        .flat_map(|n| n.body.lines().map(norm_line).collect::<Vec<_>>())
        .collect();
    store
        .load_forgotten()
        .nodes
        .iter()
        .flat_map(|n| n.body.lines().map(norm_line).collect::<Vec<_>>())
        .filter(|l| !l.is_empty() && !live.contains(l))
        .collect()
}

fn norm_line(line: &str) -> String {
    let t = line.trim();
    let t = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .unwrap_or(t);
    t.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// What reflect would change in USER.md. Shown to the user; never written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReflectDiff {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

impl ReflectDiff {
    pub fn is_empty(&self) -> bool {
        self.add.is_empty() && self.remove.is_empty()
    }

    /// `+ line` / `- line`, removals first.
    pub fn render(&self) -> String {
        let mut out: Vec<String> = self.remove.iter().map(|l| format!("- {l}")).collect();
        out.extend(self.add.iter().map(|l| format!("+ {l}")));
        out.join("\n")
    }
}

/// Plain learned rows at or over [`REFLECT_MIN_CONFIDENCE`] that USER.md
/// lacks, and USER.md lines a forgotten or superseded node still holds.
/// Sealed nodes are never proposed: USER.md is plain text.
pub fn reflect_diff(store: &AmrStore, user_md: &str) -> ReflectDiff {
    let have: Vec<String> = user_md.lines().map(norm_line).collect();
    let (nodes, _) = store.recallable();
    let mut diff = ReflectDiff::default();
    for node in nodes.iter().filter(|n| proposable(n)) {
        let line = node
            .body
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("");
        if !line.is_empty()
            && !have.contains(&norm_line(line))
            && !diff.add.iter().any(|l| l == line)
        {
            diff.add.push(line.to_string());
        }
    }
    let superseded = store.superseded_ids();
    let live: std::collections::BTreeSet<String> = nodes
        .iter()
        .flat_map(|n| n.body.lines().map(norm_line).collect::<Vec<_>>())
        .collect();
    let mut stale = forgotten_lines(store);
    stale.extend(
        store
            .load_live()
            .nodes
            .into_iter()
            .filter(|n| superseded.contains(&n.id))
            .flat_map(|n| n.body.lines().map(norm_line).collect::<Vec<_>>())
            .filter(|l| !l.is_empty() && !live.contains(l)),
    );
    for line in user_md.lines() {
        if stale.contains(&norm_line(line)) {
            diff.remove.push(line.trim().to_string());
        }
    }
    diff
}

fn proposable(node: &Node) -> bool {
    matches!(
        node.node_type,
        NodeType::Fact | NodeType::Preference | NodeType::Routine | NodeType::Need
    ) && !node.sensitivity.sealed()
        && node.confidence >= REFLECT_MIN_CONFIDENCE
        && !node.source.starts_with("pulse:")
}

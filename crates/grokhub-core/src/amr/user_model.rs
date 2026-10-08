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
                    Ok(turn) => Self::Chat { thread: thread.to_string(), turn: Some(turn) },
                    Err(_) => Self::Chat { thread: rest.to_string(), turn: None },
                },
                None => Self::Chat { thread: rest.to_string(), turn: None },
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
            Self::Chat { turn: Some(turn), .. } => format!("you said it in chat (turn {turn})"),
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
            Self::Chat { thread, turn: Some(turn) } => format!("chat:{thread}#{turn}"),
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
}

impl MemoryRow {
    /// `- <text> · why: [<label>](<target>)`.
    pub fn line(&self) -> String {
        format!("- {} · why: [{}]({})", self.text, self.source.label(), self.source.target())
    }
}

/// Learned user-model rows (fact, preference, routine, need, mind prior),
/// newest first. Forgotten and superseded nodes are left out; sealed ones
/// show only while they open. The count is sealed nodes that stayed shut.
pub fn memory_rows(store: &AmrStore) -> (Vec<MemoryRow>, usize) {
    let (nodes, locked) = store.recallable();
    let mut rows: Vec<MemoryRow> = nodes
        .into_iter()
        .filter(|n| n.node_type.is_user_model())
        .map(|n| MemoryRow {
            text: n.body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string(),
            source: SourceLink::parse(&n.source),
            id: n.id,
            node_type: n.node_type,
            confidence: n.confidence,
            sensitivity: n.sensitivity,
            updated: n.updated,
        })
        .collect();
    rows.sort_by(|a, b| b.updated.cmp(&a.updated).then(a.id.cmp(&b.id)));
    (rows, locked)
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
    let (nodes, _) = store.recallable();
    let old = nodes
        .iter()
        .find(|n| n.id == id)
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
        sensitivity: if old.sensitivity.sealed() { old.sensitivity } else { super::write::sensitivity_for(text) },
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
    Ok(new_id)
}

/// Forget one node because the user asked: tombstone, out of
/// `index.sqlite`, and one line (id only) in `dreams/forget-<date>.md`.
pub fn forget_node(store: &AmrStore, id: &str, _ask: UserForget, now_ms: u64) -> Result<(), AmrError> {
    store.forget(id)?;
    let date: String = crate::oauth::unix_ms_to_rfc3339(now_ms).chars().take(10).collect();
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
            let kept: Vec<&str> = file.content.lines().filter(|l| !forgotten.contains(&norm_line(l))).collect();
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
    let live: std::collections::BTreeSet<String> =
        store.recallable().0.iter().flat_map(|n| n.body.lines().map(norm_line).collect::<Vec<_>>()).collect();
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
    let t = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).unwrap_or(t);
    t.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
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
        let line = node.body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
        if !line.is_empty() && !have.contains(&norm_line(line)) && !diff.add.iter().any(|l| l == line) {
            diff.add.push(line.to_string());
        }
    }
    let superseded = store.superseded_ids();
    let live: std::collections::BTreeSet<String> =
        nodes.iter().flat_map(|n| n.body.lines().map(norm_line).collect::<Vec<_>>()).collect();
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
    matches!(node.node_type, NodeType::Fact | NodeType::Preference | NodeType::Routine | NodeType::Need)
        && !node.sensitivity.sealed()
        && node.confidence >= REFLECT_MIN_CONFIDENCE
        && !node.source.starts_with("pulse:")
}

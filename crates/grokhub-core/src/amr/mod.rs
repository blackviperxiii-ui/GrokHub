//! Agent Memory Repo, milestone 0.
//!
//! Local files under `{config}/amr`. Nothing here is synced: do not add `amr/`
//! to hub sync in M0. A node is plain markdown you can `cat` or commit yourself.
//! `remember` redacts secrets before the file is written. There is no network
//! and no daemon. Spike-5a adds `index.sqlite`, an FTS5 index rebuilt from the
//! plain node files ([`AmrStore::rebuild_index`]); the files stay the truth.
//!
//! ```text
//! amr/
//!   README.md          amr_schema: 1, plus the conventions
//!   nodes/<id>.md      one preference, fact, decision, trail, person, or project
//!   edges/edges.jsonl  one JSON edge per line
//!   dreams/            reports, such as the one-time import-<date>.md
//! ```
//!
//! `/recall` stays on SOUL/USER/MEMORY unless `app.json` sets
//! `"memory_backend": "amr"`. That file is the cabin settings file. The default
//! backend is legacy, and a default save omits the key. M0 does not migrate old
//! files, does not dream, and does not take writes from `/learn`, `/memory`, or
//! the Pulse ledger.
//!
//! M1–M2 (only when the backend is `amr`): new remembers go here instead of
//! MEMORY.md ([`remember_line`]), `/recall` reads this store and the legacy
//! files together, `forget` leaves a tombstone (`nodes/<id>.tombstone`, the
//! node stays on disk), and [`import_legacy`] copies LearningState insights
//! and durable chip preferences in once, with deterministic ids.
//! Scratch is a flag on [`AmrStore`]: `remember` and `link` then return
//! [`AmrError::Scratch`] and write nothing.
//!
//! Spike-4b tiers: a [`Sensitivity::Plain`] node stays `nodes/<id>.md`. A
//! personal or sensitive node is sealed at rest as `nodes/<id>.sealed` by the
//! store's [`Sealer`] (AEAD, key in the OS keyring; grokhub-agent provides
//! it). No sealer, or a locked one, means [`AmrError::Paused`] and no write.
//! This crate stays free of crypto: it only calls the trait.
//!
//! Spike-5a user model: `fact`, `preference`, `routine`, `need` and
//! `mind_prior` nodes, written by rule-based signal writers ([`note_chat`],
//! corrections with `supersedes` / `contradicts` edges, edits, Pulse ledger
//! lines, card signals, usage), listed with their source by [`memory_rows`],
//! edited by [`edit_node`] and forgotten by [`forget_node`]. Reflect returns a
//! [`ReflectDiff`]; nothing writes USER.md on its own.

mod dream;
mod import;
mod index;
mod schema;
mod signal;
mod store;
mod user_model;
mod write;

pub use dream::{
    dream_report_path, latest_dream, DreamMerge, DreamOpts, DreamReport, DreamRetire, PruneReport, DREAM_DUP_JACCARD,
    DREAM_PROPOSE_CONFIDENCE, DREAM_RECENT_DAYS, DREAM_STALE_BELOW, DREAM_TAG_JACCARD, DREAM_TTL_DAYS,
};
pub use import::{durable_chip_prefs, import_legacy, write_import_report, ImportReport, ImportTally};
pub use schema::{Edge, EdgeRel, Node, NodeDraft, NodeHit, NodeId, NodeType, Sensitivity, AMR_SCHEMA};
pub use index::{AmrIndex, INDEX_FILE};
pub use signal::{
    detect_correction, import_pulse_ledger, note_card_signal, note_chat, note_usage, note_user_edit, pulse_ledger_dual,
    unsafe_to_learn, user_model_type, ChatLine, Correction, Noted, Usage,
};
pub use store::{AmrStore, ForgetReport, RecallReport};
pub use user_model::{
    edit_node, forget_node, memory_rows, memory_text, reflect_diff, strip_forgotten, MemoryRow, ReflectDiff, SourceLink,
    UserForget, REFLECT_MIN_CONFIDENCE,
};
pub use write::{line_id, node_type_for, remember_line, sensitivity_for, trail_id, LineWrite, Remembered};

/// `source` of a node a local indexer wrote (Spike-8a): `scope:<scope key>`.
/// The dream leaves these alone; "Forget these" in Settings retires them.
pub const SCOPE_SOURCE_PREFIX: &str = "scope:";

/// Seals and opens personal and sensitive nodes. `aad` binds a node to its id.
/// Errors are plain sentences for the user and never carry key material.
pub trait Sealer: Send + Sync {
    fn seal(&self, aad: &str, plain: &str) -> Result<String, String>;
    fn open(&self, aad: &str, sealed: &str) -> Result<String, String>;
}

/// Why a schema, store, or adapter call refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmrError {
    /// The markdown did not open and close a `---` frontmatter block.
    MissingFrontmatter,
    /// `type:` was not one of the six schema types.
    UnknownType(String),
    /// A frontmatter line was missing, repeated, or not `key: value`.
    BadFrontmatter(String),
    /// `confidence` was missing, not a number, or outside 0.0–1.0.
    ConfidenceOutOfRange,
    /// The id was empty, too long, started with `-`, or left `[a-z0-9-]`.
    BadId(String),
    /// `nodes/<id>.md` is already there. Remember does not overwrite.
    DuplicateId(String),
    /// `link` names a node file that is not in the store.
    MissingNode(String),
    /// Scratch is on. Nothing was written.
    Scratch,
    /// This backend does not take that write. Legacy keeps its own files.
    Unsupported,
    /// A node file or the edge log could not be read or written.
    Io(String),
    /// One `edges.jsonl` line was not an edge object.
    BadEdge(String),
    /// A personal or sensitive node can't be sealed right now (no keyring,
    /// missing key). Learning pauses; nothing was written.
    Paused(String),
}

impl std::fmt::Display for AmrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingFrontmatter => write!(f, "missing amr frontmatter"),
            Self::UnknownType(kind) => write!(f, "unknown amr node type: {kind}"),
            Self::BadFrontmatter(detail) => write!(f, "bad amr frontmatter: {detail}"),
            Self::ConfidenceOutOfRange => write!(f, "amr confidence must be from 0.0 to 1.0"),
            Self::BadId(id) => write!(f, "bad amr node id: {id}"),
            Self::DuplicateId(id) => write!(f, "amr node already exists: {id}"),
            Self::MissingNode(id) => write!(f, "amr node is missing: {id}"),
            Self::Scratch => write!(f, "scratch: amr writes are off"),
            Self::Unsupported => write!(f, "this memory backend does not support that write"),
            Self::Io(detail) => write!(f, "amr io: {detail}"),
            Self::BadEdge(detail) => write!(f, "bad amr edge: {detail}"),
            Self::Paused(why) => write!(f, "learning paused: {why}"),
        }
    }
}

impl std::error::Error for AmrError {}

/// Which store `/recall` reads. Missing settings stay [`Legacy`](Self::Legacy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryBackend {
    /// SOUL.md, USER.md, MEMORY.md, and learned insights.
    #[default]
    Legacy,
    /// `{config}/amr` nodes.
    Amr,
}

impl MemoryBackend {
    /// Default saves skip the settings key when this is true.
    pub fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy)
    }
}

/// Host-facing memory seam (harness design §9.4).
///
/// `recall`, `note`, `reflect`, `forget` and `scratch` (Spike-5a), plus the
/// M0 `remember` and `link`. Scratch is set on [`AmrStore`]; `scratch` reads
/// it, and every write then refuses.
pub trait MemoryEngine {
    /// Display lines for `/recall`. An empty store is an empty vec.
    fn recall(&self, query: &str) -> Vec<String>;

    /// Note one chat line during the chat (a correction supersedes).
    /// Legacy returns [`AmrError::Unsupported`].
    fn note(&self, line: &ChatLine<'_>, now_ms: u64) -> Result<Noted, AmrError>;

    /// What reflect would change in USER.md. Never writes.
    fn reflect(&self, user_md: &str) -> ReflectDiff;

    /// Write one node. Legacy returns [`AmrError::Unsupported`].
    fn remember(&self, draft: &NodeDraft) -> Result<NodeId, AmrError>;

    /// Record one edge. Legacy returns [`AmrError::Unsupported`].
    fn link(&self, from: &str, to: &str, rel: EdgeRel) -> Result<(), AmrError>;

    /// Tombstone one node so recall stops returning it. Legacy returns
    /// [`AmrError::Unsupported`].
    fn forget(&self, id: &str) -> Result<(), AmrError>;

    /// True when this chat is scratch: nothing is written.
    fn scratch(&self) -> bool;
}

impl MemoryEngine for AmrStore {
    fn recall(&self, query: &str) -> Vec<String> {
        AmrStore::recall(self, query)
            .into_iter()
            .map(|hit| hit.display())
            .collect()
    }

    fn note(&self, line: &ChatLine<'_>, now_ms: u64) -> Result<Noted, AmrError> {
        note_chat(self, line, now_ms)
    }

    fn reflect(&self, user_md: &str) -> ReflectDiff {
        reflect_diff(self, user_md)
    }

    fn remember(&self, draft: &NodeDraft) -> Result<NodeId, AmrError> {
        AmrStore::remember(self, draft)
    }

    fn link(&self, from: &str, to: &str, rel: EdgeRel) -> Result<(), AmrError> {
        AmrStore::link(self, from, to, rel)
    }

    fn forget(&self, id: &str) -> Result<(), AmrError> {
        AmrStore::forget(self, id)
    }

    fn scratch(&self) -> bool {
        self.is_scratch()
    }
}

/// Today's files. `recall` is [`crate::host_safety::recall_hits`]. Writes stay
/// where they already are; this adapter does not move them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyMemory {
    corpus: Vec<(String, String)>,
}

impl LegacyMemory {
    pub fn new(corpus: Vec<(String, String)>) -> Self {
        Self { corpus }
    }
}

impl MemoryEngine for LegacyMemory {
    fn recall(&self, query: &str) -> Vec<String> {
        let refs: Vec<(&str, &str)> = self
            .corpus
            .iter()
            .map(|(name, body)| (name.as_str(), body.as_str()))
            .collect();
        crate::host_safety::recall_hits(query, &refs)
    }

    fn note(&self, _line: &ChatLine<'_>, _now_ms: u64) -> Result<Noted, AmrError> {
        Err(AmrError::Unsupported)
    }

    fn reflect(&self, _user_md: &str) -> ReflectDiff {
        ReflectDiff::default()
    }

    fn remember(&self, _draft: &NodeDraft) -> Result<NodeId, AmrError> {
        Err(AmrError::Unsupported)
    }

    fn link(&self, _from: &str, _to: &str, _rel: EdgeRel) -> Result<(), AmrError> {
        Err(AmrError::Unsupported)
    }

    fn forget(&self, _id: &str) -> Result<(), AmrError> {
        Err(AmrError::Unsupported)
    }

    fn scratch(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod user_model_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_MD: &str = "\
---
id: pref-composer-glow
type: preference
created: 2026-10-06T11:00:00-05:00
updated: 2026-10-06T16:00:00Z
source: chat:thread-1
confidence: 0.75
tags: [ui, composer]
---
Glow stays white.
Second line.
";

    fn fixture_node() -> Node {
        Node {
            id: "pref-composer-glow".into(),
            node_type: NodeType::Preference,
            created: "2026-10-06T11:00:00-05:00".into(),
            updated: "2026-10-06T16:00:00Z".into(),
            source: "chat:thread-1".into(),
            confidence: 0.75,
            tags: vec!["ui".into(), "composer".into()],
            body: "Glow stays white.\nSecond line.\n".into(),
            consent_ref: String::new(),
            sensitivity: Sensitivity::Plain,
        }
    }

    fn draft(id: &str, body: &str) -> NodeDraft {
        NodeDraft {
            id: id.into(),
            node_type: NodeType::Fact,
            created: "2026-10-07T00:00:00Z".into(),
            updated: "2026-10-07T00:00:00Z".into(),
            source: "user".into(),
            confidence: 0.9,
            tags: vec!["dock".into()],
            body: body.into(),
            sensitivity: Sensitivity::Plain,
            consent_ref: String::new(),
        }
    }

    struct Tmp {
        path: std::path::PathBuf,
    }

    impl Tmp {
        fn new(label: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("grokhub-amr-{label}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            Self { path }
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn schema_round_trip_matches_the_literal_markdown_and_edge_line() {
        let node = fixture_node();
        assert_eq!(node.to_markdown(), FIXTURE_MD);
        assert_eq!(Node::from_markdown(FIXTURE_MD).unwrap(), node);
        assert_eq!(Node::from_markdown(&node.to_markdown()).unwrap(), node);
        assert_eq!(
            Node::from_markdown(FIXTURE_MD).unwrap().to_markdown(),
            FIXTURE_MD
        );

        let edge = Edge {
            from: "pref-composer-glow".into(),
            to: "fact-harbor".into(),
            rel: EdgeRel::LearnedFromSpan,
        };
        let line = serde_json::to_string(&edge).unwrap();
        assert_eq!(
            line,
            "{\"from\":\"pref-composer-glow\",\"to\":\"fact-harbor\",\"rel\":\"learned_from_span\"}"
        );
        let back: Edge = serde_json::from_str(&line).unwrap();
        assert_eq!(back, edge);
        assert_eq!(NodeType::Fact.as_str(), "fact");
        assert_eq!(NodeType::Decision.as_str(), "decision");
        assert_eq!(NodeType::Trail.as_str(), "trail");
        assert_eq!(NodeType::Person.as_str(), "person");
        assert_eq!(NodeType::Project.as_str(), "project");
        assert_eq!(NodeType::parse("project").unwrap(), NodeType::Project);
        assert_eq!(AMR_SCHEMA, 1);
    }

    #[test]
    fn parse_errors_are_unknown_type_missing_fence_bad_confidence_and_bad_id() {
        let unknown = "\
---
id: pref-composer-glow
type: widget
created: 2026-10-06T11:00:00-05:00
updated: 2026-10-06T16:00:00Z
source: chat:thread-1
confidence: 0.75
tags: [ui, composer]
---
Glow stays white.
";
        assert_eq!(
            Node::from_markdown(unknown).unwrap_err(),
            AmrError::UnknownType("widget".into())
        );
        assert_eq!(
            Node::from_markdown("id: pref-composer-glow\ntype: fact\n").unwrap_err(),
            AmrError::MissingFrontmatter
        );
        let confident = "\
---
id: pref-composer-glow
type: preference
created: 2026-10-06T11:00:00-05:00
updated: 2026-10-06T16:00:00Z
source: chat:thread-1
confidence: 1.5
tags: [ui, composer]
---
Glow stays white.
";
        assert_eq!(
            Node::from_markdown(confident).unwrap_err(),
            AmrError::ConfidenceOutOfRange
        );
        let traversed = "\
---
id: ../etc
type: preference
created: 2026-10-06T11:00:00-05:00
updated: 2026-10-06T16:00:00Z
source: chat:thread-1
confidence: 0.75
tags: [ui, composer]
---
Glow stays white.
";
        assert_eq!(
            Node::from_markdown(traversed).unwrap_err(),
            AmrError::BadId("../etc".into())
        );
    }

    #[test]
    fn node_ids_reject_separators_dots_and_a_leading_dash() {
        assert_eq!(
            NodeId::parse("pref-composer-1").unwrap().as_str(),
            "pref-composer-1"
        );
        assert_eq!(NodeId::parse("a").unwrap().as_str(), "a");
        assert_eq!(NodeId::parse(&"a".repeat(96)).unwrap().as_str().len(), 96);
        for bad in [
            "../etc",
            "..",
            ".",
            "foo/bar",
            "foo\\bar",
            "-abc",
            "",
            "HasUpper",
            "has space",
            "has.dot",
            &"a".repeat(97),
        ] {
            assert_eq!(
                NodeId::parse(bad).unwrap_err(),
                AmrError::BadId(bad.to_string()),
                "{bad}"
            );
        }
    }

    #[test]
    fn empty_store_recall_creates_nothing_and_init_is_idempotent() {
        let tmp = Tmp::new("empty");
        let store = AmrStore::at(&tmp.path);
        assert!(store.recall("x").is_empty());
        assert!(!tmp.path.exists(), "recall must not create the store");
        assert!(!store.is_initialized());
        assert_eq!(store.schema_version(), None);

        store.init().unwrap();
        assert!(tmp.path.join("nodes").is_dir());
        assert!(tmp.path.join("edges").is_dir());
        assert!(tmp.path.join("dreams").is_dir());
        let readme = std::fs::read_to_string(tmp.path.join("README.md")).unwrap();
        assert!(
            readme.lines().any(|line| line.trim() == "amr_schema: 1"),
            "{readme}"
        );
        assert_eq!(store.schema_version(), Some(1));
        assert!(store.is_initialized());
        assert!(store.recall("x").is_empty());

        let id = store
            .remember(&draft("fact-harbor", "harbor light\n"))
            .unwrap();
        let node_path = tmp.path.join("nodes").join(format!("{}.md", id.as_str()));
        let before = std::fs::read(&node_path).unwrap();
        std::fs::write(tmp.path.join("README.md"), "amr_schema: 1\nkeep me\n").unwrap();
        store.init().unwrap();
        assert_eq!(std::fs::read(&node_path).unwrap(), before);
        assert_eq!(
            std::fs::read_to_string(tmp.path.join("README.md")).unwrap(),
            "amr_schema: 1\nkeep me\n"
        );
        assert_eq!(store.schema_version(), Some(1));
    }

    #[test]
    fn adapter_contract_remembers_links_and_refuses_scratch_and_duplicates() {
        let tmp = Tmp::new("adapter");
        let mut store = AmrStore::at(&tmp.path);
        store.init().unwrap();
        let harbor = draft("fact-harbor", "harbor light\nshared token\n");
        let keel = NodeDraft {
            id: "pref-keel".into(),
            node_type: NodeType::Preference,
            body: "keel stays white\nshared token\n".into(),
            ..draft("pref-keel", "")
        };
        assert_eq!(
            MemoryEngine::remember(&store, &harbor).unwrap().as_str(),
            "fact-harbor"
        );
        assert_eq!(
            MemoryEngine::remember(&store, &keel).unwrap().as_str(),
            "pref-keel"
        );
        assert_eq!(
            MemoryEngine::recall(&store, "harbor"),
            vec!["amr:fact-harbor: harbor light".to_string()]
        );
        assert_eq!(
            MemoryEngine::recall(&store, "shared"),
            vec![
                "amr:fact-harbor: shared token".to_string(),
                "amr:pref-keel: shared token".to_string(),
            ]
        );
        let hits = store.recall("harbor");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "fact-harbor");
        assert_eq!(hits[0].line, "harbor light");
        assert_eq!(hits[0].display(), "amr:fact-harbor: harbor light");
        let lamp = NodeDraft {
            id: "fact-lighthouse".into(),
            body: "beacon\n".into(),
            tags: vec!["nav".into()],
            ..harbor.clone()
        };
        MemoryEngine::remember(&store, &lamp).unwrap();
        assert_eq!(
            MemoryEngine::recall(&store, "lighthouse"),
            vec!["amr:fact-lighthouse: fact-lighthouse".to_string()]
        );
        assert_eq!(
            MemoryEngine::recall(&store, "nav"),
            vec!["amr:fact-lighthouse: nav".to_string()]
        );

        let missing = store.link("fact-harbor", "missing-node", EdgeRel::Contradicts);
        assert_eq!(missing, Err(AmrError::MissingNode("missing-node".into())));
        assert!(!tmp.path.join("edges").join("edges.jsonl").exists());

        MemoryEngine::link(&store, "fact-harbor", "pref-keel", EdgeRel::References).unwrap();
        let raw = std::fs::read_to_string(tmp.path.join("edges").join("edges.jsonl")).unwrap();
        assert_eq!(
            raw,
            "{\"from\":\"fact-harbor\",\"to\":\"pref-keel\",\"rel\":\"references\"}\n"
        );
        assert_eq!(
            store.edges().unwrap(),
            vec![Edge {
                from: "fact-harbor".into(),
                to: "pref-keel".into(),
                rel: EdgeRel::References,
            }]
        );

        let dup = store.remember(&harbor).unwrap_err();
        assert_eq!(dup, AmrError::DuplicateId("fact-harbor".into()));
        let node_bytes = std::fs::read(tmp.path.join("nodes").join("fact-harbor.md")).unwrap();

        store.set_scratch(true);
        let blocked = NodeDraft {
            id: "fact-scratch".into(),
            body: "must not land\n".into(),
            ..harbor.clone()
        };
        assert_eq!(store.remember(&blocked).unwrap_err(), AmrError::Scratch);
        assert_eq!(
            store
                .link("fact-harbor", "pref-keel", EdgeRel::Supersedes)
                .unwrap_err(),
            AmrError::Scratch
        );
        assert!(!tmp.path.join("nodes").join("fact-scratch.md").exists());
        assert_eq!(
            std::fs::read(tmp.path.join("nodes").join("fact-harbor.md")).unwrap(),
            node_bytes
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path.join("edges").join("edges.jsonl")).unwrap(),
            raw
        );
    }

    #[test]
    fn scratch_on_a_missing_store_writes_nothing() {
        let tmp = Tmp::new("scratch");
        let mut store = AmrStore::at(&tmp.path);
        store.set_scratch(true);
        assert_eq!(
            store.remember(&draft("fact-harbor", "nope\n")).unwrap_err(),
            AmrError::Scratch
        );
        assert_eq!(
            store
                .link("fact-harbor", "pref-keel", EdgeRel::References)
                .unwrap_err(),
            AmrError::Scratch
        );
        assert!(!tmp.path.exists());
    }

    #[test]
    fn legacy_memory_recall_matches_literal_lines_and_writes_are_unsupported() {
        let corpus = vec![
            ("SOUL.md".into(), "soul line\n".into()),
            ("USER.md".into(), "editor: nvim\n".into()),
            ("MEMORY.md".into(), "no match\n".into()),
            ("learned".into(), "learned nvim tip\n".into()),
        ];
        let engine = LegacyMemory::new(corpus.clone());
        let refs: Vec<(&str, &str)> = corpus
            .iter()
            .map(|(name, body)| (name.as_str(), body.as_str()))
            .collect();
        let expect = vec![
            "USER.md:1: editor: nvim".to_string(),
            "learned:1: learned nvim tip".to_string(),
        ];
        assert_eq!(engine.recall("nvim"), expect);
        assert_eq!(crate::host_safety::recall_hits("nvim", &refs), expect);
        assert_eq!(
            engine
                .remember(&draft("fact-harbor", "nope\n"))
                .unwrap_err(),
            AmrError::Unsupported
        );
        assert_eq!(
            engine
                .link("fact-harbor", "pref-keel", EdgeRel::References)
                .unwrap_err(),
            AmrError::Unsupported
        );
    }

    #[test]
    fn remember_redacts_a_secret_in_the_body_source_and_tags() {
        let tmp = Tmp::new("redact");
        let store = AmrStore::at(&tmp.path);
        store.init().unwrap();
        let secret = "sk-abcdefghijklmnopqrstuv";
        let written = store
            .remember(&NodeDraft {
                id: "fact-key".into(),
                node_type: NodeType::Fact,
                created: "2026-10-07T00:00:00Z".into(),
                updated: "2026-10-07T00:00:00Z".into(),
                source: format!("note {secret}"),
                confidence: 1.0,
                tags: vec!["plain".into(), secret.into()],
                body: format!("see {secret} now\n"),
                sensitivity: Sensitivity::Plain,
                consent_ref: String::new(),
            })
            .unwrap();
        assert_eq!(written.as_str(), "fact-key");
        let disk = std::fs::read_to_string(tmp.path.join("nodes").join("fact-key.md")).unwrap();
        assert!(
            !disk.contains(secret),
            "secret landed in the node file: {disk}"
        );
        let node = Node::from_markdown(&disk).unwrap();
        assert_eq!(node.body, "see [redacted] now\n");
        assert_eq!(node.source, "note [redacted]");
        assert_eq!(
            node.tags,
            vec!["plain".to_string(), "[redacted]".to_string()]
        );
        assert_eq!(
            store.recall("see"),
            vec![NodeHit {
                id: "fact-key".into(),
                line: "see [redacted] now".into(),
            }]
        );
    }

    #[test]
    fn recall_skips_a_malformed_node_and_caps_at_twenty() {
        let tmp = Tmp::new("cap");
        let store = AmrStore::at(&tmp.path);
        store.init().unwrap();
        std::fs::write(tmp.path.join("nodes").join("broken.md"), "not a node\n").unwrap();
        for n in 0..21 {
            store
                .remember(&draft(&format!("n{n:02}"), "capword\n"))
                .unwrap();
        }
        let hits = MemoryEngine::recall(&store, "capword");
        assert_eq!(hits.len(), 20);
        assert_eq!(hits[0], "amr:n00: capword");
        assert_eq!(hits[19], "amr:n19: capword");
        assert!(!hits
            .iter()
            .any(|hit| hit.contains("n20") || hit.contains("broken")));
        assert!(store.recall("not a node").is_empty());
    }

    /// A stand-in sealer for core tests (the real one is ChaCha20-Poly1305 in
    /// grokhub-agent): hex with a label, refusing a wrong label or when "locked".
    struct FakeSealer {
        locked: std::sync::atomic::AtomicBool,
    }

    impl Sealer for FakeSealer {
        fn seal(&self, aad: &str, plain: &str) -> Result<String, String> {
            if self.locked.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("Private data is locked".into());
            }
            Ok(format!("fake:{}:{}", hex::encode(aad), hex::encode(plain)))
        }
        fn open(&self, aad: &str, sealed: &str) -> Result<String, String> {
            if self.locked.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("Private data is locked".into());
            }
            let rest = sealed
                .strip_prefix(&format!("fake:{}:", hex::encode(aad)))
                .ok_or("wrong node")?;
            String::from_utf8(hex::decode(rest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
        }
    }

    #[test]
    fn personal_nodes_are_sealed_and_a_locked_sealer_writes_nothing() {
        let tmp = Tmp::new("sealed");
        let sealer = std::sync::Arc::new(FakeSealer { locked: false.into() });
        let store = AmrStore::at(&tmp.path).with_sealer(sealer.clone());
        store.init().unwrap();
        let personal = NodeDraft { sensitivity: Sensitivity::Personal, ..draft("fact-home", "Home harbor is Pier 9.") };
        store.remember(&personal).unwrap();
        store.remember(&draft("fact-dock", "Dock layout is plain.")).unwrap();
        let sealed = tmp.path.join("nodes/fact-home.sealed");
        let text = std::fs::read_to_string(&sealed).unwrap();
        assert!(!text.contains("Pier 9") && !text.contains("harbor"), "{text}");
        assert!(!tmp.path.join("nodes/fact-home.md").exists());
        assert!(tmp.path.join("nodes/fact-dock.md").exists(), "plain nodes stay plain");
        let hits = store.recall("pier");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "fact-home");
        assert_eq!(store.remember(&personal), Err(AmrError::DuplicateId("fact-home".into())));
        assert_eq!(
            store.remember(&NodeDraft { sensitivity: Sensitivity::Plain, ..personal.clone() }),
            Err(AmrError::DuplicateId("fact-home".into())),
            "a sealed id can't be shadowed by a plain one"
        );
        store.link("fact-home", "fact-dock", EdgeRel::References).unwrap();

        // Locked: recall skips sealed nodes and says so; writes pause and leave no file.
        sealer.locked.store(true, std::sync::atomic::Ordering::SeqCst);
        let report = store.recall_report("pier");
        assert!(report.hits.is_empty());
        assert_eq!(report.locked, 1);
        assert_eq!(report.why.as_deref(), Some("Private data is locked"));
        assert_eq!(store.recall("layout").len(), 1, "plain nodes still recall");
        let more = NodeDraft { sensitivity: Sensitivity::Sensitive, ..draft("fact-card", "Card ends 4242.") };
        assert_eq!(store.remember(&more), Err(AmrError::Paused("Private data is locked".into())));
        assert!(!tmp.path.join("nodes/fact-card.sealed").exists());
        assert!(!tmp.path.join("nodes/fact-card.md").exists(), "never a plaintext fallback");

        // No sealer at all (a store opened without one): same, fail closed.
        let bare = AmrStore::at(&tmp.path);
        assert_eq!(bare.recall_report("pier").locked, 1);
        assert!(matches!(bare.remember(&more), Err(AmrError::Paused(_))));
        assert_eq!(std::fs::read_to_string(&sealed).unwrap(), text, "nothing dropped");
    }

    fn node_files(root: &std::path::Path) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(root.join("nodes"))
            .map(|read| {
                read.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out
    }

    #[test]
    fn forget_leaves_a_tombstone_that_recall_skips_and_scratch_blocks() {
        let tmp = Tmp::new("forget");
        let mut store = AmrStore::at(&tmp.path);
        store.init().unwrap();
        store.remember(&draft("fact-harbor", "harbor light\n")).unwrap();
        store.remember(&draft("fact-quay", "quay harbor lamp\n")).unwrap();
        store.remember(&draft("fact-keel", "keel stays white\n")).unwrap();
        let before = std::fs::read(tmp.path.join("nodes/fact-harbor.md")).unwrap();

        MemoryEngine::forget(&store, "fact-harbor").unwrap();
        assert!(store.is_forgotten("fact-harbor"));
        assert_eq!(std::fs::read(tmp.path.join("nodes/fact-harbor.md")).unwrap(), before, "the node stays on disk");
        let marker = std::fs::read_to_string(tmp.path.join("nodes/fact-harbor.tombstone")).unwrap();
        assert!(marker.starts_with("forgotten: ") && marker.ends_with("Z\n"), "{marker}");
        assert_eq!(MemoryEngine::recall(&store, "harbor"), vec!["amr:fact-quay: quay harbor lamp".to_string()]);
        // A second forget is a no-op, a missing id is an error.
        store.forget("fact-harbor").unwrap();
        assert_eq!(store.forget("fact-nope"), Err(AmrError::MissingNode("fact-nope".into())));
        assert_eq!(store.forget("../etc"), Err(AmrError::BadId("../etc".into())));

        // forget_matching tombstones every live match and says which.
        let report = store.forget_matching("HARBOR").unwrap();
        assert_eq!(report, ForgetReport { forgotten: vec!["fact-quay".into()], locked: 0 });
        assert!(store.recall("harbor").is_empty());
        assert_eq!(store.recall("keel").len(), 1);
        assert_eq!(store.node_file_count(), 3);

        // Revive lifts it again.
        store.revive("fact-quay").unwrap();
        assert_eq!(store.recall("quay").len(), 1);

        store.set_scratch(true);
        assert_eq!(store.forget("fact-keel"), Err(AmrError::Scratch));
        assert_eq!(store.forget_matching("keel"), Err(AmrError::Scratch));
        assert_eq!(store.revive("fact-harbor"), Err(AmrError::Scratch));
        assert!(!tmp.path.join("nodes/fact-keel.tombstone").exists());
        assert!(tmp.path.join("nodes/fact-harbor.tombstone").exists());
        assert_eq!(
            LegacyMemory::new(Vec::new()).forget("fact-keel"),
            Err(AmrError::Unsupported)
        );
    }

    #[test]
    fn a_merged_line_remembered_again_comes_back_and_an_edit_back_hides_one() {
        let tmp = Tmp::new("supersede");
        let store = AmrStore::at(&tmp.path);
        store.init().unwrap();
        let live = |store: &AmrStore| {
            let mut ids: Vec<String> = store.recallable().0.into_iter().map(|n| n.id).collect();
            ids.sort();
            ids
        };
        // The dream merged A into B (edge B→A, A tombstoned), then B was forgotten.
        store.remember(&draft("fact-old", "dark theme in the editor\n")).unwrap();
        store.remember(&draft("fact-new", "the dark theme in the editor\n")).unwrap();
        store.link("fact-new", "fact-old", EdgeRel::Supersedes).unwrap();
        store.forget("fact-old").unwrap();
        store.forget("fact-new").unwrap();
        assert!(live(&store).is_empty());
        // Remembering A again revives it, and it is recallable.
        store.revive("fact-old").unwrap();
        assert_eq!(live(&store), vec!["fact-old".to_string()]);

        // Edit tabs → spaces (spaces supersedes tabs), then back to tabs.
        store.remember(&draft("fact-tabs", "I prefer tabs\n")).unwrap();
        store.remember(&draft("fact-spaces", "I prefer spaces\n")).unwrap();
        store.link("fact-spaces", "fact-tabs", EdgeRel::Supersedes).unwrap();
        assert_eq!(live(&store), vec!["fact-old".to_string(), "fact-spaces".to_string()]);
        store.link("fact-tabs", "fact-spaces", EdgeRel::Supersedes).unwrap();
        assert_eq!(live(&store), vec!["fact-old".to_string(), "fact-tabs".to_string()], "the edit back leaves tabs, not nothing");
    }

    #[test]
    fn remember_line_is_one_node_per_line_redacts_and_pauses_personal() {
        let tmp = Tmp::new("line");
        let store = AmrStore::at(&tmp.path);
        let line = |text: &'static str, revive: bool| LineWrite {
            text,
            source: "chat:thread-7",
            tags: vec!["reflect".into()],
            confidence: 0.7,
            revive,
        };
        let first = remember_line(&store, &line("I prefer the harbor light on", false), 1_791_331_200_000).unwrap();
        let id = line_id("I prefer the harbor light on");
        assert_eq!(id.len(), "mem-".len() + 12);
        assert!(id.starts_with("mem-"), "{id}");
        assert_eq!(first, Remembered::New(id.clone()));
        assert_eq!(line_id("  i PREFER the   harbor light on "), id);
        let again = remember_line(&store, &line("i prefer the harbor light on", false), 1_791_331_300_000).unwrap();
        assert_eq!(again, Remembered::Known(id.clone()));
        assert_eq!(node_files(&tmp.path), vec![format!("{id}.md")]);
        let node = Node::from_markdown(&std::fs::read_to_string(tmp.path.join(format!("nodes/{id}.md"))).unwrap()).unwrap();
        assert_eq!(node.node_type, NodeType::Preference);
        assert_eq!(node.source, "chat:thread-7");
        assert_eq!(node.created, "2026-10-07T00:00:00Z");
        assert_eq!(node.tags, vec!["reflect".to_string()]);
        assert_eq!(node.body, "I prefer the harbor light on\n");

        // Forgotten stays forgotten for reflect; an explicit remember revives it.
        store.forget(&id).unwrap();
        remember_line(&store, &line("I prefer the harbor light on", false), 1).unwrap();
        assert!(store.recall("harbor").is_empty());
        remember_line(&store, &line("I prefer the harbor light on", true), 1).unwrap();
        assert_eq!(store.recall("harbor").len(), 1);

        let secret = "sk-abcdefghijklmnopqrstuv";
        let keyed = remember_line(
            &store,
            &LineWrite { text: "the dock key is sk-abcdefghijklmnopqrstuv", ..line("", false) },
            1,
        )
        .unwrap();
        let disk = std::fs::read_to_string(tmp.path.join(format!("nodes/{}.md", keyed.id()))).unwrap();
        assert!(!disk.contains(secret), "{disk}");
        assert!(disk.contains("the dock key is [redacted]\n"), "{disk}");
        assert_eq!(keyed.id(), line_id("the dock key is [redacted]"));

        // Personal with no sealer: paused, and no file of either kind.
        let personal = LineWrite { text: "my email is ada@example.com", ..line("", false) };
        assert_eq!(sensitivity_for(personal.text), Sensitivity::Personal);
        assert!(matches!(remember_line(&store, &personal, 1), Err(AmrError::Paused(_))));
        let pid = line_id(personal.text);
        assert!(!tmp.path.join(format!("nodes/{pid}.md")).exists());
        assert!(!tmp.path.join(format!("nodes/{pid}.sealed")).exists());
        assert_eq!(node_files(&tmp.path).len(), 2, "{:?}", node_files(&tmp.path));
        assert_eq!(
            remember_line(&store, &LineWrite { text: "  ", ..line("", false) }, 1),
            Err(AmrError::BadFrontmatter("body".into()))
        );
        assert_eq!(node_type_for("the ferry leaves at nine"), NodeType::Fact);
    }

    fn import_fixture() -> (Vec<crate::learning::LearningInsight>, crate::chips::ChipMemory) {
        let insight = |key: &str, text: &str, hits: u32| crate::learning::LearningInsight {
            key: key.into(),
            text: text.into(),
            hits,
        };
        let insights = vec![
            insight("pref:editor", "prefer nvim for quick edits", 3),
            insight("fact:ferry", "the harbor ferry leaves at nine", 1),
            insight("fact:dock", "dock seven holds the spare sails", 1),
            insight("pref:style:short", "They want short replies.", 2),
            insight("fact:quay", "the quay lantern is kept in the shed", 1),
            insight("project:keel", "project keel ships on fridays", 1),
            insight("fact:tide", "high tide is the best time to launch", 9),
        ];
        let chips: crate::chips::ChipMemory = serde_json::from_value(serde_json::json!({
            "version": 1,
            "hits": [
                {"key": "chat:plan", "label": "Plan the week", "value": "typed words stay out", "kind": "chat", "uses": 4, "picks": 3},
                {"key": "chat:once", "label": "Once only", "value": "x", "kind": "chat", "uses": 1, "picks": 1},
                {"key": "chat:nope", "label": "Dismissed", "value": "y", "kind": "chat", "uses": 3, "picks": 3, "dismisses": 1},
                {"key": "chat:typed", "label": "Typed", "value": "z", "kind": "chat", "uses": 5, "typedUses": 5, "picks": 0}
            ],
            "transitions": {"chat:plan": {"chat:once": 2}},
            "lastChipKey": "chat:plan",
            "lastSlash": "/imagine",
            "lastSurface": "skills",
            "totalEvents": 14,
            "updatedAt": 1791331200000u64
        }))
        .unwrap();
        (insights, chips)
    }

    #[test]
    fn import_turns_seven_insights_and_three_chip_fields_into_ten_nodes_once() {
        let tmp = Tmp::new("import");
        let store = AmrStore::at(&tmp.path);
        store.init().unwrap();
        let (insights, chips) = import_fixture();
        assert_eq!(
            durable_chip_prefs(&chips),
            vec![
                ("last_slash".to_string(), "From home they reach for /imagine.".to_string()),
                ("last_surface".to_string(), "From home they come back to skills.".to_string()),
                ("pick:chat:plan".to_string(), "They pick the \"Plan the week\" chip.".to_string()),
            ]
        );
        let report = import_legacy(&store, &insights, &chips, 1_791_331_200_000);
        assert_eq!(report.imported(), 10);
        assert_eq!(report.skipped(), 0);
        assert_eq!(report.learning_state.imported, 7);
        assert_eq!(report.chips.imported, 3);
        assert_eq!(
            report.status_line(),
            "Memory import: 10 imported, 0 skipped (learning_state 7, chips 3)."
        );
        let files = node_files(&tmp.path);
        assert_eq!(files.len(), 10, "{files:?}");
        assert!(files.iter().all(|f| f.starts_with("import-") && f.len() == "import-".len() + 12 + ".md".len()), "{files:?}");
        let path = write_import_report(&store, &report).unwrap();
        assert_eq!(path, tmp.path.join("dreams").join("import-2026-10-07.md"));
        let md = std::fs::read_to_string(&path).unwrap();
        assert!(md.starts_with("# Import 2026-10-07\n\n10 imported, 0 skipped.\n"), "{md}");
        assert!(md.contains("## learning_state\n\n7 imported, 0 skipped.\n"), "{md}");
        assert!(md.contains("## chips\n\n3 imported, 0 skipped.\n"), "{md}");
        assert!(!md.contains("nvim") && !md.contains("typed words"), "the report carries counts, not notes: {md}");

        let pref = store.recall("nvim");
        assert_eq!(pref.len(), 1);
        let node = Node::from_markdown(
            &std::fs::read_to_string(tmp.path.join(format!("nodes/{}.md", pref[0].id))).unwrap(),
        )
        .unwrap();
        assert_eq!(node.node_type, NodeType::Preference);
        assert_eq!(node.source, "learning_state");
        assert_eq!(node.body, "prefer nvim for quick edits\n");
        let tide = &store.recall("high tide")[0];
        let tide = Node::from_markdown(&std::fs::read_to_string(tmp.path.join(format!("nodes/{}.md", tide.id))).unwrap()).unwrap();
        assert_eq!(tide.node_type, NodeType::Fact);
        assert_eq!(tide.confidence, 0.9);
        let chip = &store.recall("plan the week")[0];
        let chip = Node::from_markdown(&std::fs::read_to_string(tmp.path.join(format!("nodes/{}.md", chip.id))).unwrap()).unwrap();
        assert_eq!(chip.source, "chips");
        assert_eq!(chip.node_type, NodeType::Preference);
        assert!(store.recall("typed words").is_empty(), "hit values never become nodes");

        let second = import_legacy(&store, &insights, &chips, 1_791_331_300_000);
        assert_eq!(second.imported(), 0);
        assert_eq!(second.skipped(), 10);
        assert_eq!(second.learning_state.skipped.get("already imported"), Some(&7));
        assert_eq!(node_files(&tmp.path).len(), 10);

        // Habit lines are rebuilt from chips, so they are skipped with a reason.
        let habits = vec![crate::learning::LearningInsight {
            key: "habit:21:chat:plan".into(),
            text: "Around 21:00 they use Plan the week.".into(),
            hits: 1,
        }];
        let third = import_legacy(&store, &habits, &crate::chips::ChipMemory::default(), 1);
        assert_eq!(third.imported(), 0);
        assert_eq!(third.learning_state.skipped.get("chip habit, rebuilt from chips each night"), Some(&1));
    }
}

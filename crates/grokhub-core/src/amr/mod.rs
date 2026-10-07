//! Agent Memory Repo, milestone 0.
//!
//! Local files under `{config}/amr`. Nothing here is synced: do not add `amr/`
//! to hub sync in M0. A node is plain markdown you can `cat` or commit yourself.
//! `remember` redacts secrets before the file is written. There is no network,
//! no daemon, and no sqlite index.
//!
//! ```text
//! amr/
//!   README.md          amr_schema: 1, plus the conventions
//!   nodes/<id>.md      one preference, fact, decision, trail, person, or project
//!   edges/edges.jsonl  one JSON edge per line
//!   dreams/            overnight reports; M0 does not write them
//! ```
//!
//! `/recall` stays on SOUL/USER/MEMORY unless `app.json` sets
//! `"memory_backend": "amr"`. That file is the cabin settings file. The default
//! backend is legacy, and a default save omits the key. M0 does not migrate old
//! files, does not dream, and does not take writes from `/learn`, `/memory`, or
//! the Pulse ledger. `forget` lands in M1+ (a tombstone, not a silent delete).
//! Scratch is a flag on [`AmrStore`]: `remember` and `link` then return
//! [`AmrError::Scratch`] and write nothing.

mod schema;
mod store;

pub use schema::{Edge, EdgeRel, Node, NodeDraft, NodeHit, NodeId, NodeType, AMR_SCHEMA};
pub use store::AmrStore;

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
/// M0 is `recall`, `remember`, and `link`. `forget` lands in M1+. Scratch is
/// not a trait method: set it on [`AmrStore`] so writes refuse.
pub trait MemoryEngine {
    /// Display lines for `/recall`. An empty store is an empty vec.
    fn recall(&self, query: &str) -> Vec<String>;

    /// Write one node. Legacy returns [`AmrError::Unsupported`].
    fn remember(&self, draft: &NodeDraft) -> Result<NodeId, AmrError>;

    /// Record one edge. Legacy returns [`AmrError::Unsupported`].
    fn link(&self, from: &str, to: &str, rel: EdgeRel) -> Result<(), AmrError>;
}

impl MemoryEngine for AmrStore {
    fn recall(&self, query: &str) -> Vec<String> {
        AmrStore::recall(self, query)
            .into_iter()
            .map(|hit| hit.display())
            .collect()
    }

    fn remember(&self, draft: &NodeDraft) -> Result<NodeId, AmrError> {
        AmrStore::remember(self, draft)
    }

    fn link(&self, from: &str, to: &str, rel: EdgeRel) -> Result<(), AmrError> {
        AmrStore::link(self, from, to, rel)
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

    fn remember(&self, _draft: &NodeDraft) -> Result<NodeId, AmrError> {
        Err(AmrError::Unsupported)
    }

    fn link(&self, _from: &str, _to: &str, _rel: EdgeRel) -> Result<(), AmrError> {
        Err(AmrError::Unsupported)
    }
}

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
}

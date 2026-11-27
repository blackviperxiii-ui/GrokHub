//! The memory as a graph for the person's own map: each live node with a
//! short label and its state, and the edges between the nodes shown.
//!
//! A read for the desktop app only. Forgotten (tombstoned) nodes are left
//! out. A sealed (personal or sensitive) node is listed with its type and
//! state but no label, so its words stay closed until the person opens it.
//! Superseded, proposed and paused nodes are kept and flagged, so the map can
//! draw a line of versions and the person can act on a held-back line.

use std::collections::BTreeSet;

use serde::Serialize;

use super::{AmrError, AmrStore, EdgeRel};

/// The most nodes one map holds, newest first. A bigger store says so in
/// [`MemoryGraph::truncated`].
pub const GRAPH_NODE_CAP: usize = 400;

/// The longest label, in characters, cut from a node's first line.
const LABEL_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphNode {
    pub id: String,
    /// The node type, as its frontmatter spells it (`fact`, `person`, ...).
    pub kind: &'static str,
    /// The first line of the body, cut short. Empty for a sealed node.
    pub label: String,
    pub confidence: f32,
    pub updated: String,
    pub sealed: bool,
    pub proposed: bool,
    pub paused: bool,
    pub superseded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeRel,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryGraph {
    pub nodes: Vec<GraphNode>,
    /// Edges whose two ends are both in `nodes`, in log order, each once.
    pub edges: Vec<GraphEdge>,
    /// Sealed nodes that could not be opened, so are not on the map.
    pub locked: usize,
    /// True when the store had more live nodes than [`GRAPH_NODE_CAP`].
    pub truncated: bool,
}

impl AmrStore {
    /// The graph of the live memory, at most `cap` nodes, most recently
    /// updated first. A scratch (incognito) store has no memory to show.
    pub fn graph(&self, cap: usize) -> Result<MemoryGraph, AmrError> {
        if self.is_scratch() {
            return Err(AmrError::Scratch);
        }
        let loaded = self.load_live();
        let superseded = self.superseded_ids();
        let mut nodes = loaded.nodes;
        nodes.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
        let truncated = nodes.len() > cap;
        nodes.truncate(cap);
        let nodes: Vec<GraphNode> = nodes
            .into_iter()
            .map(|node| {
                let sealed = node.sensitivity.sealed();
                GraphNode {
                    kind: node.node_type.as_str(),
                    label: if sealed {
                        String::new()
                    } else {
                        label_of(&node.body)
                    },
                    confidence: node.confidence,
                    sealed,
                    proposed: node.tags.iter().any(|tag| tag == "proposed"),
                    paused: self.is_paused(&node.id),
                    superseded: superseded.contains(&node.id),
                    updated: node.updated,
                    id: node.id,
                }
            })
            .collect();
        let shown: BTreeSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
        let mut seen = BTreeSet::new();
        let edges = self
            .edges()?
            .into_iter()
            .filter(|edge| shown.contains(edge.from.as_str()) && shown.contains(edge.to.as_str()))
            .filter(|edge| seen.insert((edge.from.clone(), edge.to.clone(), edge.rel as u8)))
            .map(|edge| GraphEdge {
                from: edge.from,
                to: edge.to,
                kind: edge.rel,
            })
            .collect();
        Ok(MemoryGraph {
            nodes,
            edges,
            locked: loaded.locked,
            truncated,
        })
    }
}

fn label_of(body: &str) -> String {
    let line = body.lines().map(str::trim).find(|line| !line.is_empty());
    let line = line.unwrap_or_default();
    let mut label: String = line.chars().take(LABEL_CHARS).collect();
    if line.chars().count() > LABEL_CHARS {
        label.push('…');
    }
    label
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::amr::{NodeDraft, NodeType, Sealer, Sensitivity};

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("grokhub-amr-graph-{label}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct HexSealer;

    impl Sealer for HexSealer {
        fn seal(&self, aad: &str, plain: &str) -> Result<String, String> {
            Ok(format!("hex:{}:{}", hex::encode(aad), hex::encode(plain)))
        }
        fn open(&self, aad: &str, sealed: &str) -> Result<String, String> {
            let rest = sealed
                .strip_prefix(&format!("hex:{}:", hex::encode(aad)))
                .ok_or("wrong node")?;
            String::from_utf8(hex::decode(rest).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        }
    }

    fn draft(id: &str, updated: &str, body: &str) -> NodeDraft {
        NodeDraft {
            id: id.into(),
            node_type: NodeType::Fact,
            created: "2026-10-07T00:00:00Z".into(),
            updated: updated.into(),
            source: "user".into(),
            confidence: 0.9,
            tags: vec![],
            body: body.into(),
            sensitivity: Sensitivity::Plain,
            consent_ref: String::new(),
        }
    }

    fn store(tmp: &Tmp) -> AmrStore {
        let store = AmrStore::at(&tmp.0).with_sealer(Arc::new(HexSealer));
        store.init().unwrap();
        store
    }

    /// Six nodes: a refit project, the dock it references, an old dock line
    /// the new one supersedes, a paused routine, a proposed need, a sealed
    /// person, and one forgotten fact with an edge to the dock.
    fn fixture(store: &AmrStore) {
        let day = |d: u32| format!("2026-10-{d:02}T00:00:00Z");
        store
            .remember(&NodeDraft {
                node_type: NodeType::Project,
                ..draft("boat-refit", &day(9), "Refit the boat this winter.\nMore.")
            })
            .unwrap();
        store
            .remember(&draft("dock-layout", &day(8), "Berth 4 is ours."))
            .unwrap();
        store
            .remember(&draft("dock-old", &day(2), "Berth 2 is ours."))
            .unwrap();
        store
            .remember(&NodeDraft {
                node_type: NodeType::Routine,
                ..draft("hull-check", &day(7), "Check the hull on Sundays.")
            })
            .unwrap();
        store
            .remember(&NodeDraft {
                node_type: NodeType::Need,
                tags: vec!["proposed".into()],
                ..draft("new-sails", &day(6), "Needs new sails.")
            })
            .unwrap();
        store
            .remember(&NodeDraft {
                node_type: NodeType::Person,
                sensitivity: Sensitivity::Personal,
                ..draft("harbor-master", &day(5), "Private note about a person.")
            })
            .unwrap();
        store
            .remember(&draft("gone-fact", &day(4), "Forget me."))
            .unwrap();
        store
            .link("boat-refit", "dock-layout", EdgeRel::References)
            .unwrap();
        store
            .link("boat-refit", "dock-layout", EdgeRel::References)
            .unwrap();
        store
            .link("dock-layout", "dock-old", EdgeRel::Supersedes)
            .unwrap();
        store
            .link("hull-check", "harbor-master", EdgeRel::Contradicts)
            .unwrap();
        store
            .link("gone-fact", "dock-layout", EdgeRel::References)
            .unwrap();
        store.set_paused("hull-check", true).unwrap();
        store.forget("gone-fact").unwrap();
    }

    #[test]
    fn the_graph_lists_live_nodes_newest_first_with_their_state_and_edges() {
        let tmp = Tmp::new("full");
        let store = store(&tmp);
        fixture(&store);
        let graph = store.graph(GRAPH_NODE_CAP).unwrap();
        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "boat-refit",
                "dock-layout",
                "hull-check",
                "new-sails",
                "harbor-master",
                "dock-old"
            ]
        );
        let node = |id: &str| graph.nodes.iter().find(|n| n.id == id).unwrap();
        assert_eq!(node("boat-refit").kind, "project");
        assert_eq!(node("boat-refit").label, "Refit the boat this winter.");
        assert!(node("dock-old").superseded);
        assert!(!node("dock-layout").superseded);
        assert!(node("hull-check").paused);
        assert!(node("new-sails").proposed);
        let edges: Vec<(&str, &str, EdgeRel)> = graph
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str(), e.kind))
            .collect();
        assert_eq!(
            edges,
            [
                ("boat-refit", "dock-layout", EdgeRel::References),
                ("dock-layout", "dock-old", EdgeRel::Supersedes),
                ("hull-check", "harbor-master", EdgeRel::Contradicts),
            ]
        );
        assert_eq!((graph.locked, graph.truncated), (0, false));
    }

    #[test]
    fn a_sealed_node_is_listed_without_its_words() {
        let tmp = Tmp::new("sealed");
        let store = store(&tmp);
        fixture(&store);
        let graph = store.graph(GRAPH_NODE_CAP).unwrap();
        let person = graph
            .nodes
            .iter()
            .find(|n| n.id == "harbor-master")
            .unwrap();
        assert!(person.sealed);
        assert_eq!((person.kind, person.label.as_str()), ("person", ""));
        let json = serde_json::to_string(&graph).unwrap();
        assert!(!json.contains("Private note"), "{json}");
        assert!(json.contains(r#""kind":"contradicts""#), "{json}");
    }

    #[test]
    fn a_locked_store_counts_its_sealed_nodes_instead_of_listing_them() {
        let tmp = Tmp::new("locked");
        fixture(&store(&tmp));
        let graph = AmrStore::at(&tmp.0).graph(GRAPH_NODE_CAP).unwrap();
        assert_eq!(graph.nodes.len(), 5);
        assert_eq!(graph.locked, 1);
        assert_eq!(graph.edges.len(), 2);
    }

    #[test]
    fn the_cap_keeps_the_newest_nodes_and_only_their_edges() {
        let tmp = Tmp::new("cap");
        let store = store(&tmp);
        fixture(&store);
        let graph = store.graph(2).unwrap();
        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["boat-refit", "dock-layout"]);
        assert!(graph.truncated);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    fn a_scratch_store_and_an_empty_store() {
        let tmp = Tmp::new("scratch");
        let mut store = store(&tmp);
        store.set_scratch(true);
        assert_eq!(store.graph(GRAPH_NODE_CAP), Err(AmrError::Scratch));
        store.set_scratch(false);
        let graph = store.graph(GRAPH_NODE_CAP).unwrap();
        assert!(graph.nodes.is_empty() && graph.edges.is_empty());
    }

    #[test]
    fn a_long_first_line_is_cut_and_blank_lines_are_skipped() {
        assert_eq!(label_of("\n\n  Hello  \nworld"), "Hello");
        let long = "x".repeat(100);
        let label = label_of(&long);
        assert_eq!(label.chars().count(), LABEL_CHARS + 1);
        assert!(label.ends_with('…'));
        assert_eq!(label_of(""), "");
    }
}

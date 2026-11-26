//! An Obsidian-friendly copy of the plain memory: one page per recallable
//! plain node in `amr/wiki/`, with its frontmatter (type, dates, source,
//! confidence, tags) and `[[id]]` links both ways, plus an `index.md` by
//! type. Point an Obsidian vault at the folder to browse it.
//!
//! The node files stay the truth, and this is a copy that each export brings
//! up to date: changed pages are rewritten, and pages for nodes that were
//! superseded or sealed since are removed. A line the user pauses, forgets
//! or wipes leaves the wiki right then, its page and its index line, so the
//! folder never keeps a copy of a line recall leaves out. A page is ours only
//! when its frontmatter carries `amr_export: 1`, so notes the user adds to
//! the folder are never touched.
//!
//! Sealed (personal or sensitive) nodes never get a page, and a link to one
//! is left out, so even its id stays out of the folder.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::{AmrError, AmrStore, EdgeRel, Node, NodeId};

/// The folder under the store root.
pub const WIKI_DIR: &str = "wiki";
const MARK: &str = "amr_export: 1";
const INDEX_PAGE: &str = "index";
/// Characters of a node's first line the index shows.
const INDEX_LINE: usize = 80;

/// What one export did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WikiReport {
    /// Node pages written or rewritten (unchanged ones are left as they are).
    pub written: usize,
    /// Node pages on disk now.
    pub pages: usize,
    /// Old pages removed: the node was forgotten, superseded or sealed.
    pub removed: usize,
    /// Sealed nodes, opened or locked, that got no page.
    pub private: usize,
}

fn rel_word(rel: EdgeRel) -> &'static str {
    match rel {
        EdgeRel::References => "references",
        EdgeRel::Supersedes => "supersedes",
        EdgeRel::LearnedFromSpan => "learned from",
        EdgeRel::AboutProject => "is about",
        EdgeRel::Contradicts => "contradicts",
    }
}

/// A YAML string: JSON's quoting is valid YAML.
fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

fn page(node: &Node, links: &[(EdgeRel, String)], backlinks: &[(EdgeRel, String)]) -> String {
    let tags: Vec<String> = node.tags.iter().map(|t| quoted(t)).collect();
    let mut out = format!(
        "---\n{MARK}\nid: {}\ntype: {}\ncreated: {}\nupdated: {}\nsource: {}\nconfidence: {}\ntags: [{}]\n---\n\n{}\n",
        node.id,
        node.node_type.as_str(),
        quoted(&node.created),
        quoted(&node.updated),
        quoted(&crate::redact_secrets(&node.source)),
        node.confidence,
        tags.join(", "),
        crate::redact_secrets(node.body.trim()),
    );
    if !links.is_empty() {
        out.push_str("\n## Links\n\n");
        for (rel, to) in links {
            out.push_str(&format!("- {} [[{to}]]\n", rel_word(*rel)));
        }
    }
    if !backlinks.is_empty() {
        out.push_str("\n## Linked from\n\n");
        for (rel, from) in backlinks {
            out.push_str(&format!("- [[{from}]] {} this\n", rel_word(*rel)));
        }
    }
    out
}

fn index(nodes: &[&Node]) -> String {
    let mut by_type: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
    for node in nodes {
        by_type
            .entry(node.node_type.as_str())
            .or_default()
            .push(node);
    }
    let mut out = format!("---\n{MARK}\n---\n\n# Memory\n");
    for (kind, list) in by_type {
        out.push_str(&format!("\n## {kind}\n\n"));
        for node in list {
            let first = node.body.lines().map(str::trim).find(|l| !l.is_empty());
            let mut line: String = crate::redact_secrets(first.unwrap_or_default())
                .chars()
                .take(INDEX_LINE)
                .collect();
            if line.chars().count() == INDEX_LINE {
                line.push('…');
            }
            out.push_str(&format!("- [[{}]] {line}\n", node.id));
        }
    }
    out
}

/// True when `path` is a page an export wrote.
fn ours(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|text| {
        text.strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---"))
            .is_some_and(|(front, _)| front.lines().any(|l| l.trim() == MARK))
    })
}

/// `index` without the lines that link a node `gone` names, and without a
/// type heading left with no line under it: the index an export of the rest
/// would write.
fn without_index_lines(index: &str, gone: &dyn Fn(&str) -> bool) -> String {
    let names = |line: &str| {
        line.strip_prefix("- [[")
            .and_then(|rest| rest.split_once("]]"))
            .is_some_and(|(id, _)| gone(id))
    };
    let mut parts = index.split("\n## ");
    let mut out = parts.next().unwrap_or_default().to_string();
    for part in parts {
        let kept: String = part
            .split_inclusive('\n')
            .filter(|line| !names(line))
            .collect();
        if kept.lines().any(|line| line.starts_with("- [[")) {
            out.push_str("\n## ");
            out.push_str(&kept);
        }
    }
    out
}

fn write_if_changed(path: &Path, text: &str) -> Result<bool, AmrError> {
    if fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(false);
    }
    crate::pulse_writer::write_atomic(path, text).map_err(AmrError::Io)?;
    Ok(true)
}

impl AmrStore {
    /// `amr/wiki/`.
    pub fn wiki_dir(&self) -> PathBuf {
        self.root().join(WIKI_DIR)
    }

    /// Bring the wiki in [`Self::wiki_dir`] up to date. Refuses scratch. It
    /// holds the report lock from its read of the nodes through its writes,
    /// so a pause, forget or Wipe at that moment takes its pages out after
    /// it has written them.
    pub fn export_wiki(&self) -> Result<WikiReport, AmrError> {
        if self.is_scratch() {
            return Err(AmrError::Scratch);
        }
        let _held = self.hold_reports()?;
        let dir = self.wiki_dir();
        fs::create_dir_all(&dir).map_err(|e| AmrError::Io(e.to_string()))?;
        let (nodes, locked) = self.recallable();
        #[cfg(test)]
        super::dream::tests::report_step(self.root(), super::dream::tests::ReportStep::WikiRead);
        let mut report = WikiReport {
            private: locked + nodes.iter().filter(|n| n.sensitivity.sealed()).count(),
            ..WikiReport::default()
        };
        let plain: Vec<&Node> = nodes
            .iter()
            .filter(|n| !n.sensitivity.sealed() && NodeId::parse(&n.id).is_ok())
            .collect();
        let ids: BTreeSet<&str> = plain.iter().map(|n| n.id.as_str()).collect();
        let mut links: BTreeMap<&str, Vec<(EdgeRel, String)>> = BTreeMap::new();
        let mut backlinks: BTreeMap<String, Vec<(EdgeRel, String)>> = BTreeMap::new();
        let edges = self.edges().unwrap_or_default();
        for edge in &edges {
            let (Some(&from), true) = (ids.get(edge.from.as_str()), ids.contains(edge.to.as_str()))
            else {
                continue;
            };
            let to = edge.to.clone();
            let list = links.entry(from).or_default();
            if !list.contains(&(edge.rel, to.clone())) {
                list.push((edge.rel, to.clone()));
            }
            let back = backlinks.entry(to).or_default();
            if !back.contains(&(edge.rel, edge.from.clone())) {
                back.push((edge.rel, edge.from.clone()));
            }
        }
        for node in &plain {
            let text = page(
                node,
                links
                    .get(node.id.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                backlinks.get(&node.id).map(Vec::as_slice).unwrap_or(&[]),
            );
            if write_if_changed(&dir.join(format!("{}.md", node.id)), &text)? {
                report.written += 1;
            }
        }
        report.pages = plain.len();
        write_if_changed(&dir.join(format!("{INDEX_PAGE}.md")), &index(&plain))?;
        let rd = fs::read_dir(&dir).map_err(|e| AmrError::Io(e.to_string()))?;
        for path in rd.flatten().map(|e| e.path()) {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let is_md = path.extension().and_then(|x| x.to_str()) == Some("md");
            if !is_md || stem == INDEX_PAGE || ids.contains(stem) || !ours(&path) {
                continue;
            }
            fs::remove_file(&path).map_err(|e| AmrError::Io(e.to_string()))?;
            report.removed += 1;
        }
        Ok(report)
    }

    /// Take the page of every node `gone` names out of the wiki, and its line
    /// out of the index, now rather than at the next export: what pausing,
    /// forgetting or wiping a line does, each under the report lock, so an
    /// export running at that moment writes its pages first. Only what an
    /// export wrote goes; with no wiki folder it does nothing.
    pub(super) fn drop_wiki_pages(&self, gone: &dyn Fn(&str) -> bool) -> Result<(), AmrError> {
        let io = |e: std::io::Error| AmrError::Io(e.to_string());
        let dir = self.wiki_dir();
        if !dir.is_dir() {
            return Ok(());
        }
        for path in fs::read_dir(&dir).map_err(io)?.flatten().map(|e| e.path()) {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let is_md = path.extension().and_then(|x| x.to_str()) == Some("md");
            if is_md && stem != INDEX_PAGE && gone(stem) && ours(&path) {
                fs::remove_file(&path).map_err(io)?;
            }
        }
        let index = dir.join(format!("{INDEX_PAGE}.md"));
        if ours(&index) {
            let text = fs::read_to_string(&index).map_err(io)?;
            write_if_changed(&index, &without_index_lines(&text, gone))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amr::{NodeDraft, NodeType, Sealer, Sensitivity};

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("grokhub-amr-wiki-{label}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Seals as hex with the label; enough to put a `.sealed` file on disk.
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

    fn draft(id: &str, body: &str) -> NodeDraft {
        NodeDraft {
            id: id.into(),
            node_type: NodeType::Fact,
            created: "2026-10-07T00:00:00Z".into(),
            updated: "2026-10-08T00:00:00Z".into(),
            source: "user".into(),
            confidence: 0.9,
            tags: vec!["dock".into(), "boat yard".into()],
            body: body.into(),
            sensitivity: Sensitivity::Plain,
            consent_ref: String::new(),
        }
    }

    fn store(tmp: &Tmp) -> AmrStore {
        let store = AmrStore::at(&tmp.0).with_sealer(std::sync::Arc::new(HexSealer));
        store.init().unwrap();
        store
    }

    #[test]
    fn plain_nodes_get_pages_with_links_both_ways() {
        let tmp = Tmp::new("pages");
        let store = store(&tmp);
        store
            .remember(&draft("dock-layout", "Dock layout\nBerth 4 is ours."))
            .unwrap();
        store
            .remember(&NodeDraft {
                node_type: NodeType::Project,
                ..draft("boat-refit", "Refit the boat this winter.")
            })
            .unwrap();
        store
            .link("boat-refit", "dock-layout", EdgeRel::References)
            .unwrap();

        let report = store.export_wiki().unwrap();
        assert_eq!(
            report,
            WikiReport {
                written: 2,
                pages: 2,
                removed: 0,
                private: 0
            }
        );
        let dir = store.wiki_dir();
        assert_eq!(
            fs::read_to_string(dir.join("boat-refit.md")).unwrap(),
            "---\namr_export: 1\nid: boat-refit\ntype: project\ncreated: \"2026-10-07T00:00:00Z\"\nupdated: \"2026-10-08T00:00:00Z\"\nsource: \"user\"\nconfidence: 0.9\ntags: [\"dock\", \"boat yard\"]\n---\n\nRefit the boat this winter.\n\n## Links\n\n- references [[dock-layout]]\n"
        );
        assert!(fs::read_to_string(dir.join("dock-layout.md"))
            .unwrap()
            .ends_with(
            "Dock layout\nBerth 4 is ours.\n\n## Linked from\n\n- [[boat-refit]] references this\n"
        ));
        assert_eq!(
            fs::read_to_string(dir.join("index.md")).unwrap(),
            "---\namr_export: 1\n---\n\n# Memory\n\n## fact\n\n- [[dock-layout]] Dock layout\n\n## project\n\n- [[boat-refit]] Refit the boat this winter.\n"
        );
        // Nothing changed, so nothing is rewritten.
        assert_eq!(store.export_wiki().unwrap().written, 0);
    }

    #[test]
    fn sealed_nodes_and_links_to_them_stay_out_of_the_folder() {
        let tmp = Tmp::new("sealed");
        let store = store(&tmp);
        store
            .remember(&draft("dock-layout", "Berth 4 is ours."))
            .unwrap();
        store
            .remember(&NodeDraft {
                sensitivity: Sensitivity::Personal,
                ..draft("home-harbor", "Home harbor is Pier 9.")
            })
            .unwrap();
        store
            .link("dock-layout", "home-harbor", EdgeRel::References)
            .unwrap();
        let report = store.export_wiki().unwrap();
        assert_eq!((report.pages, report.private), (1, 1));
        let dir = store.wiki_dir();
        assert!(!dir.join("home-harbor.md").exists());
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let text = fs::read_to_string(entry.path()).unwrap();
            assert!(!text.contains("home-harbor"), "{text}");
            assert!(!text.contains("Pier 9"), "{text}");
        }
    }

    #[test]
    fn forgotten_and_replaced_nodes_leave_and_the_users_own_notes_stay() {
        let tmp = Tmp::new("sync");
        let store = store(&tmp);
        store
            .remember(&draft("old-card", "graphics card is a GTX 1080"))
            .unwrap();
        store
            .remember(&draft("dock-layout", "Berth 4 is ours."))
            .unwrap();
        store.export_wiki().unwrap();
        let dir = store.wiki_dir();
        fs::write(
            dir.join("my-notes.md"),
            "---\ntitle: mine\n---\n\nKeep me.\n",
        )
        .unwrap();

        store
            .remember(&draft("new-card", "graphics card is an RTX 4080"))
            .unwrap();
        store
            .link("new-card", "old-card", EdgeRel::Supersedes)
            .unwrap();
        store.forget("dock-layout").unwrap();
        // The forget took its page out already; the export takes the
        // replaced one.
        assert!(!dir.join("dock-layout.md").exists());
        let report = store.export_wiki().unwrap();
        assert_eq!((report.written, report.pages, report.removed), (1, 1, 1));
        assert!(dir.join("new-card.md").is_file());
        assert!(!dir.join("old-card.md").exists());
        assert!(!dir.join("dock-layout.md").exists());
        assert_eq!(
            fs::read_to_string(dir.join("my-notes.md")).unwrap(),
            "---\ntitle: mine\n---\n\nKeep me.\n"
        );
    }

    /// Pausing, forgetting or wiping a line takes its page and its index line
    /// out of the wiki right then, not at the next export: the index reads
    /// as an export of what is left would write it. Resuming a line brings
    /// its page back with the next export. Wipe leaves project pages and the
    /// user's own notes.
    #[test]
    fn a_paused_forgotten_or_wiped_line_leaves_the_wiki_right_away() {
        let tmp = Tmp::new("hide");
        let store = store(&tmp);
        store
            .remember(&draft("dock-layout", "Berth 4 is ours."))
            .unwrap();
        store
            .remember(&draft("tide-table", "High tide is at noon."))
            .unwrap();
        store
            .remember(&NodeDraft {
                node_type: NodeType::Project,
                ..draft("boat-refit", "Refit the boat this winter.")
            })
            .unwrap();
        store.export_wiki().unwrap();
        let dir = store.wiki_dir();
        let mine = "---\ntitle: mine\n---\n\nBerth 4 is ours.\n";
        fs::write(dir.join("my-notes.md"), mine).unwrap();
        let pages = || {
            let mut names: Vec<String> = fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        let index = || fs::read_to_string(dir.join("index.md")).unwrap();
        const HEAD: &str = "---\namr_export: 1\n---\n\n# Memory\n";
        const PROJECT: &str = "\n## project\n\n- [[boat-refit]] Refit the boat this winter.\n";

        store.set_paused("dock-layout", true).unwrap();
        assert_eq!(
            pages(),
            ["boat-refit.md", "index.md", "my-notes.md", "tide-table.md"]
        );
        assert_eq!(
            index(),
            format!("{HEAD}\n## fact\n\n- [[tide-table]] High tide is at noon.\n{PROJECT}")
        );

        store.forget("tide-table").unwrap();
        assert_eq!(pages(), ["boat-refit.md", "index.md", "my-notes.md"]);
        assert_eq!(index(), format!("{HEAD}{PROJECT}"));

        store.set_paused("dock-layout", false).unwrap();
        assert_eq!(pages(), ["boat-refit.md", "index.md", "my-notes.md"]);
        let report = store.export_wiki().unwrap();
        assert_eq!((report.written, report.pages, report.removed), (1, 2, 0));
        assert_eq!(
            pages(),
            ["boat-refit.md", "dock-layout.md", "index.md", "my-notes.md"]
        );

        let wiped =
            crate::amr::wipe_user_model(&store, crate::amr::UserForget::from_click()).unwrap();
        assert_eq!(wiped.removed, 2);
        assert_eq!(pages(), ["boat-refit.md", "index.md", "my-notes.md"]);
        assert_eq!(index(), format!("{HEAD}{PROJECT}"));
        assert_eq!(fs::read_to_string(dir.join("my-notes.md")).unwrap(), mine);
    }

    #[test]
    fn a_scratch_store_exports_nothing() {
        let tmp = Tmp::new("scratch");
        let mut store = store(&tmp);
        store.set_scratch(true);
        assert_eq!(store.export_wiki(), Err(AmrError::Scratch));
        assert!(!store.wiki_dir().exists());
    }
}

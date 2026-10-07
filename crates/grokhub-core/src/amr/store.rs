//! Filesystem store. `at` does not create directories. `recall` never does either.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use super::schema::{check_confidence, Edge, EdgeRel, Node, NodeDraft, NodeHit, NodeId};
use super::{AmrError, Sealer};
use crate::redact::redact_secrets;

const README: &str = "\
amr_schema: 1
Local files only. Nothing in amr/ is hub-synced.
nodes/<id>.md is one preference, fact, decision, trail, person, or project. You can cat it or git it.
nodes/<id>.sealed is a personal or sensitive node, sealed at rest. Its key is in your OS keyring.
edges/edges.jsonl stores one JSON edge per line.
dreams/ is reserved for overnight reports. M0 does not write them.
Secrets are redacted on write. forget is not implemented yet.
";

const RECALL_CAP: usize = 20;
const SEALED_EXT: &str = "sealed";

/// Associated data for one sealed node: it can't be renamed to another id.
fn node_aad(id: &str) -> String {
    format!("grokhub:amr-node:v1:{id}")
}

/// `{config}/amr`. Scratch refuses `remember` and `link` before any write.
#[derive(Clone)]
pub struct AmrStore {
    root: PathBuf,
    scratch: bool,
    sealer: Option<Arc<dyn Sealer>>,
}

impl std::fmt::Debug for AmrStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AmrStore")
            .field("root", &self.root)
            .field("scratch", &self.scratch)
            .field("sealer", &self.sealer.is_some())
            .finish()
    }
}

/// `/recall` hits plus how many sealed nodes could not be opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecallReport {
    pub hits: Vec<NodeHit>,
    /// Sealed nodes skipped (locked keyring, missing key, or damaged).
    pub locked: usize,
    /// The sealer's reason for the first skipped node.
    pub why: Option<String>,
}

impl AmrStore {
    /// Point at a root. Does not create it.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            scratch: false,
            sealer: None,
        }
    }

    /// Seal personal and sensitive nodes with `sealer`. Without one they
    /// are refused ([`AmrError::Paused`]) and sealed files read as locked.
    pub fn with_sealer(mut self, sealer: Arc<dyn Sealer>) -> Self {
        self.sealer = Some(sealer);
        self
    }

    /// Incognito. Later writes return [`AmrError::Scratch`] and touch nothing.
    pub fn set_scratch(&mut self, scratch: bool) {
        self.scratch = scratch;
    }

    /// Create `nodes/`, `edges/`, `dreams/`, and `README.md` if they are missing.
    /// An existing README or node file is left as it is.
    pub fn init(&self) -> Result<(), AmrError> {
        for sub in ["nodes", "edges", "dreams"] {
            fs::create_dir_all(self.root.join(sub)).map_err(io_err)?;
        }
        let readme = self.root.join("README.md");
        if !readme.exists() {
            fs::write(&readme, README).map_err(io_err)?;
        }
        Ok(())
    }

    pub fn is_initialized(&self) -> bool {
        self.root.join("nodes").is_dir()
            && self.root.join("edges").is_dir()
            && self.root.join("dreams").is_dir()
            && self.root.join("README.md").is_file()
    }

    /// The `amr_schema:` line in README, if the file and the line are both there.
    pub fn schema_version(&self) -> Option<u32> {
        let text = fs::read_to_string(self.root.join("README.md")).ok()?;
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("amr_schema:") {
                return rest.trim().parse().ok();
            }
        }
        None
    }

    /// Case-insensitive substring over body lines, tags, and id.
    /// Sorted by id. At most 20 hits. A missing or broken store is empty.
    pub fn recall(&self, query: &str) -> Vec<NodeHit> {
        self.recall_report(query).hits
    }

    /// [`Self::recall`] plus the sealed nodes that could not be opened.
    /// Sealed nodes are opened in memory only; nothing is written.
    pub fn recall_report(&self, query: &str) -> RecallReport {
        let mut report = RecallReport::default();
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return report;
        }
        let Ok(read) = fs::read_dir(self.root.join("nodes")) else {
            return report;
        };
        let mut files: Vec<PathBuf> = Vec::new();
        for entry in read.flatten() {
            let path = entry.path();
            if matches!(path.extension().and_then(|ext| ext.to_str()), Some("md" | SEALED_EXT)) {
                files.push(path);
            }
        }
        files.sort();
        let mut hits = Vec::new();
        for path in files {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let text = if path.extension().and_then(|ext| ext.to_str()) == Some(SEALED_EXT) {
                let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                let opened = match &self.sealer {
                    Some(sealer) => sealer.open(&node_aad(id), text.trim()),
                    None => Err("private memory is locked".to_string()),
                };
                match opened {
                    Ok(plain) => plain,
                    Err(why) => {
                        report.locked += 1;
                        report.why.get_or_insert(why);
                        continue;
                    }
                }
            } else {
                text
            };
            let Ok(node) = Node::from_markdown(&text) else {
                continue;
            };
            hits.extend(match_lines(&node, &query));
        }
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        hits.truncate(RECALL_CAP);
        report.hits = hits
            .into_iter()
            .map(|(id, _, line)| NodeHit { id, line })
            .collect();
        report
    }

    /// Write `nodes/<id>.md`. Refuses duplicates, bad ids, and scratch.
    pub fn remember(&self, draft: &NodeDraft) -> Result<NodeId, AmrError> {
        if self.scratch {
            return Err(AmrError::Scratch);
        }
        let id = NodeId::parse(&draft.id)?;
        check_confidence(draft.confidence)?;
        frontmatter_safe("created", &draft.created)?;
        frontmatter_safe("updated", &draft.updated)?;
        frontmatter_safe("source", &draft.source)?;
        for tag in &draft.tags {
            frontmatter_safe("tag", tag)?;
        }
        let source = redact_secrets(&draft.source);
        let tags: Vec<String> = draft.tags.iter().map(|tag| redact_secrets(tag)).collect();
        let body = redact_secrets(&draft.body);
        frontmatter_safe("source", &source)?;
        for tag in &tags {
            frontmatter_safe("tag", tag)?;
        }
        let node = Node {
            id: id.as_str().to_string(),
            node_type: draft.node_type,
            created: draft.created.clone(),
            updated: draft.updated.clone(),
            source,
            confidence: draft.confidence,
            tags,
            body,
        };
        let plain_path = self.node_path(&id)?;
        let sealed_path = plain_path.with_extension(SEALED_EXT);
        if plain_path.exists() || sealed_path.exists() {
            return Err(AmrError::DuplicateId(id.as_str().to_string()));
        }
        // Seal before any file is created: a locked keyring writes nothing.
        let (path, bytes) = if draft.sensitivity.sealed() {
            let sealer = self
                .sealer
                .as_ref()
                .ok_or_else(|| AmrError::Paused("private memory has no key store".into()))?;
            let sealed = sealer
                .seal(&node_aad(id.as_str()), &node.to_markdown())
                .map_err(AmrError::Paused)?;
            (sealed_path, format!("{sealed}\n"))
        } else {
            (plain_path, node.to_markdown())
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_err)?;
        }
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            if draft.sensitivity.sealed() {
                opts.mode(0o600);
            }
        }
        let mut file = match opts.open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(AmrError::DuplicateId(id.as_str().to_string()));
            }
            Err(err) => return Err(io_err(err)),
        };
        file.write_all(bytes.as_bytes()).map_err(io_err)?;
        Ok(id)
    }

    /// Append one edge. Both node files must already exist.
    pub fn link(&self, from: &str, to: &str, rel: EdgeRel) -> Result<(), AmrError> {
        if self.scratch {
            return Err(AmrError::Scratch);
        }
        let from_id = NodeId::parse(from)?;
        let to_id = NodeId::parse(to)?;
        if !self.node_exists(&from_id)? {
            return Err(AmrError::MissingNode(from_id.as_str().to_string()));
        }
        if !self.node_exists(&to_id)? {
            return Err(AmrError::MissingNode(to_id.as_str().to_string()));
        }
        let edge = Edge {
            from: from_id.as_str().to_string(),
            to: to_id.as_str().to_string(),
            rel,
        };
        let line =
            serde_json::to_string(&edge).map_err(|err| AmrError::BadEdge(err.to_string()))?;
        let dir = self.root.join("edges");
        fs::create_dir_all(&dir).map_err(io_err)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("edges.jsonl"))
            .map_err(io_err)?;
        writeln!(file, "{line}").map_err(io_err)?;
        Ok(())
    }

    /// Every edge line, in file order. A missing log is empty.
    pub fn edges(&self) -> Result<Vec<Edge>, AmrError> {
        let path = self.root.join("edges").join("edges.jsonl");
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(io_err(err)),
        };
        let mut out = Vec::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            let edge =
                serde_json::from_str(line).map_err(|err| AmrError::BadEdge(err.to_string()))?;
            out.push(edge);
        }
        Ok(out)
    }

    fn node_exists(&self, id: &NodeId) -> Result<bool, AmrError> {
        let path = self.node_path(id)?;
        Ok(path.is_file() || path.with_extension(SEALED_EXT).is_file())
    }

    fn node_path(&self, id: &NodeId) -> Result<PathBuf, AmrError> {
        let nodes = self.root.join("nodes");
        let name = format!("{}.md", id.as_str());
        let path = nodes.join(&name);
        if path.file_name().and_then(|s| s.to_str()) != Some(name.as_str()) {
            return Err(AmrError::BadId(id.as_str().to_string()));
        }
        match path.parent() {
            Some(parent) if parent == nodes => Ok(path),
            _ => Err(AmrError::BadId(id.as_str().to_string())),
        }
    }
}

fn io_err(err: std::io::Error) -> AmrError {
    AmrError::Io(err.to_string())
}

fn frontmatter_safe(label: &str, value: &str) -> Result<(), AmrError> {
    if value.contains(['\n', '\r']) {
        return Err(AmrError::BadFrontmatter(label.into()));
    }
    Ok(())
}

/// Body lines first, then tags. The id is the line only when neither matched,
/// so a body hit is not repeated as `amr:<id>: <id>`.
fn match_lines(node: &Node, query: &str) -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    let mut seq = 0usize;
    for line in node.body.lines() {
        let trimmed = line.trim();
        if trimmed.to_ascii_lowercase().contains(query) {
            out.push((node.id.clone(), seq, trimmed.to_string()));
            seq += 1;
        }
    }
    for tag in &node.tags {
        if tag.to_ascii_lowercase().contains(query) && !out.iter().any(|(_, _, line)| line == tag) {
            out.push((node.id.clone(), seq, tag.clone()));
            seq += 1;
        }
    }
    if out.is_empty() && node.id.to_ascii_lowercase().contains(query) {
        out.push((node.id.clone(), 0, node.id.clone()));
    }
    out
}

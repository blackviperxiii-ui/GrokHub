//! M3: the nightly dream. A local, deterministic tidy of the store: no model
//! call, no network, no approval (it only marks notes, it never deletes one).
//!
//! 1. Look at live nodes changed in the last [`DREAM_RECENT_DAYS`] days, plus
//!    the low-confidence backlog (confidence below [`DREAM_STALE_BELOW`]).
//! 2. Group near-duplicates of the same type: normalized-token Jaccard at least
//!    [`DREAM_DUP_JACCARD`], or the same non-empty tag set and at least
//!    [`DREAM_TAG_JACCARD`]. The winner is the highest confidence, then the
//!    newest `updated`, then the lowest id.
//! 3. Each loser gets a `supersedes` edge from its winner and a tombstone.
//! 4. A node older than [`DREAM_TTL_DAYS`] with confidence below
//!    [`DREAM_STALE_BELOW`] and no edges gets a tombstone.
//! 5. `dreams/<date>.md` says what happened in plain words.
//!
//! Nothing is removed from disk: the node file count is the same before and
//! after. A node with any edge is left alone (a node whose only edges are its
//! own `supersedes` wins can still absorb new duplicates). Sealed nodes are
//! read only through the sealer, in memory; with no key they are skipped and
//! counted. The report never quotes a sealed node, runs through
//! `redact_secrets`, and quotes at most 80 characters per node. USER.md and
//! SOUL.md are never written: the report may carry a proposed USER.md diff.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::schema::{EdgeRel, Node, NodeId, NodeType};
use super::store::AmrStore;
use super::AmrError;
use crate::redact::redact_secrets;

/// N: a node changed in the last this many days is looked at.
pub const DREAM_RECENT_DAYS: u64 = 14;
/// Near-duplicate: normalized-token Jaccard at or above this.
pub const DREAM_DUP_JACCARD: f64 = 0.85;
/// Near-duplicate with the same non-empty tag set: Jaccard at or above this.
pub const DREAM_TAG_JACCARD: f64 = 0.6;
/// A node not updated for this many days is past its TTL.
pub const DREAM_TTL_DAYS: u64 = 90;
/// Confidence below this is low: the backlog, and stale once past the TTL.
pub const DREAM_STALE_BELOW: f32 = 0.3;
/// A preference at or above this confidence missing from USER.md is proposed.
pub const DREAM_PROPOSE_CONFIDENCE: f32 = 0.9;
/// At most this many proposed USER.md lines.
pub const DREAM_PROPOSE_MAX: usize = 5;
/// At most this many characters quoted from one node.
pub const DREAM_QUOTE_CHARS: usize = 80;

const DAY_MS: u64 = 86_400_000;
const PRIVATE_QUOTE: &str = "(private note, not quoted)";

/// Knobs for one pass. [`Default`] uses the named consts above.
#[derive(Debug, Clone, PartialEq)]
pub struct DreamOpts {
    /// `YYYY-MM-DD` for the report name. `None` uses the UTC date of `now`.
    pub date: Option<String>,
    pub recent_days: u64,
    pub ttl_days: u64,
    pub stale_below: f32,
    pub dup_jaccard: f64,
    pub tag_jaccard: f64,
    /// USER.md as it is now, to propose (never apply) missing preferences.
    pub user_md: Option<String>,
}

impl Default for DreamOpts {
    fn default() -> Self {
        Self {
            date: None,
            recent_days: DREAM_RECENT_DAYS,
            ttl_days: DREAM_TTL_DAYS,
            stale_below: DREAM_STALE_BELOW,
            dup_jaccard: DREAM_DUP_JACCARD,
            tag_jaccard: DREAM_TAG_JACCARD,
            user_md: None,
        }
    }
}

/// One loser folded into its winner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamMerge {
    pub kept: String,
    pub merged: String,
    pub kept_quote: String,
    pub merged_quote: String,
    /// Why they are the same note.
    pub because: String,
    /// Why `kept` won.
    pub kept_why: String,
}

/// One stale node retired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamRetire {
    pub id: String,
    pub quote: String,
    pub because: String,
}

/// What one pass did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamReport {
    pub date: String,
    /// Live nodes that could be read.
    pub looked_at: usize,
    /// Of those, changed in the last N days.
    pub recent: usize,
    /// Of those, below the confidence threshold.
    pub backlog: usize,
    pub merges: Vec<DreamMerge>,
    pub retired: Vec<DreamRetire>,
    /// Nodes left alone because an edge points at or from them.
    pub linked: usize,
    /// Sealed nodes skipped because the key was not available.
    pub sealed_skipped: usize,
    /// Lines proposed for USER.md, not applied.
    pub proposed_user: Vec<String>,
    /// Retention window used (`0` or `u64::MAX` keeps everything).
    pub ttl_days: u64,
    /// Where the report was written.
    pub path: Option<PathBuf>,
}

impl Default for DreamReport {
    fn default() -> Self {
        Self {
            date: String::new(),
            looked_at: 0,
            recent: 0,
            backlog: 0,
            merges: Vec::new(),
            retired: Vec::new(),
            linked: 0,
            sealed_skipped: 0,
            proposed_user: Vec::new(),
            ttl_days: DREAM_TTL_DAYS,
            path: None,
        }
    }
}

impl DreamReport {
    /// Nodes this pass tombstoned.
    pub fn tombstoned(&self) -> usize {
        self.merges.len() + self.retired.len()
    }

    /// One line for chat or a status bar.
    pub fn status_line(&self) -> String {
        format!(
            "Memory dream {}: merged {}, retired {}, {} private skipped.",
            self.date,
            self.merges.len(),
            self.retired.len(),
            self.sealed_skipped
        )
    }

    /// The report a person reads. Secrets are redacted.
    pub fn markdown(&self) -> String {
        let mut out = format!("# Memory dream {}\n\n", self.date);
        out.push_str(
            crate::brand::filled("{product} tidied its memory overnight. Nothing was deleted: merged and retired notes are marked forgotten and stay on disk.\n\n"),
        );
        out.push_str(&format!(
            "- Looked at: {} notes ({} changed in the last {} days, {} with low confidence)\n",
            self.looked_at, self.recent, DREAM_RECENT_DAYS, self.backlog
        ));
        out.push_str(&format!("- Merged: {}\n", self.merges.len()));
        out.push_str(&format!("- Retired: {}\n", self.retired.len()));
        out.push_str(&format!(
            "- Left alone because they are linked: {}\n",
            self.linked
        ));
        out.push_str(&format!(
            "- Private notes skipped (locked): {}\n",
            self.sealed_skipped
        ));
        out.push_str("\n## Merged\n\n");
        if self.merges.is_empty() {
            out.push_str("No duplicates found.\n");
        }
        for m in &self.merges {
            out.push_str(&format!(
                "- Kept `{}` \"{}\", merged `{}` \"{}\" because {}. Kept `{}` because {}.\n",
                m.kept, m.kept_quote, m.merged, m.merged_quote, m.because, m.kept, m.kept_why
            ));
        }
        out.push_str("\n## Retired\n\n");
        if self.retired.is_empty() {
            out.push_str("Nothing was stale.\n");
        }
        for r in &self.retired {
            out.push_str(&format!(
                "- Retired `{}` \"{}\" because {}.\n",
                r.id, r.quote, r.because
            ));
        }
        if self.sealed_skipped > 0 {
            out.push_str(&format!(
                "\n## Skipped private notes\n\n{} private notes stayed locked, so they were not read or changed. They are looked at again on a night the key is available.\n",
                self.sealed_skipped
            ));
        }
        if !self.proposed_user.is_empty() {
            out.push_str(
                "\n## Proposed USER.md changes (not applied)\n\nThese confident preferences are not in USER.md. Nothing was changed; copy them in yourself if they are right.\n\n```diff\n--- USER.md\n+++ USER.md (proposed)\n",
            );
            for line in &self.proposed_user {
                out.push_str(&format!("+- {line}\n"));
            }
            out.push_str("```\n");
        }
        let stale_setting = match self.ttl_days {
            0 | u64::MAX => format!(
                "stale below confidence {} never retired (keeps everything)",
                DREAM_STALE_BELOW
            ),
            days => format!(
                "stale below confidence {} after {} days",
                DREAM_STALE_BELOW, days
            ),
        };
        out.push_str(&format!(
            "\nSettings: duplicates at {}% shared words (or {}% with the same tags), {stale_setting}.\n",
            pct(DREAM_DUP_JACCARD),
            pct(DREAM_TAG_JACCARD),
        ));
        redact_secrets(&out)
    }
}

/// The newest `dreams/YYYY-MM-DD.md` under `amr_root` as `(date, text)`.
/// What one retention pass did (Card 12: on its own schedule, not the dream's).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// Retention in days; 0 keeps everything.
    pub days: u64,
    pub retired: Vec<DreamRetire>,
    /// Live notes left as they are.
    pub kept: usize,
}

impl PruneReport {
    /// One line for chat or a status bar.
    pub fn status_line(&self) -> String {
        if self.days == 0 {
            return format!(
                "Memory retention: keeping everything ({} notes).",
                self.kept
            );
        }
        let notes = if self.retired.len() == 1 {
            "note"
        } else {
            "notes"
        };
        format!(
            "Memory retention: retired {} unsure {notes} not updated in {} days, kept {}.",
            self.retired.len(),
            self.days,
            self.kept
        )
    }
}

/// Import reports are not dreams. Creates nothing.
pub fn latest_dream(amr_root: &Path) -> Option<(String, String)> {
    let read = fs::read_dir(amr_root.join("dreams")).ok()?;
    let mut dates: Vec<String> = read
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let date = name.strip_suffix(".md")?;
            is_date(date).then(|| date.to_string())
        })
        .collect();
    dates.sort();
    let date = dates.pop()?;
    let text = fs::read_to_string(amr_root.join("dreams").join(format!("{date}.md"))).ok()?;
    Some((date, text))
}

/// `dreams/<date>.md` for this store.
pub fn dream_report_path(store: &AmrStore, date: &str) -> PathBuf {
    store.root().join("dreams").join(format!("{date}.md"))
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

fn pct(x: f64) -> u32 {
    (x * 100.0).round() as u32
}

/// Lowercase alphanumeric words.
fn tokens(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

fn tag_set(node: &Node) -> BTreeSet<String> {
    node.tags
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// The first non-empty body line, redacted, at most 80 characters.
fn quote(node: &Node, sealed: bool) -> String {
    if sealed {
        return PRIVATE_QUOTE.to_string();
    }
    let line = node
        .body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let line = redact_secrets(line);
    let (line, _) = crate::pii::redact_pii(&line);
    let line = line
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('"', "'");
    if line.chars().count() <= DREAM_QUOTE_CHARS {
        return line;
    }
    let mut cut: String = line.chars().take(DREAM_QUOTE_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// How a dream report quotes a plain node, as in its proposed USER.md lines.
pub(super) fn report_quote(node: &Node) -> String {
    quote(node, false)
}

/// True when a report line carries text `gone` or `proposed` points at: a
/// Merged line that names a gone id on either side, a Retired line that
/// names one, or a proposed USER.md line (`+- <quote>`) `proposed` holds.
/// A quote never holds `"` (it becomes `'`), so the merged id is the first
/// `` ", merged `id` `` after the kept one.
fn dream_line_holds(
    line: &str,
    gone: &dyn Fn(&str) -> bool,
    proposed: &dyn Fn(&str) -> bool,
) -> bool {
    if let Some(quoted) = line.strip_prefix("+- ") {
        return proposed(quoted);
    }
    if let Some(rest) = line.strip_prefix("- Retired `") {
        return rest.split_once('`').is_some_and(|(id, _)| gone(id));
    }
    let Some((kept, rest)) = line
        .strip_prefix("- Kept `")
        .and_then(|rest| rest.split_once('`'))
    else {
        return false;
    };
    let merged = rest
        .split_once("\", merged `")
        .and_then(|(_, rest)| rest.split_once('`'))
        .map(|(id, _)| id);
    gone(kept) || merged.is_some_and(gone)
}

/// Winner order: higher confidence, newer `updated`, lower id.
fn rank(a: &Node, b: &Node) -> std::cmp::Ordering {
    b.confidence
        .total_cmp(&a.confidence)
        .then_with(|| b.updated.cmp(&a.updated))
        .then_with(|| a.id.cmp(&b.id))
}

fn kept_why(winner: &Node, loser: &Node) -> String {
    if winner.confidence != loser.confidence {
        format!(
            "it is more certain (confidence {} vs {})",
            winner.confidence, loser.confidence
        )
    } else if winner.updated != loser.updated {
        format!(
            "it is newer ({} vs {})",
            day_of(&winner.updated),
            day_of(&loser.updated)
        )
    } else {
        "they tie, and its id sorts first".to_string()
    }
}

fn day_of(stamp: &str) -> &str {
    stamp.get(..10).unwrap_or(stamp)
}

/// `trail_id` ids: `trail-<12 hex>`.
const TRAIL_ID_PREFIX: &str = "trail-";

/// How each live node is linked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Linked {
    None,
    /// Only `supersedes` edges out of it: an earlier dream winner.
    WinnerOnly,
    Other,
}

impl AmrStore {
    /// How every node is linked, from the edge list (trail links don't count).
    fn link_map(&self) -> Result<BTreeMap<String, Linked>, AmrError> {
        let mut links: BTreeMap<String, Linked> = BTreeMap::new();
        for edge in self.edges()? {
            // Spike-3a trail links say where a note came up, not what it
            // means: they keep the trail out of the dream, never the note.
            let trail = |id: &str| id.starts_with(TRAIL_ID_PREFIX);
            match edge.rel {
                EdgeRel::References if trail(&edge.from) => {
                    links.insert(edge.from.clone(), Linked::Other);
                    continue;
                }
                EdgeRel::LearnedFromSpan if trail(&edge.to) => {
                    links.insert(edge.to.clone(), Linked::Other);
                    continue;
                }
                _ => {}
            }
            let from = links.entry(edge.from.clone()).or_insert(Linked::None);
            *from = match (*from, edge.rel) {
                (Linked::None | Linked::WinnerOnly, EdgeRel::Supersedes) => Linked::WinnerOnly,
                _ => Linked::Other,
            };
            links.insert(edge.to.clone(), Linked::Other);
        }
        Ok(links)
    }

    /// Take every line that quotes a node `gone` names, or a proposal
    /// `proposed` holds, out of the nightly reports (`dreams/<date>.md`; the
    /// import and forget reports quote no text). Every other byte of a
    /// report stays, its counts too. A linked report is left alone.
    pub(super) fn drop_dream_lines(
        &self,
        gone: &dyn Fn(&str) -> bool,
        proposed: &dyn Fn(&str) -> bool,
    ) -> Result<(), AmrError> {
        let io = |err: std::io::Error| AmrError::Io(err.to_string());
        let read = match fs::read_dir(self.root().join("dreams")) {
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(io(err)),
        };
        for entry in read.flatten() {
            let path = entry.path();
            let report = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".md"))
                .is_some_and(is_date);
            if !report || !path.symlink_metadata().is_ok_and(|meta| meta.is_file()) {
                continue;
            }
            let text = fs::read_to_string(&path).map_err(io)?;
            let kept: String = text
                .split_inclusive('\n')
                .filter(|line| {
                    !dream_line_holds(line.trim_end_matches(['\n', '\r']), gone, proposed)
                })
                .collect();
            if kept != text {
                fs::write(&path, kept).map_err(io)?;
            }
        }
        Ok(())
    }

    /// Held by a dream pass from its read of the notes through its report,
    /// and by a pause or a Wipe from its change through taking lines out of
    /// the reports, so a report never gets a line paused or wiped while the
    /// dream ran. A lock file (`dream.lock`) covers other processes too.
    pub(super) fn hold_reports(&self) -> Result<fs::File, AmrError> {
        let io = |err: std::io::Error| AmrError::Io(err.to_string());
        fs::create_dir_all(self.root()).map_err(io)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root().join("dream.lock"))
            .map_err(io)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                #[cfg(test)]
                tests::report_step(self.root(), tests::ReportStep::Waiting);
                file.lock().map_err(io)?;
            }
            Err(fs::TryLockError::Error(err)) => return Err(io(err)),
        }
        Ok(file)
    }

    /// The retention pass: tombstone every live note older than `days` with
    /// confidence below `stale_below` and no edges, the same rule as the
    /// dream's step 4, without the rest of the dream. `days == 0` keeps
    /// everything. Indexer scope notes belong to their scope. Never removes a
    /// file; refuses scratch.
    pub fn prune_stale(
        &self,
        now_ms: u64,
        days: u64,
        stale_below: f32,
    ) -> Result<PruneReport, AmrError> {
        if self.is_scratch() {
            return Err(AmrError::Scratch);
        }
        let mut nodes = self.load_live().nodes;
        nodes.retain(|n| !n.source.starts_with(super::SCOPE_SOURCE_PREFIX));
        nodes.retain(|n| !n.tags.iter().any(|t| t == super::user_model::PROPOSED_TAG));
        let mut report = PruneReport {
            days,
            kept: nodes.len(),
            ..PruneReport::default()
        };
        if days == 0 {
            return Ok(report);
        }
        let cut =
            crate::oauth::unix_ms_to_rfc3339(now_ms.saturating_sub(days.saturating_mul(DAY_MS)));
        let date = day_of(&crate::oauth::unix_ms_to_rfc3339(now_ms)).to_string();
        let links = self.link_map()?;
        for n in &nodes {
            if n.confidence >= stale_below || n.updated >= cut || links.contains_key(&n.id) {
                continue;
            }
            let id = NodeId::parse(&n.id)?;
            report.retired.push(DreamRetire {
                id: n.id.clone(),
                quote: quote(n, self.is_sealed(&id)),
                because: format!(
                    "unsure (confidence {}), not updated since {}",
                    n.confidence,
                    day_of(&n.updated)
                ),
            });
        }
        report.retired.sort_by(|a, b| a.id.cmp(&b.id));
        for r in &report.retired {
            let id = NodeId::parse(&r.id)?;
            self.write_tombstone(
                &id,
                now_ms,
                &format!("by: retention {date}\nretired: older than {days} days\n"),
            )?;
        }
        report.kept -= report.retired.len();
        Ok(report)
    }

    /// One dream pass at `now_ms` (see the module docs). Deterministic for the
    /// same store and clock. Refuses scratch before any read. Writes edges,
    /// tombstones, and `dreams/<date>.md`; never removes a file. A second pass
    /// the same day that changes nothing leaves that day's report as it is;
    /// one that does change something appends to it.
    pub fn dream_once(&self, now_ms: u64, opts: &DreamOpts) -> Result<DreamReport, AmrError> {
        if self.is_scratch() {
            return Err(AmrError::Scratch);
        }
        let _held = self.hold_reports()?;
        let stamp = |days: u64| {
            crate::oauth::unix_ms_to_rfc3339(now_ms.saturating_sub(days.saturating_mul(DAY_MS)))
        };
        let recent_cut = stamp(opts.recent_days);
        let ttl_cut = stamp(opts.ttl_days);
        let date = opts
            .date
            .clone()
            .unwrap_or_else(|| day_of(&crate::oauth::unix_ms_to_rfc3339(now_ms)).to_string());

        let loaded = self.load_live();
        let mut nodes = loaded.nodes;
        // Spike-8a: indexer facts belong to their scope ("Forget these"), not the dream.
        // A proposed line stays in the approval queue until a person approves it.
        nodes.retain(|n| !n.source.starts_with(super::SCOPE_SOURCE_PREFIX));
        nodes.retain(|n| !n.tags.iter().any(|t| t == super::user_model::PROPOSED_TAG));
        // A paused line is one the user said not to use: the dream doesn't
        // merge, retire or quote it, so no copy of it lands in a report.
        nodes.retain(|n| !self.is_paused(&n.id));
        #[cfg(test)]
        tests::report_step(self.root(), tests::ReportStep::DreamRead);
        nodes.sort_by(rank);
        let links = self.link_map()?;
        let link_of = |id: &str| links.get(id).copied().unwrap_or(Linked::None);
        let sealed: Vec<bool> = nodes
            .iter()
            .map(|n| NodeId::parse(&n.id).is_ok_and(|id| self.is_sealed(&id)))
            .collect();

        let mut report = DreamReport {
            date: date.clone(),
            looked_at: nodes.len(),
            sealed_skipped: loaded.locked,
            ttl_days: opts.ttl_days,
            ..DreamReport::default()
        };
        let recent: Vec<bool> = nodes
            .iter()
            .map(|n| n.updated >= recent_cut || n.created >= recent_cut)
            .collect();
        let low: Vec<bool> = nodes
            .iter()
            .map(|n| n.confidence < opts.stale_below)
            .collect();
        report.recent = recent.iter().filter(|r| **r).count();
        report.backlog = low.iter().filter(|l| **l).count();
        report.linked = nodes
            .iter()
            .filter(|n| link_of(&n.id) != Linked::None)
            .count();

        // Greedy in winner order: each node joins the first head it duplicates,
        // so every loser is a direct duplicate of the note that kept it.
        let words: Vec<BTreeSet<String>> = nodes.iter().map(|n| tokens(&n.body)).collect();
        let tags: Vec<BTreeSet<String>> = nodes.iter().map(tag_set).collect();
        let mut heads: Vec<usize> = Vec::new();
        let mut losers: BTreeSet<usize> = BTreeSet::new();
        let mut winners: BTreeSet<usize> = BTreeSet::new();
        for i in 0..nodes.len() {
            let link = link_of(&nodes[i].id);
            if link == Linked::Other {
                continue;
            }
            let found = if link == Linked::None {
                heads.iter().copied().find_map(|h| {
                    if nodes[h].node_type != nodes[i].node_type || !(recent[h] || low[h] || recent[i] || low[i]) {
                        return None;
                    }
                    let j = jaccard(&words[h], &words[i]);
                    if j >= opts.dup_jaccard {
                        return Some((h, format!("{}% of their words are the same", (j * 100.0).floor())));
                    }
                    if !tags[h].is_empty() && tags[h] == tags[i] && j >= opts.tag_jaccard {
                        let list = tags[h].iter().cloned().collect::<Vec<_>>().join(", ");
                        return Some((
                            h,
                            format!("they have the same tags ({list}) and {}% of their words are the same", (j * 100.0).floor()),
                        ));
                    }
                    None
                })
            } else {
                None
            };
            match found {
                Some((h, because)) => {
                    losers.insert(i);
                    winners.insert(h);
                    report.merges.push(DreamMerge {
                        kept: nodes[h].id.clone(),
                        merged: nodes[i].id.clone(),
                        kept_quote: quote(&nodes[h], sealed[h]),
                        merged_quote: quote(&nodes[i], sealed[i]),
                        because,
                        kept_why: kept_why(&nodes[h], &nodes[i]),
                    });
                }
                None => heads.push(i),
            }
        }

        let mut stale: Vec<usize> = Vec::new();
        for i in 0..nodes.len() {
            let n = &nodes[i];
            if losers.contains(&i) || winners.contains(&i) || link_of(&n.id) != Linked::None {
                continue;
            }
            if opts.ttl_days != 0 && opts.ttl_days != u64::MAX && low[i] && n.updated < ttl_cut {
                stale.push(i);
                report.retired.push(DreamRetire {
                    id: n.id.clone(),
                    quote: quote(n, sealed[i]),
                    because: format!(
                        "it was unsure (confidence {}, below {}), not updated since {} (over {} days), and nothing links to it",
                        n.confidence,
                        opts.stale_below,
                        day_of(&n.updated),
                        opts.ttl_days
                    ),
                });
            }
        }
        report
            .merges
            .sort_by(|a, b| a.kept.cmp(&b.kept).then(a.merged.cmp(&b.merged)));
        report.retired.sort_by(|a, b| a.id.cmp(&b.id));

        if let Some(user_md) = &opts.user_md {
            let user_lines: Vec<BTreeSet<String>> = user_md.lines().map(tokens).collect();
            let mut proposed: Vec<(&str, String)> = nodes
                .iter()
                .enumerate()
                .filter(|(i, n)| {
                    !sealed[*i]
                        && !losers.contains(i)
                        && !stale.contains(i)
                        && n.node_type == NodeType::Preference
                        && n.confidence >= DREAM_PROPOSE_CONFIDENCE
                })
                .filter(|(i, _)| {
                    !user_lines
                        .iter()
                        .any(|line| jaccard(line, &words[*i]) >= opts.tag_jaccard)
                })
                .map(|(i, n)| (n.id.as_str(), quote(n, sealed[i])))
                .collect();
            proposed.sort();
            proposed.truncate(DREAM_PROPOSE_MAX);
            report.proposed_user = proposed.into_iter().map(|(_, q)| q).collect();
        }

        for m in &report.merges {
            self.link(&m.kept, &m.merged, EdgeRel::Supersedes)?;
            let id = NodeId::parse(&m.merged)?;
            self.write_tombstone(
                &id,
                now_ms,
                &format!("by: dream {date}\nmerged into: {}\n", m.kept),
            )?;
        }
        for r in &report.retired {
            let id = NodeId::parse(&r.id)?;
            self.write_tombstone(&id, now_ms, &format!("by: dream {date}\nretired: stale\n"))?;
        }

        let dir = self.root().join("dreams");
        fs::create_dir_all(&dir).map_err(|err| AmrError::Io(err.to_string()))?;
        let path = dream_report_path(self, &date);
        let existing = path.is_file();
        if !existing || report.tombstoned() > 0 {
            let mut body = report.markdown();
            if existing {
                body.insert(0, '\n');
            }
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|err| AmrError::Io(err.to_string()))?;
            file.write_all(body.as_bytes())
                .map_err(|err| AmrError::Io(err.to_string()))?;
        }
        report.path = Some(path);
        Ok(report)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::amr::{NodeDraft, Sealer, Sensitivity};
    use std::sync::Arc;

    /// 2026-10-07T03:00:00Z
    const NOW: u64 = 1_791_342_000_000;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(label: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("grokhub-dream-{label}-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct FakeSealer {
        locked: std::sync::atomic::AtomicBool,
    }

    impl Sealer for FakeSealer {
        fn seal(&self, aad: &str, plain: &str) -> Result<String, String> {
            Ok(format!("fake:{}:{}", hex::encode(aad), hex::encode(plain)))
        }
        fn open(&self, aad: &str, sealed: &str) -> Result<String, String> {
            if self.locked.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("Private data is locked".into());
            }
            let rest = sealed
                .strip_prefix(&format!("fake:{}:", hex::encode(aad)))
                .ok_or("wrong node")?;
            String::from_utf8(hex::decode(rest).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        }
    }

    fn node(id: &str, body: &str, confidence: f32, updated: &str, tags: &[&str]) -> NodeDraft {
        NodeDraft {
            id: id.into(),
            node_type: NodeType::Fact,
            created: updated.into(),
            updated: updated.into(),
            source: "user".into(),
            confidence,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            body: format!("{body}\n"),
            sensitivity: Sensitivity::Plain,
            consent_ref: String::new(),
        }
    }

    const RECENT: &str = "2026-10-01T12:00:00Z";
    const MIDDLE: &str = "2026-08-01T12:00:00Z";
    const OLD: &str = "2026-03-01T12:00:00Z";

    /// 20 nodes: 3 near-duplicates, 2 stale low-confidence with no edges,
    /// 1 stale low-confidence with an edge, 14 ordinary.
    fn fixture(root: &Path) -> AmrStore {
        let store = AmrStore::at(root);
        store.init().unwrap();
        let dups = [
            node(
                "dup-a",
                "The composer glow stays white at night",
                0.8,
                RECENT,
                &["ui"],
            ),
            node(
                "dup-b",
                "the composer glow stays white at night.",
                0.6,
                RECENT,
                &["ui"],
            ),
            node(
                "dup-c",
                "Composer glow stays white at night",
                0.8,
                "2026-09-30T12:00:00Z",
                &["ui"],
            ),
        ];
        let stale = [
            node(
                "stale-ferry",
                "Maybe the ferry leaves at six",
                0.2,
                OLD,
                &["guess"],
            ),
            node(
                "stale-kite",
                "Possibly likes red kites",
                0.1,
                OLD,
                &["guess"],
            ),
        ];
        let edged = node(
            "stale-linked",
            "Unsure about the lighthouse keeper",
            0.2,
            OLD,
            &["guess"],
        );
        let topics = [
            "Prefers tabs over spaces in Rust files",
            "Lives near the north harbor",
            "Works on GrokHub most evenings",
            "Uses CachyOS on the desktop",
            "Wants short commit titles",
            "Coffee before the standup",
            "The cat is named Juniper",
            "Runs clippy before every push",
            "Keeps the sidebar collapsed",
            "Reads release notes on Fridays",
            "Builds Windows installers with Inno Setup",
            "Dislikes modal dialogs",
            "Tracks findings in the box log",
            "Plays chess on Sunday mornings",
        ];
        for d in dups.iter().chain(&stale).chain(std::iter::once(&edged)) {
            store.remember(d).unwrap();
        }
        for (i, text) in topics.iter().enumerate() {
            let updated = if i % 2 == 0 { RECENT } else { MIDDLE };
            store
                .remember(&node(&format!("n{i:02}"), text, 0.9, updated, &[]))
                .unwrap();
        }
        store
            .link("stale-linked", "n00", EdgeRel::References)
            .unwrap();
        store
    }

    fn tombstones(root: &Path) -> Vec<String> {
        let mut out: Vec<String> = fs::read_dir(root.join("nodes"))
            .unwrap()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                name.strip_suffix(".tombstone").map(str::to_string)
            })
            .collect();
        out.sort();
        out
    }

    /// Spike-8a: two near-identical indexer facts (same scope tag, most words
    /// the same) and a stale low-confidence one are left alone: their scope's
    /// "Forget these" owns them, not the dream.
    #[test]
    fn dream_leaves_indexer_scope_nodes_alone() {
        let tmp = Tmp::new("scope");
        let store = AmrStore::at(&tmp.0);
        store.init().unwrap();
        let mut a = node(
            "scope-aaaaaaaaaaaa",
            "Visited github.com 42 times in Firefox",
            0.9,
            RECENT,
            &["scope:browser_history:firefox"],
        );
        a.source = "scope:browser_history:firefox".into();
        let mut b = node(
            "scope-bbbbbbbbbbbb",
            "Visited github.com 41 times in Firefox",
            0.9,
            RECENT,
            &["scope:browser_history:firefox"],
        );
        b.source = "scope:browser_history:firefox".into();
        let mut c = node(
            "scope-cccccccccccc",
            "System: disk / is 63% used",
            0.1,
            OLD,
            &["scope:system_state"],
        );
        c.source = "scope:system_state".into();
        for d in [&a, &b, &c] {
            store.remember(d).unwrap();
        }
        let report = store.dream_once(NOW, &DreamOpts::default()).unwrap();
        assert_eq!(report.looked_at, 0);
        assert_eq!(report.tombstoned(), 0);
        for id in [
            "scope-aaaaaaaaaaaa",
            "scope-bbbbbbbbbbbb",
            "scope-cccccccccccc",
        ] {
            assert!(!store.is_forgotten(id), "{id}");
        }
        assert_eq!(store.live_from_source("scope:").0.len(), 3);
        assert_eq!(store.live_from_source("scope:system_state").0.len(), 1);
    }

    /// A trail that mentions a note (and a fact learned from that turn) does
    /// not keep the note from being retired; the trail itself stays.
    #[test]
    fn trail_links_do_not_freeze_the_notes_they_mention() {
        let tmp = Tmp::new("trail-links");
        let store = AmrStore::at(&tmp.0);
        store.init().unwrap();
        let trail_id = crate::amr::trail_id("chat-1", 3);
        let mut trail = node(&trail_id, "Turn 3: 2 clicks, 1 type", 1.0, OLD, &["trail"]);
        trail.node_type = NodeType::Trail;
        store.remember(&trail).unwrap();
        store
            .remember(&node(
                "old-guess",
                "Unsure which harbor the keeper means",
                0.2,
                OLD,
                &["guess"],
            ))
            .unwrap();
        store
            .remember(&node(
                "learned",
                "Keeps the harbor map pinned",
                0.2,
                OLD,
                &["map"],
            ))
            .unwrap();
        store
            .link(&trail_id, "old-guess", EdgeRel::References)
            .unwrap();
        store
            .link("learned", &trail_id, EdgeRel::LearnedFromSpan)
            .unwrap();
        store.dream_once(NOW, &DreamOpts::default()).unwrap();
        assert!(
            store.is_forgotten("old-guess"),
            "a stale note a trail mentions is still retired"
        );
        assert!(
            store.is_forgotten("learned"),
            "a stale fact learned in the turn is still retired"
        );
        assert!(!store.is_forgotten(&trail_id), "the trail stays");
    }

    /// Card 12 retention: notes older than N days that are unsure and unlinked
    /// are retired, newer ones kept, with no dream run and no dream report;
    /// 0 keeps everything.
    #[test]
    fn retention_prunes_old_unsure_notes_on_its_own_and_keeps_newer_ones() {
        let tmp = Tmp::new("retention");
        let store = fixture(&tmp.0);
        let middle = node(
            "middle-guess",
            "Perhaps the tram runs late",
            0.2,
            MIDDLE,
            &["guess"],
        );
        store.remember(&middle).unwrap();
        let before_files = store.node_file_count();

        let keep = store.prune_stale(NOW, 0, DREAM_STALE_BELOW).unwrap();
        assert_eq!((keep.retired.len(), keep.kept), (0, 21));
        assert_eq!(
            keep.status_line(),
            "Memory retention: keeping everything (21 notes)."
        );

        // 2026-03-01 is 220 days back, 2026-08-01 is 67: at 90 days only the old ones go.
        let report = store.prune_stale(NOW, 90, DREAM_STALE_BELOW).unwrap();
        let ids: Vec<&str> = report.retired.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["stale-ferry", "stale-kite"]);
        assert_eq!(report.kept, 19);
        assert_eq!(
            report.status_line(),
            "Memory retention: retired 2 unsure notes not updated in 90 days, kept 19."
        );
        assert_eq!(
            report.retired[0].because,
            "unsure (confidence 0.2), not updated since 2026-03-01"
        );
        assert!(store.is_forgotten("stale-ferry") && store.is_forgotten("stale-kite"));
        assert!(!store.is_forgotten("stale-linked"), "a linked note stays");
        assert!(
            !store.is_forgotten("middle-guess"),
            "67 days old is inside 90"
        );
        assert!(
            !store.is_forgotten("dup-b"),
            "duplicates are the dream's job"
        );
        assert_eq!(
            fs::read_dir(tmp.0.join("dreams"))
                .map(|d| d.count())
                .unwrap_or(0),
            0,
            "no dream ran"
        );
        assert_eq!(
            store.node_file_count(),
            before_files,
            "nothing is removed from disk"
        );
        let stone = fs::read_to_string(tmp.0.join("nodes/stale-ferry.tombstone")).unwrap();
        assert!(
            stone.contains("by: retention 2026-10-07\nretired: older than 90 days\n"),
            "{stone}"
        );

        // Shorter retention reaches the 67-day note; a second pass finds nothing new.
        let short = store.prune_stale(NOW, 30, DREAM_STALE_BELOW).unwrap();
        assert_eq!(
            short
                .retired
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            vec!["middle-guess"]
        );
        assert_eq!(
            store
                .prune_stale(NOW, 30, DREAM_STALE_BELOW)
                .unwrap()
                .retired
                .len(),
            0
        );
    }

    /// "Keep everything" reaches the dream as an endless TTL (or 0): nothing is retired.
    #[test]
    fn an_endless_ttl_retires_nothing() {
        let tmp = Tmp::new("endless-ttl");
        let store = AmrStore::at(&tmp.0);
        store.init().unwrap();
        store
            .remember(&node(
                "old-guess",
                "Maybe the ferry leaves at six",
                0.1,
                OLD,
                &[],
            ))
            .unwrap();
        for ttl in [u64::MAX, 0] {
            let report = store
                .dream_once(
                    NOW,
                    &DreamOpts {
                        ttl_days: ttl,
                        ..DreamOpts::default()
                    },
                )
                .unwrap();
            assert_eq!(report.retired.len(), 0);
            assert!(!store.is_forgotten("old-guess"));
            let md = report.markdown();
            assert!(md.contains("never retired (keeps everything)."), "{md}");
        }
    }

    #[test]
    fn dream_report_settings_line_reflects_retention_days() {
        let tmp = Tmp::new("retention-settings");
        let store = AmrStore::at(&tmp.0);
        store.init().unwrap();
        store
            .remember(&node(
                "mid-guess",
                "Maybe the pier closes early",
                0.2,
                MIDDLE,
                &[],
            ))
            .unwrap();

        let thirty = store
            .dream_once(
                NOW,
                &DreamOpts {
                    ttl_days: 30,
                    ..DreamOpts::default()
                },
            )
            .unwrap();
        assert_eq!(thirty.retired.len(), 1);
        let written = fs::read_to_string(thirty.path.as_ref().unwrap()).unwrap();
        assert!(
            written.contains("stale below confidence 0.3 after 30 days."),
            "{written}"
        );
        assert!(!written.contains("after 90 days"), "{written}");

        let keep = DreamReport {
            ttl_days: 0,
            ..DreamReport::default()
        };
        let keep_md = keep.markdown();
        assert!(
            keep_md.contains("stale below confidence 0.3 never retired (keeps everything)."),
            "{keep_md}"
        );

        let default_md = DreamReport::default().markdown();
        assert!(
            default_md.contains("stale below confidence 0.3 after 90 days."),
            "{default_md}"
        );
    }

    #[test]
    fn dream_merges_duplicates_retires_stale_and_loses_nothing() {
        let tmp = Tmp::new("fixture");
        let store = fixture(&tmp.0);
        fs::write(tmp.0.join("USER.md"), "- Prefers tabs\n").unwrap();
        let before_files = store.node_file_count();
        let a_before = fs::read(tmp.0.join("nodes/dup-a.md")).unwrap();
        let linked_before = fs::read(tmp.0.join("nodes/stale-linked.md")).unwrap();
        assert_eq!(before_files, 20);

        let report = store.dream_once(NOW, &DreamOpts::default()).unwrap();

        let supersedes: Vec<(String, String)> = store
            .edges()
            .unwrap()
            .into_iter()
            .filter(|e| e.rel == EdgeRel::Supersedes)
            .map(|e| (e.from, e.to))
            .collect();
        assert_eq!(
            supersedes,
            vec![
                ("dup-a".into(), "dup-b".into()),
                ("dup-a".into(), "dup-c".into())
            ]
        );
        assert_eq!(
            tombstones(&tmp.0),
            vec!["dup-b", "dup-c", "stale-ferry", "stale-kite"]
        );
        assert_eq!(report.tombstoned(), 4);
        assert_eq!(store.node_file_count(), 20, "soft delete only");
        assert_eq!(
            fs::read(tmp.0.join("nodes/dup-a.md")).unwrap(),
            a_before,
            "winner untouched"
        );
        assert_eq!(
            fs::read(tmp.0.join("nodes/stale-linked.md")).unwrap(),
            linked_before
        );
        assert!(!store.is_forgotten("stale-linked") && !store.is_forgotten("dup-a"));
        assert_eq!(
            fs::read_to_string(tmp.0.join("USER.md")).unwrap(),
            "- Prefers tabs\n"
        );
        assert_eq!(
            fs::read_to_string(tmp.0.join("nodes/dup-b.tombstone")).unwrap(),
            "forgotten: 2026-10-07T03:00:00Z\nby: dream 2026-10-07\nmerged into: dup-a\n"
        );

        let text = fs::read_to_string(tmp.0.join("dreams/2026-10-07.md")).unwrap();
        assert_eq!(
            report.path.as_deref(),
            Some(tmp.0.join("dreams/2026-10-07.md").as_path())
        );
        for id in ["dup-a", "dup-b", "dup-c", "stale-ferry", "stale-kite"] {
            assert!(
                text.contains(&format!("`{id}`")),
                "{id} missing from:\n{text}"
            );
        }
        assert!(
            text.contains(
                "- Looked at: 20 notes (10 changed in the last 14 days, 3 with low confidence)\n"
            ),
            "{text}"
        );
        assert!(text.contains("- Merged: 2\n- Retired: 2\n- Left alone because they are linked: 2\n- Private notes skipped (locked): 0\n"), "{text}");
        assert!(text.contains(
            "- Kept `dup-a` \"The composer glow stays white at night\", merged `dup-b` \"the composer glow stays white at night.\" because 100% of their words are the same. Kept `dup-a` because it is more certain (confidence 0.8 vs 0.6).\n"
        ), "{text}");
        assert!(text.contains(
            "- Kept `dup-a` \"The composer glow stays white at night\", merged `dup-c` \"Composer glow stays white at night\" because 85% of their words are the same. Kept `dup-a` because it is newer (2026-10-01 vs 2026-09-30).\n"
        ), "{text}");
        assert!(text.contains(
            "- Retired `stale-kite` \"Possibly likes red kites\" because it was unsure (confidence 0.1, below 0.3), not updated since 2026-03-01 (over 90 days), and nothing links to it.\n"
        ), "{text}");
        assert!(
            !text.contains("stale-linked"),
            "the linked node is not affected:\n{text}"
        );
    }

    #[test]
    fn dream_is_deterministic_and_a_second_pass_changes_nothing() {
        let first = Tmp::new("det-a");
        let second = Tmp::new("det-b");
        let a = fixture(&first.0);
        let b = fixture(&second.0);
        a.dream_once(NOW, &DreamOpts::default()).unwrap();
        b.dream_once(NOW, &DreamOpts::default()).unwrap();
        let report_a = fs::read(first.0.join("dreams/2026-10-07.md")).unwrap();
        assert_eq!(
            report_a,
            fs::read(second.0.join("dreams/2026-10-07.md")).unwrap()
        );

        let edges_before = a.edges().unwrap().len();
        let again = a.dream_once(NOW, &DreamOpts::default()).unwrap();
        assert_eq!(again.tombstoned(), 0);
        assert_eq!(a.edges().unwrap().len(), edges_before);
        assert_eq!(
            fs::read(first.0.join("dreams/2026-10-07.md")).unwrap(),
            report_a,
            "byte-identical"
        );
        assert_eq!(a.node_file_count(), 20);
    }

    #[test]
    fn dream_skips_locked_sealed_nodes_and_writes_no_plaintext() {
        let tmp = Tmp::new("sealed");
        let sealer = Arc::new(FakeSealer {
            locked: false.into(),
        });
        let store = AmrStore::at(&tmp.0).with_sealer(sealer.clone());
        store.init().unwrap();
        let private = |id: &str| NodeDraft {
            sensitivity: Sensitivity::Personal,
            ..node(id, "Home is 12 Pier Road", 0.1, OLD, &[])
        };
        store.remember(&private("home-a")).unwrap();
        store.remember(&private("home-b")).unwrap();
        store
            .remember(&node("plain-old", "Maybe the tide is late", 0.1, OLD, &[]))
            .unwrap();
        sealer
            .locked
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let report = store.dream_once(NOW, &DreamOpts::default()).unwrap();
        assert_eq!(report.sealed_skipped, 2);
        assert_eq!(
            report
                .retired
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            vec!["plain-old"]
        );
        assert_eq!(tombstones(&tmp.0), vec!["plain-old"]);
        let text = fs::read_to_string(tmp.0.join("dreams/2026-10-07.md")).unwrap();
        assert!(
            text.contains("- Private notes skipped (locked): 2\n"),
            "{text}"
        );
        assert!(text.contains("2 private notes stayed locked"), "{text}");
        let mut names: Vec<String> = fs::read_dir(tmp.0.join("nodes"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "home-a.sealed",
                "home-b.sealed",
                "plain-old.md",
                "plain-old.tombstone"
            ]
        );
        for entry in walk(&tmp.0) {
            let body = fs::read_to_string(&entry).unwrap_or_default();
            assert!(
                !body.contains("Pier Road"),
                "plaintext in {}",
                entry.display()
            );
        }

        // With the key, the private duplicates merge, and the report still never quotes them.
        sealer
            .locked
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let open = store
            .dream_once(NOW + DAY_MS, &DreamOpts::default())
            .unwrap();
        assert_eq!(open.sealed_skipped, 0);
        assert_eq!(open.merges.len(), 1);
        assert_eq!(open.merges[0].kept_quote, PRIVATE_QUOTE);
        for entry in walk(&tmp.0) {
            let body = fs::read_to_string(&entry).unwrap_or_default();
            assert!(
                !body.contains("Pier Road"),
                "plaintext in {}",
                entry.display()
            );
        }
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
        out
    }

    #[test]
    fn dream_report_redacts_secrets_quotes_80_chars_and_proposes_user_md() {
        let tmp = Tmp::new("redact");
        let store = AmrStore::at(&tmp.0);
        store.init().unwrap();
        let long = "word ".repeat(40);
        let draft = node("long-one", long.trim(), 0.1, OLD, &[]);
        store.remember(&draft).unwrap();
        // remember redacts too; write a raw node to prove the report's own pass.
        let raw = crate::amr::Node {
            id: "raw-key".into(),
            node_type: NodeType::Fact,
            created: OLD.into(),
            updated: OLD.into(),
            source: "user".into(),
            confidence: 0.1,
            tags: vec![],
            body: "dock key sk-abcdefghijklmnopqrstuv\n".into(),
            consent_ref: String::new(),
            sensitivity: Sensitivity::Plain,
        };
        fs::write(tmp.0.join("nodes/raw-key.md"), raw.to_markdown()).unwrap();
        let pref = NodeDraft {
            node_type: NodeType::Preference,
            ..node("pref-dark", "Prefers the dark theme", 0.95, RECENT, &[])
        };
        store.remember(&pref).unwrap();
        let opts = DreamOpts {
            user_md: Some("- Likes tea\n".into()),
            ..DreamOpts::default()
        };
        let report = store.dream_once(NOW, &opts).unwrap();
        let long_quote = &report
            .retired
            .iter()
            .find(|r| r.id == "long-one")
            .unwrap()
            .quote;
        assert_eq!(long_quote.chars().count(), 80);
        assert!(long_quote.ends_with('…'));
        let text = fs::read_to_string(tmp.0.join("dreams/2026-10-07.md")).unwrap();
        assert!(!text.contains("sk-abcdefghijklmnopqrstuv"), "{text}");
        assert!(text.contains("`raw-key`"), "{text}");
        assert!(text.contains("+- Prefers the dark theme\n"), "{text}");
        assert!(
            text.contains("## Proposed USER.md changes (not applied)"),
            "{text}"
        );
        assert!(!tmp.0.join("USER.md").exists(), "USER.md is never written");
    }

    #[test]
    fn dream_refuses_scratch_and_latest_dream_ignores_imports() {
        let tmp = Tmp::new("scratch");
        let mut store = AmrStore::at(&tmp.0);
        store.set_scratch(true);
        assert_eq!(
            store.dream_once(NOW, &DreamOpts::default()),
            Err(AmrError::Scratch)
        );
        assert!(!tmp.0.exists(), "scratch creates nothing");
        assert_eq!(latest_dream(&tmp.0), None);

        store.set_scratch(false);
        store.init().unwrap();
        fs::write(tmp.0.join("dreams/import-2026-10-09.md"), "# Import\n").unwrap();
        assert_eq!(latest_dream(&tmp.0), None);
        store.dream_once(NOW, &DreamOpts::default()).unwrap();
        store
            .dream_once(NOW + DAY_MS, &DreamOpts::default())
            .unwrap();
        let (date, text) = latest_dream(&tmp.0).unwrap();
        assert_eq!(date, "2026-10-08");
        assert!(text.starts_with("# Memory dream 2026-10-08\n"), "{text}");
    }

    /// The lines of a report that quote a node: Merged, Retired, proposed.
    fn quoting_lines(text: &str) -> Vec<String> {
        text.lines()
            .filter(|l| {
                l.starts_with("- Kept ") || l.starts_with("- Retired ") || l.starts_with("+- ")
            })
            .map(str::to_string)
            .collect()
    }

    const OTHER_MERGE: &str = "- Kept `other-a` \"Ferry leaves the harbor at nine\", merged `other-b` \"ferry leaves the harbor at nine.\" because 100% of their words are the same. Kept `other-a` because it is more certain (confidence 0.8 vs 0.6).";
    const STALE_RETIRE: &str = "- Retired `stale-old` \"Maybe the dock hides on hover\" because it was unsure (confidence 0.2, below 0.3), not updated since 2026-03-01 (over 90 days), and nothing links to it.";

    /// Personal's Pause: "Keep a line but stop using it". The dream doesn't
    /// merge, retire or quote a paused line, and its live twin stays live.
    #[test]
    fn the_dream_leaves_a_paused_line_alone_and_quotes_none_of_it() {
        let tmp = Tmp::new("paused");
        let store = AmrStore::at(&tmp.0);
        store.init().unwrap();
        for draft in [
            node(
                "dup-a",
                "The composer glow stays white at night",
                0.8,
                RECENT,
                &["ui"],
            ),
            node(
                "dup-b",
                "the composer glow stays white at night.",
                0.6,
                RECENT,
                &["ui"],
            ),
            node("stale-old", "Maybe the dock hides on hover", 0.2, OLD, &[]),
        ] {
            store.remember(&draft).unwrap();
        }
        store.set_paused("dup-a", true).unwrap();
        store.set_paused("stale-old", true).unwrap();
        let report = store.dream_once(NOW, &DreamOpts::default()).unwrap();
        assert_eq!(
            (report.looked_at, report.merges.len(), report.retired.len()),
            (1, 0, 0)
        );
        assert_eq!(
            (store.is_forgotten("dup-b"), store.is_forgotten("stale-old")),
            (false, false)
        );
        let text = fs::read_to_string(tmp.0.join("dreams/2026-10-07.md")).unwrap();
        assert_eq!(quoting_lines(&text), Vec::<String>::new());
        assert!(!text.to_lowercase().contains("composer"), "{text}");
    }

    /// A line paused after a night of dreaming leaves that night's report:
    /// the Merged line it kept and its proposed USER.md line go, other lines
    /// stay as they were.
    #[test]
    fn pausing_a_line_takes_its_quotes_out_of_earlier_dream_reports() {
        let tmp = Tmp::new("pause-scrub");
        let store = AmrStore::at(&tmp.0);
        store.init().unwrap();
        let mut pref = node("pref-short", "I like short answers", 0.95, RECENT, &[]);
        pref.node_type = NodeType::Preference;
        for draft in [
            node(
                "dup-a",
                "The composer glow stays white at night",
                0.8,
                RECENT,
                &["ui"],
            ),
            node(
                "dup-b",
                "the composer glow stays white at night.",
                0.6,
                RECENT,
                &["ui"],
            ),
            node(
                "other-a",
                "Ferry leaves the harbor at nine",
                0.8,
                RECENT,
                &[],
            ),
            node(
                "other-b",
                "ferry leaves the harbor at nine.",
                0.6,
                RECENT,
                &[],
            ),
            node("stale-old", "Maybe the dock hides on hover", 0.2, OLD, &[]),
            pref,
        ] {
            store.remember(&draft).unwrap();
        }
        let opts = DreamOpts {
            user_md: Some(String::new()),
            ..DreamOpts::default()
        };
        store.dream_once(NOW, &opts).unwrap();
        let path = tmp.0.join("dreams/2026-10-07.md");
        assert_eq!(
            quoting_lines(&fs::read_to_string(&path).unwrap()),
            [
                "- Kept `dup-a` \"The composer glow stays white at night\", merged `dup-b` \"the composer glow stays white at night.\" because 100% of their words are the same. Kept `dup-a` because it is more certain (confidence 0.8 vs 0.6).",
                OTHER_MERGE,
                STALE_RETIRE,
                "+- I like short answers",
            ]
        );
        store.set_paused("dup-a", true).unwrap();
        store.set_paused("pref-short", true).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(quoting_lines(&text), [OTHER_MERGE, STALE_RETIRE]);
        assert!(text.contains("- Merged: 2\n"), "the counts stay: {text}");
        // Use it again: the report doesn't get the lines back.
        store.set_paused("dup-a", false).unwrap();
        assert_eq!(
            quoting_lines(&fs::read_to_string(&path).unwrap()),
            [OTHER_MERGE, STALE_RETIRE]
        );
    }

    /// Where a report writer is, for a test that lines two up.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in crate::amr) enum ReportStep {
        /// A dream read the notes and holds the report lock.
        DreamRead,
        /// A wiki export read the nodes and holds the report lock.
        WikiRead,
        /// A pause, forget or Wipe found the report lock held and waits for it.
        Waiting,
    }

    type StepHook = Arc<dyn Fn(ReportStep) + Send + Sync>;

    /// One hook per store root, so tests side by side never see each other's.
    static REPORT_HOOKS: std::sync::Mutex<Vec<(PathBuf, StepHook)>> =
        std::sync::Mutex::new(Vec::new());

    pub(in crate::amr) fn report_step(root: &Path, step: ReportStep) {
        let hook = REPORT_HOOKS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(at, _)| at == root)
            .map(|(_, hook)| hook.clone());
        if let Some(hook) = hook {
            hook(step);
        }
    }

    struct HookGuard(PathBuf);

    impl Drop for HookGuard {
        fn drop(&mut self) {
            REPORT_HOOKS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|(at, _)| *at != self.0);
        }
    }

    /// Runs `act` on its own thread while a dream on `root` has read the
    /// notes and not yet written its report, then lets the dream go on.
    /// Returns what `act` did first ("waits" when it found the report lock
    /// held, "done" when it ran to its end), the dream's merge count, and
    /// what `act` returned.
    fn beside_a_dream<T: Send + 'static>(
        root: &Path,
        act: impl FnOnce(&AmrStore) -> T + Send + 'static,
    ) -> (&'static str, Result<usize, AmrError>, T) {
        beside(
            root,
            |store| {
                store
                    .dream_once(NOW, &DreamOpts::default())
                    .map(|done| done.merges.len())
            },
            act,
        )
    }

    /// Runs `act` on its own thread while `parked` on `root` (a dream or a
    /// wiki export) has read what it writes from and not yet written, then
    /// lets `parked` go on. Returns what `act` did first ("waits" when it
    /// found the report lock held, "done" when it ran to its end), what
    /// `parked` returned, and what `act` returned.
    pub(in crate::amr) fn beside<P: Send + 'static, T: Send + 'static>(
        root: &Path,
        parked: impl FnOnce(&AmrStore) -> P + Send + 'static,
        act: impl FnOnce(&AmrStore) -> T + Send + 'static,
    ) -> (&'static str, P, T) {
        use std::sync::{mpsc, Mutex};
        let (event_tx, events) = mpsc::channel::<&'static str>();
        let (go_tx, go) = mpsc::channel::<()>();
        let hook_tx = Mutex::new(event_tx.clone());
        let go = Mutex::new(go);
        let hook: StepHook = Arc::new(move |step| {
            let tx = hook_tx.lock().unwrap().clone();
            match step {
                ReportStep::DreamRead | ReportStep::WikiRead => {
                    tx.send("read").unwrap();
                    go.lock().unwrap().recv().unwrap();
                }
                ReportStep::Waiting => tx.send("waits").unwrap(),
            }
        });
        REPORT_HOOKS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((root.to_path_buf(), hook));
        let _hook = HookGuard(root.to_path_buf());
        let first_run = {
            let root = root.to_path_buf();
            std::thread::spawn(move || parked(&AmrStore::at(&root)))
        };
        assert_eq!(events.recv().unwrap(), "read");
        let other = {
            let root = root.to_path_buf();
            std::thread::spawn(move || {
                let out = act(&AmrStore::at(&root));
                event_tx.send("done").unwrap();
                out
            })
        };
        // With no lock across both, `act` runs to its end right here,
        // before `parked` has written.
        let first = events.recv().unwrap();
        go_tx.send(()).unwrap();
        let parked = first_run.join().unwrap();
        (first, parked, other.join().unwrap())
    }

    /// Two copies of a line the dream merges, quoted in its report.
    fn remember_twins(store: &AmrStore) {
        store.init().unwrap();
        for draft in [
            node(
                "dup-a",
                "The composer glow stays white at night",
                0.8,
                RECENT,
                &["ui"],
            ),
            node(
                "dup-b",
                "the composer glow stays white at night.",
                0.6,
                RECENT,
                &["ui"],
            ),
        ] {
            store.remember(&draft).unwrap();
        }
    }

    /// Every file under `root` holding `needle` (any case), relative.
    fn holding(root: &Path, needle: &str) -> Vec<String> {
        let needle = needle.to_ascii_lowercase().into_bytes();
        let mut found: Vec<String> = walk(root)
            .into_iter()
            .filter(|path| {
                fs::read(path)
                    .unwrap()
                    .to_ascii_lowercase()
                    .windows(needle.len())
                    .any(|w| w == needle.as_slice())
            })
            .map(|path| {
                let rel = path.strip_prefix(root).unwrap();
                let parts: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                parts.join("/")
            })
            .collect();
        found.sort();
        found
    }

    /// Personal's Pause at the moment a dream has read the notes and not
    /// yet written its report: the pause waits for the report, then takes
    /// the paused line's quotes out of it. The counts stay.
    #[test]
    fn a_pause_beside_a_dream_leaves_no_quote_of_the_line_in_its_report() {
        let tmp = Tmp::new("pause-race");
        remember_twins(&AmrStore::at(&tmp.0));
        let (first, merges, paused) =
            beside_a_dream(&tmp.0, |store| store.set_paused("dup-a", true));
        assert_eq!((first, merges, paused), ("waits", Ok(1), Ok(())));
        let text = fs::read_to_string(tmp.0.join("dreams/2026-10-07.md")).unwrap();
        assert_eq!(quoting_lines(&text), Vec::<String>::new());
        assert!(text.contains("- Merged: 1\n"), "{text}");
        assert!(!text.to_lowercase().contains("composer"), "{text}");
    }

    /// Personal's Pause at the moment a wiki export has read the lines and
    /// not yet written its pages: the pause waits for the export, then takes
    /// the page it wrote for the line out again, and its index line.
    #[test]
    fn a_pause_beside_a_wiki_export_leaves_no_page_of_the_line() {
        let tmp = Tmp::new("wiki-race");
        remember_twins(&AmrStore::at(&tmp.0));
        let (first, written, paused) = beside(
            &tmp.0,
            |store| store.export_wiki().map(|done| done.written),
            |store| store.set_paused("dup-a", true),
        );
        assert_eq!((first, written, paused), ("waits", Ok(2), Ok(())));
        let wiki = tmp.0.join("wiki");
        assert!(wiki.join("dup-b.md").is_file());
        assert_eq!(holding(&wiki, "dup-a"), Vec::<String>::new());
    }

    /// Wipe at the same moment: it waits for the dream to finish, then takes
    /// the wiped lines out of its report, so no byte of them is left under
    /// the store. (Run under it, Wipe pulls the notes out from under the
    /// dream, which then fails part way or quotes them after the Wipe.)
    #[test]
    fn a_wipe_beside_a_dream_leaves_no_byte_of_a_wiped_line() {
        let tmp = Tmp::new("wipe-race");
        remember_twins(&AmrStore::at(&tmp.0));
        let (first, merges, wiped) = beside_a_dream(&tmp.0, |store| {
            crate::amr::wipe_user_model(store, crate::amr::UserForget::from_click())
        });
        assert_eq!(
            (first, merges, wiped),
            (
                "waits",
                Ok(1),
                Ok(crate::amr::WipeReport {
                    removed: 2,
                    locked: 0
                })
            )
        );
        assert_eq!(holding(&tmp.0, "composer"), Vec::<String>::new());
    }
}

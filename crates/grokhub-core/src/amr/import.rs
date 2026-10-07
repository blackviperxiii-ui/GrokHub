//! M2: copy what GrokHub already learned into the store, once.
//!
//! LearningState insights become `fact` or `preference` nodes with
//! `source: learning_state`. The durable preference fields of [`ChipMemory`]
//! become `preference` nodes with `source: chips`. Ids are
//! `import-<12 hex>` of the source and key, so a second run finds every id
//! taken and writes nothing. Legacy files are only read.
//!
//! Chip fields kept: `last_slash` (the home slash command they reach for),
//! `last_surface` (the home surface they come back to), and every chip hit
//! picked from the row at least twice and never dismissed (its label only).
//! Left out: hit `value` (it can hold typed prompt text), use, pick, success,
//! failure and dismiss counters, hour histograms, `transitions`,
//! `last_chip_key` (changes on every pick), `total_events`, `updated_at`.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use super::schema::{NodeDraft, NodeType};
use super::store::AmrStore;
use super::write::{hashed_id, sensitivity_for};
use super::AmrError;
use crate::chips::ChipMemory;
use crate::learning::{looks_like_user_pref, LearningInsight};

/// A chip hit needs this many row picks to count as a preference.
const CHIP_PICKS_MIN: u32 = 2;

/// Imported and skipped counts for one source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportTally {
    pub imported: usize,
    /// Why each skipped item was left out, with a count.
    pub skipped: BTreeMap<&'static str, usize>,
}

impl ImportTally {
    pub fn skipped_total(&self) -> usize {
        self.skipped.values().sum()
    }

    fn skip(&mut self, why: &'static str) {
        *self.skipped.entry(why).or_insert(0) += 1;
    }
}

/// One import run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// `YYYY-MM-DD`, for the report file name.
    pub date: String,
    pub learning_state: ImportTally,
    pub chips: ImportTally,
}

impl ImportReport {
    pub fn imported(&self) -> usize {
        self.learning_state.imported + self.chips.imported
    }

    pub fn skipped(&self) -> usize {
        self.learning_state.skipped_total() + self.chips.skipped_total()
    }

    /// The one chat line with the totals.
    pub fn status_line(&self) -> String {
        format!(
            "Memory import: {} imported, {} skipped (learning_state {}, chips {}).",
            self.imported(),
            self.skipped(),
            self.learning_state.imported,
            self.chips.imported
        )
    }

    /// `dreams/import-<date>.md`. Counts and reasons only, never node text.
    pub fn markdown(&self) -> String {
        let mut out = format!(
            "# Import {}\n\n{} imported, {} skipped.\n",
            self.date,
            self.imported(),
            self.skipped()
        );
        for (name, tally) in [
            ("learning_state", &self.learning_state),
            ("chips", &self.chips),
        ] {
            out.push_str(&format!(
                "\n## {name}\n\n{} imported, {} skipped.\n",
                tally.imported,
                tally.skipped_total()
            ));
            for (why, n) in &tally.skipped {
                out.push_str(&format!("- {n} skipped: {why}\n"));
            }
        }
        out.push_str(
            "\nChip fields kept: last_slash, last_surface, chips picked from the row at least twice and never dismissed (label only).\n\
             Chip fields left out: hit values (can hold typed text), counters, hour histograms, transitions, last_chip_key, total_events, timestamps.\n\
             Legacy files were read, not edited.\n",
        );
        out
    }
}

/// `(key, text)` for each durable chip preference, in a stable order.
pub fn durable_chip_prefs(chips: &ChipMemory) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(slash) = chips
        .last_slash
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        out.push((
            "last_slash".to_string(),
            format!("From home they reach for {slash}."),
        ));
    }
    if let Some(surface) = chips
        .last_surface
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        out.push((
            "last_surface".to_string(),
            format!("From home they come back to {surface}."),
        ));
    }
    let mut picked: Vec<_> = chips
        .hits
        .iter()
        .filter(|hit| {
            hit.picks >= CHIP_PICKS_MIN && hit.dismisses == 0 && !hit.label.trim().is_empty()
        })
        .collect();
    picked.sort_by(|a, b| a.key.cmp(&b.key));
    for hit in picked {
        out.push((
            format!("pick:{}", hit.key),
            format!("They pick the \"{}\" chip.", hit.label.trim()),
        ));
    }
    out
}

/// Import insights and chip preferences that are not in the store yet.
/// A personal line with no key is skipped as paused and tried again next run.
pub fn import_legacy(
    store: &AmrStore,
    insights: &[LearningInsight],
    chips: &ChipMemory,
    now_ms: u64,
) -> ImportReport {
    let stamp = crate::oauth::unix_ms_to_rfc3339(now_ms);
    let mut report = ImportReport {
        date: stamp.chars().take(10).collect(),
        ..ImportReport::default()
    };
    for insight in insights {
        let text = insight.text.trim();
        if insight.key.starts_with("habit:") || insight.key.starts_with("skip:") {
            report
                .learning_state
                .skip("chip habit, rebuilt from chips each night");
            continue;
        }
        if text.len() < 8 || text.contains(['\n', '\r']) || !crate::is_plain_text(text) {
            report.learning_state.skip("not a plain one-line note");
            continue;
        }
        let node_type = if insight.key.starts_with("pref") || looks_like_user_pref(text) {
            NodeType::Preference
        } else {
            NodeType::Fact
        };
        let confidence = (0.6 + 0.05 * insight.hits.min(6) as f32).min(0.9);
        let draft = NodeDraft {
            id: hashed_id("import", &["learning_state", &insight.key]),
            node_type,
            created: stamp.clone(),
            updated: stamp.clone(),
            source: "learning_state".into(),
            confidence,
            tags: vec!["imported".into()],
            body: format!("{text}\n"),
            sensitivity: sensitivity_for(text),
        };
        tally_write(&mut report.learning_state, store.remember(&draft));
    }
    for (key, text) in durable_chip_prefs(chips) {
        if !crate::is_plain_text(&text) {
            report.chips.skip("not a plain one-line note");
            continue;
        }
        let draft = NodeDraft {
            id: hashed_id("import", &["chips", &key]),
            node_type: NodeType::Preference,
            created: stamp.clone(),
            updated: stamp.clone(),
            source: "chips".into(),
            confidence: 0.6,
            tags: vec!["imported".into(), "chips".into()],
            body: format!("{text}\n"),
            sensitivity: sensitivity_for(&text),
        };
        tally_write(&mut report.chips, store.remember(&draft));
    }
    report
}

fn tally_write(tally: &mut ImportTally, result: Result<super::NodeId, AmrError>) {
    match result {
        Ok(_) => tally.imported += 1,
        Err(AmrError::DuplicateId(_)) => tally.skip("already imported"),
        Err(AmrError::Paused(_)) => {
            tally.skip("private and the key is locked; tried again next run")
        }
        Err(AmrError::Scratch) => tally.skip("scratch chat"),
        Err(_) => tally.skip("could not be written"),
    }
}

/// Write the report to `dreams/import-<date>.md`, appending when a run on the
/// same day already wrote one. Returns the path.
pub fn write_import_report(store: &AmrStore, report: &ImportReport) -> Result<PathBuf, AmrError> {
    let dir = store.root().join("dreams");
    fs::create_dir_all(&dir).map_err(|err| AmrError::Io(err.to_string()))?;
    let path = dir.join(format!("import-{}.md", report.date));
    let mut body = report.markdown();
    if path.is_file() {
        body.insert(0, '\n');
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| AmrError::Io(err.to_string()))?;
    file.write_all(body.as_bytes())
        .map_err(|err| AmrError::Io(err.to_string()))?;
    Ok(path)
}

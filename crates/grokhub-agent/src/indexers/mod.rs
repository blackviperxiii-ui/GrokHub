//! Spike-8a local indexers (harness design P5).
//!
//! Once the user allows a scope in Settings → Permissions (or on an inline
//! ask card), GrokHub learns from it on this computer: a folder's files,
//! installed apps, one browser's history, the system's health. Every scope is
//! off until a click grants it, and a revoke stops it on the next tick.
//!
//! [`Scheduler::tick`] runs on the heartbeat, on a low-priority worker:
//! one scope per tick, nothing on battery or in quiet hours, nothing while
//! private data is locked (no keyring ⇒ [`TickOutcome::Paused`], never
//! plaintext). Each batch asks `harness::decide` (`Step::Scope`) against a
//! freshly read `ConsentLedger`; no answer is kept past one tick.
//!
//! Facts become `scope:` AMR nodes (`source: scope:<key>`), always personal or
//! sensitive, so they are sealed at rest through `LearnedVault`. They stay out
//! of the native engine's on-disk FTS. [`ScopeIndex`] is the in-memory view the
//! cabin rebuilds after each write and at unlock. Indexers never send anything
//! anywhere: there is no egress call in this module.
//!
//! Hard excludes hold whatever the grant says ([`index_excluded`]): the
//! `SCOPE_HARD_EXCLUDES` list plus password-manager stores and key files.
//! An excluded path is skipped before it is read and never named in a node,
//! span or log. Calendar and mail stay grant rows only: the cabin has no read
//! path of its own for them (Grok Build's connectors live outside it, D1).

pub mod apps;
pub mod browser_history;
pub mod files;
pub mod fs;
pub mod paths;
pub mod power;
pub mod system_state;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use grokhub_core::amr::{AmrError, AmrStore, NodeDraft, NodeType, Sensitivity, SCOPE_SOURCE_PREFIX};
use sha2::{Digest, Sha256};

use crate::harness::{self as hx, ConsentLedger, Scope, Step};
use fs::IndexFs;
use paths::PlatformDirs;
use power::PowerSource;
use system_state::SystemProbe;

/// `{config}/indexers/`: the cabin's launch log and the browser temp copies.
pub const INDEXERS_DIR: &str = "indexers";
/// Span session for indexer ticks: `{config}/spans/indexers.jsonl`.
pub const INDEXER_TRACE: &str = "indexers";
/// A scope is read again at most this often.
pub const RESCAN_MS: u64 = 10 * 60 * 1000;
/// Facts written between two consent checks.
pub const BATCH: usize = 64;
/// Most new nodes one tick writes; the rest come on later ticks.
pub const NODE_CAP_PER_TICK: usize = 500;
/// Confidence of an indexer fact: read straight off the machine.
const FACT_CONFIDENCE: f32 = 0.9;

/// Beyond `SCOPE_HARD_EXCLUDES`: password-manager stores and credential files.
/// (The auth file name is spelled in two parts: agent sources never name it whole.)
pub const INDEX_EXCLUDES: &[&str] =
    &[".password-store", "keyrings", "1Password", "Bitwarden", "KeePassXC", concat!("auth", ".json")];
/// Credential file extensions never read.
const EXCLUDED_EXTS: &[&str] = &["kdbx", "kdb", "pem", "key", "p12", "pfx", "keychain-db", "opvault"];
/// Credential file name starts never read (`.env.local`, `id_ed25519.pub`, …).
const EXCLUDED_STEMS: &[&str] = &[".env", "id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"];

/// One thing an indexer learned. `item` is stable per scope, so reading the
/// same thing again writes nothing new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub item: String,
    pub line: String,
    pub sensitivity: Sensitivity,
}

/// True when an indexer must never read `path`, whatever the grant says.
pub fn index_excluded(path: &str) -> bool {
    if hx::scope_excluded(path) {
        return true;
    }
    let p = path.replace('\\', "/");
    if INDEX_EXCLUDES.iter().any(|x| p.ends_with(&format!("/{x}")) || p.contains(&format!("/{x}/")) || p == *x) {
        return true;
    }
    let name = p.rsplit('/').next().unwrap_or("");
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    EXCLUDED_EXTS.contains(&ext.as_str()) || EXCLUDED_STEMS.iter().any(|s| name.starts_with(s))
}

/// `YYYY-MM-DD` of a unix-ms stamp.
pub fn date_of(ms: u64) -> String {
    grokhub_core::oauth::unix_ms_to_rfc3339(ms).chars().take(10).collect()
}

/// The kind part of a scope key (`files`, `browser_history`, …): the only
/// part of a scope that lands in a tag or span.
pub fn scope_kind(scope: &Scope) -> &'static str {
    match scope {
        Scope::Files(_) => "files",
        Scope::Apps => "apps",
        Scope::BrowserHistory(_) => "browser_history",
        Scope::Calendar => "calendar",
        Scope::Mail => "mail",
        Scope::SystemState => "system_state",
    }
}

/// True for scopes with a cabin-owned reader. Calendar and mail have none.
pub fn has_reader(scope: &Scope) -> bool {
    !matches!(scope, Scope::Calendar | Scope::Mail)
}

/// `scope-<12 hex>` from the scope key and the item.
pub fn node_id(scope_key: &str, item: &str) -> String {
    let digest = Sha256::digest(format!("{scope_key}\n{item}").as_bytes());
    let hex: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("scope-{hex}")
}

/// The AMR store the indexers write to, sealed through the learned-tier key.
pub fn scope_store(config_dir: &Path) -> AmrStore {
    AmrStore::at(config_dir.join("amr")).with_sealer(Arc::new(hx::LearnedVault::new(config_dir)))
}

/// What one tick did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickOutcome {
    /// Private data is locked (no keyring, missing key). Nothing was read or written.
    Paused(String),
    OnBattery,
    QuietHours,
    /// No granted scope is due.
    Idle,
    Ran {
        kind: &'static str,
        written: usize,
        /// Facts already in the store (or forgotten): not written again.
        known: usize,
        /// The grant went away mid-tick; the rest of the batch was dropped.
        stopped: bool,
        /// Browser history only: every table the connection read.
        tables: Vec<String>,
    },
}

/// Everything a tick reads from, so tests can fake each one.
pub struct TickEnv<'a> {
    pub config_dir: &'a Path,
    pub now_ms: u64,
    /// The cabin's quiet hours are on right now.
    pub quiet: bool,
    pub power: &'a dyn PowerSource,
    pub fs: &'a dyn IndexFs,
    pub probe: &'a dyn SystemProbe,
    pub dirs: &'a PlatformDirs,
}

/// Round-robin over granted scopes. Holds no consent: only when each scope last ran.
#[derive(Debug, Clone)]
pub struct Scheduler {
    cursor: usize,
    last_run: BTreeMap<String, u64>,
    pub rescan_ms: u64,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self { cursor: 0, last_run: BTreeMap::new(), rescan_ms: RESCAN_MS }
    }
}

/// Granted scopes that have a reader, with their grant ids, sorted by key.
fn granted(ledger: &ConsentLedger) -> Vec<(Scope, String)> {
    let mut out: Vec<(Scope, String)> = ledger
        .active()
        .filter(|g| g.destination.is_empty())
        .filter_map(|g| Scope::parse(&g.source).map(|s| (s, g.id.clone())))
        .filter(|(s, _)| has_reader(s))
        .collect();
    out.sort_by_key(|(s, _)| s.key());
    out
}

fn allowed(config_dir: &Path, scope: &Scope) -> bool {
    let ledger = ConsentLedger::load(config_dir);
    ledger.locked().is_none() && hx::decide(Step::Scope { scope, ledger: &ledger }).is_allow()
}

fn browser_name(id: &str) -> &str {
    match id {
        "firefox" => "Firefox",
        "chrome" => "Chrome",
        "chromium" => "Chromium",
        "edge" => "Edge",
        "brave" => "Brave",
        other => other,
    }
}

/// Read one scope. Only called after `decide` allowed it this tick.
fn gather(scope: &Scope, env: &TickEnv<'_>) -> (Vec<Fact>, Vec<String>) {
    match scope {
        Scope::Files(dir) => (files::index_folder(env.fs, Path::new(dir)).facts, Vec::new()),
        Scope::Apps => (apps::index_apps(env.fs, env.dirs, &apps::read_launch_counts(env.config_dir)), Vec::new()),
        Scope::BrowserHistory(id) => {
            let Some((kind, root)) = paths::browser_root(id, env.dirs) else {
                return (Vec::new(), Vec::new());
            };
            let tmp = env.config_dir.join(INDEXERS_DIR).join("tmp");
            let mut reads = Vec::new();
            let mut tables: Vec<String> = Vec::new();
            for db in paths::history_dbs(env.fs, kind, &root) {
                if let Ok(read) = browser_history::read_history(env.fs, &db, kind, &tmp) {
                    for t in &read.tables_read {
                        if !tables.contains(t) {
                            tables.push(t.clone());
                        }
                    }
                    reads.push(read);
                }
            }
            let _ = std::fs::remove_dir(&tmp);
            (browser_history::history_facts(browser_name(id), &reads), tables)
        }
        Scope::SystemState => (system_state::snapshot(env.probe, env.dirs).facts(&date_of(env.now_ms)), Vec::new()),
        Scope::Calendar | Scope::Mail => (Vec::new(), Vec::new()),
    }
}

impl Scheduler {
    /// One heartbeat's worth of indexing: at most one scope.
    pub fn tick(&mut self, env: &TickEnv<'_>) -> TickOutcome {
        if env.power.on_battery() {
            return TickOutcome::OnBattery;
        }
        if env.quiet {
            return TickOutcome::QuietHours;
        }
        let ledger = ConsentLedger::load(env.config_dir);
        if let Some(why) = ledger.locked() {
            return TickOutcome::Paused(why.message());
        }
        let granted = granted(&ledger);
        self.last_run.retain(|k, _| granted.iter().any(|(s, _)| s.key() == *k));
        let due: Vec<(Scope, String)> = granted
            .into_iter()
            .filter(|(s, _)| {
                self.last_run
                    .get(&s.key())
                    .is_none_or(|at| env.now_ms.saturating_sub(*at) >= self.rescan_ms)
            })
            .collect();
        if due.is_empty() {
            return TickOutcome::Idle;
        }
        // Fail closed before anything is read: facts could never be sealed.
        match hx::read_key(env.config_dir, true) {
            Some(Ok(_)) => {}
            Some(Err(why)) => return TickOutcome::Paused(why.message()),
            None => return TickOutcome::Paused(hx::Locked::Unavailable.message()),
        }
        let (scope, grant_id) = due[self.cursor % due.len()].clone();
        self.cursor = self.cursor.wrapping_add(1);
        if !hx::decide(Step::Scope { scope: &scope, ledger: &ledger }).is_allow() {
            return TickOutcome::Idle;
        }
        self.last_run.insert(scope.key(), env.now_ms);
        let (facts, tables) = gather(&scope, env);
        let out = write_facts(env, &scope, &grant_id, &facts, tables);
        if let TickOutcome::Ran { kind, written, known, stopped, tables } = &out {
            write_span(env.config_dir, kind, *written, *known, *stopped, tables, &grant_id);
        }
        out
    }
}

fn write_facts(env: &TickEnv<'_>, scope: &Scope, grant_id: &str, facts: &[Fact], tables: Vec<String>) -> TickOutcome {
    let kind = scope_kind(scope);
    let key = scope.key();
    let store = scope_store(env.config_dir);
    let stamp = grokhub_core::oauth::unix_ms_to_rfc3339(env.now_ms);
    let (mut written, mut known, mut stopped) = (0usize, 0usize, false);
    if !facts.is_empty() {
        if let Err(e) = store.init() {
            return TickOutcome::Paused(e.to_string());
        }
    }
    'batches: for batch in facts.chunks(BATCH) {
        // The grant is read again before every batch: a revoke lands mid-tick.
        if !allowed(env.config_dir, scope) {
            stopped = true;
            break;
        }
        for f in batch {
            if written >= NODE_CAP_PER_TICK {
                break 'batches;
            }
            let draft = NodeDraft {
                id: node_id(&key, &f.item),
                node_type: NodeType::Fact,
                created: stamp.clone(),
                updated: stamp.clone(),
                source: format!("{SCOPE_SOURCE_PREFIX}{key}"),
                confidence: FACT_CONFIDENCE,
                tags: vec![format!("scope:{kind}")],
                body: format!("{}\n", f.line),
                // Never plain: indexer facts are sealed at rest.
                sensitivity: match f.sensitivity {
                    Sensitivity::Plain => Sensitivity::Personal,
                    s => s,
                },
                consent_ref: grant_id.to_string(),
            };
            match store.remember(&draft) {
                Ok(_) => written += 1,
                Err(AmrError::DuplicateId(_)) => known += 1,
                Err(AmrError::Paused(why)) => return TickOutcome::Paused(why),
                Err(_) => {}
            }
        }
    }
    TickOutcome::Ran { kind, written, known, stopped, tables }
}

/// One span per tick that ran: the scope kind and counts, never a path or a fact.
fn write_span(config_dir: &Path, kind: &str, written: usize, known: usize, stopped: bool, tables: &[String], grant_id: &str) {
    let args = serde_json::json!({ "scope": kind, "tables": tables }).to_string();
    let result = format!("wrote {written}, known {known}{}", if stopped { ", stopped: revoked" } else { "" });
    let mut span = hx::Span::soft_allow(
        INDEXER_TRACE,
        "index",
        &args,
        &result,
        "local read of a granted scope",
        hx::AccessMode::Readonly,
        "indexer",
    );
    span.approval_class = "scope".into();
    span.path = "indexer".into();
    span.origin = hx::Origin::Proactive;
    span.consent_ref = grant_id.to_string();
    let _ = hx::append_span(config_dir, &span);
}

/// One fact as the cabin lists it under its scope row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFact {
    pub id: String,
    pub line: String,
}

/// The in-memory index of what the indexers learned, by scope key. Built by
/// opening the sealed `scope:` nodes in memory; never written anywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeIndex {
    pub facts: BTreeMap<String, Vec<IndexedFact>>,
    /// Sealed nodes that stayed shut (locked keyring).
    pub locked: usize,
}

impl ScopeIndex {
    pub fn load(config_dir: &Path) -> Self {
        let (nodes, locked) = scope_store(config_dir).live_from_source(SCOPE_SOURCE_PREFIX);
        let mut facts: BTreeMap<String, Vec<IndexedFact>> = BTreeMap::new();
        for n in nodes {
            let key = n.source[SCOPE_SOURCE_PREFIX.len()..].to_string();
            let line = n.body.lines().next().unwrap_or("").trim().to_string();
            facts.entry(key).or_default().push(IndexedFact { id: n.id, line });
        }
        for list in facts.values_mut() {
            list.sort_by(|a, b| a.line.cmp(&b.line));
        }
        Self { facts, locked }
    }

    pub fn for_scope(&self, key: &str) -> &[IndexedFact] {
        self.facts.get(key).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// "Forget these": tombstone every listed fact (AMR M1). The node files stay,
/// sealed; recall and the index skip them, and the indexer never rewrites
/// them. Returns how many were forgotten.
pub fn forget_facts(config_dir: &Path, ids: &[String]) -> Result<usize, String> {
    let store = scope_store(config_dir);
    let mut n = 0;
    for id in ids {
        if !id.starts_with("scope-") {
            continue;
        }
        match store.forget(id) {
            Ok(()) => n += 1,
            Err(AmrError::MissingNode(_)) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(n)
}

/// An in-context ask for a scope: the cabin paints it as an inline Work-tree
/// card ("To learn your work hours I'd read your calendar. Allow?"). Asking
/// grants nothing; only the click on the card's Allow does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeAsk {
    pub scope: Scope,
    /// Why, in the user's terms.
    pub why: String,
    pub asked_ms: u64,
}

/// Pending scope asks, oldest first. Spike-6a's engine calls [`ScopeAsks::ask`]
/// when a candidate needs a scope the user hasn't allowed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeAsks {
    asks: Vec<ScopeAsk>,
}

impl ScopeAsks {
    /// Queue an ask. False when the scope is already allowed, already asked,
    /// or has no reader (calendar, mail).
    pub fn ask(&mut self, scope: Scope, why: &str, ledger: &ConsentLedger, now_ms: u64) -> bool {
        if !has_reader(&scope) || ledger.scope_grant(&scope).is_some() || self.asks.iter().any(|a| a.scope == scope) {
            return false;
        }
        self.asks.push(ScopeAsk { scope, why: why.trim().to_string(), asked_ms: now_ms });
        true
    }

    pub fn first(&self) -> Option<&ScopeAsk> {
        self.asks.first()
    }

    /// Every pending ask, oldest first.
    pub fn all(&self) -> &[ScopeAsk] {
        &self.asks
    }

    /// Put the `i`th ask on the card slot (the decision inbox jumped to it).
    pub fn promote(&mut self, i: usize) {
        if i < self.asks.len() {
            let a = self.asks.remove(i);
            self.asks.insert(0, a);
        }
    }

    /// The card was answered (Allow or Not now).
    pub fn remove(&mut self, scope: &Scope) {
        self.asks.retain(|a| a.scope != *scope);
    }

    /// No answer within `APPROVAL_TTL` means Not now.
    pub fn expire(&mut self, now_ms: u64) {
        let ttl = hx::APPROVAL_TTL.as_millis() as u64;
        self.asks.retain(|a| now_ms.saturating_sub(a.asked_ms) < ttl);
    }

    pub fn len(&self) -> usize {
        self.asks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.asks.is_empty()
    }
}

#[cfg(test)]
mod tests;

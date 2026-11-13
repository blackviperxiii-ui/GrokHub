//! Round 3 item 33, second half: per-workspace tool loading.
//!
//! Each workspace keeps a small record of which built-in tools its chats
//! called. At the start of a chat's session the record decides the up-front
//! list for that session, and the list then stays put for the whole session,
//! so the cached prompt prefix holds:
//!
//! - an eager tool outside the coding core that no session here used in the
//!   last [`WINDOW`] goes to the deferred index (still one `load_tool_schema`
//!   away);
//! - a deferred tool used in at least [`PROMOTE_USES`] of the last
//!   [`WINDOW`] sessions comes up front (at most [`PROMOTE_CAP`]).
//!
//! A workspace with fewer than [`WINDOW`] sessions gets the default list, and
//! so does every [`WINDOW`]th session, which brings the full list back and
//! lets the record see tools the shorter list hid. Dispatch and the
//! permission gate never read any of this.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

const FILE: &str = "tool_use.json";
/// Sessions a workspace needs before its list changes, and the sessions a
/// tool's use is judged over.
pub const WINDOW: u32 = 10;
/// Sessions in the window that used a deferred tool before it comes up front.
pub const PROMOTE_USES: usize = 5;
/// Deferred tools brought up front at once.
pub const PROMOTE_CAP: usize = 3;
/// Workspaces kept in the record; the longest unseen goes first.
const WORKSPACES: usize = 64;
/// Live chats remembered in memory.
const LIVE: usize = 256;
/// Eager tools every workspace keeps up front.
pub const CORE: [&str; 10] = [
    "read_file",
    "list_dir",
    "grep",
    "glob",
    "get_command_or_subagent_output",
    "write",
    "search_replace",
    "run_terminal_command",
    // A lookup always needs these; a workspace that never searched still can.
    "web_search",
    "web_fetch",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Ws {
    /// Sessions started here.
    sessions: u32,
    /// When the last one started (Unix ms), to drop the longest unseen.
    #[serde(default)]
    seen: u64,
    /// Per tool, the sessions that used it, newest last, at most [`WINDOW`].
    #[serde(default)]
    tools: BTreeMap<String, Vec<u32>>,
}

type Book = BTreeMap<String, Ws>;

/// The session's list changes, frozen at its start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Eager tools sent to the deferred index.
    pub demoted: Vec<String>,
    /// Deferred tools sent up front.
    pub promoted: Vec<String>,
}

impl Plan {
    pub fn demotes(&self, name: &str) -> bool {
        self.demoted.iter().any(|n| n == name)
    }

    pub fn promotes(&self, name: &str) -> bool {
        self.promoted.iter().any(|n| n == name)
    }
}

struct Live {
    key: String,
    number: u32,
    plan: Plan,
    noted: BTreeSet<String>,
}

fn live() -> &'static Mutex<BTreeMap<String, Live>> {
    static MAP: OnceLock<Mutex<BTreeMap<String, Live>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// One writer of the record at a time in this process.
fn io_lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

/// The record's key for a workspace: a hash, so the file holds no paths.
pub fn workspace_key(workspace: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in workspace.display().to_string().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn read_book(config_dir: &Path) -> Book {
    std::fs::read_to_string(config_dir.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_book(config_dir: &Path, book: &Book) {
    let Ok(text) = serde_json::to_string(book) else {
        return;
    };
    let _ = std::fs::create_dir_all(config_dir);
    let tmp = config_dir.join(format!("{FILE}.tmp"));
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, config_dir.join(FILE));
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Built-in tool names, each once, with whether it is deferred by default.
fn builtin_names() -> Vec<(&'static str, bool)> {
    let mut seen = BTreeSet::new();
    super::defs::all_defs()
        .filter(|def| seen.insert(def.name))
        .map(|def| (def.name, def.defer))
        .collect()
}

/// The list changes for session `number` of a workspace with record `ws`.
fn plan_for(ws: &Ws, number: u32, builtins: &[(&str, bool)]) -> Plan {
    if number <= WINDOW || number.is_multiple_of(WINDOW) {
        return Plan::default();
    }
    let after = number - 1 - WINDOW;
    let uses = |name: &str| {
        ws.tools
            .get(name)
            .map(|sessions| {
                sessions
                    .iter()
                    .filter(|&&s| s > after && s < number)
                    .count()
            })
            .unwrap_or(0)
    };
    let demoted = builtins
        .iter()
        .filter(|(name, deferred)| !deferred && !CORE.contains(name) && uses(name) == 0)
        .map(|(name, _)| name.to_string())
        .collect();
    let mut hot: Vec<(usize, &str)> = builtins
        .iter()
        .filter(|(_, deferred)| *deferred)
        .map(|(name, _)| (uses(name), *name))
        .filter(|(n, _)| *n >= PROMOTE_USES)
        .collect();
    hot.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    let mut promoted: Vec<String> = hot
        .into_iter()
        .take(PROMOTE_CAP)
        .map(|(_, name)| name.to_string())
        .collect();
    promoted.sort();
    Plan { demoted, promoted }
}

/// Start chat `session` in `workspace`: count the session and freeze its
/// list. A chat already started keeps the list it has.
pub fn start(config_dir: &Path, session: &str, workspace: &Path) -> Plan {
    if session.is_empty() {
        return Plan::default();
    }
    if let Some(live) = live()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
    {
        return live.plan.clone();
    }
    let key = workspace_key(workspace);
    let (number, plan) = {
        let _io = io_lock().lock().unwrap_or_else(|e| e.into_inner());
        let mut book = read_book(config_dir);
        let ws = book.entry(key.clone()).or_default();
        ws.sessions = ws.sessions.saturating_add(1);
        ws.seen = now_ms();
        let number = ws.sessions;
        let plan = plan_for(ws, number, &builtin_names());
        while book.len() > WORKSPACES {
            let Some(oldest) = book
                .iter()
                .min_by_key(|(_, w)| w.seen)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            book.remove(&oldest);
        }
        write_book(config_dir, &book);
        (number, plan)
    };
    let mut map = live().lock().unwrap_or_else(|e| e.into_inner());
    while map.len() >= LIVE {
        let Some(first) = map.keys().next().cloned() else {
            break;
        };
        map.remove(&first);
    }
    map.insert(
        session.to_string(),
        Live {
            key,
            number,
            plan: plan.clone(),
            noted: BTreeSet::new(),
        },
    );
    plan
}

/// The frozen list changes for `session`, if it was started.
pub fn plan(session: &str) -> Plan {
    live()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
        .map(|l| l.plan.clone())
        .unwrap_or_default()
}

/// Chat `session` called built-in tool `name`: note it once per session.
pub fn note_use(config_dir: &Path, session: &str, name: &str) {
    if !builtin_names().iter().any(|(n, _)| *n == name) {
        return;
    }
    let (key, number) = {
        let mut map = live().lock().unwrap_or_else(|e| e.into_inner());
        let Some(live) = map.get_mut(session) else {
            return;
        };
        if !live.noted.insert(name.to_string()) {
            return;
        }
        (live.key.clone(), live.number)
    };
    let _io = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut book = read_book(config_dir);
    let ws = book.entry(key).or_default();
    let sessions = ws.tools.entry(name.to_string()).or_default();
    if sessions.last() != Some(&number) {
        sessions.push(number);
    }
    let excess = sessions.len().saturating_sub(WINDOW as usize);
    sessions.drain(..excess);
    write_book(config_dir, &book);
}

/// Forget the live chat (tests).
#[cfg(test)]
pub fn forget(session: &str) {
    live()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(session);
}

#[cfg(test)]
#[path = "workspace_tools_tests.rs"]
mod tests;

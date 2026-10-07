//! EgressGuard (Spike-4a): cabin-owned outbound calls ask [`decide`] before
//! they leave, and what left is logged to `{config_dir}/egress.jsonl`.
//!
//! - xAI model hosts (`DEFAULT_CONNECTOR_HOSTS`: grok.com, x.ai, api.x.ai)
//!   are allowed by default for chat and personal data, as before this guard.
//! - A call with no user data (a public read) is allowed.
//! - Loopback is local, not egress, and is not logged.
//! - Chat, personal, or sensitive data to any other destination needs an
//!   active destination grant in the [`ConsentLedger`]; without one it is hard
//!   class `send` (a hard card: no Always, Enter does not approve, Esc / TTL deny).
//!
//! D1: Grok Build's own traffic (its model calls, its connectors, its web
//! tools) is outside the cabin. This guard never sees it and does not claim to.
//! The log holds no content: destination host, data classes, ids, counts.
//!
//! Spike-4b: each log line is sealed at rest (`at_rest`). With the keyring
//! locked no line is written (never as plain text); model-host calls still go
//! as they did before the guard, and `/privacy` says the log is locked. A
//! send that rests on a grant or a one-time approval needs its line written.
//!
//! [`decide`]: crate::harness::decide

use std::cell::{Cell, RefCell};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::harness::approval::{decide, GateOutcome, Step};
use crate::harness::at_rest::{self, Locked, AAD_EGRESS};
use crate::harness::consent::ConsentLedger;
use crate::harness::hard::HardClass;
use crate::harness::park::{post_park, wait_park, ParkRequest};
use crate::harness::span::{append_span, read_turn_context, Origin, Span};

/// The paired computers reached through the LAN hub (`/sync`).
pub const HUB_DEST: &str = "hub";
/// What `/sync` publishes: threads plus SOUL / USER / MEMORY.
pub const HUB_SYNC_DATA: &[DataClass] = &[DataClass::Chat, DataClass::Personal];
/// File name under the cabin config dir.
pub const EGRESS_FILE: &str = "egress.jsonl";
/// Past this size the log rolls to `egress.1.jsonl` (one old file kept).
const EGRESS_ROLL: u64 = 1024 * 1024;
const EGRESS_ROLLED: &str = "egress.1.jsonl";
/// How much of the log tail `/privacy` reads.
const EGRESS_TAIL: u64 = 256 * 1024;

/// What kind of user data a call carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    /// Chat text: what the user typed and what the model wrote.
    Chat,
    /// Memory and user-model data (SOUL / USER / MEMORY, learned facts).
    Personal,
    /// Health, finance, addresses, contacts, browser history. Never allowed by default.
    Sensitive,
}

impl DataClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Personal => "personal",
            Self::Sensitive => "sensitive",
        }
    }

    /// The words a user reads (cards, `/privacy`). The ids stay in logs and files.
    pub fn label(self) -> &'static str {
        match self {
            Self::Chat => "chats",
            Self::Personal => "memory",
            Self::Sensitive => "sensitive data",
        }
    }
}

/// Why an egress was allowed, or that it wasn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressBasis {
    Local,
    Public,
    ModelHost,
    Grant,
    NotGranted,
}

impl EgressBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Public => "public",
            Self::ModelHost => "model_host",
            Self::Grant => "grant",
            Self::NotGranted => "not_granted",
        }
    }
}

/// The destination key for a URL or a named destination: the host only
/// (lowercase, no user info, port, path, or query), or `hub`.
pub fn egress_dest(target: &str) -> String {
    let t = target.trim();
    let rest = t
        .strip_prefix("https://")
        .or_else(|| t.strip_prefix("http://"))
        .or_else(|| t.strip_prefix("wss://"))
        .or_else(|| t.strip_prefix("ws://"));
    let Some(rest) = rest else {
        let key: String = t.to_ascii_lowercase().chars().take(80).collect();
        return grokhub_core::redact_secrets(&key);
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(v6) = authority.strip_prefix('[') {
        format!("[{}]", v6.split(']').next().unwrap_or(""))
    } else {
        authority.split(':').next().unwrap_or("").to_string()
    };
    grokhub_core::redact_secrets(&host.to_ascii_lowercase())
}

pub fn is_local_dest(dest: &str) -> bool {
    matches!(dest, "127.0.0.1" | "localhost" | "[::1]")
}

/// grok.com, x.ai, api.x.ai, and their subdomains.
pub fn is_model_host(dest: &str) -> bool {
    grokhub_core::DEFAULT_CONNECTOR_HOSTS
        .iter()
        .any(|h| dest == *h || dest.ends_with(&format!(".{h}")))
}

/// The pure egress verdict plus the grant that allowed it.
pub(crate) fn check(dest: &str, data: &[DataClass], ledger: &ConsentLedger) -> (GateOutcome, EgressBasis, String) {
    if is_local_dest(dest) {
        return (GateOutcome::Allow, EgressBasis::Local, String::new());
    }
    if data.is_empty() {
        return (GateOutcome::Allow, EgressBasis::Public, String::new());
    }
    if is_model_host(dest) && !data.contains(&DataClass::Sensitive) {
        return (GateOutcome::Allow, EgressBasis::ModelHost, String::new());
    }
    if let Some(g) = ledger.destination_grant(dest, data) {
        return (GateOutcome::Allow, EgressBasis::Grant, g.id.clone());
    }
    let classes: Vec<&str> = data.iter().map(|c| c.as_str()).collect();
    let class = HardClass::Send;
    (
        GateOutcome::Park {
            reason: format!(
                "hard-class {}: {} to {dest} ({}) with no grant — Always cannot skip",
                class.as_str(),
                class.label(),
                classes.join(", ")
            ),
            hard: Some(class),
            needs_jeremy: true,
        },
        EgressBasis::NotGranted,
        String::new(),
    )
}

/// One line of `egress.jsonl`: what left, never the content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EgressLine {
    pub ts_ms: u64,
    pub dest: String,
    pub data_classes: Vec<DataClass>,
    #[serde(default)]
    pub node_ids: Vec<String>,
    #[serde(default)]
    pub redactions: u32,
    /// The consent grant id, `approved-once` for a hard-card click, else empty.
    #[serde(default)]
    pub grant_id: String,
    /// `{session}:{ts_ms}` of the span for this step, empty outside a harness turn.
    #[serde(default)]
    pub span_id: String,
    /// `model_host`, `public`, `grant`, or `approved_once`.
    #[serde(default)]
    pub basis: String,
    #[serde(default)]
    pub origin: Origin,
}

/// One outbound call a caller wants to make.
#[derive(Debug, Clone, Copy)]
pub struct EgressReq<'a> {
    /// A URL or a named destination (`hub`).
    pub target: &'a str,
    pub data: &'a [DataClass],
    pub node_ids: &'a [String],
    pub redactions: u32,
    pub span_id: &'a str,
    pub origin: Origin,
}

impl<'a> EgressReq<'a> {
    /// The origin is this thread's [`OriginScope`] (`user` outside one).
    pub fn new(target: &'a str, data: &'a [DataClass]) -> Self {
        Self {
            target,
            data,
            node_ids: &[],
            redactions: 0,
            span_id: "",
            origin: current_origin(),
        }
    }
}

thread_local! {
    static ORIGIN: Cell<Origin> = const { Cell::new(Origin::User) };
    static RECALL: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Who started the work on this thread (Spike-4c): a heartbeat act, a
/// scheduled job, or the user. Egress lines and spans written on the thread
/// carry it. Dropping the scope restores the previous origin.
pub struct OriginScope {
    prev: Origin,
}

impl OriginScope {
    pub fn enter(origin: Origin) -> Self {
        Self { prev: ORIGIN.with(|o| o.replace(origin)) }
    }
}

impl Drop for OriginScope {
    fn drop(&mut self) {
        ORIGIN.with(|o| o.set(self.prev));
    }
}

pub fn current_origin() -> Origin {
    ORIGIN.with(Cell::get)
}

/// Shortest recall-pack line that marks a call as carrying memory. Shorter
/// lines are too common to say anything.
const RECALL_LINE_MIN: usize = 16;

/// The recall-pack lines the model saw this turn, so a call that carries one
/// of them is classed personal. Dropping the scope clears them.
pub struct RecallScope {
    prev: Vec<String>,
}

impl RecallScope {
    pub fn enter(lines: Vec<String>) -> Self {
        let lines = lines
            .into_iter()
            .map(|l| l.trim().to_lowercase())
            .filter(|l| l.chars().count() >= RECALL_LINE_MIN)
            .collect();
        Self { prev: RECALL.with(|r| r.replace(lines)) }
    }
}

impl Drop for RecallScope {
    fn drop(&mut self) {
        let prev = std::mem::take(&mut self.prev);
        RECALL.with(|r| *r.borrow_mut() = prev);
    }
}

/// `%41` and `+` decoded, for a URL that carries a recall line in its query.
fn percent_decoded(text: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let pair = bytes.get(i + 1).copied().and_then(hex).zip(bytes.get(i + 2).copied().and_then(hex));
        match (bytes[i], pair) {
            (b'%', Some((hi, lo))) => {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
            (b'+', _) => out.push(b' '),
            (b, _) => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Honest classes for text the model wrote into a call (a URL it picked, tool
/// args): chat, plus personal when it carries a line of this turn's recall pack.
pub fn model_text_classes(text: &str) -> &'static [DataClass] {
    const CHAT: &[DataClass] = &[DataClass::Chat];
    const CHAT_PERSONAL: &[DataClass] = &[DataClass::Chat, DataClass::Personal];
    let carries = RECALL.with(|r| {
        let lines = r.borrow();
        if lines.is_empty() {
            return false;
        }
        let raw = text.to_lowercase();
        let decoded = percent_decoded(&raw);
        lines.iter().any(|l| raw.contains(l.as_str()) || decoded.contains(l.as_str()))
    });
    if carries {
        CHAT_PERSONAL
    } else {
        CHAT
    }
}

/// The guard every cabin-owned outbound call runs: [`decide`] on the current
/// ledger, then an `egress.jsonl` line when the call may leave. A `Park` is
/// hard class `send`: the caller shows a hard card or refuses, and sends nothing.
///
/// [`decide`]: crate::harness::decide
pub fn guard_egress(config_dir: &Path, req: &EgressReq<'_>) -> GateOutcome {
    let ledger = ConsentLedger::load(config_dir);
    let dest = egress_dest(req.target);
    let outcome = decide(Step::Egress { dest: &dest, data: req.data, ledger: &ledger });
    if outcome.is_allow() {
        let (_, basis, grant_id) = check(&dest, req.data, &ledger);
        if basis != EgressBasis::Local {
            let logged = append_egress(config_dir, &line_for(req, &dest, basis.as_str(), &grant_id));
            // A grant send that can't be logged doesn't go (fail closed).
            // Model-host and public calls go as before the guard.
            if let (Err(why), EgressBasis::Grant) = (logged, basis) {
                return GateOutcome::Refuse { reason: format!("not sent: {why}") };
            }
        }
    }
    outcome
}

/// Log a send the user approved once on a hard card (no standing grant).
pub fn record_approved_once(config_dir: &Path, req: &EgressReq<'_>) -> Result<(), String> {
    let dest = egress_dest(req.target);
    append_egress(config_dir, &line_for(req, &dest, "approved_once", "approved-once"))
}

/// The guard for a call with no one to ask (a background read): it goes only
/// on Allow, else nothing is sent and the reason comes back.
pub fn guard_quiet(config_dir: &Path, req: &EgressReq<'_>) -> Result<(), String> {
    match guard_egress(config_dir, req) {
        GateOutcome::Allow => Ok(()),
        GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } => Err(reason),
    }
}

/// The guard for a call that can wait on the user (Spike-4c, path E): on a
/// hard Send it posts a park file the cabin shows as a hard card and waits.
/// Approve logs one `approved_once` line and returns Ok; Deny, Esc, halt, or
/// [`APPROVAL_TTL`] return Err and nothing is sent. The card text is the tool,
/// host, and classes only.
///
/// [`APPROVAL_TTL`]: crate::harness::APPROVAL_TTL
pub fn guard_or_park(
    config_dir: &Path,
    req: &EgressReq<'_>,
    tool: &str,
    halted: &mut dyn FnMut() -> bool,
) -> Result<(), String> {
    guard_or_park_within(config_dir, req, tool, crate::harness::APPROVAL_TTL, halted)
}

pub(crate) fn guard_or_park_within(
    config_dir: &Path,
    req: &EgressReq<'_>,
    tool: &str,
    ttl: Duration,
    halted: &mut dyn FnMut() -> bool,
) -> Result<(), String> {
    let (reason, class) = match guard_egress(config_dir, req) {
        GateOutcome::Allow => return Ok(()),
        GateOutcome::Refuse { reason } => return Err(reason),
        GateOutcome::Park { reason, hard, .. } => (reason, hard.unwrap_or(HardClass::Send)),
    };
    static N: AtomicU64 = AtomicU64::new(0);
    let id = format!("egress-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
    let dest = egress_dest(req.target);
    let classes: Vec<&str> = req.data.iter().map(|c| c.label()).collect();
    let action = format!("{tool} → {dest} ({})", classes.join(", "));
    let park = ParkRequest {
        id: id.clone(),
        path: "E".into(),
        tool: tool.into(),
        action: action.clone(),
        class: class.as_str().into(),
        ts_ms: grokhub_core::now_ms(),
    };
    post_park(config_dir, &park).map_err(|why| format!("not sent: {why}"))?;
    let ctx = read_turn_context(config_dir);
    let trace = if ctx.chat_id.is_empty() { "egress" } else { ctx.chat_id.as_str() };
    let span = Span::hard_park(trace, tool, &action, class)
        .on_path("E")
        .in_turn(&ctx.chat_id, ctx.turn)
        .from_origin(req.origin);
    let _ = append_span(config_dir, &span);
    if !wait_park(config_dir, &id, ttl, Duration::from_millis(200), halted) {
        return Err(format!("{reason}. Denied: nothing was sent."));
    }
    record_approved_once(config_dir, req).map_err(|why| format!("not sent: {why}"))
}

fn line_for(req: &EgressReq<'_>, dest: &str, basis: &str, grant_id: &str) -> EgressLine {
    let mut data = req.data.to_vec();
    data.sort();
    data.dedup();
    EgressLine {
        ts_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        dest: dest.to_string(),
        data_classes: data,
        node_ids: req.node_ids.iter().map(|n| grokhub_core::redact_secrets(n)).collect(),
        redactions: req.redactions,
        grant_id: grant_id.to_string(),
        span_id: grokhub_core::redact_secrets(req.span_id),
        basis: basis.to_string(),
        origin: req.origin,
    }
}

pub fn egress_path(config_dir: &Path) -> PathBuf {
    config_dir.join(EGRESS_FILE)
}

/// Append one sealed line (secrets redacted again on the way out). Rolls the
/// file past [`EGRESS_ROLL`]. Locked ⇒ nothing is written.
pub fn append_egress(config_dir: &Path, line: &EgressLine) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let path = egress_path(config_dir);
    let rolled = config_dir.join(EGRESS_ROLLED);
    if fs::metadata(&path).map(|m| m.len() > EGRESS_ROLL).unwrap_or(false) {
        let _ = fs::rename(&path, &rolled);
    }
    let text = serde_json::to_string(line).map_err(|e| e.to_string())?;
    let text = grokhub_core::redact_secrets(&text);
    at_rest::append_sealed_line(config_dir, &path, AAD_EGRESS, &text).map_err(|why| why.message())?;
    // A pre-4b roll can still be plain text: seal it too (cheap once sealed).
    if let Ok(key) = at_rest::write_key(config_dir) {
        let _ = at_rest::migrate_file(&key, &rolled, AAD_EGRESS);
    }
    Ok(())
}

/// The egress tail as `/privacy` reads it: lines plus why some didn't open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EgressRead {
    pub lines: Vec<EgressLine>,
    pub locked: Option<Locked>,
    pub unreadable: usize,
}

/// The last lines of the log (up to [`EGRESS_TAIL`] bytes), oldest first.
pub fn read_egress(config_dir: &Path) -> Vec<EgressLine> {
    read_egress_report(config_dir).lines
}

/// [`read_egress`] plus the lock state. May wait on the keyring once.
pub fn read_egress_report(config_dir: &Path) -> EgressRead {
    let Ok(mut f) = fs::File::open(egress_path(config_dir)) else {
        return EgressRead::default();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(EGRESS_TAIL);
    if f.seek(SeekFrom::Start(from)).is_err() {
        return EgressRead::default();
    }
    let mut buf = Vec::new();
    if f.take(EGRESS_TAIL).read_to_end(&mut buf).is_err() {
        return EgressRead::default();
    }
    let text = String::from_utf8_lossy(&buf);
    let mut rows = text.lines();
    if from > 0 {
        rows.next();
    }
    let tail: Vec<&str> = rows.collect();
    let read = at_rest::read_sealed_jsonl(config_dir, &tail.join("\n"), AAD_EGRESS, true);
    EgressRead {
        lines: read
            .lines
            .iter()
            .filter_map(|l| serde_json::from_str::<EgressLine>(l.trim()).ok())
            .collect(),
        locked: read.locked,
        unreadable: read.unreadable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::consent::{grant_destination, revoke_grant, UserClick};

    struct DirGuard(PathBuf);
    impl Drop for DirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn dir(label: &str) -> (PathBuf, DirGuard) {
        let p = crate::harness::test_dir(&format!("egress-{label}"));
        (p.clone(), DirGuard(p))
    }

    fn send_park(dest: &str, classes: &str) -> GateOutcome {
        GateOutcome::Park {
            reason: format!("hard-class send: Send to {dest} ({classes}) with no grant — Always cannot skip"),
            hard: Some(HardClass::Send),
            needs_jeremy: true,
        }
    }

    #[test]
    fn dest_is_the_host_only() {
        assert_eq!(egress_dest("https://api.x.ai/v1/responses?key=1"), "api.x.ai");
        assert_eq!(egress_dest("https://me:pw@Example.ORG:8443/a"), "example.org");
        assert_eq!(egress_dest("http://127.0.0.1:4711/v1/responses"), "127.0.0.1");
        assert_eq!(egress_dest("http://[::1]:9/x"), "[::1]");
        assert_eq!(egress_dest("hub"), "hub");
        assert!(is_model_host("api.x.ai") && is_model_host("grok.com") && is_model_host("x.ai"));
        assert!(!is_model_host("evilx.ai") && !is_model_host("x.ai.example.org"));
    }

    #[test]
    fn xai_hosts_are_allowed_by_default() {
        let none = ConsentLedger::empty();
        let chat = [DataClass::Chat, DataClass::Personal];
        for host in ["api.x.ai", "grok.com", "x.ai", "imgen.x.ai"] {
            assert_eq!(decide(Step::Egress { dest: host, data: &chat, ledger: &none }), GateOutcome::Allow, "{host}");
        }
        assert_eq!(
            decide(Step::Egress { dest: "api.x.ai", data: &[DataClass::Sensitive], ledger: &none }),
            send_park("api.x.ai", "sensitive"),
            "sensitive data needs a grant even to xAI"
        );
    }

    #[test]
    fn new_destination_with_personal_data_is_hard_send() {
        let none = ConsentLedger::empty();
        assert_eq!(
            decide(Step::Egress { dest: "example.org", data: &[DataClass::Personal], ledger: &none }),
            send_park("example.org", "personal")
        );
        assert_eq!(
            decide(Step::Egress { dest: HUB_DEST, data: HUB_SYNC_DATA, ledger: &none }),
            send_park("hub", "chat, personal")
        );
        assert_eq!(
            decide(Step::Egress { dest: "example.org", data: &[], ledger: &none }),
            GateOutcome::Allow,
            "a public read carries no user data"
        );
        assert_eq!(
            decide(Step::Egress { dest: "localhost", data: &[DataClass::Personal], ledger: &none }),
            GateOutcome::Allow
        );
    }

    #[test]
    fn grant_allows_and_revoke_blocks_again() {
        let (d, _g) = dir("grant");
        let req = EgressReq::new("https://example.org/upload", &[DataClass::Personal]);
        assert_eq!(guard_egress(&d, &req), send_park("example.org", "personal"));
        assert!(read_egress(&d).is_empty(), "a blocked call logs nothing");
        let g = grant_destination(&d, "example.org", &[DataClass::Personal], UserClick::from_click()).unwrap();
        assert_eq!(guard_egress(&d, &req), GateOutcome::Allow);
        let log = read_egress(&d);
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].dest, "example.org");
        assert_eq!(log[0].grant_id, g.id);
        assert_eq!(log[0].basis, "grant");
        assert_eq!(log[0].data_classes, vec![DataClass::Personal]);
        assert_eq!(revoke_grant(&d, &g.id), Ok(true));
        assert_eq!(guard_egress(&d, &req), send_park("example.org", "personal"));
        assert_eq!(read_egress(&d).len(), 1);
    }

    #[test]
    fn egress_log_has_no_content_and_no_secrets() {
        let (d, _g) = dir("redact");
        let ids = vec!["pref-glow".to_string(), "tok-sk-abcdefghijklmnopqrstuv".to_string()];
        let req = EgressReq {
            target: "https://api.x.ai/v1/responses?api_key=sk-abcdefghijklmnopqrstuv",
            data: &[DataClass::Personal, DataClass::Chat, DataClass::Chat],
            node_ids: &ids,
            redactions: 2,
            span_id: "chat-1:42",
            origin: Origin::User,
        };
        assert_eq!(guard_egress(&d, &req), GateOutcome::Allow);
        guard_egress(&d, &EgressReq::new("http://127.0.0.1:4711/v1/responses", &[DataClass::Chat]));
        let text = fs::read_to_string(egress_path(&d)).unwrap();
        assert!(text.lines().all(at_rest::is_sealed_line), "egress log is sealed at rest");
        let opened = at_rest::read_sealed_jsonl(&d, &text, AAD_EGRESS, true);
        let line = opened.lines.first().unwrap();
        let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
        v["ts_ms"] = serde_json::json!(0);
        assert_eq!(
            v,
            serde_json::json!({
                "ts_ms": 0,
                "dest": "api.x.ai",
                "data_classes": ["chat", "personal"],
                "node_ids": ["pref-glow", "tok-[redacted]"],
                "redactions": 2,
                "grant_id": "",
                "span_id": "chat-1:42",
                "basis": "model_host",
                "origin": "user"
            })
        );
        assert_eq!(text.lines().count(), 1, "loopback is not egress: {text}");
        assert!(!text.contains("sk-abc"), "{text}");
        assert!(!opened.lines.concat().contains("sk-abc"), "{:?}", opened.lines);
        assert!(!text.contains("api.x.ai"), "nothing readable at rest: {text}");
    }

    #[test]
    fn approved_once_logs_without_a_grant() {
        let (d, _g) = dir("once");
        let req = EgressReq { span_id: "chat-2:7", ..EgressReq::new(HUB_DEST, HUB_SYNC_DATA) };
        record_approved_once(&d, &req).unwrap();
        let log = read_egress(&d);
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].dest, "hub");
        assert_eq!(log[0].grant_id, "approved-once");
        assert_eq!(log[0].basis, "approved_once");
        assert_eq!(log[0].span_id, "chat-2:7");
        assert_eq!(ConsentLedger::load(&d), ConsentLedger::empty(), "approve once writes no grant");
    }

    #[test]
    fn a_locked_log_writes_nothing_and_a_grant_send_does_not_go() {
        let (d, _g) = dir("locked");
        let store = std::sync::Arc::new(at_rest::MemoryKeyStore::new());
        at_rest::use_key_store_for(&d, store.clone());
        let req = EgressReq::new("https://example.org/upload", &[DataClass::Personal]);
        grant_destination(&d, "example.org", &[DataClass::Personal], UserClick::from_click()).unwrap();
        assert_eq!(guard_egress(&d, &req), GateOutcome::Allow);
        let before = fs::read_to_string(egress_path(&d)).unwrap();

        store.forget();
        at_rest::use_key_store_for(&d, store.clone());
        // The ledger is locked too, so the grant no longer applies: back to the hard card.
        assert_eq!(guard_egress(&d, &req), send_park("example.org", "personal"));
        let model = EgressReq::new("https://api.x.ai/v1/responses", &[DataClass::Chat]);
        assert_eq!(guard_egress(&d, &model), GateOutcome::Allow, "model calls are not blocked");
        let once = EgressReq::new(HUB_DEST, HUB_SYNC_DATA);
        assert!(record_approved_once(&d, &once).is_err(), "approve-once can't be logged, so it isn't sent");
        assert_eq!(fs::read_to_string(egress_path(&d)).unwrap(), before, "no line, no plaintext");
        let report = read_egress_report(&d);
        assert_eq!(report.locked, Some(Locked::Missing));
        assert!(report.lines.is_empty());
    }

    #[test]
    fn a_grant_send_that_cannot_be_logged_is_refused() {
        let (d, _g) = dir("refuse");
        grant_destination(&d, "example.org", &[DataClass::Personal], UserClick::from_click()).unwrap();
        // The ledger opens, but the log can't be written (a directory sits where the file goes).
        fs::create_dir_all(egress_path(&d)).unwrap();
        let req = EgressReq::new("https://example.org/upload", &[DataClass::Personal]);
        match guard_egress(&d, &req) {
            GateOutcome::Refuse { reason } => assert!(reason.starts_with("not sent: "), "{reason}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn model_text_is_chat_unless_it_carries_a_recall_line() {
        const CHAT: &[DataClass] = &[DataClass::Chat];
        const BOTH: &[DataClass] = &[DataClass::Chat, DataClass::Personal];
        assert_eq!(model_text_classes("https://example.org/?q=pier+four"), CHAT, "no recall pack: chat");
        let _recall = RecallScope::enter(vec!["The harbor ferry leaves at nine".into(), "short line".into()]);
        assert_eq!(model_text_classes("https://example.org/?q=ferry"), CHAT);
        assert_eq!(model_text_classes(r#"{"note":"the harbor ferry leaves at nine"}"#), BOTH);
        assert_eq!(model_text_classes("https://example.org/?q=the+harbor+ferry+leaves+at+nine"), BOTH);
        assert_eq!(model_text_classes("https://example.org/?q=The%20Harbor%20ferry%20leaves%20at%20nine"), BOTH);
        assert_eq!(model_text_classes("a short line"), CHAT, "lines under 16 chars say nothing");
        drop(_recall);
        assert_eq!(model_text_classes("the harbor ferry leaves at nine"), CHAT, "the scope ends with the turn");
        assert_eq!(percent_decoded("%zz%4"), "%zz%4", "bad escapes stay as typed");
    }

    #[test]
    fn origin_scope_tags_requests_and_restores() {
        assert_eq!(EgressReq::new("hub", &[]).origin, Origin::User);
        {
            let _a = OriginScope::enter(Origin::Automation);
            assert_eq!(EgressReq::new("hub", &[]).origin, Origin::Automation);
            {
                let _p = OriginScope::enter(Origin::Proactive);
                assert_eq!(current_origin(), Origin::Proactive);
            }
            assert_eq!(current_origin(), Origin::Automation);
            let (d, _g) = dir("origin");
            assert_eq!(guard_egress(&d, &EgressReq::new("https://api.x.ai/v1/x", &[DataClass::Chat])), GateOutcome::Allow);
            assert_eq!(read_egress(&d)[0].origin, Origin::Automation);
        }
        assert_eq!(current_origin(), Origin::User);
    }

    #[test]
    fn a_parked_send_times_out_to_deny_and_sends_nothing() {
        let (d, _g) = dir("park-ttl");
        let req = EgressReq::new("https://example.org/upload", &[DataClass::Personal]);
        let started = std::time::Instant::now();
        let got = guard_or_park_within(&d, &req, "web_fetch", Duration::from_millis(60), &mut || false);
        assert_eq!(
            got,
            Err("hard-class send: Send to example.org (personal) with no grant — Always cannot skip. Denied: nothing was sent.".into())
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(crate::harness::pending_parks(&d).is_empty(), "the card is gone after TTL");
        assert!(read_egress(&d).is_empty(), "no line for a send that never left");
        // Model hosts, plain reads, and loopback never wait on a card.
        assert_eq!(guard_or_park(&d, &EgressReq::new("https://api.x.ai/v1/x", &[DataClass::Personal]), "t", &mut || false), Ok(()));
        assert_eq!(guard_or_park(&d, &EgressReq::new("https://example.org/feed", &[]), "t", &mut || false), Ok(()));
        assert_eq!(guard_or_park(&d, &EgressReq::new("http://localhost:9/x", &[DataClass::Personal]), "t", &mut || false), Ok(()));
        let rows: Vec<(String, String)> = read_egress(&d).into_iter().map(|l| (l.dest, l.basis)).collect();
        assert_eq!(rows, vec![("api.x.ai".into(), "model_host".into()), ("example.org".into(), "public".into())]);
    }
}

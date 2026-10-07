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
//! [`decide`]: crate::harness::decide

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::harness::approval::{decide, GateOutcome, Step};
use crate::harness::consent::ConsentLedger;
use crate::harness::hard::HardClass;
use crate::harness::span::Origin;

/// The paired computers reached through the LAN hub (`/sync`).
pub const HUB_DEST: &str = "hub";
/// What `/sync` publishes: threads plus SOUL / USER / MEMORY.
pub const HUB_SYNC_DATA: &[DataClass] = &[DataClass::Chat, DataClass::Personal];
/// File name under the cabin config dir.
pub const EGRESS_FILE: &str = "egress.jsonl";
/// Past this size the log rolls to `egress.1.jsonl` (one old file kept).
const EGRESS_ROLL: u64 = 1024 * 1024;
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
    pub fn new(target: &'a str, data: &'a [DataClass]) -> Self {
        Self {
            target,
            data,
            node_ids: &[],
            redactions: 0,
            span_id: "",
            origin: Origin::User,
        }
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
            let _ = append_egress(config_dir, &line_for(req, &dest, basis.as_str(), &grant_id));
        }
    }
    outcome
}

/// Log a send the user approved once on a hard card (no standing grant).
pub fn record_approved_once(config_dir: &Path, req: &EgressReq<'_>) -> Result<(), String> {
    let dest = egress_dest(req.target);
    append_egress(config_dir, &line_for(req, &dest, "approved_once", "approved-once"))
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

/// Append one line (secrets redacted again on the way out). Rolls the file
/// past [`EGRESS_ROLL`].
pub fn append_egress(config_dir: &Path, line: &EgressLine) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let path = egress_path(config_dir);
    if fs::metadata(&path).map(|m| m.len() > EGRESS_ROLL).unwrap_or(false) {
        let _ = fs::rename(&path, config_dir.join("egress.1.jsonl"));
    }
    let text = serde_json::to_string(line).map_err(|e| e.to_string())?;
    let text = grokhub_core::redact_secrets(&text);
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path).map_err(|e| e.to_string())?;
    writeln!(f, "{text}").map_err(|e| e.to_string())
}

/// The last lines of the log (up to [`EGRESS_TAIL`] bytes), oldest first.
pub fn read_egress(config_dir: &Path) -> Vec<EgressLine> {
    let Ok(mut f) = fs::File::open(egress_path(config_dir)) else {
        return Vec::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(EGRESS_TAIL);
    if f.seek(SeekFrom::Start(from)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if f.take(EGRESS_TAIL).read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    if from > 0 {
        lines.next();
    }
    lines
        .filter_map(|l| serde_json::from_str::<EgressLine>(l.trim()).ok())
        .collect()
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
        let line = text.lines().next().unwrap();
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut v = v;
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
}

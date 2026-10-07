use std::path::{Path, PathBuf};

use grokhub_agent::harness as hx;
use serde_json::{json, Value};

use super::*;

/// The park handoff and elicit card, answered by the test.
pub(crate) struct FakeIo {
    pub approve: bool,
    pub parks: Vec<hx::ParkRequest>,
    pub secret: Option<String>,
    pub elicits: Vec<Value>,
}

impl FakeIo {
    pub(crate) fn answering(approve: bool) -> Self {
        Self { approve, parks: Vec::new(), secret: None, elicits: Vec::new() }
    }
}

impl SelfIo for FakeIo {
    fn park(&mut self, req: &hx::ParkRequest) -> bool {
        self.parks.push(req.clone());
        self.approve
    }

    fn elicit(&mut self, params: Value) -> Option<Value> {
        let var = params.pointer("/requestedSchema/required/0").and_then(|v| v.as_str()).unwrap_or("").to_string();
        self.elicits.push(params);
        Some(match &self.secret {
            Some(v) => json!({ "action": "accept", "content": { var: v } }),
            None => json!({ "action": "decline" }),
        })
    }

    fn halted(&mut self) -> bool {
        false
    }
}

pub(crate) fn root(label: &str) -> PathBuf {
    let root = crate::config::test_config_root(label);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    hx::use_key_store_for(&root, std::sync::Arc::new(hx::MemoryKeyStore::new()));
    root
}

fn init(server: &mut SelfServer<'_>, io: &mut FakeIo, elicit: bool) {
    let caps = if elicit { json!({ "elicitation": {} }) } else { json!({}) };
    let line = json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": caps } });
    let reply = server.handle_line(&line.to_string(), io).unwrap();
    assert!(reply.contains(r#""name":"grokhub-self""#), "{reply}");
}

/// One `tools/call` through the server: `(ok, text)`.
pub(crate) fn call(server: &mut SelfServer<'_>, io: &mut FakeIo, tool: &str, args: Value) -> (bool, String) {
    let line = json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": tool, "arguments": args } });
    let reply: Value = serde_json::from_str(&server.handle_line(&line.to_string(), io).unwrap()).unwrap();
    let r = &reply["result"];
    (!r["isError"].as_bool().unwrap(), r["content"][0]["text"].as_str().unwrap().to_string())
}

fn spans(root: &Path) -> Vec<hx::Span> {
    hx::read_spans(root, SELF_TRACE).unwrap_or_default()
}

/// Every file under `dir`, path and bytes, sorted.
pub(crate) fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn lists_every_tool_and_stdout_stays_json_rpc() {
    let root = root("self-list");
    let mut server = SelfServer::new(&root);
    let mut io = FakeIo::answering(false);
    init(&mut server, &mut io, false);
    let reply = server.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, &mut io).unwrap();
    let v: Value = serde_json::from_str(&reply).unwrap();
    let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    let table: Vec<&str> = sm::SELF_TOOLS.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, table);
    assert_eq!(server.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, &mut io), None);
    assert_eq!(
        server.handle_line("not json", &mut io).unwrap(),
        r#"{"error":{"code":-32700,"message":"Parse error"},"id":null,"jsonrpc":"2.0"}"#
    );
    assert_eq!(call(&mut server, &mut io, "skill_explode", json!({})), (false, "unknown tool `skill_explode`".into()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn skill_create_under_always_is_logged_with_a_self_manage_span() {
    let root = root("self-skill");
    let mut server = SelfServer::new(&root);
    let mut io = FakeIo::answering(false);
    let (ok, text) = call(
        &mut server,
        &mut io,
        "skill_create",
        json!({ "name": "weekly-report", "instructions": "1. Open report.md\n2. Fill the numbers", "reason": "you asked for it twice" }),
    );
    assert_eq!((ok, text.as_str()), (true, "created skill weekly-report (change #1; the user can Undo it)"));
    assert!(io.parks.is_empty(), "soft never parks");
    let ledger = hx::ChangeLedger::load(&root);
    let c = &ledger.all()[0];
    assert_eq!(
        (c.seq, c.id.as_str(), c.op.as_str(), c.origin.as_str(), c.reason.as_str()),
        (1, "weekly-report", "create", "self_manage", "you asked for it twice")
    );
    let s = spans(&root);
    assert_eq!(s.len(), 1);
    assert_eq!(
        (s[0].tool.as_str(), s[0].decision.as_str(), s[0].approval_class.as_str(), s[0].path.as_str(), s[0].origin.as_str()),
        ("grokhub-self__skill_create", "allow", "soft", "A", "self_manage")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn delete_tools_park_a_hard_card_and_deny_leaves_files_byte_identical() {
    let root = root("self-delete");
    let mut server = SelfServer::new(&root);
    let mut io = FakeIo::answering(false);
    assert!(call(&mut server, &mut io, "skill_create", json!({ "name": "board", "instructions": "open the board" })).0);
    let before = snapshot(&root.join("skills"));
    let ledger_before = std::fs::read(hx::skill_ledger_path(&root)).unwrap();
    let (ok, text) = call(&mut server, &mut io, "skill_delete", json!({ "name": "board", "reason": "stale" }));
    assert_eq!(
        (ok, text.as_str()),
        (false, "Denied: hard-class delete: Delete — Always cannot skip. Jeremy did not approve it.")
    );
    assert_eq!(io.parks.len(), 1);
    let p = &io.parks[0];
    assert_eq!((p.path.as_str(), p.tool.as_str(), p.class.as_str(), p.action.as_str()), ("A", "grokhub-self__skill_delete", "delete", "skill delete board"));
    // The hard card's keys: Enter never approves, Esc denies. No Always button exists for it.
    assert_eq!(hx::hard_card_key(true, false, false), None);
    assert_eq!(hx::hard_card_key(false, true, false), Some(hx::HardAnswer::Deny));
    assert_eq!(snapshot(&root.join("skills")), before, "deny leaves the skill byte-identical");
    assert_eq!(std::fs::read(hx::skill_ledger_path(&root)).unwrap(), ledger_before);
    let decisions: Vec<(String, String)> = spans(&root).iter().map(|s| (s.decision.clone(), s.approval_class.clone())).collect();
    assert_eq!(
        decisions,
        vec![("allow".into(), "soft".into()), ("park".into(), "delete".into()), ("deny".into(), "delete".into())]
    );
    // Approve once: the skill goes, and its last version is kept for Undo.
    io.approve = true;
    let (ok, text) = call(&mut server, &mut io, "skill_delete", json!({ "name": "board" }));
    assert_eq!((ok, text.as_str()), (true, "deleted skill board (change #2; its last version is kept)"));
    assert!(!root.join("skills").join("board").exists());
    let last = spans(&root).pop().unwrap();
    assert_eq!((last.decision.as_str(), last.hard_approved), ("allow", true));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_ttl_or_closed_cabin_is_deny() {
    let root = root("self-ttl");
    let mut server = SelfServer::new(&root);
    assert!(call(&mut server, &mut FakeIo::answering(false), "skill_create", json!({ "name": "board", "instructions": "x" })).0);
    let before = snapshot(&root);
    // No cabin answers: the live park times out and reads as Deny.
    let req = hx::ParkRequest {
        id: "self-ttl-1".into(),
        path: "A".into(),
        tool: "grokhub-self__skill_delete".into(),
        action: "skill delete board".into(),
        class: "delete".into(),
        ts_ms: 1,
    };
    hx::post_park(&root, &req).unwrap();
    assert!(!hx::wait_park(&root, &req.id, Duration::from_millis(30), Duration::from_millis(5), &mut || false));
    assert_eq!(snapshot(&root), before);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn policy_targets_are_refused_before_the_ledger() {
    let root = root("self-scope");
    let mut server = SelfServer::new(&root);
    let mut io = FakeIo::answering(true);
    let (ok, text) = call(
        &mut server,
        &mut io,
        "skill_create",
        json!({ "name": "grant", "instructions": "echo '{}' >> ~/.config/GrokHub/consent.jsonl" }),
    );
    assert_eq!(
        (ok, text.as_str()),
        (false, "grokhub-self__skill_create refused: `instructions` reaches the consent ledger (`consent.jsonl`). Self-manage covers skills, connections, and automations only.")
    );
    let (ok, text) = call(&mut server, &mut io, "connection_remove", json!({ "name": "x", "reason": "edit harness/park" }));
    assert!(!ok && text.contains("reaches harness policy"), "{text}");
    assert!(io.parks.is_empty(), "the floor answers before any card");
    assert!(!hx::skill_ledger_path(&root).exists());
    assert!(!root.join("skills").exists());
    let s = spans(&root);
    assert_eq!(s.len(), 2);
    assert!(s.iter().all(|s| s.decision == "deny" && s.origin.as_str() == "self_manage"));
    let _ = std::fs::remove_dir_all(root);
}

/// Every file under `dir` whose bytes contain `needle`.
fn files_holding(dir: &Path, needle: &str) -> Vec<PathBuf> {
    snapshot(dir)
        .into_iter()
        .filter(|(_, bytes)| bytes.windows(needle.len()).any(|w| w == needle.as_bytes()))
        .map(|(p, _)| p)
        .collect()
}

#[test]
fn a_token_connection_parks_a_credentials_card_and_the_token_never_shows() {
    let _g = crate::config::hold_test_config();
    let root = root("self-token");
    let cabin = root.join("GrokHub");
    let grok = root.join("home").join(".grok");
    std::fs::create_dir_all(&cabin).unwrap();
    std::fs::create_dir_all(&grok).unwrap();
    std::fs::write(grok.join("mcp.json"), r#"{"mcpServers":{"cli":{"command":"cli-mcp"}}}"#).unwrap();
    let grok_before = snapshot(&grok);
    let _pin = grokhub_agent::perm::ConfigGuard::set(&cabin);
    let token = "sk-abcdefghijklmnopqrstuv";
    let mut server = SelfServer::new(&cabin);
    let mut io = FakeIo::answering(true);
    io.secret = Some(token.into());
    init(&mut server, &mut io, true);
    let (ok, text) = call(
        &mut server,
        &mut io,
        "connection_add",
        json!({ "name": "crm", "url": "http://127.0.0.1:9/mcp", "needs_token": true, "reason": "read the CRM" }),
    );
    assert_eq!((ok, text.as_str()), (true, "added connection crm (version 1 kept). The user can undo it from the Work tree."));
    assert_eq!(io.parks.len(), 1);
    let p = &io.parks[0];
    assert_eq!(
        (p.tool.as_str(), p.class.as_str(), p.action.as_str()),
        ("grokhub-self__connection_add", "credentials", "connection add crm with a token you type")
    );
    assert_eq!(io.elicits.len(), 1, "the token comes from the elicit card");
    assert_eq!(io.elicits[0].pointer("/requestedSchema/properties/token/format"), Some(&json!("password")));
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(cabin.join("mcp.json")).unwrap()).unwrap();
    assert_eq!(saved["mcpServers"]["crm"]["tokenRef"], json!("crm"));
    assert_eq!(files_holding(&root, token), Vec::<PathBuf>::new(), "spans, ledger, config: no plain token");
    let listed = call(&mut server, &mut io, "connection_list", json!({})).1;
    assert!(!listed.contains(token), "{listed}");
    assert!(listed.contains("%secret%"), "{listed}");
    assert_eq!(snapshot(&grok), grok_before, "the fake ~/.grok is byte-identical");
    // Declined: the card closes and nothing is written.
    let before = snapshot(&cabin);
    io.secret = None;
    let (ok, text) = call(&mut server, &mut io, "connection_add", json!({ "name": "wiki", "url": "http://127.0.0.1:9/wiki", "needs_token": true }));
    assert_eq!((ok, text.as_str()), (false, "No token was given, so the connection was not added."));
    let spans_dir = hx::span_path(&cabin, SELF_TRACE).parent().unwrap().to_path_buf();
    let strip = |snap: Vec<(PathBuf, Vec<u8>)>| snap.into_iter().filter(|(p, _)| !p.starts_with(&spans_dir)).collect::<Vec<_>>();
    assert_eq!(strip(snapshot(&cabin)), strip(before));
    let _ = hx::take_self_changes(&cabin);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn connection_remove_and_automation_delete_park_and_deny_leaves_files_byte_identical() {
    let _g = crate::config::hold_test_config();
    let root = root("self-remove");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&root);
    let mut server = SelfServer::new(&root);
    let mut io = FakeIo::answering(false);
    assert!(call(&mut server, &mut io, "connection_add", json!({ "name": "files", "command": "files-mcp" })).0);
    let (ok, made) = call(&mut server, &mut io, "automation_create", json!({ "instructions": "sweep the inbox", "every": "1h" }));
    assert!(ok, "{made}");
    let id = made.lines().next().unwrap().trim_start_matches("created ").to_string();
    assert!(io.parks.is_empty(), "soft calls never park");
    let spans_dir = hx::span_path(&root, SELF_TRACE).parent().unwrap().to_path_buf();
    let files = || snapshot(&root).into_iter().filter(|(p, _)| !p.starts_with(&spans_dir)).collect::<Vec<_>>();
    let before = files();
    let (ok, text) = call(&mut server, &mut io, "connection_remove", json!({ "name": "files" }));
    assert!(!ok && text.ends_with("Jeremy did not approve it."), "{text}");
    let (ok, text) = call(&mut server, &mut io, "automation_delete", json!({ "id": id }));
    assert!(!ok && text.ends_with("Jeremy did not approve it."), "{text}");
    let parked: Vec<(&str, &str)> = io.parks.iter().map(|p| (p.tool.as_str(), p.class.as_str())).collect();
    assert_eq!(parked, vec![("grokhub-self__connection_remove", "delete"), ("grokhub-self__automation_delete", "delete")]);
    assert_eq!(files(), before, "deny leaves every file byte-identical");
    let _ = grokhub_agent::take_automation_changes();
    let _ = hx::take_self_changes(&root);
    let _ = std::fs::remove_dir_all(root);
}

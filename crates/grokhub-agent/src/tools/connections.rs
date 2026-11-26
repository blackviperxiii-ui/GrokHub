//! `connection_*`: MCP servers the agent adds, changes, or turns off on its
//! own (Spike-5b). Each write lands in the native MCP config inside the
//! cabin home (never `~/.grok`) through the ChangeLedger, so the user sees a
//! Work-tree row and can undo it. The user's own Add MCP stays as it was.
//!
//! - `connection_delete` is hard class delete (its name), and a
//!   `connection_add` with `needs_token` is hard class credentials: both park
//!   a card before this code runs, even under Always.
//! - A token is never an argument. The user types it on the cabin's masked
//!   elicit card; it is sealed with the keyring key and the entry only names
//!   it (`tokenRef`).
//! - A server on a host the user has not granted is refused: an agent-made
//!   connection that reaches a new host needs an egress grant first.
//! - `catalog_id` (or a name that matches the bundled catalog) reuses the
//!   Connectors install recipe. OAuth shows a URL elicit card and never
//!   opens the browser; cancel removes the entry.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use grokhub_core::mcp_catalog::{CatalogAuth, CatalogEntry};
use serde_json::{json, Map, Value};

use super::ToolOutput;
use crate::harness::{self as hx, ChangeTarget, McpFile, Origin};

/// What the user sees on the token card.
pub(crate) const TOKEN_TITLE: &str = "Token";

pub fn delete_schema() -> Value {
    json!({
        "type": "function",
        "name": "connection_delete",
        "description": "Remove one MCP server. Always asks the user first.",
        "parameters": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "reason": {"type": "string"}
            },
            "required": ["name"],
            "additionalProperties": false
        }
    })
}

fn text(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// A literal token, header, or env value in the arguments.
fn carries_secret(args: &Value) -> bool {
    let raw = args.to_string();
    grokhub_core::redact_secrets(&raw) != raw
        || [
            "bearerToken",
            "bearer_token",
            "headers",
            "env",
            "token",
            "api_key",
        ]
        .iter()
        .any(|k| args.get(*k).is_some())
}

fn name_of(args: &Value) -> Result<String, String> {
    let name = hx::entry_id(&text(args, "name"))?;
    if crate::mcp::is_desktop_server(&name) {
        return Err(format!(
            "{name} is {}'s own desktop server",
            grokhub_core::brand::product_name()
        ));
    }
    Ok(name)
}

fn record(name: &str, reason: &str, entry: Option<Value>) -> Result<Option<hx::Change>, String> {
    let config = crate::perm::config_dir();
    let path = crate::mcp::config_file();
    let target = McpFile { path: &path, name };
    let bytes = match &entry {
        Some(v) => Some(serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?),
        None => None,
    };
    let done = hx::record_change(&config, &target, Origin::SelfManage, reason, || {
        target.put(bytes.as_deref())
    })?;
    crate::mcp::invalidate();
    Ok(done)
}

fn current(name: &str) -> Result<Option<Map<String, Value>>, String> {
    let path = crate::mcp::config_file();
    let bytes = McpFile { path: &path, name }.read()?;
    Ok(bytes
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v.as_object().cloned()))
}

/// Add a connection, with the token asked through `ask` (the card's message in, the
/// typed value out). `grokhub --mcp-self` asks through MCP elicitation.
pub(crate) fn add_with(args: &Value, ask: &mut dyn FnMut(&str) -> Option<String>) -> ToolOutput {
    match add_inner(args, false, ask) {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(err),
    }
}

/// Change an existing connection. Fields left out keep their value, and a
/// sealed token stays unless `needs_token` asks for a new one.
pub(crate) fn modify_with(args: &Value, ask: &mut dyn FnMut(&str) -> Option<String>) -> ToolOutput {
    let mut run = || -> Result<String, String> {
        let name = name_of(args)?;
        let cur = current(&name)?.ok_or_else(|| format!("no connection named {name}"))?;
        let mut merged = args.as_object().cloned().unwrap_or_default();
        let has = |m: &Map<String, Value>, k: &str| {
            m.get(k)
                .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
        };
        if !has(&merged, "url") && !has(&merged, "command") {
            for k in ["url", "command"] {
                if let Some(v) = cur.get(k) {
                    merged.insert(k.into(), v.clone());
                }
            }
            if !merged.contains_key("args") {
                if let Some(v) = cur.get("args") {
                    merged.insert("args".into(), v.clone());
                }
            }
        }
        let keep_token = cur.contains_key("tokenRef") && !has(&merged, "needs_token");
        add_inner(&Value::Object(merged), keep_token, ask)
    };
    match run() {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(err),
    }
}

/// The connections in the cabin's MCP config, one line each. A sealed token
/// shows as `%secret%`.
pub(crate) fn list() -> ToolOutput {
    let path = crate::mcp::config_file();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let servers = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("mcpServers")
                .or_else(|| v.get("mcp_servers"))
                .and_then(|m| m.as_object())
                .cloned()
        })
        .unwrap_or_default();
    if servers.is_empty() {
        return ToolOutput::ok("no connections");
    }
    let rows: Vec<String> = servers
        .iter()
        .map(|(name, e)| {
            let target = e
                .get("url")
                .or_else(|| e.get("command"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let off = if e.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
                " (off)"
            } else {
                ""
            };
            let token = if e.get("tokenRef").is_some() {
                " token=%secret%"
            } else {
                ""
            };
            format!("{name}: {target}{off}{token}")
        })
        .collect();
    ToolOutput::ok(grokhub_core::redact_secrets(&rows.join("\n")))
}

/// A bundled catalog entry from `catalog_id` or a matching `name`.
pub(crate) fn resolve_catalog(args: &Value) -> Option<CatalogEntry> {
    let catalog = grokhub_core::mcp_catalog::bundled();
    let id = text(args, "catalog_id");
    if !id.is_empty() {
        return catalog.get(&id).cloned();
    }
    let name = text(args, "name");
    if name.is_empty() {
        return None;
    }
    catalog
        .entries
        .iter()
        .find(|e| e.id.eq_ignore_ascii_case(&name) || e.name.eq_ignore_ascii_case(&name))
        .cloned()
}

/// True when this add is a catalog key connector (credentials hard class).
pub(crate) fn catalog_needs_key(args: &Value) -> bool {
    resolve_catalog(args).is_some_and(|e| e.auth == CatalogAuth::Key)
}

fn drop_catalog_setup(name: &str, reason: &str) {
    let config = crate::perm::config_dir();
    let _ = record(name, reason, None);
    hx::forget_connection_token(&config, name);
    crate::mcp::invalidate();
}

pub(crate) fn add_catalog(
    entry: &CatalogEntry,
    reason: &str,
    ask: &mut dyn FnMut(&str) -> Option<String>,
) -> Result<String, String> {
    let name = entry.id.clone();
    if crate::mcp::is_desktop_server(&name) {
        return Err(format!(
            "{name} is {}'s own desktop server",
            grokhub_core::brand::product_name()
        ));
    }
    let config_entry = entry.config_entry();
    let obj = config_entry
        .as_object()
        .cloned()
        .ok_or_else(|| format!("{name}'s catalog entry is not an object"))?;
    match record(&name, reason, Some(Value::Object(obj)))? {
        Some(_) | None => {}
    }
    let next_turn =
        "Its tools join the next turn once this chat's tool list refreshes.";
    match entry.auth {
        CatalogAuth::None | CatalogAuth::Own => Ok(format!(
            "added catalog connection {name} ({label}). {next_turn}",
            label = entry.name
        )),
        CatalogAuth::Google => Ok(format!(
            "added catalog connection {name} ({label}). Finish Google setup on the MCPs & Connectors page. {next_turn}",
            label = entry.name
        )),
        CatalogAuth::Key => {
            let message = format!(
                "API key for {}. It is sealed with your keychain key and never shown to the model.",
                entry.name
            );
            let token = match ask(&message) {
                Some(t) => t,
                None => {
                    drop_catalog_setup(&name, "cancelled connector key");
                    return Err(format!(
                        "No key was given for {}, so nothing was kept.",
                        entry.name
                    ));
                }
            };
            let config = crate::perm::config_dir();
            if let Err(err) = hx::seal_connection_token(&config, &name, &token) {
                drop_catalog_setup(&name, "failed to seal connector key");
                return Err(err);
            }
            let mut with_ref = entry
                .config_entry()
                .as_object()
                .cloned()
                .ok_or_else(|| format!("{name}'s catalog entry is not an object"))?;
            with_ref.insert("tokenRef".into(), json!(name));
            if let Err(err) = record(&name, reason, Some(Value::Object(with_ref))) {
                hx::forget_connection_token(&config, &name);
                drop_catalog_setup(&name, "failed to save connector key");
                return Err(err);
            }
            Ok(format!(
                "added catalog connection {name} ({label}) with a sealed key. {next_turn}",
                label = entry.name
            ))
        }
        CatalogAuth::Oauth => add_catalog_oauth(entry),
    }
}

fn add_catalog_oauth(entry: &CatalogEntry) -> Result<String, String> {
    let name = entry.id.clone();
    let label = entry.name.clone();
    let session = crate::mcp::hook_session().unwrap_or_default();
    let card_id: RefCell<Option<String>> = RefCell::new(None);
    let offered = Arc::new(AtomicBool::new(false));
    let offered_flag = offered.clone();
    let open = |link: &str| -> Result<(), String> {
        let message = format!(
            "Sign in to {label} in your browser, then press I've finished. The app never opens the browser by itself."
        );
        match crate::mcp::offer_url(&label, &message, link) {
            Some(id) => {
                *card_id.borrow_mut() = Some(id);
                offered_flag.store(true, Ordering::SeqCst);
                Ok(())
            }
            None => Err(format!(
                "No sign-in card for {label}, so nothing was kept."
            )),
        }
    };
    let stop = || {
        let id = card_id.borrow().clone();
        match id.and_then(|id| crate::mcp::try_take_elicit(&session, &id)) {
            Some(crate::mcp::ElicitAnswer::Decline)
            | Some(crate::mcp::ElicitAnswer::Cancel) => true,
            _ => false,
        }
    };
    match crate::mcp::sign_in_until(&name, &open, &stop) {
        Ok(line) => Ok(format!(
            "{line}. Its tools join the next turn once this chat's tool list refreshes."
        )),
        Err(err) => {
            drop_catalog_setup(&name, "cancelled connector sign-in");
            if !offered.load(Ordering::SeqCst) {
                return Err(err);
            }
            if err.contains("stopped") {
                Err(format!(
                    "Sign-in for {label} was cancelled, so nothing was kept."
                ))
            } else {
                Err(format!(
                    "Sign-in for {label} did not finish ({err}), so nothing was kept."
                ))
            }
        }
    }
}

fn add_inner(
    args: &Value,
    keep_token: bool,
    ask: &mut dyn FnMut(&str) -> Option<String>,
) -> Result<String, String> {
    if carries_secret(args) {
        return Err("Never pass a token or header. Set needs_token and the user types it in. Nothing was added.".into());
    }
    let reason = text(args, "reason");
    if let Some(entry) = resolve_catalog(args) {
        let url = text(args, "url");
        let command = text(args, "command");
        if !url.is_empty() || !command.is_empty() {
            return Err(
                "catalog_id already carries the install recipe; leave url and command out."
                    .into(),
            );
        }
        return add_catalog(&entry, &reason, ask);
    }
    let name = name_of(args)?;
    let url = text(args, "url");
    let command = text(args, "command");
    let mut entry = Map::new();
    match (url.is_empty(), command.is_empty()) {
        (false, true) => {
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err("url must start with https:// or http://".into());
            }
            let config = crate::perm::config_dir();
            let ledger = hx::ConsentLedger::load(&config);
            let dest = hx::egress_dest(&url);
            // A standing connection is not a one-off chat fetch. Chat-only calls are
            // allowed without a grant; this still needs one unless the host is local
            // or a model host.
            let data = [hx::DataClass::Personal];
            if !hx::decide(hx::Step::Egress {
                dest: &dest,
                data: &data,
                ledger: &ledger,
            })
            .is_allow()
            {
                return Err(format!(
                    "{dest} is a new host for {product}. Grant it in Settings, Privacy before a connection can reach it. Nothing was added.", product = grokhub_core::brand::product_name()
                ));
            }
            entry.insert("url".into(), json!(url));
            entry.insert("type".into(), json!("http"));
        }
        (true, false) => {
            entry.insert("command".into(), json!(command));
            let list: Vec<String> = args
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if !list.is_empty() {
                entry.insert("args".into(), json!(list));
            }
        }
        _ => return Err("give url or command, not both".into()),
    }
    let needs_token = args.get("needs_token").and_then(|v| v.as_bool()) == Some(true);
    let config = crate::perm::config_dir();
    if needs_token {
        if !command.is_empty() {
            return Err("only an HTTP connection takes a token".into());
        }
        let message = format!("Token for the {name} connection. It is sealed with your keychain key and never shown to the model.");
        let token = ask(&message).ok_or("No token was given, so the connection was not added.")?;
        hx::seal_connection_token(&config, &name, &token)?;
        entry.insert("tokenRef".into(), json!(name));
    } else if keep_token {
        entry.insert("tokenRef".into(), json!(name));
    }
    match record(&name, &reason, Some(Value::Object(entry))) {
        Ok(Some(c)) => Ok(format!(
            "{} connection {name} (version {} kept). The user can undo it from the Work tree.",
            if c.op == hx::ChangeOp::Create {
                "added"
            } else {
                "updated"
            },
            c.seq
        )),
        Ok(None) => Ok(format!("connection {name} is already set up that way")),
        Err(err) => {
            if needs_token {
                hx::forget_connection_token(&config, &name);
            }
            Err(err)
        }
    }
}

pub fn disable(args: &Value) -> ToolOutput {
    let run = || -> Result<String, String> {
        let name = name_of(args)?;
        let mut entry = current(&name)?.ok_or_else(|| format!("no connection named {name}"))?;
        entry.insert("enabled".into(), json!(false));
        match record(&name, &text(args, "reason"), Some(Value::Object(entry)))? {
            Some(c) => Ok(format!(
                "turned off connection {name} (version {} kept). The user can undo it.",
                c.seq
            )),
            None => Ok(format!("connection {name} is already off")),
        }
    };
    match run() {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(err),
    }
}

pub fn delete(args: &Value) -> ToolOutput {
    let run = || -> Result<String, String> {
        let name = name_of(args)?;
        current(&name)?.ok_or_else(|| format!("no connection named {name}"))?;
        let reason = text(args, "reason");
        // The sealed token stays, so Undo brings the connection back whole.
        let c =
            record(&name, &reason, None)?.ok_or_else(|| format!("no connection named {name}"))?;
        Ok(format!(
            "removed connection {name} (version {} kept). The user can undo it.",
            c.seq
        ))
    };
    match run() {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::{Gate, PermMode};
    use crate::harness::{
        decide_harness, hard_card_key, resolve_park, AccessMode, ChangeKind, ChangeLedger,
        GateOutcome, HardClass, HardPark, UndoAsk, APPROVAL_TTL,
    };
    use crate::tools::{dispatch, ToolCtx};
    use std::path::{Path, PathBuf};

    const FAKE_TOKEN: &str = "sk-abcdefghijklmnopqrstuv";

    fn ctx_run(dir: &Path, name: &str, args: &str) -> ToolOutput {
        let ctx = ToolCtx {
            workspace: dir,
            desktop: None,
            stop: &|| false,
            tasks: None,
            owner: None,
            session: None,
        };
        dispatch(&ctx, name, args)
    }

    /// Every file under `dir`, with its bytes, in path order.
    fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push((p.clone(), std::fs::read(&p).unwrap_or_default()));
                }
            }
        }
        out.sort();
        out
    }

    /// Acceptance 1 and 5: the agent adds a connection. v1 is in the ledger
    /// (and queued for the Work-tree row), the entry lands in the cabin's own
    /// config, a fake `~/.grok` is byte-for-byte unchanged, and Undo removes
    /// the entry entirely.
    #[test]
    fn an_agent_connection_lands_in_the_cabin_home_and_undo_removes_it() {
        let root = crate::harness::test_dir("conn-add");
        let cabin = root.join("GrokHub");
        let grok = root.join("home").join(".grok");
        std::fs::create_dir_all(&grok).unwrap();
        std::fs::write(
            grok.join("mcp.json"),
            r#"{"mcpServers":{"cli":{"command":"cli-mcp"}}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(&cabin).unwrap();
        crate::harness::use_key_store_for(
            &cabin,
            std::sync::Arc::new(crate::harness::MemoryKeyStore::new()),
        );
        let _guard = crate::perm::ConfigGuard::set(&cabin);
        let grok_before = tree(&grok);
        let out = ctx_run(
            &root,
            "connection_add",
            r#"{"name":"notes","url":"http://127.0.0.1:9/mcp","reason":"read the notes server"}"#,
        );
        assert!(!out.failed, "{}", out.text);
        assert_eq!(
            out.text,
            "added connection notes (version 1 kept). The user can undo it from the Work tree."
        );
        let mcp = cabin.join("mcp.json");
        let text = std::fs::read_to_string(&mcp).unwrap();
        assert!(
            text.contains("\"notes\"") && text.contains("127.0.0.1:9"),
            "{text}"
        );
        assert_eq!(tree(&grok), grok_before, "~/.grok is untouched");
        let ledger = ChangeLedger::load_kind(&cabin, ChangeKind::Connection);
        let c = &ledger.all()[0];
        assert_eq!(
            (c.seq, c.id.as_str(), c.op.as_str(), c.origin.as_str()),
            (1, "notes", "create", "self_manage")
        );
        assert_eq!(c.reason, "read the notes server");
        let fresh = crate::harness::take_self_changes(&cabin);
        assert_eq!(fresh.len(), 1);
        assert_eq!((fresh[0].0, fresh[0].1.seq), (ChangeKind::Connection, 1));
        let back =
            crate::harness::undo_connection(&cabin, &mcp, "notes", UndoAsk::from_click()).unwrap();
        assert_eq!(back.now, None);
        let text = std::fs::read_to_string(&mcp).unwrap();
        assert!(!text.contains("notes"), "{text}");
        assert!(crate::mcp::load_servers().is_empty());
        assert_eq!(tree(&grok), grok_before);
    }

    #[test]
    fn a_new_host_needs_a_grant_and_a_literal_token_is_refused() {
        let root = crate::harness::test_dir("conn-host");
        let _guard = crate::perm::ConfigGuard::set(&root);
        let out = ctx_run(
            &root,
            "connection_add",
            r#"{"name":"weather","url":"https://weather.example.com/mcp","reason":"forecasts"}"#,
        );
        assert!(out.failed);
        assert_eq!(
            out.text,
            "weather.example.com is a new host for Loadout. Grant it in Settings, Privacy before a connection can reach it. Nothing was added."
        );
        let out = ctx_run(
            &root,
            "connection_add",
            &format!(
                r#"{{"name":"weather","url":"http://127.0.0.1:9/mcp","reason":"use {FAKE_TOKEN}"}}"#
            ),
        );
        assert!(out.failed);
        assert!(out.text.starts_with("Never pass a token"), "{}", out.text);
        assert!(!root.join("mcp.json").exists(), "nothing written");
        assert!(ChangeLedger::load_kind(&root, ChangeKind::Connection)
            .all()
            .is_empty());
    }

    /// Acceptance 4: a connector that needs a token is a credentials hard
    /// card. The token the user types is sealed: it is absent from the
    /// ledger, kept copies, the config, the tool output the model reads,
    /// and every other file in the fixture.
    #[test]
    fn a_token_connection_is_a_credentials_card_and_the_token_stays_out_of_everything() {
        let args = r#"{"name":"crm","url":"http://127.0.0.1:9/mcp","needs_token":true,"reason":"read the CRM"}"#;
        let always = Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop: true,
        };
        assert_eq!(
            decide_harness(
                &always,
                "connection_add",
                args,
                true,
                None,
                AccessMode::Full
            ),
            GateOutcome::Park {
                reason: "hard-class credentials: Credentials / secrets — Always cannot skip".into(),
                hard: Some(HardClass::Credentials),
                needs_user: true,
            }
        );
        let root = crate::harness::test_dir("conn-token");
        let _guard = crate::perm::ConfigGuard::set(&root);
        let mut emit = |_view: crate::mcp::ElicitView| {};
        let mut wait = |_id: &str| {
            crate::mcp::ElicitAnswer::Accept(serde_json::json!({ "token": FAKE_TOKEN }))
        };
        let out = crate::mcp::with_elicit(true, "conn-token", &mut emit, &mut wait, || {
            ctx_run(&root, "connection_add", args)
        });
        assert!(!out.failed, "{}", out.text);
        assert!(!out.text.contains(FAKE_TOKEN));
        for (path, bytes) in tree(&root) {
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !text.contains(FAKE_TOKEN) && !text.contains("abcdefghijklmnopqrstuv"),
                "token in {}",
                path.display()
            );
        }
        let entry = std::fs::read_to_string(root.join("mcp.json")).unwrap();
        assert!(entry.contains("\"tokenRef\": \"crm\""), "{entry}");
        assert_eq!(
            crate::harness::open_connection_token(&root, "crm").as_deref(),
            Some(FAKE_TOKEN)
        );
        // Unattended (or declined): no card, no token, nothing added.
        let other = crate::harness::test_dir("conn-token-none");
        let _guard = crate::perm::ConfigGuard::set(&other);
        let out = ctx_run(&other, "connection_add", args);
        assert!(out.failed);
        assert_eq!(
            out.text,
            "No token was given, so the connection was not added."
        );
        assert!(!other.join("mcp.json").exists());
    }

    /// Acceptance 3: deleting a skill, an automation, or a connection under
    /// Always + Full parks a hard delete card. Enter never approves, Esc and
    /// the TTL deny.
    #[test]
    fn agent_deletes_are_hard_cards_even_under_always_and_full() {
        let always = Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop: true,
        };
        for (tool, args) in [
            ("connection_delete", r#"{"name":"notes"}"#),
            ("scheduler_delete", r#"{"task_id":"auto-1"}"#),
            (
                "run_terminal_command",
                r#"{"command":"rm -rf skills/weekly-report"}"#,
            ),
            ("grokhub__skill_delete", r#"{"name":"weekly-report"}"#),
        ] {
            assert_eq!(
                decide_harness(&always, tool, args, true, None, AccessMode::Full),
                GateOutcome::Park {
                    reason: "hard-class delete: Delete — Always cannot skip".into(),
                    hard: Some(HardClass::Delete),
                    needs_user: true,
                },
                "{tool}"
            );
        }
        assert_eq!(
            hard_card_key(true, false, false),
            None,
            "Enter does not approve"
        );
        let park = HardPark {
            id: "p".into(),
            tool: "connection_delete".into(),
            class: HardClass::Delete,
            reason: String::new(),
            parked_at: std::time::Instant::now() - APPROVAL_TTL,
        };
        assert!(matches!(
            resolve_park(
                &park,
                std::time::Instant::now(),
                Some(crate::harness::HardAnswer::ApproveOnce)
            ),
            GateOutcome::Refuse { .. }
        ));
        // Soft: add and turn off follow the GB pill.
        assert_eq!(
            decide_harness(
                &always,
                "connection_disable",
                r#"{"name":"notes","reason":"x"}"#,
                true,
                None,
                AccessMode::Full
            ),
            GateOutcome::Allow
        );
    }

    #[test]
    fn disable_and_delete_keep_versions_and_undo_walks_back() {
        let root = crate::harness::test_dir("conn-disable");
        let _guard = crate::perm::ConfigGuard::set(&root);
        let add = ctx_run(
            &root,
            "connection_add",
            r#"{"name":"local","command":"local-mcp","args":["--stdio"],"reason":"files"}"#,
        );
        assert!(!add.failed, "{}", add.text);
        let off = ctx_run(
            &root,
            "connection_disable",
            r#"{"name":"local","reason":"it kept failing"}"#,
        );
        assert_eq!(
            off.text,
            "turned off connection local (version 2 kept). The user can undo it."
        );
        assert!(std::fs::read_to_string(root.join("mcp.json"))
            .unwrap()
            .contains("\"enabled\": false"));
        let gone = ctx_run(
            &root,
            "connection_delete",
            r#"{"name":"local","reason":"unused"}"#,
        );
        assert_eq!(
            gone.text,
            "removed connection local (version 3 kept). The user can undo it."
        );
        let mcp = root.join("mcp.json");
        crate::harness::undo_connection(&root, &mcp, "local", UndoAsk::from_click()).unwrap();
        let text = std::fs::read_to_string(&mcp).unwrap();
        assert!(
            text.contains("local-mcp") && text.contains("\"enabled\": false"),
            "{text}"
        );
        crate::harness::undo_connection(&root, &mcp, "local", UndoAsk::from_typing()).unwrap();
        assert!(!std::fs::read_to_string(&mcp)
            .unwrap()
            .contains("\"enabled\""));
        let ops: Vec<&str> = ChangeLedger::load_kind(&root, ChangeKind::Connection)
            .all()
            .iter()
            .map(|c| c.op.as_str())
            .collect();
        assert_eq!(ops, ["create", "modify", "delete", "undo", "undo"]);
        let refused = ctx_run(
            &root,
            "connection_add",
            r#"{"name":"grokhub-desktop","command":"x","reason":"y"}"#,
        );
        assert_eq!(
            refused.text,
            "grokhub-desktop is Loadout's own desktop server"
        );
    }

    /// Settings → Connectors Disconnect: the entry, its sealed token, and its
    /// sign-in go; the ledger line is the user's (no Work-tree row), and
    /// `/connections` undo brings the entry back.
    #[test]
    fn a_user_disconnect_removes_the_entry_token_and_sign_in() {
        let root = crate::harness::test_dir("conn-disconnect");
        crate::harness::use_key_store_for(
            &root,
            std::sync::Arc::new(crate::harness::MemoryKeyStore::new()),
        );
        let _guard = crate::perm::ConfigGuard::set(&root);
        let mcp = root.join("mcp.json");
        std::fs::write(
            &mcp,
            r#"{"mcpServers":{"linear":{"url":"https://mcp.linear.app/mcp","tokenRef":"linear"},"notes":{"command":"notes-mcp"}}}"#,
        )
        .unwrap();
        crate::harness::seal_connection_token(&root, "linear", FAKE_TOKEN).unwrap();
        crate::harness::seal_mcp_signin(&root, "linear", r#"{"access_token":"x"}"#).unwrap();
        assert!(crate::harness::open_mcp_signin(&root, "linear").is_some());

        assert_eq!(
            crate::mcp::disconnect("linear").unwrap(),
            "Disconnected linear"
        );

        let names: Vec<String> = crate::mcp::load_servers().into_keys().collect();
        assert_eq!(names, ["notes"]);
        assert_eq!(crate::harness::open_connection_token(&root, "linear"), None);
        assert_eq!(crate::harness::open_mcp_signin(&root, "linear"), None);
        let ledger = ChangeLedger::load_kind(&root, ChangeKind::Connection);
        let c = &ledger.all()[0];
        assert_eq!(
            (
                c.id.as_str(),
                c.op.as_str(),
                c.origin.as_str(),
                c.reason.as_str()
            ),
            ("linear", "delete", "user", "disconnected in Settings")
        );
        assert!(
            crate::harness::take_self_changes(&root).is_empty(),
            "a user's own disconnect posts no Work-tree row"
        );
        assert_eq!(
            crate::mcp::disconnect("linear").unwrap_err(),
            "linear isn't one of Loadout's connections"
        );
        crate::harness::undo_connection(&root, &mcp, "linear", UndoAsk::from_click()).unwrap();
        let names: Vec<String> = crate::mcp::load_servers().into_keys().collect();
        assert_eq!(names, ["linear", "notes"]);
    }

    #[test]
    fn a_marketplace_install_is_idempotent_and_never_overwrites() {
        let root = crate::harness::test_dir("conn-install");
        let _guard = crate::perm::ConfigGuard::set(&root);
        let mcp = root.join("mcp.json");
        std::fs::write(&mcp, r#"{"mcpServers":{"notes":{"command":"notes-mcp"}}}"#).unwrap();
        let entry = json!({"command": "npx", "args": ["-y", "@playwright/mcp@latest"]});

        assert_eq!(
            crate::mcp::install("playwright", &entry).unwrap(),
            crate::mcp::Installed::Added
        );
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&mcp).unwrap()).unwrap();
        assert_eq!(saved["mcpServers"]["playwright"], entry);
        assert_eq!(
            saved["mcpServers"]["notes"],
            json!({"command": "notes-mcp"})
        );
        assert_eq!(
            crate::mcp::install("playwright", &entry).unwrap(),
            crate::mcp::Installed::AlreadyThere
        );
        let ledger = ChangeLedger::load_kind(&root, ChangeKind::Connection);
        assert_eq!(
            ledger.all().len(),
            1,
            "a second install writes no ledger line"
        );
        let c = &ledger.all()[0];
        assert_eq!(
            (
                c.id.as_str(),
                c.op.as_str(),
                c.origin.as_str(),
                c.reason.as_str()
            ),
            ("playwright", "create", "user", "installed from Marketplace")
        );
        assert!(crate::harness::take_self_changes(&root).is_empty());

        assert_eq!(
            crate::mcp::install("notes", &json!({"command": "other"})).unwrap_err(),
            "A connector named notes is already set up differently; disconnect it first"
        );
        assert_eq!(
            crate::mcp::install(
                "exa",
                &json!({"url": "https://mcp.exa.ai/mcp", "headers": {"x-api-key": "k"}})
            )
            .unwrap_err(),
            "exa's entry carries credentials; a key is saved separately"
        );
        let names: Vec<String> = crate::mcp::load_servers().into_keys().collect();
        assert_eq!(names, ["notes", "playwright"]);
    }

    #[test]
    fn a_saved_key_is_sealed_and_named_by_the_entry() {
        let root = crate::harness::test_dir("conn-save-key");
        crate::harness::use_key_store_for(
            &root,
            std::sync::Arc::new(crate::harness::MemoryKeyStore::new()),
        );
        let _guard = crate::perm::ConfigGuard::set(&root);
        let mcp = root.join("mcp.json");
        let entry = json!({"command": "npx", "args": ["-y", "brave-search-mcp"], "tokenEnv": "BRAVE_API_KEY"});
        crate::mcp::install("brave", &entry).unwrap();
        assert_eq!(
            crate::mcp::save_key("missing", FAKE_TOKEN).unwrap_err(),
            "missing isn't installed, so it has nowhere to keep a key"
        );
        assert_eq!(
            crate::mcp::save_key("brave", "  ").unwrap_err(),
            "No key was typed, so nothing was saved"
        );

        assert_eq!(
            crate::mcp::save_key("brave", FAKE_TOKEN).unwrap(),
            "Saved the key for brave"
        );

        let text = std::fs::read_to_string(&mcp).unwrap();
        assert!(!text.contains(FAKE_TOKEN), "{text}");
        let saved: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(saved["mcpServers"]["brave"]["tokenRef"], json!("brave"));
        assert_eq!(
            saved["mcpServers"]["brave"]["tokenEnv"],
            json!("BRAVE_API_KEY")
        );
        assert_eq!(
            crate::harness::open_connection_token(&root, "brave").as_deref(),
            Some(FAKE_TOKEN)
        );
        let def = &crate::mcp::load_servers()["brave"];
        assert_eq!(
            (def.token_ref.as_deref(), def.token_env.as_deref()),
            (Some("brave"), Some("BRAVE_API_KEY"))
        );
        assert_eq!(
            crate::mcp::install("brave", &entry).unwrap(),
            crate::mcp::Installed::AlreadyThere,
            "installing again keeps the saved key"
        );
    }

    #[test]
    fn catalog_id_or_name_resolves_a_bundled_connector() {
        let by_id = resolve_catalog(&json!({"catalog_id": "notion", "name": "x", "reason": "r"}));
        assert_eq!(by_id.as_ref().map(|e| e.id.as_str()), Some("notion"));
        let by_name = resolve_catalog(&json!({"name": "Notion", "reason": "r"}));
        assert_eq!(by_name.as_ref().map(|e| e.id.as_str()), Some("notion"));
        assert!(catalog_needs_key(&json!({"catalog_id": "brave-search"})));
        assert!(!catalog_needs_key(&json!({"catalog_id": "notion"})));
        assert!(!catalog_needs_key(&json!({"name": "notes"})));
    }

    #[test]
    fn a_catalog_none_connector_lands_and_cancel_is_n_a() {
        let root = crate::harness::test_dir("conn-catalog-none");
        let _guard = crate::perm::ConfigGuard::set(&root);
        let out = ctx_run(
            &root,
            "connection_add",
            r#"{"name":"context7","catalog_id":"context7","reason":"docs"}"#,
        );
        assert!(!out.failed, "{}", out.text);
        assert!(
            out.text.contains("added catalog connection context7")
                && out.text.contains("next turn"),
            "{}",
            out.text
        );
        let text = std::fs::read_to_string(root.join("mcp.json")).unwrap();
        assert!(text.contains("mcp.context7.com"), "{text}");
        assert!(!text.contains("tokenRef"), "{text}");
    }

    #[test]
    fn a_catalog_key_decline_leaves_no_config() {
        let root = crate::harness::test_dir("conn-catalog-key-deny");
        crate::harness::use_key_store_for(
            &root,
            std::sync::Arc::new(crate::harness::MemoryKeyStore::new()),
        );
        let _guard = crate::perm::ConfigGuard::set(&root);
        let always = Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop: true,
        };
        let args = r#"{"name":"brave-search","catalog_id":"brave-search","reason":"search"}"#;
        assert_eq!(
            decide_harness(
                &always,
                "connection_add",
                args,
                true,
                None,
                AccessMode::Full
            ),
            GateOutcome::Park {
                reason: "hard-class credentials: Credentials / secrets — Always cannot skip".into(),
                hard: Some(HardClass::Credentials),
                needs_user: true,
            }
        );
        let mut emit = |_view: crate::mcp::ElicitView| {};
        let mut wait = |_id: &str| crate::mcp::ElicitAnswer::Decline;
        let out = crate::mcp::with_elicit(true, "conn-catalog-key", &mut emit, &mut wait, || {
            ctx_run(&root, "connection_add", args)
        });
        assert!(out.failed, "{}", out.text);
        assert!(
            out.text.contains("No key was given") && out.text.contains("nothing was kept"),
            "{}",
            out.text
        );
        assert!(
            !root.join("mcp.json").exists()
                || !std::fs::read_to_string(root.join("mcp.json"))
                    .unwrap_or_default()
                    .contains("brave-search"),
            "cancel must leave no brave-search entry"
        );
    }

    #[test]
    fn a_catalog_oauth_card_never_opens_the_browser_and_decline_clears_config() {
        let root = crate::harness::test_dir("conn-catalog-oauth-deny");
        crate::harness::use_key_store_for(
            &root,
            std::sync::Arc::new(crate::harness::MemoryKeyStore::new()),
        );
        let _guard = crate::perm::ConfigGuard::set(&root);
        let server = crate::mcp::AuthMcp::start();
        let entry = CatalogEntry {
            id: "auth-demo".into(),
            name: "Auth Demo".into(),
            publisher: "Test".into(),
            category: "Test".into(),
            description: "OAuth fixture".into(),
            logo: "A".into(),
            transport: "http".into(),
            command: Vec::new(),
            url: server.url(),
            auth: CatalogAuth::Oauth,
            key_env: String::new(),
            key_hint: String::new(),
            registry_id: String::new(),
            source: "https://example.com/auth-demo".into(),
            capabilities: vec!["echo".into()],
            permissions: "Test only.".into(),
        };
        let (tx, inbox) = crate::mcp::ElicitInbox::pair();
        crate::mcp::attach_elicit("oauth-deny", inbox);
        let opened = Arc::new(std::sync::Mutex::new(String::new()));
        let seen = opened.clone();
        let mut emit = move |view: crate::mcp::ElicitView| {
            assert_eq!(view.mode, "url");
            assert!(!view.url.is_empty());
            *seen.lock().unwrap() = view.url.clone();
            let _ = tx.send(crate::mcp::ElicitNote {
                id: view.id,
                action: "decline".into(),
                content: None,
            });
        };
        let mut wait = |_id: &str| crate::mcp::ElicitAnswer::Decline;
        let mut ask = |_m: &str| None;
        let out = crate::mcp::with_elicit(true, "oauth-deny", &mut emit, &mut wait, || {
            add_catalog(&entry, "connect from chat", &mut ask)
        });
        crate::mcp::detach_elicit("oauth-deny");
        assert!(out.is_err(), "{out:?}");
        let err = out.unwrap_err();
        assert!(
            err.contains("cancelled") && err.contains("nothing was kept"),
            "{err}"
        );
        assert!(
            !opened.lock().unwrap().is_empty(),
            "the URL card must show the authorize link"
        );
        let text = std::fs::read_to_string(root.join("mcp.json")).unwrap_or_default();
        assert!(
            !text.contains("auth-demo"),
            "decline must remove the staged entry: {text}"
        );
        assert!(crate::harness::open_mcp_signin(&root, "auth-demo").is_none());
    }

    #[test]
    fn a_catalog_oauth_finish_seals_sign_in_for_the_next_turn() {
        let root = crate::harness::test_dir("conn-catalog-oauth-ok");
        crate::harness::use_key_store_for(
            &root,
            std::sync::Arc::new(crate::harness::MemoryKeyStore::new()),
        );
        let _guard = crate::perm::ConfigGuard::set(&root);
        let server = crate::mcp::AuthMcp::start();
        let entry = CatalogEntry {
            id: "auth-ok".into(),
            name: "Auth Ok".into(),
            publisher: "Test".into(),
            category: "Test".into(),
            description: "OAuth fixture".into(),
            logo: "A".into(),
            transport: "http".into(),
            command: Vec::new(),
            url: server.url(),
            auth: CatalogAuth::Oauth,
            key_env: String::new(),
            key_hint: String::new(),
            registry_id: String::new(),
            source: "https://example.com/auth-ok".into(),
            capabilities: vec!["echo".into()],
            permissions: "Test only.".into(),
        };
        let (tx, inbox) = crate::mcp::ElicitInbox::pair();
        crate::mcp::attach_elicit("oauth-ok", inbox);
        let approve = crate::mcp::approving_browser("good-code");
        let mut emit = move |view: crate::mcp::ElicitView| {
            assert_eq!(view.mode, "url");
            approve(&view.url).unwrap();
            let _ = tx.send(crate::mcp::ElicitNote {
                id: view.id,
                action: "accept".into(),
                content: Some(json!({})),
            });
        };
        let mut wait = |_id: &str| crate::mcp::ElicitAnswer::Accept(json!({}));
        let mut ask = |_m: &str| None;
        let out = crate::mcp::with_elicit(true, "oauth-ok", &mut emit, &mut wait, || {
            add_catalog(&entry, "connect from chat", &mut ask)
        });
        crate::mcp::detach_elicit("oauth-ok");
        let line = out.expect("oauth should finish");
        assert!(
            line.contains("Signed in to auth-ok") && line.contains("next turn"),
            "{line}"
        );
        let text = std::fs::read_to_string(root.join("mcp.json")).unwrap();
        assert!(text.contains("auth-ok") && text.contains(&server.url()), "{text}");
        assert!(crate::harness::open_mcp_signin(&root, "auth-ok").is_some());
    }
}

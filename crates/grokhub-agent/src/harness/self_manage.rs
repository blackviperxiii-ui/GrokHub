//! Spike-5b self-management targets: connections and automations the agent
//! makes or changes on its own go through the ChangeLedger like skills do.
//!
//! - A connection is one server entry in the native MCP config
//!   (`{config_dir}/mcp.json`, inside the cabin home; never `~/.grok`).
//! - An automation is one job in `automations.json`. Undo writes the list
//!   back with the same pretty JSON every writer uses, so a modify undone
//!   leaves the file byte-identical.
//! - A token a connection needs is sealed with the private-data key held in
//!   the OS keyring (`at_rest`), never in the entry, the ledger, a kept copy,
//!   a span, or the model input. The entry names it with `tokenRef`.
//! - At most [`SELF_AUTOMATION_WEEK_CAP`] new automations a week on the
//!   agent's own, until the user has kept one ([`automation_cap_refusal`]).

use std::fs;
use std::path::{Path, PathBuf};

use grokhub_core::amr::Sealer;
use grokhub_core::Automation;
use serde_json::{json, Map, Value};

use crate::harness::at_rest::LearnedVault;
use crate::harness::changes::{
    entry_id, private_write, undo_change, ChangeKind, ChangeLedger, ChangeOp, ChangeTarget, Reverted, UndoAsk,
};
use crate::harness::span::Origin;

/// New agent-made automations allowed in [`WEEK_MS`] before the user keeps one.
pub const SELF_AUTOMATION_WEEK_CAP: usize = 2;
pub const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Largest `automations.json` or `mcp.json` a target reads.
const STORE_CAP: u64 = 32 * 1024 * 1024;
/// Sealed connection tokens, one file per server.
const TOKEN_DIR: &str = "connection-tokens";

fn read_store(path: &Path) -> Result<String, String> {
    match fs::metadata(path) {
        Ok(m) if m.len() > STORE_CAP => Err(format!("{} is too large", file_label(path))),
        Ok(_) => fs::read_to_string(path).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.to_string()),
    }
}

fn file_label(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// One job in `automations.json`, by id.
pub struct AutomationsFile<'a> {
    pub path: &'a Path,
    pub id: &'a str,
}

impl AutomationsFile<'_> {
    fn list(&self) -> Result<Vec<Automation>, String> {
        let text = read_store(self.path)?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file_label(self.path)))
    }
}

impl ChangeTarget for AutomationsFile<'_> {
    fn kind(&self) -> ChangeKind {
        ChangeKind::Automation
    }

    fn id(&self) -> Result<String, String> {
        entry_id(self.id)
    }

    fn file(&self) -> Result<PathBuf, String> {
        Ok(self.path.to_path_buf())
    }

    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        let id = self.id()?;
        match self.list()?.iter().find(|a| a.id == id) {
            Some(a) => serde_json::to_vec_pretty(a).map(Some).map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    fn put(&self, bytes: Option<&[u8]>) -> Result<(), String> {
        let id = self.id()?;
        let mut list = self.list()?;
        match bytes {
            Some(b) => {
                let row: Automation = serde_json::from_slice(b).map_err(|e| e.to_string())?;
                match list.iter_mut().find(|a| a.id == id) {
                    Some(slot) => *slot = row,
                    None => list.push(row),
                }
            }
            None => list.retain(|a| a.id != id),
        }
        let text = serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?;
        private_write(self.path, text.as_bytes())
    }

    fn label_of(&self, bytes: &[u8]) -> String {
        serde_json::from_slice::<Automation>(bytes).map(|a| a.name).unwrap_or_default()
    }
}

/// One server entry in an MCP config file (`mcpServers`), by name.
pub struct McpFile<'a> {
    pub path: &'a Path,
    pub name: &'a str,
}

impl McpFile<'_> {
    fn servers(&self) -> Result<Map<String, Value>, String> {
        let text = read_store(self.path)?;
        if text.trim().is_empty() {
            return Ok(Map::new());
        }
        let value: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file_label(self.path)))?;
        Ok(value
            .get("mcpServers")
            .or_else(|| value.get("mcp_servers"))
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default())
    }
}

impl ChangeTarget for McpFile<'_> {
    fn kind(&self) -> ChangeKind {
        ChangeKind::Connection
    }

    fn id(&self) -> Result<String, String> {
        entry_id(self.name)
    }

    fn file(&self) -> Result<PathBuf, String> {
        Ok(self.path.to_path_buf())
    }

    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        let id = self.id()?;
        match self.servers()?.get(&id) {
            Some(v) => serde_json::to_vec_pretty(v).map(Some).map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    fn put(&self, bytes: Option<&[u8]>) -> Result<(), String> {
        let id = self.id()?;
        let mut servers = self.servers()?;
        match bytes {
            Some(b) => {
                let entry: Value = serde_json::from_slice(b).map_err(|e| e.to_string())?;
                servers.insert(id, entry);
            }
            None => {
                servers.remove(&id);
            }
        }
        let text = serde_json::to_string_pretty(&json!({ "mcpServers": servers })).map_err(|e| e.to_string())?;
        private_write(self.path, text.as_bytes())
    }
}

fn token_path(config_dir: &Path, name: &str) -> Result<PathBuf, String> {
    Ok(config_dir.join(TOKEN_DIR).join(format!("{}.sealed", entry_id(name)?)))
}

fn token_aad(name: &str) -> String {
    format!("grokhub-connection-token:v1:{name}")
}

/// Seal a token the user typed for one connection. Fails closed when the
/// keyring has no key: nothing is written.
pub(crate) fn seal_connection_token(config_dir: &Path, name: &str, token: &str) -> Result<(), String> {
    let path = token_path(config_dir, name)?;
    let sealed = LearnedVault::new(config_dir).seal(&token_aad(name), token)?;
    private_write(&path, sealed.as_bytes())
}

/// The token sealed for one connection, opened with the keyring key. `None`
/// when there is none or it can't be opened.
pub fn open_connection_token(config_dir: &Path, name: &str) -> Option<String> {
    let sealed = fs::read_to_string(token_path(config_dir, name).ok()?).ok()?;
    LearnedVault::new(config_dir).open(&token_aad(name), sealed.trim()).ok()
}

/// Remove a connection's sealed token (its entry left the config).
pub(crate) fn forget_connection_token(config_dir: &Path, name: &str) {
    if let Ok(path) = token_path(config_dir, name) {
        let _ = fs::remove_file(path);
    }
}

/// Undo the newest change on one connection. When that leaves the entry
/// gone (an undone add), its sealed token goes too, so nothing of it stays.
pub fn undo_connection(config_dir: &Path, mcp_file: &Path, name: &str, ask: UndoAsk) -> Result<Reverted, String> {
    let done = undo_change(config_dir, &McpFile { path: mcp_file, name }, ask)?;
    if done.now.is_none() {
        forget_connection_token(config_dir, name);
    }
    Ok(done)
}

/// Why one more agent-made automation is refused, or `None` when it may go:
/// [`SELF_AUTOMATION_WEEK_CAP`] creates in the [`WEEK_MS`] before `now_ms`,
/// and the user has not kept one yet.
pub fn automation_cap_refusal(config_dir: &Path, now_ms: u64) -> Option<String> {
    let ledger = ChangeLedger::load_kind(config_dir, ChangeKind::Automation);
    if ledger.accepted_any() {
        return None;
    }
    let made = ledger
        .all()
        .iter()
        .filter(|c| c.op == ChangeOp::Create && c.origin == Origin::SelfManage)
        .filter(|c| c.at <= now_ms && now_ms - c.at < WEEK_MS)
        .count();
    (made >= SELF_AUTOMATION_WEEK_CAP).then(|| {
        format!(
            "Not added: GrokHub already made {SELF_AUTOMATION_WEEK_CAP} automations on its own this week. \
             Keep one from its Work-tree row, or add this one yourself on the Automations page."
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::changes::{
        accept_change, ledger_path, read_scope_findings, record_change, LEDGER_SCOPE_VIOLATION,
    };

    fn job(id: &str, name: &str, every: u32) -> Automation {
        Automation {
            id: id.into(),
            name: name.into(),
            schedule: "heartbeat".into(),
            time: "09:00".into(),
            times: vec![],
            instructions: format!("{name} now"),
            heartbeat_every_min: every,
            check_command: String::new(),
            enabled: true,
            last_run: None,
            next_run: Some(1_000),
            run_count: 0,
            health: Default::default(),
        }
    }

    fn save(path: &Path, list: &[Automation]) {
        fs::write(path, serde_json::to_string_pretty(list).unwrap()).unwrap();
    }

    /// Acceptance 2: the agent modifies an automation (v2); Undo leaves
    /// `automations.json` byte-identical to v1.
    #[test]
    fn undoing_a_modified_automation_leaves_the_file_byte_identical() {
        let root = crate::harness::test_dir("self-auto-modify");
        let path = root.join("automations.json");
        save(&path, &[job("auto-a", "board digest", 30), job("auto-b", "inbox sweep", 60)]);
        let v1 = fs::read(&path).unwrap();
        let target = AutomationsFile { path: &path, id: "auto-b" };
        let c = record_change(&root, &target, Origin::SelfManage, "every 15 min instead", || {
            save(&path, &[job("auto-a", "board digest", 30), job("auto-b", "inbox sweep", 15)]);
            Ok(())
        })
        .unwrap()
        .expect("a change");
        assert_eq!((c.kind.as_str(), c.op, c.id.as_str(), c.label.as_str()), ("automation", ChangeOp::Modify, "auto-b", "inbox sweep"));
        assert_ne!(fs::read(&path).unwrap(), v1);
        let line = fs::read_to_string(ledger_path(&root, ChangeKind::Automation)).unwrap();
        assert!(!line.contains("inbox sweep now"), "no job text in the ledger: {line}");
        let back = undo_change(&root, &target, UndoAsk::from_click()).unwrap();
        assert_eq!(back.change.undoes, Some(1));
        assert_eq!(fs::read(&path).unwrap(), v1, "byte-identical to v1");
        // Undo survives a restart: the ledger reloads from disk.
        let ledger = ChangeLedger::load_kind(&root, ChangeKind::Automation);
        assert_eq!(ledger.all().len(), 2);
        assert_eq!(ledger.undo_target("auto-b"), None);
        assert_eq!(ChangeLedger::load(&root).all().len(), 0, "the skill ledger is its own file");
    }

    #[test]
    fn an_automation_holding_a_secret_is_not_kept_or_changed() {
        let root = crate::harness::test_dir("self-auto-secret");
        let path = root.join("automations.json");
        let mut row = job("auto-a", "deploy", 30);
        row.instructions = "curl -H 'Bearer abcdefghijklmnopqrstuvwxyz' https://example.com".into();
        save(&path, &[row]);
        let before = fs::read(&path).unwrap();
        let target = AutomationsFile { path: &path, id: "auto-a" };
        let err = record_change(&root, &target, Origin::SelfManage, "tweak", || {
            save(&path, &[]);
            Ok(())
        })
        .unwrap_err();
        assert_eq!(err, "this automation holds a secret, so GrokHub won't keep or change it");
        assert_eq!(fs::read(&path).unwrap(), before, "the write never ran");
        assert!(!root.join("changes/automations").exists(), "no kept copy");
    }

    /// Acceptance 1 (ledger half): a created connection is v1, and Undo
    /// removes it from the config entirely.
    #[test]
    fn undoing_a_created_connection_removes_the_entry() {
        let root = crate::harness::test_dir("self-conn-create");
        let path = root.join("mcp.json");
        fs::write(&path, "{\n  \"mcpServers\": {\n    \"mine\": {\n      \"command\": \"mine-mcp\"\n    }\n  }\n}").unwrap();
        let target = McpFile { path: &path, name: "weather" };
        let c = record_change(&root, &target, Origin::SelfManage, "weather lookups", || {
            target.put(Some(br#"{"url":"https://weather.example.com/mcp","type":"http"}"#))
        })
        .unwrap()
        .unwrap();
        assert_eq!((c.seq, c.op, c.kind.as_str()), (1, ChangeOp::Create, "connection"));
        assert!(fs::read_to_string(&path).unwrap().contains("weather.example.com"));
        undo_change(&root, &target, UndoAsk::from_click()).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("weather"), "{text}");
        assert!(text.contains("mine-mcp"), "the user's own server stays: {text}");
    }

    /// Acceptance 6: a ledger write aimed at harness policy or the consent
    /// store is refused, nothing is written, and a finding is logged.
    #[test]
    fn the_scope_guard_refuses_policy_and_consent_targets() {
        let root = crate::harness::test_dir("self-scope");
        fs::create_dir_all(root.join("harness")).unwrap();
        for (file, why) in [
            (root.join("harness").join("hard-class.json"), "Refused: GrokHub never changes harness policy on its own."),
            (root.join("consent.jsonl"), "Refused: GrokHub never changes the consent store on its own."),
            (root.join("app.json"), "Refused: GrokHub never changes the Access settings on its own."),
            (root.join("home").join(".grok").join("mcp.json"), "Refused: the Grok CLI home (~/.grok) is not GrokHub's to change."),
        ] {
            let target = McpFile { path: &file, name: "sneaky" };
            let mut ran = false;
            let err = record_change(&root, &target, Origin::SelfManage, "widen", || {
                ran = true;
                Ok(())
            })
            .unwrap_err();
            assert_eq!(err, why);
            assert!(!ran, "the write never runs");
            assert!(!file.exists());
        }
        let findings = read_scope_findings(&root);
        assert_eq!(findings.len(), 4);
        assert_eq!(findings[0].detector, LEDGER_SCOPE_VIOLATION);
        assert_eq!((findings[1].kind.as_str(), findings[1].id.as_str(), findings[1].target.as_str()), ("connection", "sneaky", "consent.jsonl"));
        assert!(!ledger_path(&root, ChangeKind::Connection).exists(), "no ledger line");
    }

    #[test]
    fn the_cap_allows_two_agent_automations_a_week_until_one_is_kept() {
        let root = crate::harness::test_dir("self-cap");
        let path = root.join("automations.json");
        let mut list = Vec::new();
        for (i, id) in ["auto-1", "auto-2"].iter().enumerate() {
            let target = AutomationsFile { path: &path, id };
            list.push(job(id, &format!("job {i}"), 30));
            let snapshot = list.clone();
            record_change(&root, &target, Origin::SelfManage, "made by the agent", || {
                save(&path, &snapshot);
                Ok(())
            })
            .unwrap();
        }
        let at = ChangeLedger::load_kind(&root, ChangeKind::Automation).all()[1].at;
        assert_eq!(
            automation_cap_refusal(&root, at + 1).as_deref(),
            Some(
                "Not added: GrokHub already made 2 automations on its own this week. \
                 Keep one from its Work-tree row, or add this one yourself on the Automations page."
            )
        );
        assert_eq!(automation_cap_refusal(&root, at + WEEK_MS + 1), None, "a week later the cap resets");
        accept_change(&root, ChangeKind::Automation, "auto-2", UndoAsk::from_click()).unwrap();
        assert_eq!(automation_cap_refusal(&root, at + 1), None, "kept one, so the cap lifts");
        let ledger = ChangeLedger::load_kind(&root, ChangeKind::Automation);
        assert_eq!(ledger.open_self_change("auto-2"), None, "kept: no row");
        assert_eq!(ledger.open_self_change("auto-1").map(|c| c.seq), Some(1));
        assert_eq!(ledger.undo_target("auto-2").map(|c| c.seq), Some(2), "Keep is not an undo target");
    }

    #[test]
    fn connection_tokens_are_sealed_and_never_plain() {
        let root = crate::harness::test_dir("self-token");
        seal_connection_token(&root, "notes", "sk-abcdefghijklmnopqrstuv").unwrap();
        let on_disk = fs::read_to_string(root.join(TOKEN_DIR).join("notes.sealed")).unwrap();
        assert!(on_disk.starts_with("gh-sealed:v1:"), "{on_disk}");
        assert!(!on_disk.contains("abcdefghijklmnopqrstuv"));
        assert_eq!(open_connection_token(&root, "notes").as_deref(), Some("sk-abcdefghijklmnopqrstuv"));
        assert_eq!(open_connection_token(&root, "other"), None);
        forget_connection_token(&root, "notes");
        assert_eq!(open_connection_token(&root, "notes"), None);
    }
}

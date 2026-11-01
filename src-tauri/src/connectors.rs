//! Connectors (cross-platform recode, Phase 3e): the MCP servers in the same
//! `mcp.json` the cabin and the agent loop use. The page lists them, checks
//! them, adds one from the catalog or a pasted config, signs one in, saves
//! its key after an approval card, switches one off and on, reconnects one,
//! signs one out, and disconnects one after an OK.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use grokhub_agent::harness::{self as hx, ParkRequest};
use grokhub_agent::mcp::{self, DoctorRow, SignIn};
use grokhub_core::mcp_catalog::{self, Catalog, CatalogAuth, CatalogEntry};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub name: String,
    /// "connected", "error", "stopped", "not checked" or "disabled".
    pub status: String,
    pub tools: usize,
    pub error: String,
    /// The command or URL the entry runs.
    pub detail: String,
    /// "Signed in to Linear as ada@example.com" when signed in.
    pub account: String,
    /// An HTTP server with no sign-in yet; the cabin signs it in for now.
    pub needs_sign_in: bool,
    /// Computer-use servers belong to the desktop and are not disconnected here.
    pub can_disconnect: bool,
    /// False once the switch turned it off; it is listed but never started.
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    pub status: String,
    pub rows: Vec<Row>,
}

fn row(r: DoctorRow) -> Row {
    Row {
        can_disconnect: !mcp::is_desktop_server(&r.name),
        needs_sign_in: r.sign_in == SignIn::SignedOut,
        enabled: r.status != "disabled",
        account: match r.sign_in {
            SignIn::SignedIn(line) => line,
            _ => String::new(),
        },
        name: r.name,
        status: r.status,
        tools: r.tool_count,
        error: r.last_error,
        detail: r.detail,
    }
}

fn rows(list: Vec<DoctorRow>) -> Vec<Row> {
    list.into_iter().map(row).collect()
}

/// The entries as configured, without starting any of them.
pub fn list(workspace: &Path) -> Vec<Row> {
    mcp::set_workspace(workspace);
    rows(mcp::configured())
}

/// Starts each enabled server that is not running and pings the rest.
pub fn check(workspace: &Path) -> Vec<Row> {
    mcp::set_workspace(workspace);
    rows(mcp::doctor())
}

pub fn restart(workspace: &Path, name: &str) -> Changed {
    mcp::set_workspace(workspace);
    let status = match mcp::restart(name) {
        Ok(()) => format!("Reconnected {name}"),
        Err(err) => err,
    };
    Changed {
        status,
        rows: rows(mcp::doctor()),
    }
}

pub fn sign_out(workspace: &Path, name: &str) -> Changed {
    mcp::set_workspace(workspace);
    let status = mcp::sign_out(name);
    Changed {
        status,
        rows: rows(mcp::configured()),
    }
}

/// Only after the page asked "Disconnect {name}?" and got a yes.
pub fn disconnect(workspace: &Path, name: &str) -> Result<Changed, String> {
    if mcp::is_desktop_server(name) {
        return Err(format!(
            "{name} belongs to the desktop and stays connected."
        ));
    }
    mcp::set_workspace(workspace);
    let status = mcp::disconnect(name)?;
    Ok(Changed {
        status,
        rows: rows(mcp::configured()),
    })
}

/// One catalog connector for the Add sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRow {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub category: String,
    pub description: String,
    /// "none", "oauth", "key", "google" or "own".
    pub auth: &'static str,
    /// What the key field asks for, when `auth` is "key".
    pub key_hint: String,
    /// What Add runs or reaches.
    pub target: String,
    pub installed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    pub rows: Vec<CatalogRow>,
    /// What Refresh did, when it ran.
    pub note: Option<String>,
}

/// A key waiting on its approval card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyCard {
    pub id: String,
    pub name: String,
    /// What the card says, with no key in it.
    pub action: String,
}

fn auth_word(auth: CatalogAuth) -> &'static str {
    match auth {
        CatalogAuth::None => "none",
        CatalogAuth::Oauth => "oauth",
        CatalogAuth::Key => "key",
        CatalogAuth::Google => "google",
        CatalogAuth::Own => "own",
    }
}

/// The registry's newer names and descriptions, after Refresh.
static OVERLAY: Mutex<Option<Catalog>> = Mutex::new(None);

fn current_catalog() -> Catalog {
    OVERLAY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(mcp_catalog::bundled)
}

fn catalog_rows(catalog: &Catalog, installed: &[String]) -> Vec<CatalogRow> {
    catalog
        .entries
        .iter()
        .map(|e| CatalogRow {
            id: e.id.clone(),
            name: e.name.clone(),
            publisher: e.publisher.clone(),
            category: e.category.clone(),
            description: e.description.clone(),
            auth: auth_word(e.auth),
            key_hint: e.key_hint.clone(),
            target: e.target(),
            installed: installed.contains(&e.id),
        })
        .collect()
}

/// The catalog shipped in this build. `refresh` (the user's Refresh click,
/// never on its own) reads newer names and descriptions from the official
/// MCP registry through the guarded fetch; a failure keeps the list.
pub fn catalog(workspace: &Path, refresh: bool) -> CatalogView {
    let note = refresh.then(|| match mcp_catalog::refresh_from_official_registry() {
        Ok((fresh, updated)) => {
            *OVERLAY.lock().unwrap_or_else(|e| e.into_inner()) = Some(fresh);
            if updated > 0 {
                format!(
                    "Updated {updated} connector{} from the official registry.",
                    if updated == 1 { "" } else { "s" }
                )
            } else {
                "The catalog already matches the official registry.".to_string()
            }
        }
        Err(err) => format!("Couldn't refresh the catalog ({err}). Showing the built-in list."),
    });
    let installed: Vec<String> = list(workspace).into_iter().map(|r| r.name).collect();
    CatalogView {
        rows: catalog_rows(&current_catalog(), &installed),
        note,
    }
}

fn catalog_entry(id: &str) -> Result<CatalogEntry, String> {
    current_catalog()
        .get(id)
        .cloned()
        .ok_or_else(|| format!("{id} isn't in the catalog"))
}

/// Add from the catalog. It is written enabled and starts on Check or first
/// use; a key or a sign-in comes after.
pub fn add_catalog(workspace: &Path, id: &str) -> Result<Changed, String> {
    let entry = catalog_entry(id)?;
    mcp::set_workspace(workspace);
    let status = match mcp::install(&entry.id, &entry.config_entry())? {
        mcp::Installed::Added => match entry.auth {
            CatalogAuth::Key => format!("Added {}. Paste its key to finish", entry.name),
            CatalogAuth::Oauth => format!("Added {}. Sign in to finish", entry.name),
            _ => format!("Added {}", entry.name),
        },
        mcp::Installed::AlreadyThere => format!("{} was already added", entry.name),
    };
    Ok(Changed {
        status,
        rows: rows(mcp::configured()),
    })
}

/// Add every server in a pasted `mcpServers` config. Names already set up
/// are kept as they are.
pub fn add_config(workspace: &Path, text: &str) -> Result<Changed, String> {
    if text.trim().is_empty() {
        return Err("Paste an mcpServers config first.".into());
    }
    mcp::set_workspace(workspace);
    let report = mcp::import_documents(&[text.to_string()])?;
    mcp::invalidate();
    mcp::shutdown_all();
    Ok(Changed {
        status: report.summary(),
        rows: rows(mcp::configured()),
    })
}

/// Turn a server off or back on.
pub fn set_enabled(workspace: &Path, name: &str, on: bool) -> Result<Changed, String> {
    mcp::set_workspace(workspace);
    let status = mcp::set_enabled(name, on)?;
    Ok(Changed {
        status,
        rows: rows(mcp::configured()),
    })
}

/// Only an https sign-in page goes to the browser.
pub fn https_only(url: &str, open: &dyn Fn(&str) -> Result<(), String>) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("That sign-in page isn't https, so it wasn't opened.".into());
    }
    open(url)
}

/// Sign in to an HTTP server in the browser `open` launches, then reconnect.
pub fn sign_in(
    workspace: &Path,
    name: &str,
    open: &dyn Fn(&str) -> Result<(), String>,
) -> Result<Changed, String> {
    mcp::set_workspace(workspace);
    let status = mcp::sign_in(name, &|url| https_only(url, open))?;
    Ok(Changed {
        status,
        rows: rows(mcp::configured()),
    })
}

/// Keys waiting on their card: the waiter thread holds the key, never this map.
type Waiter = (std::path::PathBuf, JoinHandle<Result<String, String>>);
static KEY_WAITERS: Mutex<Option<HashMap<String, Waiter>>> = Mutex::new(None);

/// How long a key card waits before it counts as Deny.
pub const KEY_CARD_WAIT: Duration = hx::APPROVAL_TTL;

/// Post the credentials card for `name`'s key and wait for its answer on a
/// background thread. Approve seals the key and names it in `mcp.json`;
/// Deny, a timeout or a closed app saves nothing. The key is never logged
/// or returned.
pub fn start_key(workspace: &Path, name: &str, key: String) -> Result<KeyCard, String> {
    if key.trim().is_empty() {
        return Err(format!("Type the {name} key first."));
    }
    mcp::set_workspace(workspace);
    if !list(workspace).iter().any(|r| r.name == name) {
        return Err(format!(
            "{name} isn't added yet, so it has nowhere to keep a key."
        ));
    }
    let config = grokhub_agent::perm::config_dir();
    let id = format!("mcp-key-{name}-{}", grokhub_core::pkce::oauth_nonce());
    let action = format!("Save its API key ({name})");
    hx::post_park(
        &config,
        &ParkRequest {
            id: id.clone(),
            path: "E".into(),
            tool: "mcp_connector".into(),
            action: action.clone(),
            class: "credentials".into(),
            ts_ms: grokhub_core::now_ms(),
        },
    )?;
    let waiter = std::thread::spawn({
        let (id, name, config) = (id.clone(), name.to_string(), config.clone());
        move || {
            // The same config folder as the card, on this thread too.
            let _pin = grokhub_agent::perm::ConfigGuard::set(config.clone());
            let ok = hx::wait_park(
                &config,
                &id,
                KEY_CARD_WAIT,
                Duration::from_millis(100),
                &mut || false,
            );
            if !ok {
                return Err(format!("Nothing saved. {name}'s key was denied."));
            }
            mcp::save_key(&name, &key)
        }
    });
    KEY_WAITERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(id.clone(), (config, waiter));
    Ok(KeyCard {
        id,
        name: name.to_string(),
        action,
    })
}

/// The card's Save or Deny.
pub fn answer_key(id: &str, approve: bool) -> Result<Changed, String> {
    let (config, waiter) = KEY_WAITERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .remove(id)
        .ok_or("That key card is closed. Type the key again.")?;
    hx::answer_park(&config, id, approve)?;
    let status = match waiter.join() {
        Ok(Ok(line)) => line,
        Ok(Err(err)) => err,
        Err(_) => "Nothing saved.".to_string(),
    };
    Ok(Changed {
        status,
        rows: rows(mcp::configured()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Dirs {
        workspace: PathBuf,
        config: PathBuf,
        _guard: grokhub_agent::perm::ConfigGuard,
    }

    impl Drop for Dirs {
        fn drop(&mut self) {
            mcp::shutdown_all();
            mcp::invalidate();
        }
    }

    fn dirs(name: &str, servers: serde_json::Value) -> Dirs {
        let root = std::env::temp_dir().join(format!(
            "grokhub-shell-mcp-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let (config, workspace) = (root.join("cfg"), root.join("ws"));
        std::fs::create_dir_all(&config).expect("cfg");
        std::fs::create_dir_all(&workspace).expect("ws");
        let guard = grokhub_agent::perm::ConfigGuard::set(&config);
        std::fs::write(
            mcp::config_file(),
            serde_json::json!({ "mcpServers": servers }).to_string(),
        )
        .expect("mcp.json");
        mcp::shutdown_all();
        mcp::invalidate();
        Dirs {
            workspace,
            config,
            _guard: guard,
        }
    }

    fn fake_server() -> serde_json::Value {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/e2e/fake-mcp.mjs");
        serde_json::json!({
            "command": "node",
            "args": [script.to_string_lossy()],
            "startup_timeout_sec": 15,
        })
    }

    #[test]
    fn a_working_server_checks_as_connected_with_its_tool() {
        let d = dirs("ok", serde_json::json!({ "box": fake_server() }));
        let listed = list(&d.workspace);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "box");
        assert_eq!(listed[0].status, "not checked");
        assert!(listed[0].detail.contains("fake-mcp.mjs"), "{:?}", listed[0]);

        let checked = check(&d.workspace);
        assert_eq!(checked[0].status, "connected", "{:?}", checked[0]);
        assert_eq!(checked[0].tools, 1);
        assert_eq!(checked[0].error, "");
        assert!(!checked[0].needs_sign_in);
        assert!(checked[0].can_disconnect);

        let again = restart(&d.workspace, "box");
        assert_eq!(again.status, "Reconnected box");
        assert_eq!(again.rows[0].status, "connected");
    }

    #[test]
    fn a_missing_command_shows_its_error_and_disconnect_removes_it() {
        let d = dirs(
            "bad",
            serde_json::json!({
                "gone": { "command": "/no/such/grokhub-shell-mcp-binary" },
                "box": fake_server(),
            }),
        );
        let checked = check(&d.workspace);
        let gone = checked.iter().find(|r| r.name == "gone").expect("gone");
        assert_eq!(gone.status, "error");
        assert!(!gone.error.is_empty());
        assert_eq!(gone.tools, 0);

        let changed = disconnect(&d.workspace, "gone").expect("disconnect");
        assert_eq!(changed.status, "Disconnected gone");
        assert_eq!(
            changed
                .rows
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            vec!["box"]
        );
        let saved = std::fs::read_to_string(d.config.join("mcp.json")).expect("read");
        assert!(!saved.contains("grokhub-shell-mcp-binary"), "{saved}");
        assert!(saved.contains("fake-mcp.mjs"), "{saved}");

        assert_eq!(
            disconnect(&d.workspace, "gone"),
            Err(format!(
                "gone isn't one of {}'s connections",
                grokhub_core::brand::product_name()
            ))
        );
    }

    #[test]
    fn a_disabled_server_is_not_started() {
        let mut entry = fake_server();
        entry["enabled"] = serde_json::json!(false);
        let d = dirs("off", serde_json::json!({ "box": entry }));
        let checked = check(&d.workspace);
        assert_eq!(checked[0].status, "disabled");
        assert_eq!(checked[0].tools, 0);
        let again = restart(&d.workspace, "box");
        assert_eq!(again.status, "MCP server `box` is disabled");
    }

    #[test]
    fn add_from_the_catalog_writes_the_entry_once() {
        let d = dirs("add", serde_json::json!({}));
        let view = catalog(&d.workspace, false);
        assert!(view.note.is_none());
        let brave = view
            .rows
            .iter()
            .find(|r| r.id == "brave-search")
            .expect("brave");
        assert_eq!(brave.auth, "key");
        assert!(!brave.installed);
        let added = add_catalog(&d.workspace, "brave-search").expect("add");
        assert_eq!(added.status, "Added Brave Search. Paste its key to finish");
        assert_eq!(added.rows[0].name, "brave-search");
        assert!(added.rows[0].enabled);
        let saved = std::fs::read_to_string(d.config.join("mcp.json")).expect("read");
        assert!(saved.contains("brave-search"), "{saved}");
        assert!(catalog(&d.workspace, false)
            .rows
            .iter()
            .any(|r| r.id == "brave-search" && r.installed));
        assert_eq!(
            add_catalog(&d.workspace, "brave-search").map(|c| c.status),
            Ok("Brave Search was already added".to_string())
        );
        assert_eq!(
            add_catalog(&d.workspace, "no-such-connector").map(|c| c.status),
            Err("no-such-connector isn't in the catalog".to_string())
        );
    }

    #[test]
    fn a_pasted_config_adds_both_servers() {
        let d = dirs("paste", serde_json::json!({}));
        let text = r#"{"mcpServers":{"one":{"command":"echo"},"two":{"url":"https://mcp.example.com/mcp"}}}"#;
        let changed = add_config(&d.workspace, text).expect("paste");
        assert_eq!(changed.status, "Imported one, two");
        let names: Vec<_> = changed.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["one", "two"]);
        assert!(changed.rows[1].needs_sign_in);
        assert_eq!(
            add_config(&d.workspace, "  "),
            Err("Paste an mcpServers config first.".to_string())
        );
    }

    #[test]
    fn the_switch_turns_a_server_off_and_on() {
        let d = dirs("switch", serde_json::json!({ "box": fake_server() }));
        let off = set_enabled(&d.workspace, "box", false).expect("off");
        assert_eq!(off.status, "Turned off box");
        assert!(!off.rows[0].enabled);
        assert_eq!(check(&d.workspace)[0].status, "disabled");
        let on = set_enabled(&d.workspace, "box", true).expect("on");
        assert_eq!(on.status, "Turned on box");
        assert!(on.rows[0].enabled);
        assert_eq!(on.rows[0].status, "not checked");
    }

    #[test]
    fn sign_in_refuses_a_stdio_server_and_a_non_https_page() {
        let d = dirs("sign-in", serde_json::json!({ "box": fake_server() }));
        let opened = std::cell::RefCell::new(Vec::<String>::new());
        let open = |url: &str| {
            opened.borrow_mut().push(url.to_string());
            Ok(())
        };
        assert_eq!(
            sign_in(&d.workspace, "box", &open).map(|c| c.status),
            Err("MCP server `box` runs on this machine and has no sign-in".to_string())
        );
        assert!(https_only("http://evil.example/login", &open).is_err());
        assert!(https_only("https://mcp.example.com/authorize", &open).is_ok());
        assert_eq!(*opened.borrow(), vec!["https://mcp.example.com/authorize"]);
    }

    fn card_seen(config: PathBuf) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            for _ in 0..400 {
                if let Some(req) = hx::pending_parks(&config).pop() {
                    assert_eq!(req.class, "credentials");
                    assert!(!req.action.contains("abcdefghijklmnopqrstuv"));
                    return req.id;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panic!("no credentials card");
        })
    }

    #[test]
    fn a_key_waits_for_its_card_and_deny_saves_nothing() {
        let d = dirs("key", serde_json::json!({}));
        add_catalog(&d.workspace, "brave-search").expect("add");
        assert_eq!(
            start_key(&d.workspace, "brave-search", "  ".into()),
            Err("Type the brave-search key first.".to_string())
        );
        let seen = card_seen(d.config.clone());
        let card = start_key(
            &d.workspace,
            "brave-search",
            "sk-abcdefghijklmnopqrstuv".into(),
        )
        .expect("card");
        assert_eq!(seen.join().expect("park"), card.id);
        assert_eq!(card.action, "Save its API key (brave-search)");
        let denied = answer_key(&card.id, false).expect("deny");
        assert_eq!(
            denied.status,
            "Nothing saved. brave-search's key was denied."
        );
        assert!(hx::open_connection_token(&d.config, "brave-search").is_none());
        let saved = std::fs::read_to_string(d.config.join("mcp.json")).expect("read");
        assert!(!saved.contains("tokenRef"), "{saved}");
        assert!(answer_key(&card.id, true).is_err());

        let card = start_key(
            &d.workspace,
            "brave-search",
            "sk-abcdefghijklmnopqrstuv".into(),
        )
        .expect("card");
        let approved = answer_key(&card.id, true).expect("approve");
        let saved = std::fs::read_to_string(d.config.join("mcp.json")).expect("read");
        assert!(!saved.contains("abcdefghijklmnopqrstuv"), "{saved}");
        if approved.status == "Saved the key for brave-search" {
            assert_eq!(
                hx::open_connection_token(&d.config, "brave-search").as_deref(),
                Some("sk-abcdefghijklmnopqrstuv")
            );
            assert!(saved.contains("tokenRef"), "{saved}");
        } else {
            // A machine with no system password store seals nothing.
            assert!(
                approved.status.contains("system password store"),
                "{}",
                approved.status
            );
            assert!(!saved.contains("tokenRef"), "{saved}");
        }
    }
}

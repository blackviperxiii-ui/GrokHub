//! Connectors (cross-platform recode, Phase 3e): the MCP servers in the same
//! `mcp.json` the cabin and the agent loop use. The page lists them, checks
//! them, reconnects one, signs one out, and disconnects one after an OK.

use std::path::Path;

use grokhub_agent::mcp::{self, DoctorRow, SignIn};
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
}

//! Marketplace install → mcp.json → chat tool schemas → mock stdio MCP call.
//! No network; uses the `fake_mcp` test binary as the stdio server.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use grokhub_agent::mcp::{self, Installed};
use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::{
    execute, run_loop, schemas_for, CancelToken, ClosedPermits, FunctionCall, Gate, HaltCheck,
    InputItem, LoopIn, ModelClient, PermMode, SteerQueue, StreamEvent, TurnOutput, Usage,
};
use grokhub_core::mcp_catalog;
use serde_json::{json, Value};

struct Clean(PathBuf);

impl Drop for Clean {
    fn drop(&mut self) {
        mcp::shutdown_all();
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(label: &str) -> (PathBuf, Clean, ConfigGuard) {
    let dir = std::env::temp_dir().join(format!(
        "gh-mcp-chat-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let clean = Clean(dir.clone());
    let guard = ConfigGuard::set(&dir);
    (dir, clean, guard)
}

fn gate_always() -> Gate {
    Gate {
        mode: PermMode::Always,
        readonly_session: false,
        attended: true,
        desktop: false,
    }
}

/// Same shape as [`mcp_catalog::CatalogEntry::config_entry`] for a stdio connector.
#[test]
fn reinstall_treats_legacy_stdio_entries_as_already_there() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (_dir, _clean, _guard) = scratch("idem");
    let legacy = json!({"command": bin, "args": []});
    mcp::install("playwright", &legacy).unwrap();
    let catalog = catalog_stdio_entry(bin, &[]);
    assert_eq!(
        mcp::install("playwright", &catalog).unwrap(),
        Installed::AlreadyThere
    );
}

fn catalog_stdio_entry(command: &str, extra_args: &[&str]) -> Value {
    let catalog = mcp_catalog::bundled();
    let recipe = catalog.get("playwright").expect("playwright");
    let template = recipe.config_entry();
    let mut out = template.as_object().cloned().expect("object");
    out.insert("command".into(), json!(command));
    if extra_args.is_empty() {
        out.remove("args");
    } else {
        out.insert("args".into(), json!(extra_args));
    }
    out.remove("tokenEnv");
    Value::Object(out)
}

fn tool_names(gate: &Gate) -> Vec<String> {
    schemas_for(gate)
        .iter()
        .filter_map(|schema| {
            schema
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .collect()
}

fn import_fake(bin: &str, env: &[(&str, &str)]) {
    let mut env_map = serde_json::Map::new();
    for (key, value) in env {
        env_map.insert((*key).to_string(), json!(value));
    }
    let doc = json!({
        "mcpServers": {
            "box": {
                "command": bin,
                "env": env_map,
                "startup_timeout_sec": 8,
                "tool_timeout_sec": 8
            }
        }
    });
    mcp::import_documents(&[doc.to_string()]).unwrap();
}

struct NoHalt;

impl HaltCheck for NoHalt {
    fn halted(&self) -> bool {
        false
    }
}

struct Once {
    name: String,
    arguments: String,
    turn: AtomicUsize,
}

impl ModelClient for Once {
    fn stream(
        &self,
        _req: &grokhub_agent::ResponsesRequest,
        _cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let n = self.turn.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            return Ok(TurnOutput {
                text: String::new(),
                reasoning: String::new(),
                calls: vec![FunctionCall {
                    call_id: "c1".into(),
                    name: self.name.clone(),
                    arguments: self.arguments.clone(),
                }],
                usage: Usage::default(),
                ..Default::default()
            });
        }
        sink(StreamEvent::TextDelta("done".into()));
        Ok(TurnOutput {
            text: "done".into(),
            reasoning: String::new(),
            calls: Vec::new(),
            usage: Usage::default(),
            ..Default::default()
        })
    }
}

use grokhub_agent::ClientError;

fn run_qualified(dir: &Path, qualified: &str) -> String {
    run_qualified_with(dir, qualified, "{}")
}

/// One chat turn that calls `qualified` with `arguments`; the tool's output.
fn run_qualified_with(dir: &Path, qualified: &str, arguments: &str) -> String {
    let client = Once {
        name: qualified.to_string(),
        arguments: arguments.to_string(),
        turn: AtomicUsize::new(0),
    };
    let cancel = CancelToken::new();
    let steer = SteerQueue::new();
    let halt = NoHalt;
    let permits = ClosedPermits;
    let input = LoopIn {
        client: &client,
        workspace: dir,
        model: "grok-4.7",
        effort: None,
        system: "",
        conversation_id: "mcp-in-chat",
        max_turns: 3,
        usage_base: Usage::default(),
        cancel: &cancel,
        steer: &steer,
        halt: &halt,
        gate: gate_always(),
        desktop: None,
        permits: &permits,
        perms: None,
        context_length: 0,
        tasks: None,
        depth: 0,
        agent_id: None,
        shared_client: None,
        shared_permits: None,
        shared_desktop: None,
    };
    let mut history = Vec::new();
    let _ = run_loop(&input, &mut history, "go", None, &mut |_| {});
    history
        .iter()
        .find_map(|item| match item {
            InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn install_after_warm_mcp_cache_still_lists_tools() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (_dir, _clean, _guard) = scratch("warm");
    let _ = mcp::doctor();
    let entry = catalog_stdio_entry(bin, &[]);
    mcp::install("playwright", &entry).unwrap();
    let names = tool_names(&gate_always());
    assert!(
        names.iter().any(|n| n == "playwright__echo"),
        "tools appear without an explicit shutdown after install: {names:?}"
    );
}

#[test]
fn marketplace_install_surfaces_mock_tools_in_schemas() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (_dir, _clean, _guard) = scratch("schema");
    let entry = catalog_stdio_entry(bin, &["--fixture", "arg"]);
    assert_eq!(
        mcp::install("playwright", &entry).unwrap(),
        Installed::Added,
        "install writes the catalog-shaped entry"
    );

    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(mcp::config_file()).unwrap()).unwrap();
    let spec = &saved["mcpServers"]["playwright"];
    assert_eq!(spec["command"], json!(bin));
    assert_eq!(spec["args"], json!(["--fixture", "arg"]));

    let names = tool_names(&gate_always());
    assert!(
        names.iter().any(|n| n == "playwright__echo"),
        "schemas list the connector tool: {names:?}"
    );

    let rows = mcp::doctor();
    let row = rows.iter().find(|r| r.name == "playwright").expect("row");
    assert_eq!(row.status, "connected");
    assert_eq!(row.tool_count, 1);
}

#[test]
fn marketplace_install_reload_and_deferred_tools_both_offer_mcp() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("defer");

    let entry = catalog_stdio_entry(bin, &[]);
    mcp::install("playwright", &entry).unwrap();
    assert!(tool_names(&gate_always()).contains(&"playwright__echo".to_string()));

    std::fs::write(dir.join("deferred_tools.toml"), "enabled = true\n").unwrap();
    let deferred = tool_names(&gate_always());
    assert!(
        deferred.contains(&"playwright__echo".to_string()),
        "deferred native mode still lists MCP tools: {deferred:?}"
    );

    import_fake(bin, &[("FAKE_MCP_TOOLS", "45")]);
    mcp::shutdown_all();
    mcp::invalidate();
    let many = tool_names(&gate_always());
    assert!(
        many.iter().any(|n| n == "search_tool"),
        "many MCP tools defer to search_tool: {many:?}"
    );
    assert!(!many.iter().any(|n| n == "box__t0"));

    let found = execute(&dir, "shell-a-b", "search_tool", r#"{"query":"widget"}"#);
    assert!(
        found.text.contains("box__t0"),
        "search_tool finds deferred MCP tools: {found:?}"
    );
}

#[test]
fn chat_run_calls_installed_mock_and_fences_the_result() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("run");
    let entry = catalog_stdio_entry(bin, &[]);
    mcp::install("playwright", &entry).unwrap();
    let output = run_qualified(&dir, "playwright__echo");
    assert!(output.contains("pid="), "{output}");
    assert!(
        output.contains("source=\"mcp\"") || output.contains("UNTRUSTED"),
        "MCP output is fenced as untrusted: {output}"
    );
}

#[test]
fn disconnect_drops_installed_tools_from_schemas() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (_dir, _clean, _guard) = scratch("drop");
    mcp::install("playwright", &catalog_stdio_entry(bin, &[])).unwrap();
    assert!(tool_names(&gate_always()).contains(&"playwright__echo".to_string()));
    mcp::disconnect("playwright").unwrap();
    mcp::shutdown_all();
    let names = tool_names(&gate_always());
    assert!(!names.iter().any(|n| n.contains("playwright__")));
}

#[test]
fn missing_stdio_command_is_a_named_error_not_a_panic() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("bad");
    mcp::install("playwright", &catalog_stdio_entry(bin, &[])).unwrap();
    mcp::disconnect("playwright").unwrap();

    let bad = json!({"command": "/no/such/grokhub-mock-mcp-binary"});
    mcp::install("playwright", &bad).unwrap();
    let rows = mcp::doctor();
    let row = rows.iter().find(|r| r.name == "playwright").unwrap();
    assert_eq!(row.status, "error");
    assert!(!row.last_error.is_empty());

    let output = run_qualified(&dir, "playwright__echo");
    assert!(!output.contains("pid="));
    assert!(!output.is_empty() || rows[0].last_error.contains("MCP"));
}

#[test]
fn legacy_argv_array_install_lists_tools() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (_dir, _clean, _guard) = scratch("argv");
    let entry = json!({
        "type": "stdio",
        "command": [bin, "--fixture", "from-array"]
    });
    mcp::install("playwright", &entry).unwrap();
    let names = tool_names(&gate_always());
    assert!(
        names.iter().any(|n| n == "playwright__echo"),
        "argv-array connector entries load in chat: {names:?}"
    );
}

#[test]
fn brave_catalog_config_entry_keeps_args_and_token_env() {
    let catalog = mcp_catalog::bundled();
    let brave = catalog.get("brave-search").expect("brave");
    let entry = brave.config_entry();
    assert_eq!(entry["type"], json!("stdio"));
    assert_eq!(entry["command"], json!("npx"));
    assert_eq!(
        entry["args"],
        json!([
            "-y",
            "@brave/brave-search-mcp-server",
            "--transport",
            "stdio"
        ])
    );
    assert_eq!(entry["tokenEnv"], json!("BRAVE_API_KEY"));
}

/// Imports a Playwright entry `name` whose browser dies at launch (the fake
/// does that until it is started `--isolated`). `--browser` skips detection.
fn import_dying_browser(name: &str, bin: &str, dies: &str) {
    let doc = json!({
        "mcpServers": {
            name: {
                "command": bin,
                "args": ["@playwright/mcp@latest", "--browser=chromium"],
                "env": {"FAKE_MCP_BROWSER_DIES": dies},
            }
        }
    });
    mcp::import_documents(&[doc.to_string()]).unwrap();
}

#[test]
fn a_browser_that_dies_at_launch_is_retried_isolated_once() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("pw-dies");
    import_dying_browser("pwdies", bin, "1");
    let out = run_qualified(&dir, "pwdies__echo");
    assert!(out.contains("pid="), "the isolated relaunch answers: {out}");
}

#[test]
fn a_browser_that_keeps_dying_points_the_model_at_web_fetch() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("pw-always");
    import_dying_browser("pwalways", bin, "always");
    let out = run_qualified(&dir, "pwalways__echo");
    assert!(
        out.contains("The browser can't start on this computer"),
        "{out}"
    );
    assert!(out.contains("web_fetch"), "{out}");
    assert!(out.contains("has been closed"), "{out}");
}

/// Imports a Playwright entry `name` whose every answer shows a page on `url`
/// after `pad` characters, with the user's own browser attached and only
/// github.com allowed.
fn import_attached_browser(dir: &Path, name: &str, bin: &str, url: &str, pad: usize) {
    let attach = json!({ "endpoint": "http://127.0.0.1:9222", "origins": ["https://github.com"] });
    std::fs::create_dir_all(dir.join("browser")).unwrap();
    std::fs::write(dir.join("browser").join("attach.json"), attach.to_string()).unwrap();
    let doc = json!({
        "mcpServers": {
            name: {
                "command": bin,
                "args": ["@playwright/mcp@latest"],
                "env": {"FAKE_MCP_PAGE_URL": url, "FAKE_MCP_PAGE_PAD": pad.to_string()},
            }
        }
    });
    mcp::import_documents(&[doc.to_string()]).unwrap();
}

#[test]
fn an_attached_browser_answer_from_another_site_is_held_back() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("pw-attached");
    import_attached_browser(&dir, "pwattach", bin, "https://bank.example/accounts", 0);
    // The server answers on another site: the page text never reaches the model.
    let new_tab = r#"{"action":"new","url":"https://github.com/"}"#;
    let out = run_qualified_with(&dir, "pwattach__browser_tabs", new_tab);
    assert!(
        out.contains("The tab is on https://bank.example, which isn't one of the sites"),
        "{out}"
    );
    assert!(!out.contains("account 1234"), "{out}");
    // A new tab on a site that isn't allowed never reaches the server.
    let other = r#"{"action":"new","url":"https://bank.example/"}"#;
    let out = run_qualified_with(&dir, "pwattach__browser_tabs", other);
    assert!(
        out.contains("https://bank.example isn't one of them"),
        "{out}"
    );
}

#[test]
fn an_attached_browser_page_past_the_answer_cut_is_still_held_back() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("pw-attached-long");
    // The page section starts after 50,000 characters, past where the
    // answer is cut for the model.
    import_attached_browser(&dir, "pwlong", bin, "https://bank.example/accounts", 50_000);
    let new_tab = r#"{"action":"new","url":"https://github.com/"}"#;
    let out = run_qualified_with(&dir, "pwlong__browser_tabs", new_tab);
    assert!(
        out.contains("The tab is on https://bank.example, which isn't one of the sites"),
        "{}",
        &out[..out.len().min(300)]
    );
    assert!(!out.contains("account 1234"));
    assert!(!out.contains("xxxx"));
}

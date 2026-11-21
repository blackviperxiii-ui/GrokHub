//! The headless tools run as a real `grokhub-hub` process.

use std::io::Write;
use std::process::{Command, Stdio};

fn temp_config(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("grokhub-hub-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn mcp_self_answers_initialize_and_stdout_is_json_rpc_only() {
    let dir = temp_config("mcp-self");
    let mut child = Command::new(env!("CARGO_BIN_EXE_grokhub-hub"))
        .arg("--mcp-self")
        .env("GROKHUB_CONFIG", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}}}}}}"#
        )
        .unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"ping"}}"#).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {l}: {e}")))
        .collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "grokhub-self");
    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(
        lines[1],
        serde_json::json!({"jsonrpc":"2.0","id":2,"result":{}})
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn askpass_without_a_listening_socket_exits_1_and_prints_nothing() {
    let dir = temp_config("askpass");
    let out = Command::new(env!("CARGO_BIN_EXE_grokhub-hub"))
        .args([
            "--askpass",
            dir.join("no-such-socket").to_str().unwrap(),
            "[sudo] password: ",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn version_prints_the_workspace_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_grokhub-hub"))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{}\n", env!("CARGO_PKG_VERSION"))
    );
}

fn rpc(
    args: &[&str],
    dir: &std::path::Path,
    input: &[&str],
) -> (Option<i32>, Vec<serde_json::Value>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_grokhub-hub"))
        .args(args)
        .env("GROKHUB_CONFIG", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for line in input {
            writeln!(stdin, "{line}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let lines = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {l}: {e}")))
        .collect();
    (out.status.code(), lines)
}

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}"#;

#[test]
fn mcp_bridge_answers_initialize_and_denies_sealed_memory() {
    let dir = temp_config("mcp-bridge");
    std::fs::create_dir_all(dir.join("amr/nodes")).unwrap();
    std::fs::write(
        dir.join("amr/nodes/fact-dock.md"),
        "---\nid: fact-dock\ntype: fact\ncreated: 2026-11-19T00:00:00Z\nupdated: 2026-11-19T00:00:00Z\nsource: user\nconfidence: 0.9\ntags: [dock]\n---\nDock layout is plain.\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("amr/nodes/fact-secret.sealed"),
        "gh-sealed:v1:not-readable-by-bridge\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("automations.json"),
        r#"[{"id":"a1","name":"Inbox","schedule":"heartbeat","time":"","heartbeatEveryMin":15,"instructions":"check","enabled":true,"runCount":1,"health":{"outcome":"ok"}}]"#,
    )
    .unwrap();
    let (code, lines) = rpc(
        &["--mcp-bridge"],
        &dir,
        &[
            INIT,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"dock"}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"fact-secret"}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"automation_status","arguments":{}}}"#,
        ],
    );
    assert_eq!(code, Some(0));
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "grokhub-bridge");
    let dock = lines[1]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(dock.contains("fact-dock"), "{dock}");
    assert_eq!(lines[1]["result"]["isError"], false);
    assert_eq!(
        lines[2]["result"]["content"][0]["text"],
        "Sealed memory nodes are not available over the MCP bridge."
    );
    assert_eq!(lines[2]["result"]["isError"], true);
    let status = lines[3]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(status.contains("a1\ton\tevery 15 min\tok"), "{status}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mcp_desktop_answers_initialize_and_refuses_while_the_switch_is_off() {
    let dir = temp_config("mcp-desktop");
    let (code, lines) = rpc(
        &["--mcp-desktop"],
        &dir,
        &[
            INIT,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"click","arguments":{"x":1,"y":1}}}"#,
        ],
    );
    assert_eq!(code, Some(0));
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "grokhub-desktop");
    assert_eq!(
        lines[0]["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(lines[1]["id"], 2);
    let reply = lines[1].to_string();
    assert!(reply.contains("Desktop control is off."), "{reply}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
fn tighten_doctor_fixture(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let secrets = dir.join("secrets.json");
    if secrets.is_file() {
        std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn doctor_with_a_saved_key_and_a_live_hub_exits_0() {
    let dir = temp_config("doctor");
    std::fs::write(dir.join("secrets.json"), r#"{"apiKey":"xai-test"}"#).unwrap();
    #[cfg(unix)]
    tighten_doctor_fixture(&dir);
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut hub = Command::new(env!("CARGO_BIN_EXE_grokhub-hub"))
        .env("GROKHUB_CONFIG", &dir)
        .env("GROKHUB_HUB_PORT", port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let doctor = || {
        Command::new(env!("CARGO_BIN_EXE_grokhub-hub"))
            .arg("--doctor")
            .env("GROKHUB_CONFIG", &dir)
            .env("GROKHUB_HUB_PORT", port.to_string())
            .output()
            .unwrap()
    };
    let mut out = doctor();
    for _ in 0..50 {
        if out.status.code() == Some(0) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        out = doctor();
    }
    let _ = hub.kill();
    let _ = hub.wait();
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.starts_with("ok  xAI auth present\nok  memory dir readable\nok  hub kind "),
        "{text}"
    );
    assert!(dir.join("memory").is_dir());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn doctor_fix_tightens_perms_then_passes_the_perm_rows() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_config("doctor-fix");
    std::fs::write(dir.join("secrets.json"), r#"{"apiKey":"xai-test"}"#).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(
        dir.join("secrets.json"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let before = Command::new(env!("CARGO_BIN_EXE_grokhub-hub"))
        .arg("--doctor")
        .env("GROKHUB_CONFIG", &dir)
        .env("GROKHUB_HUB_PORT", port.to_string())
        .output()
        .unwrap();
    let before_text = String::from_utf8(before.stdout).unwrap();
    assert!(
        before_text.contains("ERR secrets.json mode"),
        "{before_text}"
    );
    let after = Command::new(env!("CARGO_BIN_EXE_grokhub-hub"))
        .args(["--doctor", "--fix"])
        .env("GROKHUB_CONFIG", &dir)
        .env("GROKHUB_HUB_PORT", port.to_string())
        .output()
        .unwrap();
    let after_text = String::from_utf8(after.stdout).unwrap();
    assert!(
        after_text.contains("ok  secrets.json mode 0600"),
        "{after_text}"
    );
    assert!(
        after_text.contains("ok  config dir mode 0700"),
        "{after_text}"
    );
    let mode = std::fs::metadata(dir.join("secrets.json"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn doctor_with_no_hub_reports_it_unreachable_and_exits_1() {
    let dir = temp_config("doctor-nohub");
    std::fs::write(dir.join("secrets.json"), r#"{"apiKey":"xai-test"}"#).unwrap();
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let out = Command::new(env!("CARGO_BIN_EXE_grokhub-hub"))
        .arg("--doctor")
        .env("GROKHUB_CONFIG", &dir)
        .env("GROKHUB_HUB_PORT", port.to_string())
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("\nERR hub kind unreachable\n"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_explains_and_runs_nothing() {
    let dir = temp_config("update");
    let out = Command::new(env!("CARGO_BIN_EXE_grokhub-hub"))
        .arg("--update")
        .env("GROKHUB_CONFIG", &dir)
        .env("APPIMAGE", "/tmp/Loadout.AppImage")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "Open Loadout → Settings → Updates to update this install.\n"
    );
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "--update wrote nothing"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

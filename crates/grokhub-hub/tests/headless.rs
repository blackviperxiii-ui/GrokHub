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

#[test]
fn doctor_with_a_saved_key_and_a_live_hub_exits_0() {
    let dir = temp_config("doctor");
    std::fs::write(dir.join("secrets.json"), r#"{"apiKey":"xai-test"}"#).unwrap();
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

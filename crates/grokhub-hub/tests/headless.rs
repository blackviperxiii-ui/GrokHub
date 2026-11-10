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

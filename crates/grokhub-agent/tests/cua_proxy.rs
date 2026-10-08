//! Spike-2a: `grokhub --mcp-cua` against a fake Cua child process. Never
//! launches a real `cua-driver` and makes no network calls.

use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_agent::harness::{self as hx, CuaGate, CuaProxy, StdioChild};
use serde_json::{json, Value};

const ON: CuaGate = CuaGate { enabled: true, flag: true, halted: false };

fn scratch(label: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cua-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    hx::write_turn_context(&dir, &hx::TurnContext { chat_id: "chat-p".into(), turn: 1, access: "supervised".into(), episode: String::new() }).unwrap();
    dir
}

fn fake() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_cua"))
}

fn call(tool: &str, args: Value) -> String {
    json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":tool,"arguments":args}}).to_string()
}

/// Lines the child logged: its argv/env line, then what it received.
fn child_log(dir: &Path) -> Vec<Value> {
    let path = format!("{}.log", hx::cua_socket_path(dir).display());
    for _ in 0..200 {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if !text.is_empty() {
                return text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Vec::new()
}

#[test]
fn fake_child_gets_the_bounded_env_one_soft_click_and_nothing_after_a_halt() {
    let dir = scratch("proxy");
    assert_eq!(hx::verify_cua_driver(&fake()), Ok(()));
    let bin = fake();
    let child_dir = dir.clone();
    let mut proxy: CuaProxy<StdioChild> = CuaProxy::new(Box::new(move || hx::spawn_cua_child(&bin, &child_dir)));

    let out = proxy.handle_line(&call("click", json!({"x":10,"y":20})), ON, &dir, &mut || false).unwrap();
    assert!(out.contains("click done"), "{out}");
    // A session-ending key parks; Halt while parked denies it.
    let out = proxy.handle_line(&call("hotkey", json!({"keys":["ctrl","alt","backspace"]})), ON, &dir, &mut || true).unwrap();
    assert!(out.contains("Denied: hard-class irreversible_os"), "{out}");
    let out = proxy.handle_line(&call("type_text", json!({"text":"hello"})), ON, &dir, &mut || false).unwrap();
    assert!(out.contains(grokhub_core::desktop_mcp::HALT_MSG), "{out}");

    let log = child_log(&dir);
    let head = &log[0];
    let manifest = hx::cua_manifest_path(&dir).display().to_string();
    assert_eq!(head["argv"], json!(["mcp", "--socket", hx::cua_socket_path(&dir).display().to_string()]));
    assert_eq!(
        head["env"],
        json!({
            "CUA_DRIVER_PERMISSION_MODE": "bounded",
            "CUA_DRIVER_CAPABILITY_MANIFEST_FILE": manifest,
            "CUA_DRIVER_CAPABILITY_MANIFEST_APPROVED": "1",
        })
    );
    assert_eq!(std::fs::read_to_string(&manifest).unwrap(), hx::cua_manifest());
    let tools: Vec<&str> = log[1..].iter().map(|m| m["params"]["name"].as_str().unwrap_or("")).collect();
    assert_eq!(tools, vec!["get_window_state", "click", "get_window_state"], "one click, nothing after the halt");

    let spans = hx::read_spans(&dir, "chat-p").unwrap();
    let got: Vec<_> = spans.iter().map(|s| (s.tool.as_str(), s.decision.as_str(), s.driver.as_str())).collect();
    assert_eq!(
        got,
        vec![("click", "allow", "cua"), ("key", "park", "cua"), ("key", "deny", "cua"), ("type", "deny", "cua")]
    );
    assert_eq!(spans[3].args_redacted, r#"{"chars":5}"#);
    drop(proxy);
    let _ = std::fs::remove_dir_all(dir);
}

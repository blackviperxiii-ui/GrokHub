//! Fake `cua-driver` for tests. Never drives a screen.
//!
//! `--version` prints the pinned version. `serve --socket <path>` writes
//! `<path>.serve.log` (argv and env), creates `<path>` as a ready marker,
//! and parks until killed. `mcp --socket <path>` speaks newline-delimited
//! JSON-RPC on stdio and writes `<path>.log`: first its argv and
//! `CUA_DRIVER_*` env, then every line it received.

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("cua-driver 0.34.0");
        return;
    }
    let Some(socket) = args
        .iter()
        .position(|a| a == "--socket")
        .and_then(|i| args.get(i + 1))
        .cloned()
    else {
        std::process::exit(2);
    };
    match args.first().map(String::as_str) {
        Some("serve") => run_serve(&socket, &args),
        Some("mcp") => run_mcp(&socket, &args),
        _ => std::process::exit(2),
    }
}

fn screen_env() -> serde_json::Map<String, Value> {
    std::env::vars()
        .filter(|(k, _)| {
            [
                "DISPLAY",
                "XAUTHORITY",
                "WAYLAND_DISPLAY",
                "XDG_SESSION_TYPE",
            ]
            .contains(&k.as_str())
        })
        .map(|(k, v)| (k, Value::String(v)))
        .collect()
}

fn cua_env() -> serde_json::Map<String, Value> {
    std::env::vars()
        .filter(|(k, _)| k.starts_with("CUA_DRIVER_"))
        .map(|(k, v)| (k, Value::String(v)))
        .collect()
}

fn run_serve(socket: &str, args: &[String]) {
    let mut log = std::fs::File::create(format!("{socket}.serve.log")).expect("serve log");
    let _ = writeln!(
        log,
        "{}",
        json!({ "argv": args, "env": cua_env(), "screen": screen_env() })
    );
    let _ = log.flush();
    // Ready marker so the parent can start `mcp --socket` (real serve makes
    // a Unix socket here; an empty file is enough for the fake).
    std::fs::File::create(socket).expect("socket ready");
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn run_mcp(socket: &str, args: &[String]) {
    let mut log = std::fs::File::create(format!("{socket}.log")).expect("log");
    let _ = writeln!(
        log,
        "{}",
        json!({ "argv": args, "env": cua_env(), "screen": screen_env() })
    );
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let _ = writeln!(log, "{line}");
        let _ = log.flush();
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let name = msg["params"]["name"].as_str().unwrap_or("");
        let reply = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "content": [{ "type": "text", "text": format!("{name} done") }], "isError": false },
        });
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
}

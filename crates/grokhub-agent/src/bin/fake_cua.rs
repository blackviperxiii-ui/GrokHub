//! Fake `cua-driver` for tests. Never drives a screen.
//!
//! `--version` prints the pinned version. `mcp --socket <path>` speaks
//! newline-delimited JSON-RPC on stdio and writes `<path>.log`: first its
//! argv and `CUA_DRIVER_*` env, then every line it received.

use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("cua-driver 0.34.0");
        return;
    }
    let Some(socket) = args.iter().position(|a| a == "--socket").and_then(|i| args.get(i + 1)) else {
        std::process::exit(2);
    };
    let mut log = std::fs::File::create(format!("{socket}.log")).expect("log");
    let env: serde_json::Map<String, Value> = std::env::vars()
        .filter(|(k, _)| k.starts_with("CUA_DRIVER_"))
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    let _ = writeln!(log, "{}", json!({ "argv": args, "env": env }));
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let _ = writeln!(log, "{line}");
        let _ = log.flush();
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = msg.get("id").cloned() else { continue };
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

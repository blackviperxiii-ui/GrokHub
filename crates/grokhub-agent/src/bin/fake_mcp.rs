//! Tiny stdio MCP server for tests. Speaks newline-delimited JSON-RPC.

use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn main() {
    if std::env::args().any(|arg| arg == "--sleep") {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
    spawn_child();
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    while let Some(msg) = read_msg(&mut reader) {
        let Some(method) = msg.get("method").and_then(|item| item.as_str()) else {
            continue;
        };
        if method.starts_with("notifications/") {
            continue;
        }
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        match method {
            "initialize" => {
                respond(
                    &id,
                    json!({
                        "protocolVersion": "2025-03-26",
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "fake", "version": "0"}
                    }),
                );
                if std::env::var("FAKE_MCP_CRASH").ok().as_deref() == Some("1") {
                    std::process::exit(0);
                }
            }
            "tools/list" => respond(&id, tools_page(msg.get("params"))),
            "tools/call" => respond(&id, call_tool(&mut reader, msg.get("params"))),
            "ping" => respond(&id, json!({})),
            _ => send(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "method not supported"}
            })),
        }
    }
}

fn spawn_child() {
    if std::env::var("FAKE_MCP_CHILD").ok().as_deref() != Some("1") {
        return;
    }
    let Ok(path) = std::env::var("FAKE_MCP_CHILD_FILE") else {
        return;
    };
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Ok(child) = std::process::Command::new(exe).arg("--sleep").spawn() else {
        return;
    };
    let _ = std::fs::write(path, child.id().to_string());
    std::mem::forget(child);
}

fn tools_page(params: Option<&Value>) -> Value {
    if std::env::var("FAKE_MCP_PAGE").ok().as_deref() == Some("1") {
        let cursor = params
            .and_then(|item| item.get("cursor"))
            .and_then(|item| item.as_str())
            .unwrap_or("");
        if cursor.is_empty() {
            return json!({
                "tools": [tool("echo", "Echo arguments")],
                "nextCursor": "2"
            });
        }
        return json!({ "tools": [tool("page2", "Second page")] });
    }
    json!({ "tools": tool_list() })
}

fn tool_list() -> Vec<Value> {
    if let Ok(raw) = std::env::var("FAKE_MCP_TOOLS") {
        let count: usize = raw.parse().unwrap_or(0);
        if count > 0 {
            return (0..count)
                .map(|idx| {
                    let description = if idx == 0 {
                        "widget alpha"
                    } else {
                        "generated"
                    };
                    tool(&format!("t{idx}"), description)
                })
                .collect();
        }
    }
    vec![tool("echo", "Echo arguments")]
}

fn tool(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": {}}
    })
}

fn call_tool(reader: &mut impl BufRead, params: Option<&Value>) -> Value {
    let args = params
        .and_then(|item| item.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    if std::env::var("FAKE_MCP_ELICIT").ok().as_deref() == Some("1") {
        let elicit_id = json!(9001);
        send(&json!({
            "jsonrpc": "2.0",
            "id": elicit_id,
            "method": "elicitation/create",
            "params": {
                "message": "Need a value",
                "mode": "form",
                "requestedSchema": {
                    "type": "object",
                    "properties": {"note": {"type": "string", "title": "Note"}},
                    "required": ["note"]
                }
            }
        }));
        let answer = read_until(reader, &elicit_id);
        let action = answer
            .get("result")
            .and_then(|item| item.get("action"))
            .and_then(|item| item.as_str())
            .unwrap_or("decline");
        let content = answer
            .get("result")
            .and_then(|item| item.get("content"))
            .cloned()
            .unwrap_or(Value::Null);
        return text_result(&format!("action={action} content={content}"));
    }
    text_result(&format!("pid={} args={args}", std::process::id()))
}

fn text_result(text: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}

fn read_until(reader: &mut impl BufRead, id: &Value) -> Value {
    loop {
        let Some(msg) = read_msg(reader) else {
            return json!({});
        };
        if msg.get("id") == Some(id) && msg.get("method").is_none() {
            return msg;
        }
    }
}

fn read_msg(reader: &mut impl BufRead) -> Option<Value> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).ok()?;
        if n == 0 {
            return None;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        return serde_json::from_str(trimmed).ok();
    }
}

fn respond(id: &Value, result: Value) {
    send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
}

fn send(msg: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, msg);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

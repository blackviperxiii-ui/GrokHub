//! Fake Grok Build ACP agent for tests.

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn write_json(v: &Value) {
    let mut out = io::stdout();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn result(id: &Value, body: Value) {
    write_json(&json!({ "jsonrpc": "2.0", "id": id, "result": body }));
}

fn notify(method: &str, params: Value) {
    write_json(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
}

fn main() {
    let thought = std::env::var("FAKE_ACP_THOUGHT").unwrap_or_else(|_| "thinking".into());
    let text = std::env::var("FAKE_ACP_TEXT").unwrap_or_else(|_| "hello from grok build".into());
    let tool = std::env::var("FAKE_ACP_TOOL").unwrap_or_default();
    let want_perm = std::env::var("FAKE_ACP_PERMISSION").ok().as_deref() == Some("1");
    let image = std::env::var("FAKE_ACP_IMAGE").unwrap_or_default();

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        match method {
            "initialize" => {
                if let Some(id) = id {
                    result(
                        &id,
                        json!({
                            "protocolVersion": 1,
                            "authMethods": [
                                { "id": "cached_token", "name": "cached" },
                                { "id": "xai.api_key", "name": "key" }
                            ],
                            "agentCapabilities": {}
                        }),
                    );
                }
            }
            "authenticate" => {
                if let Some(id) = id {
                    result(&id, json!({}));
                }
            }
            "session/new" => {
                if let Some(id) = id {
                    result(&id, json!({ "sessionId": "sess-test" }));
                }
            }
            "session/load" => {
                if std::env::var("FAKE_ACP_LOAD_FAIL").ok().as_deref() == Some("1") {
                    if let Some(id) = id {
                        write_json(&json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32000, "message": "session not found" }
                        }));
                    }
                    continue;
                }
                let sid = msg
                    .get("params")
                    .and_then(|p| p.get("sessionId").or_else(|| p.get("session_id")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("sess-test")
                    .to_string();
                if std::env::var("FAKE_ACP_LOAD_PERM_FIRST").ok().as_deref() == Some("1") {
                    write_json(&json!({
                        "jsonrpc": "2.0",
                        "id": 9001,
                        "method": "session/request_permission",
                        "params": {
                            "sessionId": sid,
                            "toolCall": { "toolCallId": "load-perm-first", "title": "LOAD_REPLAY_SHOULD_NOT_PAINT" }
                        }
                    }));
                }
                notify(
                    "session/update",
                    json!({
                        "sessionId": sid,
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "text": "LOAD_REPLAY_SHOULD_NOT_PAINT" }
                        }
                    }),
                );
                if let Some(id) = id {
                    result(&id, json!({ "sessionId": sid }));
                }
                write_json(&json!({
                    "jsonrpc": "2.0",
                    "id": 9002,
                    "method": "session/request_permission",
                    "params": {
                        "sessionId": sid,
                        "toolCall": { "toolCallId": "load-perm", "title": "LOAD_REPLAY_SHOULD_NOT_PAINT" }
                    }
                }));
                notify(
                    "session/update",
                    json!({
                        "sessionId": sid,
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "text": "LOAD_REPLAY_SHOULD_NOT_PAINT" }
                        }
                    }),
                );
            }
            "session/cancel" => {
                if let Some(id) = id {
                    result(&id, json!({ "stopReason": "cancelled" }));
                }
            }
            "session/prompt" => {
                notify(
                    "session/update",
                    json!({
                        "sessionId": "sess-test",
                        "update": {
                            "sessionUpdate": "agent_thought_chunk",
                            "content": { "text": thought }
                        }
                    }),
                );
                if !tool.is_empty() {
                    let mut content = vec![json!({ "type": "text", "text": "running" })];
                    if !image.is_empty() {
                        content.push(json!({
                            "type": "image",
                            "mimeType": "image/jpeg",
                            "data": image
                        }));
                    }
                    notify(
                        "session/update",
                        json!({
                            "sessionId": "sess-test",
                            "update": {
                                "sessionUpdate": "tool_call",
                                "toolCallId": "tool-1",
                                "title": tool,
                                "kind": "other",
                                "status": "completed",
                                "content": content
                            }
                        }),
                    );
                }
                if want_perm {
                    write_json(&json!({
                        "jsonrpc": "2.0",
                        "id": 9001,
                        "method": "session/request_permission",
                        "params": {
                            "sessionId": "sess-test",
                            "toolCall": { "toolCallId": "tool-1", "title": "bash" }
                        }
                    }));
                    // wait for the client's permission reply before finishing
                    continue;
                }
                notify(
                    "session/update",
                    json!({
                        "sessionId": "sess-test",
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "text": text }
                        }
                    }),
                );
                if let Some(id) = id {
                    result(&id, json!({ "stopReason": "end_turn" }));
                }
            }
            _ => {
                if msg.get("result").is_some() && want_perm {
                    notify(
                        "session/update",
                        json!({
                            "sessionId": "sess-test",
                            "update": {
                                "sessionUpdate": "agent_message_chunk",
                                "content": { "text": text }
                            }
                        }),
                    );
                    write_json(&json!({
                        "jsonrpc": "2.0",
                        "id": 2,
                        "result": { "stopReason": "end_turn" }
                    }));
                }
            }
        }
    }
}

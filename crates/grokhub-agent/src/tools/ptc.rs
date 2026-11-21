//! Programmatic Tool Calling (PTC) sandboxed script runner (`run_tool_script`).
//!
//! Runs a short deterministic Python 3 script inside Loadout's default-on OS
//! sandbox (`crate::sandbox`) with a stdio JSON-RPC bridge limited strictly to
//! allowlisted read-only workspace tools. Mutating or privileged actions are
//! never reachable from PTC and must still go through the normal harness
//! (`HardClass`, `ApprovalGate`, `file_checkpoint`).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{ToolCtx, ToolOutput};

pub const NAME: &str = "run_tool_script";
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const MAX_TIMEOUT_MS: u64 = 60_000;
pub const MAX_SCRIPT_BYTES: usize = 32 * 1024;
pub const MAX_TOOL_CALLS: usize = 32;
pub const MAX_OUTPUT_CHARS: usize = 16_000;
const MEMORY_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(20);

/// Strict allowlist of read-only tools reachable over the PTC RPC bridge.
/// Mutating, shell, desktop, and network tools are intentionally excluded.
pub const ALLOWED_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "grep",
    "glob",
    "skill",
    "diagnose",
    "scheduler_list",
    "local_ai_status",
];

pub fn is_allowed_tool(name: &str) -> bool {
    ALLOWED_TOOLS.contains(&name)
}

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": NAME,
        "description": "Run a short read-only Python 3 script inside the OS sandbox to chain multiple read/search/compute tool calls in one turn (helpers: call_tool(name, args), read_file(path), list_dir(path), grep(pattern, path), glob(pattern, path)). Only read-only allowlisted tools are available; print the compact final result.",
        "parameters": {
            "type": "object",
            "properties": {
                "script": {
                    "type": "string",
                    "description": "Python 3 script body (max 32 KB). Use print(...) to emit the compact result."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds (default 30000, max 60000)."
                }
            },
            "required": ["script"],
            "additionalProperties": false
        }
    })
}

const PYTHON_HARNESS: &str = r#"
import io, json, os, sys, traceback

_rpc_in = sys.stdin
_rpc_out = sys.stdout

try:
    import resource
    _limit = int(os.environ.get("LOADOUT_PTC_MEM_BYTES", "536870912"))
    _soft, _hard = resource.getrlimit(resource.RLIMIT_AS)
    if _hard == resource.RLIM_INFINITY or _hard >= _limit:
        resource.setrlimit(resource.RLIMIT_AS, (_limit, _hard))
except Exception:
    pass

def call_tool(name, args=None):
    payload = {"op": "call", "name": str(name), "args": args if isinstance(args, dict) else {}}
    _rpc_out.write(json.dumps(payload) + "\n")
    _rpc_out.flush()
    line = _rpc_in.readline()
    if not line:
        raise RuntimeError("PTC RPC bridge closed")
    resp = json.loads(line)
    if not resp.get("ok"):
        raise RuntimeError(resp.get("error") or resp.get("text") or "tool call failed")
    return resp.get("text", "")

def read_file(path, offset=None, limit=None):
    a = {"target_file": path}
    if offset is not None:
        a["offset"] = offset
    if limit is not None:
        a["limit"] = limit
    return call_tool("read_file", a)

def list_dir(path="."):
    return call_tool("list_dir", {"path": path})

def grep(pattern, path="."):
    return call_tool("grep", {"pattern": pattern, "path": path})

def glob(pattern, path="."):
    return call_tool("glob", {"pattern": pattern, "path": path})

_init_line = _rpc_in.readline()
_init = json.loads(_init_line) if _init_line else {}
_code = _init.get("script", "")

_buf = io.StringIO()
sys.stdout = _buf
_err = None
try:
    _ns = {
        "__name__": "__main__",
        "call_tool": call_tool,
        "read_file": read_file,
        "list_dir": list_dir,
        "grep": grep,
        "glob": glob,
    }
    exec(compile(_code, "<ptc>", "exec"), _ns, _ns)
except BaseException as _e:
    if isinstance(_e, SystemExit) and (_e.code is None or _e.code == 0):
        pass
    else:
        _err = "".join(traceback.format_exception(type(_e), _e, _e.__traceback__))
finally:
    sys.stdout = _rpc_out

_rpc_out.write(json.dumps({"op": "done", "stdout": _buf.getvalue(), "error": _err}) + "\n")
_rpc_out.flush()
"#;

fn python_program() -> &'static str {
    #[cfg(windows)]
    {
        "python"
    }
    #[cfg(not(windows))]
    {
        "python3"
    }
}

fn timeout_from(args: &Value) -> Duration {
    let ms = args
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .min(MAX_TIMEOUT_MS);
    Duration::from_millis(ms)
}

fn clip_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max).collect();
    format!(
        "{kept}\n\n... (PTC output truncated: {} chars omitted) ...",
        count - max
    )
}

fn spawn_sandboxed_python(cwd: &Path) -> Result<Child, String> {
    let mut plan = crate::sandbox::plan_for_call("run_terminal_command", &json!({}), cwd);
    // PTC scripts are read-only; remove all writable filesystem roots under the OS sandbox.
    // The harness runs on this computer, so it keeps the host sandbox even
    // when the shell's commands go to a container.
    if let Some(p) = plan.as_mut() {
        p.writable.clear();
        p.container = None;
    }
    let py = python_program();
    let argv = [py, "-I", "-B", "-u", "-c", PYTHON_HARNESS];

    #[cfg(target_os = "macos")]
    let mut cmd = match plan
        .as_ref()
        .and_then(|p| p.isolation.as_ref().map(|iso| (p, iso)))
    {
        Some((plan, iso)) => {
            let port = plan.proxy_port().filter(|_| iso.net_confined);
            let profile = crate::sandbox::seatbelt::profile(&plan.writable, &iso.hide, port);
            let (program, args) = crate::sandbox::seatbelt::wrap(&profile, &argv);
            let mut cmd = Command::new(program);
            cmd.args(args);
            cmd
        }
        None => {
            let mut cmd = Command::new(argv[0]);
            cmd.args(&argv[1..]);
            cmd
        }
    };

    #[cfg(not(target_os = "macos"))]
    let mut cmd = {
        let mut cmd = Command::new(argv[0]);
        cmd.args(&argv[1..]);
        cmd
    };

    cmd.current_dir(cwd)
        .env("LOADOUT_PTC_MEM_BYTES", MEMORY_LIMIT_BYTES.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    if let Some(ref plan) = plan {
        crate::sandbox::apply(&mut cmd, plan)?;
    }

    cmd.spawn()
        .map_err(|e| format!("could not start {py} for PTC script: {e}"))
}

pub fn run(ctx: &ToolCtx<'_>, args: &Value) -> ToolOutput {
    let script = args
        .get("script")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if script.is_empty() {
        return ToolOutput::err("script is required");
    }
    if script.len() > MAX_SCRIPT_BYTES {
        return ToolOutput::err(format!(
            "PTC script is {} bytes, over the {} byte limit",
            script.len(),
            MAX_SCRIPT_BYTES
        ));
    }
    let cwd = match ctx.workspace.canonicalize() {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(format!("workspace is not available: {e}")),
    };
    let timeout = timeout_from(args);
    let mut child = match spawn_sandboxed_python(&cwd) {
        Ok(c) => c,
        Err(e) => return ToolOutput::err(e),
    };

    let mut stdin = match child.stdin.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return ToolOutput::err("PTC stdin pipe unavailable");
        }
    };
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return ToolOutput::err("PTC stdout pipe unavailable");
        }
    };
    let stderr = child.stderr.take();

    // Send the script payload over stdin so we never hit OS argv length limits.
    let init_line = format!("{}\n", json!({ "script": script }));
    if let Err(e) = stdin
        .write_all(init_line.as_bytes())
        .and_then(|_| stdin.flush())
    {
        let _ = child.kill();
        let _ = child.wait();
        return ToolOutput::err(format!("failed to send PTC script: {e}"));
    }

    let (line_tx, line_rx) = mpsc::channel::<Result<String, String>>();
    let reader_handle = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if line_tx.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = line_tx.send(Err(e.to_string()));
                    break;
                }
            }
        }
    });

    let stderr_handle = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut se) = stderr {
            let _ = se.read_to_string(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + timeout;
    let mut call_count = 0usize;
    let mut final_out: Option<ToolOutput> = None;

    loop {
        if (ctx.stop)() {
            let _ = child.kill();
            let _ = child.wait();
            drop(stdin);
            let _ = reader_handle.join();
            let _ = stderr_handle.join();
            return ToolOutput::err(crate::gate::user_cancelled(NAME));
        }
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            drop(stdin);
            let _ = reader_handle.join();
            let _ = stderr_handle.join();
            return ToolOutput::err(format!(
                "PTC script timed out after {} ms",
                timeout.as_millis()
            ));
        }
        let wait = deadline.saturating_duration_since(now).min(POLL);
        match line_rx.recv_timeout(wait) {
            Ok(Ok(line)) => {
                let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else {
                    continue;
                };
                match msg.get("op").and_then(Value::as_str) {
                    Some("call") => {
                        let tool_name =
                            msg.get("name").and_then(Value::as_str).unwrap_or("").trim();
                        let tool_args = msg.get("args").cloned().unwrap_or_else(|| json!({}));
                        let resp = if !is_allowed_tool(tool_name) {
                            json!({
                                "ok": false,
                                "error": format!(
                                    "tool `{tool_name}` is not allowed in {NAME} (read-only allowlist)"
                                )
                            })
                        } else if call_count >= MAX_TOOL_CALLS {
                            json!({
                                "ok": false,
                                "error": format!("PTC tool-call limit ({MAX_TOOL_CALLS}) reached")
                            })
                        } else {
                            call_count += 1;
                            let out = super::dispatch_readonly(ctx, tool_name, &tool_args);
                            json!({
                                "ok": !out.failed,
                                "text": out.text,
                                "error": if out.failed { Value::String(out.text) } else { Value::Null }
                            })
                        };
                        let reply = format!("{resp}\n");
                        if stdin
                            .write_all(reply.as_bytes())
                            .and_then(|_| stdin.flush())
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("done") => {
                        let out_text = msg.get("stdout").and_then(Value::as_str).unwrap_or("");
                        let err_text = msg.get("error").and_then(Value::as_str);
                        if let Some(err) = err_text.filter(|s| !s.trim().is_empty()) {
                            let combined = if out_text.trim().is_empty() {
                                err.to_string()
                            } else {
                                format!("{out_text}\n{err}")
                            };
                            final_out =
                                Some(ToolOutput::err(clip_chars(&combined, MAX_OUTPUT_CHARS)));
                        } else {
                            final_out =
                                Some(ToolOutput::ok(clip_chars(out_text, MAX_OUTPUT_CHARS)));
                        }
                        break;
                    }
                    _ => {}
                }
            }
            Ok(Err(e)) => {
                final_out = Some(ToolOutput::err(format!("PTC read error: {e}")));
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    drop(stdin);
    let status = child.wait().ok();
    let _ = reader_handle.join();
    let stderr_text = stderr_handle.join().unwrap_or_default();

    if let Some(out) = final_out {
        return out;
    }
    let code = status.and_then(|s| s.code()).unwrap_or(1);
    let detail = if stderr_text.trim().is_empty() {
        format!("PTC script exited with status {code} without a result")
    } else {
        format!(
            "PTC script exited with status {code}:\n{}",
            clip_chars(stderr_text.trim(), MAX_OUTPUT_CHARS)
        )
    };
    ToolOutput::err(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_ws(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-ptc-{tag}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ctx_for<'a>(ws: &'a Path, stop: &'a dyn Fn() -> bool) -> ToolCtx<'a> {
        ToolCtx {
            workspace: ws,
            desktop: None,
            stop,
            tasks: None,
            owner: None,
            session: Some("ptc-test"),
        }
    }

    #[test]
    fn ptc_chains_glob_and_read_file_in_one_turn() {
        let ws = temp_ws("chain");
        fs::write(ws.join("a.txt"), "alpha=10\n").unwrap();
        fs::write(ws.join("b.txt"), "beta=25\n").unwrap();
        let stop = || false;
        let ctx = ctx_for(&ws, &stop);

        let script = r#"
import json
files = [l.strip() for l in glob("*.txt").splitlines() if l.strip()]
total = 0
for f in sorted(files):
    text = read_file(f)
    for line in text.splitlines():
        if "=" in line:
            total += int(line.split("=", 1)[1].strip())
print(json.dumps({"count": len(files), "total": total}))
"#;
        let out = run(&ctx, &json!({ "script": script }));
        assert!(!out.failed, "PTC failed: {}", out.text);
        let parsed: Value = serde_json::from_str(out.text.trim()).expect("valid json output");
        assert_eq!(parsed["count"], 2);
        assert_eq!(parsed["total"], 35);
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn ptc_refuses_mutating_and_shell_tools() {
        let ws = temp_ws("refuse");
        let stop = || false;
        let ctx = ctx_for(&ws, &stop);

        let script = r#"
for bad in ["write", "search_replace", "run_terminal_command", "click"]:
    try:
        call_tool(bad, {})
        print("UNEXPECTED", bad)
    except RuntimeError as e:
        assert "not allowed" in str(e), str(e)
print("ALL_REFUSED")
"#;
        let out = run(&ctx, &json!({ "script": script }));
        assert!(!out.failed, "{}", out.text);
        assert_eq!(out.text.trim(), "ALL_REFUSED");
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn ptc_enforces_tool_call_cap_and_script_size_cap() {
        let ws = temp_ws("caps");
        fs::write(ws.join("x.txt"), "hi\n").unwrap();
        let stop = || false;
        let ctx = ctx_for(&ws, &stop);

        let big = "a".repeat(MAX_SCRIPT_BYTES + 1);
        let out_big = run(&ctx, &json!({ "script": big }));
        assert!(out_big.failed);
        assert!(out_big.text.contains("byte limit"));

        let script = r#"
hit = False
for i in range(40):
    try:
        list_dir(".")
    except RuntimeError as e:
        if "tool-call limit" in str(e):
            hit = True
            break
print("HIT_CAP" if hit else "NO_CAP")
"#;
        let out = run(&ctx, &json!({ "script": script }));
        assert!(!out.failed, "{}", out.text);
        assert_eq!(out.text.trim(), "HIT_CAP");
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn ptc_enforces_timeout() {
        let ws = temp_ws("timeout");
        let stop = || false;
        let ctx = ctx_for(&ws, &stop);
        let out = run(
            &ctx,
            &json!({
                "script": "import time\ntime.sleep(5)\n",
                "timeout_ms": 200
            }),
        );
        assert!(out.failed);
        assert!(out.text.contains("timed out"), "{}", out.text);
        let _ = fs::remove_dir_all(&ws);
    }
}

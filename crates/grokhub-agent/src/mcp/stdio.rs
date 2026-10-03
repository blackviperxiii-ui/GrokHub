//! Stdio MCP session. The child sits in its own process group (Unix) or job
//! (Windows) so shutdown kills the tree. A crashed child is an error string.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::rpc::{self, RawTool};

enum IoMsg {
    Msg(Value),
    Eof(String),
}

pub(crate) struct StdioConn {
    server: String,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    rx: Receiver<IoMsg>,
    reader: Option<JoinHandle<()>>,
    next_id: u64,
    #[cfg(unix)]
    pgid: i32,
    #[cfg(windows)]
    job: usize,
    killed: bool,
}

pub(crate) fn connect(
    server: &str,
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    cwd: &Path,
    timeout: Duration,
) -> Result<(StdioConn, Vec<RawTool>), String> {
    let mut conn = spawn(server, command, args, env, cwd)?;
    let init = conn.roundtrip("initialize", rpc::initialize_params(), timeout)?;
    let _ = init;
    conn.write_value(&rpc::notification(
        "notifications/initialized",
        serde_json::json!({}),
    ))?;
    let tools = list_tools(&mut conn, timeout)?;
    Ok((conn, tools))
}

impl StdioConn {
    pub(crate) fn call(
        &mut self,
        name: &str,
        args: &Value,
        timeout: Duration,
    ) -> Result<(String, bool), String> {
        let result = self.roundtrip(
            "tools/call",
            serde_json::json!({"name": name, "arguments": args}),
            timeout,
        )?;
        Ok(rpc::tool_output(&result))
    }

    pub(crate) fn ping(&mut self, timeout: Duration) -> Result<(), String> {
        self.roundtrip("ping", serde_json::json!({}), timeout)
            .map(|_| ())
    }

    pub(crate) fn shutdown(&mut self) {
        self.kill_tree();
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
        self.stdin.take();
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
    }

    fn roundtrip(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        self.write_value(&rpc::request(id, method, params))?;
        let deadline = Instant::now() + timeout;
        loop {
            if self.child_exited() {
                return Err(format!("MCP server `{0}` stopped", self.server));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                self.kill_tree();
                return Err(format!("MCP server `{0}` timed out", self.server));
            }
            match self.rx.recv_timeout(left.min(Duration::from_millis(200))) {
                Ok(IoMsg::Msg(msg)) => {
                    if rpc::id_matches(&msg, id) {
                        if let Some(err) = rpc::rpc_error(&msg) {
                            return Err(err);
                        }
                        return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
                    }
                    if let Some(reply) = super::elicit::answer_elicitation(&self.server, &msg) {
                        self.write_value(&reply)?;
                        continue;
                    }
                    if msg.get("method").is_some() && msg.get("id").is_some() {
                        let rid = msg.get("id").cloned().unwrap_or(Value::Null);
                        self.write_value(&serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": rid,
                            "error": {"code": -32601, "message": "method not supported"}
                        }))?;
                    }
                }
                Ok(IoMsg::Eof(err)) => {
                    return Err(if err.is_empty() {
                        format!("MCP server `{}` stopped", self.server)
                    } else {
                        format!("MCP server `{}` stopped: {err}", self.server)
                    });
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(format!("MCP server `{}` stopped", self.server));
                }
            }
        }
    }

    fn write_value(&mut self, value: &Value) -> Result<(), String> {
        let mut line = serde_json::to_string(value).map_err(|err| err.to_string())?;
        line.push('\n');
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| format!("MCP server `{}` stopped", self.server))?;
        stdin
            .write_all(line.as_bytes())
            .map_err(|err| format!("MCP server `{}` stopped: {err}", self.server))?;
        stdin
            .flush()
            .map_err(|err| format!("MCP server `{}` stopped: {err}", self.server))?;
        Ok(())
    }

    fn child_exited(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return true;
        };
        match child.try_wait() {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => true,
        }
    }

    fn kill_tree(&mut self) {
        if self.killed {
            return;
        }
        self.killed = true;
        #[cfg(unix)]
        kill_group(self.pgid);
        #[cfg(windows)]
        terminate_job(self.job);
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
    }
}

impl Drop for StdioConn {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn list_tools(conn: &mut StdioConn, timeout: Duration) -> Result<Vec<RawTool>, String> {
    let mut all = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..100 {
        let mut params = serde_json::json!({});
        if let Some(token) = &cursor {
            params["cursor"] = serde_json::json!(token);
        }
        let result = conn.roundtrip("tools/list", params, timeout)?;
        let (page, next) = rpc::parse_tools(&result);
        all.extend(page);
        match next {
            Some(token) => cursor = Some(token),
            None => return Ok(all),
        }
    }
    Ok(all)
}

fn spawn(
    server: &str,
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<StdioConn, String> {
    if command.trim().is_empty() {
        return Err(format!("MCP server `{server}` has no command"));
    }
    let mut cmd = Command::new(command);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|err| format!("MCP server `{server}` failed to start: {err}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| format!("MCP server `{server}` has no stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("MCP server `{server}` has no stdout"))?;
    let stderr = child.stderr.take();
    #[cfg(unix)]
    let pgid = child.id() as i32;
    #[cfg(windows)]
    let job = assign_job(child.id()).unwrap_or(0);
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || read_stdout(stdout, tx));
    if let Some(stderr) = stderr {
        std::thread::spawn(move || drain_stderr(stderr));
    }
    Ok(StdioConn {
        server: server.to_string(),
        child: Some(child),
        stdin: Some(stdin),
        rx,
        reader: Some(reader),
        next_id: 0,
        #[cfg(unix)]
        pgid,
        #[cfg(windows)]
        job,
        killed: false,
    })
}

fn read_stdout(stdout: std::process::ChildStdout, tx: mpsc::Sender<IoMsg>) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_message(&mut reader) {
            Ok(Some(msg)) => {
                if tx.send(IoMsg::Msg(msg)).is_err() {
                    break;
                }
            }
            Ok(None) => {
                let _ = tx.send(IoMsg::Eof(String::new()));
                break;
            }
            Err(err) => {
                let _ = tx.send(IoMsg::Eof(err));
                break;
            }
        }
    }
}

fn read_message(
    reader: &mut BufReader<std::process::ChildStdout>,
) -> Result<Option<Value>, String> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|err| err.to_string())?;
        if n == 0 {
            return Ok(None);
        }
        if line.len() > 8 * 1024 * 1024 {
            return Err("message too large".into());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            let len: usize = rest.trim().parse().map_err(|_| "bad framing".to_string())?;
            if len > 8 * 1024 * 1024 {
                return Err("message too large".into());
            }
            loop {
                line.clear();
                let n = reader.read_line(&mut line).map_err(|err| err.to_string())?;
                if n == 0 {
                    return Ok(None);
                }
                if line.trim().is_empty() {
                    break;
                }
            }
            let mut buf = vec![0u8; len];
            use std::io::Read;
            reader.read_exact(&mut buf).map_err(|err| err.to_string())?;
            let text = String::from_utf8_lossy(&buf);
            return serde_json::from_str(&text)
                .map(Some)
                .map_err(|err| err.to_string());
        }
        if trimmed.starts_with('{') {
            return serde_json::from_str(trimmed)
                .map(Some)
                .map_err(|err| err.to_string());
        }
    }
}

fn drain_stderr(pipe: std::process::ChildStderr) {
    let mut reader = BufReader::new(pipe);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        line.clear();
    }
}

#[cfg(unix)]
fn kill_group(pgid: i32) {
    if pgid > 1 {
        let own = unsafe { libc::getpgrp() };
        if pgid != own {
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
}

#[cfg(windows)]
fn assign_job(pid: u32) -> Result<usize, String> {
    use std::ptr;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };
    unsafe {
        let job = CreateJobObjectW(ptr::null(), ptr::null());
        if job.is_null() {
            return Err("job".into());
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const _,
            u32::try_from(std::mem::size_of_val(&limits)).unwrap_or(u32::MAX),
        );
        if set == 0 {
            CloseHandle(job);
            return Err("job limits".into());
        }
        let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
        if process.is_null() {
            CloseHandle(job);
            return Err("process".into());
        }
        let assigned = AssignProcessToJobObject(job, process);
        CloseHandle(process);
        if assigned == 0 {
            CloseHandle(job);
            return Err("assign".into());
        }
        Ok(job as usize)
    }
}

#[cfg(windows)]
fn terminate_job(job: usize) {
    if job == 0 {
        return;
    }
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::TerminateJobObject;
    unsafe {
        let handle = job as *mut _;
        TerminateJobObject(handle, 1);
        CloseHandle(handle);
    }
}

pub(crate) fn workspace_or(cwd: Option<PathBuf>, fallback: &Path) -> PathBuf {
    cwd.filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| fallback.to_path_buf())
}

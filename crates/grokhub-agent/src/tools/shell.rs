// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! `run_terminal_command`. Bash on Unix, PowerShell on Windows.
//! The child is its own process group or job so a stop kills the whole tree.

use std::path::Path;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use serde_json::{json, Value};

use super::ToolOutput;

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 300_000;
const OUTPUT_CHARS: usize = 20_000;
const MARKER: &str = "\n\n... (output truncated) ...\n\n";
const POLL: Duration = Duration::from_millis(20);

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "run_terminal_command",
        "description": "Run a shell command in the workspace. Bash on Linux, PowerShell on Windows. timeout is milliseconds (default 120000, max 300000). The process tree is killed on timeout, cancel, or halt.",
        "parameters": {
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "timeout": {"type": "integer", "description": "Milliseconds. Default 120000. Maximum 300000."}
            },
            "required": ["command"],
            "additionalProperties": false
        }
    })
}

pub fn run(workspace: &Path, args: &Value, stop: &dyn Fn() -> bool) -> ToolOutput {
    let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if command.is_empty() {
        return ToolOutput::err("command is required");
    }
    let timeout_ms = timeout_ms(args);
    let cwd = match workspace.canonicalize() {
        Ok(path) => path,
        Err(err) => return ToolOutput::err(format!("workspace is not available: {err}")),
    };
    match spawn_and_wait(&cwd, &command, timeout_ms, stop) {
        Ok(output) => output,
        Err(err) => ToolOutput::err(err),
    }
}

fn timeout_ms(args: &Value) -> u64 {
    let Some(n) = args.get("timeout").and_then(|v| v.as_u64()) else {
        return DEFAULT_TIMEOUT_MS;
    };
    if n == 0 {
        DEFAULT_TIMEOUT_MS
    } else {
        n.min(MAX_TIMEOUT_MS)
    }
}

fn cap_output(raw: &str) -> String {
    let text = raw.trim();
    if text.chars().count() <= OUTPUT_CHARS {
        return text.to_string();
    }
    let half = OUTPUT_CHARS / 2;
    let total = text.chars().count();
    let front: String = text.chars().take(half).collect();
    let back: String = text.chars().skip(total - half).collect();
    format!("{front}{MARKER}{back}")
}

fn finish(status: &str, raw: &[u8], failed: bool) -> ToolOutput {
    let text_raw = String::from_utf8_lossy(raw);
    let body = cap_output(&text_raw);
    let text = if body.is_empty() {
        status.to_string()
    } else {
        format!("{status}\n{body}")
    };
    ToolOutput {
        text,
        image_data_url: None,
        failed,
    }
}

struct CapBuf {
    head: Vec<u8>,
    tail: std::collections::VecDeque<u8>,
    overflow: bool,
}

impl CapBuf {
    const KEEP: usize = 96 * 1024;

    fn new() -> Self {
        Self {
            head: Vec::new(),
            tail: std::collections::VecDeque::new(),
            overflow: false,
        }
    }

    fn push(&mut self, data: &[u8]) {
        if !self.overflow {
            let room = Self::KEEP.saturating_sub(self.head.len());
            let n = room.min(data.len());
            self.head.extend_from_slice(&data[..n]);
            if n == data.len() {
                return;
            }
            self.overflow = true;
            self.push_tail(&data[n..]);
            return;
        }
        self.push_tail(data);
    }

    fn push_tail(&mut self, data: &[u8]) {
        for byte in data {
            if self.tail.len() == Self::KEEP {
                self.tail.pop_front();
            }
            self.tail.push_back(*byte);
        }
    }

    fn bytes(self) -> Vec<u8> {
        if !self.overflow {
            return self.head;
        }
        let mut out = self.head;
        out.extend(self.tail.iter().copied());
        out
    }
}

fn merge_output(stdout: Vec<u8>, stderr: Vec<u8>) -> Vec<u8> {
    if stderr.is_empty() {
        return stdout;
    }
    if stdout.is_empty() {
        return stderr;
    }
    let mut out = stdout;
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend(stderr);
    out
}

#[cfg(unix)]
fn spawn_and_wait(cwd: &Path, command: &str, timeout_ms: u64, stop: &dyn Fn() -> bool) -> Result<ToolOutput, String> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::thread;

    let mut child = Command::new("bash")
        .args(["--noprofile", "--norc", "-c", command])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|err| format!("could not start bash: {err}"))?;
    let pgid = child.id() as i32;
    let mut group = Group { pgid, armed: pgid > 1 };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_thread = thread::spawn(move || read_pipe(stdout));
    let err_thread = thread::spawn(move || read_pipe(stderr));
    let started = Instant::now();
    let limit = Duration::from_millis(timeout_ms);
    let status = loop {
        if stop() {
            kill_group(group.pgid);
            group.disarm();
            let _ = child.wait();
            break StopKind::Killed;
        }
        if started.elapsed() >= limit {
            kill_group(group.pgid);
            group.disarm();
            let _ = child.wait();
            break StopKind::TimedOut;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                group.disarm();
                break StopKind::Exited(status.code().unwrap_or(1));
            }
            Ok(None) => thread::sleep(POLL),
            Err(err) => {
                kill_group(group.pgid);
                group.disarm();
                let _ = child.wait();
                return Err(format!("could not wait for bash: {err}"));
            }
        }
    };
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    let raw = merge_output(stdout, stderr);
    Ok(match status {
        StopKind::Exited(code) => finish(&format!("exit {code}"), &raw, code != 0),
        StopKind::TimedOut => finish(&format!("timed out after {timeout_ms}ms"), &raw, true),
        StopKind::Killed => finish("killed", &raw, true),
    })
}

#[cfg(unix)]
fn read_pipe(pipe: Option<impl std::io::Read>) -> Vec<u8> {
    let mut cap = CapBuf::new();
    let Some(mut pipe) = pipe else {
        return cap.bytes();
    };
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => cap.push(&buf[..n]),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    cap.bytes()
}

#[cfg(unix)]
struct Group {
    pgid: i32,
    armed: bool,
}

#[cfg(unix)]
impl Group {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(unix)]
impl Drop for Group {
    fn drop(&mut self) {
        if self.armed {
            kill_group(self.pgid);
        }
    }
}

#[cfg(unix)]
fn kill_group(pgid: i32) {
    if pgid > 1 {
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
    }
}

#[cfg(unix)]
enum StopKind {
    Exited(i32),
    TimedOut,
    Killed,
}

#[cfg(windows)]
fn spawn_and_wait(cwd: &Path, command: &str, timeout_ms: u64, stop: &dyn Fn() -> bool) -> Result<ToolOutput, String> {
    win::run(cwd, command, timeout_ms, stop)
}

#[cfg(not(any(unix, windows)))]
fn spawn_and_wait(
    _cwd: &Path,
    _command: &str,
    _timeout_ms: u64,
    _stop: &dyn Fn() -> bool,
) -> Result<ToolOutput, String> {
    Err("shell is not available on this system".into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    #[cfg(unix)]
    use super::run;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-sh-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn timeout_is_clamped() {
        assert_eq!(super::timeout_ms(&json!({})), 120_000);
        assert_eq!(super::timeout_ms(&json!({"timeout": 0})), 120_000);
        assert_eq!(super::timeout_ms(&json!({"timeout": 200})), 200);
        assert_eq!(super::timeout_ms(&json!({"timeout": 900_000})), 300_000);
    }

    #[cfg(unix)]
    #[test]
    fn shell_echo_timeout_cap_and_tree_kill() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let dir = scratch("echo");
        let ok = run(&dir, &json!({"command": "printf 'hello\\n'"}), &|| false);
        assert!(!ok.failed, "{}", ok.text);
        assert!(ok.text.contains("exit 0"), "{}", ok.text);
        assert!(ok.text.contains("hello"), "{}", ok.text);

        let bad = run(&dir, &json!({"command": "exit 7"}), &|| false);
        assert!(bad.failed, "{}", bad.text);
        assert!(bad.text.contains("exit 7"), "{}", bad.text);

        let started = Instant::now();
        let timed = run(&dir, &json!({"command": "sleep 30", "timeout": 200}), &|| false);
        assert!(timed.failed, "{}", timed.text);
        assert!(timed.text.contains("timed out after 200ms"), "{}", timed.text);
        assert!(started.elapsed() < Duration::from_secs(5), "timeout took {:?}", started.elapsed());

        let noisy = run(
            &dir,
            &json!({"command": "dd if=/dev/zero bs=30000 count=1 status=none | tr '\\0' 'x'"}),
            &|| false,
        );
        assert!(!noisy.failed, "{}", noisy.text);
        assert!(noisy.text.contains("\n\n... (output truncated) ...\n\n"), "{}", noisy.text.chars().take(80).collect::<String>());
        assert!(noisy.text.chars().count() < 30_000, "{}", noisy.text.chars().count());

        let tree = scratch("tree");
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let tree_job = tree.clone();
        let worker = std::thread::spawn(move || {
            run(
                &tree_job,
                &json!({"command": "sleep 30 & echo $! > child.pid; wait", "timeout": 20000}),
                &|| flag.load(Ordering::SeqCst),
            )
        });
        let pidfile = tree.join("child.pid");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut pid = 0i32;
        while Instant::now() < deadline {
            if let Ok(text) = std::fs::read_to_string(&pidfile) {
                if let Ok(n) = text.trim().parse::<i32>() {
                    if n > 1 {
                        pid = n;
                        break;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(pid > 1, "child pid was not written");
        let own = unsafe { libc::getpgrp() };
        assert_ne!(own, pid, "refusing to signal the test process group");
        stop.store(true, Ordering::SeqCst);
        let output = worker.join().expect("shell thread");
        assert!(output.failed, "{}", output.text);
        assert!(output.text.contains("killed"), "{}", output.text);
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!process_alive(pid), "child {pid} still alive");
        assert!(process_alive(own), "test process group was killed");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&tree);
    }

    #[cfg(unix)]
    fn process_alive(pid: i32) -> bool {
        if pid <= 0 {
            return false;
        }
        let rc = unsafe { libc::kill(pid, 0) };
        if rc == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::OsStr;
    use std::fs::File;
    use std::io::Read;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::path::Path;
    use std::ptr;
    use std::thread;
    use std::time::{Duration, Instant};

    use base64::Engine;
    use windows_sys::Win32::Foundation::{
        CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
        STILL_ACTIVE, TRUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, GetExitCodeProcess, ResumeThread, TerminateProcess, WaitForSingleObject,
        CREATE_NO_WINDOW, CREATE_SUSPENDED, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW,
    };

    use super::{finish, CapBuf, POLL};

    pub fn run(
        cwd: &Path,
        command: &str,
        timeout_ms: u64,
        stop: &dyn Fn() -> bool,
    ) -> Result<super::ToolOutput, String> {
        let mut child = JobChild::spawn(cwd, command)?;
        let started = Instant::now();
        let limit = Duration::from_millis(timeout_ms);
        let kind = loop {
            if stop() {
                child.kill();
                break Kind::Killed;
            }
            if started.elapsed() >= limit {
                child.kill();
                break Kind::TimedOut;
            }
            match child.poll() {
                Poll::Running => thread::sleep(POLL),
                Poll::Exited(code) => break Kind::Exited(code),
                Poll::Failed(err) => {
                    child.kill();
                    return Err(err);
                }
            }
        };
        let raw = child.output();
        Ok(match kind {
            Kind::Exited(code) => finish(&format!("exit {code}"), &raw, code != 0),
            Kind::TimedOut => finish(&format!("timed out after {timeout_ms}ms"), &raw, true),
            Kind::Killed => finish("killed", &raw, true),
        })
    }

    enum Kind {
        Exited(u32),
        TimedOut,
        Killed,
    }

    enum Poll {
        Running,
        Exited(u32),
        Failed(String),
    }

    struct JobChild {
        job: HANDLE,
        process: HANDLE,
        thread: HANDLE,
        stdout_read: HANDLE,
        stderr_read: HANDLE,
        stdout_write: HANDLE,
        stderr_write: HANDLE,
        stdin_read: HANDLE,
        stdin_write: HANDLE,
        reader: Option<thread::JoinHandle<Vec<u8>>>,
        err_reader: Option<thread::JoinHandle<Vec<u8>>>,
    }

    impl JobChild {
        fn spawn(cwd: &Path, command: &str) -> Result<Self, String> {
            let mut child = Self {
                job: ptr::null_mut(),
                process: ptr::null_mut(),
                thread: ptr::null_mut(),
                stdout_read: ptr::null_mut(),
                stderr_read: ptr::null_mut(),
                stdout_write: ptr::null_mut(),
                stderr_write: ptr::null_mut(),
                stdin_read: ptr::null_mut(),
                stdin_write: ptr::null_mut(),
                reader: None,
                err_reader: None,
            };
            unsafe {
                child.job = CreateJobObjectW(ptr::null(), ptr::null());
                if child.job.is_null() {
                    return Err(os_err("could not create job"));
                }
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let set = SetInformationJobObject(
                    child.job,
                    JobObjectExtendedLimitInformation,
                    &limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const _,
                    u32::try_from(std::mem::size_of_val(&limits)).unwrap_or(u32::MAX),
                );
                if set == 0 {
                    return Err(os_err("could not set job limits"));
                }
                let (stdout_read, stdout_write) = make_pipe()?;
                let (stderr_read, stderr_write) = make_pipe()?;
                let (stdin_read, stdin_write) = make_pipe()?;
                child.stdout_read = stdout_read;
                child.stderr_read = stderr_read;
                child.stdout_write = stdout_write;
                child.stderr_write = stderr_write;
                child.stdin_read = stdin_read;
                child.stdin_write = stdin_write;
                hide_inherit(child.stdout_read)?;
                hide_inherit(child.stderr_read)?;
                hide_inherit(child.stdin_write)?;
                let exe = powershell_exe();
                let encoded = encode_ps(command);
                let mut cmdline = wide(&format!("\"{exe}\" -NoProfile -NonInteractive -EncodedCommand {encoded}"));
                let app = wide(&exe);
                let dir = wide(&cwd.display().to_string());
                let mut si: STARTUPINFOW = std::mem::zeroed();
                si.cb = u32::try_from(std::mem::size_of::<STARTUPINFOW>()).unwrap_or(0);
                si.dwFlags = STARTF_USESTDHANDLES;
                si.hStdInput = child.stdin_read;
                si.hStdOutput = child.stdout_write;
                si.hStdError = child.stderr_write;
                let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
                let created = CreateProcessW(
                    app.as_ptr(),
                    cmdline.as_mut_ptr(),
                    ptr::null(),
                    ptr::null(),
                    TRUE,
                    CREATE_SUSPENDED | CREATE_NO_WINDOW,
                    ptr::null(),
                    dir.as_ptr(),
                    &si,
                    &mut pi,
                );
                if created == 0 {
                    return Err(os_err("could not start PowerShell"));
                }
                child.process = pi.hProcess;
                child.thread = pi.hThread;
                if AssignProcessToJobObject(child.job, child.process) == 0 {
                    return Err(os_err("could not assign job"));
                }
                if ResumeThread(child.thread) == u32::MAX {
                    return Err(os_err("could not resume process"));
                }
                close(&mut child.stdout_write);
                close(&mut child.stderr_write);
                close(&mut child.stdin_read);
                close(&mut child.stdin_write);
                let stdout = take_file(&mut child.stdout_read);
                let stderr = take_file(&mut child.stderr_read);
                child.reader = Some(thread::spawn(move || read_file(stdout)));
                child.err_reader = Some(thread::spawn(move || read_file(stderr)));
            }
            Ok(child)
        }

        fn kill(&mut self) {
            unsafe {
                if !self.job.is_null() {
                    TerminateJobObject(self.job, 1);
                }
                if !self.process.is_null() {
                    let _ = WaitForSingleObject(self.process, 2_000);
                }
            }
        }

        fn poll(&self) -> Poll {
            unsafe {
                let wait = WaitForSingleObject(self.process, 0);
                if wait == WAIT_TIMEOUT {
                    return Poll::Running;
                }
                if wait != WAIT_OBJECT_0 {
                    return Poll::Failed(os_err("wait failed"));
                }
                let mut code = 0u32;
                if GetExitCodeProcess(self.process, &mut code) == 0 {
                    return Poll::Failed(os_err("exit code failed"));
                }
                if code == STILL_ACTIVE as u32 {
                    Poll::Running
                } else {
                    Poll::Exited(code)
                }
            }
        }

        fn output(mut self) -> Vec<u8> {
            close(&mut self.stdout_write);
            close(&mut self.stderr_write);
            close(&mut self.stdin_write);
            close(&mut self.stdin_read);
            let stdout = self.reader.take().and_then(|handle| handle.join().ok()).unwrap_or_default();
            let stderr = self.err_reader.take().and_then(|handle| handle.join().ok()).unwrap_or_default();
            super::merge_output(stdout, stderr)
        }
    }

    impl Drop for JobChild {
        fn drop(&mut self) {
            unsafe {
                if !self.job.is_null() {
                    TerminateJobObject(self.job, 1);
                }
                if !self.process.is_null() {
                    TerminateProcess(self.process, 1);
                }
            }
            close(&mut self.thread);
            close(&mut self.process);
            close(&mut self.job);
            close(&mut self.stdout_read);
            close(&mut self.stderr_read);
            close(&mut self.stdout_write);
            close(&mut self.stderr_write);
            close(&mut self.stdin_read);
            close(&mut self.stdin_write);
        }
    }

    fn take_file(handle: &mut HANDLE) -> File {
        let raw = std::mem::replace(handle, ptr::null_mut());
        // The pipe end is exclusively owned from here. Drop closes it.
        unsafe { File::from_raw_handle(raw) }
    }

    fn read_file(mut file: File) -> Vec<u8> {
        let mut cap = CapBuf::new();
        let mut buf = [0u8; 8192];
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => cap.push(&buf[..n]),
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        cap.bytes()
    }

    fn os_err(prefix: &str) -> String {
        let err = std::io::Error::last_os_error();
        format!("{prefix}: {err}")
    }

    fn close(handle: &mut HANDLE) {
        if !handle.is_null() && *handle != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(*handle);
            }
            *handle = ptr::null_mut();
        }
    }

    unsafe fn make_pipe() -> Result<(HANDLE, HANDLE), String> {
        let mut attrs: SECURITY_ATTRIBUTES = std::mem::zeroed();
        attrs.nLength = u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0);
        attrs.bInheritHandle = TRUE;
        attrs.lpSecurityDescriptor = ptr::null_mut();
        let mut read = ptr::null_mut();
        let mut write = ptr::null_mut();
        if CreatePipe(&mut read, &mut write, &attrs, 0) == 0 {
            return Err(os_err("could not create pipe"));
        }
        Ok((read, write))
    }

    unsafe fn hide_inherit(handle: HANDLE) -> Result<(), String> {
        if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(os_err("could not set handle flags"));
        }
        Ok(())
    }

    fn powershell_exe() -> String {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe")
    }

    fn encode_ps(script: &str) -> String {
        let bytes: Vec<u8> = script.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain(std::iter::once(0)).collect()
    }

    #[cfg(test)]
    fn process_alive(pid: u32) -> bool {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            ok != 0 && code == STILL_ACTIVE as u32
        }
    }

    #[test]
    fn shell_cancel_kills_the_child_tree() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!("gh-sh-win-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = concat!(
            "$p = Start-Process -PassThru -WindowStyle Hidden -FilePath $env:ComSpec ",
            "-ArgumentList '/c','ping -n 40 127.0.0.1 >nul'; ",
            "Set-Content -LiteralPath 'child.pid' -Value $p.Id; ",
            "Wait-Process -Id $p.Id"
        );
        let args = serde_json::json!({"command": script, "timeout": 20000});
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let dir_job = dir.clone();
        let worker = thread::spawn(move || super::run(&dir_job, &args, &|| flag.load(Ordering::SeqCst)));
        let pidfile = dir.join("child.pid");
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut pid = 0u32;
        while Instant::now() < deadline {
            if let Ok(text) = std::fs::read_to_string(&pidfile) {
                if let Ok(n) = text.trim().parse::<u32>() {
                    if n > 1 {
                        pid = n;
                        break;
                    }
                }
            }
            thread::sleep(POLL);
        }
        assert!(pid > 1, "child pid was not written");
        stop.store(true, Ordering::SeqCst);
        let output = worker.join().expect("shell thread");
        assert!(output.failed, "{}", output.text);
        assert!(output.text.contains("killed"), "{}", output.text);
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_alive(pid) && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        assert!(!process_alive(pid), "child {pid} still alive");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

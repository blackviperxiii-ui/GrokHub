//! Confirm-and-remember store for Code tab runs (tests now; deploy in slice 5b).
//!
//! A remembered command is valid only while its text is unchanged and it has
//! not been forgotten. Callers pass a [`Mode`] so Ask still confirms every
//! time and Auto may run a remembered command without asking.

use crate::secrets::{Keychain, Loaded, Shield, Vault};
use grokhub_core::model_download::sha256_hex;
use grokhub_core::proc_util::hide_windows_console;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// The on-disk store, in the app config folder.
pub const FILE: &str = "code-runs.json";
/// The most output one run keeps (and sends to the log pane).
pub const OUTPUT_CAP: usize = 256 * 1024;
/// Event the shell window listens to for log chunks while a run is in progress.
pub const RUN_EVENT: &str = "code-run";

/// What kind of command this entry is for. Deploy uses the same store in 5b.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Test,
    Deploy,
}

impl Kind {
    /// The word for this kind of command in an error sentence.
    fn word(self) -> &'static str {
        match self {
            Kind::Test => "test",
            Kind::Deploy => "deploy",
        }
    }
}

/// How a remembered command is treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Confirm every run, even when this exact command is remembered.
    Ask,
    /// A remembered matching command may run without asking.
    Auto,
}

/// Whether this command may run under the given mode, or needs a confirm first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "camelCase")]
pub enum Decision {
    NeedsConfirm { command: String },
    Remembered,
}

/// What the Code tab shows before a test run: the command to use, and whether
/// it still needs a confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub command: String,
    pub needs_confirm: bool,
    /// The command currently remembered for this project and kind, if any.
    pub remembered: Option<String>,
}

/// A finished run, with output already redacted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub command: String,
    pub exit: i32,
    pub output: String,
    pub cut: bool,
}

/// Settings → Code: whether this project's test command is remembered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RememberedView {
    pub command: Option<String>,
}

/// A chunk of redacted output (or the end of the run) for the log pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunChunk {
    pub text: String,
    pub done: bool,
    pub exit: Option<i32>,
    pub cut: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    repo_key: String,
    kind: Kind,
    command: String,
}

/// The process id of the one test run or deploy in progress, if any.
static CURRENT: Mutex<Option<u32>> = Mutex::new(None);
const BUSY: &str = "A run is already in progress.";

/// A stable id for a project: sha256 of `remote + newline + repo root`, or
/// just the root when there is no remote. The raw URL and path are not stored.
pub fn repo_key(root: &Path, remote: Option<&str>) -> String {
    let root = root
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    let input = match remote.map(str::trim).filter(|s| !s.is_empty()) {
        Some(remote) => format!("{remote}\n{root}"),
        None => root,
    };
    sha256_hex(input.as_bytes())
}

/// The default test command for this folder, or empty if we can't tell.
pub fn detect_test_command(dir: &Path) -> String {
    if dir.join("Cargo.toml").is_file() {
        return "cargo test".into();
    }
    if dir.join("package.json").is_file() && package_has_test(dir) {
        if dir.join("pnpm-lock.yaml").is_file() {
            return "pnpm test".into();
        }
        if dir.join("yarn.lock").is_file() {
            return "yarn test".into();
        }
        return "npm test".into();
    }
    if dir.join("go.mod").is_file() {
        return "go test ./...".into();
    }
    String::new()
}

fn package_has_test(dir: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(dir.join("package.json")) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| {
            v.get("scripts")?
                .get("test")?
                .as_str()
                .map(|s| !s.trim().is_empty())
        })
        .unwrap_or(false)
}

/// Look up whether this (kind, repo, command) may run under `mode`.
///
/// A remembered entry is invalid when the command text differs or was forgotten.
/// [`Mode::Ask`] always returns [`Decision::NeedsConfirm`].
pub fn check(
    config_dir: &Path,
    kind: Kind,
    repo_key: &str,
    command: &str,
    mode: Mode,
) -> Result<Decision, String> {
    Ok(decide(&load(config_dir)?, kind, repo_key, command, mode))
}

fn decide(store: &Store, kind: Kind, repo_key: &str, command: &str, mode: Mode) -> Decision {
    let command = command.trim().to_string();
    match mode {
        Mode::Ask => Decision::NeedsConfirm { command },
        Mode::Auto => {
            if remembered_command(store, kind, repo_key).as_deref() == Some(command.as_str()) {
                Decision::Remembered
            } else {
                Decision::NeedsConfirm { command }
            }
        }
    }
}

fn remembered_command(store: &Store, kind: Kind, repo_key: &str) -> Option<String> {
    store
        .entries
        .iter()
        .find(|e| e.kind == kind && e.repo_key == repo_key)
        .map(|e| e.command.clone())
}

/// Record that this exact command was confirmed for this project and kind.
pub fn remember(
    config_dir: &Path,
    kind: Kind,
    repo_key: &str,
    command: &str,
) -> Result<(), String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("Set a command first.".into());
    }
    let mut store = load(config_dir)?;
    store
        .entries
        .retain(|e| !(e.kind == kind && e.repo_key == repo_key));
    store.entries.push(Entry {
        repo_key: repo_key.to_string(),
        kind,
        command: command.to_string(),
    });
    save(config_dir, &store)
}

/// The command remembered for this project and kind, if any.
pub fn remembered(config_dir: &Path, kind: Kind, repo_key: &str) -> Result<Option<String>, String> {
    Ok(remembered_command(&load(config_dir)?, kind, repo_key))
}

/// Forget the remembered command for this project and kind, if any.
pub fn forget(config_dir: &Path, kind: Kind, repo_key: &str) -> Result<(), String> {
    let mut store = load(config_dir)?;
    let before = store.entries.len();
    store
        .entries
        .retain(|e| !(e.kind == kind && e.repo_key == repo_key));
    if store.entries.len() != before {
        save(config_dir, &store)?;
    }
    Ok(())
}

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

fn load(config_dir: &Path) -> Result<Store, String> {
    let file = path(config_dir);
    if !file.exists() {
        return Ok(Store::default());
    }
    let raw = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn save(config_dir: &Path, store: &Store) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
}

fn project_key(workspace: &Path) -> String {
    repo_key(workspace, crate::repo::origin_url(workspace).as_deref())
}

/// The command the Tests section should show, and whether it still needs a confirm.
pub fn plan(
    config_dir: &Path,
    workspace: &Path,
    command: Option<&str>,
    mode: Mode,
) -> Result<Plan, String> {
    let key = project_key(workspace);
    let store = load(config_dir)?;
    let remembered = remembered_command(&store, Kind::Test, &key);
    let command = command
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| remembered.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| detect_test_command(workspace));
    let decision = decide(&store, Kind::Test, &key, &command, mode);
    Ok(Plan {
        needs_confirm: matches!(decision, Decision::NeedsConfirm { .. }),
        remembered,
        command,
    })
}

/// The remembered test command for Settings → Code.
pub fn remembered_view(config_dir: &Path, workspace: &Path) -> Result<RememberedView, String> {
    let key = project_key(workspace);
    let store = load(config_dir)?;
    Ok(RememberedView {
        command: remembered_command(&store, Kind::Test, &key),
    })
}

/// Forget this project's remembered test command.
pub fn forget_test(config_dir: &Path, workspace: &Path) -> Result<RememberedView, String> {
    let key = project_key(workspace);
    forget(config_dir, Kind::Test, &key)?;
    remembered_view(config_dir, workspace)
}

/// Confirm (when needed), remember, and run the test command in the workspace.
///
/// Output is redacted before `on_chunk` sees it and before it is stored in the
/// result: the usual token patterns, and the project's secret values from the
/// keychain (a test run never has them in its environment, but the project
/// may print them itself). [`Mode::Ask`] still requires `confirmed` on every
/// run; memory is kept so a later Auto call can use it.
pub fn run(
    config_dir: &Path,
    workspace: &Path,
    command: &str,
    confirmed: bool,
    mode: Mode,
    on_chunk: impl FnMut(&str),
) -> Result<RunResult, String> {
    run_with(
        config_dir, workspace, command, confirmed, mode, &Keychain, on_chunk,
    )
}

/// [`run`], with the project's secret values read from `vault`.
pub fn run_with(
    config_dir: &Path,
    workspace: &Path,
    command: &str,
    confirmed: bool,
    mode: Mode,
    vault: &dyn Vault,
    mut on_chunk: impl FnMut(&str),
) -> Result<RunResult, String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("Set a test command first.".into());
    }
    let key = project_key(workspace);
    let store = load(config_dir)?;
    match decide(&store, Kind::Test, &key, command, mode) {
        Decision::NeedsConfirm { .. } if !confirmed => {
            return Err("This command needs a confirmation first.".into());
        }
        Decision::NeedsConfirm { .. } => {
            remember(config_dir, Kind::Test, &key, command)?;
        }
        Decision::Remembered => {}
    }
    let held = crate::secrets::held_values(config_dir, vault, &key);
    exec(workspace, command, Kind::Test, &held, None, &mut on_chunk)
}

/// Run `command` in the workspace through the shell, as a test run or a
/// deploy. Each of `held` is hidden from the output. With `env`, the
/// project's secrets are also set in the child's environment.
pub(crate) fn exec(
    workspace: &Path,
    command: &str,
    kind: Kind,
    held: &[String],
    env: Option<&Loaded>,
    on_chunk: &mut impl FnMut(&str),
) -> Result<RunResult, String> {
    if !workspace.is_dir() {
        return Err("The workspace folder isn't there.".into());
    }
    // Held from the check until the child's id is stored, so two calls at the
    // same moment can never both start.
    let mut current = CURRENT.lock().map_err(|e| e.to_string())?;
    if current.is_some() {
        return Err(BUSY.into());
    }

    #[cfg(windows)]
    let mut c = {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    };
    #[cfg(not(windows))]
    let mut c = {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };
    c.current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "dumb")
        .env("GIT_TERMINAL_PROMPT", "0");
    // The child leads its own process group, so [`stop`] reaches everything
    // the command started.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    if let Some(secrets) = env {
        secrets.inject(&mut c);
    }
    hide_windows_console(&mut c);
    let mut child = c
        .spawn()
        .map_err(|e| format!("couldn't start the {} command: {e}", kind.word()))?;
    let pid = child.id();
    *current = Some(pid);
    drop(current);
    // Frees the runner however this function leaves, a panic included.
    let _running = Running(pid);

    let (tx, rx) = std::sync::mpsc::channel::<(usize, Vec<u8>)>();
    if let Some(out) = child.stdout.take() {
        spawn_pump(0, out, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        spawn_pump(1, err, tx.clone());
    }
    drop(tx);

    let mut output = String::new();
    let mut cut = false;
    let mut keep = |chunk: String| {
        if chunk.is_empty() {
            return;
        }
        // Once a chunk hasn't fit, the output has ended: a later, shorter
        // chunk is not kept or sent, even if the cut left a byte or two free.
        if cut || output.len() >= OUTPUT_CAP {
            cut = true;
            return;
        }
        cut |= push_capped(&mut output, &chunk, OUTPUT_CAP);
        on_chunk(&chunk);
    };
    // One shield for each stream: a line on one never lands inside a
    // half-written line of the other and splits a value there.
    let mut shields = [Shield::new(held.to_vec()), Shield::new(held.to_vec())];
    for (stream, buf) in rx {
        keep(shields[stream].push_bytes(&buf));
    }
    for shield in &mut shields {
        keep(shield.finish());
    }
    let exit = match child.wait() {
        Ok(st) => st.code().unwrap_or(-1),
        Err(e) => return Err(format!("the {} command did not finish: {e}", kind.word())),
    };
    Ok(RunResult {
        command: command.to_string(),
        exit,
        output,
        cut,
    })
}

/// Stop the test run or deploy in progress, and everything it started. False
/// when nothing is running. The stopped run ends by itself, with exit -1.
pub fn stop() -> bool {
    let current = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    match *current {
        Some(pid) => kill_tree(pid),
        None => false,
    }
}

/// Kill the run's whole process group: its shell and what the shell started.
#[cfg(unix)]
fn kill_tree(pid: u32) -> bool {
    Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|st| st.success())
}

/// Kill the run's shell and every process under it.
#[cfg(not(unix))]
fn kill_tree(pid: u32) -> bool {
    let mut c = Command::new("taskkill");
    c.args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_windows_console(&mut c);
    c.status().is_ok_and(|st| st.success())
}

/// Send what `pipe` gives, read by read, marked with its `stream` number.
fn spawn_pump<R: Read + Send + 'static>(
    stream: usize,
    mut pipe: R,
    tx: std::sync::mpsc::Sender<(usize, Vec<u8>)>,
) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send((stream, buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

/// Adds as much of `chunk` as fits under `cap` bytes, ending on a whole
/// character. True when some of it was left out.
fn push_capped(output: &mut String, chunk: &str, cap: usize) -> bool {
    let mut room = cap.saturating_sub(output.len()).min(chunk.len());
    while !chunk.is_char_boundary(room) {
        room -= 1;
    }
    output.push_str(&chunk[..room]);
    room < chunk.len()
}

/// The run in progress; dropping it lets the next run start.
struct Running(u32);

impl Drop for Running {
    fn drop(&mut self) {
        clear_current(self.0);
    }
}

fn clear_current(pid: u32) {
    if let Ok(mut current) = CURRENT.lock() {
        if *current == Some(pid) {
            *current = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_cap_ends_on_a_whole_character() {
        // The euro sign is three bytes; a cap of 8 falls inside the third one.
        let mut out = String::from("ab");
        assert!(push_capped(&mut out, "\u{20ac}\u{20ac}\u{20ac}", 8));
        assert_eq!(out, "ab\u{20ac}\u{20ac}");
        let mut out = String::from("ab");
        assert!(!push_capped(&mut out, "cd", 8));
        assert_eq!(out, "abcd");
        let mut full = String::from("abcdefgh");
        assert!(push_capped(&mut full, "i", 8));
        assert_eq!(full, "abcdefgh");
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-code-runs-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn store_check(config: &Path, kind: Kind, key: &str, command: &str, mode: Mode) -> Decision {
        check(config, kind, key, command, mode).unwrap()
    }

    #[test]
    fn first_run_of_cargo_test_needs_confirm_with_the_exact_command() {
        let config = temp_dir("first");
        let got = store_check(&config, Kind::Test, "repo-a", "cargo test", Mode::Auto);
        assert_eq!(
            got,
            Decision::NeedsConfirm {
                command: "cargo test".into()
            }
        );
    }

    #[test]
    fn after_confirm_the_same_command_is_remembered() {
        let config = temp_dir("remember");
        remember(&config, Kind::Test, "repo-a", "cargo test").unwrap();
        assert_eq!(
            store_check(&config, Kind::Test, "repo-a", "cargo test", Mode::Auto),
            Decision::Remembered
        );
    }

    #[test]
    fn a_changed_command_needs_confirm_again() {
        let config = temp_dir("changed");
        remember(&config, Kind::Test, "repo-a", "cargo test").unwrap();
        assert_eq!(
            store_check(
                &config,
                Kind::Test,
                "repo-a",
                "cargo test -- --ignored",
                Mode::Auto
            ),
            Decision::NeedsConfirm {
                command: "cargo test -- --ignored".into()
            }
        );
        // The original memory is still there until this new command is confirmed.
        assert_eq!(
            store_check(&config, Kind::Test, "repo-a", "cargo test", Mode::Auto),
            Decision::Remembered
        );
    }

    #[test]
    fn forget_clears_the_remembered_test_command() {
        let config = temp_dir("forget");
        remember(&config, Kind::Test, "repo-a", "cargo test").unwrap();
        forget(&config, Kind::Test, "repo-a").unwrap();
        assert_eq!(
            store_check(&config, Kind::Test, "repo-a", "cargo test", Mode::Auto),
            Decision::NeedsConfirm {
                command: "cargo test".into()
            }
        );
    }

    #[test]
    fn ask_mode_needs_confirm_even_when_the_command_is_remembered() {
        let config = temp_dir("ask");
        remember(&config, Kind::Test, "repo-a", "cargo test").unwrap();
        assert_eq!(
            store_check(&config, Kind::Test, "repo-a", "cargo test", Mode::Ask),
            Decision::NeedsConfirm {
                command: "cargo test".into()
            }
        );
        // Memory stays, so Auto can still use it.
        assert_eq!(
            store_check(&config, Kind::Test, "repo-a", "cargo test", Mode::Auto),
            Decision::Remembered
        );
    }

    #[test]
    fn deploy_kind_uses_the_same_remember_change_and_forget_rules() {
        let config = temp_dir("deploy");
        let key = "repo-b";
        let cmd = "npm run deploy";
        assert_eq!(
            store_check(&config, Kind::Deploy, key, cmd, Mode::Auto),
            Decision::NeedsConfirm {
                command: "npm run deploy".into()
            }
        );
        remember(&config, Kind::Deploy, key, cmd).unwrap();
        assert_eq!(
            store_check(&config, Kind::Deploy, key, cmd, Mode::Auto),
            Decision::Remembered
        );
        assert_eq!(
            store_check(
                &config,
                Kind::Deploy,
                key,
                "npm run deploy -- --prod",
                Mode::Auto
            ),
            Decision::NeedsConfirm {
                command: "npm run deploy -- --prod".into()
            }
        );
        forget(&config, Kind::Deploy, key).unwrap();
        assert_eq!(
            store_check(&config, Kind::Deploy, key, cmd, Mode::Auto),
            Decision::NeedsConfirm {
                command: "npm run deploy".into()
            }
        );
        // Forgetting deploy does not forget a test command on the same repo.
        remember(&config, Kind::Test, key, "cargo test").unwrap();
        forget(&config, Kind::Deploy, key).unwrap();
        assert_eq!(
            store_check(&config, Kind::Test, key, "cargo test", Mode::Auto),
            Decision::Remembered
        );
    }

    #[test]
    fn detect_cargo_toml_is_cargo_test() {
        let dir = temp_dir("cargo");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        assert_eq!(detect_test_command(&dir), "cargo test");
    }

    #[test]
    fn detect_package_json_with_pnpm_lock_is_pnpm_test() {
        let dir = temp_dir("pnpm");
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"test":"vitest run"}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();
        assert_eq!(detect_test_command(&dir), "pnpm test");
    }

    #[test]
    fn detect_package_json_without_a_test_script_is_empty() {
        let dir = temp_dir("npm-empty");
        std::fs::write(dir.join("package.json"), r#"{"name":"demo"}"#).unwrap();
        assert_eq!(detect_test_command(&dir), "");
    }

    #[test]
    fn detect_go_mod_is_go_test() {
        let dir = temp_dir("go");
        std::fs::write(dir.join("go.mod"), "module example.com/demo\n").unwrap();
        assert_eq!(detect_test_command(&dir), "go test ./...");
    }

    #[test]
    fn repo_key_is_the_sha256_of_the_remote_and_normalized_root() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-code-runs-rkey-{}-missing",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!dir.exists());
        let norm = dir.to_string_lossy().replace('\\', "/");
        let input = format!("https://github.com/ex/app.git\n{norm}");
        let expected = sha256_hex(input.as_bytes());
        assert_eq!(
            repo_key(&dir, Some("https://github.com/ex/app.git")),
            expected
        );
        assert_eq!(expected.len(), 64);
        assert_ne!(repo_key(&dir, None), expected);
    }

    #[test]
    fn run_without_confirm_is_refused_until_confirmed() {
        let config = temp_dir("gate");
        let work = temp_dir("gate-work");
        let err = run(&config, &work, "echo already-ok", false, Mode::Auto, |_| {}).unwrap_err();
        assert_eq!(err, "This command needs a confirmation first.");
        let out = run(&config, &work, "echo already-ok", true, Mode::Auto, |_| {}).unwrap();
        assert_eq!(out.command, "echo already-ok");
        assert_eq!(out.exit, 0);
        assert!(out.output.contains("already-ok"), "{}", out.output);
        // Second run does not need confirm.
        let again = run(&config, &work, "echo already-ok", false, Mode::Auto, |_| {}).unwrap();
        assert_eq!(again.exit, 0);
    }

    #[test]
    fn run_redacts_a_github_token_in_the_output() {
        let config = temp_dir("redact");
        let work = temp_dir("redact-work");
        // A token-shaped value the shared redactor is known to scrub.
        let token = "ghp_abcdefghijklmnopqrstuv";
        #[cfg(windows)]
        let command = format!("echo token={token}");
        #[cfg(not(windows))]
        let command = format!("printf 'token={token}\\n'");
        let mut chunks = String::new();
        let out = run(&config, &work, &command, true, Mode::Auto, |c| {
            chunks.push_str(c)
        })
        .unwrap();
        assert!(!out.output.contains(token), "{}", out.output);
        assert!(!chunks.contains(token), "{chunks}");
        assert!(
            out.output.contains("token=[redacted]") || out.output.contains("[redacted]"),
            "{}",
            out.output
        );
    }

    #[test]
    fn a_second_run_starts_once_the_first_has_ended() {
        let work = temp_dir("twice-work");
        let one = exec(&work, "echo one", Kind::Test, &[], None, &mut |_: &str| {}).unwrap();
        assert_eq!(one.exit, 0);
        assert_eq!(one.output.lines().collect::<Vec<_>>(), ["one"]);
        assert_eq!(*CURRENT.lock().unwrap(), None);
        let two = exec(&work, "echo two", Kind::Test, &[], None, &mut |_: &str| {}).unwrap();
        assert_eq!(two.exit, 0);
        assert_eq!(two.output.lines().collect::<Vec<_>>(), ["two"]);
        assert_eq!(*CURRENT.lock().unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn nothing_is_kept_or_sent_after_the_output_cap_cuts() {
        let work = temp_dir("cap-work");
        // 87,381 three-byte lines leave one byte of room under the cap, so
        // the euro sign is the chunk that doesn't fit. The command prints its
        // last two bytes only once that chunk has been sent on.
        let command = "yes ab | head -c 262143; printf '\\342\\202\\254'; \
            i=0; while [ ! -e sent ] && [ $i -lt 100 ]; do sleep 0.1; i=$((i+1)); done; \
            printf zz";
        let mark = work.join("sent");
        let mut sent = String::new();
        let out = exec(&work, command, Kind::Test, &[], None, &mut |chunk: &str| {
            sent.push_str(chunk);
            if chunk.contains(['\u{20ac}', '\u{fffd}']) {
                std::fs::write(&mark, "").unwrap();
            }
        })
        .unwrap();
        assert_eq!(out.exit, 0);
        assert!(out.cut);
        assert_eq!(out.output.len(), 262_143);
        assert!(out.output.ends_with("ab\nab\n"));
        assert!(!sent.contains('z'), "a chunk was sent after the cut");
    }

    #[cfg(unix)]
    #[test]
    fn a_key_that_falls_across_two_reads_is_hidden_whole() {
        let config = temp_dir("split");
        let work = temp_dir("split-work");
        // The shell writes 8192 bytes, then the rest: the cut lands 13
        // characters into the key.
        let key = concat!("ghp_abcdefghijklm", "nopqrstuvwxyz0123456789");
        std::fs::write(
            work.join("t.sh"),
            format!("printf '%s key=%s end\\n' \"$(head -c 8170 /dev/zero | tr '\\0' a)\" {key}\n"),
        )
        .unwrap();
        let mut chunks = String::new();
        let out = run(&config, &work, "sh t.sh", true, Mode::Auto, |c| {
            chunks.push_str(c)
        })
        .unwrap();
        let expected = format!("{} key=[redacted] end\n", "a".repeat(8170));
        assert_eq!(out.output, expected);
        assert_eq!(chunks, expected);
    }

    #[cfg(unix)]
    #[test]
    fn a_character_that_falls_across_two_reads_stays_whole() {
        let config = temp_dir("utf8");
        let work = temp_dir("utf8-work");
        // 8191 bytes, then a three-byte euro sign across byte 8192.
        std::fs::write(
            work.join("t.sh"),
            "printf '%s\\342\\202\\254\\n' \"$(head -c 8191 /dev/zero | tr '\\0' a)\"\n",
        )
        .unwrap();
        let out = run(&config, &work, "sh t.sh", true, Mode::Auto, |_| {}).unwrap();
        assert_eq!(out.output, format!("{}\u{20ac}\n", "a".repeat(8191)));
    }

    #[cfg(unix)]
    #[test]
    fn two_runs_started_together_never_both_run() {
        let work = temp_dir("pair-work");
        for round in 0..20 {
            let gate = std::sync::Barrier::new(2);
            let results: Vec<Result<RunResult, String>> = std::thread::scope(|s| {
                let both: Vec<_> = (0..2)
                    .map(|_| {
                        s.spawn(|| {
                            gate.wait();
                            exec(
                                &work,
                                "sleep 0.3; echo x",
                                Kind::Test,
                                &[],
                                None,
                                &mut |_: &str| {},
                            )
                        })
                    })
                    .collect();
                both.into_iter().map(|h| h.join().unwrap()).collect()
            });
            let ran = results
                .iter()
                .filter(|r| r.as_ref().is_ok_and(|done| done.output == "x\n"))
                .count();
            let refused = results
                .iter()
                .filter(|r| r.as_ref().err().map(String::as_str) == Some(BUSY))
                .count();
            assert_eq!((ran, refused), (1, 1), "round {round}: {results:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn stop_ends_a_run_and_everything_it_started() {
        let work = temp_dir("stop-work");
        // Nothing is running yet.
        assert!(!stop());
        let up = std::sync::atomic::AtomicBool::new(false);
        let started = std::time::Instant::now();
        let (stopped, result) = std::thread::scope(|s| {
            // The background sleep holds the output open after the shell is
            // gone, so the run ends early only if that sleep is killed too.
            let run = s.spawn(|| {
                exec(
                    &work,
                    "sleep 8 & echo up; sleep 8",
                    Kind::Test,
                    &[],
                    None,
                    &mut |chunk: &str| {
                        if chunk.contains("up") {
                            up.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                    },
                )
            });
            while !up.load(std::sync::atomic::Ordering::SeqCst) && !run.is_finished() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            (stop(), run.join().unwrap())
        });
        assert!(stopped);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(4),
            "the run went on for {:?}",
            started.elapsed()
        );
        let done = result.unwrap();
        assert_eq!((done.exit, done.output.as_str()), (-1, "up\n"));
        assert_eq!(*CURRENT.lock().unwrap(), None);
        // A new run starts at once, and there is nothing left to stop.
        let next = exec(&work, "echo next", Kind::Test, &[], None, &mut |_: &str| {}).unwrap();
        assert_eq!(next.output, "next\n");
        assert!(!stop());
    }

    #[test]
    fn empty_command_is_refused() {
        let config = temp_dir("empty");
        let work = temp_dir("empty-work");
        assert_eq!(
            run(&config, &work, "   ", true, Mode::Auto, |_| {}).unwrap_err(),
            "Set a test command first."
        );
    }
}

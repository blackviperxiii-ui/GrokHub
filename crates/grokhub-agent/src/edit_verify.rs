//! Edit-test-fix verification loop before a coding turn reports done (Order 24).
//!
//! When a root coding turn modifies source files and tries to finish without a
//! passing test or check run covering its latest edits, the runner executes the
//! detected workspace check inside the OS sandbox, feeds any failure back into
//! the conversation so the model can fix it, and bounds retries at
//! [`MAX_ATTEMPTS`].

use std::path::Path;

use serde_json::Value;

use crate::tools::ToolOutput;

/// Maximum automatic verify-fix attempts per coding turn before stopping.
pub const MAX_ATTEMPTS: u8 = 3;

/// Default timeout per sandboxed verification run (milliseconds).
pub const VERIFY_TIMEOUT_MS: u64 = 120_000;

/// Maximum characters of failure output included in a retry cabin note.
const NOTE_OUTPUT_MAX: usize = 3_000;

/// Extensions treated as code/build inputs that warrant verification on edit.
const CODE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "c", "cpp", "cc", "h", "hpp", "java",
    "kt", "rb", "sh", "ps1",
];

/// Manifest/config filenames whose edits affect build or test behavior.
const CODE_MANIFESTS: &[&str] = &[
    "cargo.toml",
    "package.json",
    "tsconfig.json",
    "pyproject.toml",
    "go.mod",
];

/// Concrete verification command discovered for a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyCommand {
    pub kind: &'static str,
    pub command: String,
    pub label: String,
}

/// Result of one automatic verification step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    Passed {
        command: String,
    },
    Failed {
        command: String,
        output: String,
        attempt: u8,
    },
    Exhausted {
        command: String,
        output: String,
        attempts: u8,
    },
}

/// Per-turn state tracking code edits, manual checks, and bounded retries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditVerifyState {
    edit_gen: u64,
    verified_gen: u64,
    attempts: u8,
    edited_paths: Vec<String>,
    last_outcome: Option<VerifyOutcome>,
}

impl EditVerifyState {
    /// Records a successful write/edit tool call. Non-code files are ignored.
    pub fn note_edit(&mut self, path: &str) {
        let trimmed = path.trim();
        if trimmed.is_empty() || !is_code_path(trimmed) {
            return;
        }
        if !self.edited_paths.iter().any(|p| p == trimmed) {
            self.edited_paths.push(trimmed.to_string());
        }
        self.edit_gen += 1;
    }

    /// Records a manual shell command executed by the model during the turn.
    /// A passing test/check command satisfies verification for edits made so far.
    pub fn note_manual_command(&mut self, command: &str, ok: bool) {
        if ok && self.edit_gen > 0 && is_verify_command(command) {
            self.verified_gen = self.edit_gen;
            self.last_outcome = Some(VerifyOutcome::Passed {
                command: command.trim().to_string(),
            });
        }
    }

    /// True when code edits remain unverified and attempts remain under [`MAX_ATTEMPTS`].
    pub fn needs_verify(&self) -> bool {
        self.edit_gen > self.verified_gen && self.attempts < MAX_ATTEMPTS
    }

    /// Number of failed automatic verify attempts recorded so far.
    pub fn attempts(&self) -> u8 {
        self.attempts
    }

    /// True once automatic verify attempts reached [`MAX_ATTEMPTS`] without passing.
    pub fn exhausted(&self) -> bool {
        self.edit_gen > self.verified_gen && self.attempts >= MAX_ATTEMPTS
    }

    /// Most recent verification outcome recorded in this turn.
    pub fn last_outcome(&self) -> Option<&VerifyOutcome> {
        self.last_outcome.as_ref()
    }

    /// Records an automatic verification execution result and returns the classified outcome.
    pub fn record_result(&mut self, command: &str, ok: bool, output: &str) -> VerifyOutcome {
        let cmd = command.trim().to_string();
        let outcome = if ok {
            self.verified_gen = self.edit_gen;
            VerifyOutcome::Passed { command: cmd }
        } else {
            self.attempts = self.attempts.saturating_add(1);
            let trimmed_out = clip_output(output, NOTE_OUTPUT_MAX);
            if self.attempts >= MAX_ATTEMPTS {
                VerifyOutcome::Exhausted {
                    command: cmd,
                    output: trimmed_out,
                    attempts: self.attempts,
                }
            } else {
                VerifyOutcome::Failed {
                    command: cmd,
                    output: trimmed_out,
                    attempt: self.attempts,
                }
            }
        };
        self.last_outcome = Some(outcome.clone());
        outcome
    }

    /// Builds a bounded retry instruction for the model when verification failed.
    pub fn failure_note(outcome: &VerifyOutcome) -> Option<String> {
        match outcome {
            VerifyOutcome::Failed {
                command,
                output,
                attempt,
            } => Some(format!(
                "Verification failed before completing your coding turn \
                 (attempt {attempt} of {MAX_ATTEMPTS}).\n\
                 Command: `{command}`\n\
                 Output:\n```\n{output}\n```\n\
                 Fix the failing code or build error, then finish so verification can re-run."
            )),
            VerifyOutcome::Passed { .. } | VerifyOutcome::Exhausted { .. } => None,
        }
    }
}

/// Returns `true` when `path` is a source code file or build/test manifest.
pub fn is_code_path(path: &str) -> bool {
    let norm = path.trim().replace('\\', "/").to_ascii_lowercase();
    let file_name = norm.rsplit('/').next().unwrap_or(norm.as_str());
    if CODE_MANIFESTS.contains(&file_name) {
        return true;
    }
    let Some((_, ext)) = file_name.rsplit_once('.') else {
        return false;
    };
    CODE_EXTENSIONS.contains(&ext)
}

/// Heuristic check for whether a shell command already runs tests or builds.
pub fn is_verify_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let trimmed = lower.trim();
    if trimmed.is_empty() {
        return false;
    }
    const PREFIXES: &[&str] = &[
        "cargo test",
        "cargo nextest",
        "cargo check",
        "cargo clippy",
        "cargo build",
        "rustc ",
        "npm test",
        "npm run test",
        "npm run check",
        "npm run typecheck",
        "pnpm test",
        "pnpm run test",
        "yarn test",
        "bun test",
        "npx vitest",
        "npx jest",
        "npx tsc",
        "vitest",
        "jest",
        "pytest",
        "python -m pytest",
        "python3 -m pytest",
        "py -m pytest",
        "go test",
        "go build",
    ];
    trimmed
        .split(['&', ';', '|'])
        .map(str::trim)
        .filter(|seg| !seg.is_empty())
        .any(|seg| PREFIXES.iter().any(|p| seg.starts_with(p)))
}

/// Detects the most targeted sandboxed test or build command for `workspace`
/// given the files modified in the current turn.
pub fn detect_command(workspace: &Path, modified_paths: &[String]) -> Option<VerifyCommand> {
    if !modified_paths.iter().any(|p| is_code_path(p)) {
        return None;
    }

    // 1. Standalone single-task Rust fixture (Eval v1 layout: `tests/check.rs` without Cargo.toml).
    if workspace.join("tests").join("check.rs").is_file()
        && !workspace.join("Cargo.toml").is_file()
        && modified_paths.iter().any(|p| has_ext(p, "rs"))
    {
        const WIN_RUSTC_CMD: &str = "rustc --edition 2021 --test --color never tests/check.rs -o eval-verify-bin.exe; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; .\\eval-verify-bin.exe; $rc = $LASTEXITCODE; Remove-Item -Force eval-verify-bin.exe -ErrorAction SilentlyContinue; exit $rc";
        const UNIX_RUSTC_CMD: &str = "rustc --edition 2021 --test --color never tests/check.rs -o ./eval-verify-bin && { ./eval-verify-bin; rc=$?; rm -f ./eval-verify-bin; exit $rc; }";
        let command = if cfg!(windows) {
            WIN_RUSTC_CMD
        } else {
            UNIX_RUSTC_CMD
        };
        return Some(VerifyCommand {
            kind: "rustc-test",
            command: command.to_string(),
            label: "rustc --test tests/check.rs".to_string(),
        });
    }

    // 2. Cargo workspace or package.
    if workspace.join("Cargo.toml").is_file()
        && modified_paths
            .iter()
            .any(|p| has_ext(p, "rs") || file_name_eq(p, "cargo.toml"))
    {
        if let Some(pkg) = single_workspace_crate(workspace, modified_paths) {
            return Some(VerifyCommand {
                kind: "cargo-crate",
                command: format!("cargo test -p {pkg}"),
                label: format!("cargo test -p {pkg}"),
            });
        }
        return Some(VerifyCommand {
            kind: "cargo",
            command: "cargo test".to_string(),
            label: "cargo test".to_string(),
        });
    }

    // 3. Node / TypeScript project.
    let pkg_json = workspace.join("package.json");
    if pkg_json.is_file()
        && modified_paths.iter().any(|p| {
            has_ext(p, "ts")
                || has_ext(p, "tsx")
                || has_ext(p, "js")
                || has_ext(p, "jsx")
                || has_ext(p, "mjs")
                || has_ext(p, "cjs")
                || file_name_eq(p, "package.json")
                || file_name_eq(p, "tsconfig.json")
        })
    {
        if has_real_npm_test(&pkg_json) {
            return Some(VerifyCommand {
                kind: "npm-test",
                command: "npm test".to_string(),
                label: "npm test".to_string(),
            });
        }
        if workspace.join("tsconfig.json").is_file() {
            return Some(VerifyCommand {
                kind: "tsc",
                command: "npx tsc --noEmit".to_string(),
                label: "npx tsc --noEmit".to_string(),
            });
        }
    }

    // 4. Python project.
    if modified_paths.iter().any(|p| has_ext(p, "py"))
        && (workspace.join("pyproject.toml").is_file()
            || workspace.join("pytest.ini").is_file()
            || workspace.join("setup.cfg").is_file()
            || workspace.join("tests").is_dir())
    {
        return Some(VerifyCommand {
            kind: "pytest",
            command: "pytest -q".to_string(),
            label: "pytest -q".to_string(),
        });
    }

    // 5. Go module.
    if workspace.join("go.mod").is_file() && modified_paths.iter().any(|p| has_ext(p, "go")) {
        return Some(VerifyCommand {
            kind: "go-test",
            command: "go test ./...".to_string(),
            label: "go test ./...".to_string(),
        });
    }

    None
}

fn single_workspace_crate(workspace: &Path, modified_paths: &[String]) -> Option<String> {
    let mut found: Option<String> = None;
    for raw in modified_paths {
        if !has_ext(raw, "rs") && !file_name_eq(raw, "cargo.toml") {
            continue;
        }
        let norm = raw.trim().replace('\\', "/");
        let trimmed = norm.strip_prefix("./").unwrap_or(&norm);
        let rest = trimmed.strip_prefix("crates/")?;
        let pkg = rest.split('/').next()?.trim();
        if pkg.is_empty()
            || !workspace
                .join("crates")
                .join(pkg)
                .join("Cargo.toml")
                .is_file()
        {
            return None;
        }
        match &found {
            Some(prev) if prev != pkg => return None,
            Some(_) => {}
            None => found = Some(pkg.to_string()),
        }
    }
    found
}

fn has_real_npm_test(package_json: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(package_json) else {
        return false;
    };
    let Ok(val) = serde_json::from_str::<Value>(&raw) else {
        return false;
    };
    let Some(script) = val
        .get("scripts")
        .and_then(|s| s.get("test"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    let trimmed = script.trim();
    !trimmed.is_empty() && !trimmed.to_ascii_lowercase().contains("no test specified")
}

fn has_ext(path: &str, ext: &str) -> bool {
    let norm = path.trim().replace('\\', "/").to_ascii_lowercase();
    let file_name = norm.rsplit('/').next().unwrap_or(norm.as_str());
    file_name.rsplit_once('.').is_some_and(|(_, e)| e == ext)
}

fn file_name_eq(path: &str, expected_lower: &str) -> bool {
    let norm = path.trim().replace('\\', "/").to_ascii_lowercase();
    let file_name = norm.rsplit('/').next().unwrap_or(norm.as_str());
    file_name == expected_lower
}

fn clip_output(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let total = trimmed.chars().count();
    if total <= max_chars {
        return trimmed.to_string();
    }
    let head_len = max_chars / 2;
    let tail_len = max_chars.saturating_sub(head_len + 8);
    let head: String = trimmed.chars().take(head_len).collect();
    let tail: String = trimmed.chars().skip(total - tail_len).collect();
    format!("{head}\n…\n{tail}")
}

/// Runs `cmd` inside the OS sandbox via `run_terminal_command` (`outside_sandbox: false`).
pub fn run_sandboxed(
    workspace: &Path,
    session: Option<&str>,
    cmd: &VerifyCommand,
    stop: &dyn Fn() -> bool,
) -> ToolOutput {
    #[cfg(test)]
    if let Some(mock) = next_test_override(workspace, cmd) {
        return mock;
    }
    let args = serde_json::json!({
        "command": cmd.command,
        "timeout": VERIFY_TIMEOUT_MS,
        "outside_sandbox": false,
    });
    crate::tools::shell::run_adoptable(workspace, session, &args, stop, None)
}

#[cfg(test)]
type TestRunner = Box<dyn FnMut(&Path, &VerifyCommand) -> ToolOutput + Send>;

#[cfg(test)]
std::thread_local! {
    static TEST_RUNNER: std::cell::RefCell<Option<TestRunner>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn next_test_override(workspace: &Path, cmd: &VerifyCommand) -> Option<ToolOutput> {
    TEST_RUNNER.with(|slot| {
        let mut guard = slot.borrow_mut();
        guard.as_mut().map(|runner| runner(workspace, cmd))
    })
}

/// Scoped thread-local override for [`run_sandboxed`] in deterministic unit tests.
#[cfg(test)]
pub(crate) struct TestRunnerGuard(Option<TestRunner>);

#[cfg(test)]
impl TestRunnerGuard {
    pub(crate) fn set(
        runner: impl FnMut(&Path, &VerifyCommand) -> ToolOutput + Send + 'static,
    ) -> Self {
        let prev = TEST_RUNNER.with(|slot| slot.borrow_mut().replace(Box::new(runner)));
        Self(prev)
    }
}

#[cfg(test)]
impl Drop for TestRunnerGuard {
    fn drop(&mut self) {
        TEST_RUNNER.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ws(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-edit-verify-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detect_eval_v1_layout_uses_rustc_test() {
        let dir = temp_ws("eval-v1");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("src/bug.rs"), "pub fn f() -> i32 { 1 }\n").unwrap();
        std::fs::write(dir.join("tests/check.rs"), "#[test] fn t() {}\n").unwrap();

        let cmd = detect_command(&dir, &["src/bug.rs".to_string()]).expect("eval v1 detected");
        assert_eq!(cmd.kind, "rustc-test");
        assert!(cmd.command.contains("tests/check.rs"), "{}", cmd.command);
        assert!(cmd.command.contains("rustc"), "{}", cmd.command);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_cargo_targets_single_workspace_crate_or_whole_workspace() {
        let dir = temp_ws("cargo");
        std::fs::create_dir_all(dir.join("crates/alpha/src")).unwrap();
        std::fs::create_dir_all(dir.join("crates/beta/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers=[\"crates/*\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("crates/alpha/Cargo.toml"),
            "[package]\nname=\"alpha\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("crates/beta/Cargo.toml"),
            "[package]\nname=\"beta\"\n",
        )
        .unwrap();

        let single = detect_command(&dir, &["crates/alpha/src/lib.rs".to_string()]).unwrap();
        assert_eq!(single.command, "cargo test -p alpha");

        let both = detect_command(
            &dir,
            &[
                "crates/alpha/src/lib.rs".to_string(),
                "crates/beta/src/lib.rs".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(both.command, "cargo test");

        let docs_only = detect_command(&dir, &["README.md".to_string()]);
        assert!(docs_only.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_node_python_go_and_empty_workspaces() {
        let node = temp_ws("node");
        std::fs::write(
            node.join("package.json"),
            r#"{"scripts":{"test":"vitest run"}}"#,
        )
        .unwrap();
        let npm_cmd = detect_command(&node, &["src/app.ts".to_string()]).unwrap();
        assert_eq!(npm_cmd.command, "npm test");

        std::fs::write(
            node.join("package.json"),
            r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#,
        )
        .unwrap();
        std::fs::write(node.join("tsconfig.json"), "{}").unwrap();
        let tsc_cmd = detect_command(&node, &["src/app.ts".to_string()]).unwrap();
        assert_eq!(tsc_cmd.command, "npx tsc --noEmit");
        let _ = std::fs::remove_dir_all(&node);

        let py = temp_ws("py");
        std::fs::write(py.join("pyproject.toml"), "[project]\nname=\"x\"\n").unwrap();
        let py_cmd = detect_command(&py, &["app.py".to_string()]).unwrap();
        assert_eq!(py_cmd.command, "pytest -q");
        let _ = std::fs::remove_dir_all(&py);

        let go = temp_ws("go");
        std::fs::write(go.join("go.mod"), "module example.com/m\n").unwrap();
        let go_cmd = detect_command(&go, &["main.go".to_string()]).unwrap();
        assert_eq!(go_cmd.command, "go test ./...");
        let _ = std::fs::remove_dir_all(&go);

        let plain = temp_ws("plain");
        assert!(detect_command(&plain, &["notes.rs".to_string()]).is_none());
        let _ = std::fs::remove_dir_all(&plain);
    }

    #[test]
    fn state_tracks_manual_runs_rearms_on_new_edit_and_bounds_retries() {
        let mut st = EditVerifyState::default();
        assert!(!st.needs_verify());

        st.note_edit("README.md");
        assert!(!st.needs_verify());

        st.note_edit("src/lib.rs");
        assert!(st.needs_verify());

        // Passing manual verify satisfies current edit generation.
        st.note_manual_command("cargo test -p grokhub-agent", true);
        assert!(!st.needs_verify());

        // New edit re-arms verification.
        st.note_edit("src/lib.rs");
        assert!(st.needs_verify());

        let o1 = st.record_result("cargo test", false, "assertion failed 1");
        assert!(matches!(o1, VerifyOutcome::Failed { attempt: 1, .. }));
        assert!(EditVerifyState::failure_note(&o1).is_some());
        assert!(st.needs_verify());

        let o2 = st.record_result("cargo test", false, "assertion failed 2");
        assert!(matches!(o2, VerifyOutcome::Failed { attempt: 2, .. }));
        assert!(st.needs_verify());

        let o3 = st.record_result("cargo test", false, "assertion failed 3");
        assert!(matches!(o3, VerifyOutcome::Exhausted { attempts: 3, .. }));
        assert!(!st.needs_verify());
        assert!(st.exhausted());
        assert!(EditVerifyState::failure_note(&o3).is_none());
    }

    #[test]
    fn run_sandboxed_executes_inside_sandbox() {
        let dir = temp_ws("sandbox-exec");
        let cmd = VerifyCommand {
            kind: "shell-check",
            command: "echo sandbox-verify-ok".to_string(),
            label: "echo".to_string(),
        };
        let out = run_sandboxed(&dir, None, &cmd, &|| false);
        assert!(!out.failed, "{}", out.text);
        assert!(out.text.contains("sandbox-verify-ok"), "{}", out.text);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

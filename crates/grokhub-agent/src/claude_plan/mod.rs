//! Claude (your plan): chats run by the official Claude Code CLI on this
//! computer, signed in through Claude Code's own `claude auth login`. This
//! file finds the CLI and asks it whether it's signed in. It never reads,
//! copies or keeps a Claude credential: from `claude auth status` only the
//! `authMethod` field is read, and the rest of the reply is dropped.
//! Design: `docs/design/claude-plan/README.md`.

pub mod ask;
pub mod engine;
pub mod stream;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Serialize;

/// `claude --version` and `claude auth status` answer at once; a CLI that
/// hangs longer than this is treated as not answering.
const ASK_WITHIN: Duration = Duration::from_secs(10);
/// Both replies are a line or a small JSON object.
const REPLY_CAP: u64 = 16 * 1024;

/// What this computer's Claude Code can do for chats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ClaudePlan {
    /// No `claude` on this computer.
    Missing,
    /// A `claude` file is there, but `claude --version` didn't answer (it
    /// failed, hung, or isn't a program this computer can run).
    NoAnswer,
    /// Installed, but signed out, or `claude auth status` didn't answer.
    SignedOut { version: String },
    /// Signed in with a Claude plan: chats count against the plan.
    Plan { version: String },
    /// Signed in another way (an API key, a token, a cloud provider): chats
    /// are billed there, not to a plan.
    OtherBilling { version: String, method: String },
}

/// The `claude` file names to look for, in order.
fn names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    }
}

/// Where the installers put `claude` when the app (started from the desktop)
/// doesn't see the shell's `PATH`. Includes Claude Desktop's versioned
/// `claude-code` / `claude-code-vm` folders (newest first; host before VM).
fn usual_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![
        home.join(".local").join("bin"),
        home.join(".claude").join("local"),
    ];
    if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    dirs.extend(desktop_version_dirs(home));
    dirs
}

/// Claude Desktop user-data roots. Platform preference first; the others are
/// checked too so a test home with `.config/Claude` works on every CI OS.
fn desktop_claude_roots(home: &Path) -> Vec<PathBuf> {
    let linux = home.join(".config").join("Claude");
    let macos = home.join("Library").join("Application Support").join("Claude");
    let windows = home.join("AppData").join("Roaming").join("Claude");
    if cfg!(windows) {
        vec![windows, linux, macos]
    } else if cfg!(target_os = "macos") {
        vec![macos, linux, windows]
    } else {
        vec![linux, macos, windows]
    }
}

/// Numeric parts of a version folder name (`"2.1.330"` → `[2, 1, 330]`).
fn version_key(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// Version folders under one Desktop kind dir, newest first. Each version
/// directory is included; so is any immediate child that holds a `claude`
/// binary (covers a nested layout under `<ver>/…/claude`).
fn version_dirs_newest_first(kind_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(kind_dir) else {
        return Vec::new();
    };
    let mut vers: Vec<(Vec<u64>, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (version_key(&name), e.path())
        })
        .collect();
    vers.sort_by(|a, b| b.0.cmp(&a.0));
    let mut dirs = Vec::with_capacity(vers.len());
    for (_, path) in vers {
        dirs.push(path.clone());
        if let Ok(children) = std::fs::read_dir(&path) {
            for child in children.filter_map(|c| c.ok()) {
                if !child.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let child_path = child.path();
                if names().iter().any(|n| child_path.join(n).is_file()) {
                    dirs.push(child_path);
                }
            }
        }
    }
    dirs
}

/// Desktop-managed Claude Code install dirs: `claude-code/<ver>/…` then
/// `claude-code-vm/<ver>/…`, newest version first within each kind.
fn desktop_version_dirs(home: &Path) -> Vec<PathBuf> {
    const KINDS: &[&str] = &["claude-code", "claude-code-vm"];
    let mut dirs = Vec::new();
    for root in desktop_claude_roots(home) {
        for kind in KINDS {
            dirs.extend(version_dirs_newest_first(&root.join(kind)));
        }
    }
    dirs
}

/// The first `claude` in `path_dirs`, then in the usual install folders
/// under `home`. Relative `PATH` entries are skipped: they'd mean whatever
/// folder the app was started from.
pub fn find_in(
    path_dirs: impl IntoIterator<Item = PathBuf>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = path_dirs.into_iter().filter(|d| d.is_absolute()).collect();
    if let Some(home) = home {
        dirs.extend(usual_dirs(home));
    }
    dirs.iter()
        .flat_map(|dir| names().iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// `claude` on this computer, if any.
pub fn find() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    find_in(std::env::split_paths(&path), home_dir().as_deref())
}

fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// The version from `claude --version` ("2.1.300 (Claude Code)" gives
/// "2.1.300"); `None` when the first word doesn't start with a digit.
pub fn parse_version(stdout: &str) -> Option<String> {
    let word = stdout.lines().next()?.split_whitespace().next()?;
    (word.starts_with(|c: char| c.is_ascii_digit()) && word.len() <= 32).then(|| word.to_string())
}

/// The sign-in kind from `claude auth status`'s JSON. Only `authMethod` is
/// read. `claude.ai` (signed in through `claude auth login`) and
/// `oauth_token` (a `claude setup-token` token, which needs a plan) are a
/// plan; `none` or a missing or unreadable field is signed out; anything else
/// (an API key, a key helper, a cloud provider) is billed elsewhere.
pub fn parse_auth(stdout: &str, version: String) -> ClaudePlan {
    #[derive(serde::Deserialize)]
    struct Status {
        #[serde(rename = "authMethod")]
        auth_method: Option<String>,
    }
    let method = serde_json::from_str::<Status>(stdout.trim())
        .ok()
        .and_then(|s| s.auth_method)
        .filter(|m| !m.is_empty() && m.len() <= 32);
    match method.as_deref() {
        Some("claude.ai" | "oauth_token") => ClaudePlan::Plan { version },
        None | Some("none") => ClaudePlan::SignedOut { version },
        Some(other) => ClaudePlan::OtherBilling {
            version,
            method: other.to_string(),
        },
    }
}

fn ask(cli: &Path, args: &[&str], cwd: Option<&Path>) -> Option<String> {
    let mut cmd = Command::new(cli);
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    grokhub_core::proc_util::output_within(&mut cmd, ASK_WITHIN, REPLY_CAP)
}

/// Ask `cli` for its version and sign-in. `claude auth status` exits 1 when
/// signed out, which reads as no reply, so both are signed out.
pub fn detect_at(cli: &Path) -> ClaudePlan {
    detect_with(cli, None)
}

/// [`detect_at`] from `cwd`, a chat's folder: a project's own Claude Code
/// settings there can sign it in another way (an API key in their `env`),
/// so a chat's turn asks from the folder the turn runs in.
pub fn detect_in(cli: &Path, cwd: &Path) -> ClaudePlan {
    detect_with(cli, Some(cwd))
}

fn detect_with(cli: &Path, cwd: Option<&Path>) -> ClaudePlan {
    let Some(version) = ask(cli, &["--version"], cwd)
        .as_deref()
        .and_then(parse_version)
    else {
        return if cli.is_file() {
            ClaudePlan::NoAnswer
        } else {
            ClaudePlan::Missing
        };
    };
    match ask(cli, &["auth", "status"], cwd) {
        Some(json) => parse_auth(&json, version),
        None => ClaudePlan::SignedOut { version },
    }
}

/// This computer's Claude Code, found and asked.
pub fn detect() -> ClaudePlan {
    find().map_or(ClaudePlan::Missing, |cli| detect_at(&cli))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test, removed first so a rerun starts empty.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-claude-plan-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_version_is_the_first_word_when_it_is_a_number() {
        assert_eq!(
            parse_version("2.1.300 (Claude Code)\n").as_deref(),
            Some("2.1.300")
        );
        assert_eq!(parse_version("2.1.300\n").as_deref(), Some("2.1.300"));
        assert_eq!(parse_version("command not found"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn only_the_auth_method_decides_the_billing() {
        let v = || "2.1.300".to_string();
        assert_eq!(
            parse_auth(
                r#"{"loggedIn":true,"authMethod":"claude.ai","email":"user@example.com"}"#,
                v()
            ),
            ClaudePlan::Plan { version: v() }
        );
        assert_eq!(
            parse_auth(r#"{"authMethod":"oauth_token"}"#, v()),
            ClaudePlan::Plan { version: v() }
        );
        assert_eq!(
            parse_auth(r#"{"authMethod":"api_key"}"#, v()),
            ClaudePlan::OtherBilling {
                version: v(),
                method: "api_key".into()
            }
        );
        assert_eq!(
            parse_auth(r#"{"authMethod":"none"}"#, v()),
            ClaudePlan::SignedOut { version: v() }
        );
        assert_eq!(
            parse_auth("Not logged in", v()),
            ClaudePlan::SignedOut { version: v() }
        );
        assert_eq!(
            parse_auth("{}", v()),
            ClaudePlan::SignedOut { version: v() }
        );
    }

    #[test]
    fn what_reaches_the_page_never_carries_the_rest_of_the_status() {
        let plan = parse_auth(
            r#"{"authMethod":"claude.ai","email":"user@example.com","orgName":"my-org"}"#,
            "2.1.300".into(),
        );
        assert_eq!(
            serde_json::to_string(&plan).unwrap(),
            r#"{"state":"plan","version":"2.1.300"}"#
        );
    }

    #[test]
    fn finds_claude_on_path_first_then_in_the_usual_folders() {
        let root = scratch("find");
        let on_path = root.join("bin");
        let home = root.join("home");
        let local = home.join(".local").join("bin");
        std::fs::create_dir_all(&on_path).unwrap();
        std::fs::create_dir_all(&local).unwrap();
        let name = names()[0];
        assert_eq!(find_in([on_path.clone()], Some(&home)), None);
        std::fs::write(local.join(name), "").unwrap();
        assert_eq!(
            find_in([on_path.clone()], Some(&home)),
            Some(local.join(name))
        );
        std::fs::write(on_path.join(name), "").unwrap();
        assert_eq!(
            find_in([on_path.clone()], Some(&home)),
            Some(on_path.join(name))
        );
        assert_eq!(find_in([PathBuf::from("bin")], None), None);
    }

    /// Empty `PATH` dirs so only home-based usual / Desktop dirs can match.
    fn empty_path() -> [PathBuf; 0] {
        []
    }

    fn touch_cli(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let cli = dir.join(names()[0]);
        std::fs::write(&cli, "").unwrap();
        cli
    }

    #[test]
    fn finds_desktop_claude_code_under_config_claude() {
        let home = scratch("desktop-code").join("home");
        let ver = home
            .join(".config")
            .join("Claude")
            .join("claude-code")
            .join("2.1.330");
        let cli = touch_cli(&ver);
        assert_eq!(find_in(empty_path(), Some(&home)), Some(cli));
    }

    #[test]
    fn finds_desktop_claude_code_vm_when_host_kind_is_absent() {
        let home = scratch("desktop-vm").join("home");
        let ver = home
            .join(".config")
            .join("Claude")
            .join("claude-code-vm")
            .join("2.1.330");
        let cli = touch_cli(&ver);
        assert_eq!(find_in(empty_path(), Some(&home)), Some(cli));
    }

    #[test]
    fn prefers_newer_desktop_claude_code_version() {
        let home = scratch("desktop-newer").join("home");
        let base = home.join(".config").join("Claude").join("claude-code");
        let _old = touch_cli(&base.join("2.1.325"));
        let newer = touch_cli(&base.join("2.1.330"));
        assert_eq!(find_in(empty_path(), Some(&home)), Some(newer));
    }

    #[test]
    fn prefers_desktop_claude_code_over_claude_code_vm() {
        let home = scratch("desktop-prefer").join("home");
        let root = home.join(".config").join("Claude");
        let host = touch_cli(&root.join("claude-code").join("2.1.325"));
        let _vm = touch_cli(&root.join("claude-code-vm").join("2.1.330"));
        assert_eq!(find_in(empty_path(), Some(&home)), Some(host));
    }

    #[test]
    fn finds_desktop_claude_nested_one_level_under_the_version() {
        let home = scratch("desktop-nested").join("home");
        let nested = home
            .join(".config")
            .join("Claude")
            .join("claude-code")
            .join("2.1.330")
            .join("linux-x64");
        let cli = touch_cli(&nested);
        assert_eq!(find_in(empty_path(), Some(&home)), Some(cli));
    }

    #[test]
    fn path_still_wins_over_a_desktop_claude_install() {
        let root = scratch("desktop-path");
        let on_path = root.join("bin");
        let home = root.join("home");
        let desktop = home
            .join(".config")
            .join("Claude")
            .join("claude-code")
            .join("2.1.330");
        let path_cli = touch_cli(&on_path);
        let _desktop_cli = touch_cli(&desktop);
        assert_eq!(
            find_in([on_path.clone()], Some(&home)),
            Some(path_cli)
        );
    }

    #[cfg(unix)]
    fn fake_cli(dir: &Path, auth: &str, auth_exit: i32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let cli = dir.join("claude");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo '2.1.300 (Claude Code)'; exit 0; fi\nif [ \"$1 $2\" = \"auth status\" ]; then echo '{auth}'; exit {auth_exit}; fi\nexit 2\n"
        );
        std::fs::write(&cli, script).unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        cli
    }

    #[cfg(unix)]
    #[test]
    fn asks_a_real_cli_for_its_version_and_sign_in() {
        let dir = scratch("in");
        let v = || "2.1.300".to_string();
        let signed_in = fake_cli(&dir, r#"{"authMethod":"claude.ai"}"#, 0);
        assert_eq!(detect_at(&signed_in), ClaudePlan::Plan { version: v() });
        let other = scratch("out");
        let signed_out = fake_cli(&other, r#"{"authMethod":"none","loggedIn":false}"#, 1);
        assert_eq!(
            detect_at(&signed_out),
            ClaudePlan::SignedOut { version: v() }
        );
        assert_eq!(detect_at(&dir.join("nothing-here")), ClaudePlan::Missing);
        let broken = scratch("broken");
        let cli = fake_cli(&broken, "", 0);
        std::fs::write(&cli, "#!/bin/sh\nexit 3\n").unwrap();
        assert_eq!(detect_at(&cli), ClaudePlan::NoAnswer);
    }

    #[cfg(unix)]
    #[test]
    fn a_chat_asks_from_its_own_folder_where_project_settings_can_change_the_billing() {
        let dir = scratch("cwd");
        // Like a project whose Claude Code settings set an API key.
        let cli = fake_cli(&dir, r#"{"authMethod":"claude.ai"}"#, 0);
        let script = std::fs::read_to_string(&cli).unwrap().replace(
            "echo '{\"authMethod\":\"claude.ai\"}'",
            "if [ -f ./api-key-project ]; then echo '{\"authMethod\":\"api_key\"}'; else echo '{\"authMethod\":\"claude.ai\"}'; fi",
        );
        std::fs::write(&cli, script).unwrap();
        let project = dir.join("my-app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("api-key-project"), "").unwrap();
        let v = || "2.1.300".to_string();
        assert_eq!(detect_in(&cli, &dir), ClaudePlan::Plan { version: v() });
        assert_eq!(
            detect_in(&cli, &project),
            ClaudePlan::OtherBilling {
                version: v(),
                method: "api_key".into()
            }
        );
    }
}

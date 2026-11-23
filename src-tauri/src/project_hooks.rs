//! Connectors → Hooks: the hooks a chat can run (yours, plugins' and this
//! project's) and this folder's trust. A project's hooks come from the
//! repository, so they run only after you trust its folder; the chat reads the
//! same `trusted_folders.json` at the start of every turn
//! (`grokhub_agent::hooks::attach`). Trust also lets this project's
//! `.claude/settings.json` allow rules apply.

use std::path::{Path, PathBuf};

use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::{HookInfo, HookRun};
use serde::Serialize;

/// Trusting the home folder or the filesystem root would trust every
/// project below them.
pub const HOME_REFUSED: &str = "Your home folder and the filesystem root can't be trusted: that would trust every project below them. Open a project folder first.";

/// One hook, with any secret in its command line hidden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// When it runs, like `PreToolUse`.
    pub event: String,
    /// The tool it runs for; empty means any.
    pub matcher: String,
    pub command: String,
    /// "command" or "http".
    pub kind: String,
    /// "user", "project" or "plugin".
    pub origin: &'static str,
    /// The file it was read from.
    pub path: String,
    /// Whether a turn runs it now.
    pub runs: bool,
    /// Why it doesn't run; empty when it does.
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub folder: String,
    pub trusted: bool,
    /// How many of the rows come from this project.
    pub project_hooks: usize,
    pub rows: Vec<Row>,
    pub status: String,
}

pub const WAITS_FOR_TRUST: &str = "Runs after you trust this folder";
pub const HTTP_NOT_RUN: &str = "http hooks don't run here yet";
pub const BAD_MATCHER: &str = "Never runs: its matcher isn't a valid pattern";
pub const NEVER_MATCHES: &str = "Never runs: its matcher never matches this event";

fn row(info: HookInfo, run: HookRun) -> Row {
    let not_fired = format!("Never runs: {} isn't fired here yet", info.event);
    Row {
        runs: run == HookRun::Runs,
        note: match run {
            HookRun::Runs => "",
            HookRun::WaitsForTrust => WAITS_FOR_TRUST,
            HookRun::Http => HTTP_NOT_RUN,
            HookRun::BadMatcher => BAD_MATCHER,
            HookRun::NeverMatches => NEVER_MATCHES,
            HookRun::NotFired => &not_fired,
        }
        .to_string(),
        event: info.event,
        matcher: info.matcher,
        command: crate::hook_command::shown(&info.command),
        kind: info.kind,
        origin: info.origin.as_str(),
        path: info.path.display().to_string(),
    }
}

/// Call with the config pinned: trust lives in its `trusted_folders.json`.
/// `plugins` are the enabled plugins' hook files, as a turn finds them.
fn page(workspace: &Path, home: Option<&Path>, status: String, plugins: &[PathBuf]) -> Page {
    let trusted = grokhub_agent::folder_trusted(workspace);
    // The same merge a turn makes, so a row never says a hook waits while
    // a plugin's copy of it runs.
    let rows: Vec<Row> = grokhub_agent::hooks_by_trust(workspace, home, plugins)
        .into_iter()
        .map(|(info, run)| row(info, run))
        .collect();
    Page {
        folder: workspace.display().to_string(),
        trusted,
        project_hooks: rows.iter().filter(|r| r.origin == "project").count(),
        rows,
        status,
    }
}

/// The hooks for `workspace`. `home` is where your own hooks live.
pub fn load(config_dir: &Path, workspace: &Path, home: Option<&Path>) -> Page {
    let _config = ConfigGuard::set(config_dir);
    let plugins = grokhub_agent::plugins::hook_paths(workspace);
    page(workspace, home, String::new(), &plugins)
}

/// Trust this folder's hooks, or stop trusting them. It takes effect from
/// the next message: a turn reads trust when it starts.
pub fn set_trust(
    config_dir: &Path,
    workspace: &Path,
    home: Option<&Path>,
    trusted: bool,
) -> Result<Page, String> {
    let _config = ConfigGuard::set(config_dir);
    grokhub_agent::set_folder_trust(workspace, trusted).map_err(|err| {
        if err.starts_with("refusing to trust") {
            HOME_REFUSED.to_string()
        } else {
            err
        }
    })?;
    let folder = workspace.display();
    let status = if trusted {
        format!("Trusted {folder}. Its hooks run from your next message.")
    } else {
        format!("Stopped trusting {folder}. Its hooks don't run from your next message.")
    };
    let plugins = grokhub_agent::plugins::hook_paths(workspace);
    Ok(page(workspace, home, status, &plugins))
}

#[cfg(test)]
#[path = "project_hooks_tests.rs"]
mod tests;

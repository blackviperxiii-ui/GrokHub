//! Code tab slice 5b: the per-project deploy command.
//!
//! Off by default. When on, the command runs through the same confirm-and-remember
//! store as test runs (`code_runs`, kind deploy): Ask mode confirms every run;
//! Auto-beta runs a remembered command without asking and asks again when the
//! command text changes. The project's secrets are set in the child's
//! environment, and the output and the activity line are redacted (held values
//! first, then the usual token patterns).

use crate::activity::{self as code_activity, Entry, Kind as ActKind};
use crate::runs::{self as code_runs, Decision, Kind, Mode, RunResult};
use crate::secrets::{self as code_secrets, Vault};
use crate::ship::{CodeShip, ShipMode};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Per-project deploy settings, in the app config folder (never in the repo).
pub const FILE: &str = "code-deploy.json";

/// Event the window listens to for deploy log chunks (apart from test runs).
pub const RUN_EVENT: &str = "code-deploy-run";

pub const DISABLED: &str = "Deploy is off for this project. Turn it on in Settings → Code.";
pub const NO_COMMAND: &str = "Set a deploy command in Settings → Code first.";
pub const NEEDS_CONFIRM: &str = "This command needs a confirmation first.";
pub const CHANGED: &str = "The deploy command changed. Look at it and confirm again.";

/// One project's deploy settings. `enabled` is false until the user turns it on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub deploy_enabled: bool,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    entries: Vec<Named>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Named {
    repo_key: String,
    #[serde(flatten)]
    settings: Settings,
}

static WRITE: Mutex<()> = Mutex::new(());

/// What the Code tab shows before a deploy, and what Settings shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub deploy_enabled: bool,
    pub command: String,
    pub needs_confirm: bool,
    /// The command currently remembered for this project, if any.
    pub remembered: Option<String>,
    /// `ask` or `autoBeta`.
    pub mode: String,
    /// Why a deploy can't run now, if it can't.
    pub blocked: Option<String>,
}

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

fn load(config_dir: &Path) -> Store {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(config_dir: &Path, store: &Store) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
}

/// This project's deploy settings: off, with no command, until set.
pub fn settings(config_dir: &Path, repo_key: &str) -> Settings {
    load(config_dir)
        .entries
        .into_iter()
        .find(|e| e.repo_key == repo_key)
        .map(|e| e.settings)
        .unwrap_or_default()
}

/// Save this project's deploy switch and command. The command is trimmed. A
/// changed command is not remembered, so the next run asks.
pub fn save_settings(
    config_dir: &Path,
    repo_key: &str,
    next: Settings,
) -> Result<Settings, String> {
    let next = Settings {
        deploy_enabled: next.deploy_enabled,
        command: next.command.trim().to_string(),
    };
    if next.deploy_enabled && next.command.is_empty() {
        return Err(NO_COMMAND.into());
    }
    let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut store = load(config_dir);
    store.entries.retain(|e| e.repo_key != repo_key);
    store.entries.push(Named {
        repo_key: repo_key.to_string(),
        settings: next.clone(),
    });
    save(config_dir, &store)?;
    Ok(next)
}

fn run_mode(mode: ShipMode) -> Mode {
    match mode {
        ShipMode::Ask => Mode::Ask,
        ShipMode::AutoBeta => Mode::Auto,
    }
}

/// The deploy for this project, and whether it asks first.
pub fn plan_for_key(config_dir: &Path, repo_key: &str, ship: &CodeShip) -> Result<Plan, String> {
    let s = settings(config_dir, repo_key);
    let remembered = code_runs::remembered(config_dir, Kind::Deploy, repo_key)?;
    let blocked = if !s.deploy_enabled {
        Some(DISABLED.to_string())
    } else if s.command.is_empty() {
        Some(NO_COMMAND.to_string())
    } else {
        None
    };
    let needs_confirm = match code_runs::check(
        config_dir,
        Kind::Deploy,
        repo_key,
        &s.command,
        run_mode(ship.mode),
    )? {
        Decision::NeedsConfirm { .. } => true,
        Decision::Remembered => false,
    };
    Ok(Plan {
        deploy_enabled: s.deploy_enabled,
        command: s.command,
        needs_confirm,
        remembered,
        mode: ship.mode.id().into(),
        blocked,
    })
}

/// Forget the remembered deploy command, so the next run asks.
pub fn forget_for_key(config_dir: &Path, repo_key: &str) -> Result<(), String> {
    code_runs::forget(config_dir, Kind::Deploy, repo_key)
}

/// Where a deploy ran, for the activity line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Where {
    pub host: String,
    pub branch: String,
}

/// Run the deploy. `expected` is the command the person was shown; if the
/// saved command differs now, nothing runs. Project secrets are set in the
/// child's environment; output and the activity line are redacted.
#[allow(clippy::too_many_arguments)]
pub fn run_with(
    config_dir: &Path,
    repo_key: &str,
    workspace: &Path,
    ship: &CodeShip,
    expected: &str,
    confirmed: bool,
    vault: &dyn Vault,
    place: &Where,
    mut on_chunk: impl FnMut(&str),
) -> Result<RunResult, String> {
    let s = settings(config_dir, repo_key);
    if !s.deploy_enabled {
        return Err(DISABLED.into());
    }
    if s.command.is_empty() {
        return Err(NO_COMMAND.into());
    }
    if expected.trim() != s.command {
        return Err(CHANGED.into());
    }
    let decision = code_runs::check(
        config_dir,
        Kind::Deploy,
        repo_key,
        &s.command,
        run_mode(ship.mode),
    )?;
    let approved_by = match decision {
        Decision::NeedsConfirm { .. } if !confirmed => return Err(NEEDS_CONFIRM.into()),
        Decision::NeedsConfirm { .. } => "user",
        Decision::Remembered => "remembered",
    };
    // A secret with no value stops the deploy before anything runs.
    let secrets = code_secrets::load_values(config_dir, vault, repo_key)?;
    if approved_by == "user" {
        code_runs::remember(config_dir, Kind::Deploy, repo_key, &s.command)?;
    }
    let held = secrets.held();
    let result = code_runs::exec(
        workspace,
        &s.command,
        Kind::Deploy,
        &held,
        Some(&secrets),
        &mut on_chunk,
    );
    let (exit, summary) = match &result {
        Ok(done) if done.exit == 0 => (Some(0), "Deploy finished.".to_string()),
        Ok(done) => (
            Some(done.exit),
            format!("Deploy exited with {}.", done.exit),
        ),
        Err(err) => (None, format!("Deploy didn't run: {err}")),
    };
    let _ = code_activity::append_held(
        config_dir,
        &Entry {
            ts_ms: grokhub_core::now_ms(),
            kind: ActKind::Deploy,
            repo_key: repo_key.to_string(),
            host: place.host.clone(),
            branch: place.branch.clone(),
            command: s.command.clone(),
            mode: ship.mode.id().into(),
            approved_by: approved_by.into(),
            exit,
            summary,
        },
        ship.activity_keep,
        &held,
    );
    result.map(|done| RunResult {
        command: secrets.redact(&done.command),
        ..done
    })
}

// ---- what the Code tab calls ----

fn key_of(dir: &Path) -> String {
    code_runs::repo_key(dir, crate::repo::origin_url(dir).as_deref())
}

pub fn settings_of(config_dir: &Path, workspace: &Path) -> Settings {
    settings(config_dir, &key_of(workspace))
}

pub fn save_settings_of(
    config_dir: &Path,
    workspace: &Path,
    next: Settings,
) -> Result<Settings, String> {
    save_settings(config_dir, &key_of(workspace), next)
}

pub fn plan(config_dir: &Path, workspace: &Path, ship: &CodeShip) -> Result<Plan, String> {
    plan_for_key(config_dir, &key_of(workspace), ship)
}

pub fn forget(config_dir: &Path, workspace: &Path, ship: &CodeShip) -> Result<Plan, String> {
    let key = key_of(workspace);
    forget_for_key(config_dir, &key)?;
    plan_for_key(config_dir, &key, ship)
}

pub fn run(
    config_dir: &Path,
    workspace: &Path,
    ship: &CodeShip,
    expected: &str,
    confirmed: bool,
    on_chunk: impl FnMut(&str),
) -> Result<RunResult, String> {
    let place = Where {
        host: crate::ship::push_host(workspace)
            .map(|host| host.id().to_string())
            .unwrap_or_else(|| "other".into()),
        branch: crate::ship::current_branch(workspace).unwrap_or_default(),
    };
    run_with(
        config_dir,
        &key_of(workspace),
        workspace,
        ship,
        expected,
        confirmed,
        &code_secrets::Keychain,
        &place,
        on_chunk,
    )
}

#[cfg(test)]
#[path = "deploy_tests.rs"]
mod tests;

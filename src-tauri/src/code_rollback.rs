//! Code tab slice 5c commands: list the branch's newest commits, show what a
//! revert would run, and run it (always confirmed) through the push guard.

use crate::{current, off_main, workspace, ShellState};
use grokhub_code::rollback;
use tauri::State;

/// Code tab: the newest commits on the checked-out branch.
#[tauri::command]
pub async fn code_revert_list(
    state: State<'_, ShellState>,
) -> Result<Vec<rollback::Commit>, String> {
    let dir = workspace(&state);
    off_main(move || rollback::commits(&dir, rollback::SHOW)).await
}

/// Code tab: what reverting `sha` would run, and why it won't when it won't.
#[tauri::command]
pub async fn code_revert_plan(
    state: State<'_, ShellState>,
    sha: String,
) -> Result<rollback::RevertPlan, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    let cfg = current(&state).code_ship;
    off_main(move || rollback::plan(&config, &dir, &cfg, &sha)).await
}

/// Code tab: revert `sha` as a new commit and push it. `confirmed` must be
/// true, and both commands must be the ones the confirm showed.
#[tauri::command]
pub async fn code_revert_run(
    state: State<'_, ShellState>,
    sha: String,
    confirmed: bool,
    revert_command: String,
    push_command: String,
) -> Result<rollback::Reverted, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    let cfg = current(&state).code_ship;
    off_main(move || {
        rollback::revert(
            &config,
            &dir,
            &cfg,
            &sha,
            confirmed,
            &revert_command,
            &push_command,
        )
    })
    .await?
}

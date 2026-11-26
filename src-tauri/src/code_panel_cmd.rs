//! Code tab Settings panel commands: per-project toggles (`localOnlyNonstop`
//! and `backupBeforeRisky`), backup listing with retention warning, and
//! one-click backup restore.

use crate::{off_main, workspace, ShellState};
use grokhub_agent::code_panel::{
    self, BackupList, PanelSettings, PanelState, RestoreResult,
};
use tauri::State;

/// Code tab Settings panel: current project settings, backups, and retention status.
#[tauri::command]
pub async fn code_panel_state(state: State<'_, ShellState>) -> Result<PanelState, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    off_main(move || code_panel::panel_state(&config, &dir)).await
}

/// Code tab Settings panel: save per-project toggles and return updated panel state.
#[tauri::command]
pub async fn code_panel_settings_save(
    state: State<'_, ShellState>,
    settings: PanelSettings,
) -> Result<PanelState, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    off_main(move || {
        code_panel::save_settings(&config, &dir, settings)?;
        Ok(code_panel::panel_state(&config, &dir))
    })
    .await?
}

/// Code tab Settings panel: list saved backups for the active project.
#[tauri::command]
pub async fn code_panel_backups(state: State<'_, ShellState>) -> Result<BackupList, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    off_main(move || code_panel::list_backups(&config, &dir)).await
}

/// Code tab Settings panel: one-click restore of a saved backup (never deletes the backup).
#[tauri::command]
pub async fn code_panel_backup_restore(
    state: State<'_, ShellState>,
    backup_id: String,
) -> Result<RestoreResult, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    off_main(move || code_panel::restore_backup(&config, &dir, &backup_id)).await?
}

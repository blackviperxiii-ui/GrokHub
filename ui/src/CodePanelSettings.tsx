import { useCallback, useEffect, useState } from "react";
import {
  codePanelBackupRestore,
  codePanelSettingsSave,
  codePanelState,
  type CodeBackupEntry,
  type CodeBackupStrategy,
  type CodePanelSettings as CodePanelSettingsData,
  type CodePanelState,
} from "./api";
import { folderName } from "./CodeProjects";

export const CODE_PANEL_SETTINGS_TITLE = "Project settings";
export const LOCAL_NONSTOP_LABEL = "Local models only and nonstop coding";
export const BACKUP_RISKY_LABEL = "Backup before risky actions";
export const HARD_RULE_NOTE =
  "Hard-class and dangerous commands still ask, even with both toggles on.";
export const BACKUPS_EMPTY = "No backups saved for this project yet.";

function strategyLabel(strategy: CodeBackupStrategy): string {
  switch (strategy) {
    case "git_commit":
      return "Work-branch commit";
    case "git_stash":
      return "Git stash";
    case "git_head_ref":
      return "Git snapshot";
    case "folder_copy":
      return "Folder backup";
  }
}

function formatWhen(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "";
  try {
    return new Date(ms).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

interface Props {
  /** Bumped when a turn finishes or the project changes so new backups appear. */
  refresh?: number;
  /** Called whenever settings are loaded or saved, so the Code header updates its route chip. */
  onState?: (state: CodePanelState) => void;
  /** Called after a one-click restore succeeds, so the Editor/diff view reloads. */
  onRestored?: () => void;
}

export function CodePanelSettings({ refresh = 0, onState, onRestored }: Props) {
  const [state, setState] = useState<CodePanelState | null>(null);
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const [restoringId, setRestoringId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const next = await codePanelState();
      if (!next?.settings) return;
      setState(next);
      onState?.(next);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }, [onState]);

  useEffect(() => {
    let active = true;
    codePanelState().then(
      (next) => {
        if (!active || !next?.settings) return;
        setState(next);
        setError(null);
        onState?.(next);
      },
      (err) => {
        if (!active) return;
        setError(String(err));
      },
    );
    return () => {
      active = false;
    };
  }, [onState, refresh]);

  async function save(patch: Partial<CodePanelSettingsData>) {
    if (!state || saving) return;
    const nextSettings: CodePanelSettingsData = {
      ...state.settings,
      ...patch,
    };
    setSaving(true);
    setError(null);
    setNotice(null);
    try {
      const updated = await codePanelSettingsSave(nextSettings);
      setState(updated);
      onState?.(updated);
      setNotice("Saved for this project.");
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function restore(entry: CodeBackupEntry) {
    if (restoringId) return;
    setRestoringId(entry.id);
    setError(null);
    setNotice(null);
    try {
      const res = await codePanelBackupRestore(entry.id);
      setNotice(res.summary);
      onRestored?.();
      const refreshed = await codePanelState();
      setState(refreshed);
      onState?.(refreshed);
    } catch (err) {
      setError(String(err));
    } finally {
      setRestoringId(null);
    }
  }

  const settings = state?.settings;
  const projectLabel = state?.workspace ? folderName(state.workspace) : "Open project";

  return (
    <div className="code-panel-settings">
      <header className="code-head">
        <div>
          <h4>{CODE_PANEL_SETTINGS_TITLE}</h4>
          <p className="note">
            Saved per project ({projectLabel}
            {state?.branch ? ` · ${state.branch}` : state && !state.isRepo ? " · not a git repo" : ""}).
          </p>
        </div>
        <button type="button" className="quiet" disabled={busy || saving} onClick={() => void load()}>
          Refresh
        </button>
      </header>

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="note" role="status">
          {notice}
        </p>
      )}

      {settings && (
        <>
          <section className="code-settings-card" aria-label={LOCAL_NONSTOP_LABEL}>
            <div className="code-toggle-row">
              <div className="code-toggle-copy">
                <div className="code-toggle-title">
                  <span>{LOCAL_NONSTOP_LABEL}</span>
                  {settings.localOnlyNonstop && (
                    <span className="code-route-chip local" data-route="local">
                      Local
                    </span>
                  )}
                </div>
                <p className="note">
                  Routes Code turns to local models only (never escalates to a frontier model), keeps working through
                  the task list without waiting, checkpoints on a work branch before risky steps, and stops with a
                  status card on repeated failures, no progress, or a repetition-guard cut.
                </p>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={settings.localOnlyNonstop}
                aria-label={LOCAL_NONSTOP_LABEL}
                className={`code-switch${settings.localOnlyNonstop ? " on" : ""}`}
                disabled={saving}
                onClick={() => void save({ localOnlyNonstop: !settings.localOnlyNonstop })}
              >
                {settings.localOnlyNonstop ? "On" : "Off"}
              </button>
            </div>
            <div className="code-guard-limits">
              <label>
                <span>Stop after repeated failures</span>
                <input
                  type="number"
                  min={1}
                  max={20}
                  value={settings.maxRepeatedFailures}
                  disabled={saving}
                  aria-label="Stop after repeated failures"
                  onChange={(e) => {
                    const n = Number(e.target.value);
                    if (Number.isFinite(n) && n >= 1) {
                      void save({ maxRepeatedFailures: Math.min(20, Math.round(n)) });
                    }
                  }}
                />
              </label>
              <label>
                <span>No-progress stop (minutes)</span>
                <input
                  type="number"
                  min={1}
                  max={240}
                  value={settings.stallTimeoutMins}
                  disabled={saving}
                  aria-label="No-progress stop (minutes)"
                  onChange={(e) => {
                    const n = Number(e.target.value);
                    if (Number.isFinite(n) && n >= 1) {
                      void save({ stallTimeoutMins: Math.min(240, Math.round(n)) });
                    }
                  }}
                />
              </label>
            </div>
          </section>

          <section className="code-settings-card" aria-label={BACKUP_RISKY_LABEL}>
            <div className="code-toggle-row">
              <div className="code-toggle-copy">
                <div className="code-toggle-title">
                  <span>{BACKUP_RISKY_LABEL}</span>
                </div>
                <p className="note">
                  When Allow always is on, risky actions (deletes, overwrites, mass edits, git reset/clean/checkout
                  over changes, package installs) save a git stash/snapshot or .loadout-backups copy first, then
                  continue without prompting.
                </p>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={settings.backupBeforeRisky}
                aria-label={BACKUP_RISKY_LABEL}
                className={`code-switch${settings.backupBeforeRisky ? " on" : ""}`}
                disabled={saving}
                onClick={() => void save({ backupBeforeRisky: !settings.backupBeforeRisky })}
              >
                {settings.backupBeforeRisky ? "On" : "Off"}
              </button>
            </div>
            <p className="code-safety-rule">{HARD_RULE_NOTE}</p>
          </section>

          <section className="code-settings-card" aria-label="Saved backups">
            <div className="code-backups-head">
              <h4>Backups ({state.backups.length})</h4>
              <span className="note">Never auto-deleted</span>
            </div>
            {state.retentionWarning && (
              <p className="code-retention-warn" role="status">
                {state.retentionWarning}
              </p>
            )}
            {state.backups.length === 0 ? (
              <p className="note">{BACKUPS_EMPTY}</p>
            ) : (
              <ul className="code-backup-list">
                {state.backups.map((entry) => (
                  <li key={entry.id} className="code-backup-row">
                    <div className="code-backup-meta">
                      <span className="code-backup-label">{entry.label}</span>
                      <span className="note">
                        {strategyLabel(entry.strategy)} · {entry.filesCount} file
                        {entry.filesCount === 1 ? "" : "s"}
                        {entry.createdAtMs ? ` · ${formatWhen(entry.createdAtMs)}` : ""}
                        {entry.restoredAtMs ? " · Restored" : ""}
                      </span>
                    </div>
                    <button
                      type="button"
                      className="quiet"
                      disabled={restoringId !== null}
                      onClick={() => void restore(entry)}
                    >
                      {restoringId === entry.id ? "Restoring…" : "Restore"}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </>
      )}
    </div>
  );
}

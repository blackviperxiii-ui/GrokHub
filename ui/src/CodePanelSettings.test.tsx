import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeBackupRestoreResult, CodePanelSettings as PanelSettingsData, CodePanelState } from "./api";

const api = vi.hoisted(() => ({
  codePanelState: vi.fn<() => Promise<CodePanelState>>(),
  codePanelSettingsSave: vi.fn<(settings: PanelSettingsData) => Promise<CodePanelState>>(),
  codePanelBackupRestore: vi.fn<(backupId: string) => Promise<CodeBackupRestoreResult>>(),
}));

vi.mock("./api", () => api);

import {
  BACKUP_RISKY_LABEL,
  BACKUPS_EMPTY,
  CodePanelSettings,
  HARD_RULE_NOTE,
  LOCAL_NONSTOP_LABEL,
} from "./CodePanelSettings";

function stateFixture(over: Partial<CodePanelState> = {}): CodePanelState {
  return {
    workspace: "/work/demo",
    isRepo: true,
    branch: "work/feature",
    settings: {
      localOnlyNonstop: false,
      backupBeforeRisky: false,
      maxRepeatedFailures: 3,
      stallTimeoutMins: 15,
      backupRetentionWarn: 20,
    },
    backups: [],
    retentionWarnLimit: 20,
    retentionWarning: null,
    ...over,
  };
}

describe("CodePanelSettings", () => {
  beforeEach(() => {
    api.codePanelState.mockReset();
    api.codePanelSettingsSave.mockReset();
    api.codePanelBackupRestore.mockReset();
  });

  it("renders both toggles off by default and shows the hard-class rule note", async () => {
    api.codePanelState.mockResolvedValue(stateFixture());
    render(<CodePanelSettings />);

    const localToggle = await screen.findByRole("switch", { name: LOCAL_NONSTOP_LABEL });
    const backupToggle = screen.getByRole("switch", { name: BACKUP_RISKY_LABEL });
    expect(localToggle).toHaveAttribute("aria-checked", "false");
    expect(backupToggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(HARD_RULE_NOTE)).toBeInTheDocument();
    expect(screen.getByText(BACKUPS_EMPTY)).toBeInTheDocument();
  });

  it("saves toggles per project and shows the Local route chip when enabled", async () => {
    const initial = stateFixture();
    const afterLocal = stateFixture({
      settings: { ...initial.settings, localOnlyNonstop: true },
    });
    const afterBoth = stateFixture({
      settings: { ...afterLocal.settings, backupBeforeRisky: true },
    });
    api.codePanelState.mockResolvedValue(initial);
    api.codePanelSettingsSave
      .mockResolvedValueOnce(afterLocal)
      .mockResolvedValueOnce(afterBoth);

    const onState = vi.fn();
    render(<CodePanelSettings onState={onState} />);

    await userEvent.click(await screen.findByRole("switch", { name: LOCAL_NONSTOP_LABEL }));
    expect(api.codePanelSettingsSave).toHaveBeenCalledWith({
      localOnlyNonstop: true,
      backupBeforeRisky: false,
      maxRepeatedFailures: 3,
      stallTimeoutMins: 15,
      backupRetentionWarn: 20,
    });
    expect(await screen.findByText("Local")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: LOCAL_NONSTOP_LABEL })).toHaveAttribute("aria-checked", "true");

    await userEvent.click(screen.getByRole("switch", { name: BACKUP_RISKY_LABEL }));
    expect(api.codePanelSettingsSave).toHaveBeenLastCalledWith({
      localOnlyNonstop: true,
      backupBeforeRisky: true,
      maxRepeatedFailures: 3,
      stallTimeoutMins: 15,
      backupRetentionWarn: 20,
    });
    await waitFor(() => {
      expect(screen.getByRole("switch", { name: BACKUP_RISKY_LABEL })).toHaveAttribute("aria-checked", "true");
    });
    expect(onState).toHaveBeenLastCalledWith(afterBoth);
  });

  it("lists backups, displays retention warning without deleting, and restores in one click", async () => {
    const backupEntry = {
      id: "20261126-143000-001",
      createdAtMs: 1_795_617_000_000,
      label: "Before delete: rm important.txt",
      actionKind: "delete" as const,
      strategy: "folder_copy" as const,
      refValue: ".loadout-backups/20261126-143000-001",
      branch: null,
      filesCount: 2,
      restoredAtMs: null,
    };
    const warn =
      "You have 21 saved backups (warning threshold: 20). Old backups are never auto-deleted.";
    api.codePanelState
      .mockResolvedValueOnce(
        stateFixture({
          isRepo: false,
          branch: null,
          backups: [backupEntry],
          retentionWarning: warn,
        }),
      )
      .mockResolvedValueOnce(
        stateFixture({
          isRepo: false,
          branch: null,
          backups: [{ ...backupEntry, restoredAtMs: 1_795_617_060_000 }],
          retentionWarning: warn,
        }),
      );
    api.codePanelBackupRestore.mockResolvedValue({
      id: backupEntry.id,
      summary: "Restored 2 file(s) from .loadout-backups/20261126-143000-001.",
      restoredFiles: 2,
    });

    const onRestored = vi.fn();
    render(<CodePanelSettings onRestored={onRestored} />);

    expect(await screen.findByText(warn)).toBeInTheDocument();
    expect(screen.getByText("Before delete: rm important.txt")).toBeInTheDocument();
    expect(screen.getByText(/Folder backup · 2 files/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect(api.codePanelBackupRestore).toHaveBeenCalledWith("20261126-143000-001");
    expect(
      await screen.findByText("Restored 2 file(s) from .loadout-backups/20261126-143000-001."),
    ).toBeInTheDocument();
    expect(onRestored).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(/· Restored/)).toBeInTheDocument();
  });
});

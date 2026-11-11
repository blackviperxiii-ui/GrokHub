import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FilesHelpers } from "./pcFilesApi";

const api = vi.hoisted(() => ({
  getPcFiles: vi.fn(),
  setPcFilesSwitch: vi.fn(),
  setPcFolders: vi.fn(),
  setPcModelsFolder: vi.fn(),
  addPcPrinter: vi.fn(),
  removePcPrinter: vi.fn(),
  setPcPrinterOn: vi.fn(),
  pcTidyReview: vi.fn(),
  pcTidyTrash: vi.fn(),
  pcResumeRows: vi.fn(),
  pcSpeedTest: vi.fn(),
  pcNetCsv: vi.fn(),
}));

vi.mock("./pcFilesApi", () => api);

import { PcFiles } from "./PcFiles";

const FRESH: FilesHelpers = {
  downloadsTidy: false,
  modelDrop: false,
  modelsFolder: "",
  backupNudge: false,
  importantFolders: [],
  resumeCode: false,
  codeFolders: [],
  netLog: false,
  netDrops: 0,
  netByHour: Array(24).fill(0),
  netDown: false,
  netLastSpeedMbps: null,
  printAlerts: false,
  printCamera: false,
  printers: [],
};

const SWITCHES = [
  "Suggest a Downloads cleanup",
  "Offer next steps for new 3D models",
  "Remind me to back up and push",
  "Pick up where I left off in code",
  "Log internet drops",
  "Watch 3D printers on my network",
];

describe("PcFiles", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.getPcFiles.mockResolvedValue(FRESH);
  });

  it("shows every switch off, as in the cabin, and none of their extras", async () => {
    render(<PcFiles />);
    for (const name of SWITCHES) {
      expect(await screen.findByRole("switch", { name })).toHaveAttribute("aria-checked", "false");
    }
    expect(screen.queryByRole("button", { name: "Review Downloads cleanup" })).toBeNull();
    expect(screen.queryByRole("switch", { name: "Include a camera snapshot" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Add printer" })).toBeNull();
  });

  it("saves a switch under its key and shows what the shell kept", async () => {
    api.setPcFilesSwitch.mockResolvedValue({ ...FRESH, netLog: true });
    render(<PcFiles />);
    await userEvent.click(await screen.findByRole("switch", { name: "Log internet drops" }));
    expect(api.setPcFilesSwitch).toHaveBeenCalledWith("netLog", true);
    expect(await screen.findByText("Saved.")).toBeVisible();
    expect(screen.getByRole("switch", { name: "Log internet drops" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("No drops logged yet.")).toBeVisible();
  });

  it("shows the shell's refusal and leaves the switch where it was", async () => {
    api.setPcFilesSwitch.mockRejectedValue("Couldn't write app.json");
    render(<PcFiles />);
    await userEvent.click(await screen.findByRole("switch", { name: "Pick up where I left off in code" }));
    expect(await screen.findByText("Couldn't write app.json")).toBeVisible();
    expect(screen.getByRole("switch", { name: "Pick up where I left off in code" })).toHaveAttribute("aria-checked", "false");
  });

  it("moves nothing until files are picked and the exact list is approved", async () => {
    api.getPcFiles.mockResolvedValue({ ...FRESH, downloadsTidy: true });
    api.pcTidyReview.mockResolvedValue([
      { name: "setup.exe", bytes: 3_000_000, size: "3 MB", kind: "Old installer", why: "Already installed" },
      { name: "a (1).zip", bytes: 2_000_000, size: "2 MB", kind: "Duplicate", why: "Same file as a.zip" },
    ]);
    api.pcTidyTrash.mockResolvedValue({ moved: 1, note: "Moved 1 file (3 MB) to the Trash." });
    render(<PcFiles />);
    await userEvent.click(await screen.findByRole("button", { name: "Review Downloads cleanup" }));
    const ask = await screen.findByRole("button", { name: "Move picked files to the Trash…" });
    expect(ask).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "setup.exe · 3 MB" }));
    await userEvent.click(ask);
    const dialog = screen.getByRole("alertdialog", { name: "Approve the list" });
    expect(within(dialog).getByText(/Move 1 file to the Trash: setup\.exe\./)).toBeVisible();
    expect(api.pcTidyTrash).not.toHaveBeenCalled();
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(api.pcTidyTrash).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Move picked files to the Trash…" }));
    await userEvent.click(screen.getByRole("button", { name: "Move to Trash" }));
    expect(api.pcTidyTrash).toHaveBeenCalledWith(["setup.exe"]);
    expect(await screen.findByText("Moved 1 file (3 MB) to the Trash.")).toBeVisible();
  });

  it("adds and removes code folders and lists where the repos stand", async () => {
    const withFolder = { ...FRESH, resumeCode: true, codeFolders: ["/code"] };
    api.getPcFiles.mockResolvedValue({ ...FRESH, resumeCode: true });
    api.setPcFolders.mockResolvedValueOnce(withFolder).mockResolvedValueOnce({ ...FRESH, resumeCode: true });
    api.pcResumeRows.mockResolvedValue([{ name: "proj", line: "1 commit not pushed", staleBranches: 2 }]);
    render(<PcFiles />);
    await userEvent.type(await screen.findByLabelText("Add to code folders"), "/code");
    await userEvent.click(screen.getByRole("button", { name: "Add folder" }));
    expect(api.setPcFolders).toHaveBeenCalledWith("codeFolders", ["/code"]);
    expect(await screen.findByRole("button", { name: "Remove /code" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Look at my repos now" }));
    expect(await screen.findByText("1 commit not pushed")).toBeVisible();
    expect(screen.getByText(/2 merged branches to tidy/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Remove /code" }));
    expect(api.setPcFolders).toHaveBeenLastCalledWith("codeFolders", []);
  });

  it("adds a printer by address, pauses it, and keeps the camera as its own switch", async () => {
    const printer = { key: "192.168.1.50:7125", name: "voron", kind: "Moonraker", enabled: true };
    api.getPcFiles.mockResolvedValue({ ...FRESH, printAlerts: true });
    api.addPcPrinter.mockResolvedValue({ ...FRESH, printAlerts: true, printers: [printer] });
    api.setPcPrinterOn.mockResolvedValue({ ...FRESH, printAlerts: true, printers: [{ ...printer, enabled: false }] });
    api.setPcFilesSwitch.mockResolvedValue({ ...FRESH, printAlerts: true, printCamera: true, printers: [printer] });
    render(<PcFiles />);
    expect(await screen.findByRole("switch", { name: "Include a camera snapshot" })).toHaveAttribute("aria-checked", "false");
    const add = screen.getByRole("button", { name: "Add printer" });
    expect(add).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Printer address"), "192.168.1.50:7125");
    await userEvent.click(add);
    expect(api.addPcPrinter).toHaveBeenCalledWith("192.168.1.50:7125");
    await userEvent.click(await screen.findByRole("button", { name: "Watch voron" }));
    expect(api.setPcPrinterOn).toHaveBeenCalledWith("192.168.1.50:7125", false);
    expect(await screen.findByRole("button", { name: "Watch voron" })).toHaveTextContent("Paused");
    await userEvent.click(screen.getByRole("switch", { name: "Include a camera snapshot" }));
    expect(api.setPcFilesSwitch).toHaveBeenCalledWith("printCamera", true);
  });

  it("shows the drops by hour, runs a speed test and exports the log", async () => {
    const byHour = Array(24).fill(0);
    byHour[7] = 3;
    api.getPcFiles.mockResolvedValue({ ...FRESH, netLog: true, netDrops: 3, netByHour: byHour });
    api.pcSpeedTest.mockResolvedValue(94.5);
    api.pcNetCsv.mockResolvedValue("/files/Reports/internet-log-2026-11-11.csv");
    render(<PcFiles />);
    expect(await screen.findByText("3 drops in the last 30 days.")).toBeVisible();
    expect(screen.getByText("Most drops: 7:00")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Test speed now" }));
    expect(await screen.findByText("Download speed: 94.5 Mbit/s.")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Export CSV" }));
    expect(await screen.findByText("Saved the log to /files/Reports/internet-log-2026-11-11.csv.")).toBeVisible();
  });

  it("saves the models folder", async () => {
    api.getPcFiles.mockResolvedValue({ ...FRESH, modelDrop: true });
    api.setPcModelsFolder.mockResolvedValue({ ...FRESH, modelDrop: true, modelsFolder: "/models" });
    render(<PcFiles />);
    await userEvent.type(await screen.findByLabelText("Models folder"), "/models");
    await userEvent.click(screen.getByRole("button", { name: "Save folder" }));
    expect(api.setPcModelsFolder).toHaveBeenCalledWith("/models");
  });
});

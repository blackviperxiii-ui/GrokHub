import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ModGame, ModRow } from "./api";
import type { ModSheet } from "./modsInstallApi";

const api = vi.hoisted(() => ({ openUrl: vi.fn(() => Promise.resolve()) }));
vi.mock("./api", () => api);

const inst = vi.hoisted(() => ({
  modsFiles: vi.fn(),
  modsInstalled: vi.fn(),
  modsPlanInstall: vi.fn(),
  modsPlanUninstall: vi.fn(),
  modsPickupDownload: vi.fn(),
  modsWaitForBrowser: vi.fn(() => Promise.resolve()),
  modsConfirm: vi.fn(),
  modsCancel: vi.fn(() => Promise.resolve()),
  onModsJob: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("./modsInstallApi", () => inst);

import { InstalledTab, InstallSheet, ModDrawerPanel, StepLine, useInstall } from "./ModsInstall";

const GAME: ModGame = {
  id: "stardew",
  name: "Stardew Valley",
  modeNote: "Singleplayer and your own co-op",
  sources: [{ id: "nexus", label: "Nexus Mods" }],
  lastPlayed: 0,
  playtimeMin: 0,
  newCount: 0,
  suggestionsHidden: false,
};

const MOD: ModRow = {
  id: "2100",
  name: "Better Crops",
  author: "farmer",
  summary: "Crops grow better.",
  downloads: 10,
  likes: 1,
  updated: 0,
  pageUrl: "https://www.nexusmods.com/stardewvalley/mods/2100",
  iconUrl: null,
  categories: [],
};

const INSTALL_SHEET: ModSheet = {
  token: 7,
  title: "Install Better Crops 1.0.0?",
  primary: "Install",
  into: "Into Stardew Valley",
  lists: [{ title: "Adds (2)", rows: ["Mods/BetterCrops/BetterCrops.dll", "Mods/BetterCrops/manifest.json"] }],
  notes: ["Nexus Mods publishes no hash Loadout can check for this file."],
};

function Drawer() {
  const install = useInstall();
  return (
    <>
      <StepLine install={install} />
      <ModDrawerPanel
        game={GAME}
        source="nexus"
        sourceLabel="Nexus Mods"
        mod={MOD}
        offline={false}
        install={install}
        onClose={() => {}}
      />
      <InstallSheet install={install} />
    </>
  );
}

function Installed() {
  const install = useInstall();
  return (
    <>
      <StepLine install={install} />
      <InstalledTab game={GAME} install={install} />
      <InstallSheet install={install} />
    </>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  inst.modsFiles.mockResolvedValue({
    files: [
      { id: "7", name: "Main file", line: "1.0.0 · 880 KB", offer: { kind: "install" }, waiting: false },
      {
        id: "8",
        name: "Optional",
        line: "1.0.0 · download on the site",
        offer: { kind: "browser", url: "https://www.nexusmods.com/f/8" },
        waiting: false,
      },
    ],
    blocked: null,
    needs: ["SMAPI"],
    about: "Crops grow better.",
    note: null,
  });
  inst.modsPlanInstall.mockResolvedValue(INSTALL_SHEET);
  inst.modsConfirm.mockResolvedValue("Better Crops 1.0.0 is in. Uninstall puts every file back.");
});

describe("the install sheet", () => {
  it("lists every file, and only the sheet's own button installs", async () => {
    const user = userEvent.setup();
    render(<Drawer />);
    const files = within(await screen.findByRole("region", { name: "Files" }));
    expect(screen.getByText("SMAPI")).toBeInTheDocument();
    await user.click(files.getByRole("button", { name: "Install" }));
    expect(inst.modsPlanInstall).toHaveBeenCalledWith("stardew", "nexus", "2100", "Better Crops", "7");
    const sheet = within(await screen.findByRole("dialog", { name: "Install Better Crops 1.0.0?" }));
    // The first click only planned.
    expect(inst.modsConfirm).not.toHaveBeenCalled();
    expect(screen.getByText("Waiting for you on the sheet.")).toBeInTheDocument();
    expect(sheet.getByText("Mods/BetterCrops/BetterCrops.dll")).toBeInTheDocument();
    expect(sheet.getByText("Mods/BetterCrops/manifest.json")).toBeInTheDocument();
    expect(sheet.getByText("Adds (2)")).toBeInTheDocument();
    expect(sheet.getByText("Nexus Mods publishes no hash Loadout can check for this file.")).toBeInTheDocument();
    await user.click(sheet.getByRole("button", { name: "Install" }));
    expect(inst.modsConfirm).toHaveBeenCalledWith(7);
    expect(await screen.findByText("Better Crops 1.0.0 is in. Uninstall puts every file back.")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("never confirms from the keyboard, and Cancel writes nothing", async () => {
    const user = userEvent.setup();
    render(<Drawer />);
    const files = within(await screen.findByRole("region", { name: "Files" }));
    await user.click(files.getByRole("button", { name: "Install" }));
    const sheet = within(await screen.findByRole("dialog"));
    // A key press on the button arrives as a click with no pointer.
    fireEvent.click(sheet.getByRole("button", { name: "Install" }), { detail: 0 });
    expect(inst.modsConfirm).not.toHaveBeenCalled();
    await user.click(sheet.getByRole("button", { name: "Cancel" }));
    expect(inst.modsCancel).toHaveBeenCalledWith(7);
    expect(inst.modsConfirm).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("a browser file opens the site and waits for the download", async () => {
    const user = userEvent.setup();
    render(<Drawer />);
    const files = within(await screen.findByRole("region", { name: "Files" }));
    await user.click(files.getByRole("button", { name: "Download on Nexus Mods" }));
    expect(api.openUrl).toHaveBeenCalledWith("https://www.nexusmods.com/f/8");
    expect(inst.modsWaitForBrowser).toHaveBeenCalledWith("stardew", "nexus", "2100", "8");
  });

  it("shows why the plan failed", async () => {
    const user = userEvent.setup();
    inst.modsPlanInstall.mockRejectedValue("Close Stardew Valley first.");
    render(<Drawer />);
    const files = within(await screen.findByRole("region", { name: "Files" }));
    await user.click(files.getByRole("button", { name: "Install" }));
    expect(await screen.findByText("Close Stardew Valley first.")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

describe("the Installed tab", () => {
  it("Uninstall asks first, with what it deletes and puts back", async () => {
    const user = userEvent.setup();
    inst.modsInstalled.mockResolvedValue({
      ours: [{ key: "nexus:2100", name: "Better Crops", version: "1.0.0", offlineOnly: null }],
      others: [{ name: "Workshop item 5", detail: "1.0 MB, from Steam Workshop", link: null }],
      recovered: null,
    });
    inst.modsPlanUninstall.mockResolvedValue({
      token: 9,
      title: "Uninstall Better Crops?",
      primary: "Uninstall",
      into: null,
      lists: [
        { title: "Deletes (1)", rows: ["Mods/BetterCrops/BetterCrops.dll"] },
        { title: "Puts back (1)", rows: ["Mods/Shared/config.json"] },
      ],
      notes: [],
    });
    inst.modsConfirm.mockResolvedValue("Better Crops is out; the folder is as it was.");
    render(<Installed />);
    const rows = within(await screen.findByRole("list", { name: "Installed mods" }));
    expect(screen.getByText("2 on this PC")).toBeInTheDocument();
    expect(rows.getByText("1.0.0, installed by Loadout")).toBeInTheDocument();
    expect(rows.getByText("Workshop item 5")).toBeInTheDocument();
    await user.click(rows.getByRole("button", { name: "Uninstall" }));
    expect(inst.modsPlanUninstall).toHaveBeenCalledWith("stardew", "nexus:2100");
    const sheet = within(await screen.findByRole("dialog", { name: "Uninstall Better Crops?" }));
    expect(inst.modsConfirm).not.toHaveBeenCalled();
    expect(sheet.getByText("Mods/Shared/config.json")).toBeInTheDocument();
    await user.click(sheet.getByRole("button", { name: "Uninstall" }));
    expect(inst.modsConfirm).toHaveBeenCalledWith(9);
    expect(await screen.findByText("Better Crops is out; the folder is as it was.")).toBeInTheDocument();
    // What is installed is read again after the change.
    expect(inst.modsInstalled).toHaveBeenCalledTimes(2);
  });

  it("says when nothing is installed", async () => {
    inst.modsInstalled.mockResolvedValue({ ours: [], others: [], recovered: null });
    render(<Installed />);
    expect(
      await screen.findByText("No mods found for this game yet. Mods Loadout installs will list here too."),
    ).toBeInTheDocument();
  });
});

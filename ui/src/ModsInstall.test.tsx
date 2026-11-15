import "@testing-library/jest-dom/vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ModGame, ModRow } from "./api";
import type { ModRequested, ModSheet } from "./modsInstallApi";

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
  modsRequested: vi.fn<() => Promise<ModRequested | null>>(() => Promise.resolve(null)),
  modsDropRequest: vi.fn(() => Promise.resolve()),
  onModsRequested: vi.fn((handler: () => void) => {
    asked.handler = handler;
    return Promise.resolve(() => {});
  }),
}));
const asked = vi.hoisted(() => ({ handler: null as (() => void) | null }));
vi.mock("./modsInstallApi", () => inst);

import { InstalledTab, InstallSheet, ModDrawerPanel, RequestedCard, StepLine, useInstall } from "./ModsInstall";

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

function Asked({ offline = false }: { offline?: boolean }) {
  const install = useInstall();
  return (
    <>
      <RequestedCard install={install} offline={offline} />
      <StepLine install={install} />
      <InstallSheet install={install} />
    </>
  );
}

const ASKED: ModRequested = {
  key: "nexus:2100",
  game: "stardew",
  source: "nexus",
  sourceLabel: "Nexus Mods",
  title: "Install Better Crops for Stardew Valley?",
  line: "BetterCrops.zip · 1.0.0 · 880 KB · from Nexus Mods · asked for in chat",
  risk: "Singleplayer mods only.",
  blocked: null,
  mod: MOD,
  file: { id: "7", name: "Main file", line: "1.0.0 · 880 KB", offer: { kind: "install" }, waiting: false },
};

beforeEach(() => {
  vi.clearAllMocks();
  asked.handler = null;
  inst.modsRequested.mockResolvedValue(null);
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

describe("the chat's Install card", () => {
  it("shows the ask, and its Install plans through the same sheet", async () => {
    const user = userEvent.setup();
    inst.modsRequested.mockResolvedValueOnce(ASKED);
    render(<Asked />);
    const card = within(await screen.findByRole("region", { name: "Asked for in chat" }));
    expect(card.getByText("Install Better Crops for Stardew Valley?")).toBeInTheDocument();
    expect(
      card.getByText("BetterCrops.zip · 1.0.0 · 880 KB · from Nexus Mods · asked for in chat"),
    ).toBeInTheDocument();
    expect(card.getByText("Singleplayer mods only.")).toBeInTheDocument();
    await user.click(card.getByRole("button", { name: "Install" }));
    expect(inst.modsPlanInstall).toHaveBeenCalledWith("stardew", "nexus", "2100", "Better Crops", "7");
    // The plan answered the ask: the card reads it again and goes.
    await screen.findByRole("dialog", { name: "Install Better Crops 1.0.0?" });
    await waitFor(() => expect(screen.queryByRole("region", { name: "Asked for in chat" })).not.toBeInTheDocument());
    expect(inst.modsConfirm).not.toHaveBeenCalled();
  });

  it("Dismiss drops only the ask it shows", async () => {
    const user = userEvent.setup();
    inst.modsRequested.mockResolvedValueOnce(ASKED);
    render(<Asked />);
    const card = within(await screen.findByRole("region", { name: "Asked for in chat" }));
    await user.click(card.getByRole("button", { name: "Dismiss" }));
    expect(inst.modsDropRequest).toHaveBeenCalledWith("nexus:2100");
    expect(screen.queryByRole("region", { name: "Asked for in chat" })).not.toBeInTheDocument();
    expect(inst.modsPlanInstall).not.toHaveBeenCalled();
  });

  it("a new ask from the chat shows without reopening the page", async () => {
    render(<Asked />);
    await waitFor(() => expect(inst.modsRequested).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("region", { name: "Asked for in chat" })).not.toBeInTheDocument();
    inst.modsRequested.mockResolvedValueOnce(ASKED);
    await waitFor(() => expect(asked.handler).not.toBeNull());
    act(() => asked.handler?.());
    expect(await screen.findByText("Install Better Crops for Stardew Valley?")).toBeInTheDocument();
  });

  it("a game the app can't write says why and offers no Install", async () => {
    inst.modsRequested.mockResolvedValueOnce({
      ...ASKED,
      blocked: "Vortex looks after Stardew Valley on this PC, so install its mods there.",
      file: { ...ASKED.file, offer: { kind: "blocked" } },
    });
    render(<Asked />);
    const card = within(await screen.findByRole("region", { name: "Asked for in chat" }));
    expect(
      card.getByText("Vortex looks after Stardew Valley on this PC, so install its mods there."),
    ).toBeInTheDocument();
    expect(card.queryByRole("button", { name: "Install" })).not.toBeInTheDocument();
    expect(card.getByRole("button", { name: "Dismiss" })).toBeInTheDocument();
  });

  it("Offline only shows the ask with no way to fetch the file", async () => {
    inst.modsRequested.mockResolvedValueOnce(ASKED);
    render(<Asked offline />);
    const card = within(await screen.findByRole("region", { name: "Asked for in chat" }));
    expect(card.getByText("Offline only is on.")).toBeInTheDocument();
    expect(card.queryByRole("button", { name: "Install" })).not.toBeInTheDocument();
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

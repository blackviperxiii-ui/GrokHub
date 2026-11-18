import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FleetMachine, FleetRow, FleetUsage } from "./api";

const api = vi.hoisted(() => ({
  fleetMachines: vi.fn<() => Promise<FleetMachine[]>>(),
  fleetUsage: vi.fn<() => Promise<FleetUsage>>(),
  getFleetRows: vi.fn<() => Promise<FleetRow[]>>(),
  setFleetRow: vi.fn<(row: FleetRow) => Promise<FleetRow[]>>(),
}));

vi.mock("./api", () => api);

import { Fleet, machineLine, NO_CLAUDE, NO_GROK, NO_OTHERS, PCT_ONLY, pctValue, resetsIn, ROLLING_NOTE, seenLine, tokens, tokensLine, usedOf } from "./Fleet";

const MINUTE = 60_000;
const NOW = Date.now();
const DESK: FleetMachine = {
  id: "this",
  name: "desk-1",
  cpuPct: 23,
  ramMb: 16384,
  freeRamMb: 7168,
  appVersion: "3.6.0",
  atMs: NOW,
  online: true,
  thisComputer: true,
};
const LAPTOP: FleetMachine = {
  id: "p1",
  name: "laptop-2",
  cpuPct: null,
  ramMb: 8192,
  freeRamMb: 2048,
  appVersion: "3.5.0",
  atMs: NOW - 20 * MINUTE,
  online: false,
  thisComputer: false,
};
const TOKENS = { input: 1_200, output: 340_000, cacheWrite: 100_000, cacheRead: 2_000_000 };
const USAGE: FleetUsage = {
  claude: {
    window: TOKENS,
    windowResetsAtMs: NOW + 27 * MINUTE,
    windowRolling: false,
    week: { input: 100_000, output: 1_500_000, cacheWrite: 0, cacheRead: 9_000_000 },
    lastAtMs: NOW - 2 * MINUTE,
  },
  grok: { requestsLeft: 10, requestsLimit: 100, tokensLeft: null, tokensLimit: null, resetsAtMs: NOW + 90 * MINUTE, seenMs: NOW },
};
const EMPTY_ROWS: FleetRow[] = [
  { id: "cursor", windowPct: null, windowResetsAtMs: null, periodPct: null, periodResetsAtMs: null, updatedAt: 0 },
  { id: "antigravity", windowPct: null, windowResetsAtMs: null, periodPct: null, periodResetsAtMs: null, updatedAt: 0 },
];

beforeEach(() => {
  vi.clearAllMocks();
  api.fleetMachines.mockResolvedValue([DESK, LAPTOP]);
  api.fleetUsage.mockResolvedValue(USAGE);
  api.getFleetRows.mockResolvedValue(EMPTY_ROWS);
});

describe("Fleet lines", () => {
  it("shortens token counts", () => {
    expect(tokens(950)).toBe("950");
    expect(tokens(340_000)).toBe("340k");
    expect(tokens(999_499)).toBe("999k");
    expect(tokens(999_600)).toBe("1.0M");
    expect(tokens(2_101_200)).toBe("2.1M");
    expect(tokensLine(TOKENS)).toBe("2.1M in · 340k out");
  });

  it("says when a limit resets", () => {
    expect(resetsIn(NOW + 27 * MINUTE, NOW)).toBe("resets in 27 min");
    expect(resetsIn(NOW + 70 * MINUTE, NOW)).toBe("resets in 1 h 10 min");
    expect(resetsIn(NOW + 120 * MINUTE, NOW)).toBe("resets in 2 h");
    expect(resetsIn(NOW + 3 * 24 * 60 * MINUTE, NOW)).toBe("resets in 3 days");
    expect(resetsIn(NOW - MINUTE, NOW)).toBe("reset");
  });

  it("describes a machine and when it was seen", () => {
    expect(machineLine(DESK)).toBe("CPU 23% · 9.0 of 16 GB used · 3.6.0");
    expect(machineLine(LAPTOP)).toBe("CPU – · 6.0 of 8.0 GB used · 3.5.0");
    expect(machineLine({ ...LAPTOP, ramMb: null, freeRamMb: null })).toBe("CPU – · Memory – · 3.5.0");
    expect(seenLine(DESK, NOW)).toBe("This computer");
    expect(seenLine(LAPTOP, NOW)).toBe("Asleep · last seen 20 min ago");
    expect(seenLine({ ...LAPTOP, online: true, atMs: NOW - MINUTE }, NOW)).toBe("Online · 1 min ago");
  });

  it("reads a percent field: empty clears, over 100 caps, anything else is refused", () => {
    expect(pctValue("")).toBeNull();
    expect(pctValue(" 40% ")).toBe(40);
    expect(pctValue("140")).toBe(100);
    expect(pctValue("forty")).toBeUndefined();
    expect(pctValue("4o")).toBeUndefined();
    expect(pctValue("-5")).toBeUndefined();
  });

  it("works out the part of a limit used", () => {
    expect(usedOf(10, 100)).toBe(0.9);
    expect(usedOf(null, 100)).toBeNull();
    expect(usedOf(5, 0)).toBeNull();
    expect(usedOf(200, 100)).toBe(0);
  });
});

describe("Fleet page", () => {
  it("shows every machine, an asleep one dimmed, and each plan", async () => {
    render(<Fleet />);
    const machines = await screen.findByRole("list", { name: "Machines" });
    const rows = within(machines).getAllByRole("listitem");
    expect(rows.map((row) => row.querySelector(".card-title")?.textContent)).toEqual(["desk-1", "laptop-2"]);
    expect(rows[0]).not.toHaveClass("fleet-machine--asleep");
    expect(rows[1]).toHaveClass("fleet-machine--asleep");
    expect(screen.queryByText(NO_OTHERS)).not.toBeInTheDocument();

    const claude = await screen.findByLabelText("Claude");
    expect(claude).toHaveTextContent("5-hour window: 2.1M in · 340k out, resets in 27 min");
    expect(claude).toHaveTextContent("Last 7 days: 9.1M in · 1.5M out");
    expect(claude).toHaveTextContent("Last used 2 min ago");

    const grok = screen.getByLabelText("Grok");
    expect(grok).toHaveTextContent("resets in 1 h 30 min");
    expect(grok).toHaveTextContent("Requests: 10 of 100 left");
    const bar = within(grok).getByRole("progressbar", { name: "Grok requests used" });
    expect(bar).toHaveAttribute("aria-valuenow", "90");
    expect(bar).toHaveClass("glow-bar--hot", "fleet-bar");
    expect(grok.querySelector(".usage-ring--hot .usage-ring-pct")).toHaveTextContent("90%");

    expect(screen.getByLabelText("Cursor")).toHaveTextContent("Not filled in yet.");
    expect(screen.getByLabelText("Antigravity")).toHaveTextContent("Not filled in yet.");
  });

  it("says what's missing instead of a blank", async () => {
    api.fleetMachines.mockResolvedValue([DESK]);
    api.fleetUsage.mockResolvedValue({ claude: null, grok: null });
    render(<Fleet />);
    expect(await screen.findByText(NO_OTHERS)).toBeInTheDocument();
    expect(await screen.findByText(NO_CLAUDE)).toBeInTheDocument();
    expect(screen.getByText(NO_GROK)).toBeInTheDocument();
  });

  it("marks a rolling Claude window", async () => {
    api.fleetUsage.mockResolvedValue({ ...USAGE, claude: { ...USAGE.claude!, windowRolling: true, windowResetsAtMs: null } });
    render(<Fleet />);
    const claude = await screen.findByLabelText("Claude");
    expect(claude).toHaveTextContent(ROLLING_NOTE);
    expect(claude).not.toHaveTextContent("resets in");
  });

  it("saves a Cursor row the user fills in, clamped to 100", async () => {
    const saved: FleetRow = { id: "cursor", windowPct: null, windowResetsAtMs: null, periodPct: 100, periodResetsAtMs: null, updatedAt: NOW };
    api.setFleetRow.mockResolvedValue([saved, EMPTY_ROWS[1]]);
    render(<Fleet />);
    const cursor = await screen.findByLabelText("Cursor");
    await userEvent.click(within(cursor).getByRole("button", { name: "Fill in" }));
    await userEvent.type(within(cursor).getByLabelText("This period % used"), "140");
    await userEvent.click(within(cursor).getByRole("button", { name: "Save" }));
    expect(api.setFleetRow).toHaveBeenCalledWith({
      id: "cursor",
      windowPct: null,
      windowResetsAtMs: null,
      periodPct: 100,
      periodResetsAtMs: null,
      updatedAt: 0,
    });
    expect(await within(cursor).findByText("You filled this in just now.")).toBeInTheDocument();
    expect(within(cursor).getByRole("progressbar", { name: "Cursor period used" })).toHaveAttribute("aria-valuenow", "100");
    expect(within(cursor).getByRole("button", { name: "Update" })).toBeInTheDocument();
  });

  it("keeps the form and what was typed when a save fails or a percent isn't a number", async () => {
    const saved: FleetRow = { ...EMPTY_ROWS[0], windowPct: 40, updatedAt: NOW - 5 * MINUTE };
    api.getFleetRows.mockResolvedValue([saved, EMPTY_ROWS[1]]);
    api.setFleetRow.mockRejectedValue("No space left on device");
    render(<Fleet />);
    const cursor = await screen.findByLabelText("Cursor");
    await userEvent.click(within(cursor).getByRole("button", { name: "Update" }));
    const field = within(cursor).getByLabelText("Short window % used");
    await userEvent.clear(field);
    await userEvent.type(field, "forty");
    await userEvent.click(within(cursor).getByRole("button", { name: "Save" }));
    expect(within(cursor).getByText(PCT_ONLY)).toBeInTheDocument();
    expect(api.setFleetRow).not.toHaveBeenCalled();

    await userEvent.clear(field);
    await userEvent.type(field, "77");
    await userEvent.click(within(cursor).getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("status")).toHaveTextContent("No space left on device");
    expect(api.setFleetRow).toHaveBeenCalledTimes(1);
    expect(within(cursor).getByLabelText("Short window % used")).toHaveValue("77");
    expect(within(cursor).queryByText(PCT_ONLY)).not.toBeInTheDocument();
  });

  it("refreshes on Refresh and shows a failure without dropping what it had", async () => {
    render(<Fleet />);
    await screen.findByLabelText("Claude");
    expect(api.fleetMachines).toHaveBeenCalledTimes(1);
    api.fleetMachines.mockRejectedValue("The hub isn't running.");
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByRole("status")).toHaveTextContent("The hub isn't running.");
    expect(api.fleetMachines).toHaveBeenCalledTimes(2);
    expect(api.fleetUsage).toHaveBeenCalledTimes(2);
    expect(screen.getByText("laptop-2")).toBeInTheDocument();
  });

  it("reads nothing while hidden", () => {
    render(<Fleet shown={false} />);
    expect(api.fleetMachines).not.toHaveBeenCalled();
    expect(api.fleetUsage).not.toHaveBeenCalled();
  });
});

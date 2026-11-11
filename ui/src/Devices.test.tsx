import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Devices as Shape, Share } from "./api";

const api = vi.hoisted(() => ({
  getDevices: vi.fn<() => Promise<Shape>>(),
  forgetAllDevices: vi.fn<() => Promise<Shape>>(),
  getShare: vi.fn<() => Promise<Share>>(),
  startShare: vi.fn<() => Promise<Share>>(),
  stopShare: vi.fn<() => Promise<Share>>(),
  newShareCode: vi.fn<() => Promise<Share>>(),
}));

vi.mock("./api", () => api);

import {
  ago,
  browserLine,
  Devices,
  NONE_PAIRED,
  OFF_NOTE,
  PAIR_STEP,
  CODE_GONE,
  PAIRED_NOTE,
  RUN_NOTE,
  SHARE_OFF,
  SIGNED_OUT,
} from "./Devices";

const MINUTE = 60_000;
const NOW = Date.now();
const ON: Shape = {
  computer: "desk-1",
  on: true,
  lan: "https://192.168.1.20:4620",
  browsers: [
    { name: "Firefox on Android", firstSeen: NOW - 30 * MINUTE, lastCall: NOW, chats: 2 },
    { name: "Safari on iPhone", firstSeen: NOW - 3 * 60 * MINUTE, lastCall: NOW - 5 * MINUTE, chats: 1 },
  ],
};
const SHARE_OFF_VIEW: Share = { on: false, port: 18766, code: "", address: "", pairedSinceCode: false, peers: [] };
const SHARE_ON_VIEW: Share = {
  on: true,
  port: 18766,
  code: "K7M2QX",
  address: "http://192.168.1.20:18766",
  pairedSinceCode: false,
  peers: [],
};
const SIGNED_OUT_VIEW: Shape = { ...ON, browsers: [] };
const OFF: Shape = { computer: "desk-1", on: false, lan: "", browsers: [] };

describe("ago", () => {
  it("says how long ago in minutes, hours or days", () => {
    expect(ago(NOW, NOW)).toBe("just now");
    expect(ago(NOW + 5_000, NOW)).toBe("just now");
    expect(ago(NOW - 59_999, NOW)).toBe("just now");
    expect(ago(NOW - 5 * MINUTE, NOW)).toBe("5 min ago");
    expect(ago(NOW - 125 * MINUTE, NOW)).toBe("2 h ago");
    expect(ago(NOW - 24 * 60 * MINUTE, NOW)).toBe("1 day ago");
    expect(ago(NOW - 3 * 24 * 60 * MINUTE, NOW)).toBe("3 days ago");
  });

  it("puts a browser's pairing, last use and chats on one line", () => {
    expect(browserLine(ON.browsers[1], NOW)).toBe("Paired 3 h ago · Last used 5 min ago · 1 chat");
  });
});

describe("Devices", () => {
  beforeEach(() => {
    api.getDevices.mockReset();
    api.forgetAllDevices.mockReset();
    api.getShare.mockReset();
    api.startShare.mockReset();
    api.stopShare.mockReset();
    api.newShareCode.mockReset();
    api.getShare.mockResolvedValue(SHARE_OFF_VIEW);
  });

  it("shows this computer, web access and the paired browsers", async () => {
    api.getDevices.mockResolvedValue(ON);
    const open = vi.fn();
    render(<Devices onOpenWebAccess={open} />);
    const list = await screen.findByRole("list", { name: "Paired browsers" });
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((row) => row.querySelector(".card-title")?.textContent)).toEqual([
      "Firefox on Android",
      "Safari on iPhone",
    ]);
    expect(rows[0]).toHaveTextContent("Last used just now · 2 chats");
    expect(screen.getByLabelText("This computer")).toHaveTextContent("desk-1");
    expect(screen.getByText("https://192.168.1.20:4620")).toBeInTheDocument();
    expect(screen.getByLabelText("Web access")).toHaveTextContent("On");

    await userEvent.click(screen.getByRole("button", { name: "Open Web access settings" }));
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("signs out every browser with one click", async () => {
    api.getDevices.mockResolvedValue(ON);
    api.forgetAllDevices.mockResolvedValue(SIGNED_OUT_VIEW);
    render(<Devices onOpenWebAccess={() => {}} />);
    await screen.findByRole("list", { name: "Paired browsers" });

    await userEvent.click(screen.getByRole("button", { name: "Sign out all browsers" }));
    expect(api.forgetAllDevices).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("status")).toHaveTextContent(SIGNED_OUT);
    expect(screen.queryByRole("list", { name: "Paired browsers" })).toBeNull();
    expect(screen.getByText(NONE_PAIRED)).toBeInTheDocument();
  });

  it("says web access is off and keeps the button off", async () => {
    api.getDevices.mockResolvedValue(OFF);
    render(<Devices onOpenWebAccess={() => {}} />);
    expect(await screen.findByText(OFF_NOTE)).toBeInTheDocument();
    expect(screen.getByLabelText("Web access")).toHaveTextContent("Off");
    expect(screen.getByRole("button", { name: "Sign out all browsers" })).toBeDisabled();
    expect(screen.queryByText("https://192.168.1.20:4620")).toBeNull();
  });

  it("shows why the list couldn't be read", async () => {
    api.getDevices.mockRejectedValue("Not on this platform.");
    render(<Devices onOpenWebAccess={() => {}} />);
    expect(await screen.findByRole("status")).toHaveTextContent("Not on this platform.");
  });

  it("starts share, shows the code and address, then stops", async () => {
    api.getDevices.mockResolvedValue(OFF);
    api.startShare.mockResolvedValue(SHARE_ON_VIEW);
    api.stopShare.mockResolvedValue(SHARE_OFF_VIEW);
    render(<Devices onOpenWebAccess={() => {}} />);
    const card = await screen.findByLabelText("Share");
    expect(await within(card).findByText(SHARE_OFF)).toBeInTheDocument();

    await userEvent.click(within(card).getByRole("button", { name: "Start share" }));
    expect(api.startShare).toHaveBeenCalledTimes(1);
    expect(await screen.findByLabelText("Pairing code")).toHaveTextContent("K7M2QX");
    expect(card).toHaveTextContent(`${PAIR_STEP} http://192.168.1.20:18766`);
    expect(card).toHaveTextContent("On, port 18766");

    await userEvent.click(within(card).getByRole("button", { name: "Stop share" }));
    expect(api.stopShare).toHaveBeenCalledTimes(1);
    expect(await within(card).findByText(SHARE_OFF)).toBeInTheDocument();
    expect(screen.queryByLabelText("Pairing code")).toBeNull();
  });

  it("makes a new code and lists a paired computer", async () => {
    api.getDevices.mockResolvedValue(OFF);
    api.getShare.mockResolvedValue({
      ...SHARE_ON_VIEW,
      code: "",
      pairedSinceCode: true,
      peers: [{ id: "laptop-1", name: "Laptop", lastSeen: 0 }],
    });
    api.newShareCode.mockResolvedValue({ ...SHARE_ON_VIEW, code: "P4R8TZ" });
    render(<Devices onOpenWebAccess={() => {}} />);
    const card = await screen.findByLabelText("Share");
    expect(await within(card).findByText(PAIRED_NOTE)).toBeInTheDocument();
    const peers = screen.getByRole("list", { name: "Paired computers" });
    expect(peers).toHaveTextContent("Laptop");
    expect(peers).toHaveTextContent("Paired, not used yet");

    await userEvent.click(within(card).getByRole("button", { name: "New code" }));
    expect(await screen.findByLabelText("Pairing code")).toHaveTextContent("P4R8TZ");
  });

  it("a dead code with nobody paired does not say Paired", async () => {
    api.getDevices.mockResolvedValue(OFF);
    // Ten wrong guesses burned the code: sharing is on, no code, nobody joined.
    api.getShare.mockResolvedValue({ ...SHARE_ON_VIEW, code: "", pairedSinceCode: false });
    render(<Devices onOpenWebAccess={() => {}} />);
    const card = await screen.findByLabelText("Share");
    expect(await within(card).findByText("That code stopped working. Make a new one.")).toBeInTheDocument();
    expect(CODE_GONE).toBe("That code stopped working. Make a new one.");
    expect(within(card).queryByText(PAIRED_NOTE)).toBeNull();
    expect(screen.queryByRole("list", { name: "Paired computers" })).toBeNull();
    expect(within(card).getByRole("button", { name: "New code" })).toBeEnabled();
  });

  it("a code that stopped working after an earlier pairing does not say Paired either", async () => {
    api.getDevices.mockResolvedValue(OFF);
    api.getShare.mockResolvedValue({
      ...SHARE_ON_VIEW,
      code: "",
      pairedSinceCode: false,
      peers: [{ id: "laptop-1", name: "Laptop", lastSeen: 0 }],
    });
    render(<Devices onOpenWebAccess={() => {}} />);
    const card = await screen.findByLabelText("Share");
    expect(await within(card).findByText(CODE_GONE)).toBeInTheDocument();
    expect(within(card).queryByText(PAIRED_NOTE)).toBeNull();
  });

  it("lists two paired computers with the same name as two rows", async () => {
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    api.getDevices.mockResolvedValue(OFF);
    api.getShare.mockResolvedValue({
      ...SHARE_ON_VIEW,
      peers: [
        { id: "pc-a", name: "Computer", lastSeen: 0 },
        { id: "pc-b", name: "Computer", lastSeen: NOW - 5 * MINUTE },
      ],
    });
    render(<Devices onOpenWebAccess={() => {}} />);
    const rows = within(await screen.findByRole("list", { name: "Paired computers" })).getAllByRole("listitem");
    expect(rows.map((row) => row.textContent)).toEqual([
      "ComputerPaired, not used yet",
      "ComputerLast used 5 min ago",
    ]);
    expect(errors).not.toHaveBeenCalled();
    errors.mockRestore();
  });

  it("shows why share could not start", async () => {
    api.getDevices.mockResolvedValue(OFF);
    api.startShare.mockRejectedValue("Port 18766 is already in use.");
    render(<Devices onOpenWebAccess={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Start share" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Port 18766 is already in use.");
  });

  it("says the task runs here, under the usual approvals", async () => {
    api.getDevices.mockResolvedValue(OFF);
    render(<Devices onOpenWebAccess={() => {}} onRun={vi.fn()} />);
    expect(await screen.findByRole("button", { name: "Run it here" })).toBeInTheDocument();
    expect(screen.getByText(RUN_NOTE)).toBeInTheDocument();
    expect(RUN_NOTE).toBe(
      "Opens a chat with this task, so it runs on this PC with the same approvals as anything else you ask.",
    );
  });

  it("hands a trimmed task to the chat from Run it here", async () => {
    api.getDevices.mockResolvedValue(OFF);
    const run = vi.fn();
    render(<Devices onOpenWebAccess={() => {}} onRun={run} />);
    const button = await screen.findByRole("button", { name: "Run it here" });
    expect(button).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Task for this PC"), "  tidy my downloads  ");
    await userEvent.click(button);
    expect(run).toHaveBeenCalledWith("tidy my downloads");
    expect(screen.getByLabelText("Task for this PC")).toHaveValue("");
  });
});

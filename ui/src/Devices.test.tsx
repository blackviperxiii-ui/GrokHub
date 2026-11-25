import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Companions, Devices as Shape, HubJoin, Share } from "./api";

const api = vi.hoisted(() => ({
  getDevices: vi.fn<() => Promise<Shape>>(),
  forgetAllDevices: vi.fn<() => Promise<Shape>>(),
  getShare: vi.fn<() => Promise<Share>>(),
  startShare: vi.fn<() => Promise<Share>>(),
  stopShare: vi.fn<() => Promise<Share>>(),
  newShareCode: vi.fn<() => Promise<Share>>(),
  getHubJoin: vi.fn<() => Promise<HubJoin>>(),
  joinHub: vi.fn<(address: string, code: string) => Promise<HubJoin>>(),
  leaveHub: vi.fn<() => Promise<HubJoin>>(),
  getCompanions: vi.fn<() => Promise<Companions>>(),
  startCompanions: vi.fn<() => Promise<Companions>>(),
  stopCompanions: vi.fn<() => Promise<Companions>>(),
  newCompanionCode: vi.fn<() => Promise<Companions>>(),
  approveCompanion: vi.fn<(id: string, allowed: string[]) => Promise<Companions>>(),
  setCompanionAllowed: vi.fn<(id: string, allowed: string[]) => Promise<Companions>>(),
  removeCompanion: vi.fn<(id: string) => Promise<Companions>>(),
}));

vi.mock("./api", () => api);

import {
  ago,
  browserLine,
  companionLine,
  COMPANIONS_OFF,
  NO_OFFERS,
  pairCommand,
  Devices,
  JOIN_OFF,
  joinLine,
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

const NOT_JOINED: HubJoin = { joined: null, lastSentAt: null, lastError: null };
const JOINED: HubJoin = {
  joined: { address: "192.168.1.20:18766", hubId: "d-desk", hubName: "Desk", joinedAt: NOW },
  lastSentAt: NOW - 2 * MINUTE,
  lastError: null,
};

const COMPANIONS_OFF_VIEW: Companions = { on: false, port: 18767, code: "", address: "", companions: [] };
const COMPANIONS_ON: Companions = { on: true, port: 18767, code: "ABC-123", address: "192.168.1.20", companions: [] };
const PIXEL_PENDING: Companions["companions"][number] = {
  id: "pixel",
  name: "Pixel",
  platform: "android",
  approved: false,
  offers: ["camera", "sms"],
  allowed: [],
  online: true,
  pairedAt: NOW - 5 * MINUTE,
};
const PIXEL_YES = { ...PIXEL_PENDING, approved: true, allowed: ["camera"] };

describe("companions", () => {
  it("says the command to pair and each companion's state", () => {
    expect(pairCommand(COMPANIONS_ON)).toBe("grokhub-hub node pair 192.168.1.20 ABC-123");
    expect(pairCommand({ ...COMPANIONS_ON, address: "" })).toBe(
      "grokhub-hub node pair <this computer's address> ABC-123",
    );
    expect(companionLine(PIXEL_PENDING, NOW)).toBe("Android · Waiting for your yes · Paired 5 min ago");
    expect(companionLine(PIXEL_YES, NOW)).toBe("Android · Online · Paired 5 min ago");
    expect(companionLine({ ...PIXEL_YES, online: false, platform: "plan9" }, NOW)).toBe(
      "Other device · Offline · Paired 5 min ago",
    );
  });
});

describe("joinLine", () => {
  it("names the computer, its address and how the last send went", () => {
    expect(joinLine(NOT_JOINED, NOW)).toBe(JOIN_OFF);
    expect(joinLine(JOINED, NOW)).toBe("Sending to Desk (192.168.1.20:18766). Last sent 2 min ago.");
    expect(joinLine({ ...JOINED, lastSentAt: null }, NOW)).toBe("Sending to Desk (192.168.1.20:18766).");
    expect(joinLine({ ...JOINED, lastError: "Desk isn't sharing right now." }, NOW)).toBe(
      "Sending to Desk (192.168.1.20:18766). Desk isn't sharing right now.",
    );
  });
});

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
    api.getHubJoin.mockReset();
    api.joinHub.mockReset();
    api.leaveHub.mockReset();
    api.getHubJoin.mockResolvedValue(NOT_JOINED);
    for (const fn of [
      api.getCompanions,
      api.startCompanions,
      api.stopCompanions,
      api.newCompanionCode,
      api.approveCompanion,
      api.setCompanionAllowed,
      api.removeCompanion,
    ])
      fn.mockReset();
    api.getCompanions.mockResolvedValue(COMPANIONS_OFF_VIEW);
  });

  it("starts companions and shows the code and the command to type on the companion", async () => {
    api.getDevices.mockResolvedValue(OFF);
    api.startCompanions.mockResolvedValue(COMPANIONS_ON);
    render(<Devices onOpenWebAccess={() => {}} />);
    const card = await screen.findByRole("generic", { name: "Companions" });
    expect(await within(card).findByText(COMPANIONS_OFF)).toBeInTheDocument();
    await userEvent.click(within(card).getByRole("button", { name: "Start companions" }));
    expect(api.startCompanions).toHaveBeenCalledTimes(1);
    expect(within(card).getByLabelText("Companion pairing code")).toHaveTextContent("ABC-123");
    expect(within(card).getByText("grokhub-hub node pair 192.168.1.20 ABC-123")).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Stop companions" })).toBeEnabled();
  });

  it("allows only what is ticked for a waiting companion, then removes it", async () => {
    api.getDevices.mockResolvedValue(OFF);
    api.getCompanions.mockResolvedValue({
      ...COMPANIONS_ON,
      code: "",
      companions: [PIXEL_PENDING, { ...PIXEL_PENDING, id: "box", name: "Box", offers: [] }],
    });
    api.approveCompanion.mockResolvedValue({ ...COMPANIONS_ON, code: "", companions: [PIXEL_YES] });
    api.removeCompanion.mockResolvedValue({ ...COMPANIONS_ON, code: "", companions: [] });
    render(<Devices onOpenWebAccess={() => {}} />);
    const row = await screen.findByRole("listitem", { name: "Pixel" });
    expect(within(row).getByText("Android · Waiting for your yes · Paired 5 min ago")).toBeInTheDocument();
    expect(within(screen.getByRole("listitem", { name: "Box" })).getByText(NO_OFFERS)).toBeInTheDocument();
    const camera = within(row).getByRole("checkbox", { name: "Camera" });
    expect(camera).not.toBeChecked();
    expect(within(row).getByRole("checkbox", { name: "Text messages" })).not.toBeChecked();
    await userEvent.click(camera);
    await userEvent.click(within(row).getByRole("button", { name: "Allow" }));
    expect(api.approveCompanion).toHaveBeenCalledWith("pixel", ["camera"]);

    const saved = await screen.findByRole("listitem", { name: "Pixel" });
    expect(within(saved).getByText("Android · Online · Paired 5 min ago")).toBeInTheDocument();
    expect(within(saved).getByRole("checkbox", { name: "Camera" })).toBeChecked();
    expect(within(saved).getByRole("button", { name: "Save" })).toBeDisabled();
    await userEvent.click(within(saved).getByRole("button", { name: "Remove" }));
    expect(api.removeCompanion).toHaveBeenCalledWith("pixel");
    expect(screen.queryByRole("listitem", { name: "Pixel" })).not.toBeInTheDocument();
  });

  it("changing what an approved companion may lend saves the new set", async () => {
    api.getDevices.mockResolvedValue(OFF);
    api.getCompanions.mockResolvedValue({ ...COMPANIONS_ON, code: "", companions: [PIXEL_YES] });
    api.setCompanionAllowed.mockResolvedValue({
      ...COMPANIONS_ON,
      code: "",
      companions: [{ ...PIXEL_YES, allowed: ["camera", "sms"] }],
    });
    render(<Devices onOpenWebAccess={() => {}} />);
    const row = await screen.findByRole("listitem", { name: "Pixel" });
    await userEvent.click(within(row).getByRole("checkbox", { name: "Text messages" }));
    await userEvent.click(within(row).getByRole("button", { name: "Save" }));
    expect(api.setCompanionAllowed).toHaveBeenCalledWith("pixel", ["camera", "sms"]);
    expect(api.approveCompanion).not.toHaveBeenCalled();
  });

  it("joins another computer with its address and code, then leaves it", async () => {
    api.getDevices.mockResolvedValue(OFF);
    api.joinHub.mockResolvedValue(JOINED);
    api.leaveHub.mockResolvedValue(NOT_JOINED);
    render(<Devices onOpenWebAccess={() => {}} />);
    const card = await screen.findByLabelText("Join");
    expect(await within(card).findByText(JOIN_OFF)).toBeInTheDocument();
    const join = within(card).getByRole("button", { name: "Join" });
    expect(join).toBeDisabled();

    await userEvent.type(within(card).getByLabelText("Other computer's address"), "http://192.168.1.20:18766");
    await userEvent.type(within(card).getByLabelText("Other computer's code"), "K7M2QX");
    await userEvent.click(join);
    expect(api.joinHub).toHaveBeenCalledWith("http://192.168.1.20:18766", "K7M2QX");
    expect(await within(card).findByText(/^Sending to Desk \(192\.168\.1\.20:18766\)\. Last sent/)).toBeInTheDocument();
    expect(within(card).queryByLabelText("Other computer's code")).toBeNull();

    await userEvent.click(within(card).getByRole("button", { name: "Leave" }));
    expect(api.leaveHub).toHaveBeenCalledTimes(1);
    expect(await within(card).findByText(JOIN_OFF)).toBeInTheDocument();
  });

  it("shows why joining failed and keeps what was typed", async () => {
    api.getDevices.mockResolvedValue(OFF);
    api.joinHub.mockRejectedValue("Pairing code does not match.");
    render(<Devices onOpenWebAccess={() => {}} />);
    const card = await screen.findByLabelText("Join");
    await userEvent.type(await within(card).findByLabelText("Other computer's address"), "192.168.1.20");
    await userEvent.type(within(card).getByLabelText("Other computer's code"), "WRONG1");
    await userEvent.click(within(card).getByRole("button", { name: "Join" }));
    expect(await within(card).findByText("Pairing code does not match.")).toBeInTheDocument();
    expect(within(card).getByLabelText("Other computer's address")).toHaveValue("192.168.1.20");

    api.joinHub.mockResolvedValue(JOINED);
    await userEvent.clear(within(card).getByLabelText("Other computer's code"));
    await userEvent.type(within(card).getByLabelText("Other computer's code"), "K7M2QX");
    await userEvent.click(within(card).getByRole("button", { name: "Join" }));
    expect(await within(card).findByText(/^Sending to Desk/)).toBeInTheDocument();
    expect(within(card).queryByText("Pairing code does not match.")).toBeNull();
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

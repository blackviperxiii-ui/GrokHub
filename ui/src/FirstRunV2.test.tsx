import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OnboardAct, OnboardReply } from "./onboardApi";

const mock = vi.hoisted(() => ({
  onboard: vi.fn<(act: OnboardAct) => Promise<OnboardReply | null>>(),
  connectorCatalog: vi.fn(),
  addCatalogConnector: vi.fn(),
  signInConnector: vi.fn(),
  getSkills: vi.fn(),
  setSkillOn: vi.fn(),
  getLocalAi: vi.fn(),
}));

vi.mock("./onboardApi", async (orig) => ({
  ...(await orig<typeof import("./onboardApi")>()),
  onboard: mock.onboard,
}));

vi.mock("./api", async (orig) => ({
  ...(await orig<typeof import("./api")>()),
  connectorCatalog: mock.connectorCatalog,
  addCatalogConnector: mock.addCatalogConnector,
  signInConnector: mock.signInConnector,
  getSkills: mock.getSkills,
  setSkillOn: mock.setSkillOn,
  getLocalAi: mock.getLocalAi,
}));

import { EMPTY_ANSWERS, OPEN_EVENT } from "./onboardApi";
import { FirstRunV2, SANDBOX_NOTE, TITLE } from "./FirstRunV2";
import { finishFirstRunV2, loadFirstRunV2, reopenFirstRunV2 } from "./firstRunV2Api";

const DONE: OnboardReply = {
  pending: false,
  stage: "done",
  step: "done",
  answers: EMPTY_ANSWERS,
  drafts: null,
  amr: true,
  openclaw: false,
  importSources: [],
  saved: null,
  note: null,
};

beforeEach(() => {
  localStorage.clear();
  reopenFirstRunV2();
  mock.onboard.mockReset().mockResolvedValue(DONE);
  mock.connectorCatalog.mockReset().mockResolvedValue({ rows: [], note: null });
  mock.getSkills.mockReset().mockResolvedValue({ rows: [] });
  mock.getLocalAi.mockReset().mockResolvedValue({
    status: { tier: "", model: "", state: "Not set up", reason: "", tokS: "", memory: "" },
    picker: { models: [], onThisPc: [], selected: "" },
    engine: { state: "missing", version: "", sizeLabel: "", hint: "", button: "", progress: null, error: null },
  } as never);
});

describe("FirstRunV2", () => {
  it("waits while Get to know you is still pending", async () => {
    mock.onboard.mockResolvedValue({ ...DONE, pending: true, stage: "pending", step: "welcome" });
    render(<FirstRunV2 after onAsk={() => {}} onLayout={() => {}} />);
    await waitFor(() => expect(mock.onboard).toHaveBeenCalled());
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("shows after Get to know you is done and walks layout to apps", async () => {
    render(<FirstRunV2 after onAsk={() => {}} onLayout={() => {}} />);
    expect(await screen.findByRole("dialog", { name: "Pick a layout" })).toBeInTheDocument();
    expect(TITLE).toContain("Loadout");
    await userEvent.click(screen.getByRole("button", { name: "Advanced" }));
    await userEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByRole("heading", { name: "Connect apps" })).toBeInTheDocument();
  });

  it("sandbox step never offers a turn-off control", async () => {
    saveAt("sandbox");
    render(<FirstRunV2 after onAsk={() => {}} onLayout={() => {}} />);
    expect(await screen.findByText(SANDBOX_NOTE)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /turn .*off|disable sandbox/i })).toBeNull();
  });

  it("first task sends the prompt and finishes", async () => {
    const ask = vi.fn();
    saveAt("task");
    render(<FirstRunV2 after onAsk={ask} onLayout={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Tour this app" }));
    expect(ask).toHaveBeenCalledWith(
      expect.stringContaining("Walk me through this app"),
    );
    expect(loadFirstRunV2().stage).toBe("done");
  });

  it("Settings reopen event brings the wizard back", async () => {
    finishFirstRunV2(false);
    render(<FirstRunV2 after onAsk={() => {}} onLayout={() => {}} />);
    expect(screen.queryByRole("dialog")).toBeNull();
    window.dispatchEvent(new CustomEvent(OPEN_EVENT, { detail: { firstRunV2: true } }));
    expect(await screen.findByRole("dialog", { name: "Pick a layout" })).toBeInTheDocument();
  });
});

function saveAt(step: "sandbox" | "task") {
  localStorage.setItem("shell.firstRunV2", JSON.stringify({ stage: "pending", step }));
}

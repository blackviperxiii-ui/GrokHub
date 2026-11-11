import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { OnboardReply } from "./onboardApi";

const shell = vi.hoisted(() => ({
  call: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}));
vi.mock("./transport", () => ({ inShell: () => true, call: shell.call }));

import { EMPTY_ANSWERS, NO_WORKSPACE, OPEN_EVENT, openImport } from "./onboardApi";

const VIEW: OnboardReply = {
  pending: false,
  stage: "done",
  step: "done",
  answers: EMPTY_ANSWERS,
  drafts: null,
  amr: true,
  openclaw: false,
  saved: null,
  note: null,
};

describe("openImport (/import)", () => {
  const opened = vi.fn<(e: Event) => void>();

  beforeEach(() => {
    shell.call.mockReset();
    opened.mockReset();
    window.addEventListener(OPEN_EVENT, opened);
  });
  afterEach(() => window.removeEventListener(OPEN_EVENT, opened));

  it("with no workspace on this computer, opens nothing and gives the line for the chat", async () => {
    shell.call.mockResolvedValue(VIEW);
    expect(await openImport()).toBe("No OpenClaw or Hermes workspace on this computer.");
    expect(NO_WORKSPACE).toBe("No OpenClaw or Hermes workspace on this computer.");
    expect(opened).not.toHaveBeenCalled();
    // It only looked: nothing was opened again, drafted or saved.
    expect(shell.call.mock.calls).toEqual([["onboard_identity", { act: { act: "view" } }]]);
  });

  it("when the shell refuses, gives its reason for the chat and opens nothing", async () => {
    shell.call.mockRejectedValue("Get to know you isn't available on this computer.");
    expect(await openImport()).toBe("Get to know you isn't available on this computer.");
    expect(opened).not.toHaveBeenCalled();
  });

  it("with a workspace, opens Review for the import and leaves the chat alone", async () => {
    shell.call.mockResolvedValue({ ...VIEW, openclaw: true });
    expect(await openImport()).toBeNull();
    expect(opened).toHaveBeenCalledTimes(1);
    expect((opened.mock.calls[0][0] as CustomEvent).detail).toEqual({ import: true });
    expect(shell.call.mock.calls).toEqual([["onboard_identity", { act: { act: "view" } }]]);
  });
});

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GrokSignInPoll, GrokSignInStart, Settings } from "./api";

const api = vi.hoisted(() => ({
  startGrokSignIn: vi.fn<() => Promise<GrokSignInStart>>(),
  pollGrokSignIn: vi.fn<() => Promise<GrokSignInPoll>>(),
  grokSignOut: vi.fn<() => Promise<Settings>>(),
  getSettings: vi.fn<() => Promise<Settings>>(),
  openExternal: vi.fn<(url: string) => Promise<void>>(),
}));

vi.mock("./api", () => ({
  startGrokSignIn: api.startGrokSignIn,
  pollGrokSignIn: api.pollGrokSignIn,
  grokSignOut: api.grokSignOut,
  getSettings: api.getSettings,
}));
vi.mock("./external", () => ({ openExternal: api.openExternal }));

import { GrokSignIn, signedInAs } from "./GrokSignIn";

const SETTINGS: Settings = {
  model: "grok-4.7",
  closeToTray: true,
  hasApiKey: false,
  signedIn: null,
  workspace: "",
  workspacePath: "/home/me",
  approvals: "ask",
  desktopControl: false,
  theme: "system",
  version: "3.1.0",
};

const CODE: GrokSignInStart = {
  userCode: "ABCD-EFGH",
  verificationUri: "https://accounts.x.ai/device",
  verificationUriComplete: null,
  interval: 1,
  expiresIn: 600,
};

describe("Sign in with Grok", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    api.startGrokSignIn.mockResolvedValue(CODE);
    api.openExternal.mockResolvedValue();
  });
  afterEach(() => vi.useRealTimers());

  it("names the account in a few words", () => {
    expect(signedInAs({ name: "Sam", email: "sam@example.com" })).toBe("Sam (sam@example.com)");
    expect(signedInAs({ name: null, email: "sam@example.com" })).toBe("sam@example.com");
    expect(signedInAs({ name: null, email: null })).toBe("your Grok account");
  });

  it("shows the code with Open x.ai, waits, then shows the account", async () => {
    api.pollGrokSignIn
      .mockResolvedValueOnce({ status: "pending", interval: 1 })
      .mockResolvedValueOnce({ status: "ready", signedIn: { name: "Sam", email: "sam@example.com" } });
    const after = { ...SETTINGS, signedIn: { name: "Sam", email: "sam@example.com" } };
    api.getSettings.mockResolvedValue(after);
    const onChange = vi.fn();
    render(<GrokSignIn signedIn={null} onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    expect(await screen.findByLabelText("Sign-in code")).toHaveTextContent("ABCD-EFGH");
    await userEvent.click(screen.getByRole("button", { name: "Open x.ai" }));
    expect(api.openExternal).toHaveBeenCalledWith("https://accounts.x.ai/device");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2500);
    });
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(after));
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("status")).toHaveTextContent("Signed in as Sam (sam@example.com).");
  });

  it("a refused code shows why and offers sign-in again", async () => {
    api.pollGrokSignIn.mockResolvedValue({ status: "failed", error: "The sign-in was refused." });
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1500);
    });
    expect(await screen.findByText("The sign-in was refused.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in with Grok" })).toBeInTheDocument();
  });

  it("a start error shows in words", async () => {
    api.startGrokSignIn.mockRejectedValue("Offline only is on, so Loadout can't reach the internet.");
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Offline only is on");
  });

  it("signed in shows the account and Sign out", async () => {
    api.grokSignOut.mockResolvedValue(SETTINGS);
    const onChange = vi.fn();
    render(<GrokSignIn signedIn={{ name: "Sam", email: null }} onChange={onChange} />);
    expect(screen.getByText("Signed in as Sam.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(SETTINGS));
  });
});

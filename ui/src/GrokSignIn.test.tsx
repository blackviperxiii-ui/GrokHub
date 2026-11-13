import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Activity } from "react";
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
  closeToTray: true,
  hasApiKey: false,
  signedIn: null,
  workspace: "",
  workspacePath: "/home/me",
  approvals: "ask",
  desktopControl: false,
  theme: "system",
  version: "3.1.0",
  summonHotkey: false,
  notifications: true,
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
    expect(signedInAs(undefined)).toBe("your Grok account");
    expect(signedInAs(null)).toBe("your Grok account");
  });

  it("a ready poll with no signedIn still shows Signed in as, not a TypeError", async () => {
    // Reproduces the wire shape when Rust sent `signed_in` and the UI expected
    // `signedIn`: status ready, account missing.
    api.pollGrokSignIn.mockResolvedValue({ status: "ready" } as GrokSignInPoll);
    const after = { ...SETTINGS, signedIn: { name: "Sam", email: "sam@example.com" } };
    api.getSettings.mockResolvedValue(after);
    const onChange = vi.fn();
    render(<GrokSignIn signedIn={null} onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2500);
    });
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(after));
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Signed in as your Grok account.");
    expect(status.textContent).not.toMatch(/TypeError|undefined is not an object/);
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

  it("keeps waiting after Settings is hidden and shown again", async () => {
    api.pollGrokSignIn.mockResolvedValue({ status: "ready", signedIn: { name: "Sam", email: null } });
    api.getSettings.mockResolvedValue(SETTINGS);
    const onChange = vi.fn();
    const page = (mode: "visible" | "hidden") => (
      <Activity mode={mode}>
        <GrokSignIn signedIn={null} onChange={onChange} />
      </Activity>
    );
    const { rerender } = render(page("visible"));
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    rerender(page("hidden"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2500);
    });
    expect(api.pollGrokSignIn).not.toHaveBeenCalled();
    rerender(page("visible"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2500);
    });
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(SETTINGS));
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
  });

  it("goes back to waiting at the interval xAI asked for last", async () => {
    api.pollGrokSignIn
      .mockResolvedValueOnce({ status: "pending", interval: 3 })
      .mockResolvedValueOnce({ status: "ready", signedIn: { name: "Sam", email: null } });
    api.getSettings.mockResolvedValue(SETTINGS);
    const onChange = vi.fn();
    const page = (mode: "visible" | "hidden") => (
      <Activity mode={mode}>
        <GrokSignIn signedIn={null} onChange={onChange} />
      </Activity>
    );
    const { rerender } = render(page("visible"));
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    rerender(page("hidden"));
    rerender(page("visible"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1500);
    });
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(SETTINGS));
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(2);
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

  it("a poll with no answer keeps the code up, and the next answer signs in", async () => {
    api.pollGrokSignIn
      .mockResolvedValueOnce({ status: "noAnswer", error: "xAI didn't answer." })
      .mockResolvedValueOnce({ status: "ready", signedIn: { name: "Sam", email: null } });
    api.getSettings.mockResolvedValue(SETTINGS);
    const onChange = vi.fn();
    render(<GrokSignIn signedIn={null} onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Sign-in code")).toHaveTextContent("ABCD-EFGH");
    expect(screen.getByRole("status")).toHaveTextContent(
      "Still waiting for you to approve it on x.ai. The last check failed: xAI didn't answer.",
    );
    expect(screen.queryByRole("button", { name: "Sign in with Grok" })).not.toBeInTheDocument();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(SETTINGS));
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("status")).toHaveTextContent("Signed in as Sam.");
  });

  it("says why once while polls keep failing, then plain waiting on an answer", async () => {
    api.pollGrokSignIn
      .mockResolvedValueOnce({ status: "noAnswer", error: "xAI didn't answer." })
      .mockResolvedValueOnce({ status: "noAnswer", error: "xAI didn't answer." })
      .mockResolvedValue({ status: "pending", interval: 1 });
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2200);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(2);
    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Still waiting for you to approve it on x.ai. The last check failed: xAI didn't answer.",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(3);
    expect(screen.getByLabelText("Sign-in code")).toHaveTextContent("ABCD-EFGH");
    expect(screen.getByRole("status")).toHaveTextContent("Waiting for you to approve it on x.ai…");
  });

  it("a refusal after a failed poll still stops and says so", async () => {
    api.pollGrokSignIn
      .mockResolvedValueOnce({ status: "noAnswer", error: "xAI didn't answer." })
      .mockResolvedValue({ status: "failed", error: "The sign-in was refused." });
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(screen.getByRole("status")).toHaveTextContent("The sign-in was refused.");
    expect(screen.queryByLabelText("Sign-in code")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in with Grok" })).toBeInTheDocument();
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(2);
  });

  it("a code xAI says ran out after a failed poll still stops", async () => {
    api.pollGrokSignIn
      .mockResolvedValueOnce({ status: "noAnswer", error: "xAI didn't answer." })
      .mockResolvedValue({ status: "failed", error: "The code ran out. Start again." });
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(screen.getByRole("status")).toHaveTextContent("The code ran out. Start again.");
    expect(screen.queryByLabelText("Sign-in code")).not.toBeInTheDocument();
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(2);
  });

  it("stops when the code's own time is up, however the polls fail", async () => {
    api.startGrokSignIn.mockResolvedValue({ ...CODE, interval: 2, expiresIn: 6 });
    api.pollGrokSignIn.mockResolvedValue({ status: "noAnswer", error: "xAI didn't answer." });
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(4500);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(2);
    expect(screen.getByLabelText("Sign-in code")).toHaveTextContent("ABCD-EFGH");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20000);
    });
    expect(screen.getByRole("status")).toHaveTextContent("The code ran out. Start again.");
    expect(screen.queryByLabelText("Sign-in code")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in with Grok" })).toBeInTheDocument();
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(2);
  });

  it("stops when the shell says it holds no code", async () => {
    api.pollGrokSignIn.mockRejectedValue("Start Sign in with Grok first.");
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(screen.getByRole("status")).toHaveTextContent("Start Sign in with Grok first.");
    expect(screen.queryByLabelText("Sign-in code")).not.toBeInTheDocument();
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
  });

  it("a check that fails another way stops and says why, as a keychain that can't save does", async () => {
    // xAI said yes, then the shell could not keep the sign-in: asking again
    // can't help, and the person has already approved.
    api.pollGrokSignIn.mockRejectedValue("The system keychain is not available: no secret service.");
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(screen.getByRole("status").textContent).toBe(
      "The system keychain is not available: no secret service.",
    );
    expect(screen.queryByLabelText("Sign-in code")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in with Grok" })).toBeInTheDocument();
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
  });

  it("asks nothing more once the page is closed after a failed poll", async () => {
    api.pollGrokSignIn.mockResolvedValue({ status: "noAnswer", error: "xAI didn't answer." });
    const { unmount } = render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Sign-in code")).toHaveTextContent("ABCD-EFGH");
    unmount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
  });

  it("a poll that fails after the page is closed asks nothing more", async () => {
    let lose: (why: string) => void = () => {};
    api.pollGrokSignIn.mockReturnValue(
      new Promise<GrokSignInPoll>((resolve) => {
        lose = (error) => resolve({ status: "noAnswer", error });
      }),
    );
    const { unmount } = render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    unmount();
    await act(async () => {
      lose("xAI didn't answer.");
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
  });

  it("a new code starts without the last code's failed check", async () => {
    api.pollGrokSignIn.mockResolvedValue({ status: "noAnswer", error: "xAI didn't answer." });
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "Still waiting for you to approve it on x.ai. The last check failed: xAI didn't answer.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    expect(screen.getByRole("status").textContent).toBe("Waiting for you to approve it on x.ai…");
  });

  it("Cancel after a failed check takes the code down and asks nothing more", async () => {
    api.pollGrokSignIn.mockResolvedValue({ status: "noAnswer", error: "xAI didn't answer." });
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    expect(screen.queryByLabelText("Sign-in code")).not.toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in with Grok" })).toBeInTheDocument();
  });

  it("a check that fails after Cancel asks nothing more and says nothing", async () => {
    let lose: (why: string) => void = () => {};
    api.pollGrokSignIn.mockReturnValue(
      new Promise<GrokSignInPoll>((resolve) => {
        lose = (error) => resolve({ status: "noAnswer", error });
      }),
    );
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await act(async () => {
      lose("xAI didn't answer.");
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in with Grok" })).toBeInTheDocument();
  });

  it("hidden and shown between failed checks, it still asks once per interval", async () => {
    api.startGrokSignIn.mockResolvedValue({ ...CODE, interval: 2 });
    api.pollGrokSignIn.mockResolvedValue({ status: "noAnswer", error: "xAI didn't answer." });
    const page = (mode: "visible" | "hidden") => (
      <Activity mode={mode}>
        <GrokSignIn signedIn={null} onChange={() => {}} />
      </Activity>
    );
    const { rerender } = render(page("visible"));
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2100);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    rerender(page("hidden"));
    rerender(page("visible"));
    expect(screen.getByRole("status")).toHaveTextContent(
      "Still waiting for you to approve it on x.ai. The last check failed: xAI didn't answer.",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1500);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(2);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(3);
  });

  it("a check of the last code that fails under a new code is not asked again at once", async () => {
    let lose: (why: string) => void = () => {};
    api.startGrokSignIn
      .mockResolvedValueOnce({ ...CODE, interval: 5 })
      .mockResolvedValueOnce({ ...CODE, userCode: "WXYZ-1234", interval: 5 });
    api.pollGrokSignIn
      .mockReturnValueOnce(
        new Promise<GrokSignInPoll>((resolve) => {
          lose = (error) => resolve({ status: "noAnswer", error });
        }),
      )
      .mockResolvedValue({ status: "noAnswer", error: "xAI is busy." });
    render(<GrokSignIn signedIn={null} onChange={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5100);
    });
    // The first code's check is out, with no answer yet.
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    expect(await screen.findByLabelText("Sign-in code")).toHaveTextContent("WXYZ-1234");
    await act(async () => {
      lose("xAI didn't answer.");
      await vi.advanceTimersByTimeAsync(1000);
    });
    // The old check says nothing about the new code, and nothing is asked early.
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status").textContent).toBe("Waiting for you to approve it on x.ai…");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(4100);
    });
    // The new code's own first check, one interval later.
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(2);
    expect(screen.getByLabelText("Sign-in code")).toHaveTextContent("WXYZ-1234");
    expect(screen.getByRole("status")).toHaveTextContent(
      "Still waiting for you to approve it on x.ai. The last check failed: xAI is busy.",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(api.pollGrokSignIn).toHaveBeenCalledTimes(3);
  });

  it("a code that ran out while Settings was hidden is gone as soon as it is shown", async () => {
    api.startGrokSignIn.mockResolvedValue({ ...CODE, interval: 5, expiresIn: 8 });
    api.pollGrokSignIn.mockResolvedValue({ status: "pending", interval: 5 });
    const page = (mode: "visible" | "hidden") => (
      <Activity mode={mode}>
        <GrokSignIn signedIn={null} onChange={() => {}} />
      </Activity>
    );
    const { rerender } = render(page("visible"));
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Grok" }));
    await screen.findByLabelText("Sign-in code");
    rerender(page("hidden"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60000);
    });
    rerender(page("visible"));
    expect(screen.queryByLabelText("Sign-in code")).not.toBeInTheDocument();
    expect(screen.getByRole("status").textContent).toBe("The code ran out. Start again.");
    expect(screen.getByRole("button", { name: "Sign in with Grok" })).toBeInTheDocument();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(api.pollGrokSignIn).not.toHaveBeenCalled();
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

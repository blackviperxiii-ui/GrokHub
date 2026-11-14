import "@testing-library/jest-dom/vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PersonalityPage, PrivacyPage } from "./api";

const api = vi.hoisted(() => ({
  getPersonalities: vi.fn<() => Promise<PersonalityPage>>(),
  getPrivacy: vi.fn<() => Promise<PrivacyPage>>(),
  quietChip: vi.fn<() => Promise<string | null>>(),
}));
vi.mock("./api", () => api);

import { localText, personalityName, StatusPills } from "./StatusPills";

const PRIVACY: PrivacyPage = {
  offlineOnly: false,
  localReady: false,
  sent: "3 calls sent off-device this week",
  sentCalls: 3,
  hosts: [],
  encrypted: true,
  encryptedNote: "",
  dataFolder: "/home/user/.config/GrokHub",
  filesFolder: "/home/user/Documents/Loadout",
  canOpenFolder: true,
};

const PERSONALITY = {
  presets: [
    { id: "default", name: "Default" },
    { id: "viper", name: "Viper" },
  ],
  current: "viper",
} as unknown as PersonalityPage;

describe("pill text", () => {
  it("names the default voice, or Home's fallback when no preset matches", () => {
    expect(personalityName({ current: "viper", presets: [{ id: "viper", name: "Viper" }] })).toBe("Viper");
    expect(personalityName({ current: "gone", presets: [{ id: "viper", name: "Viper" }] })).toBe("Default voice");
  });

  it("counts calls, never bytes, and says less when the count is missing", () => {
    expect(localText({ offlineOnly: false, sentCalls: 0 })).toBe("Local · 0 sent");
    expect(localText({ offlineOnly: false, sentCalls: 3 })).toBe("Local first · 3 sent");
    expect(localText({ offlineOnly: false, sentCalls: null })).toBe("Local first");
    expect(localText({ offlineOnly: false })).toBe("Local first");
    expect(localText(null)).toBe("Local first");
    expect(localText({ offlineOnly: true, sentCalls: 2 })).toBe("Offline only · 2 sent");
    expect(localText({ offlineOnly: true, sentCalls: null })).toBe("Offline only");
  });
});

describe("StatusPills", () => {
  beforeEach(() => {
    api.getPersonalities.mockReset().mockResolvedValue(PERSONALITY);
    api.getPrivacy.mockReset().mockResolvedValue(PRIVACY);
    api.quietChip.mockReset().mockResolvedValue(null);
  });

  it("shows Quiet until while quiet hours hold, and it opens Behavior", async () => {
    api.quietChip.mockResolvedValue("Quiet until 07:00");
    const onSettings = vi.fn();
    render(<StatusPills remote={false} refresh="a" onSettings={onSettings} />);
    const quiet = await screen.findByRole("button", { name: "Quiet until 07:00" });
    expect(quiet).toHaveAttribute("title", "Automations wait until then. Opens Behavior.");
    await userEvent.click(quiet);
    expect(onSettings).toHaveBeenLastCalledWith("behavior");
    // The pill sits first in the row, ahead of the personality.
    expect(screen.getAllByRole("button")[0]).toBe(quiet);
  });

  it("hides Quiet until outside quiet hours, when the desktop doesn't answer, and on the web app", async () => {
    const view = render(<StatusPills remote={false} refresh="a" onSettings={() => {}} />);
    await screen.findByRole("button", { name: "Personality · Viper" });
    expect(screen.queryByRole("button", { name: /^Quiet until/ })).toBeNull();
    view.unmount();
    api.quietChip.mockRejectedValue(new Error("refused"));
    const failed = render(<StatusPills remote={false} refresh="a" onSettings={() => {}} />);
    await screen.findByRole("button", { name: "Personality · Viper" });
    expect(screen.queryByRole("button", { name: /^Quiet until/ })).toBeNull();
    failed.unmount();
    api.quietChip.mockClear().mockResolvedValue("Quiet until 07:00");
    render(<StatusPills remote refresh="a" onSettings={() => {}} />);
    await screen.findByRole("button", { name: "Local first · 3 sent" });
    expect(api.quietChip).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /^Quiet until/ })).toBeNull();
  });

  it("reads quiet hours again at the turn of each minute, so the pill comes and goes on time", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      vi.setSystemTime(new Date(2026, 10, 14, 21, 59, 30));
      render(<StatusPills remote={false} refresh="a" onSettings={() => {}} />);
      await screen.findByRole("button", { name: "Personality · Viper" });
      expect(api.quietChip).toHaveBeenCalledTimes(1);
      api.quietChip.mockResolvedValue("Quiet until 07:00");
      await act(async () => {
        await vi.advanceTimersByTimeAsync(30_100);
      });
      expect(api.quietChip).toHaveBeenCalledTimes(2);
      expect(await screen.findByRole("button", { name: "Quiet until 07:00" })).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows the voice and the week's count, and each opens its Settings section", async () => {
    const onSettings = vi.fn();
    render(<StatusPills remote={false} refresh="a" onSettings={onSettings} />);
    await userEvent.click(await screen.findByRole("button", { name: "Personality · Viper" }));
    expect(onSettings).toHaveBeenLastCalledWith("personality");
    const local = await screen.findByRole("button", { name: "Local first · 3 sent" });
    expect(local).toHaveAttribute("title", "3 calls sent off-device this week");
    await userEvent.click(local);
    expect(onSettings).toHaveBeenLastCalledWith("privacy");
  });

  it("falls back to the bare keys when the desktop doesn't answer", async () => {
    api.getPersonalities.mockRejectedValue(new Error("refused"));
    api.getPrivacy.mockRejectedValue(new Error("refused"));
    render(<StatusPills remote={false} refresh="a" onSettings={() => {}} />);
    expect(await screen.findByRole("button", { name: "Personality" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Local first" })).toHaveAttribute("title", "Calls sent off-device this week");
  });

  it("leaves the personality out of the web app, which may not read it", async () => {
    render(<StatusPills remote refresh="a" onSettings={() => {}} />);
    expect(await screen.findByRole("button", { name: "Local first · 3 sent" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Personality/ })).toBeNull();
    expect(api.getPersonalities).not.toHaveBeenCalled();
  });

  it("reads again when refresh changes", async () => {
    const { rerender } = render(<StatusPills remote={false} refresh="a" onSettings={() => {}} />);
    await screen.findByRole("button", { name: "Local first · 3 sent" });
    api.getPrivacy.mockResolvedValue({ ...PRIVACY, sentCalls: 0 });
    rerender(<StatusPills remote={false} refresh="b" onSettings={() => {}} />);
    expect(await screen.findByRole("button", { name: "Local · 0 sent" })).toBeInTheDocument();
    expect(api.getPrivacy).toHaveBeenCalledTimes(2);
  });
});

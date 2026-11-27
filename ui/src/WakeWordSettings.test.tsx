import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WakeWordPage } from "./api";

const basePage = (): WakeWordPage => ({
  micConsent: false,
  enabled: false,
  phrases: [],
  stopPhrases: [],
  routes: [],
  defaultProfile: "",
  phase: "off",
  voiceBusy: false,
  defaultPhrases: ["hey loadout", "hey grok"],
  defaultStop: ["stop listening", "that's enough", "cancel"],
  privacy:
    "Wake listening stays on this computer. Mic access is opt-in; audio for wake matching is never sent to the cloud.",
});

const api = vi.hoisted(() => ({
  getWakeWord: vi.fn<() => Promise<WakeWordPage>>(),
  wakeWordAct: vi.fn<(act: unknown) => Promise<WakeWordPage>>(),
}));

vi.mock("./api", () => api);

import { WakeWordSettings } from "./WakeWordSettings";

describe("WakeWordSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getWakeWord.mockResolvedValue(basePage());
    api.wakeWordAct.mockImplementation(async (act) => {
      const page = basePage();
      if (act && typeof act === "object" && "kind" in act) {
        const a = act as { kind: string; on?: boolean };
        if (a.kind === "setMicConsent" && a.on) {
          page.micConsent = true;
        }
        if (a.kind === "setEnabled" && a.on) {
          page.micConsent = true;
          page.enabled = true;
        }
      }
      return page;
    });
  });

  it("shows privacy copy and opt-in mic before listen", async () => {
    render(<WakeWordSettings />);
    expect(
      await screen.findByText(/Wake listening stays on this computer/),
    ).toBeInTheDocument();
    const mic = screen.getByRole("button", { name: "Allow mic for wake word" });
    expect(mic).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "Listen for wake phrases" })).toBeDisabled();
    expect(screen.queryByRole("combobox")).toBeNull();
  });

  it("allows mic then enables listen without a model picker", async () => {
    render(<WakeWordSettings />);
    await screen.findByRole("button", { name: "Allow mic for wake word" });
    await userEvent.click(screen.getByRole("button", { name: "Allow mic for wake word" }));
    expect(api.wakeWordAct).toHaveBeenCalledWith({ kind: "setMicConsent", on: true });
  });
});

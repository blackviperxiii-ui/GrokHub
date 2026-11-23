import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { VoiceRouteConfig } from "./api";

const api = vi.hoisted(() => ({
  getVoiceRoute: vi.fn<() => Promise<VoiceRouteConfig>>(),
  setVoiceRoute: vi.fn<(config: VoiceRouteConfig) => Promise<VoiceRouteConfig>>(),
}));

vi.mock("./api", () => api);

import { VoiceRouteSettings } from "./VoiceRouteSettings";

describe("VoiceRouteSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getVoiceRoute.mockResolvedValue({ mode: "auto" });
    api.setVoiceRoute.mockImplementation(async (cfg) => cfg);
  });

  it("loads the current mode and switches between Auto and Local only", async () => {
    render(<VoiceRouteSettings />);
    const group = await screen.findByRole("group", { name: "Voice routing mode" });
    expect(within(group).getByRole("button", { name: "Auto" })).toHaveAttribute("aria-pressed", "true");

    await userEvent.click(within(group).getByRole("button", { name: "Local only" }));
    expect(api.setVoiceRoute).toHaveBeenCalledWith({ mode: "local_only" });
    expect(within(group).getByRole("button", { name: "Local only" })).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByText("Local only keeps speech to text, replies, and spoken voice on this computer."),
    ).toBeInTheDocument();
  });

  it("offers no per-stage source or model picker", async () => {
    render(<VoiceRouteSettings />);
    const group = await screen.findByRole("group", { name: "Voice routing mode" });
    expect(within(group).getAllByRole("button").map((b) => b.textContent)).toEqual(["Auto", "Local only"]);
    expect(screen.queryByRole("combobox")).toBeNull();
    expect(screen.queryByText(/pinned/i)).toBeNull();
    expect(screen.queryByText(/reply model/i)).toBeNull();
  });

  it("shows save failures cleanly", async () => {
    api.setVoiceRoute.mockRejectedValueOnce(new Error("Could not save voice-route.json"));
    render(<VoiceRouteSettings />);
    const group = await screen.findByRole("group", { name: "Voice routing mode" });
    await userEvent.click(within(group).getByRole("button", { name: "Local only" }));
    expect(await screen.findByText("Could not save voice-route.json")).toHaveClass("note", "error");
  });
});

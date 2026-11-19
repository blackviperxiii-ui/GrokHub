import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AiModelsView, VoiceRouteConfig } from "./api";

const api = vi.hoisted(() => ({
  getVoiceRoute: vi.fn<() => Promise<VoiceRouteConfig>>(),
  setVoiceRoute: vi.fn<(config: VoiceRouteConfig) => Promise<VoiceRouteConfig>>(),
  getAiModels: vi.fn<() => Promise<AiModelsView>>(),
}));

vi.mock("./api", () => api);

import { VoiceRouteSettings } from "./VoiceRouteSettings";

function baseConfig(over: Partial<VoiceRouteConfig> = {}): VoiceRouteConfig {
  return {
    mode: "auto",
    pinned: { stt: null, reply: null, tts: null },
    ...over,
  };
}

function modelsView(providers: AiModelsView["providers"]): AiModelsView {
  return {
    providers,
    spend: { weeklyCapUsd: 0, priceCeilingUsdPerM: 0, caps: [], ceilings: [] },
    premium: [],
    benched: [],
    table: [],
    tableBuiltAt: 0,
    locked: "",
    webSearch: {
      backend: "duckduckgo",
      backends: [],
      searxngUrl: "",
      searxngProblem: "",
      braveKeySet: false,
    },
    status: "",
  };
}

describe("VoiceRouteSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getVoiceRoute.mockResolvedValue(baseConfig());
    api.setVoiceRoute.mockImplementation(async (cfg) => cfg);
    api.getAiModels.mockResolvedValue(
      modelsView([
        {
          id: "openrouter",
          kind: "openai_compatible",
          baseUrl: "https://openrouter.ai/api/v1",
          dest: "openrouter.ai",
          hasKey: true,
          state: "Allowed",
          ready: true,
        },
        {
          id: "anthropic",
          kind: "anthropic",
          baseUrl: "https://api.anthropic.com",
          dest: "api.anthropic.com",
          hasKey: false,
          state: "No key",
          ready: false,
        },
      ]),
    );
  });

  it("loads the current mode and switches between Auto, Local only and Pinned", async () => {
    render(<VoiceRouteSettings />);
    const group = await screen.findByRole("group", { name: "Voice routing mode" });
    expect(within(group).getByRole("button", { name: "Auto" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("group", { name: "Pinned voice stages" })).toBeNull();

    await userEvent.click(within(group).getByRole("button", { name: "Local only" }));
    expect(api.setVoiceRoute).toHaveBeenCalledWith(baseConfig({ mode: "local_only" }));
    expect(within(group).getByRole("button", { name: "Local only" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    await userEvent.click(within(group).getByRole("button", { name: "Pinned" }));
    expect(api.setVoiceRoute).toHaveBeenLastCalledWith(baseConfig({ mode: "pinned" }));
    expect(await screen.findByRole("group", { name: "Pinned voice stages" })).toBeInTheDocument();
  });

  it("lets each voice stage be pinned independently, including ready providers", async () => {
    api.getVoiceRoute.mockResolvedValue(baseConfig({ mode: "pinned" }));
    render(<VoiceRouteSettings />);

    const stt = await screen.findByLabelText("Speech to text");
    const reply = screen.getByLabelText("Reply model");
    const tts = screen.getByLabelText("Voice / TTS");

    expect(within(reply).getByRole("option", { name: "openrouter" })).toBeInTheDocument();
    expect(within(reply).queryByRole("option", { name: "anthropic" })).toBeNull();

    await userEvent.selectOptions(stt, "grok_voice_3");
    expect(api.setVoiceRoute).toHaveBeenLastCalledWith({
      mode: "pinned",
      pinned: { stt: "grok_voice_3", reply: null, tts: null },
    });

    await userEvent.selectOptions(reply, "openrouter");
    expect(api.setVoiceRoute).toHaveBeenLastCalledWith({
      mode: "pinned",
      pinned: { stt: "grok_voice_3", reply: "openrouter", tts: null },
    });

    await userEvent.selectOptions(tts, "local");
    expect(api.setVoiceRoute).toHaveBeenLastCalledWith({
      mode: "pinned",
      pinned: { stt: "grok_voice_3", reply: "openrouter", tts: "local" },
    });
  });

  it("shows save failures cleanly", async () => {
    api.setVoiceRoute.mockRejectedValueOnce(new Error("Could not save voice-route.json"));
    render(<VoiceRouteSettings />);
    const group = await screen.findByRole("group", { name: "Voice routing mode" });
    await userEvent.click(within(group).getByRole("button", { name: "Local only" }));
    expect(await screen.findByText("Could not save voice-route.json")).toHaveClass("note", "error");
  });
});

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AiModelsCard, AiModelsView } from "./api";

const api = vi.hoisted(() => ({
  getAiModels: vi.fn<() => Promise<AiModelsView>>(),
  addAiProvider: vi.fn<(baseUrl: string, key: string) => Promise<AiModelsCard>>(),
  allowAiProvider: vi.fn<(id: string) => Promise<AiModelsCard>>(),
  answerAiModelsCard: vi.fn<(id: string, approve: boolean) => Promise<AiModelsView>>(),
  removeAiProvider: vi.fn<(id: string) => Promise<AiModelsView>>(),
  saveAiSpend: vi.fn<(cap: number, ceiling: number) => Promise<AiModelsView>>(),
  revokeAiPremium: vi.fn<(id: string) => Promise<AiModelsView>>(),
}));
vi.mock("./api", () => api);

import { AiModels, PREMIUM_NONE } from "./AiModels";

const EMPTY: AiModelsView = {
  providers: [],
  spend: { weeklyCapUsd: 0, priceCeilingUsdPerM: 15, caps: [0, 5, 10, 25, 50, 100], ceilings: [5, 15, 30, 60] },
  premium: [],
  table: [],
  tableBuiltAt: 0,
  locked: "",
  status: "",
};

const WITH_PROVIDER: AiModelsView = {
  ...EMPTY,
  providers: [
    {
      id: "openrouter",
      kind: "OpenAI-compatible",
      baseUrl: "https://openrouter.ai/api/v1",
      dest: "openrouter.ai",
      hasKey: true,
      state: "Not allowed yet",
      ready: false,
    },
  ],
};

const KEY_CARD: AiModelsCard = {
  id: "ai-key-openrouter-1",
  kind: "key",
  provider: "openrouter",
  action: "Save its API key (openrouter)",
};

beforeEach(() => {
  vi.resetAllMocks();
  api.getAiModels.mockResolvedValue(EMPTY);
});

describe("AiModels", () => {
  it("renders the sections with nothing set up", async () => {
    render(<AiModels />);
    expect(await screen.findByText("No other providers yet.")).toBeVisible();
    expect(screen.getByRole("heading", { name: "Other AI providers" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Spend limits" })).toBeVisible();
    expect(screen.getByText(PREMIUM_NONE)).toBeVisible();
    expect(screen.getByText("No routing table yet. It builds after the first few chats.")).toBeVisible();
    expect(screen.getByLabelText("Weekly spend cap")).toHaveValue("0");
    expect(screen.getByLabelText("Price limit")).toHaveValue("15");
  });

  it("keeps the key field write-only and empties it as the key leaves", async () => {
    api.addAiProvider.mockResolvedValue(KEY_CARD);
    render(<AiModels />);
    await screen.findByText("No other providers yet.");
    const keyField = screen.getByLabelText("Provider API key");
    expect(keyField).toHaveAttribute("type", "password");
    expect(keyField).toHaveAttribute("autocomplete", "off");
    await userEvent.type(screen.getByLabelText("Provider address"), "https://openrouter.ai/api/v1");
    await userEvent.type(keyField, "test-key-aaaaaaaa");
    await userEvent.click(screen.getByRole("button", { name: "Add provider" }));
    expect(api.addAiProvider).toHaveBeenCalledWith("https://openrouter.ai/api/v1", "test-key-aaaaaaaa");
    expect(keyField).toHaveValue("");
    // The card names the action but never the key.
    const group = await screen.findByRole("group", { name: "Save its API key (openrouter)" });
    expect(group).not.toHaveTextContent("test-key-aaaaaaaa");
    expect(api.answerAiModelsCard).not.toHaveBeenCalled();
  });

  it("saves nothing on Deny", async () => {
    api.addAiProvider.mockResolvedValue(KEY_CARD);
    api.answerAiModelsCard.mockResolvedValue({ ...EMPTY, status: "Nothing saved. The key was denied." });
    render(<AiModels />);
    await screen.findByText("No other providers yet.");
    await userEvent.type(screen.getByLabelText("Provider address"), "https://openrouter.ai/api/v1");
    await userEvent.type(screen.getByLabelText("Provider API key"), "test-key-aaaaaaaa");
    await userEvent.click(screen.getByRole("button", { name: "Add provider" }));
    await userEvent.click(await screen.findByRole("button", { name: "Deny" }));
    expect(api.answerAiModelsCard).toHaveBeenCalledWith("ai-key-openrouter-1", false);
    expect(await screen.findByText("Nothing saved. The key was denied.")).toBeVisible();
    expect(screen.getByText("No other providers yet.")).toBeVisible();
  });

  it("shows a saved provider as off until its destination is allowed, then allows it", async () => {
    api.getAiModels.mockResolvedValue(WITH_PROVIDER);
    api.allowAiProvider.mockResolvedValue({
      id: "ai-allow-openrouter-1",
      kind: "allow",
      provider: "openrouter",
      action: "Send chats and memory to openrouter.ai",
    });
    api.answerAiModelsCard.mockResolvedValue({
      ...WITH_PROVIDER,
      providers: [{ ...WITH_PROVIDER.providers[0], state: "Allowed", ready: true }],
      status: "Allowed. Revoke it in Settings → Permissions any time.",
    });
    render(<AiModels />);
    const row = await screen.findByRole("listitem", { name: "openrouter" });
    expect(within(row).getByText("Not allowed yet")).toBeVisible();
    await userEvent.click(within(row).getByRole("button", { name: "Allow openrouter.ai" }));
    await userEvent.click(await screen.findByRole("button", { name: "Allow" }));
    expect(api.answerAiModelsCard).toHaveBeenCalledWith("ai-allow-openrouter-1", true);
    expect(await screen.findByText("Allowed")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Allow openrouter.ai" })).toBeNull();
  });

  it("saves a new weekly cap keeping the price limit", async () => {
    api.saveAiSpend.mockResolvedValue({ ...EMPTY, spend: { ...EMPTY.spend, weeklyCapUsd: 25 }, status: "Saved." });
    render(<AiModels />);
    await screen.findByText("No other providers yet.");
    await userEvent.selectOptions(screen.getByLabelText("Weekly spend cap"), "25");
    expect(api.saveAiSpend).toHaveBeenCalledWith(25, 15);
    expect(await screen.findByText("Saved.")).toBeVisible();
    expect(screen.getByLabelText("Weekly spend cap")).toHaveValue("25");
  });

  it("revokes a paid upgrade and shows the empty line", async () => {
    api.getAiModels.mockResolvedValue({
      ...EMPTY,
      premium: [{ id: "g-1", label: "Use a premium model at priority speed" }],
    });
    api.revokeAiPremium.mockResolvedValue({ ...EMPTY, status: "Revoked. Auto stops using it on the next turn." });
    render(<AiModels />);
    const row = await screen.findByRole("listitem", { name: "Use a premium model at priority speed" });
    await userEvent.click(within(row).getByRole("button", { name: "Revoke" }));
    expect(api.revokeAiPremium).toHaveBeenCalledWith("g-1");
    expect(await screen.findByText(PREMIUM_NONE)).toBeVisible();
  });

  it("lists the routing table with quality as a percent", async () => {
    api.getAiModels.mockResolvedValue({
      ...EMPTY,
      tableBuiltAt: 1,
      table: [
        {
          class: "chat",
          rows: [
            { model: "model-a", effort: "low", quality: 0.8, estCostUsd: null, p50LatencyMs: null, why: "fast" },
            { model: "model-b", effort: null, quality: null, estCostUsd: null, p50LatencyMs: null, why: "" },
          ],
        },
      ],
    });
    render(<AiModels />);
    expect(await screen.findByText("model-a (low) · quality 80% · fast")).toBeVisible();
    expect(screen.getByText("model-b · quality unscored")).toBeVisible();
  });

  it("removes a provider only after a confirm", async () => {
    api.getAiModels.mockResolvedValue(WITH_PROVIDER);
    api.removeAiProvider.mockResolvedValue({ ...EMPTY, status: "Provider removed. Its key is gone from the keyring." });
    render(<AiModels />);
    const row = await screen.findByRole("listitem", { name: "openrouter" });
    await userEvent.click(within(row).getByRole("button", { name: "Remove" }));
    expect(api.removeAiProvider).not.toHaveBeenCalled();
    const confirm = await screen.findByRole("group", { name: "Remove openrouter?" });
    await userEvent.click(within(confirm).getByRole("button", { name: "Remove" }));
    expect(api.removeAiProvider).toHaveBeenCalledWith("openrouter");
  });
});

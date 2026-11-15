import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AiModelsCard, AiModelsView, OpenRouterSignIn, OpenRouterSignInStatus } from "./api";

const api = vi.hoisted(() => ({
  getAiModels: vi.fn<() => Promise<AiModelsView>>(),
  addAiProvider: vi.fn<(baseUrl: string, key: string) => Promise<AiModelsCard>>(),
  allowAiProvider: vi.fn<(id: string) => Promise<AiModelsCard>>(),
  answerAiModelsCard: vi.fn<(id: string, approve: boolean) => Promise<AiModelsView>>(),
  removeAiProvider: vi.fn<(id: string) => Promise<AiModelsView>>(),
  saveAiSpend: vi.fn<(cap: number, ceiling: number) => Promise<AiModelsView>>(),
  revokeAiPremium: vi.fn<(id: string) => Promise<AiModelsView>>(),
  saveWebSearch: vi.fn<(backend: string, searxngUrl: string) => Promise<AiModelsView>>(),
  saveBraveKey: vi.fn<(key: string) => Promise<AiModelsView>>(),
  startOpenRouterSignIn: vi.fn<() => Promise<OpenRouterSignIn>>(),
  waitOpenRouterSignIn: vi.fn<() => Promise<AiModelsCard>>(),
  stopOpenRouterSignIn: vi.fn<() => Promise<void>>(),
  openRouterSignInStatus: vi.fn<() => Promise<OpenRouterSignInStatus>>(),
}));
vi.mock("./api", () => api);

import {
  AiModels,
  BRAVE_SET,
  BRAVE_UNSET,
  LOCAL_NOTE,
  OPENROUTER_BY_HAND,
  OPENROUTER_KEY_WAIT,
  OPENROUTER_REOPEN,
  OPENROUTER_STOPPED,
  OPENROUTER_WAIT,
  PREMIUM_NONE,
  SEARXNG_NOTE,
  WEB_SEARCH_NOTE,
} from "./AiModels";

const SIGN_IN_URL = "https://openrouter.ai/auth?callback_url=http%3A%2F%2F127.0.0.1%3A4000%2Fcallback&state=s";

/** A promise the test settles by hand. */
function later<T>() {
  let settle: (value: T) => void = () => undefined;
  let refuse: (err: unknown) => void = () => undefined;
  const promise = new Promise<T>((resolve, reject) => {
    settle = resolve;
    refuse = reject;
  });
  return { promise, settle, refuse };
}

const EMPTY: AiModelsView = {
  providers: [],
  spend: { weeklyCapUsd: 0, priceCeilingUsdPerM: 15, caps: [0, 5, 10, 25, 50, 100], ceilings: [5, 15, 30, 60] },
  premium: [],
  table: [],
  tableBuiltAt: 0,
  locked: "",
  webSearch: {
    backend: "duckduckgo",
    backends: [
      { id: "duckduckgo", label: "DuckDuckGo Lite (free, no key)" },
      { id: "searxng", label: "SearXNG (self-hosted URL)" },
      { id: "brave", label: "Brave Search (your API key)" },
    ],
    searxngUrl: "",
    searxngProblem: "",
    braveKeySet: false,
  },
  status: "",
};

function searching(over: Partial<AiModelsView["webSearch"]>): AiModelsView {
  return { ...EMPTY, webSearch: { ...EMPTY.webSearch, ...over } };
}

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
  api.stopOpenRouterSignIn.mockResolvedValue(undefined);
  api.openRouterSignInStatus.mockResolvedValue({ waiting: null, card: null });
});

describe("AiModels", () => {
  it("points local models and Dynamic to the Local AI page instead of repeating them", async () => {
    render(<AiModels />);
    expect(await screen.findByText(LOCAL_NOTE)).toBeVisible();
    expect(LOCAL_NOTE).toBe("Models that run on this PC, and whether each task gets its own model, are set in Local AI.");
    expect(screen.queryByText("Dynamic")).toBeNull();
  });

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

  it("shows the page title once: the Settings page already has the heading", async () => {
    const { container } = render(<AiModels />);
    // While it loads, and after.
    expect(screen.queryByRole("heading", { name: "AI models" })).toBeNull();
    await screen.findByText("No other providers yet.");
    expect(screen.queryByRole("heading", { name: "AI models" })).toBeNull();
    expect(container.querySelector("section")).toHaveAttribute("aria-label", "AI models");
  });

  it("signs in with OpenRouter: waits for the browser, then shows the key card", async () => {
    const card = later<AiModelsCard>();
    api.startOpenRouterSignIn.mockResolvedValue({ url: SIGN_IN_URL, opened: true, why: "" });
    api.waitOpenRouterSignIn.mockReturnValue(card.promise);
    api.answerAiModelsCard.mockResolvedValue({ ...WITH_PROVIDER, status: "openrouter added, still off. Allow it so Auto can use its models." });
    render(<AiModels />);
    await userEvent.click(await screen.findByRole("button", { name: "Sign in with OpenRouter" }));
    expect(await screen.findByText(OPENROUTER_WAIT)).toBeVisible();
    expect(OPENROUTER_WAIT).toBe("Waiting for OpenRouter. Finish signing in in your browser.");
    // The browser opened; the link stays at hand for a tab that was closed.
    const byHand = screen.getByRole("group", { name: "Open OpenRouter by hand" });
    expect(byHand).toHaveTextContent(OPENROUTER_REOPEN);
    expect(within(byHand).getByLabelText("OpenRouter sign-in link")).toHaveValue(SIGN_IN_URL);
    // Nothing else starts meanwhile.
    expect(screen.getByRole("button", { name: "Sign in with OpenRouter" })).toBeDisabled();

    card.settle(KEY_CARD);
    const group = await screen.findByRole("group", { name: "Save its API key (openrouter)" });
    expect(screen.getByText(OPENROUTER_KEY_WAIT)).toBeVisible();
    expect(api.answerAiModelsCard).not.toHaveBeenCalled();
    await userEvent.click(within(group).getByRole("button", { name: "Save" }));
    expect(api.answerAiModelsCard).toHaveBeenCalledWith("ai-key-openrouter-1", true);
    expect(await screen.findByText("openrouter added, still off. Allow it so Auto can use its models.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Sign in to OpenRouter again" })).toBeEnabled();
  });

  it("shows the sign-in link to open by hand when the browser didn't open", async () => {
    const card = later<AiModelsCard>();
    api.startOpenRouterSignIn.mockResolvedValue({ url: SIGN_IN_URL, opened: false, why: "No browser opened" });
    api.waitOpenRouterSignIn.mockReturnValue(card.promise);
    const writeText = vi.fn<(text: string) => Promise<void>>().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<AiModels />);
    await userEvent.click(await screen.findByRole("button", { name: "Sign in with OpenRouter" }));
    const byHand = await screen.findByRole("group", { name: "Open OpenRouter by hand" });
    expect(byHand).toHaveTextContent(`${OPENROUTER_BY_HAND} (No browser opened)`);
    expect(within(byHand).getByLabelText("OpenRouter sign-in link")).toHaveValue(SIGN_IN_URL);
    await userEvent.click(within(byHand).getByRole("button", { name: "Copy link" }));
    expect(writeText).toHaveBeenCalledWith(SIGN_IN_URL);
    expect(await screen.findByText("Link copied.")).toBeVisible();

    card.settle(KEY_CARD);
    await screen.findByRole("group", { name: "Save its API key (openrouter)" });
    expect(screen.queryByRole("group", { name: "Open OpenRouter by hand" })).toBeNull();
  });

  it("shows why a sign-in ended with no key and lets you try again", async () => {
    api.startOpenRouterSignIn.mockResolvedValue({ url: SIGN_IN_URL, opened: true, why: "" });
    api.waitOpenRouterSignIn.mockRejectedValue("OpenRouter sign-in timed out");
    render(<AiModels />);
    await userEvent.click(await screen.findByRole("button", { name: "Sign in with OpenRouter" }));
    expect(await screen.findByText("OpenRouter sign-in timed out")).toBeVisible();
    expect(screen.queryByRole("group", { name: "Save its API key (openrouter)" })).toBeNull();
    expect(screen.getByRole("button", { name: "Sign in with OpenRouter" })).toBeEnabled();
  });

  it("shows a refused start, such as Offline only, without waiting", async () => {
    api.startOpenRouterSignIn.mockRejectedValue("Offline only is on.");
    render(<AiModels />);
    await userEvent.click(await screen.findByRole("button", { name: "Sign in with OpenRouter" }));
    expect(await screen.findByText("Offline only is on.")).toBeVisible();
    expect(api.waitOpenRouterSignIn).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Sign in with OpenRouter" })).toBeEnabled();
  });

  it("denies the sign-in's key card when it arrives after the page was left", async () => {
    const card = later<AiModelsCard>();
    api.startOpenRouterSignIn.mockResolvedValue({ url: SIGN_IN_URL, opened: true, why: "" });
    api.waitOpenRouterSignIn.mockReturnValue(card.promise);
    api.answerAiModelsCard.mockResolvedValue(EMPTY);
    const { unmount } = render(<AiModels />);
    await userEvent.click(await screen.findByRole("button", { name: "Sign in with OpenRouter" }));
    await screen.findByText(OPENROUTER_WAIT);
    expect(api.stopOpenRouterSignIn).not.toHaveBeenCalled();
    unmount();
    // Leaving stops the sign-in, so no new key is bought for a page nobody sees.
    expect(api.stopOpenRouterSignIn).toHaveBeenCalledTimes(1);
    expect(api.answerAiModelsCard).not.toHaveBeenCalled();
    card.settle(KEY_CARD);
    await waitFor(() => expect(api.answerAiModelsCard).toHaveBeenCalledWith("ai-key-openrouter-1", false));
    expect(api.answerAiModelsCard).toHaveBeenCalledTimes(1);
  });

  it("stops a sign-in on Stop signing in and says nothing was saved", async () => {
    const card = later<AiModelsCard>();
    api.startOpenRouterSignIn.mockResolvedValue({ url: SIGN_IN_URL, opened: true, why: "" });
    api.waitOpenRouterSignIn.mockReturnValue(card.promise);
    api.stopOpenRouterSignIn.mockImplementation(() => {
      card.refuse("OpenRouter sign-in stopped");
      return Promise.resolve();
    });
    const { unmount } = render(<AiModels />);
    await userEvent.click(await screen.findByRole("button", { name: "Sign in with OpenRouter" }));
    expect(screen.getByRole("button", { name: "Add provider" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Stop signing in" }));
    expect(await screen.findByText(OPENROUTER_STOPPED)).toBeVisible();
    expect(OPENROUTER_STOPPED).toBe("Sign-in stopped. Nothing was saved.");
    expect(screen.queryByText(OPENROUTER_WAIT)).toBeNull();
    expect(screen.getByRole("button", { name: "Sign in with OpenRouter" })).toBeEnabled();
    expect(api.stopOpenRouterSignIn).toHaveBeenCalledTimes(1);
    // The sign-in is over, so leaving stops nothing more.
    unmount();
    expect(api.stopOpenRouterSignIn).toHaveBeenCalledTimes(1);
  });

  it("picks up a sign-in that still waits when the page loads after a reload", async () => {
    const card = later<AiModelsCard>();
    api.openRouterSignInStatus.mockResolvedValue({ waiting: { url: SIGN_IN_URL, opened: true, why: "" }, card: null });
    api.waitOpenRouterSignIn.mockReturnValue(card.promise);
    render(<AiModels />);
    expect(await screen.findByText(OPENROUTER_WAIT)).toBeVisible();
    expect(screen.getByLabelText("OpenRouter sign-in link")).toHaveValue(SIGN_IN_URL);
    // Nothing new was started: this page waits for the one that runs.
    expect(api.startOpenRouterSignIn).not.toHaveBeenCalled();
    expect(api.waitOpenRouterSignIn).toHaveBeenCalledTimes(1);
    card.settle(KEY_CARD);
    expect(await screen.findByRole("group", { name: "Save its API key (openrouter)" })).toBeVisible();
    expect(screen.getByText(OPENROUTER_KEY_WAIT)).toBeVisible();
  });

  it("shows the key card a reloaded page never got to show", async () => {
    api.openRouterSignInStatus.mockResolvedValue({ waiting: null, card: KEY_CARD });
    api.answerAiModelsCard.mockResolvedValue({ ...EMPTY, status: "Nothing saved. The key was denied." });
    render(<AiModels />);
    const group = await screen.findByRole("group", { name: "Save its API key (openrouter)" });
    expect(api.waitOpenRouterSignIn).not.toHaveBeenCalled();
    await userEvent.click(within(group).getByRole("button", { name: "Deny" }));
    expect(api.answerAiModelsCard).toHaveBeenCalledWith("ai-key-openrouter-1", false);
  });

  it("ends quietly when a newer page took the sign-in over", async () => {
    api.startOpenRouterSignIn.mockResolvedValue({ url: SIGN_IN_URL, opened: true, why: "" });
    api.waitOpenRouterSignIn.mockRejectedValue("This sign-in moved to a newer page.");
    render(<AiModels />);
    await userEvent.click(await screen.findByRole("button", { name: "Sign in with OpenRouter" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Sign in with OpenRouter" })).toBeEnabled());
    expect(screen.queryByText("This sign-in moved to a newer page.")).toBeNull();
    expect(screen.queryByText(OPENROUTER_WAIT)).toBeNull();
  });

  it("denies a key card that is still open when the page is left", async () => {
    api.addAiProvider.mockResolvedValue(KEY_CARD);
    api.answerAiModelsCard.mockResolvedValue(EMPTY);
    const { unmount } = render(<AiModels />);
    await screen.findByText("No other providers yet.");
    await userEvent.type(screen.getByLabelText("Provider address"), "https://openrouter.ai/api/v1");
    await userEvent.type(screen.getByLabelText("Provider API key"), "test-key-aaaaaaaa");
    await userEvent.click(screen.getByRole("button", { name: "Add provider" }));
    await screen.findByRole("group", { name: "Save its API key (openrouter)" });
    expect(api.answerAiModelsCard).not.toHaveBeenCalled();
    unmount();
    expect(api.answerAiModelsCard).toHaveBeenCalledTimes(1);
    expect(api.answerAiModelsCard).toHaveBeenCalledWith("ai-key-openrouter-1", false);
  });

  it("denies a card that arrives after the page was left", async () => {
    let arrive: (card: AiModelsCard) => void = () => undefined;
    api.addAiProvider.mockReturnValue(
      new Promise<AiModelsCard>((resolve) => {
        arrive = resolve;
      }),
    );
    api.answerAiModelsCard.mockResolvedValue(EMPTY);
    const { unmount } = render(<AiModels />);
    await screen.findByText("No other providers yet.");
    await userEvent.type(screen.getByLabelText("Provider address"), "https://openrouter.ai/api/v1");
    await userEvent.type(screen.getByLabelText("Provider API key"), "test-key-aaaaaaaa");
    await userEvent.click(screen.getByRole("button", { name: "Add provider" }));
    unmount();
    expect(api.answerAiModelsCard).not.toHaveBeenCalled();
    arrive(KEY_CARD);
    await waitFor(() => expect(api.answerAiModelsCard).toHaveBeenCalledWith("ai-key-openrouter-1", false));
    expect(api.answerAiModelsCard).toHaveBeenCalledTimes(1);
  });

  it("answers a card once: leaving after Deny or with no card sends nothing more", async () => {
    api.addAiProvider.mockResolvedValue(KEY_CARD);
    api.answerAiModelsCard.mockResolvedValue({ ...EMPTY, status: "Nothing saved. The key was denied." });
    const first = render(<AiModels />);
    await screen.findByText("No other providers yet.");
    first.unmount();
    expect(api.answerAiModelsCard).not.toHaveBeenCalled();

    const second = render(<AiModels />);
    await screen.findByText("No other providers yet.");
    await userEvent.type(screen.getByLabelText("Provider address"), "https://openrouter.ai/api/v1");
    await userEvent.type(screen.getByLabelText("Provider API key"), "test-key-aaaaaaaa");
    await userEvent.click(screen.getByRole("button", { name: "Add provider" }));
    await userEvent.click(await screen.findByRole("button", { name: "Deny" }));
    await screen.findByText("Nothing saved. The key was denied.");
    second.unmount();
    expect(api.answerAiModelsCard).toHaveBeenCalledTimes(1);
  });

  it("shows a key the keyring could not take as an error and keeps the address", async () => {
    api.addAiProvider.mockResolvedValue(KEY_CARD);
    api.answerAiModelsCard.mockRejectedValue("The keyring isn't available, so the key wasn't saved.");
    render(<AiModels />);
    await screen.findByText("No other providers yet.");
    await userEvent.type(screen.getByLabelText("Provider address"), "https://openrouter.ai/api/v1");
    await userEvent.type(screen.getByLabelText("Provider API key"), "test-key-aaaaaaaa");
    await userEvent.click(screen.getByRole("button", { name: "Add provider" }));
    await userEvent.click(await screen.findByRole("button", { name: "Save" }));
    const said = await screen.findByText("The keyring isn't available, so the key wasn't saved.");
    expect(said).toHaveClass("status--error");
    expect(screen.getByLabelText("Provider address")).toHaveValue("https://openrouter.ai/api/v1");
    expect(screen.getByLabelText("Provider API key")).toHaveValue("");
  });

  describe("web search", () => {
    it("picks free DuckDuckGo Lite by default and shows no URL or key field", async () => {
      render(<AiModels />);
      expect(await screen.findByText(WEB_SEARCH_NOTE)).toBeVisible();
      expect(WEB_SEARCH_NOTE).toBe(
        "For OpenRouter and the local model. Free DuckDuckGo Lite by default, no account. Grok still uses xAI's own search.",
      );
      expect(screen.getByLabelText("Search backend")).toHaveValue("duckduckgo");
      expect(screen.getByRole("option", { name: "DuckDuckGo Lite (free, no key)" })).toBeInTheDocument();
      expect(screen.queryByLabelText("SearXNG URL")).toBeNull();
      expect(screen.queryByLabelText("Brave API key")).toBeNull();
    });

    it("saves SearXNG when picked and then shows its URL field", async () => {
      api.saveWebSearch.mockResolvedValue({ ...searching({ backend: "searxng" }), status: "Saved." });
      render(<AiModels />);
      await userEvent.selectOptions(await screen.findByLabelText("Search backend"), "searxng");
      expect(api.saveWebSearch).toHaveBeenCalledWith("searxng", "");
      expect(await screen.findByLabelText("SearXNG URL")).toHaveValue("");
      expect(screen.getByText(SEARXNG_NOTE)).toBeVisible();
      expect(screen.getByText("Saved.")).toBeVisible();
    });

    it("saves the SearXNG URL trimmed, on Enter", async () => {
      api.getAiModels.mockResolvedValue(searching({ backend: "searxng" }));
      api.saveWebSearch.mockResolvedValue({
        ...searching({ backend: "searxng", searxngUrl: "https://searx.example" }),
        status: "Saved.",
      });
      render(<AiModels />);
      const field = await screen.findByLabelText("SearXNG URL");
      expect(screen.getByRole("button", { name: "Save URL" })).toBeDisabled();
      await userEvent.type(field, "  https://searx.example  {Enter}");
      expect(api.saveWebSearch).toHaveBeenCalledWith("searxng", "https://searx.example");
      await waitFor(() => expect(field).toHaveValue("https://searx.example"));
      expect(screen.getByRole("button", { name: "Save URL" })).toBeDisabled();
    });

    it("shows why a SearXNG URL wasn't saved", async () => {
      api.getAiModels.mockResolvedValue(searching({ backend: "searxng" }));
      api.saveWebSearch.mockRejectedValue("SearXNG URL must start with http:// or https://.");
      render(<AiModels />);
      await userEvent.type(await screen.findByLabelText("SearXNG URL"), "searx.example");
      await userEvent.click(screen.getByRole("button", { name: "Save URL" }));
      expect(await screen.findByText("SearXNG URL must start with http:// or https://.")).toHaveClass("status--error");
      expect(screen.getByLabelText("SearXNG URL")).toHaveValue("searx.example");
    });

    it("lets you pick SearXNG with an old LAN URL and shows why it won't work", async () => {
      api.getAiModels.mockResolvedValue(searching({ searxngUrl: "http://192.168.1.5:8888" }));
      api.saveWebSearch.mockResolvedValue({
        ...searching({
          backend: "searxng",
          searxngUrl: "http://192.168.1.5:8888",
          searxngProblem: "Web search only reaches public addresses, so a SearXNG server on this computer or your own network can't be used.",
        }),
        status: "Saved.",
      });
      render(<AiModels />);
      await userEvent.selectOptions(await screen.findByLabelText("Search backend"), "searxng");
      expect(api.saveWebSearch).toHaveBeenCalledWith("searxng", "http://192.168.1.5:8888");
      expect(await screen.findByText("Web search only reaches public addresses, so a SearXNG server on this computer or your own network can't be used.")).toHaveClass("warn");
      expect(screen.getByLabelText("SearXNG URL")).toHaveValue("http://192.168.1.5:8888");
      expect(screen.getByLabelText("SearXNG URL")).toBeEnabled();
    });

    it("closes the Remove key confirm when the backend changes", async () => {
      api.getAiModels.mockResolvedValue(searching({ backend: "brave", braveKeySet: true }));
      api.saveWebSearch
        .mockResolvedValueOnce({ ...searching({ backend: "duckduckgo", braveKeySet: true }), status: "Saved." })
        .mockResolvedValueOnce({ ...searching({ backend: "brave", braveKeySet: true }), status: "Saved." });
      render(<AiModels />);
      await userEvent.click(await screen.findByRole("button", { name: "Remove key" }));
      expect(screen.getByRole("group", { name: "Remove the Brave Search key?" })).toBeVisible();
      await userEvent.selectOptions(screen.getByLabelText("Search backend"), "duckduckgo");
      await waitFor(() => expect(screen.queryByLabelText("Brave API key")).toBeNull());
      await userEvent.selectOptions(screen.getByLabelText("Search backend"), "brave");
      expect(await screen.findByLabelText("Brave API key")).toBeVisible();
      expect(screen.queryByRole("group", { name: "Remove the Brave Search key?" })).toBeNull();
      expect(api.saveBraveKey).not.toHaveBeenCalled();
    });

    it("sends the Brave key only to saveBraveKey and empties the field", async () => {
      api.getAiModels.mockResolvedValue(searching({ backend: "brave" }));
      api.saveBraveKey.mockResolvedValue({ ...searching({ backend: "brave", braveKeySet: true }), status: "Brave search key saved." });
      render(<AiModels />);
      const field = await screen.findByLabelText("Brave API key");
      expect(field).toHaveAttribute("type", "password");
      expect(screen.getByText(BRAVE_UNSET)).toBeVisible();
      expect(screen.getByRole("button", { name: "Save key" })).toBeDisabled();
      await userEvent.type(field, "sk-abcdefghijklmnopqrstuv");
      await userEvent.click(screen.getByRole("button", { name: "Save key" }));
      expect(api.saveBraveKey).toHaveBeenCalledWith("sk-abcdefghijklmnopqrstuv");
      expect(api.saveWebSearch).not.toHaveBeenCalled();
      expect(field).toHaveValue("");
      expect(await screen.findByText(BRAVE_SET)).toBeVisible();
      expect(screen.getByText("Brave search key saved.")).toBeVisible();
    });

    it("removes a saved Brave key only after a confirm, never on Enter", async () => {
      api.getAiModels.mockResolvedValue(searching({ backend: "brave", braveKeySet: true }));
      api.saveBraveKey.mockResolvedValue({ ...searching({ backend: "brave" }), status: "Brave search key cleared." });
      render(<AiModels />);
      await userEvent.type(await screen.findByLabelText("Brave API key"), "{Enter}");
      expect(api.saveBraveKey).not.toHaveBeenCalled();
      await userEvent.click(screen.getByRole("button", { name: "Remove key" }));
      expect(api.saveBraveKey).not.toHaveBeenCalled();
      await userEvent.click(screen.getByRole("button", { name: "Remove" }));
      expect(api.saveBraveKey).toHaveBeenCalledWith("");
      expect(await screen.findByText(BRAVE_UNSET)).toBeVisible();
      expect(screen.queryByRole("button", { name: "Remove key" })).toBeNull();
    });
  });
});

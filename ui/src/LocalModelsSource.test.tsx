import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LocalModelNews } from "./api";

const api = vi.hoisted(() => ({
  getLocalModelNews: vi.fn<() => Promise<LocalModelNews>>(),
  setLocalModelNews: vi.fn<(on: boolean) => Promise<LocalModelNews>>(),
  localModelNewsCheckNow: vi.fn<() => Promise<LocalModelNews>>(),
}));
vi.mock("./api", () => api);

import { CHECKING, LOCAL_MODELS_HELP, LocalModelsSource, POLL_MS } from "./LocalModelsSource";

const OFF: LocalModelNews = { on: false, note: "" };
const ON: LocalModelNews = { on: true, note: "" };

describe("LocalModelsSource", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts off with no Check now, and the toggle turns it on", async () => {
    api.getLocalModelNews.mockResolvedValue(OFF);
    api.setLocalModelNews.mockResolvedValue(ON);
    render(<LocalModelsSource />);
    const toggle = await screen.findByRole("switch", { name: "Suggest new local models that fit this PC" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByRole("button", { name: "Check now" })).toBeNull();
    expect(screen.queryByText(LOCAL_MODELS_HELP)).toBeNull();
    await userEvent.click(toggle);
    expect(api.setLocalModelNews).toHaveBeenCalledWith(true);
    expect(await screen.findByRole("button", { name: "Check now" })).toBeEnabled();
    expect(screen.getByText(/^Once a week, from this PC's memory and huihui-ai's abliterated builds/)).toBeInTheDocument();
  });

  it("Check now shows the check running, then what it found", async () => {
    api.getLocalModelNews.mockResolvedValue(ON);
    api.localModelNewsCheckNow.mockResolvedValue({ on: true, note: CHECKING });
    render(<LocalModelsSource />);
    const check = await screen.findByRole("button", { name: "Check now" });
    vi.useFakeTimers({ shouldAdvanceTime: true });
    api.getLocalModelNews.mockResolvedValue({ on: true, note: "Checked. 1 new for this PC." });
    await userEvent.click(check);
    expect(api.localModelNewsCheckNow).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status")).toHaveTextContent("Checking for new local models…");
    expect(screen.getByRole("button", { name: "Check now" })).toBeDisabled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    expect(screen.getByRole("status")).toHaveTextContent("Checked. 1 new for this PC.");
    expect(screen.getByRole("button", { name: "Check now" })).toBeEnabled();
    expect(api.localModelNewsCheckNow).toHaveBeenCalledTimes(1);
  });

  it("shows why a change didn't go through", async () => {
    api.getLocalModelNews.mockResolvedValue(ON);
    api.localModelNewsCheckNow.mockRejectedValue("Turn local model suggestions on first.");
    render(<LocalModelsSource />);
    await userEvent.click(await screen.findByRole("button", { name: "Check now" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Turn local model suggestions on first.");
  });
});

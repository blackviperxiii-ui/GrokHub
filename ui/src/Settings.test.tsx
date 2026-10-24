import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings as Shape } from "./api";

const api = vi.hoisted(() => ({
  getSettings: vi.fn<() => Promise<Shape>>(),
  saveSettings: vi.fn<(model: string, closeToTray: boolean) => Promise<Shape>>(),
  setApiKey: vi.fn<(key: string) => Promise<Shape>>(),
  clearApiKey: vi.fn<() => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import { Settings } from "./Settings";

describe("Settings", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.getSettings.mockResolvedValue({ model: "grok-4.7", closeToTray: true, hasApiKey: false });
  });

  it("loads and saves the model and tray choice", async () => {
    api.saveSettings.mockResolvedValue({ model: "grok-5", closeToTray: false, hasApiKey: false });
    render(<Settings />);

    const model = await screen.findByLabelText("Model");
    expect(model).toHaveValue("grok-4.7");
    await userEvent.clear(model);
    await userEvent.type(model, " grok-5 ");
    await userEvent.click(screen.getByLabelText(/keeps GrokHub in the tray/));
    await userEvent.click(screen.getByRole("button", { name: "Save settings" }));

    expect(api.saveSettings).toHaveBeenCalledWith("grok-5", false);
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
    expect(screen.getByLabelText(/keeps GrokHub in the tray/)).not.toBeChecked();
  });

  it("saves a key without echoing it, then removes it", async () => {
    api.setApiKey.mockResolvedValue({ model: "grok-4.7", closeToTray: true, hasApiKey: true });
    api.clearApiKey.mockResolvedValue({ model: "grok-4.7", closeToTray: true, hasApiKey: false });
    render(<Settings />);

    expect(await screen.findByText("No key saved yet.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove key" })).toBeNull();
    await userEvent.type(screen.getByLabelText("API key"), "sk-abcdefghijklmnopqrstuv");
    await userEvent.click(screen.getByRole("button", { name: "Save key" }));

    expect(api.setApiKey).toHaveBeenCalledWith("sk-abcdefghijklmnopqrstuv");
    expect(await screen.findByText("A key is saved.")).toBeInTheDocument();
    expect(screen.getByLabelText("API key")).toHaveValue("");

    await userEvent.click(screen.getByRole("button", { name: "Remove key" }));
    expect(api.clearApiKey).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("No key saved yet.")).toBeInTheDocument();
  });

  it("shows a keychain error instead of hanging", async () => {
    api.setApiKey.mockRejectedValue("No system keychain is available.");
    render(<Settings />);
    await userEvent.type(await screen.findByLabelText("API key"), "sk-abcdefghijklmnopqrstuv");
    await userEvent.click(screen.getByRole("button", { name: "Save key" }));
    expect(await screen.findByText("No system keychain is available.")).toBeInTheDocument();
  });
});

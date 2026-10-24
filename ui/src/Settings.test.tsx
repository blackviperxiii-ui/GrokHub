import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings as Shape } from "./api";

const api = vi.hoisted(() => ({
  getSettings: vi.fn<() => Promise<Shape>>(),
  saveSettings: vi.fn<(model: string, closeToTray: boolean, workspace: string) => Promise<Shape>>(),
  setApiKey: vi.fn<(key: string) => Promise<Shape>>(),
  clearApiKey: vi.fn<() => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import { Settings } from "./Settings";

function shape(over: Partial<Shape>): Shape {
  return {
    model: "grok-4.7",
    closeToTray: true,
    hasApiKey: false,
    workspace: "",
    workspacePath: "/home/viper/GrokHub",
    ...over,
  };
}

describe("Settings", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.getSettings.mockResolvedValue(shape({}));
  });

  it("loads and saves the model, work folder and tray choice", async () => {
    api.saveSettings.mockResolvedValue(
      shape({ model: "grok-5", closeToTray: false, workspace: "/work", workspacePath: "/work" }),
    );
    render(<Settings />);

    const model = await screen.findByLabelText("Model");
    expect(model).toHaveValue("grok-4.7");
    await userEvent.clear(model);
    await userEvent.type(model, " grok-5 ");
    expect(screen.getByText("/home/viper/GrokHub")).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Work folder"), " /work ");
    await userEvent.click(screen.getByLabelText(/keeps GrokHub in the tray/));
    await userEvent.click(screen.getByRole("button", { name: "Save settings" }));

    expect(api.saveSettings).toHaveBeenCalledWith("grok-5", false, "/work");
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
    expect(screen.getByLabelText(/keeps GrokHub in the tray/)).not.toBeChecked();
    expect(screen.getByText("/work")).toBeInTheDocument();
  });

  it("saves a key without echoing it, then removes it", async () => {
    api.setApiKey.mockResolvedValue(shape({ hasApiKey: true }));
    api.clearApiKey.mockResolvedValue(shape({}));
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

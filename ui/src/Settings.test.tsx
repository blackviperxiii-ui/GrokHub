import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Capabilities, Settings as Shape } from "./api";

const api = vi.hoisted(() => ({
  getSettings: vi.fn<() => Promise<Shape>>(),
  saveSettings:
    vi.fn<
      (
        model: string,
        closeToTray: boolean,
        workspace: string,
        approvals: string,
        desktopControl: boolean,
      ) => Promise<Shape>
    >(),
  setApiKey: vi.fn<(key: string) => Promise<Shape>>(),
  clearApiKey: vi.fn<() => Promise<Shape>>(),
}));

vi.mock("./api", () => api);
// Local AI has its own tests (LocalAi.test.tsx).
vi.mock("./LocalAi", () => ({ LocalAi: () => <p>Local AI panel</p> }));

import { DESKTOP_NOTE, Settings } from "./Settings";

const DESKTOP: Capabilities = { desktopControl: true, pulse: true, localAi: true, tray: true };
const PHONE: Capabilities = { desktopControl: false, pulse: false, localAi: false, tray: false };

function shape(over: Partial<Shape>): Shape {
  return {
    model: "grok-4.7",
    closeToTray: true,
    hasApiKey: false,
    workspace: "",
    workspacePath: "/home/user/GrokHub",
    approvals: "ask",
    desktopControl: false,
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
    render(<Settings caps={DESKTOP} />);

    const model = await screen.findByLabelText("Model");
    expect(model).toHaveValue("grok-4.7");
    await userEvent.clear(model);
    await userEvent.type(model, " grok-5 ");
    expect(screen.getByText("/home/user/GrokHub")).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Work folder"), " /work ");
    await userEvent.click(screen.getByLabelText(/keeps GrokHub in the tray/));
    await userEvent.click(screen.getByRole("button", { name: "Save settings" }));

    expect(api.saveSettings).toHaveBeenCalledWith("grok-5", false, "/work", "ask", false);
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
    expect(screen.getByLabelText(/keeps GrokHub in the tray/)).not.toBeChecked();
    expect(screen.getByText("/work")).toBeInTheDocument();
  });

  it("approvals start at Ask and Always says what still asks", async () => {
    api.saveSettings.mockResolvedValue(shape({ approvals: "always" }));
    render(<Settings caps={DESKTOP} />);

    const pick = await screen.findByLabelText("Approvals");
    expect(pick).toHaveValue("ask");
    expect(
      screen.getByText("It asks before it changes a file or runs a command."),
    ).toBeInTheDocument();
    await userEvent.selectOptions(pick, "always");
    expect(
      screen.getByText(
        "Files change and commands run without asking. Deleting, sending, paying and credentials still ask.",
      ),
    ).toHaveClass("warn");
    await userEvent.click(screen.getByRole("button", { name: "Save settings" }));

    expect(api.saveSettings).toHaveBeenCalledWith("grok-4.7", true, "", "always", false);
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
    expect(screen.getByLabelText("Approvals")).toHaveValue("always");
  });

  it("desktop control starts off, says what still asks, and saves", async () => {
    api.saveSettings.mockResolvedValue(shape({ desktopControl: true }));
    render(<Settings caps={DESKTOP} />);

    const desk = await screen.findByLabelText("Let Grok control the desktop");
    expect(desk).not.toBeChecked();
    expect(screen.queryByText(DESKTOP_NOTE)).not.toBeInTheDocument();
    await userEvent.click(desk);
    expect(screen.getByText(DESKTOP_NOTE)).toHaveClass("warn");
    await userEvent.click(screen.getByRole("button", { name: "Save settings" }));

    expect(api.saveSettings).toHaveBeenCalledWith("grok-4.7", true, "", "ask", true);
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
    expect(screen.getByLabelText("Let Grok control the desktop")).toBeChecked();
  });

  it("saves a key without echoing it, then removes it", async () => {
    api.setApiKey.mockResolvedValue(shape({ hasApiKey: true }));
    api.clearApiKey.mockResolvedValue(shape({}));
    render(<Settings caps={DESKTOP} />);

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
    render(<Settings caps={DESKTOP} />);
    await userEvent.type(await screen.findByLabelText("API key"), "sk-abcdefghijklmnopqrstuv");
    await userEvent.click(screen.getByRole("button", { name: "Save key" }));
    expect(await screen.findByText("No system keychain is available.")).toBeInTheDocument();
  });

  it("hides the desktop-only settings on a phone", async () => {
    api.getSettings.mockResolvedValue(shape({ desktopControl: true }));
    api.saveSettings.mockResolvedValue(shape({}));
    render(<Settings caps={PHONE} />);

    expect(await screen.findByLabelText("Model")).toBeInTheDocument();
    expect(screen.queryByLabelText("Let Grok control the desktop")).toBeNull();
    expect(screen.queryByText(DESKTOP_NOTE)).toBeNull();
    expect(screen.queryByLabelText(/keeps GrokHub in the tray/)).toBeNull();
    expect(screen.queryByText("Local AI panel")).toBeNull();
  });

  it("shows them on a desktop", async () => {
    render(<Settings caps={DESKTOP} />);
    expect(await screen.findByLabelText("Let Grok control the desktop")).toBeInTheDocument();
    expect(screen.getByLabelText(/keeps GrokHub in the tray/)).toBeInTheDocument();
    expect(screen.getByText("Local AI panel")).toBeInTheDocument();
  });
});

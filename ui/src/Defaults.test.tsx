import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "./api";

const api = vi.hoisted(() => ({
  saveDefaults: vi.fn<(workspace: string, approvals: string) => Promise<Settings>>(),
  setModel: vi.fn<(id: string) => Promise<Settings>>(),
  listModels: vi.fn(),
}));

vi.mock("./api", () => api);

import { Defaults } from "./Defaults";

function shape(over: Partial<Settings> = {}): Settings {
  return {
    model: "grok-4.7",
    closeToTray: true,
    hasApiKey: false,
    workspace: "",
    workspacePath: "/home/user/GrokHub",
    approvals: "ask",
    desktopControl: false,
    theme: "system",
    summonHotkey: false,
    notifications: true,
    version: "3.1.0",
    ...over,
  };
}

describe("Defaults", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.listModels.mockResolvedValue({
      current: "grok-4.7",
      options: [
        { id: "grok-4.7", label: "Grok 4.7", source: "catalog" },
        { id: "grok-5", label: "Grok 5", source: "catalog" },
      ],
      note: null,
    });
  });

  it("a model pick saves at once with the picker from the composer", async () => {
    const onSaved = vi.fn();
    const saved = shape({ model: "grok-5" });
    api.setModel.mockResolvedValue(saved);
    render(<Defaults settings={shape()} onSaved={onSaved} />);
    expect(screen.getByLabelText("Model")).toHaveTextContent("grok-4.7");
    await userEvent.click(screen.getByLabelText("Model"));
    await userEvent.click(await screen.findByRole("button", { name: /Grok 5/ }));
    expect(api.setModel).toHaveBeenCalledWith("grok-5");
    expect(onSaved).toHaveBeenCalledWith(saved);
    expect(await screen.findByText("New messages use grok-5.")).toBeInTheDocument();
    expect(api.saveDefaults).not.toHaveBeenCalled();
  });

  it("saves the work folder and approvals, and Always says what still asks", async () => {
    const onSaved = vi.fn();
    const saved = shape({ workspace: "/work", workspacePath: "/work", approvals: "always" });
    api.saveDefaults.mockResolvedValue(saved);
    render(<Defaults settings={shape()} onSaved={onSaved} />);

    expect(screen.getByText("/home/user/GrokHub")).toBeInTheDocument();
    const pick = screen.getByLabelText("Approvals");
    expect(pick).toHaveValue("ask");
    expect(screen.getByText("It asks before it changes a file or runs a command.")).toBeInTheDocument();
    await userEvent.selectOptions(pick, "always");
    expect(
      screen.getByText(
        "Files change and commands run without asking. Deleting, sending, paying and credentials still ask.",
      ),
    ).toHaveClass("warn");
    await userEvent.type(screen.getByLabelText("Work folder"), " /work ");
    await userEvent.click(screen.getByRole("button", { name: "Save defaults" }));

    expect(api.saveDefaults).toHaveBeenCalledWith("/work", "always");
    expect(onSaved).toHaveBeenCalledWith(saved);
    expect(await screen.findByText("Defaults saved.")).toBeInTheDocument();
  });

  it("shows a save error", async () => {
    api.saveDefaults.mockRejectedValue("disk full");
    render(<Defaults settings={shape()} />);
    await userEvent.click(screen.getByRole("button", { name: "Save defaults" }));
    expect(await screen.findByText("disk full")).toBeInTheDocument();
  });

  it("the web app sees them with nothing to change them", () => {
    render(<Defaults settings={shape({ approvals: "auto" })} readOnly />);
    expect(screen.getByText("grok-4.7")).toBeInTheDocument();
    expect(screen.getByText("/home/user/GrokHub")).toBeInTheDocument();
    expect(screen.getByText("Auto")).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
  });
});

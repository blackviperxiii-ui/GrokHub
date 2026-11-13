import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "./api";

const api = vi.hoisted(() => ({
  saveDefaults: vi.fn<(workspace: string, approvals: string) => Promise<Settings>>(),
}));

vi.mock("./api", () => api);

import { Defaults } from "./Defaults";

function shape(over: Partial<Settings> = {}): Settings {
  return {
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
  });

  it("has no model or effort to see or pick: the router picks both", () => {
    const view = render(<Defaults settings={shape()} />);
    expect(screen.queryByLabelText("Model")).toBeNull();
    expect(screen.queryByText(/model/i)).toBeNull();
    expect(screen.queryByText(/effort/i)).toBeNull();
    expect(screen.queryByText(/grok-/i)).toBeNull();
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual(["Save defaults"]);
    view.unmount();
    render(<Defaults settings={shape()} readOnly />);
    expect(screen.queryByText(/model/i)).toBeNull();
    expect(screen.queryByText(/effort/i)).toBeNull();
    expect(screen.queryByText(/grok-/i)).toBeNull();
  });

  it("saves the work folder and approvals, and Always says what still asks", async () => {
    const onSaved = vi.fn();
    const saved = shape({ workspace: "/work", workspacePath: "/work", approvals: "always" });
    api.saveDefaults.mockResolvedValue(saved);
    render(<Defaults settings={shape()} onSaved={onSaved} />);

    expect(screen.getByText("/home/user/GrokHub")).toBeInTheDocument();
    const pick = screen.getByLabelText("When the AI wants to change things");
    expect(pick).toHaveValue("ask");
    expect(screen.getByText("It asks before it changes a file or runs a command.")).toBeInTheDocument();
    await userEvent.selectOptions(pick, "always");
    expect(
      screen.getByText(
        "Files change and commands run without asking. Deleting, sending, paying and passwords still ask.",
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
    expect(screen.getByText("/home/user/GrokHub")).toBeInTheDocument();
    expect(screen.getByText("Only ask for risky things")).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
  });
});

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CODE_SETTINGS_NONE, CodeSettings, SHIP_MODE_ASK, SHIP_MODE_AUTO, SHIP_SETTINGS_RULES } from "./CodeSettings";

const api = vi.hoisted(() => ({
  codeTestRemembered: vi.fn<() => Promise<{ command: string | null }>>(),
  codeTestForget: vi.fn<() => Promise<{ command: string | null }>>(),
  getCodeShip: vi.fn(),
  saveCodeShip: vi.fn(),
  codeSecretList: vi.fn(() => Promise.resolve({ names: [] as string[] })),
  codeSecretSet: vi.fn(),
  codeSecretDelete: vi.fn(),
}));

vi.mock("./api", () => api);

const SHIP = {
  mode: "ask" as const,
  autoBase: "beta",
  hosts: { github: true, other: false },
  extraProtected: ["main"],
  activityKeep: 2000,
};

describe("Settings → Code", () => {
  beforeEach(() => {
    api.codeTestRemembered.mockReset();
    api.codeTestForget.mockReset();
    api.getCodeShip.mockReset();
    api.saveCodeShip.mockReset();
    api.getCodeShip.mockResolvedValue(SHIP);
    api.saveCodeShip.mockImplementation((ship: typeof SHIP) => Promise.resolve(ship));
  });

  it("shows the remembered command and forgets it when the toggle is turned off", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: "cargo test" });
    api.codeTestForget.mockResolvedValue({ command: null });
    render(<CodeSettings />);
    expect(await screen.findByText(/Remembered command/)).toHaveTextContent("cargo test");
    const toggle = screen.getByRole("switch", { name: "Remember this project's test command" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await userEvent.click(toggle);
    expect(api.codeTestForget).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(CODE_SETTINGS_NONE)).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Remember this project's test command" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });

  it("leaves the forget toggle off and disabled when nothing is remembered", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    render(<CodeSettings />);
    expect(await screen.findByText(CODE_SETTINGS_NONE)).toBeInTheDocument();
    const toggle = screen.getByRole("switch", { name: "Remember this project's test command" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle).toBeDisabled();
  });

  it("saves the push mode, PR base and protected branches", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    render(<CodeSettings />);
    const ask = await screen.findByRole("radio", { name: SHIP_MODE_ASK });
    expect(ask).toBeChecked();
    expect(screen.getByText(SHIP_SETTINGS_RULES)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("radio", { name: SHIP_MODE_AUTO }));
    expect(api.saveCodeShip).toHaveBeenLastCalledWith({ ...SHIP, mode: "autoBeta" });
    expect(await screen.findByRole("radio", { name: SHIP_MODE_AUTO })).toBeChecked();
    const extra = screen.getByRole("textbox", { name: "Also protected" });
    await userEvent.clear(extra);
    await userEvent.type(extra, "main, release");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.saveCodeShip).toHaveBeenLastCalledWith({ ...SHIP, mode: "autoBeta", extraProtected: ["main", "release"] });
    const other = screen.getByRole("switch", { name: "Push to git hosts other than GitHub (always asks)" });
    expect(other).toHaveAttribute("aria-checked", "false");
    await userEvent.click(other);
    expect(api.saveCodeShip).toHaveBeenLastCalledWith({
      ...SHIP,
      mode: "autoBeta",
      extraProtected: ["main", "release"],
      hosts: { github: true, other: true },
    });
  });

  it("shows why a PR base was refused", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    api.saveCodeShip.mockRejectedValue("The PR base can't be main: main is never pushed to.");
    render(<CodeSettings />);
    const base = await screen.findByRole("textbox", { name: "PR base" });
    await userEvent.clear(base);
    await userEvent.type(base, "main");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("The PR base can't be main: main is never pushed to.")).toBeInTheDocument();
  });
});

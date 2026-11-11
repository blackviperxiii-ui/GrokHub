import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CommandBoxVerdict, CommandBoxView } from "./api";
import {
  COMMAND_DESTRUCTIVE,
  COMMAND_FIRST,
  COMMAND_FIRST_NOTE,
  COMMAND_NONE_YET,
  COMMAND_SENT,
  CommandBox,
} from "./CommandBox";

const api = vi.hoisted(() => ({
  commandBoxView: vi.fn<() => Promise<CommandBoxView>>(),
  commandBoxCheck: vi.fn<(line: string) => Promise<CommandBoxVerdict>>(),
  commandBoxApprove: vi.fn<(line: string) => Promise<string>>(),
  commandBoxForget: vi.fn<() => Promise<CommandBoxView>>(),
}));

vi.mock("./api", () => api);

const PROMPT_LS = "Run this shell command in the project folder: ls";

describe("CommandBox", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.commandBoxView.mockResolvedValue({ project: "demo", remembered: [] });
  });

  it("names the project folder and shows nothing run yet", async () => {
    render(<CommandBox onRun={() => {}} />);
    expect(await screen.findByText("Runs in demo")).toBeInTheDocument();
    expect(screen.getByText(COMMAND_NONE_YET)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Run" })).toBeDisabled();
  });

  it("a remembered command goes straight to the agent and lands in History", async () => {
    api.commandBoxCheck.mockResolvedValue({ verdict: "queue", command: "ls", prompt: PROMPT_LS });
    const onRun = vi.fn();
    render(<CommandBox onRun={onRun} />);
    await userEvent.type(screen.getByLabelText("Command"), " ls {Enter}");
    expect(api.commandBoxCheck).toHaveBeenCalledWith("ls");
    expect(onRun).toHaveBeenCalledWith(PROMPT_LS);
    expect(screen.getByLabelText("Command")).toHaveValue("");
    expect(screen.getByRole("status")).toHaveTextContent(COMMAND_SENT);
    expect(within(screen.getByRole("list", { name: "History" })).getByRole("button", { name: "ls" })).toBeVisible();
  });

  it("a first run asks and shows the exact command; Run it approves and sends", async () => {
    api.commandBoxCheck.mockResolvedValue({ verdict: "confirm", command: "cargo build", destructive: false });
    api.commandBoxApprove.mockResolvedValue("Run this shell command in the project folder: cargo build");
    const onRun = vi.fn();
    render(<CommandBox onRun={onRun} />);
    await userEvent.type(screen.getByLabelText("Command"), "cargo build{Enter}");
    const ask = await screen.findByRole("group", { name: COMMAND_FIRST });
    expect(within(ask).getByLabelText("Command to run")).toHaveTextContent("cargo build");
    expect(ask).toHaveTextContent(COMMAND_FIRST_NOTE);
    expect(onRun).not.toHaveBeenCalled();
    api.commandBoxView.mockResolvedValue({ project: "demo", remembered: ["cargo build"] });
    await userEvent.click(within(ask).getByRole("button", { name: "Run it" }));
    expect(api.commandBoxApprove).toHaveBeenCalledWith("cargo build");
    expect(onRun).toHaveBeenCalledWith("Run this shell command in the project folder: cargo build");
    expect(screen.queryByRole("group", { name: COMMAND_FIRST })).toBeNull();
    const kept = await screen.findByRole("list", { name: "Remembered for this project" });
    expect(kept).toHaveTextContent("cargo build");
  });

  it("Cancel on the ask sends nothing", async () => {
    api.commandBoxCheck.mockResolvedValue({ verdict: "confirm", command: "make", destructive: false });
    const onRun = vi.fn();
    render(<CommandBox onRun={onRun} />);
    await userEvent.type(screen.getByLabelText("Command"), "make{Enter}");
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("group", { name: COMMAND_FIRST })).toBeNull();
    expect(api.commandBoxApprove).not.toHaveBeenCalled();
    expect(onRun).not.toHaveBeenCalled();
  });

  it("a destructive line says it asks every time", async () => {
    api.commandBoxCheck.mockResolvedValue({ verdict: "confirm", command: "rm -r target", destructive: true });
    render(<CommandBox onRun={() => {}} />);
    await userEvent.type(screen.getByLabelText("Command"), "rm -r target{Enter}");
    const ask = await screen.findByRole("group", { name: COMMAND_FIRST });
    expect(ask).toHaveTextContent(COMMAND_DESTRUCTIVE);
    expect(ask).not.toHaveTextContent(COMMAND_FIRST_NOTE);
  });

  it("a blocked push shows the guard's message and sends nothing", async () => {
    const message = "main is the main or default branch. The app never pushes to it; make a branch first.";
    api.commandBoxCheck.mockResolvedValue({ verdict: "blocked", command: "git push origin main", message });
    const onRun = vi.fn();
    render(<CommandBox onRun={onRun} />);
    await userEvent.type(screen.getByLabelText("Command"), "git push origin main{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    expect(screen.getByLabelText("Command")).toHaveValue("git push origin main");
    expect(onRun).not.toHaveBeenCalled();
  });

  it("the up arrow walks back through what ran, and down comes back to an empty box", async () => {
    api.commandBoxCheck.mockImplementation((line) =>
      Promise.resolve({ verdict: "queue", command: line, prompt: `run ${line}` }),
    );
    render(<CommandBox onRun={() => {}} />);
    const box = screen.getByLabelText("Command");
    await userEvent.type(box, "ls{Enter}");
    await waitFor(() => expect(box).toHaveValue(""));
    await userEvent.type(box, "git status{Enter}");
    await waitFor(() => expect(box).toHaveValue(""));
    await userEvent.type(box, "{ArrowUp}");
    expect(box).toHaveValue("git status");
    await userEvent.type(box, "{ArrowUp}");
    expect(box).toHaveValue("ls");
    await userEvent.type(box, "{ArrowDown}");
    expect(box).toHaveValue("git status");
    await userEvent.type(box, "{ArrowDown}");
    expect(box).toHaveValue("");
  });

  it("Forget clears the remembered commands", async () => {
    api.commandBoxView.mockResolvedValue({ project: "demo", remembered: ["npm test", "ls"] });
    api.commandBoxForget.mockResolvedValue({ project: "demo", remembered: [] });
    render(<CommandBox onRun={() => {}} />);
    const kept = await screen.findByRole("list", { name: "Remembered for this project" });
    expect(within(kept).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["npm test", "ls"]);
    await userEvent.click(screen.getByRole("button", { name: "Forget remembered commands" }));
    expect(api.commandBoxForget).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByRole("list", { name: "Remembered for this project" })).toBeNull());
  });
});

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeActivityEntry, CodeShipPlan } from "./api";

const api = vi.hoisted(() => ({
  codeShipPlan: vi.fn<(action: "push" | "pr") => Promise<CodeShipPlan>>(),
  codeShipRun: vi.fn<(action: "push" | "pr", confirmed: boolean, command?: string | null) => Promise<{ command: string; exit: number; output: string; summary: string }>>(),
  codeNewBranch: vi.fn<(name: string) => Promise<string>>(),
  codeActivity: vi.fn<() => Promise<CodeActivityEntry[]>>(),
}));

vi.mock("./api", () => api);

import { CodeShip, SHIP_CONFIRM_PR, SHIP_CONFIRM_PUSH, SHIP_NO_ACTIVITY, SHIP_RULES } from "./CodeShip";

function plan(action: "push" | "pr", over: Partial<CodeShipPlan> = {}): CodeShipPlan {
  return {
    action,
    branch: "code/fix",
    command:
      action === "push"
        ? "git push --porcelain origin HEAD:refs/heads/code/fix"
        : "gh pr create --base beta --head code/fix --fill",
    needsConfirm: true,
    blocked: null,
    mode: "ask",
    autoBase: "beta",
    appMade: true,
    prUrl: null,
    ...over,
  };
}

describe("Code tab → Push and PR", () => {
  beforeEach(() => {
    api.codeShipPlan.mockReset();
    api.codeShipRun.mockReset();
    api.codeNewBranch.mockReset();
    api.codeActivity.mockReset();
    api.codeActivity.mockResolvedValue([]);
    api.codeShipPlan.mockImplementation((action) => Promise.resolve(plan(action)));
    api.codeShipRun.mockResolvedValue({
      command: "git push --porcelain origin HEAD:refs/heads/code/fix",
      exit: 0,
      output: "To github.com:o/r.git\n",
      summary: "Pushed code/fix to origin.",
    });
  });

  it("in Ask mode every push shows the exact command first", async () => {
    render(<CodeShip />);
    expect(await screen.findByText(new RegExp(SHIP_RULES.replace(/[.]/g, "\\.")))).toBeInTheDocument();
    expect(screen.getByText(SHIP_NO_ACTIVITY)).toBeInTheDocument();
    for (let i = 0; i < 2; i++) {
      await userEvent.click(await screen.findByRole("button", { name: "Push code/fix" }));
      const card = await screen.findByRole("group", { name: SHIP_CONFIRM_PUSH });
      expect(card).toHaveTextContent("git push --porcelain origin HEAD:refs/heads/code/fix");
      expect(api.codeShipRun).toHaveBeenCalledTimes(i);
      await userEvent.click(screen.getByRole("button", { name: "Push" }));
      expect(api.codeShipRun).toHaveBeenLastCalledWith("push", true, "git push --porcelain origin HEAD:refs/heads/code/fix");
      expect(await screen.findByRole("status")).toHaveTextContent("Pushed code/fix to origin.");
    }
  });

  it("Auto-beta pushes an app branch without a confirm", async () => {
    api.codeShipPlan.mockImplementation((action) => Promise.resolve(plan(action, { mode: "autoBeta", needsConfirm: false })));
    render(<CodeShip />);
    expect(await screen.findByText(/Auto-beta: a branch made here pushes into beta without asking\./)).toBeInTheDocument();
    await userEvent.click(await screen.findByRole("button", { name: "Push code/fix" }));
    expect(api.codeShipRun).toHaveBeenCalledWith("push", false);
    expect(screen.queryByRole("group", { name: SHIP_CONFIRM_PUSH })).not.toBeInTheDocument();
  });

  it("a blocked push says why and runs nothing", async () => {
    const why = "main is the main or default branch. The app never pushes to it; make a branch first.";
    api.codeShipPlan.mockImplementation((action) => Promise.resolve(plan(action, { branch: "main", blocked: why })));
    render(<CodeShip />);
    await userEvent.click(await screen.findByRole("button", { name: "Push main" }));
    expect(await screen.findByRole("status")).toHaveTextContent(why);
    expect(api.codeShipRun).not.toHaveBeenCalled();
    expect(screen.queryByRole("group", { name: SHIP_CONFIRM_PUSH })).not.toBeInTheDocument();
  });

  it("Open PR into beta confirms the gh command", async () => {
    render(<CodeShip />);
    await userEvent.click(await screen.findByRole("button", { name: "Open PR into beta" }));
    const card = await screen.findByRole("group", { name: SHIP_CONFIRM_PR });
    expect(card).toHaveTextContent("gh pr create --base beta --head code/fix --fill");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.codeShipRun).not.toHaveBeenCalled();
  });

  it("makes a branch and lists the activity log", async () => {
    api.codeNewBranch.mockResolvedValue("code/new");
    api.codeActivity.mockResolvedValue([
      {
        tsMs: 1_700_000_000_000,
        kind: "blocked",
        repoKey: "k",
        host: "github",
        branch: "main",
        command: "git push --porcelain origin HEAD:refs/heads/main",
        mode: "ask",
        approvedBy: "none",
        exit: null,
        summary: "main is the main or default branch.",
      },
    ]);
    render(<CodeShip />);
    const log = await screen.findByRole("list", { name: "Activity" });
    expect(log).toHaveTextContent("Blocked");
    expect(log).toHaveTextContent("git push --porcelain origin HEAD:refs/heads/main");
    await userEvent.type(screen.getByRole("textbox", { name: "New branch" }), "code/new");
    await userEvent.click(screen.getByRole("button", { name: "Make branch" }));
    expect(api.codeNewBranch).toHaveBeenCalledWith("code/new");
    expect(await screen.findByRole("status")).toHaveTextContent("On code/new now.");
  });

  it("a confirm card goes away when the branch changes, and a confirm sends the command it showed", async () => {
    let branch = "code/a";
    api.codeShipPlan.mockImplementation((action) =>
      Promise.resolve(plan(action, { branch, command: `git push --porcelain origin HEAD:refs/heads/${branch}` })),
    );
    api.codeNewBranch.mockImplementation((name) => {
      branch = name;
      return Promise.resolve(name);
    });
    render(<CodeShip />);
    await userEvent.click(await screen.findByRole("button", { name: "Push code/a" }));
    expect(await screen.findByRole("group", { name: SHIP_CONFIRM_PUSH })).toHaveTextContent(
      "git push --porcelain origin HEAD:refs/heads/code/a",
    );
    await userEvent.type(screen.getByRole("textbox", { name: "New branch" }), "code/b");
    await userEvent.click(screen.getByRole("button", { name: "Make branch" }));
    expect(await screen.findByRole("button", { name: "Push code/b" })).toBeEnabled();
    expect(screen.queryByRole("group", { name: SHIP_CONFIRM_PUSH })).toBeNull();
    expect(api.codeShipRun).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Push code/b" }));
    expect(await screen.findByRole("group", { name: SHIP_CONFIRM_PUSH })).toHaveTextContent(
      "git push --porcelain origin HEAD:refs/heads/code/b",
    );
    await userEvent.click(screen.getByRole("button", { name: "Push" }));
    expect(api.codeShipRun).toHaveBeenCalledTimes(1);
    expect(api.codeShipRun).toHaveBeenLastCalledWith("push", true, "git push --porcelain origin HEAD:refs/heads/code/b");
  });
});

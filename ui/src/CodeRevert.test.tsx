import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeCommit, CodeRevertPlan } from "./codeRevertApi";

const api = vi.hoisted(() => ({
  codeRevertList: vi.fn<() => Promise<CodeCommit[]>>(),
  codeRevertPlan: vi.fn<(sha: string) => Promise<CodeRevertPlan>>(),
  codeRevertRun: vi.fn<(plan: CodeRevertPlan) => Promise<{ commit: string; summary: string; output: string }>>(),
}));

vi.mock("./codeRevertApi", () => api);

import { CodeRevert, REVERT_CONFIRM, REVERT_EMPTY, REVERT_MERGE, REVERT_RULES } from "./CodeRevert";

const THREE = "c".repeat(40);
const COMMITS: CodeCommit[] = [
  { sha: THREE, short: "ccccccc", subject: "Fix the header", merge: false },
  { sha: "b".repeat(40), short: "bbbbbbb", subject: "Merge branch side", merge: true },
];

function plan(over: Partial<CodeRevertPlan> = {}): CodeRevertPlan {
  return {
    sha: THREE,
    short: "ccccccc",
    subject: "Fix the header",
    branch: "code/fix",
    revertCommand: `git revert --no-edit ${THREE}`,
    pushCommand: "git push --porcelain origin HEAD:refs/heads/code/fix",
    blocked: null,
    ...over,
  };
}

describe("Code tab → Roll back", () => {
  beforeEach(() => {
    api.codeRevertList.mockReset();
    api.codeRevertPlan.mockReset();
    api.codeRevertRun.mockReset();
    api.codeRevertList.mockResolvedValue(COMMITS);
    api.codeRevertPlan.mockResolvedValue(plan());
    api.codeRevertRun.mockResolvedValue({
      commit: "ddddddd",
      summary: "Reverted ccccccc as new commit ddddddd. Pushed code/fix to origin.",
      output: "",
    });
  });

  it("lists the commits with the rules, and a merge has no Revert", async () => {
    render(<CodeRevert />);
    expect(await screen.findByText("Fix the header")).toBeInTheDocument();
    expect(screen.getByText(REVERT_RULES)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Revert ccccccc" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Revert bbbbbbb" })).toBeNull();
    expect(screen.getByText(REVERT_MERGE)).toBeInTheDocument();
  });

  it("always asks first, showing both commands, and runs only on yes", async () => {
    const user = userEvent.setup();
    render(<CodeRevert />);
    await user.click(await screen.findByRole("button", { name: "Revert ccccccc" }));
    const card = await screen.findByRole("group", { name: REVERT_CONFIRM });
    expect(within(card).getByLabelText("Commands to run")).toHaveTextContent(
      `git revert --no-edit ${THREE} git push --porcelain origin HEAD:refs/heads/code/fix`,
    );
    expect(api.codeRevertRun).not.toHaveBeenCalled();
    await user.click(within(card).getByRole("button", { name: "Revert and push" }));
    expect(api.codeRevertRun).toHaveBeenCalledWith(plan());
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Reverted ccccccc as new commit ddddddd. Pushed code/fix to origin.",
    );
  });

  it("Cancel runs nothing", async () => {
    const user = userEvent.setup();
    render(<CodeRevert />);
    await user.click(await screen.findByRole("button", { name: "Revert ccccccc" }));
    const card = await screen.findByRole("group", { name: REVERT_CONFIRM });
    await user.click(within(card).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("group", { name: REVERT_CONFIRM })).toBeNull();
    expect(api.codeRevertRun).not.toHaveBeenCalled();
  });

  it("a blocked revert shows why and offers no confirm", async () => {
    const user = userEvent.setup();
    const why = "main is the main or default branch. The app never pushes to it; make a branch first.";
    api.codeRevertPlan.mockResolvedValue(plan({ blocked: why, branch: "main" }));
    render(<CodeRevert />);
    await user.click(await screen.findByRole("button", { name: "Revert ccccccc" }));
    expect(await screen.findByRole("status")).toHaveTextContent(why);
    expect(screen.queryByRole("group", { name: REVERT_CONFIRM })).toBeNull();
    expect(api.codeRevertRun).not.toHaveBeenCalled();
  });

  it("an empty branch says so", async () => {
    api.codeRevertList.mockResolvedValue([]);
    render(<CodeRevert />);
    expect(await screen.findByText(REVERT_EMPTY)).toBeInTheDocument();
  });
});

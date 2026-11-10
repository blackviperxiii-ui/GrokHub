import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeBuiltin, CodeDiff, CodeRepo } from "./api";

const api = vi.hoisted(() => ({
  codeRepo: vi.fn<() => Promise<CodeRepo>>(),
  codeDiff: vi.fn<(path: string) => Promise<CodeDiff>>(),
  codeBuiltins: vi.fn<() => Promise<CodeBuiltin[]>>(),
  codeTestPlan: vi.fn<(command?: string | null) => Promise<{ command: string; needsConfirm: boolean; remembered: string | null }>>(),
  codeTestRun: vi.fn<(command: string, confirmed: boolean) => Promise<{ command: string; exit: number; output: string; cut: boolean }>>(),
  onCodeRun: vi.fn(() => Promise.resolve(() => {})),
  codeShipPlan: vi.fn((action: "push" | "pr") =>
    Promise.resolve({
      action,
      branch: "beta",
      command: "git push --porcelain origin HEAD:refs/heads/beta",
      needsConfirm: true,
      blocked: null,
      mode: "ask",
      autoBase: "beta",
      appMade: false,
      prUrl: null,
    }),
  ),
  codeShipRun: vi.fn(),
  codeNewBranch: vi.fn(),
  codeActivity: vi.fn(() => Promise.resolve([])),
}));

vi.mock("./api", () => api);
vi.mock("./Chat", () => ({ newConversationId: () => "shell-new" }));

import { Code, CODE_BUILTINS, CODE_CLEAN, CODE_CONFIRM, CODE_NEXT, CODE_NOT_REPO, CODE_REVIEW_PROMPT, CODE_TESTS, overrideLabel } from "./Code";

function repo(over: Partial<CodeRepo> = {}): CodeRepo {
  return {
    workspace: "/work/demo",
    isRepo: true,
    branch: "beta",
    upstream: "origin/beta",
    ahead: 1,
    behind: 0,
    files: [
      { path: "src/a.rs", status: "modified", staged: false },
      { path: "notes.md", status: "untracked", staged: false },
    ],
    more: false,
    ...over,
  };
}

describe("Code tab", () => {
  beforeEach(() => {
    api.codeRepo.mockReset();
    api.codeDiff.mockReset();
    api.codeBuiltins.mockReset();
    api.codeBuiltins.mockResolvedValue([]);
    api.codeTestPlan.mockReset();
    api.codeTestRun.mockReset();
    api.onCodeRun.mockReset();
    api.onCodeRun.mockResolvedValue(() => {});
    api.codeTestPlan.mockResolvedValue({ command: "cargo test", needsConfirm: true, remembered: null });
    api.codeTestRun.mockResolvedValue({ command: "cargo test", exit: 0, output: "test ok\n", cut: false });
  });

  it("shows the branch and changed files, and opens a diff", async () => {
    api.codeRepo.mockResolvedValue(repo());
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+new\n", cut: false });
    render(<Code onTask={() => {}} />);
    expect(await screen.findByText(/On beta → origin\/beta · 1 ahead, 0 behind/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /src\/a\.rs/ }));
    expect(api.codeDiff).toHaveBeenCalledWith("src/a.rs");
    expect(await screen.findByLabelText("Diff of src/a.rs")).toHaveTextContent("+new");
  });

  it("says so when the workspace isn't a repo or has no changes", async () => {
    api.codeRepo.mockResolvedValueOnce(repo({ isRepo: false, branch: null, files: [] }));
    const { unmount } = render(<Code onTask={() => {}} />);
    expect(await screen.findByText(CODE_NOT_REPO)).toBeInTheDocument();
    unmount();
    api.codeRepo.mockResolvedValueOnce(repo({ files: [] }));
    render(<Code onTask={() => {}} />);
    expect(await screen.findByText(CODE_CLEAN)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Ask the agent to review" })).not.toBeInTheDocument();
  });

  it("hands a review task to the chat", async () => {
    api.codeRepo.mockResolvedValue(repo());
    const onTask = vi.fn();
    render(<Code onTask={onTask} />);
    await userEvent.click(await screen.findByRole("button", { name: "Ask the agent to review" }));
    expect(onTask).toHaveBeenCalledWith({ prompt: CODE_REVIEW_PROMPT, conversationId: "shell-new" });
  });

  it("shows a load error", async () => {
    api.codeRepo.mockRejectedValue("git isn't installed or couldn't start");
    render(<Code onTask={() => {}} />);
    expect(await screen.findByRole("status")).toHaveTextContent("git isn't installed");
  });

  const BUILTINS: CodeBuiltin[] = [
    {
      name: "qa-walkthrough",
      description: "Ultimate ship check.",
      body: "# QA Walkthrough\nWalk the app.",
      overriddenBy: null,
    },
    {
      name: "av-qa-walkthrough",
      description: "AV ship gate.",
      body: "# AV QA\nPlayback checks.",
      overriddenBy: null,
    },
    {
      name: "create-verification-skill",
      description: "Project skill that drives the real app.",
      body: "# Create verification skill",
      overriddenBy: null,
    },
    {
      name: "maintain-verification-skill",
      description: "Audit verify skill.",
      body: "# Maintain verification skill",
      overriddenBy: null,
    },
    {
      name: "follow-through",
      description: "Ledger, check-ins, stall detection, status updates; proven done and handoff. /follow-through",
      body: "# Follow Through\nLedger, check-ins.",
      overriddenBy: null,
    },
  ];

  it("shows the built-in skills on a clean repo, with no toggle, edit or Open folder", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeBuiltins.mockResolvedValue(BUILTINS);
    render(<Code onTask={() => {}} />);
    expect(await screen.findByText(CODE_CLEAN)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: CODE_BUILTINS })).toBeInTheDocument();
    const list = screen.getByRole("list", { name: CODE_BUILTINS });
    expect(list.querySelectorAll("li button").length).toBe(5);
    expect(screen.getByRole("button", { name: /^qa-walkthrough/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^follow-through/ })).toBeInTheDocument();
    const skills = screen.getByRole("list", { name: CODE_BUILTINS });
    expect(skills.querySelector('[role="switch"]')).toBeNull();
    expect(skills.querySelector("textarea, input, [contenteditable]")).toBeNull();
    expect(screen.queryByRole("button", { name: /open folder/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /edit/i })).not.toBeInTheDocument();
    expect(CODE_NEXT).not.toMatch(/skill/i);
    expect(CODE_NEXT).not.toMatch(/\btests\b/i);
  });

  it("opens a built-in and shows its body", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeBuiltins.mockResolvedValue(BUILTINS);
    render(<Code onTask={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: /^follow-through/ }));
    expect(await screen.findByLabelText("Built-in skill follow-through")).toHaveTextContent("# Follow Through");
    expect(screen.getByLabelText("Built-in skill follow-through")).toHaveTextContent("Ledger, check-ins.");
  });

  it("renders the overridden-by label on a shadowed built-in", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    const over = { scope: "project" as const, name: "follow-through" };
    api.codeBuiltins.mockResolvedValue(
      BUILTINS.map((row) => (row.name === "follow-through" ? { ...row, overriddenBy: over } : row)),
    );
    render(<Code onTask={() => {}} />);
    expect(await screen.findByText(overrideLabel(over))).toBeInTheDocument();
    expect(screen.getByText("overridden by project skill follow-through")).toBeInTheDocument();
  });

  it("shows the exact command on the first-run confirm card", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    render(<Code onTask={() => {}} />);
    expect(await screen.findByRole("heading", { name: CODE_TESTS })).toBeInTheDocument();
    expect(await screen.findByLabelText("Test command")).toHaveValue("cargo test");
    await userEvent.click(screen.getByRole("button", { name: "Run tests" }));
    expect(await screen.findByRole("group", { name: CODE_CONFIRM })).toBeInTheDocument();
    expect(screen.getByLabelText("Command to run")).toHaveTextContent("cargo test");
    expect(api.codeTestRun).not.toHaveBeenCalled();
  });

  it("runs after confirm and does not ask again for the same command", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeTestPlan
      .mockResolvedValueOnce({ command: "cargo test", needsConfirm: true, remembered: null })
      .mockResolvedValueOnce({ command: "cargo test", needsConfirm: true, remembered: null })
      .mockResolvedValueOnce({ command: "cargo test", needsConfirm: false, remembered: "cargo test" })
      .mockResolvedValueOnce({ command: "cargo test", needsConfirm: false, remembered: "cargo test" })
      .mockResolvedValue({ command: "cargo test", needsConfirm: false, remembered: "cargo test" });
    render(<Code onTask={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Run tests" }));
    const card = await screen.findByRole("group", { name: CODE_CONFIRM });
    await userEvent.click(card.querySelector("button")!);
    expect(api.codeTestRun).toHaveBeenCalledWith("cargo test", true);
    expect(await screen.findByLabelText("Test output")).toHaveTextContent("test ok");
    expect(screen.queryByRole("group", { name: CODE_CONFIRM })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Run tests" }));
    expect(api.codeTestRun).toHaveBeenCalledWith("cargo test", false);
    expect(screen.queryByRole("group", { name: CODE_CONFIRM })).not.toBeInTheDocument();
  });

  it("asks again when the command changes", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeTestPlan.mockResolvedValue({ command: "cargo test", needsConfirm: false, remembered: "cargo test" });
    render(<Code onTask={() => {}} />);
    const field = await screen.findByLabelText("Test command");
    await userEvent.clear(field);
    await userEvent.type(field, "cargo test -- --ignored");
    api.codeTestPlan.mockResolvedValue({
      command: "cargo test -- --ignored",
      needsConfirm: true,
      remembered: "cargo test",
    });
    await userEvent.click(screen.getByRole("button", { name: "Run tests" }));
    expect(await screen.findByLabelText("Command to run")).toHaveTextContent("cargo test -- --ignored");
    expect(api.codeTestRun).not.toHaveBeenCalled();
  });
});

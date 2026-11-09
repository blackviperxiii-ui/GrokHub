import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeDiff, CodeRepo } from "./api";

const api = vi.hoisted(() => ({
  codeRepo: vi.fn<() => Promise<CodeRepo>>(),
  codeDiff: vi.fn<(path: string) => Promise<CodeDiff>>(),
}));

vi.mock("./api", () => api);
vi.mock("./Chat", () => ({ newConversationId: () => "shell-new" }));

import { Code, CODE_CLEAN, CODE_NOT_REPO, CODE_REVIEW_PROMPT } from "./Code";

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
});

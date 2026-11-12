import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeBuiltin, CodeDeployPlan, CodeDiff, CodeFileListing, CodeRepo } from "./api";

const api = vi.hoisted(() => ({
  codeRepo: vi.fn<() => Promise<CodeRepo>>(),
  codeDiff: vi.fn<(path: string) => Promise<CodeDiff>>(),
  codeBuiltins: vi.fn<() => Promise<CodeBuiltin[]>>(),
  codeTestPlan: vi.fn<(command?: string | null) => Promise<{ command: string; needsConfirm: boolean; remembered: string | null }>>(),
  codeTestRun: vi.fn<(command: string, confirmed: boolean) => Promise<{ command: string; exit: number; output: string; cut: boolean }>>(),
  codeRunStop: vi.fn<() => Promise<boolean>>(),
  codeFilesList: vi.fn<(path: string) => Promise<CodeFileListing>>(),
  codeFileRead: vi.fn(),
  codeFileWrite: vi.fn(),
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
      skipsHook: false,
    }),
  ),
  codeShipRun: vi.fn(),
  codeNewBranch: vi.fn(),
  codeActivity: vi.fn(() => Promise.resolve([])),
  codeDeployPlan: vi.fn<() => Promise<CodeDeployPlan>>(),
  codeDeployRun: vi.fn<(command: string, confirmed: boolean) => Promise<{ command: string; exit: number; output: string; cut: boolean }>>(),
  onCodeDeployRun: vi.fn(() => Promise.resolve(() => {})),
  codeSessionBind: vi.fn<(conversationId: string) => Promise<string>>(),
  codeSessionLead: vi.fn<(conversationId: string) => Promise<boolean>>(),
}));

vi.mock("./api", () => api);
// The conversation pane has its own tests; here it shows what it was told and
// reports a session and a reply the way the real one does.
vi.mock("./Chat", async () => {
  const { useEffect, useState } = await import("react");
  let made = 0;
  interface StubProps {
    embedded?: boolean;
    lead?: boolean;
    active?: boolean;
    prefill?: { text: string; conversationId?: string };
    open?: { id: string };
    onConversation?: (id: string) => void;
    onBusy?: (busy: boolean) => void;
    onRan?: () => void;
    deleted?: { n: number };
  }
  function Chat({ embedded, lead, active, prefill, open, onConversation, onBusy, onRan, deleted }: StubProps) {
    const [busy, setBusy] = useState(false);
    const id = open?.id ?? prefill?.conversationId ?? "shell-first";
    useEffect(() => onConversation?.(id), [id, onConversation]);
    useEffect(() => onBusy?.(busy), [busy, onBusy]);
    return (
      <div data-testid="agent-chat" data-embedded={String(embedded)} data-lead={String(lead)} data-active={String(active)}>
        <span>session {id}</span>
        <span>box [{prefill?.text ?? ""}]</span>
        <span>deleted {deleted?.n ?? 0}</span>
        <button type="button" onClick={onRan}>
          stub slash
        </button>
        <button type="button" onClick={() => setBusy((now) => !now)}>
          stub reply
        </button>
      </div>
    );
  }
  return { Chat, newConversationId: () => `shell-new-${++made}` };
});
vi.mock("./Agents", () => ({
  Agents: (p: { conversationId?: string; active?: boolean; live?: boolean; onOpenChat?: () => void }) => (
    <div
      data-testid="agent-tasks"
      data-active={String(p.active)}
      data-live={String(p.live)}
      data-links={String(Boolean(p.onOpenChat))}
    >
      tasks of {p.conversationId}
    </div>
  ),
}));
// The terminal has its own tests; here it would only pull in its screen widget.
vi.mock("./CodeTerminal", () => ({ default: () => null }));
vi.mock("./CodeRevert", () => ({ CodeRevert: () => null }));
// The project bar has its own tests; here it is buttons that call back, and
// it shows what the tab tells it.
vi.mock("./CodeProjects", () => ({
  folderName: (path: string) => path.split("/").filter(Boolean).pop() ?? path,
  CodeProjects: (p: {
    onSwitched: () => void;
    onNewSession: () => void;
    onOpenSession: (id: string) => void;
    onFolder?: (folder: string) => void;
    agentBusy?: boolean;
    openSession?: string;
    refresh?: number;
  }) => (
    <div data-testid="bar" data-busy={String(p.agentBusy)} data-open={p.openSession} data-refresh={p.refresh}>
      <button type="button" onClick={p.onSwitched}>
        stub switch
      </button>
      <button type="button" onClick={p.onNewSession}>
        stub new session
      </button>
      <button type="button" onClick={() => p.onOpenSession("chat-7")}>
        stub open session
      </button>
      <button type="button" onClick={() => p.onFolder?.("/work/demo")}>
        stub folder demo
      </button>
      <button type="button" onClick={() => p.onFolder?.("/work/other")}>
        stub folder other
      </button>
    </div>
  ),
}));

import { AGENT_TITLE, Code, CODE_BUILTINS, CODE_CLEAN, CODE_CONFIRM, CODE_NOT_REPO, CODE_REVIEW_PROMPT, CODE_TESTS, overrideLabel } from "./Code";

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

/** The tab as the app holds it: mounted once, hidden while another tab shows. */
function Tab({ shown }: { shown: boolean }) {
  return (
    <Code shown={shown} onOpenSettings={() => {}} />
  );
}

/** Let every answer already on its way land before looking at the page. */
const settle = () => act(() => new Promise<void>((done) => setTimeout(done, 0)));

describe("Code tab", () => {
  beforeEach(() => {
    api.codeRepo.mockReset();
    api.codeSessionBind.mockReset();
    api.codeSessionBind.mockResolvedValue("/work/demo");
    api.codeSessionLead.mockReset();
    api.codeSessionLead.mockResolvedValue(false);
    api.codeDiff.mockReset();
    api.codeBuiltins.mockReset();
    api.codeBuiltins.mockResolvedValue([]);
    api.codeTestPlan.mockReset();
    api.codeTestRun.mockReset();
    api.onCodeRun.mockReset();
    api.onCodeRun.mockResolvedValue(() => {});
    api.codeTestPlan.mockResolvedValue({ command: "cargo test", needsConfirm: true, remembered: null });
    api.codeTestRun.mockResolvedValue({ command: "cargo test", exit: 0, output: "test ok\n", cut: false });
    api.codeRunStop.mockReset();
    api.codeRunStop.mockResolvedValue(true);
    api.codeFilesList.mockReset();
    api.codeFilesList.mockResolvedValue({ path: "", entries: [{ name: "README.md", path: "README.md", dir: false }], more: false });
    api.codeActivity.mockClear();
    api.codeDeployRun.mockReset();
    api.codeDeployPlan.mockReset();
    api.codeDeployPlan.mockResolvedValue({
      deployEnabled: false,
      command: "",
      needsConfirm: true,
      remembered: null,
      mode: "ask",
      blocked: "Deploy is off for this project. Turn it on in Settings → Code.",
    });
  });

  afterEach(() => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("shows the branch and changed files, and opens a diff", async () => {
    api.codeRepo.mockResolvedValue(repo());
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+new\n", cut: false });
    render(<Code shown onOpenSettings={() => {}} />);
    expect(await screen.findByText(/On beta → origin\/beta · 1 ahead, 0 behind/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /src\/a\.rs/ }));
    expect(api.codeDiff).toHaveBeenCalledWith("src/a.rs");
    expect(await screen.findByLabelText("Diff of src/a.rs")).toHaveTextContent("+new");
  });

  it("Refresh reads the open diff again", async () => {
    api.codeRepo.mockResolvedValue(repo());
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+first edit\n", cut: false });
    render(<Code shown onOpenSettings={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: /src\/a\.rs/ }));
    expect(await screen.findByLabelText("Diff of src/a.rs")).toHaveTextContent("+first edit");
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+second edit\n", cut: false });
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(screen.getByLabelText("Diff of src/a.rs")).toHaveTextContent("+second edit"));
    expect(api.codeDiff).toHaveBeenCalledTimes(2);
    expect(api.codeDiff).toHaveBeenLastCalledWith("src/a.rs");
  });

  it("coming back to the tab reads the open diff again", async () => {
    api.codeRepo.mockResolvedValue(repo());
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+first edit\n", cut: false });
    const view = render(<Tab shown />);
    await userEvent.click(await screen.findByRole("button", { name: /src\/a\.rs/ }));
    expect(await screen.findByLabelText("Diff of src/a.rs")).toHaveTextContent("+first edit");
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+second edit\n", cut: false });
    view.rerender(<Tab shown={false} />);
    view.rerender(<Tab shown />);
    await waitFor(() => expect(screen.getByLabelText("Diff of src/a.rs")).toHaveTextContent("+second edit"));
    expect(api.codeDiff).toHaveBeenCalledTimes(2);
  });

  it("coming back closes the diff of a file that is no longer changed", async () => {
    api.codeRepo.mockResolvedValue(repo());
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+first edit\n", cut: false });
    const view = render(<Tab shown />);
    await userEvent.click(await screen.findByRole("button", { name: /src\/a\.rs/ }));
    expect(await screen.findByLabelText("Diff of src/a.rs")).toHaveTextContent("+first edit");
    api.codeRepo.mockResolvedValue(repo({ files: [{ path: "notes.md", status: "untracked", staged: false }] }));
    view.rerender(<Tab shown={false} />);
    view.rerender(<Tab shown />);
    await waitFor(() => expect(screen.queryByRole("button", { name: /src\/a\.rs/ })).toBeNull());
    expect(screen.getByRole("button", { name: /notes\.md/ })).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByLabelText("Diff of src/a.rs")).toBeNull());
    expect(api.codeDiff).toHaveBeenCalledTimes(1);
  });

  it("a slow refresh does not replace the diff of a file picked after it", async () => {
    api.codeRepo.mockResolvedValue(repo());
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+a edit\n", cut: false });
    render(<Code shown onOpenSettings={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: /src\/a\.rs/ }));
    expect(await screen.findByLabelText("Diff of src/a.rs")).toHaveTextContent("+a edit");
    let answerSlow: (diff: CodeDiff) => void = () => {};
    api.codeDiff.mockImplementationOnce(() => new Promise<CodeDiff>((done) => (answerSlow = done)));
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(api.codeDiff).toHaveBeenCalledTimes(2));
    api.codeDiff.mockResolvedValue({ path: "notes.md", text: "+a note\n", cut: false });
    await userEvent.click(screen.getByRole("button", { name: /notes\.md/ }));
    expect(await screen.findByLabelText("Diff of notes.md")).toHaveTextContent("+a note");
    answerSlow({ path: "src/a.rs", text: "-old\n+late answer\n", cut: false });
    await waitFor(() => expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled());
    expect(screen.getByLabelText("Diff of notes.md")).toHaveTextContent("+a note");
    expect(screen.queryByLabelText("Diff of src/a.rs")).toBeNull();
  });

  it("after a file fails to open, Refresh reads the diff on screen, not a file picked on the way", async () => {
    const files = [
      { path: "c.txt", status: "modified" as const, staged: false },
      { path: "src/a.rs", status: "modified" as const, staged: false },
      { path: "notes.md", status: "untracked" as const, staged: false },
    ];
    api.codeRepo.mockResolvedValue(repo({ files }));
    api.codeDiff.mockResolvedValue({ path: "c.txt", text: "+c edit\n", cut: false });
    render(<Code shown onOpenSettings={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: /c\.txt/ }));
    expect(await screen.findByLabelText("Diff of c.txt")).toHaveTextContent("+c edit");

    let answerSlow: (diff: CodeDiff) => void = () => {};
    api.codeDiff.mockImplementationOnce(() => new Promise<CodeDiff>((done) => (answerSlow = done)));
    await userEvent.click(screen.getByRole("button", { name: /src\/a\.rs/ }));
    api.codeDiff.mockRejectedValueOnce("git diff failed");
    await userEvent.click(screen.getByRole("button", { name: /notes\.md/ }));
    expect(await screen.findByText("git diff failed")).toBeInTheDocument();
    answerSlow({ path: "src/a.rs", text: "-old\n+a edit\n", cut: false });
    await settle();
    expect(screen.getByLabelText("Diff of c.txt")).toHaveTextContent("+c edit");

    api.codeDiff.mockImplementation((path) => Promise.resolve({ path, text: `+second edit of ${path}\n`, cut: false }));
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled());
    expect(screen.getByLabelText("Diff of c.txt")).toHaveTextContent("+second edit of c.txt");
    expect(screen.queryByLabelText("Diff of src/a.rs")).toBeNull();
    expect(api.codeDiff.mock.calls.map(([path]) => path)).toEqual(["c.txt", "src/a.rs", "notes.md", "c.txt"]);
  });

  it("a file that fails to open does not drop a Refresh of the diff on screen", async () => {
    api.codeRepo.mockResolvedValue(repo());
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+first\n", cut: false });
    render(<Code shown onOpenSettings={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: /src\/a\.rs/ }));
    expect(await screen.findByLabelText("Diff of src/a.rs")).toHaveTextContent("+first");

    let answerSlow: (diff: CodeDiff) => void = () => {};
    api.codeDiff.mockImplementationOnce(() => new Promise<CodeDiff>((done) => (answerSlow = done)));
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(api.codeDiff).toHaveBeenCalledTimes(2));
    api.codeDiff.mockRejectedValueOnce("git diff failed");
    await userEvent.click(screen.getByRole("button", { name: /notes\.md/ }));
    expect(await screen.findByText("git diff failed")).toBeInTheDocument();
    answerSlow({ path: "src/a.rs", text: "-old\n+second\n", cut: false });
    await waitFor(() => expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled());
    expect(screen.getByLabelText("Diff of src/a.rs")).toHaveTextContent("+second");
    expect(api.codeDiff.mock.calls.map(([path]) => path)).toEqual(["src/a.rs", "src/a.rs", "notes.md"]);
  });

  it("a file picked just before Refresh still opens", async () => {
    api.codeRepo.mockResolvedValue(repo());
    let answerSlow: (diff: CodeDiff) => void = () => {};
    api.codeDiff.mockImplementationOnce(() => new Promise<CodeDiff>((done) => (answerSlow = done)));
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+new\n", cut: false });
    render(<Code shown onOpenSettings={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: /src\/a\.rs/ }));
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled());
    answerSlow({ path: "src/a.rs", text: "-old\n+new\n", cut: false });
    expect(await screen.findByLabelText("Diff of src/a.rs")).toHaveTextContent("+new");
    expect(screen.getByRole("button", { name: /src\/a\.rs/ })).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps a typed test command when the tab shows again, and runs it", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeTestPlan.mockImplementation((typed) =>
      Promise.resolve({ command: typed ?? "cargo test", needsConfirm: false, remembered: "cargo test" }),
    );
    const view = render(<Tab shown />);
    const box = await screen.findByLabelText("Test command");
    await waitFor(() => expect(box).toHaveValue("cargo test"));
    await userEvent.clear(box);
    await userEvent.type(box, "cargo test -p grokhub-core --lib perm");
    view.rerender(<Tab shown={false} />);
    view.rerender(<Tab shown />);
    await waitFor(() => expect(api.codeTestPlan).toHaveBeenCalledTimes(2));
    await settle();
    expect(screen.getByLabelText("Test command")).toHaveValue("cargo test -p grokhub-core --lib perm");
    await userEvent.click(screen.getByRole("button", { name: "Run tests" }));
    await waitFor(() => expect(api.codeTestRun).toHaveBeenCalledTimes(1));
    expect(api.codeTestPlan).toHaveBeenCalledWith("cargo test -p grokhub-core --lib perm");
    expect(api.codeTestRun).toHaveBeenCalledWith("cargo test -p grokhub-core --lib perm", false);
  });

  it("an untouched test command takes the fresh one when the tab shows again", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeTestPlan.mockResolvedValueOnce({ command: "cargo test", needsConfirm: true, remembered: null });
    api.codeTestPlan.mockResolvedValue({ command: "npm test", needsConfirm: false, remembered: "npm test" });
    const view = render(<Tab shown />);
    await waitFor(() => expect(screen.getByLabelText("Test command")).toHaveValue("cargo test"));
    view.rerender(<Tab shown={false} />);
    view.rerender(<Tab shown />);
    await waitFor(() => expect(screen.getByLabelText("Test command")).toHaveValue("npm test"));
    expect(screen.queryByText("The first run of this command will ask before it starts.")).toBeNull();
  });

  it("says so when the workspace isn't a repo or has no changes", async () => {
    api.codeRepo.mockResolvedValueOnce(repo({ isRepo: false, branch: null, files: [] }));
    const { unmount } = render(<Code shown onOpenSettings={() => {}} />);
    expect(await screen.findByText(CODE_NOT_REPO)).toBeInTheDocument();
    unmount();
    api.codeRepo.mockResolvedValueOnce(repo({ files: [] }));
    render(<Code shown onOpenSettings={() => {}} />);
    expect(await screen.findByText(CODE_CLEAN)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Ask the agent to review" })).not.toBeInTheDocument();
  });

  it("puts a review task in a new session of the agent window, on the same tab", async () => {
    api.codeRepo.mockResolvedValue(repo());
    render(<Code shown onOpenSettings={() => {}} />);
    const chat = screen.getByTestId("agent-chat");
    expect(chat).toHaveTextContent("session shell-first");
    await userEvent.click(await screen.findByRole("button", { name: "Ask the agent to review" }));
    await waitFor(() => expect(chat).toHaveTextContent(`box [${CODE_REVIEW_PROMPT}]`));
    expect(chat).not.toHaveTextContent("session shell-first");
    expect(chat).toHaveTextContent(/session shell-new-\d+/);
  });

  it("shows a load error", async () => {
    api.codeRepo.mockRejectedValue("git isn't installed or couldn't start");
    render(<Code shown onOpenSettings={() => {}} />);
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
    render(<Code shown onOpenSettings={() => {}} />);
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
  });

  it("opens a built-in and shows its body", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeBuiltins.mockResolvedValue(BUILTINS);
    render(<Code shown onOpenSettings={() => {}} />);
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
    render(<Code shown onOpenSettings={() => {}} />);
    expect(await screen.findByText(overrideLabel(over))).toBeInTheDocument();
    expect(screen.getByText("overridden by project skill follow-through")).toBeInTheDocument();
  });

  it("shows the exact command on the first-run confirm card", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    render(<Code shown onOpenSettings={() => {}} />);
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
    render(<Code shown onOpenSettings={() => {}} />);
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
    render(<Code shown onOpenSettings={() => {}} />);
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

  it("draws a changed file as a quiet row, and its diff as a region the keyboard can scroll", async () => {
    api.codeRepo.mockResolvedValue(repo());
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+new\n", cut: false });
    render(<Code shown onOpenSettings={() => {}} />);
    const row = await screen.findByRole("button", { name: /src\/a\.rs/ });
    expect(row).toHaveClass("code-file", "quiet", "modified");
    await userEvent.click(row);
    const diff = await screen.findByRole("region", { name: "Diff of src/a.rs" });
    expect(diff).toHaveAttribute("tabindex", "0");
    expect(diff).toHaveTextContent("+new");
  });

  it("marks the changed-files list as the one that scrolls beside the diff", async () => {
    api.codeRepo.mockResolvedValue(repo());
    render(<Code shown onOpenSettings={() => {}} />);
    expect(await screen.findByRole("list", { name: "Changed files" })).toHaveClass("rows", "code-changed");
  });

  it("gives the test output and a skill's body a role and a place in the tab order", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeBuiltins.mockResolvedValue([{ name: "follow-through", description: "Ledger.", body: "# Follow Through", overriddenBy: null }]);
    api.codeTestPlan.mockResolvedValue({ command: "cargo test", needsConfirm: false, remembered: "cargo test" });
    render(<Code shown onOpenSettings={() => {}} />);
    await waitFor(() => expect(screen.getByLabelText("Test command")).toHaveValue("cargo test"));
    await userEvent.click(screen.getByRole("button", { name: "Run tests" }));
    const output = await screen.findByRole("log", { name: "Test output" });
    expect(output).toHaveAttribute("tabindex", "0");
    expect(output).toHaveTextContent("test ok");
    await userEvent.click(screen.getByRole("button", { name: /^follow-through/ }));
    expect(screen.getByRole("region", { name: "Built-in skill follow-through" })).toHaveAttribute("tabindex", "0");
  });

  it("moves focus to Cancel when the test confirm card appears", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    render(<Code shown onOpenSettings={() => {}} />);
    await waitFor(() => expect(screen.getByLabelText("Test command")).toHaveValue("cargo test"));
    await userEvent.click(screen.getByRole("button", { name: "Run tests" }));
    const card = await screen.findByRole("group", { name: CODE_CONFIRM });
    expect(document.activeElement).toBe(within(card).getByRole("button", { name: "Cancel" }));
  });

  it("a running test shows Stop beside it, and Stop ends the run", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeTestPlan.mockResolvedValue({ command: "cargo test", needsConfirm: false, remembered: "cargo test" });
    let finish: (result: { command: string; exit: number; output: string; cut: boolean }) => void = () => {};
    api.codeTestRun.mockImplementationOnce(() => new Promise((done) => (finish = done)));
    render(<Code shown onOpenSettings={() => {}} />);
    await waitFor(() => expect(screen.getByLabelText("Test command")).toHaveValue("cargo test"));
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Run tests" }));
    const running = await screen.findByRole("button", { name: "Running…" });
    expect(running).toBeDisabled();
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(running.nextElementSibling).toBe(stop);
    await userEvent.click(stop);
    expect(api.codeRunStop).toHaveBeenCalledTimes(1);
    expect(api.codeRunStop).toHaveBeenCalledWith();
    await act(async () => finish({ command: "cargo test", exit: -1, output: "running 12 tests\n", cut: false }));
    expect(await screen.findByText("Tests stopped.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
    expect(screen.getByRole("button", { name: "Run tests" })).toBeEnabled();
    expect(screen.getByRole("log", { name: "Test output" })).toHaveTextContent("running 12 tests");
  });

  it("says why a run could not be stopped", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeTestPlan.mockResolvedValue({ command: "cargo test", needsConfirm: false, remembered: "cargo test" });
    api.codeTestRun.mockImplementationOnce(() => new Promise(() => {}));
    api.codeRunStop.mockRejectedValue("The run could not be stopped.");
    render(<Code shown onOpenSettings={() => {}} />);
    await waitFor(() => expect(screen.getByLabelText("Test command")).toHaveValue("cargo test"));
    await userEvent.click(screen.getByRole("button", { name: "Run tests" }));
    await userEvent.click(await screen.findByRole("button", { name: "Stop" }));
    expect(await screen.findByRole("status")).toHaveTextContent("The run could not be stopped.");
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
  });

  /** Start a run whose answer the test gives by hand, and wait for its Stop button. */
  async function startHeldRun() {
    let finish: (result: { command: string; exit: number; output: string; cut: boolean }) => void = () => {};
    api.codeTestRun.mockImplementationOnce(() => new Promise((done) => (finish = done)));
    await userEvent.click(screen.getByRole("button", { name: "Run tests" }));
    const stop = await screen.findByRole("button", { name: "Stop" });
    return { stop, finish: (exit: number, output: string) => act(async () => finish({ command: "cargo test", exit, output, cut: false })) };
  }

  /** The Tests section with a command that runs without asking. */
  async function testsReady() {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeTestPlan.mockResolvedValue({ command: "cargo test", needsConfirm: false, remembered: "cargo test" });
    render(<Code shown onOpenSettings={() => {}} />);
    await waitFor(() => expect(screen.getByLabelText("Test command")).toHaveValue("cargo test"));
  }

  it("a Stop that stopped nothing leaves the run's own exit code on screen", async () => {
    await testsReady();
    // Nothing was running any more, or the kill itself failed.
    api.codeRunStop.mockResolvedValue(false);
    const run = await startHeldRun();
    await userEvent.click(run.stop);
    expect(api.codeRunStop).toHaveBeenCalledTimes(1);
    await run.finish(101, "test result: FAILED. 3 failed\n");
    expect(await screen.findByRole("status")).toHaveTextContent(/^Tests exited with 101\.$/);
    expect(screen.getByRole("log", { name: "Test output" })).toHaveTextContent("test result: FAILED. 3 failed");
  });

  it("a run that fails after an earlier run was stopped shows its own exit code", async () => {
    await testsReady();
    const first = await startHeldRun();
    await userEvent.click(first.stop);
    await first.finish(-1, "running 12 tests\n");
    expect(await screen.findByRole("status")).toHaveTextContent(/^Tests stopped\.$/);

    const second = await startHeldRun();
    await second.finish(1, "test result: FAILED. 1 failed\n");
    expect(await screen.findByRole("status")).toHaveTextContent(/^Tests exited with 1\.$/);
    expect(api.codeRunStop).toHaveBeenCalledTimes(1);
  });

  it("a run that passes just as Stop is pressed is reported as passed", async () => {
    await testsReady();
    const run = await startHeldRun();
    await userEvent.click(run.stop);
    await run.finish(0, "test result: ok. 12 passed\n");
    expect(await screen.findByRole("status")).toHaveTextContent(/^Tests passed\.$/);
  });

  it("a Stop that failed does not label the run's later failure as stopped", async () => {
    await testsReady();
    api.codeRunStop.mockRejectedValue("The run could not be stopped.");
    const run = await startHeldRun();
    await userEvent.click(run.stop);
    expect(await screen.findByRole("status")).toHaveTextContent(/^The run could not be stopped\.$/);
    await run.finish(1, "test result: FAILED. 1 failed\n");
    expect(await screen.findByRole("status")).toHaveTextContent(/^Tests exited with 1\.$/);
  });

  it("a second Stop that finds nothing left to stop does not undo the first", async () => {
    await testsReady();
    const run = await startHeldRun();
    await userEvent.click(run.stop);
    // The run is on its way out: the desktop has nothing left to kill.
    api.codeRunStop.mockResolvedValueOnce(false);
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    api.codeRunStop.mockRejectedValueOnce("The run could not be stopped.");
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.codeRunStop).toHaveBeenCalledTimes(3);
    await run.finish(-1, "running 12 tests\n");
    expect(await screen.findByRole("status")).toHaveTextContent(/^Tests stopped\.$/);
  });

  it("a deploy that finishes reads the activity list again and keeps the Push and PR section as it was", async () => {
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    api.codeDeployPlan.mockResolvedValue({
      deployEnabled: true,
      command: "npm run deploy",
      needsConfirm: false,
      remembered: "npm run deploy",
      mode: "autoBeta",
      blocked: null,
    });
    api.codeDeployRun.mockResolvedValue({ command: "npm run deploy", exit: 0, output: "done\n", cut: false });
    render(<Code shown onOpenSettings={() => {}} />);
    await userEvent.type(await screen.findByLabelText("New branch"), "code/keep-me");
    await waitFor(() => expect(screen.getByRole("button", { name: "Deploy" })).toBeEnabled());
    expect(api.codeActivity).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Deploy" }));
    expect(await screen.findByText("Deploy finished.")).toBeInTheDocument();
    await waitFor(() => expect(api.codeActivity).toHaveBeenCalledTimes(2));
    await settle();
    expect(screen.getByLabelText("New branch")).toHaveValue("code/keep-me");
  });

  it("Refresh lists the editor's folder again", async () => {
    Object.assign(window, { __TAURI_INTERNALS__: {} });
    api.codeRepo.mockResolvedValue(repo({ files: [] }));
    render(<Code shown onOpenSettings={() => {}} />);
    expect(await screen.findByRole("button", { name: "README.md" })).toBeInTheDocument();
    expect(api.codeFilesList.mock.calls).toEqual([[""]]);
    // A file made outside the app.
    api.codeFilesList.mockResolvedValue({
      path: "",
      entries: ["README.md", "made-outside.txt"].map((name) => ({ name, path: name, dir: false })),
      more: false,
    });
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByRole("button", { name: "made-outside.txt" })).toBeInTheDocument();
    expect(api.codeFilesList.mock.calls).toEqual([[""], [""]]);
  });

  it("a project switch starts the workspace afresh in the new folder", async () => {
    api.codeRepo.mockResolvedValueOnce(repo());
    api.codeDiff.mockResolvedValue({ path: "src/a.rs", text: "-old\n+new\n", cut: false });
    api.codeTestPlan.mockResolvedValueOnce({ command: "cargo test", needsConfirm: true, remembered: null });
    render(<Code shown onOpenSettings={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: /src\/a\.rs/ }));
    expect(await screen.findByLabelText("Diff of src/a.rs")).toBeInTheDocument();
    expect(screen.getByLabelText("Test command")).toHaveValue("cargo test");

    api.codeRepo.mockResolvedValue(repo({ branch: "main", upstream: null, ahead: 0, files: [{ path: "lib/b.ts", status: "added", staged: true }] }));
    api.codeTestPlan.mockResolvedValue({ command: "npm test", needsConfirm: true, remembered: null });
    await userEvent.click(screen.getByRole("button", { name: "stub switch" }));
    expect(await screen.findByRole("button", { name: /lib\/b\.ts/ })).toBeInTheDocument();
    expect(screen.getByText("On main")).toBeInTheDocument();
    // Nothing of the old project stays: its file, its open diff, its test command.
    expect(screen.queryByRole("button", { name: /src\/a\.rs/ })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Diff of src/a.rs")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText("Test command")).toHaveValue("npm test"));
  });

  it("New session starts a fresh chat and a session row opens its saved chat", async () => {
    api.codeRepo.mockResolvedValue(repo());
    render(<Code shown onOpenSettings={() => {}} />);
    const chat = screen.getByTestId("agent-chat");
    await userEvent.click(screen.getByRole("button", { name: "stub new session" }));
    await waitFor(() => expect(chat).toHaveTextContent(/session shell-new-\d+/));
    expect(chat).toHaveTextContent("box []");
    await userEvent.click(screen.getByRole("button", { name: "stub open session" }));
    await waitFor(() => expect(chat).toHaveTextContent("session chat-7"));
    expect(screen.getByTestId("bar")).toHaveAttribute("data-open", "chat-7");
    expect(screen.getByTestId("agent-tasks")).toHaveTextContent("tasks of chat-7");
  });

  it("gives a session its folder before the agent window opens it", async () => {
    api.codeRepo.mockResolvedValue(repo());
    let bound: (dir: string) => void = () => {};
    render(<Code shown onOpenSettings={() => {}} />);
    const chat = screen.getByTestId("agent-chat");
    api.codeSessionBind.mockImplementation(() => new Promise<string>((done) => (bound = done)));
    await userEvent.click(screen.getByRole("button", { name: "stub open session" }));
    expect(api.codeSessionBind).toHaveBeenLastCalledWith("chat-7");
    // Until the folder is set the saved chat is not loaded.
    expect(chat).toHaveTextContent("session shell-first");
    await act(async () => bound("/work/demo"));
    expect(chat).toHaveTextContent("session chat-7");
  });

  it("sends a session the Chat tab has open to the Chat tab, and tells the app its own session", async () => {
    api.codeRepo.mockResolvedValue(repo());
    const onShowChat = vi.fn();
    const onSession = vi.fn();
    render(<Code shown onOpenSettings={() => {}} chatSession="chat-7" onShowChat={onShowChat} onSession={onSession} />);
    expect(onSession).toHaveBeenLastCalledWith("shell-first");
    await userEvent.click(screen.getByRole("button", { name: "stub open session" }));
    expect(onShowChat).toHaveBeenCalledTimes(1);
    await settle();
    expect(screen.getByTestId("agent-chat")).toHaveTextContent("session shell-first");
    expect(api.codeSessionBind).not.toHaveBeenCalledWith("chat-7");
  });

  it("passes History's deletes to the agent window, and reads the project again after a typed command", async () => {
    api.codeRepo.mockResolvedValue(repo());
    render(<Code shown onOpenSettings={() => {}} deleted={{ id: "chat-7", ids: ["chat-7"], n: 3 }} />);
    expect(screen.getByTestId("agent-chat")).toHaveTextContent("deleted 3");
    const bar = screen.getByTestId("bar");
    const before = Number(bar.getAttribute("data-refresh"));
    await userEvent.click(screen.getByRole("button", { name: "stub slash" }));
    expect(Number(bar.getAttribute("data-refresh"))).toBe(before + 1);
  });

  it("shows the agent window beside the workspace, with the folder its session works in", async () => {
    api.codeRepo.mockResolvedValue(repo());
    render(<Code shown onOpenSettings={() => {}} />);
    const pane = screen.getByRole("complementary", { name: "Agent window" });
    expect(within(pane).getByRole("heading", { name: AGENT_TITLE })).toBeInTheDocument();
    expect(within(pane).getByTestId("agent-chat")).toHaveAttribute("data-embedded", "true");
    expect(api.codeSessionBind).toHaveBeenCalledWith("shell-first");
    expect(await within(pane).findByText("Works in demo")).toHaveAttribute("title", "/work/demo");
    // The Tasks panel follows the same session and has no link out of the tab.
    const tasks = within(pane).getByTestId("agent-tasks");
    expect(tasks).toHaveTextContent("tasks of shell-first");
    expect(tasks).toHaveAttribute("data-links", "false");
  });

  it("holds the project and the sessions while the agent replies, then reads the sessions again", async () => {
    api.codeRepo.mockResolvedValue(repo());
    render(<Code shown onOpenSettings={() => {}} />);
    const bar = screen.getByTestId("bar");
    expect(bar).toHaveAttribute("data-busy", "false");
    const before = Number(bar.getAttribute("data-refresh"));
    await userEvent.click(screen.getByRole("button", { name: "stub reply" }));
    expect(bar).toHaveAttribute("data-busy", "true");
    expect(await screen.findByRole("button", { name: "Ask the agent to review" })).toBeDisabled();
    expect(Number(bar.getAttribute("data-refresh"))).toBe(before);
    await userEvent.click(screen.getByRole("button", { name: "stub reply" }));
    expect(bar).toHaveAttribute("data-busy", "false");
    expect(Number(bar.getAttribute("data-refresh"))).toBe(before + 1);
    expect(screen.getByRole("button", { name: "Ask the agent to review" })).toBeEnabled();
  });

  it("starts a session in the new folder when the app's folder changes, but not under a running reply", async () => {
    api.codeRepo.mockResolvedValue(repo());
    render(<Code shown onOpenSettings={() => {}} />);
    const chat = screen.getByTestId("agent-chat");
    // The first read of the folder is where the tab starts, not a change.
    await userEvent.click(screen.getByRole("button", { name: "stub folder demo" }));
    expect(chat).toHaveTextContent("session shell-first");
    await userEvent.click(screen.getByRole("button", { name: "stub reply" }));
    await userEvent.click(screen.getByRole("button", { name: "stub folder other" }));
    expect(chat).toHaveTextContent("session shell-first");
    await userEvent.click(screen.getByRole("button", { name: "stub reply" }));
    api.codeSessionBind.mockResolvedValue("/work/demo");
    await userEvent.click(screen.getByRole("button", { name: "stub folder demo" }));
    await waitFor(() => expect(chat).not.toHaveTextContent("session shell-first"));
    expect(await screen.findByText("Works in demo")).toBeInTheDocument();
  });

  it("tells the agent window whether its session has a lead, after the session has its folder", async () => {
    api.codeRepo.mockResolvedValue(repo());
    api.codeSessionLead.mockImplementation((id) => Promise.resolve(id === "shell-first"));
    render(<Tab shown />);
    const chat = screen.getByTestId("agent-chat");
    const tasks = screen.getByTestId("agent-tasks");
    // Until the answer is in, the Tasks panel keeps its timed read.
    expect(tasks).toHaveAttribute("data-live", "false");
    await waitFor(() => expect(chat).toHaveAttribute("data-lead", "true"));
    expect(tasks).toHaveAttribute("data-live", "true");
    expect(api.codeSessionLead).toHaveBeenLastCalledWith("shell-first");
    expect(api.codeSessionBind.mock.invocationCallOrder[0]).toBeLessThan(
      api.codeSessionLead.mock.invocationCallOrder[0],
    );
    // A saved chat opened here has none.
    await userEvent.click(screen.getByRole("button", { name: "stub open session" }));
    await waitFor(() => expect(chat).toHaveTextContent("session chat-7"));
    await waitFor(() => expect(api.codeSessionLead).toHaveBeenLastCalledWith("chat-7"));
    expect(chat).toHaveAttribute("data-lead", "false");
    expect(tasks).toHaveAttribute("data-live", "false");
  });

  it("keeps the agent window mounted and idle while another tab shows", async () => {
    api.codeRepo.mockResolvedValue(repo());
    const view = render(<Tab shown />);
    await userEvent.click(screen.getByRole("button", { name: "stub open session" }));
    await waitFor(() => expect(screen.getByTestId("agent-chat")).toHaveTextContent("session chat-7"));
    view.rerender(<Tab shown={false} />);
    const chat = screen.getByTestId("agent-chat");
    expect(chat).toHaveAttribute("data-active", "false");
    expect(chat).toHaveTextContent("session chat-7");
    expect(screen.getByTestId("agent-tasks")).toHaveAttribute("data-active", "false");
    expect(chat.closest(".code-tab")).toHaveAttribute("hidden");
  });
});

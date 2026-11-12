import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeTestPlan, CodeTestResult, HelperWork, Queue, Review, ShipStatus, TaskDetail } from "./api";

const api = vi.hoisted(() => ({
  getQueue: vi.fn<(conversationId: string) => Promise<Queue>>(),
  stopTask: vi.fn<(conversationId: string, taskId?: string) => Promise<Queue>>(),
  onTasksChanged: vi.fn<(handler: (conversationId: string) => void) => Promise<() => void>>(),
  onEventGap: vi.fn<(handler: () => void) => Promise<() => void>>(),
  getTaskDetail: vi.fn<(conversationId: string, taskId: string) => Promise<TaskDetail>>(),
  mergeTaskWork: vi.fn<(conversationId: string, key: string) => Promise<Queue>>(),
  resumeTaskWork: vi.fn<(conversationId: string, key: string, finish: boolean) => Promise<Queue>>(),
  discardTaskWork: vi.fn<(conversationId: string, key: string) => Promise<Queue>>(),
  shipStatus: vi.fn<(conversationId: string) => Promise<ShipStatus>>(),
  codeTestPlan: vi.fn<(command?: string | null, conversationId?: string) => Promise<CodeTestPlan>>(),
  codeTestRun: vi.fn<(command: string, confirmed: boolean, conversationId?: string) => Promise<CodeTestResult>>(),
}));
/** Whether the page runs in the desktop app; a paired browser does not. */
const shell = vi.hoisted(() => ({ desktop: true }));
// Apart from `api`, whose every value beforeEach resets: what the panel
// subscribed with, and the spies that stand for its unlisten calls.
const feed = vi.hoisted(() => ({
  changed: (() => {}) as (conversationId: string) => void,
  gap: (() => {}) as () => void,
  unlisten: vi.fn<() => void>(),
  unlistenGap: vi.fn<() => void>(),
}));

vi.mock("./api", () => api);
vi.mock("./transport", async (original) => ({
  ...(await original<typeof import("./transport")>()),
  inShell: () => shell.desktop,
}));

import {
  Agents,
  AGENTS_EMPTY,
  AGENTS_NO_CHAT,
  CHECKS_BLOCKED,
  CHECKS_NOTE,
  CHECKS_PASS,
  FIX_ASK,
  LEAD_LABELS,
  LIVE_FALLBACK_MS,
  mergeOffer,
  NOT_REVIEWED,
  REVIEW_ASK,
  REVIEW_DESKTOP_ONLY,
  REVIEW_STALE,
  reviewLine,
  secretLine,
  taskProgress,
  TASKS_EMPTY,
  TASKS_NOTE,
  TESTS_CHANGED_FILES,
  testsLine,
  WORK_DESKTOP_ONLY,
  WORK_NOTE,
  workLine,
} from "./Agents";

const QUEUE: Queue = {
  tasks: [
    { id: "t1", title: "cargo test --workspace", status: "running", live: true },
    { id: "s2", title: "failed to fetch the docs", status: "failed", live: true },
    { id: "t3", title: "npm run build", status: "failed", live: false },
    { id: "m4", title: "Monitor: build log", status: "done", live: false },
  ],
  todos: [
    { id: "a", content: "Read the spec", status: "completed", state: "done" },
    { id: "b", content: "Write the page", status: "in_progress", state: "running", owner: "s2" },
  ],
};

/** The same queue, with helper s2 three steps into its task. */
const PROGRESS_QUEUE: Queue = {
  ...QUEUE,
  tasks: QUEUE.tasks.map((task) =>
    task.id === "s2" ? { ...task, progress: { steps: 3, lastStep: "read_file", tokens: 1234, seconds: 125 } } : task,
  ),
};

/** A finished helper's work, sealed and waiting: 1 new, 1 changed, 1 deleted. */
function work(over: Partial<HelperWork> = {}): HelperWork {
  return {
    key: "k1-aaaa",
    task: "s1",
    title: "task-1: Add the parser",
    state: "ready",
    partial: false,
    ended: "it finished",
    files: ["src/a.rs", "src/b.rs", "old.rs"],
    counts: { added: 1, changed: 1, deleted: 1 },
    ignored: [],
    conflicts: [],
    skipped: [],
    path: "/home/user/.config/GrokHub/helper-work/chat/k1-aaaa",
    treeKept: true,
    landing: false,
    live: false,
    ...over,
  };
}

/** A helper that was stopped after it wrote two files. */
const PARTIAL = work({
  key: "k2-bbbb",
  title: "task-2: Add the page",
  state: "held",
  why: "partial",
  whyText: "the helper did not finish",
  partial: true,
  ended: "it was stopped",
  files: ["ui/page.tsx", "ui/old.tsx"],
  counts: { added: 1, changed: 0, deleted: 1 },
  path: "/home/user/.config/GrokHub/helper-work/chat/k2-bbbb",
});

/** Merged work whose worktree is kept for the ignored files in it. */
const KEPT = work({
  key: "k3-cccc",
  title: "task-3: Build it",
  state: "merged",
  ignored: ["target/", ".env.local"],
  path: "/home/user/.config/GrokHub/helper-work/chat/k3-cccc",
});

/** A merge that stopped part way. */
const STOPPED_MERGE = work({
  state: "held",
  why: "applyFailed",
  whyText: "the merge stopped part way",
  landing: true,
});

/** A review with one blocker and two notes, as the queue lists it. */
const REVIEW: Review = {
  findings: [
    { severity: "high", text: "src/a.rs:3 the total is off by one" },
    { severity: "medium", text: "src/b.rs:9 the name is unclear" },
    { severity: "info", text: "README:1 reads well" },
  ],
  blockers: 1,
  atMs: 1_700_000_000_000,
  planGiven: true,
};

/** A lead session whose one task is done. */
const LEAD_QUEUE: Queue = {
  tasks: [],
  todos: [{ id: "task-1", content: "Add the parser", status: "completed", state: "done" }],
};

/** A change with nothing in the way: reviewed clean, tested, on a work branch. */
const SHIP_READY: ShipStatus = {
  blocked: [],
  checks: {
    branch: "work/one",
    review: { state: "clean" },
    tests: { state: "passed", command: "cargo test" },
    secrets: [],
  },
};

/** The same change before its tests ran, with a blocker and two flagged places. */
const SHIP_BLOCKED: ShipStatus = {
  blocked: [
    "The review found 1 blocker.",
    "The tests have not run on this change.",
    "The secret scan flagged 2 places in the change.",
  ],
  checks: {
    branch: "work/one",
    review: { state: "blockers", count: 1 },
    tests: { state: "notRun", command: "cargo test" },
    secrets: [
      { path: "src/client.ts", line: 12, kind: "shape", commit: null },
      { path: ".env", line: null, kind: "name", commit: "0123456789abcdef0123456789abcdef01234567" },
    ],
  },
};

/** The text of each row of the list with this name. */
function rowsOf(name: string): (string | null)[] {
  return within(screen.getByRole("list", { name }))
    .getAllByRole("listitem")
    .map((row) => row.textContent);
}

/** A relaunched chat: its helper work, and no tasks or to-dos. */
function workQueue(...rows: HelperWork[]): Queue {
  return { tasks: [], todos: [], work: rows };
}

/** The Helper work row with this title. */
async function workRow(title: string): Promise<HTMLElement> {
  const list = await screen.findByRole("list", { name: "Helper work" });
  return within(list).getByRole("listitem", { name: title });
}

function buttons(row: HTMLElement): (string | null)[] {
  return within(row)
    .queryAllByRole("button")
    .map((button) => button.textContent);
}

describe("Agents", () => {
  beforeEach(() => {
    shell.desktop = true;
    for (const fn of Object.values(api)) fn.mockReset();
    feed.unlisten.mockReset();
    feed.unlistenGap.mockReset();
    api.onTasksChanged.mockImplementation((handler) => {
      feed.changed = handler;
      return Promise.resolve(feed.unlisten);
    });
    api.onEventGap.mockImplementation((handler) => {
      feed.gap = handler;
      return Promise.resolve(feed.unlistenGap);
    });
    api.shipStatus.mockResolvedValue(SHIP_READY);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("asks for a chat first and reads nothing without one", () => {
    render(<Agents onOpenChat={() => {}} />);
    expect(screen.getByText(AGENTS_NO_CHAT)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open chat" })).toBeDisabled();
    expect(api.getQueue).not.toHaveBeenCalled();
  });

  it("lists the chat's tasks and to-dos with their status, Stop only on live tasks", async () => {
    api.getQueue.mockResolvedValue(QUEUE);
    render(<Agents conversationId="shell-c1" onOpenChat={() => {}} />);

    const tasks = await screen.findByRole("list", { name: "Tasks" });
    const rows = within(tasks).getAllByRole("listitem");
    expect(rows.map((row) => row.querySelector(".card-status")?.textContent)).toEqual([
      "Running · t1",
      "Failed · s2",
      "Failed · t3",
      "Done · m4",
    ]);
    expect(rows.map((row) => within(row).queryByRole("button", { name: "Stop" }) !== null)).toEqual([
      true,
      true,
      false,
      false,
    ]);
    const todos = within(screen.getByRole("list", { name: "To-dos" })).getAllByRole("listitem");
    expect(todos.map((row) => row.textContent)).toEqual(["Read the specDone", "Write the pageIn progress"]);
    expect(api.getQueue).toHaveBeenCalledWith("shell-c1");
  });

  it("Stop calls stop_task for that task and shows what came back", async () => {
    api.getQueue.mockResolvedValue(QUEUE);
    api.stopTask.mockResolvedValue({
      ...QUEUE,
      tasks: [{ id: "t1", title: "cargo test --workspace", status: "stopped", live: false }],
    });
    render(<Agents conversationId="shell-c1" onOpenChat={() => {}} />);

    const row = await screen.findByRole("listitem", { name: "cargo test --workspace" });
    await userEvent.click(within(row).getByRole("button", { name: "Stop" }));
    expect(api.stopTask).toHaveBeenCalledWith("shell-c1", "t1");
    expect(await within(row).findByText("Stopped · t1")).toBeInTheDocument();
    expect(within(row).queryByRole("button", { name: "Stop" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Stop all" })).toBeNull();
  });

  it("Stop all stops every task of the chat", async () => {
    api.getQueue.mockResolvedValue(QUEUE);
    api.stopTask.mockResolvedValue({ tasks: [], todos: [] });
    render(<Agents conversationId="shell-c1" onOpenChat={() => {}} />);

    await userEvent.click(await screen.findByRole("button", { name: "Stop all" }));
    expect(api.stopTask).toHaveBeenCalledWith("shell-c1", undefined);
    expect(await screen.findByText(AGENTS_EMPTY)).toBeInTheDocument();
  });

  it("shows a refused stop", async () => {
    api.getQueue.mockResolvedValue(QUEUE);
    api.stopTask.mockRejectedValue("That chat isn't open in this browser.");
    render(<Agents conversationId="shell-c1" onOpenChat={() => {}} />);

    const row = await screen.findByRole("listitem", { name: "cargo test --workspace" });
    await userEvent.click(within(row).getByRole("button", { name: "Stop" }));
    expect(await screen.findByRole("status")).toHaveTextContent("That chat isn't open in this browser.");
  });

  it("a row opens its chat", async () => {
    api.getQueue.mockResolvedValue(QUEUE);
    const onOpenChat = vi.fn();
    render(<Agents conversationId="shell-c1" onOpenChat={onOpenChat} />);

    await userEvent.click(await screen.findByRole("button", { name: "Write the page" }));
    expect(onOpenChat).toHaveBeenCalledTimes(1);
  });

  it("reads the queue again every 2 seconds until it closes", async () => {
    vi.useFakeTimers();
    api.getQueue.mockResolvedValue({ tasks: [], todos: [] });
    const { unmount } = render(<Agents conversationId="shell-c1" onOpenChat={() => {}} />);
    await act(async () => {});
    expect(api.getQueue).toHaveBeenCalledTimes(1);
    expect(screen.getByText(AGENTS_EMPTY)).toBeInTheDocument();

    await act(async () => {
      vi.advanceTimersByTime(1999);
    });
    expect(api.getQueue).toHaveBeenCalledTimes(1);
    await act(async () => {
      vi.advanceTimersByTime(1);
    });
    expect(api.getQueue).toHaveBeenCalledTimes(2);

    unmount();
    await act(async () => {
      vi.advanceTimersByTime(6000);
    });
    expect(api.getQueue).toHaveBeenCalledTimes(2);
  });

  it("as the Code tab's Tasks panel it has no way out of the tab, and Stop still works", async () => {
    api.getQueue.mockResolvedValue(QUEUE);
    api.stopTask.mockResolvedValue({ ...QUEUE, tasks: QUEUE.tasks.map((t) => ({ ...t, live: false })) });
    render(<Agents conversationId="shell-c1" />);

    const panel = screen.getByRole("region", { name: "Agent tasks" });
    expect(within(panel).getByText(TASKS_NOTE)).toBeInTheDocument();
    expect(TASKS_NOTE).toBe("The lead's tasks and helpers for this session.");
    expect(await within(panel).findByText("Write the page")).toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: "Open chat" })).not.toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: "Write the page" })).not.toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: "cargo test --workspace" })).not.toBeInTheDocument();
    const live = within(panel).getByRole("listitem", { name: "cargo test --workspace" });
    await userEvent.click(within(live).getByRole("button", { name: "Stop" }));
    expect(api.stopTask).toHaveBeenCalledWith("shell-c1", "t1");
  });

  it("the Tasks panel lists the lead's tasks first, each with its state and helper", async () => {
    api.getQueue.mockResolvedValue({
      tasks: QUEUE.tasks,
      todos: [
        { id: "task-1", content: "Add the parser", status: "in_progress", state: "running", owner: "s2" },
        { id: "task-2", content: "Wire the page", status: "in_progress", state: "review", owner: "s3" },
        { id: "task-3", content: "Write the docs", status: "pending", state: "planned" },
        { id: "task-4", content: "Ask about the port", status: "in_progress", state: "needsYou", owner: "s5" },
        { id: "task-5", content: "Read the spec", status: "completed", state: "done", owner: "s1" },
        { id: "task-6", content: "Drop the old flag", status: "cancelled", state: "failed" },
      ],
    });
    render(<Agents conversationId="shell-c1" />);

    const tasks = await screen.findByRole("list", { name: "Tasks" });
    expect(within(tasks).getAllByRole("listitem").map((row) => row.textContent)).toEqual([
      "Add the parserRunning · helper s2",
      "Wire the pageReview · helper s3",
      "Write the docsPlanned",
      "Ask about the portNeeds you · helper s5",
      "Read the specDone · helper s1",
      "Drop the old flagFailed",
    ]);
    const hub = screen.getByRole("list", { name: "Helpers and commands" });
    expect(within(hub).getAllByRole("listitem").map((row) => row.querySelector(".card-status")?.textContent)).toEqual([
      "Running · t1",
      "Failed · s2",
      "Failed · t3",
      "Done · m4",
    ]);
    expect(screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent)).toEqual([
      "Tasks",
      "Helpers and commands",
    ]);
    expect(screen.queryByRole("list", { name: "To-dos" })).toBeNull();
    expect(LEAD_LABELS).toEqual({
      planned: "Planned",
      running: "Running",
      needsYou: "Needs you",
      review: "Review",
      notMerged: "Not merged",
      done: "Done",
      failed: "Failed",
    });
  });

  it("a task whose helper's work waits in a worktree says Not merged", async () => {
    api.getQueue.mockResolvedValue({
      tasks: [],
      todos: [{ id: "task-1", content: "Add the parser", status: "completed", state: "notMerged" }],
      work: [work()],
    });
    render(<Agents conversationId="shell-c1" />);

    const task = await screen.findByRole("listitem", { name: "Add the parser" });
    expect(task.textContent).toBe("Add the parserNot merged");
    expect(screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent)).toEqual(["Tasks", "Helper work"]);
  });

  it("lists a relaunched chat's helper work, each row with its state, files and buttons", async () => {
    const conflict = work({
      key: "k4-dddd",
      title: "task-4: Fix the test",
      state: "conflict",
      conflicts: ["src/a.rs", "src/b.rs"],
      skipped: ["notes.md"],
    });
    const gone = work({ key: "k5-eeee", title: "task-5: Merged and gone", state: "merged", treeKept: false });
    const discarded = work({ key: "k6-ffff", title: "task-6: Thrown away", state: "discarded", treeKept: false });
    const running = work({ key: "k7-gggg", title: "task-7: Still going", state: "running", files: [] });
    api.getQueue.mockResolvedValue(workQueue(work(), PARTIAL, KEPT, conflict, gone, discarded, running));
    render(<Agents conversationId="shell-c1" />);

    const list = await screen.findByRole("list", { name: "Helper work" });
    expect(screen.getByText(WORK_NOTE)).toBeInTheDocument();
    expect(screen.queryByText(TASKS_EMPTY)).toBeNull();
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((row) => [...row.querySelectorAll(".card-title, .card-status")].map((el) => el.textContent))).toEqual([
      [
        "task-1: Add the parser",
        "Ready to merge · 3 files",
        "1 new, 1 changed, 1 deleted",
        "Files: src/a.rs, src/b.rs, old.rs",
        "Worktree: /home/user/.config/GrokHub/helper-work/chat/k1-aaaa",
      ],
      [
        "task-2: Add the page",
        "Held · the helper did not finish",
        "Partial · it was stopped",
        "1 new, 0 changed, 1 deleted",
        "Files: ui/page.tsx, ui/old.tsx",
        "Worktree: /home/user/.config/GrokHub/helper-work/chat/k2-bbbb",
      ],
      [
        "task-3: Build it",
        "Merged · 3 files",
        "1 new, 1 changed, 1 deleted",
        "Files: src/a.rs, src/b.rs, old.rs",
        "Worktree: /home/user/.config/GrokHub/helper-work/chat/k3-cccc",
        "2 ignored paths (target/, .env.local) are not merged. They stay in the worktree until you delete it.",
      ],
      [
        "task-4: Fix the test",
        "Conflict · src/a.rs, src/b.rs",
        "1 new, 1 changed, 1 deleted",
        "Files: src/a.rs, src/b.rs, old.rs",
        "Left as you changed them: notes.md",
        "Worktree: /home/user/.config/GrokHub/helper-work/chat/k1-aaaa",
      ],
    ]);
    expect(rows.map(buttons)).toEqual([
      ["Merge", "Discard"],
      ["Merge partial work", "Discard"],
      ["Delete worktree"],
      ["Merge again", "Discard"],
    ]);
  });

  it("a redo names the work it redid, and that work says a newer helper redid it and keeps its buttons", async () => {
    const old = work({ key: "k4-dddd", title: "task-4: Fix the test", state: "conflict", conflicts: ["src/a.rs"], redone: true });
    const redo = work({ key: "k8-hhhh", title: "task-4: Fix the test again", redoOf: "task-4: Fix the test" });
    api.getQueue.mockResolvedValue(workQueue(old, redo, work({ key: "k9-iiii", title: "task-9: Other" })));
    render(<Agents conversationId="shell-c1" />);

    const lines = (row: HTMLElement) => [...row.querySelectorAll(".card-status")].map((el) => el.textContent);
    const oldRow = await workRow("task-4: Fix the test");
    expect(lines(oldRow)).toEqual([
      "Conflict · src/a.rs",
      "A newer helper redid this",
      "1 new, 1 changed, 1 deleted",
      "Files: src/a.rs, src/b.rs, old.rs",
      "Worktree: /home/user/.config/GrokHub/helper-work/chat/k1-aaaa",
    ]);
    expect(buttons(oldRow)).toEqual(["Merge again", "Discard"]);
    const redoRow = await workRow("task-4: Fix the test again");
    expect(lines(redoRow)).toEqual([
      "Ready to merge · 3 files",
      "Redo of: task-4: Fix the test",
      "1 new, 1 changed, 1 deleted",
      "Files: src/a.rs, src/b.rs, old.rs",
      "Worktree: /home/user/.config/GrokHub/helper-work/chat/k1-aaaa",
    ]);
    expect(buttons(redoRow)).toEqual(["Merge", "Discard"]);
    expect(lines(await workRow("task-9: Other"))).toEqual([
      "Ready to merge · 3 files",
      "1 new, 1 changed, 1 deleted",
      "Files: src/a.rs, src/b.rs, old.rs",
      "Worktree: /home/user/.config/GrokHub/helper-work/chat/k1-aaaa",
    ]);
  });

  it("a row names five files and counts the rest, and one ignored path reads in the singular", async () => {
    const big = work({
      files: ["a", "b", "c", "d", "e", "f", "g"],
      counts: { added: 200, changed: 40, deleted: 10 },
      state: "merged",
      ignored: ["target/"],
    });
    api.getQueue.mockResolvedValue(workQueue(big));
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    expect([...row.querySelectorAll(".card-status")].map((el) => el.textContent)).toEqual([
      "Merged · 250 files",
      "200 new, 40 changed, 10 deleted",
      "Files: a, b, c, d, e and 245 more",
      "Worktree: /home/user/.config/GrokHub/helper-work/chat/k1-aaaa",
      "1 ignored path (target/) is not merged. It stays in the worktree until you delete it.",
    ]);
  });

  it("a helper's own row says what became of its work, only for a helper of this launch", async () => {
    const lines = (over: Partial<HelperWork>) => workLine(work(over));
    expect(lines({})).toBe("Ready to merge · 3 files");
    expect(lines({ counts: { added: 1, changed: 0, deleted: 0 } })).toBe("Ready to merge · 1 file");
    expect(lines({ state: "held", why: "partial", whyText: "the helper did not finish" })).toBe("Partial · 3 files kept");
    expect(lines({ state: "held", why: "sealFailed", whyText: "the work could not be sealed: disk full" })).toBe(
      "Held · the work could not be sealed: disk full",
    );
    expect(lines({ state: "conflict", conflicts: ["a.rs", "b.rs"] })).toBe("Conflict · a.rs, b.rs");
    expect(lines({ state: "conflict", conflicts: ["a", "b", "c", "d", "e", "f"] })).toBe(
      "Conflict · a, b, c, d, e and 1 more",
    );
    expect(lines({ state: "merged" })).toBe("Merged · 3 files");
    expect(lines({ state: "empty" })).toBe("No changes");
    expect(lines({ state: "discarded" })).toBe("Discarded");
    expect(lines({ state: "running" })).toBeNull();

    api.getQueue.mockResolvedValue({
      tasks: [
        { id: "s1", title: "task-1: Add the parser", status: "done", live: false },
        { id: "s2", title: "task-2: Add the page", status: "done", live: false },
        { id: "s3", title: "task-3: Build it", status: "done", live: false },
      ],
      todos: [],
      work: [
        work({ key: "old", task: "s1", state: "merged", treeKept: false }),
        work({ key: "k1", task: "s1", live: true }),
        work({ key: "k2", task: "s2", live: true, state: "empty", treeKept: false }),
        // An earlier launch's s3: this launch's s3 made no work of its own.
        work({ key: "older", task: "s3", state: "discarded", treeKept: false }),
      ],
    });
    render(<Agents conversationId="shell-c1" />);

    const helpers = await screen.findByRole("list", { name: "Helpers and commands" });
    expect(within(helpers).getAllByRole("listitem").map((row) => row.textContent)).toEqual([
      "task-1: Add the parserDone · s1Ready to merge · 3 files",
      "task-2: Add the pageDone · s2No changes",
      "task-3: Build itDone · s3",
    ]);
  });

  it("Merge calls merge_task_work, and when the row is gone a line says so and takes focus", async () => {
    api.getQueue.mockResolvedValue(workQueue(work(), PARTIAL));
    api.mergeTaskWork.mockResolvedValue(workQueue(PARTIAL));
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    await userEvent.click(within(row).getByRole("button", { name: "Merge" }));
    expect(api.mergeTaskWork).toHaveBeenCalledTimes(1);
    expect(api.mergeTaskWork).toHaveBeenCalledWith("shell-c1", "k1-aaaa");
    const said = await screen.findByText("Merged 3 files into the project.");
    expect(said).toHaveAttribute("role", "status");
    expect(said).toHaveFocus();
    expect(screen.queryByRole("listitem", { name: "task-1: Add the parser" })).toBeNull();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("the last work merged leaves the line, not the empty panel", async () => {
    api.getQueue.mockResolvedValue(workQueue(work()));
    api.mergeTaskWork.mockResolvedValue(workQueue());
    render(<Agents conversationId="shell-c1" />);

    await userEvent.click(within(await workRow("task-1: Add the parser")).getByRole("button", { name: "Merge" }));
    expect(await screen.findByText("Merged 3 files into the project.")).toHaveFocus();
    expect(screen.queryByRole("list", { name: "Helper work" })).toBeNull();
    expect(screen.queryByText(TASKS_EMPTY)).toBeNull();
    expect(screen.queryByText(WORK_NOTE)).toBeNull();
  });

  it("a Merge that is refused shows why on its row, and the row stays as it was", async () => {
    api.getQueue.mockResolvedValue(workQueue(work(), PARTIAL));
    api.mergeTaskWork.mockRejectedValue(
      "the project folder /home/user/my-app can't be found. Put it back where it was, then try again.",
    );
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    const merge = within(row).getByRole("button", { name: "Merge" });
    await userEvent.click(merge);
    expect(await within(row).findByRole("alert")).toHaveTextContent(
      "the project folder /home/user/my-app can't be found. Put it back where it was, then try again.",
    );
    expect(within(row).getByText("Ready to merge · 3 files")).toBeInTheDocument();
    expect(buttons(row)).toEqual(["Merge", "Discard"]);
    expect(merge).toBeEnabled();
    expect(merge).toHaveFocus();
    // The other row has no error of its own, and nothing claims a merge.
    expect(within(await workRow("task-2: Add the page")).queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("a Merge that ends in a conflict changes the row and offers Merge again", async () => {
    api.getQueue.mockResolvedValue(workQueue(work()));
    api.mergeTaskWork.mockResolvedValue(workQueue(work({ state: "conflict", conflicts: ["src/a.rs"] })));
    render(<Agents conversationId="shell-c1" />);

    await userEvent.click(within(await workRow("task-1: Add the parser")).getByRole("button", { name: "Merge" }));
    const row = await workRow("task-1: Add the parser");
    expect(await within(row).findByText("Conflict · src/a.rs")).toBeInTheDocument();
    expect(row).toHaveClass("conflict");
    expect(buttons(row)).toEqual(["Merge again", "Discard"]);
    expect(screen.queryByRole("status")).toBeNull();
    expect(within(row).getByRole("button", { name: "Merge again" })).toHaveFocus();
  });

  it("buttons wait while a click's call runs", async () => {
    let answer: (queue: Queue) => void = () => {};
    api.getQueue.mockResolvedValue(workQueue(work(), PARTIAL));
    api.mergeTaskWork.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    await userEvent.click(within(row).getByRole("button", { name: "Merge" }));
    expect(buttons(row)).toEqual(["Merging…", "Discard"]);
    for (const button of screen.getAllByRole("button")) expect(button).toBeDisabled();
    await act(async () => answer(workQueue(PARTIAL)));
    expect(within(await workRow("task-2: Add the page")).getByRole("button", { name: "Merge partial work" })).toBeEnabled();
  });

  it("Merge partial work asks first, with the counts, and Cancel or Escape merges nothing", async () => {
    api.getQueue.mockResolvedValue(workQueue(PARTIAL));
    api.mergeTaskWork.mockResolvedValue(workQueue());
    render(<Agents conversationId="shell-c1" />);

    const opener = within(await workRow("task-2: Add the page")).getByRole("button", { name: "Merge partial work" });
    await userEvent.click(opener);
    const dialog = screen.getByRole("alertdialog", { name: "Merge partial work?" });
    expect(dialog.textContent).toBe(
      "Merge partial work?This helper did not finish (it was stopped). Merge its 2 files (1 new, 0 changed, 1 deleted) anyway?Merge partial workCancel",
    );
    // Focus starts on Cancel, and Tab stays inside the window.
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    expect(cancel).toHaveFocus();
    await userEvent.tab();
    expect(within(dialog).getByRole("button", { name: "Merge partial work" })).toHaveFocus();
    await userEvent.tab();
    expect(cancel).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(opener).toHaveFocus();

    await userEvent.click(opener);
    await userEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(opener).toHaveFocus();
    expect(api.mergeTaskWork).not.toHaveBeenCalled();

    await userEvent.click(opener);
    await userEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Merge partial work" }));
    expect(api.mergeTaskWork).toHaveBeenCalledTimes(1);
    expect(api.mergeTaskWork).toHaveBeenCalledWith("shell-c1", "k2-bbbb");
    expect(await screen.findByText("Merged 2 files into the project.")).toHaveFocus();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("the confirm says what is unusual about the work", async () => {
    const changed = work({
      key: "k8",
      title: "changed later",
      state: "held",
      why: "changedAfterSeal",
      whyText: "the worktree changed after the helper ended",
      counts: { added: 1, changed: 0, deleted: 0 },
    });
    const git = work({
      key: "k9",
      title: "switched branch",
      state: "held",
      why: "gitStateChanged",
      whyText: "the helper changed the worktree's git state",
    });
    api.getQueue.mockResolvedValue(workQueue(changed, git));
    render(<Agents conversationId="shell-c1" />);

    await userEvent.click(within(await workRow("changed later")).getByRole("button", { name: "Merge partial work" }));
    expect(screen.getByRole("alertdialog").querySelector("p")?.textContent).toBe(
      "This worktree changed after the helper ended. Merge its 1 file (1 new, 0 changed, 0 deleted) anyway?",
    );
    await userEvent.keyboard("{Escape}");
    await userEvent.click(within(await workRow("switched branch")).getByRole("button", { name: "Merge partial work" }));
    expect(screen.getByRole("alertdialog").querySelector("p")?.textContent).toBe(
      "This helper changed its worktree's git state. Merge its 3 files (1 new, 1 changed, 1 deleted) anyway?",
    );
  });

  it("work the app closed under is read first, then asks like partial work", async () => {
    const interrupted = work({
      state: "held",
      why: "interrupted",
      whyText: "the app closed while the helper ran",
      partial: true,
      ended: "the app closed while it ran",
      files: [],
      counts: { added: 0, changed: 0, deleted: 0 },
    });
    const sealed = work({
      state: "held",
      why: "partial",
      whyText: "the helper did not finish",
      partial: true,
      ended: "the app closed while it ran",
    });
    api.getQueue.mockResolvedValue(workQueue(interrupted));
    api.mergeTaskWork.mockResolvedValue(workQueue(sealed));
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    expect([...row.querySelectorAll(".card-status")].map((el) => el.textContent)).toEqual([
      "Held · the app closed while the helper ran",
      "Partial · the app closed while it ran",
      "Worktree: /home/user/.config/GrokHub/helper-work/chat/k1-aaaa",
    ]);
    await userEvent.click(within(row).getByRole("button", { name: "Merge partial work" }));
    expect(api.mergeTaskWork).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(await within(row).findByText("1 new, 1 changed, 1 deleted")).toBeInTheDocument();
    expect(screen.queryByRole("status")).toBeNull();

    await userEvent.click(within(row).getByRole("button", { name: "Merge partial work" }));
    expect(screen.getByRole("alertdialog").querySelector("p")?.textContent).toBe(
      "This helper did not finish (the app closed while it ran). Merge its 3 files (1 new, 1 changed, 1 deleted) anyway?",
    );
    expect(api.mergeTaskWork).toHaveBeenCalledTimes(1);
  });

  it("each kind of work offers the Merge the app will take, or none", () => {
    const offer = (over: Partial<HelperWork>) => mergeOffer(work(over));
    expect(offer({})).toEqual({ label: "Merge", asks: false });
    expect(offer({ state: "conflict" })).toEqual({ label: "Merge again", asks: false });
    for (const why of ["partial", "gitStateChanged", "changedAfterSeal"] as const) {
      expect(offer({ state: "held", why })).toEqual({ label: "Merge partial work", asks: true });
    }
    expect(offer({ state: "held", why: "interrupted" })).toEqual({ label: "Merge partial work", asks: false });
    for (const why of ["projectChanged", "blocked", "mergeFailed", "rolledBack"] as const) {
      expect(offer({ state: "held", why })).toEqual({ label: "Merge", asks: false });
    }
    for (const why of ["ignoredOnly", "nestedRepo", "sealFailed", "orphan", "applyFailed"] as const) {
      expect(offer({ state: "held", why })).toBeNull();
    }
    expect(offer({ state: "held" })).toBeNull();
    expect(offer({ state: "merged" })).toBeNull();
    expect(offer({ state: "running" })).toBeNull();
    expect(offer({ state: "empty" })).toBeNull();
    expect(offer({ state: "discarded" })).toBeNull();
    // A merge that stopped part way is finished or put back, never merged again.
    expect(offer({ landing: true })).toBeNull();
    expect(offer({ state: "held", why: "partial", landing: true })).toBeNull();
  });

  it("work that can't be merged as it is offers only Discard", async () => {
    const unsealed = work({
      state: "held",
      why: "sealFailed",
      whyText: "the work could not be sealed: disk full",
      files: [],
      counts: { added: 0, changed: 0, deleted: 0 },
    });
    api.getQueue.mockResolvedValue(workQueue(unsealed));
    api.discardTaskWork.mockResolvedValue(workQueue());
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    expect(within(row).getByText("Held · the work could not be sealed: disk full")).toBeInTheDocument();
    expect(buttons(row)).toEqual(["Discard"]);
    await userEvent.click(within(row).getByRole("button", { name: "Discard" }));
    expect(screen.getByRole("alertdialog", { name: "Discard this work?" }).querySelector("p")?.textContent).toBe(
      "Delete this helper's worktree and everything in it? This can't be undone.",
    );
  });

  it("a merge that stopped part way offers Finish merge and Put files back, and nothing else", async () => {
    api.getQueue.mockResolvedValue(workQueue(STOPPED_MERGE));
    api.resumeTaskWork.mockResolvedValueOnce(workQueue());
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    expect(within(row).getByText("Held · the merge stopped part way")).toBeInTheDocument();
    expect(buttons(row)).toEqual(["Finish merge", "Put files back"]);
    await userEvent.click(within(row).getByRole("button", { name: "Finish merge" }));
    expect(api.resumeTaskWork).toHaveBeenCalledTimes(1);
    expect(api.resumeTaskWork).toHaveBeenCalledWith("shell-c1", "k1-aaaa", true);
    expect(await screen.findByText("Merged 3 files into the project.")).toHaveFocus();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("Finish merge can come back as work that changed since, which then asks", async () => {
    api.getQueue.mockResolvedValue(workQueue(STOPPED_MERGE));
    api.resumeTaskWork.mockResolvedValue(
      workQueue(
        work({
          state: "held",
          why: "changedAfterSeal",
          whyText: "the worktree changed after the helper ended",
          files: ["late.rs"],
          counts: { added: 1, changed: 0, deleted: 0 },
          skipped: ["src/a.rs"],
        }),
      ),
    );
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    await userEvent.click(within(row).getByRole("button", { name: "Finish merge" }));
    expect(await within(row).findByText("Held · the worktree changed after the helper ended")).toBeInTheDocument();
    expect(within(row).getByText("Left as you changed them: src/a.rs")).toBeInTheDocument();
    expect(buttons(row)).toEqual(["Merge partial work", "Discard"]);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("Put files back returns the row to what it was", async () => {
    api.getQueue.mockResolvedValue(workQueue(STOPPED_MERGE));
    api.resumeTaskWork.mockResolvedValue(workQueue(work()));
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    await userEvent.click(within(row).getByRole("button", { name: "Put files back" }));
    expect(api.resumeTaskWork).toHaveBeenCalledWith("shell-c1", "k1-aaaa", false);
    expect(await screen.findByRole("status")).toHaveTextContent("Files put back.");
    expect(within(row).getByText("Ready to merge · 3 files")).toBeInTheDocument();
    expect(buttons(row)).toEqual(["Merge", "Discard"]);
    expect(within(row).getByRole("button", { name: "Merge" })).toHaveFocus();
  });

  it("a Put files back that could not finish leaves the row and claims nothing", async () => {
    api.getQueue.mockResolvedValue(workQueue(STOPPED_MERGE));
    api.resumeTaskWork.mockResolvedValue(workQueue(STOPPED_MERGE));
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    await userEvent.click(within(row).getByRole("button", { name: "Put files back" }));
    expect(api.resumeTaskWork).toHaveBeenCalledTimes(1);
    expect(await within(row).findByRole("button", { name: "Put files back" })).toBeEnabled();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("Discard asks first, and Keep deletes nothing", async () => {
    api.getQueue.mockResolvedValue(workQueue(work(), PARTIAL));
    api.discardTaskWork.mockResolvedValue(workQueue(PARTIAL));
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    const opener = within(row).getByRole("button", { name: "Discard" });
    await userEvent.click(opener);
    const dialog = screen.getByRole("alertdialog", { name: "Discard this work?" });
    expect(dialog.textContent).toBe(
      "Discard this work?Delete this helper's 3 changed files? This can't be undone.DiscardKeep",
    );
    const keep = within(dialog).getByRole("button", { name: "Keep" });
    expect(keep).toHaveFocus();
    await userEvent.click(keep);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(opener).toHaveFocus();
    expect(api.discardTaskWork).not.toHaveBeenCalled();

    await userEvent.click(opener);
    await userEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Discard" }));
    expect(api.discardTaskWork).toHaveBeenCalledTimes(1);
    expect(api.discardTaskWork).toHaveBeenCalledWith("shell-c1", "k1-aaaa");
    expect(await screen.findByText("Work discarded.")).toHaveFocus();
    expect(screen.queryByRole("listitem", { name: "task-1: Add the parser" })).toBeNull();
    expect(screen.getByRole("listitem", { name: "task-2: Add the page" })).toBeInTheDocument();
  });

  it("a queue read behind the confirm leaves focus on the button the person chose", async () => {
    // A new answer each time, as from the app: every read repaints the panel.
    api.getQueue.mockImplementation(() => Promise.resolve(workQueue(work(), PARTIAL)));
    api.discardTaskWork.mockResolvedValue(workQueue(PARTIAL));
    render(<Agents conversationId="shell-c1" live />);

    const row = await workRow("task-1: Add the parser");
    await userEvent.click(within(row).getByRole("button", { name: "Discard" }));
    const dialog = screen.getByRole("alertdialog", { name: "Discard this work?" });
    expect(within(dialog).getByRole("button", { name: "Keep" })).toHaveFocus();
    await userEvent.tab({ shift: true });
    const yes = within(dialog).getByRole("button", { name: "Discard" });
    expect(yes).toHaveFocus();

    const reads = api.getQueue.mock.calls.length;
    await act(async () => feed.changed("shell-c1"));
    expect(api.getQueue).toHaveBeenCalledTimes(reads + 1);
    expect(yes).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    expect(api.discardTaskWork).toHaveBeenCalledTimes(1);
    expect(api.discardTaskWork).toHaveBeenCalledWith("shell-c1", "k1-aaaa");
  });

  it("Escape still closes the confirm after a queue read", async () => {
    api.getQueue.mockImplementation(() => Promise.resolve(workQueue(work())));
    render(<Agents conversationId="shell-c1" live />);

    const row = await workRow("task-1: Add the parser");
    await userEvent.click(within(row).getByRole("button", { name: "Discard" }));
    await act(async () => feed.changed("shell-c1"));
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(api.discardTaskWork).not.toHaveBeenCalled();
  });

  it("a Discard that is refused shows why on its row", async () => {
    api.getQueue.mockResolvedValue(workQueue(work({ counts: { added: 1, changed: 0, deleted: 0 }, files: ["a.rs"] })));
    api.discardTaskWork.mockRejectedValue("that helper is still running");
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    await userEvent.click(within(row).getByRole("button", { name: "Discard" }));
    expect(screen.getByRole("alertdialog").querySelector("p")?.textContent).toBe(
      "Delete this helper's 1 changed file? This can't be undone.",
    );
    await userEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Discard" }));
    expect(await within(row).findByRole("alert")).toHaveTextContent("that helper is still running");
    expect(buttons(row)).toEqual(["Merge", "Discard"]);
    // The window gave focus back to the button that opened it.
    expect(within(row).getByRole("button", { name: "Discard" })).toHaveFocus();
  });

  it("Delete worktree asks about the ignored paths in it", async () => {
    api.getQueue.mockResolvedValue(workQueue(KEPT));
    api.discardTaskWork.mockResolvedValue(workQueue());
    render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-3: Build it");
    await userEvent.click(within(row).getByRole("button", { name: "Delete worktree" }));
    const dialog = screen.getByRole("alertdialog", { name: "Delete this worktree?" });
    expect(dialog.textContent).toBe(
      "Delete this worktree?Delete the worktree and the 2 ignored paths in it?Delete worktreeKeep",
    );
    expect(within(dialog).getByRole("button", { name: "Keep" })).toHaveFocus();
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete worktree" }));
    expect(api.discardTaskWork).toHaveBeenCalledWith("shell-c1", "k3-cccc");
    expect(await screen.findByText("Worktree deleted.")).toHaveFocus();
  });

  it("a kept worktree with nothing ignored in it is asked about plainly", async () => {
    api.getQueue.mockResolvedValue(
      workQueue(work({ state: "merged", ignored: ["target/"] }), work({ key: "k2", title: "bare", state: "merged" })),
    );
    render(<Agents conversationId="shell-c1" />);

    await userEvent.click(within(await workRow("task-1: Add the parser")).getByRole("button", { name: "Delete worktree" }));
    expect(screen.getByRole("alertdialog").querySelector("p")?.textContent).toBe(
      "Delete the worktree and the 1 ignored path in it?",
    );
    await userEvent.keyboard("{Escape}");
    await userEvent.click(within(await workRow("bare")).getByRole("button", { name: "Delete worktree" }));
    expect(screen.getByRole("alertdialog").querySelector("p")?.textContent).toBe("Delete the worktree?");
  });

  it("a helper work list that can't be read says so in one line", async () => {
    api.getQueue.mockResolvedValue({
      tasks: [],
      todos: [],
      work: [],
      workError: "/home/user/.config/GrokHub/helper-work/chat/work.json: EOF while parsing a value at line 1 column 18",
    });
    render(<Agents conversationId="shell-c1" />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The helper work list can't be read: /home/user/.config/GrokHub/helper-work/chat/work.json: EOF while parsing a value at line 1 column 18",
    );
    expect(screen.getByRole("heading", { level: 2 })).toHaveTextContent("Helper work");
    expect(screen.queryByRole("list", { name: "Helper work" })).toBeNull();
    expect(screen.queryByText(TASKS_EMPTY)).toBeNull();
  });

  it("a paired browser lists helper work with no buttons", async () => {
    shell.desktop = false;
    api.getQueue.mockResolvedValue(workQueue(work(), KEPT, STOPPED_MERGE));
    render(<Agents conversationId="shell-c1" onOpenChat={() => {}} />);

    const list = await screen.findByRole("list", { name: "Helper work" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(3);
    expect(within(list).queryByRole("button")).toBeNull();
    expect(screen.getByText(WORK_DESKTOP_ONLY)).toBeInTheDocument();
    expect(screen.queryByText(WORK_NOTE)).toBeNull();
  });

  it("another chat starts without the last chat's refusal or result line", async () => {
    api.getQueue.mockImplementation((id) => Promise.resolve(id === "shell-c1" ? workQueue(work()) : workQueue(PARTIAL)));
    api.mergeTaskWork.mockRejectedValue(
      "the project is in the middle of a merge or rebase, or has unresolved conflicts. Finish or abort it, then try again.",
    );
    const { rerender } = render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    await userEvent.click(within(row).getByRole("button", { name: "Merge" }));
    expect(await within(row).findByRole("alert")).toBeInTheDocument();
    rerender(<Agents conversationId="shell-c2" />);
    expect(await workRow("task-2: Add the page")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("a Merge that answers after a chat switch changes nothing on the new chat's panel", async () => {
    api.getQueue.mockImplementation((id) => Promise.resolve(id === "shell-c1" ? workQueue(work()) : workQueue(PARTIAL)));
    let answer: (queue: Queue) => void = () => {};
    api.mergeTaskWork.mockReturnValue(new Promise<Queue>((resolve) => (answer = resolve)));
    const { rerender } = render(<Agents conversationId="shell-c1" />);

    const row = await workRow("task-1: Add the parser");
    await userEvent.click(within(row).getByRole("button", { name: "Merge" }));
    rerender(<Agents conversationId="shell-c2" />);
    expect(await workRow("task-2: Add the page")).toBeInTheDocument();
    // The first chat's merge ends now, with that chat's queue: no work left.
    await act(async () => answer(workQueue()));
    expect(screen.getByRole("listitem", { name: "task-2: Add the page" })).toBeInTheDocument();
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByText("Merged 3 files into the project.")).toBeNull();
  });

  it("the Tasks panel of a session with nothing queued says what will show up", async () => {
    api.getQueue.mockResolvedValue({ tasks: [], todos: [] });
    render(<Agents conversationId="shell-c1" />);
    expect(await screen.findByText(TASKS_EMPTY)).toBeInTheDocument();
    expect(screen.queryByText(AGENTS_EMPTY)).not.toBeInTheDocument();
  });

  it("a lead's Tasks panel says Not reviewed, and Review asks the lead once per read", async () => {
    const onAsk = vi.fn<(text: string) => void>();
    api.getQueue.mockResolvedValue(LEAD_QUEUE);
    render(<Agents conversationId="shell-c1" live onAsk={onAsk} />);

    expect(await screen.findByRole("heading", { level: 2, name: "Review" })).toBeInTheDocument();
    expect(screen.getByText(NOT_REVIEWED)).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Review findings" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Ask the lead to fix" })).toBeNull();
    const button = screen.getByRole("button", { name: "Review" });
    await userEvent.click(button);
    expect(onAsk.mock.calls).toEqual([[REVIEW_ASK]]);
    expect(REVIEW_ASK).toBe("Review the change: start one helper with type review, then tell me what it found.");
    // The click is taken: the button waits for the panel's next read.
    expect(button).toBeDisabled();
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: REVIEW });
    await act(async () => feed.changed("shell-c1"));
    expect(await screen.findByRole("button", { name: "Review again" })).toBeEnabled();
  });

  it("a review lists its findings, blockers first, with Review again and Ask the lead to fix", async () => {
    const onAsk = vi.fn<(text: string) => void>();
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: REVIEW });
    render(<Agents conversationId="shell-c1" live onAsk={onAsk} />);

    const list = await screen.findByRole("list", { name: "Review findings" });
    expect(screen.getByText("Reviewed: 1 blocker, 2 notes")).toBeInTheDocument();
    expect(within(list).getAllByRole("listitem").map((row) => row.textContent)).toEqual([
      "src/a.rs:3 the total is off by oneBlocker",
      "src/b.rs:9 the name is unclearMedium",
      "README:1 reads wellNote",
    ]);
    expect(screen.queryByRole("button", { name: "Review" })).toBeNull();
    expect(screen.getByRole("button", { name: "Review again" })).toBeEnabled();
    await userEvent.click(screen.getByRole("button", { name: "Ask the lead to fix" }));
    expect(onAsk.mock.calls).toEqual([[FIX_ASK]]);
    expect(FIX_ASK).toBe("Fix each blocker the review found through a helper, then review again.");
    expect(screen.getByRole("button", { name: "Review again" })).toBeDisabled();
  });

  it("a review's line counts blockers and notes, and says when there was no plan", () => {
    expect(reviewLine(REVIEW)).toBe("Reviewed: 1 blocker, 2 notes");
    const clean: Review = { ...REVIEW, findings: [{ severity: "info", text: "README:1 nothing to fix" }], blockers: 0 };
    expect(reviewLine(clean)).toBe("Reviewed: 1 note");
    const two: Review = {
      ...REVIEW,
      findings: [
        { severity: "high", text: "a.rs:1 one" },
        { severity: "high", text: "a.rs:2 two" },
      ],
      blockers: 2,
      planGiven: false,
    };
    expect(reviewLine(two)).toBe("Reviewed: 2 blockers · no plan");
    // A change too long to show the helper whole.
    expect(reviewLine({ ...clean, cut: true })).toBe("Reviewed: 1 note · only part of the change was shown");
    expect(reviewLine({ ...two, cut: true })).toBe(
      "Reviewed: 2 blockers · no plan · only part of the change was shown",
    );
  });

  it("Review and Ask the lead to fix wait while a turn of the session runs", async () => {
    const onAsk = vi.fn<(text: string) => void>();
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: REVIEW, working: true });
    render(<Agents conversationId="shell-c1" live onAsk={onAsk} />);
    // An ask sent now would wait behind the turn, and so would a second one.
    const again = await screen.findByRole("button", { name: "Review again" });
    expect(again).toBeDisabled();
    expect(screen.getByRole("button", { name: "Ask the lead to fix" })).toBeDisabled();
    await userEvent.click(again);
    expect(onAsk).not.toHaveBeenCalled();
    // The turn ends: both can be pressed.
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: REVIEW });
    await act(async () => feed.changed("shell-c1"));
    expect(await screen.findByRole("button", { name: "Review again" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Ask the lead to fix" })).toBeEnabled();
  });

  it("a clean review offers no fix", async () => {
    const clean: Review = { ...REVIEW, findings: [{ severity: "info", text: "README:1 nothing to fix" }], blockers: 0 };
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: clean });
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    expect(await screen.findByText("Reviewed: 1 note")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Review again" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Ask the lead to fix" })).toBeNull();
  });

  it("a session with no lead and the Agents page have no Review section", async () => {
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: REVIEW });
    const view = render(<Agents conversationId="shell-c1" />);
    expect(await screen.findByRole("list", { name: "Tasks" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Review" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Review again" })).toBeNull();
    view.unmount();
    render(<Agents conversationId="shell-c1" onOpenChat={() => {}} onAsk={() => {}} />);
    expect(await screen.findByRole("list", { name: "To-dos" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Review" })).toBeNull();
  });

  it("a lead session with no tasks yet has no Review section", async () => {
    api.getQueue.mockResolvedValue({ tasks: [], todos: [] });
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    expect(await screen.findByText(TASKS_EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Review" })).toBeNull();
  });

  it("a review that can't be read says so and is not Not reviewed", async () => {
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, reviewError: "review.json: EOF while parsing" });
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The review can't be read: review.json: EOF while parsing",
    );
    expect(screen.queryByText(NOT_REVIEWED)).toBeNull();
    expect(screen.getByRole("button", { name: "Review" })).toBeEnabled();
  });

  it("a paired browser lists the findings and starts no review", async () => {
    shell.desktop = false;
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: REVIEW });
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    const list = await screen.findByRole("list", { name: "Review findings" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(3);
    // It reads no checks, so it says where the review's age is shown.
    expect(
      screen.getByText("Start a review in the desktop app. The desktop app says whether this review is still current."),
    ).toBeInTheDocument();
    expect(REVIEW_DESKTOP_ONLY).toBe("Start a review in the desktop app.");
    expect(screen.queryByRole("button", { name: "Review again" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Ask the lead to fix" })).toBeNull();
    expect(api.shipStatus).not.toHaveBeenCalled();
  });

  it("a paired browser with no review yet says only where to start one", async () => {
    shell.desktop = false;
    api.getQueue.mockResolvedValue(LEAD_QUEUE);
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    expect(await screen.findByText(REVIEW_DESKTOP_ONLY)).toBeInTheDocument();
  });

  it("a lead's Tasks panel lists the checks and what is in the way of a commit", async () => {
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: REVIEW });
    api.shipStatus.mockResolvedValue(SHIP_BLOCKED);
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);

    expect(await screen.findByRole("list", { name: "Checks" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: "Checks" })).toBeInTheDocument();
    expect(screen.getByText(CHECKS_NOTE)).toBeInTheDocument();
    expect(rowsOf("Checks")).toEqual([
      "TestsNot run: cargo testRun",
      "Secret scansrc/client.ts:12 · looks like a key or a password.env · a file that holds secrets · in commit 01234567",
      "Branchwork/one",
    ]);
    expect(screen.getByText(CHECKS_BLOCKED)).toBeInTheDocument();
    expect(CHECKS_BLOCKED).toBe("In the way of a commit:");
    expect(rowsOf("In the way of a commit")).toEqual([
      "The review found 1 blocker.",
      "The tests have not run on this change.",
      "The secret scan flagged 2 places in the change.",
    ]);
    expect(screen.queryByText(CHECKS_PASS)).toBeNull();
    expect(screen.getByText("Reviewed: 1 blocker, 2 notes")).toBeInTheDocument();
    expect(api.shipStatus.mock.calls).toEqual([["shell-c1"]]);
    // Nothing on the panel commits or pushes.
    expect(screen.queryByRole("button", { name: /commit|push/i })).toBeNull();
  });

  it("a change with nothing in the way says every check passes", async () => {
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: { ...REVIEW, blockers: 0, findings: [] } });
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    expect(await screen.findByText(CHECKS_PASS)).toBeInTheDocument();
    expect(CHECKS_PASS).toBe("Every check passes.");
    expect(rowsOf("Checks")).toEqual(["TestsPassed: cargo testRun", "Secret scanNothing flagged.", "Branchwork/one"]);
    expect(screen.queryByText(CHECKS_BLOCKED)).toBeNull();
    expect(screen.queryByRole("list", { name: "In the way of a commit" })).toBeNull();
  });

  it("a review of an earlier state of the files reads Stale in place of its count", async () => {
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: REVIEW });
    api.shipStatus.mockResolvedValue({
      blocked: ["Files or commits changed since the review. Review again."],
      checks: { ...SHIP_READY.checks!, review: { state: "stale" } },
    });
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    expect(await screen.findByText(REVIEW_STALE)).toBeInTheDocument();
    expect(REVIEW_STALE).toBe("Stale: files or commits changed since the review");
    expect(screen.queryByText("Reviewed: 1 blocker, 2 notes")).toBeNull();
    // The findings it made stay listed.
    expect(rowsOf("Review findings")).toHaveLength(3);
    expect(screen.getByRole("button", { name: "Review again" })).toBeEnabled();
  });

  it("a new review is not called stale by the checks read for the one before", async () => {
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: REVIEW });
    api.shipStatus.mockResolvedValueOnce({
      blocked: ["Files or commits changed since the review. Review again."],
      checks: { ...SHIP_READY.checks!, review: { state: "stale" } },
    });
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    expect(await screen.findByText(REVIEW_STALE)).toBeInTheDocument();
    // A new review arrives. Until its own checks are read, the old read
    // says nothing about it.
    let answer: (status: ShipStatus) => void = () => {};
    api.shipStatus.mockReturnValueOnce(new Promise<ShipStatus>((resolve) => (answer = resolve)));
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: { ...REVIEW, atMs: REVIEW.atMs + 60_000 } });
    await act(async () => feed.changed("shell-c1"));
    expect(await screen.findByText("Reviewed: 1 blocker, 2 notes")).toBeInTheDocument();
    expect(screen.queryByText(REVIEW_STALE)).toBeNull();
    await act(async () => answer(SHIP_BLOCKED));
    expect(screen.getByText("Reviewed: 1 blocker, 2 notes")).toBeInTheDocument();
  });

  it("a project that can't be read lists the reason and no checks", async () => {
    api.getQueue.mockResolvedValue(LEAD_QUEUE);
    api.shipStatus.mockResolvedValue({ blocked: ["This folder is not a git repository."], checks: null });
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    expect(await screen.findByRole("list", { name: "In the way of a commit" })).toBeInTheDocument();
    expect(rowsOf("In the way of a commit")).toEqual(["This folder is not a git repository."]);
    expect(screen.queryByRole("list", { name: "Checks" })).toBeNull();
    expect(screen.getByText(NOT_REVIEWED)).toBeInTheDocument();
  });

  it("each state of the tests and each kind of flagged place has its own line", () => {
    expect(testsLine({ state: "noCommand" })).toBe("No test command is known, so no tests are run.");
    expect(testsLine({ state: "notRun", command: "npm test" })).toBe("Not run: npm test");
    expect(testsLine({ state: "stale", command: "npm test" })).toBe("Files changed since the last run: npm test");
    expect(testsLine({ state: "failed", command: "npm test", exit: 3 })).toBe("Failed (exit 3): npm test");
    expect(testsLine({ state: "passed", command: "npm test" })).toBe("Passed: npm test");
    expect(secretLine({ path: "a/b.ts", line: 7, kind: "held", commit: null })).toBe(
      "a/b.ts:7 · one of this project's secrets",
    );
    expect(secretLine({ path: "id_rsa", line: null, kind: "name", commit: null })).toBe(
      "id_rsa · a file that holds secrets",
    );
  });

  it("a scan that could not run says so, a long list is counted, and no command has no Run", async () => {
    api.getQueue.mockResolvedValue(LEAD_QUEUE);
    api.shipStatus.mockResolvedValue({
      blocked: ["The secret scan could not run: git diff-tree failed"],
      checks: { branch: null, review: { state: "none" }, tests: { state: "noCommand" }, secrets: null },
    });
    const view = render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    await screen.findByRole("list", { name: "Checks" });
    expect(rowsOf("Checks")).toEqual([
      "TestsNo test command is known, so no tests are run.",
      "Secret scanThe scan could not run.",
      "BranchDetached HEAD",
    ]);
    expect(screen.queryByRole("button", { name: "Run" })).toBeNull();
    view.unmount();

    const many = Array.from({ length: 23 }, (_, at) => ({
      path: "keys.txt",
      line: at + 1,
      kind: "shape" as const,
      commit: null,
    }));
    api.shipStatus.mockResolvedValue({
      blocked: ["The secret scan flagged 23 places in the change."],
      checks: { ...SHIP_READY.checks!, secrets: many },
    });
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    const scan = within(await screen.findByRole("list", { name: "Checks" })).getByRole("listitem", {
      name: "Secret scan",
    });
    expect(within(scan).getByText("keys.txt:20 · looks like a key or a password")).toBeInTheDocument();
    expect(within(scan).queryByText("keys.txt:21 · looks like a key or a password")).toBeNull();
    expect(within(scan).getByText("and 3 more")).toBeInTheDocument();
  });

  it("the first Run of a test command asks with the exact command, then runs it in the session", async () => {
    api.getQueue.mockResolvedValue(LEAD_QUEUE);
    api.shipStatus.mockResolvedValue(SHIP_BLOCKED);
    api.codeTestPlan.mockResolvedValue({ command: "cargo test", needsConfirm: true, remembered: null });
    api.codeTestRun.mockResolvedValue({ command: "cargo test", exit: 0, output: "ok\n", cut: false, changedFiles: false });
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);

    await userEvent.click(await screen.findByRole("button", { name: "Run" }));
    const ask = await screen.findByRole("alertdialog", { name: "Run the tests?" });
    expect(within(ask).getByText("cargo test")).toBeInTheDocument();
    expect(api.codeTestPlan.mock.calls).toEqual([[null, "shell-c1"]]);
    expect(api.codeTestRun).not.toHaveBeenCalled();
    // Cancel runs nothing.
    await userEvent.click(within(ask).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(api.codeTestRun).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole("button", { name: "Run" }));
    api.shipStatus.mockResolvedValue(SHIP_READY);
    await userEvent.click(
      within(await screen.findByRole("alertdialog", { name: "Run the tests?" })).getByRole("button", { name: "Run" }),
    );
    expect(api.codeTestRun.mock.calls).toEqual([["cargo test", true, "shell-c1"]]);
    const tests = within(screen.getByRole("list", { name: "Checks" })).getByRole("listitem", { name: "Tests" });
    expect(await within(tests).findByText("Tests passed.")).toBeInTheDocument();
    // The checks are read again after the run.
    expect(await within(tests).findByText("Passed: cargo test")).toBeInTheDocument();
    expect(api.shipStatus).toHaveBeenCalledTimes(2);
  });

  it("a remembered test command runs with no ask, and a failed or file-changing run says so", async () => {
    api.getQueue.mockResolvedValue(LEAD_QUEUE);
    api.shipStatus.mockResolvedValue(SHIP_BLOCKED);
    api.codeTestPlan.mockResolvedValue({ command: "cargo test", needsConfirm: false, remembered: "cargo test" });
    api.codeTestRun.mockResolvedValue({ command: "cargo test", exit: 3, output: "", cut: false, changedFiles: false });
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);

    await userEvent.click(await screen.findByRole("button", { name: "Run" }));
    expect(await screen.findByText("Tests exited with 3.")).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(api.codeTestRun.mock.calls).toEqual([["cargo test", false, "shell-c1"]]);

    api.codeTestRun.mockResolvedValue({ command: "cargo test", exit: 0, output: "", cut: false, changedFiles: true });
    await userEvent.click(screen.getByRole("button", { name: "Run" }));
    expect(await screen.findByText(TESTS_CHANGED_FILES)).toBeInTheDocument();
    expect(TESTS_CHANGED_FILES).toBe("The tests changed files; run them again.");
    expect(screen.queryByText("Tests passed.")).toBeNull();

    api.codeTestRun.mockRejectedValue("A run is already in progress.");
    await userEvent.click(screen.getByRole("button", { name: "Run" }));
    expect(await screen.findByText("A run is already in progress.")).toBeInTheDocument();
  });

  it("the checks are read again when the review, the helper work or the turn changes, not on every nudge", async () => {
    api.getQueue.mockResolvedValue(LEAD_QUEUE);
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    await screen.findByRole("list", { name: "Checks" });
    expect(api.shipStatus).toHaveBeenCalledTimes(1);

    // A nudge with the same queue takes no new snapshot of the project.
    await act(async () => feed.changed("shell-c1"));
    expect(api.getQueue).toHaveBeenCalledTimes(2);
    expect(api.shipStatus).toHaveBeenCalledTimes(1);

    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, working: true });
    await act(async () => feed.changed("shell-c1"));
    expect(api.shipStatus).toHaveBeenCalledTimes(2);

    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, working: true, review: REVIEW });
    await act(async () => feed.changed("shell-c1"));
    expect(api.shipStatus).toHaveBeenCalledTimes(3);

    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, working: true, review: REVIEW, work: [work()] });
    await act(async () => feed.changed("shell-c1"));
    expect(api.shipStatus).toHaveBeenCalledTimes(4);

    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, working: true, review: REVIEW, work: [work({ state: "merged" })] });
    await act(async () => feed.changed("shell-c1"));
    expect(api.shipStatus).toHaveBeenCalledTimes(5);
  });

  it("Check again reads the checks, and a read that fails says so", async () => {
    api.getQueue.mockResolvedValue(LEAD_QUEUE);
    render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    expect(await screen.findByText(CHECKS_PASS)).toBeInTheDocument();

    api.shipStatus.mockResolvedValue(SHIP_BLOCKED);
    await userEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(await screen.findByText("The tests have not run on this change.")).toBeInTheDocument();
    expect(api.shipStatus).toHaveBeenCalledTimes(2);

    api.shipStatus.mockRejectedValue("the window is closing");
    await userEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The checks can't be read: the window is closing");
  });

  it("a paired browser, a session with no lead and the Agents page read no checks", async () => {
    shell.desktop = false;
    api.getQueue.mockResolvedValue({ ...LEAD_QUEUE, review: REVIEW });
    const paired = render(<Agents conversationId="shell-c1" live onAsk={() => {}} />);
    await screen.findByRole("list", { name: "Review findings" });
    expect(screen.queryByRole("heading", { name: "Checks" })).toBeNull();
    paired.unmount();

    shell.desktop = true;
    const alone = render(<Agents conversationId="shell-c1" />);
    await screen.findByRole("list", { name: "Tasks" });
    expect(screen.queryByRole("heading", { name: "Checks" })).toBeNull();
    alone.unmount();

    render(<Agents conversationId="shell-c1" onOpenChat={() => {}} onAsk={() => {}} />);
    await screen.findByRole("list", { name: "To-dos" });
    expect(screen.queryByRole("heading", { name: "Checks" })).toBeNull();
    expect(api.shipStatus).not.toHaveBeenCalled();
  });

  it("reads nothing while its tab is not the one on screen, and reads when it shows", async () => {
    api.getQueue.mockResolvedValue(QUEUE);
    const view = render(<Agents conversationId="shell-c1" active={false} />);
    await act(async () => {});
    expect(api.getQueue).not.toHaveBeenCalled();
    view.rerender(<Agents conversationId="shell-c1" active />);
    expect(await screen.findByText("Write the page")).toBeInTheDocument();
    expect(api.getQueue).toHaveBeenCalledTimes(1);
  });

  it("a live panel reads on an event, not every 2 seconds", async () => {
    vi.useFakeTimers();
    api.getQueue.mockResolvedValue({ tasks: [], todos: [] });
    const { unmount } = render(<Agents conversationId="shell-c1" live />);
    await act(async () => {});
    expect(api.onTasksChanged).toHaveBeenCalledTimes(1);
    expect(api.getQueue).toHaveBeenCalledTimes(1);
    await act(async () => {
      vi.advanceTimersByTime(2000);
    });
    expect(api.getQueue).toHaveBeenCalledTimes(1);

    await act(async () => feed.changed("shell-c1"));
    expect(api.getQueue).toHaveBeenCalledTimes(2);
    await act(async () => feed.changed("other"));
    expect(api.getQueue).toHaveBeenCalledTimes(2);

    expect(LIVE_FALLBACK_MS).toBe(15000);
    await act(async () => {
      vi.advanceTimersByTime(15000);
    });
    expect(api.getQueue).toHaveBeenCalledTimes(3);

    unmount();
    expect(feed.unlisten).toHaveBeenCalledTimes(1);
    expect(feed.unlistenGap).toHaveBeenCalledTimes(1);
    await act(async () => {
      feed.changed("shell-c1");
      vi.advanceTimersByTime(30000);
    });
    expect(api.getQueue).toHaveBeenCalledTimes(3);
  });

  it("a live panel waits for its listener before the first read, and reads again after lost events", async () => {
    api.getQueue.mockResolvedValue({ tasks: [], todos: [] });
    let listening: (unlisten: () => void) => void = () => {};
    api.onTasksChanged.mockImplementation((handler) => {
      feed.changed = handler;
      return new Promise((resolve) => {
        listening = resolve;
      });
    });
    render(<Agents conversationId="shell-c1" live />);
    await act(async () => {});
    expect(api.getQueue).not.toHaveBeenCalled();
    await act(async () => listening(feed.unlisten));
    expect(api.getQueue).toHaveBeenCalledTimes(1);
    await act(async () => feed.gap());
    expect(api.getQueue).toHaveBeenCalledTimes(2);
  });

  it("a live panel shows the newest read when an older one answers last", async () => {
    const answers: ((queue: Queue) => void)[] = [];
    api.getQueue.mockImplementation(() => new Promise((resolve) => answers.push(resolve)));
    render(<Agents conversationId="shell-c1" live />);
    await act(async () => {});
    await act(async () => feed.changed("shell-c1"));
    expect(answers).toHaveLength(2);
    await act(async () => answers[1](QUEUE));
    expect(screen.getByText("Write the page")).toBeInTheDocument();
    await act(async () => answers[0]({ tasks: [], todos: [] }));
    expect(screen.getByText("Write the page")).toBeInTheDocument();
    expect(screen.queryByText(TASKS_EMPTY)).toBeNull();
  });

  it("a panel that is not live keeps the 2 second read", async () => {
    vi.useFakeTimers();
    api.getQueue.mockResolvedValue({ tasks: [], todos: [] });
    render(<Agents conversationId="shell-c1" />);
    await act(async () => {});
    expect(api.getQueue).toHaveBeenCalledTimes(1);
    await act(async () => {
      vi.advanceTimersByTime(2000);
    });
    expect(api.getQueue).toHaveBeenCalledTimes(2);
    expect(api.onTasksChanged).not.toHaveBeenCalled();
    expect(api.onEventGap).not.toHaveBeenCalled();
  });

  it("a panel that turns live reads once more, then only on events", async () => {
    vi.useFakeTimers();
    api.getQueue.mockResolvedValue({ tasks: [], todos: [] });
    const view = render(<Agents conversationId="shell-c1" />);
    await act(async () => {});
    expect(api.getQueue).toHaveBeenCalledTimes(1);
    view.rerender(<Agents conversationId="shell-c1" live />);
    await act(async () => {});
    expect(api.getQueue).toHaveBeenCalledTimes(2);
    await act(async () => {
      vi.advanceTimersByTime(6000);
    });
    expect(api.getQueue).toHaveBeenCalledTimes(2);
  });

  it("says a helper's progress in one line", () => {
    expect(taskProgress({ steps: 3, lastStep: "read_file", tokens: 1234, seconds: 125 })).toBe(
      "Step 3: read_file · 1.2k tokens · 2m 5s",
    );
    expect(taskProgress({ steps: 0, lastStep: "", tokens: 0, seconds: 3 })).toBe("3s");
    expect(taskProgress({ steps: 1, lastStep: "shell", tokens: 950, seconds: 42 })).toBe(
      "Step 1: shell · 950 tokens · 42s",
    );
  });

  it("the Tasks panel shows a helper's progress on its task and on its own row, with one Details", async () => {
    api.getQueue.mockResolvedValue(PROGRESS_QUEUE);
    render(<Agents conversationId="shell-c1" />);

    const tasks = await screen.findByRole("list", { name: "Tasks" });
    expect(within(tasks).getAllByRole("listitem").map((row) => row.textContent)).toEqual([
      "Read the specDone",
      "Write the pageRunning · helper s2Step 3: read_file · 1.2k tokens · 2m 5s",
    ]);
    const hub = screen.getByRole("list", { name: "Helpers and commands" });
    expect(within(hub).getAllByRole("listitem").map((row) => row.textContent)).toEqual([
      "cargo test --workspaceRunning · t1Stop",
      "failed to fetch the docsFailed · s2Step 3: read_file · 1.2k tokens · 2m 5sStopDetails",
      "npm run buildFailed · t3",
      "Monitor: build logDone · m4",
    ]);
    expect(screen.getAllByRole("button", { name: /details$/ }).map((b) => b.getAttribute("aria-label"))).toEqual([
      "Helper s2 details",
    ]);
  });

  it("a helper that has ended keeps its Details and loses its Stop", async () => {
    api.getQueue.mockResolvedValue({
      todos: [],
      tasks: [
        {
          id: "s1",
          title: "write the session note",
          status: "done",
          live: false,
          progress: { steps: 1, lastStep: "read_file", tokens: 0, seconds: 61 },
        },
      ],
    });
    render(<Agents conversationId="shell-c1" />);
    const row = await screen.findByRole("listitem", { name: "write the session note" });
    expect(row.textContent).toBe("write the session noteDone · s1Step 1: read_file · 1m 1sDetails");
  });

  it("Details opens the helper's steps and result, and Close shuts it", async () => {
    api.getQueue.mockResolvedValue(PROGRESS_QUEUE);
    api.getTaskDetail.mockResolvedValue({ steps: ["read_file", "shell"], result: "the answer" });
    render(<Agents conversationId="shell-c1" />);

    const opener = await screen.findByRole("button", { name: "Helper s2 details" });
    await userEvent.click(opener);
    expect(api.getTaskDetail).toHaveBeenCalledTimes(1);
    expect(api.getTaskDetail).toHaveBeenCalledWith("shell-c1", "s2");
    const dialog = await screen.findByRole("dialog", { name: "Helper s2" });
    const steps = await within(dialog).findByRole("list", { name: "Steps" });
    expect(within(steps).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["read_file", "shell"]);
    expect(within(dialog).getByRole("region", { name: "Result" }).textContent).toBe("the answer");

    // Tab stays inside the window: Close, the result, Close again.
    const close = within(dialog).getByRole("button", { name: "Close" });
    expect(close).toHaveFocus();
    await userEvent.tab();
    expect(within(dialog).getByRole("region", { name: "Result" })).toHaveFocus();
    await userEvent.tab();
    expect(close).toHaveFocus();

    await userEvent.click(close);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(opener).toHaveFocus();
  });

  it("the details window shows a result as plain text, says when there is none, and closes on Escape", async () => {
    api.getQueue.mockResolvedValue(PROGRESS_QUEUE);
    api.getTaskDetail.mockResolvedValueOnce({ steps: [], result: "" });
    render(<Agents conversationId="shell-c1" />);

    await userEvent.click(await screen.findByRole("button", { name: "Helper s2 details" }));
    const empty = await screen.findByRole("dialog", { name: "Helper s2" });
    expect(await within(empty).findByText("No steps yet.")).toBeInTheDocument();
    expect(within(empty).getByRole("region", { name: "Result" }).textContent).toBe("No result yet.");
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();

    api.getTaskDetail.mockResolvedValueOnce({ steps: ["shell"], result: "# done\n<b>bold</b> [a](https://example.com)" });
    await userEvent.click(screen.getByRole("button", { name: "Helper s2 details" }));
    const dialog = await screen.findByRole("dialog", { name: "Helper s2" });
    const result = await within(dialog).findByRole("region", { name: "Result" });
    expect(result.textContent).toBe("# done\n<b>bold</b> [a](https://example.com)");
    expect(result.children).toHaveLength(0);
  });

  it("the details window shows a refused read", async () => {
    api.getQueue.mockResolvedValue(PROGRESS_QUEUE);
    api.getTaskDetail.mockRejectedValue("task s2 not found");
    render(<Agents conversationId="shell-c1" />);

    await userEvent.click(await screen.findByRole("button", { name: "Helper s2 details" }));
    const dialog = await screen.findByRole("dialog", { name: "Helper s2" });
    expect(await within(dialog).findByRole("status")).toHaveTextContent("task s2 not found");
    expect(within(dialog).queryByRole("region", { name: "Result" })).toBeNull();
  });

  it("the Agents page shows no progress line and no Details", async () => {
    api.getQueue.mockResolvedValue(PROGRESS_QUEUE);
    render(<Agents conversationId="shell-c1" onOpenChat={() => {}} />);

    const row = await screen.findByRole("listitem", { name: "failed to fetch the docs" });
    expect(row.textContent).toBe("failed to fetch the docsFailed · s2Stop");
    expect(screen.queryByRole("button", { name: "Helper s2 details" })).toBeNull();
    expect(document.querySelector(".task-progress")).toBeNull();
  });

  it("a queued job shows with Run, and Run sends its prompt in its chat and shows it running", async () => {
    const withJobs = {
      tasks: [],
      todos: [],
      jobs: [{ title: "Summarize the logs", status: "queued", prompt: "Summarize today's build logs", threadId: "shell-t7" }],
    } as Queue;
    api.getQueue.mockResolvedValue(withJobs);
    const onRun = vi.fn<(prompt: string, threadId: string) => void>();
    render(<Agents conversationId="shell-c1" onOpenChat={() => {}} onRun={onRun} />);
    const row = await screen.findByRole("listitem", { name: "Summarize the logs" });
    expect(within(row).getByText("queued")).toBeInTheDocument();
    expect(screen.queryByText(AGENTS_EMPTY)).toBeNull();
    await userEvent.click(within(row).getByRole("button", { name: "Run" }));
    expect(onRun).toHaveBeenCalledTimes(1);
    expect(onRun).toHaveBeenCalledWith("Summarize today's build logs", "shell-t7");
    expect(within(row).getByText("running")).toBeInTheDocument();
  });

  it("an empty queue (as in practice) shows no Queued list, and the Code panel never shows jobs", async () => {
    api.getQueue.mockResolvedValue({ tasks: [], todos: [], jobs: [] } as Queue);
    const { unmount } = render(<Agents conversationId="shell-c1" onOpenChat={() => {}} onRun={() => {}} />);
    expect(await screen.findByText(AGENTS_EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Queued" })).toBeNull();
    unmount();

    api.getQueue.mockResolvedValue({
      tasks: [],
      todos: [],
      jobs: [{ title: "Summarize the logs", status: "queued", prompt: "p", threadId: "" }],
    } as Queue);
    render(<Agents conversationId="shell-c1" />);
    expect(await screen.findByText(TASKS_EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Run" })).toBeNull();
  });
});

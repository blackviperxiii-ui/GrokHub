import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Queue, TaskDetail } from "./api";

const api = vi.hoisted(() => ({
  getQueue: vi.fn<(conversationId: string) => Promise<Queue>>(),
  stopTask: vi.fn<(conversationId: string, taskId?: string) => Promise<Queue>>(),
  onTasksChanged: vi.fn<(handler: (conversationId: string) => void) => Promise<() => void>>(),
  onEventGap: vi.fn<(handler: () => void) => Promise<() => void>>(),
  getTaskDetail: vi.fn<(conversationId: string, taskId: string) => Promise<TaskDetail>>(),
}));
// Apart from `api`, whose every value beforeEach resets: what the panel
// subscribed with, and the spies that stand for its unlisten calls.
const feed = vi.hoisted(() => ({
  changed: (() => {}) as (conversationId: string) => void,
  gap: (() => {}) as () => void,
  unlisten: vi.fn<() => void>(),
  unlistenGap: vi.fn<() => void>(),
}));

vi.mock("./api", () => api);

import {
  Agents,
  AGENTS_EMPTY,
  AGENTS_NO_CHAT,
  LEAD_LABELS,
  LIVE_FALLBACK_MS,
  taskProgress,
  TASKS_EMPTY,
  TASKS_NOTE,
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

describe("Agents", () => {
  beforeEach(() => {
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
      done: "Done",
      failed: "Failed",
    });
  });

  it("the Tasks panel of a session with nothing queued says what will show up", async () => {
    api.getQueue.mockResolvedValue({ tasks: [], todos: [] });
    render(<Agents conversationId="shell-c1" />);
    expect(await screen.findByText(TASKS_EMPTY)).toBeInTheDocument();
    expect(screen.queryByText(AGENTS_EMPTY)).not.toBeInTheDocument();
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
});

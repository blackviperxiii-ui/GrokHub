import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Queue } from "./api";

const api = vi.hoisted(() => ({
  getQueue: vi.fn<(conversationId: string) => Promise<Queue>>(),
  stopTask: vi.fn<(conversationId: string, taskId?: string) => Promise<Queue>>(),
}));

vi.mock("./api", () => api);

import { Agents, AGENTS_EMPTY, AGENTS_NO_CHAT, TASKS_EMPTY, TASKS_NOTE } from "./Agents";

const QUEUE: Queue = {
  tasks: [
    { id: "t1", title: "cargo test --workspace", status: "running", live: true },
    { id: "s2", title: "failed to fetch the docs", status: "failed", live: true },
    { id: "t3", title: "npm run build", status: "failed", live: false },
    { id: "m4", title: "Monitor: build log", status: "done", live: false },
  ],
  todos: [
    { id: "a", content: "Read the spec", status: "completed" },
    { id: "b", content: "Write the page", status: "in_progress" },
  ],
};

describe("Agents", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
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
    expect(await within(panel).findByText("Write the page")).toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: "Open chat" })).not.toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: "Write the page" })).not.toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: "cargo test --workspace" })).not.toBeInTheDocument();
    const live = within(panel).getByRole("listitem", { name: "cargo test --workspace" });
    await userEvent.click(within(live).getByRole("button", { name: "Stop" }));
    expect(api.stopTask).toHaveBeenCalledWith("shell-c1", "t1");
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

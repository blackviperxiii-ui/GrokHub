import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Queue, QueueTask } from "./api";

const api = vi.hoisted(() => ({
  getQueue: vi.fn<(conversationId: string) => Promise<Queue>>(),
  stopTask: vi.fn<(conversationId: string, taskId?: string) => Promise<Queue>>(),
  steerTask: vi.fn<(conversationId: string, taskId: string, text: string) => Promise<Queue>>(),
  onTasksChanged: vi.fn<(handler: (conversationId: string) => void) => Promise<() => void>>(),
}));
vi.mock("./api", () => api);

import { HelperStrip } from "./HelperStrip";

const helper = (id: string, live = true): QueueTask => ({
  id,
  title: `Helper ${id}`,
  status: live ? "running" : "done",
  live,
  model: "grok-4",
  progress: { steps: 3, lastStep: "read", tokens: 900, seconds: 12 },
});

const queue = (tasks: QueueTask[]): Queue => ({ todos: [], tasks });

let changed: ((conversationId: string) => void) | undefined;

describe("HelperStrip", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    changed = undefined;
    api.onTasksChanged.mockImplementation((handler) => {
      changed = handler;
      return Promise.resolve(() => {});
    });
  });

  it("shows nothing while no helper runs, and ignores commands", async () => {
    const command: QueueTask = { id: "b1", title: "npm test", status: "running", live: true };
    api.getQueue.mockResolvedValue(queue([command, helper("s1", false)]));
    const { container } = render(<HelperStrip conversationId="c1" />);
    await waitFor(() => expect(api.getQueue).toHaveBeenCalledWith("c1"));
    expect(container).toBeEmptyDOMElement();
  });

  it("lists running helpers with their model and progress", async () => {
    api.getQueue.mockResolvedValue(queue([helper("s1"), helper("s2")]));
    render(<HelperStrip conversationId="c1" />);
    expect(await screen.findByText("Helper s1")).toBeInTheDocument();
    expect(screen.getByText("Helper s2")).toBeInTheDocument();
    expect(screen.getAllByText("grok-4")).toHaveLength(2);
    expect(screen.getAllByText("3 steps, last read")).toHaveLength(2);
  });

  it("steers on Enter, clears the box and closes it", async () => {
    const user = userEvent.setup();
    api.getQueue.mockResolvedValue(queue([helper("s1")]));
    api.steerTask.mockResolvedValue(queue([helper("s1")]));
    render(<HelperStrip conversationId="c1" />);
    await user.click(await screen.findByRole("button", { name: "Steer Helper s1" }));
    const box = screen.getByRole("textbox", { name: "What Helper s1 should do next" });
    expect(box).toHaveFocus();
    await user.type(box, "  check the tests first {Enter}");
    expect(api.steerTask).toHaveBeenCalledWith("c1", "s1", "check the tests first");
    await waitFor(() => expect(screen.queryByRole("textbox")).not.toBeInTheDocument());
  });

  it("sends nothing for an empty steer and says why a steer failed", async () => {
    const user = userEvent.setup();
    api.getQueue.mockResolvedValue(queue([helper("s1")]));
    api.steerTask.mockRejectedValue("helper s1 is not running");
    render(<HelperStrip conversationId="c1" />);
    await user.click(await screen.findByRole("button", { name: "Steer Helper s1" }));
    const box = screen.getByRole("textbox", { name: "What Helper s1 should do next" });
    await user.type(box, "   {Enter}");
    expect(api.steerTask).not.toHaveBeenCalled();
    await user.type(box, "go{Enter}");
    expect(await screen.findByRole("status")).toHaveTextContent("helper s1 is not running");
  });

  it("stops a helper, and drops it once the queue says it finished", async () => {
    const user = userEvent.setup();
    api.getQueue.mockResolvedValue(queue([helper("s1"), helper("s2")]));
    api.stopTask.mockResolvedValue(queue([helper("s2")]));
    render(<HelperStrip conversationId="c1" />);
    await user.click(await screen.findByRole("button", { name: "Stop Helper s1" }));
    expect(api.stopTask).toHaveBeenCalledWith("c1", "s1");
    await waitFor(() => expect(screen.queryByText("Helper s1")).not.toBeInTheDocument());
    api.getQueue.mockResolvedValue(queue([helper("s2", false)]));
    await act(async () => changed?.("c1"));
    await waitFor(() => expect(screen.queryByText("Helper s2")).not.toBeInTheDocument());
  });

  it("reads again only when this chat's tasks change", async () => {
    api.getQueue.mockResolvedValue(queue([helper("s1")]));
    render(<HelperStrip conversationId="c1" />);
    await screen.findByText("Helper s1");
    await act(async () => changed?.("other"));
    expect(api.getQueue).toHaveBeenCalledTimes(1);
    await act(async () => changed?.("c1"));
    expect(api.getQueue).toHaveBeenCalledTimes(2);
  });
});

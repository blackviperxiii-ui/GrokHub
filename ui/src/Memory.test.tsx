import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MemoryHit, MemoryNotes } from "./api";

const api = vi.hoisted(() => ({
  getMemory: vi.fn<() => Promise<MemoryNotes>>(),
  saveMemory: vi.fn<
    (
      global: boolean,
      text: string,
      base: string,
    ) => Promise<{ notes: MemoryNotes; redacted: boolean }>
  >(),
  addMemory: vi.fn<
    (text: string, global: boolean) => Promise<{ status: string; notes: MemoryNotes }>
  >(),
  searchMemory: vi.fn<(query: string) => Promise<MemoryHit[]>>(),
  editMemoryRow: vi.fn<(id: string, text: string) => Promise<MemoryNotes>>(),
  forgetMemoryRow: vi.fn<(id: string) => Promise<MemoryNotes>>(),
  approveMemoryRow: vi.fn<(id: string) => Promise<MemoryNotes>>(),
  rejectMemoryRow: vi.fn<(id: string) => Promise<MemoryNotes>>(),
  setMemoryWrites: vi.fn<(on: boolean) => Promise<MemoryNotes>>(),
}));

vi.mock("./api", () => api);

import { Memory } from "./Memory";

function notes(over: Partial<MemoryNotes> = {}): MemoryNotes {
  return {
    global: "I like short answers\n",
    workspace: "the build needs node 22\n",
    globalPath: "/cfg/memory/MEMORY.md",
    workspacePath: "/ws/MEMORY.md",
    amr: false,
    writesOn: true,
    rows: [],
    ...over,
  };
}

const LUNCH = {
  id: "mem-lunch",
  text: "Lunch is at noon on Friday",
  source: "you said it in chat (turn 2)",
  updated: "2026-10-07T00:00:00Z",
  proposed: true,
  corrects: "Lunch is at noon on Monday",
};

describe("Memory", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("shows both files with their paths", async () => {
    api.getMemory.mockResolvedValue(notes());
    render(<Memory />);
    expect(await screen.findByLabelText("Project notes")).toHaveValue("the build needs node 22\n");
    expect(screen.getByLabelText("Global notes")).toHaveValue("I like short answers\n");
    expect(screen.getByText("/ws/MEMORY.md")).toBeInTheDocument();
    expect(screen.getByText("/cfg/memory/MEMORY.md")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save project notes" })).toBeDisabled();
    expect(screen.queryByText(/AMR store/)).toBeNull();
  });

  it("saves an edit and says when a secret was redacted", async () => {
    api.getMemory.mockResolvedValue(notes());
    api.saveMemory.mockResolvedValue({
      notes: notes({ workspace: "token [redacted]\n" }),
      redacted: true,
    });
    render(<Memory />);
    const box = await screen.findByLabelText("Project notes");
    await userEvent.clear(box);
    await userEvent.type(box, "token sk-abcdefghijklmnopqrstuv");
    await userEvent.click(screen.getByRole("button", { name: "Save project notes" }));

    expect(api.saveMemory).toHaveBeenCalledWith(
      false,
      "token sk-abcdefghijklmnopqrstuv",
      "the build needs node 22\n",
    );
    expect(await screen.findByText("Saved. A secret was replaced with [redacted].")).toBeInTheDocument();
    expect(box).toHaveValue("token [redacted]\n");
  });

  it("a save refused because chat changed the file shows the new text", async () => {
    api.getMemory
      .mockResolvedValueOnce(notes())
      .mockResolvedValueOnce(notes({ global: "I like short answers\nadded from chat\n" }));
    api.saveMemory.mockRejectedValue("This file changed since you opened it, maybe from chat.");
    render(<Memory />);
    const box = await screen.findByLabelText("Global notes");
    await userEvent.type(box, "mine");
    await userEvent.click(screen.getByRole("button", { name: "Save global notes" }));

    expect(api.saveMemory).toHaveBeenCalledWith(
      true,
      "I like short answers\nmine",
      "I like short answers\n",
    );
    expect(
      await screen.findByText("This file changed since you opened it, maybe from chat."),
    ).toBeInTheDocument();
    await waitFor(() => expect(box).toHaveValue("I like short answers\nadded from chat\n"));
  });

  it("Remember adds a global note and shows the status", async () => {
    api.getMemory.mockResolvedValue(notes());
    api.addMemory.mockResolvedValue({
      status: "Remembered in global MEMORY.md",
      notes: notes({ global: "I like short answers\ndeploys on fridays\n" }),
    });
    render(<Memory />);
    await userEvent.type(await screen.findByLabelText("New note"), " deploys on fridays ");
    await userEvent.click(screen.getByRole("switch", { name: "Global" }));
    await userEvent.click(screen.getByRole("button", { name: "Remember" }));

    expect(api.addMemory).toHaveBeenCalledWith("deploys on fridays", true);
    expect(await screen.findByText("Remembered in global MEMORY.md.")).toBeInTheDocument();
    expect(screen.getByLabelText("Global notes")).toHaveValue(
      "I like short answers\ndeploys on fridays\n",
    );
    expect(screen.getByLabelText("New note")).toHaveValue("");
  });

  it("Remember status stays distinct from the AMR banner", async () => {
    api.getMemory.mockResolvedValue(notes({ amr: true }));
    api.addMemory.mockResolvedValue({
      status: "Remembered in workspace MEMORY.md",
      notes: notes({ amr: true, workspace: "the smoke test likes kumquats\n" }),
    });
    render(<Memory />);
    expect(await screen.findByText(/AMR store/)).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("New note"), "the smoke test likes kumquats");
    await userEvent.click(screen.getByRole("button", { name: "Remember" }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Remembered in workspace MEMORY.md.",
    );
  });

  it("Remember keeps an unsaved edit in the other editor", async () => {
    api.getMemory.mockResolvedValue(notes());
    api.addMemory.mockResolvedValue({
      status: "Remembered in global MEMORY.md",
      notes: notes({ global: "I like short answers\nnew\n" }),
    });
    render(<Memory />);
    const project = await screen.findByLabelText("Project notes");
    await userEvent.type(project, "half typed");
    await userEvent.type(screen.getByLabelText("New note"), "new");
    await userEvent.click(screen.getByRole("switch", { name: "Global" }));
    await userEvent.click(screen.getByRole("button", { name: "Remember" }));

    await waitFor(() =>
      expect(screen.getByLabelText("Global notes")).toHaveValue("I like short answers\nnew\n"),
    );
    expect(project).toHaveValue("the build needs node 22\nhalf typed");
    expect(screen.getByRole("button", { name: "Save project notes" })).toBeEnabled();
  });

  it("Save is off while a save is in flight", async () => {
    api.getMemory.mockResolvedValue(notes());
    let finish: () => void = () => {};
    api.saveMemory.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = () => resolve({ notes: notes({ workspace: "edited\n" }), redacted: false });
        }),
    );
    render(<Memory />);
    const box = await screen.findByLabelText("Project notes");
    await userEvent.clear(box);
    await userEvent.type(box, "edited");
    await userEvent.click(screen.getByRole("button", { name: "Save project notes" }));
    expect(screen.getByRole("button", { name: "Save project notes" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Save project notes" }));
    expect(api.saveMemory).toHaveBeenCalledTimes(1);

    finish();
    expect(await screen.findByText("Saved.")).toBeInTheDocument();
    expect(box).toHaveValue("edited\n");
  });

  it("search lists hits by scope and says when nothing matches", async () => {
    api.getMemory.mockResolvedValue(notes({ amr: true }));
    api.searchMemory
      .mockResolvedValueOnce([{ scope: "workspace", body: "the build needs node 22" }])
      .mockResolvedValueOnce([]);
    render(<Memory />);
    expect(await screen.findByText(/AMR store/)).toBeInTheDocument();

    await userEvent.type(screen.getByLabelText("Search memory"), "node{Enter}");
    expect(api.searchMemory).toHaveBeenCalledWith("node");
    const results = await screen.findByRole("list", { name: "Search results" });
    expect(results).toHaveTextContent("workspace");
    expect(results).toHaveTextContent("the build needs node 22");

    await userEvent.clear(screen.getByLabelText("Search memory"));
    await userEvent.type(screen.getByLabelText("Search memory"), "python{Enter}");
    expect(await screen.findByText("Nothing matches.")).toBeInTheDocument();
  });

  it("a load error is shown instead of the editors", async () => {
    api.getMemory.mockRejectedValue("notes.md is too large to edit here.");
    render(<Memory />);
    expect(await screen.findByText("notes.md is too large to edit here.")).toBeInTheDocument();
    expect(screen.queryByLabelText("Project notes")).toBeNull();
  });

  it("shows a learned line's source and date and can edit, delete, approve, or reject it", async () => {
    const learned = notes({ rows: [LUNCH] });
    api.getMemory.mockResolvedValue(learned);
    api.approveMemoryRow.mockResolvedValue(learned);
    api.rejectMemoryRow.mockResolvedValue(learned);
    api.editMemoryRow.mockResolvedValue(learned);
    api.forgetMemoryRow.mockResolvedValue(notes());
    api.setMemoryWrites.mockResolvedValue(notes({ writesOn: false }));
    render(<Memory />);
    const list = await screen.findByRole("list", { name: "Learned lines" });
    expect(list).toHaveTextContent("Lunch is at noon on Friday");
    expect(list).toHaveTextContent("you said it in chat (turn 2)");
    expect(list).toHaveTextContent("2026-10-07T00:00:00Z");
    expect(list).toHaveTextContent("Corrects: Lunch is at noon on Monday");
    expect(list).toHaveTextContent("Waiting for approval");

    await userEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(api.approveMemoryRow).toHaveBeenCalledWith("mem-lunch");
    expect(await screen.findByText("Approved the line.")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Edit Lunch is at noon on Friday" }));
    const box = screen.getByLabelText("Edit text for Lunch is at noon on Friday");
    await userEvent.clear(box);
    await userEvent.type(box, "Lunch is at 1");
    await userEvent.click(screen.getByRole("button", { name: "Save line" }));
    expect(api.editMemoryRow).toHaveBeenCalledWith("mem-lunch", "Lunch is at 1");

    await userEvent.click(screen.getByRole("button", { name: "Reject" }));
    expect(api.rejectMemoryRow).toHaveBeenCalledWith("mem-lunch");

    await userEvent.click(screen.getByRole("button", { name: "Delete Lunch is at noon on Friday" }));
    expect(api.forgetMemoryRow).toHaveBeenCalledWith("mem-lunch");

    await userEvent.click(screen.getByRole("switch", { name: "Remember new lines" }));
    expect(api.setMemoryWrites).toHaveBeenCalledWith(false);
    expect(await screen.findByText("Memory is off. Nothing new is written.")).toBeInTheDocument();
  });
});

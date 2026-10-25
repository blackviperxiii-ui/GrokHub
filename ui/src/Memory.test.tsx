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
    ...over,
  };
}

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
    await userEvent.click(screen.getByRole("checkbox", { name: "Global" }));
    await userEvent.click(screen.getByRole("button", { name: "Remember" }));

    expect(api.addMemory).toHaveBeenCalledWith("deploys on fridays", true);
    expect(await screen.findByText("Remembered in global MEMORY.md.")).toBeInTheDocument();
    expect(screen.getByLabelText("Global notes")).toHaveValue(
      "I like short answers\ndeploys on fridays\n",
    );
    expect(screen.getByLabelText("New note")).toHaveValue("");
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
});

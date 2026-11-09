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
  pauseMemoryRow: vi.fn<(id: string, paused: boolean) => Promise<MemoryNotes>>(),
  addPersonalFact: vi.fn<(text: string, group: string) => Promise<MemoryNotes>>(),
  exportPersonal: vi.fn<() => Promise<string>>(),
  wipePersonal:
    vi.fn<() => Promise<{ removed: number; locked: number; notes: MemoryNotes }>>(),
}));

vi.mock("./api", () => ({
  ...api,
  PERSONAL_ADD_GROUPS: [
    ["about", "About you"],
    ["likes", "What you like"],
    ["routines", "Your routines"],
    ["needs", "What you need"],
    ["thinking", "How you like things done"],
  ],
}));

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
  paused: false,
  group: "Waiting for your OK",
  corrects: "Lunch is at noon on Monday",
};

const DOG = {
  id: "mem-dog",
  text: "Has a dog called Biscuit",
  source: "you wrote it",
  updated: "2026-11-01T00:00:00Z",
  proposed: false,
  paused: false,
  group: "About you",
  corrects: "",
};

const NIGHTS = {
  id: "mem-nights",
  text: "Plays after the kids are in bed",
  source: "you said it in chat",
  updated: "2026-11-02T00:00:00Z",
  proposed: false,
  paused: true,
  group: "Your routines",
  corrects: "",
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

  it("a waiting line shows where it came from and takes OK or No", async () => {
    const learned = notes({ rows: [LUNCH] });
    api.getMemory.mockResolvedValue(learned);
    api.approveMemoryRow.mockResolvedValue(learned);
    api.rejectMemoryRow.mockResolvedValue(learned);
    api.setMemoryWrites.mockResolvedValue(notes({ writesOn: false }));
    render(<Memory />);
    const list = await screen.findByRole("list", { name: "Waiting for your OK" });
    expect(list).toHaveTextContent("Lunch is at noon on Friday");
    expect(list).toHaveTextContent("Waiting for your OK · you said it in chat (turn 2) · 2026-10-07");
    expect(list).toHaveTextContent("Corrects: Lunch is at noon on Monday");

    await userEvent.click(screen.getByRole("button", { name: "OK Lunch is at noon on Friday" }));
    expect(api.approveMemoryRow).toHaveBeenCalledWith("mem-lunch");
    expect(await screen.findByText("Saved.")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "No Lunch is at noon on Friday" }));
    expect(api.rejectMemoryRow).toHaveBeenCalledWith("mem-lunch");

    await userEvent.click(screen.getByRole("switch", { name: "Learn from chats" }));
    expect(api.setMemoryWrites).toHaveBeenCalledWith(false);
    expect(
      await screen.findByText("Not learning from chats. Nothing new is saved."),
    ).toBeInTheDocument();
  });

  it("groups lines, searches them, and edits, pauses and forgets one", async () => {
    const learned = notes({ rows: [NIGHTS, DOG] });
    api.getMemory.mockResolvedValue(learned);
    api.editMemoryRow.mockResolvedValue(learned);
    api.pauseMemoryRow.mockResolvedValue(learned);
    api.forgetMemoryRow.mockResolvedValue(notes());
    render(<Memory />);
    const about = await screen.findByRole("list", { name: "About you" });
    const routines = screen.getByRole("list", { name: "Your routines" });
    // About you comes before Your routines, whatever order the rows arrive in.
    expect(
      about.compareDocumentPosition(routines) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(routines).toHaveTextContent("Paused, not used · you said it in chat · 2026-11-02");

    await userEvent.type(screen.getByLabelText("Search what it knows"), "biscuit");
    expect(screen.queryByRole("list", { name: "Your routines" })).toBeNull();
    await userEvent.clear(screen.getByLabelText("Search what it knows"));
    await userEvent.type(screen.getByLabelText("Search what it knows"), "zebra");
    expect(screen.getByText("None matched.")).toBeInTheDocument();
    await userEvent.clear(screen.getByLabelText("Search what it knows"));

    await userEvent.click(screen.getByRole("button", { name: "Edit Has a dog called Biscuit" }));
    const box = screen.getByLabelText("Edit text for Has a dog called Biscuit");
    await userEvent.clear(box);
    await userEvent.type(box, "Has two dogs");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.editMemoryRow).toHaveBeenCalledWith("mem-dog", "Has two dogs");

    await userEvent.click(screen.getByRole("button", { name: "Don't use Has a dog called Biscuit" }));
    expect(api.pauseMemoryRow).toHaveBeenCalledWith("mem-dog", true);
    await userEvent.click(
      screen.getByRole("button", { name: "Use again Plays after the kids are in bed" }),
    );
    expect(api.pauseMemoryRow).toHaveBeenCalledWith("mem-nights", false);
    expect(
      screen.queryByRole("button", { name: "Edit Plays after the kids are in bed" }),
      "a paused line is used again before it is edited",
    ).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "Forget Has a dog called Biscuit" }));
    expect(api.forgetMemoryRow).toHaveBeenCalledWith("mem-dog");
    expect(await screen.findByText(/Nothing learned yet/)).toBeInTheDocument();
  });

  it("adds a line in your words under the group you pick", async () => {
    api.getMemory.mockResolvedValue(notes());
    api.addPersonalFact.mockResolvedValue(notes({ rows: [NIGHTS] }));
    render(<Memory />);
    await userEvent.selectOptions(await screen.findByLabelText("Group"), "routines");
    await userEvent.type(
      screen.getByLabelText("Add something about you"),
      "Plays after the kids are in bed",
    );
    await userEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(api.addPersonalFact).toHaveBeenCalledWith("Plays after the kids are in bed", "routines");
    expect(await screen.findByRole("list", { name: "Your routines" })).toBeInTheDocument();
    expect(screen.getByLabelText("Add something about you")).toHaveValue("");
  });

  it("export and wipe stay off the web", async () => {
    api.getMemory.mockResolvedValue(notes({ rows: [DOG] }));
    render(<Memory />);
    await screen.findByRole("list", { name: "About you" });
    expect(screen.queryByRole("button", { name: "Export to Downloads" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Wipe everything" })).toBeNull();
  });

  it("on the desktop, export says where the copy went and wipe asks first", async () => {
    const w = window as unknown as Record<string, unknown>;
    w.__TAURI_INTERNALS__ = {};
    try {
      api.getMemory.mockResolvedValue(notes({ rows: [DOG] }));
      api.exportPersonal.mockResolvedValue("/home/me/Downloads/what it knows.md");
      api.wipePersonal.mockResolvedValue({
        removed: 1,
        locked: 0,
        notes: notes({ global: "# MEMORY.md\n" }),
      });
      render(<Memory />);
      await userEvent.click(await screen.findByRole("button", { name: "Export to Downloads" }));
      expect(
        await screen.findByText("Saved to /home/me/Downloads/what it knows.md."),
      ).toBeInTheDocument();

      await userEvent.click(screen.getByRole("button", { name: "Wipe everything" }));
      expect(api.wipePersonal).not.toHaveBeenCalled();
      expect(screen.getByRole("group", { name: "Wipe everything?" })).toHaveTextContent("export first");
      await userEvent.click(screen.getByRole("button", { name: "Wipe for good" }));
      expect(api.wipePersonal).toHaveBeenCalledTimes(1);
      expect(await screen.findByText("Wiped 1 lines. Personal is empty.")).toBeInTheDocument();
      expect(screen.getByLabelText("Global notes")).toHaveValue("# MEMORY.md\n");
    } finally {
      delete w.__TAURI_INTERNALS__;
    }
  });
});

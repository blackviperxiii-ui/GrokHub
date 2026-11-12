import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChangeAnswer, ChangeRowsView } from "./changeRowsApi";

const api = vi.hoisted(() => ({
  getChangeRows: vi.fn<() => Promise<ChangeRowsView>>(),
  changeRowUndo: vi.fn<(kind: string, id: string) => Promise<ChangeAnswer>>(),
  changeRowKeep: vi.fn<(kind: string, id: string) => Promise<ChangeAnswer>>(),
  changeRowRestore: vi.fn<(kind: string, id: string) => Promise<ChangeAnswer>>(),
}));
vi.mock("./changeRowsApi", () => api);

import { ChangeRows } from "./ChangeRows";

const HEAD = "What Loadout changed on its own";

function view(over: Partial<ChangeRowsView> = {}): ChangeRowsView {
  return {
    heading: HEAD,
    empty: "Nothing to undo. When Loadout changes a skill, connection or automation on its own, it shows here with Undo and Keep.",
    groups: [
      { kind: "skill", title: "Skills", rows: [{ id: "weekly-report", label: "weekly-report · added 2m ago", act: "undo", keep: true }] },
      { kind: "connection", title: "Connections", rows: [{ id: "notes", label: "notes · removed 1d ago", act: "restore", keep: false }] },
    ],
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  api.getChangeRows.mockResolvedValue(view());
});

describe("ChangeRows", () => {
  it("lists each kind's rows with Undo, Keep only while open, and Restore", async () => {
    render(<ChangeRows />);
    const skills = within(await screen.findByRole("list", { name: "Skills" }));
    const row = within(skills.getByRole("listitem", { name: "weekly-report · added 2m ago" }));
    expect(row.getByRole("button", { name: "Undo" })).toBeInTheDocument();
    expect(row.getByRole("button", { name: "Keep" })).toBeInTheDocument();
    const conn = within(screen.getByRole("list", { name: "Connections" }));
    expect(conn.getByRole("button", { name: "Restore" })).toBeInTheDocument();
    expect(conn.queryByRole("button", { name: "Keep" })).toBeNull();
  });

  it("asks before Undo and sends the kind and id", async () => {
    api.changeRowUndo.mockResolvedValue({
      view: view({ groups: [] }),
      status: "Removed weekly-report: it was new. Its text is kept, and /skills restore weekly-report brings it back.",
    });
    render(<ChangeRows />);
    await userEvent.click(await screen.findByRole("button", { name: "Undo" }));
    expect(api.changeRowUndo).not.toHaveBeenCalled();
    const ask = within(screen.getByRole("group", { name: "Undo weekly-report?" }));
    await userEvent.click(ask.getByRole("button", { name: "Undo" }));
    expect(api.changeRowUndo).toHaveBeenCalledWith("skill", "weekly-report");
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Removed weekly-report: it was new. Its text is kept, and /skills restore weekly-report brings it back.",
    );
    expect(screen.getByText(view().empty)).toBeInTheDocument();
  });

  it("restores after asking", async () => {
    api.changeRowRestore.mockResolvedValue({ view: view(), status: "Restored notes from its kept copy." });
    render(<ChangeRows />);
    await userEvent.click(await screen.findByRole("button", { name: "Restore" }));
    await userEvent.click(within(screen.getByRole("group", { name: "Restore notes?" })).getByRole("button", { name: "Restore" }));
    expect(api.changeRowRestore).toHaveBeenCalledWith("connection", "notes");
  });

  it("keeps from a click, never from the keyboard", async () => {
    api.changeRowKeep.mockResolvedValue({ view: view(), status: "Kept the skill weekly-report." });
    render(<ChangeRows />);
    const keepBtn = await screen.findByRole("button", { name: "Keep" });
    keepBtn.focus();
    await userEvent.keyboard("{Enter}");
    await userEvent.keyboard(" ");
    expect(api.changeRowKeep).not.toHaveBeenCalled();
    await userEvent.click(keepBtn);
    expect(api.changeRowKeep).toHaveBeenCalledWith("skill", "weekly-report");
    expect(await screen.findByRole("status")).toHaveTextContent("Kept the skill weekly-report.");
  });

  it("says when nothing changed", async () => {
    api.getChangeRows.mockResolvedValue(view({ groups: [] }));
    render(<ChangeRows />);
    expect(await screen.findByText(view().empty)).toBeInTheDocument();
  });
});

import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PlaybookPage, PlaybookRow } from "./api";

const api = vi.hoisted(() => ({
  getPlaybooks: vi.fn<() => Promise<PlaybookPage>>(),
  savePlaybook: vi.fn<(text: string) => Promise<[string, PlaybookPage]>>(),
  duplicatePlaybook: vi.fn<(id: string) => Promise<[string, PlaybookPage]>>(),
  resetPlaybook: vi.fn<(id: string) => Promise<PlaybookPage>>(),
}));

vi.mock("./api", () => api);

import { Playbooks, PLAYBOOKS_NOTE } from "./Playbooks";

const FILES: PlaybookRow = {
  id: "files",
  name: "Files",
  summary: "List what will change before touching anything.",
  source: "shipped",
  fits: "Fits task · files",
  steps: "6 steps, up to Risky, asks first",
  text: 'id = "files"\nname = "Files"\n',
};
const COPY: PlaybookRow = {
  ...FILES,
  id: "files-copy",
  name: "Files (copy)",
  source: "yours",
  text: 'id = "files-copy"\nname = "Files (copy)"\n',
};

function page(rows: PlaybookRow[], problems: string[] = []): PlaybookPage {
  return { rows, problems };
}

function card(name: string) {
  const list = within(screen.getByRole("list", { name: "All playbooks" }));
  return within(list.getByRole("listitem", { name }));
}

describe("Playbooks", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("lists a built-in with what it fits and offers no Reset for it", async () => {
    api.getPlaybooks.mockResolvedValue(page([FILES], ["bad.toml: missing `id`"]));
    render(<Playbooks />);
    expect(await screen.findByText(PLAYBOOKS_NOTE)).toBeVisible();
    await screen.findByRole("list", { name: "All playbooks" });
    expect(card("Files").getByText("Built in")).toBeVisible();
    expect(card("Files").getByText("Fits task · files · 6 steps, up to Risky, asks first")).toBeVisible();
    expect(card("Files").getByRole("button", { name: "View / Edit" })).toBeVisible();
    expect(card("Files").queryByRole("button", { name: "Reset" })).toBeNull();
    expect(screen.getByText("Skipped bad.toml: missing `id`")).toBeVisible();
  });

  it("duplicates into the editor, keeps it open on a refused save, then saves and removes", async () => {
    api.getPlaybooks.mockResolvedValue(page([FILES]));
    api.duplicatePlaybook.mockResolvedValue(["files-copy", page([FILES, COPY])]);
    const tidy = { ...COPY, name: "Tidy files" };
    api.savePlaybook
      .mockRejectedValueOnce("Not saved: a step that deletes must be risky")
      .mockResolvedValueOnce(["Tidy files", page([FILES, tidy])]);
    api.resetPlaybook.mockResolvedValue(page([FILES]));
    render(<Playbooks />);
    await screen.findByRole("list", { name: "All playbooks" });

    await userEvent.click(card("Files").getByRole("button", { name: "Duplicate" }));
    expect(api.duplicatePlaybook).toHaveBeenCalledWith("files");
    expect(screen.getByRole("status")).toHaveTextContent("Made Files (copy).");
    const editor = within(screen.getByRole("form", { name: "Edit Files (copy)" }));
    const box = editor.getByLabelText("Playbook TOML");
    expect(box).toHaveValue(COPY.text);

    await userEvent.click(editor.getByRole("button", { name: "Save" }));
    expect(api.savePlaybook).toHaveBeenLastCalledWith(COPY.text);
    expect(editor.getByText("Not saved: a step that deletes must be risky")).toBeVisible();
    expect(screen.getByRole("form", { name: "Edit Files (copy)" })).toBeVisible();

    await userEvent.type(box, "# tidy");
    await userEvent.click(editor.getByRole("button", { name: "Save" }));
    expect(api.savePlaybook).toHaveBeenLastCalledWith(`${COPY.text}# tidy`);
    expect(screen.queryByRole("form")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("Saved playbook Tidy files.");
    expect(card("Tidy files").getByText("Yours")).toBeVisible();

    await userEvent.click(card("Tidy files").getByRole("button", { name: "Remove" }));
    expect(api.resetPlaybook).toHaveBeenCalledWith("files-copy");
    expect(screen.getByRole("status")).toHaveTextContent("Removed Tidy files.");
  });

  it("resets an edited built-in back to the shipped one", async () => {
    api.getPlaybooks.mockResolvedValue(page([{ ...FILES, source: "edited", name: "My files" }]));
    api.resetPlaybook.mockResolvedValue(page([FILES]));
    render(<Playbooks />);
    await screen.findByRole("list", { name: "All playbooks" });
    expect(card("My files").getByText("Edited")).toBeVisible();
    await userEvent.click(card("My files").getByRole("button", { name: "Reset" }));
    expect(api.resetPlaybook).toHaveBeenCalledWith("files");
    expect(screen.getByRole("status")).toHaveTextContent("My files is back to the built-in one.");
    expect(card("Files").getByText("Built in")).toBeVisible();
  });
});

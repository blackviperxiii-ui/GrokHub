import "@testing-library/jest-dom/vitest";
import { Activity } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EditorView } from "@codemirror/view";
import type { CodeFileListing, CodeFileText } from "./api";

const api = vi.hoisted(() => ({
  codeFilesList: vi.fn<(path: string) => Promise<CodeFileListing>>(),
  codeFileRead: vi.fn<(path: string) => Promise<CodeFileText>>(),
  codeFileWrite: vi.fn<(path: string, text: string) => Promise<CodeFileText>>(),
}));

vi.mock("./api", () => api);

import CodeEditor, { EDITOR_DISCARD, EDITOR_EMPTY, languageFor, parentOf } from "./CodeEditor";

const ROOT: CodeFileListing = {
  path: "",
  entries: [
    { name: "src", path: "src", dir: true },
    { name: "README.md", path: "README.md", dir: false },
  ],
  more: false,
};
const SRC: CodeFileListing = {
  path: "src",
  entries: [{ name: "main.rs", path: "src/main.rs", dir: false }],
  more: false,
};

/** The live CodeMirror view, found through its public DOM lookup. */
function editorView(): EditorView {
  const view = EditorView.findFromDOM(document.querySelector(".cm-editor") as HTMLElement);
  if (!view) throw new Error("no editor on screen");
  return view;
}

function editorText(): string {
  return document.querySelector(".cm-content")?.textContent ?? "";
}

describe("Code tab → Editor", () => {
  beforeEach(() => {
    api.codeFilesList.mockReset();
    api.codeFileRead.mockReset();
    api.codeFileWrite.mockReset();
    api.codeFilesList.mockImplementation((p) => Promise.resolve(p === "src" ? SRC : ROOT));
    api.codeFileRead.mockImplementation((p) =>
      Promise.resolve({ path: p, text: p === "README.md" ? "# Demo\n" : "fn main() {}\n" }),
    );
    api.codeFileWrite.mockImplementation((p, text) => Promise.resolve({ path: p, text }));
  });

  it("lists the project root, walks into a folder and back up", async () => {
    render(<CodeEditor />);
    expect(await screen.findByRole("button", { name: "src/" })).toBeInTheDocument();
    expect(screen.getByText(EDITOR_EMPTY)).toBeInTheDocument();
    expect(api.codeFilesList).toHaveBeenCalledWith("");
    await userEvent.click(screen.getByRole("button", { name: "src/" }));
    expect(await screen.findByRole("button", { name: "main.rs" })).toBeInTheDocument();
    expect(api.codeFilesList).toHaveBeenLastCalledWith("src");
    await userEvent.click(screen.getByRole("button", { name: /up from src/ }));
    expect(await screen.findByRole("button", { name: "README.md" })).toBeInTheDocument();
  });

  it("opens a file, edits it and saves the exact new text", async () => {
    render(<CodeEditor />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    await waitFor(() => expect(editorText()).toBe("# Demo"));
    expect(api.codeFileRead).toHaveBeenCalledWith("README.md");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    const view = editorView();
    view.dispatch({ changes: { from: view.state.doc.length, insert: "More text.\n" } });

    expect(await screen.findByText("· unsaved")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.codeFileWrite).toHaveBeenCalledWith("README.md", "# Demo\nMore text.\n"));
    expect(await screen.findByText("Saved README.md.")).toBeInTheDocument();
    expect(screen.queryByText("· unsaved")).not.toBeInTheDocument();
  });

  it("asks before throwing away unsaved edits when another file is picked", async () => {
    render(<CodeEditor />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    await waitFor(() => expect(editorText()).toBe("# Demo"));
    const view = editorView();
    view.dispatch({ changes: { from: 0, insert: "x" } });
    await screen.findByText("· unsaved");

    await userEvent.click(screen.getByRole("button", { name: "src/" }));
    await userEvent.click(await screen.findByRole("button", { name: "main.rs" }));
    expect(screen.getByRole("group", { name: EDITOR_DISCARD })).toBeInTheDocument();
    expect(api.codeFileRead).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Keep editing" }));
    expect(screen.queryByRole("group", { name: EDITOR_DISCARD })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "main.rs" }));
    await userEvent.click(screen.getByRole("button", { name: "Discard and open src/main.rs" }));
    await waitFor(() => expect(editorText()).toBe("fn main() {}"));
    expect(api.codeFileWrite).not.toHaveBeenCalled();
  });

  it("keeps unsaved edits while the Code page is hidden", async () => {
    function Page({ shown }: { shown: boolean }) {
      return (
        <Activity mode={shown ? "visible" : "hidden"}>
          <CodeEditor />
        </Activity>
      );
    }
    const { rerender } = render(<Page shown />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    await waitFor(() => expect(editorText()).toBe("# Demo"));
    editorView().dispatch({ changes: { from: 0, insert: "Draft " } });
    rerender(<Page shown={false} />);
    rerender(<Page shown />);
    await waitFor(() => expect(editorText()).toBe("Draft # Demo"));
    expect(screen.getByText("· unsaved")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.codeFileWrite).toHaveBeenCalledWith("README.md", "Draft # Demo\n"));
  });

  it("shows the backend's refusal for a path outside the project", async () => {
    api.codeFileRead.mockRejectedValueOnce("That file is outside this workspace.");
    render(<CodeEditor />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    expect(await screen.findByText("That file is outside this workspace.")).toBeInTheDocument();
    expect(screen.getByText(EDITOR_EMPTY)).toBeInTheDocument();
  });

  it("picks syntax by extension and finds a parent folder", async () => {
    expect(await languageFor("src/lib.rs")).toHaveLength(1);
    expect(await languageFor("ui/App.tsx")).toHaveLength(1);
    expect(await languageFor("README.md")).toHaveLength(1);
    expect(await languageFor("notes.txt")).toHaveLength(0);
    expect(parentOf("src/a/b.rs")).toBe("src/a");
    expect(parentOf("src")).toBe("");
  });
});

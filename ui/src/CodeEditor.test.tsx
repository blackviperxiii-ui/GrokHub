import "@testing-library/jest-dom/vitest";
import { Activity } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EditorView } from "@codemirror/view";
import type { CodeFileListing, CodeFileText } from "./api";

const api = vi.hoisted(() => ({
  codeFilesList: vi.fn<(path: string) => Promise<CodeFileListing>>(),
  codeFileRead: vi.fn<(path: string) => Promise<CodeFileText>>(),
  codeFileWrite: vi.fn<(path: string, text: string, root: string) => Promise<CodeFileText>>(),
}));

vi.mock("./api", () => api);

import CodeEditor, { EDITOR_DISCARD, EDITOR_EMPTY, languageFor, parentOf, usesCrlf } from "./CodeEditor";

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

/** The project folder the stand-in desktop reads from. */
const DEMO = "/home/user/demo";

/** A folder with a few files, for the tests that pick one after another. */
const WIDE: CodeFileListing = {
  path: "",
  entries: ["README.md", "b.rs", "slow.txt"].map((name) => ({ name, path: name, dir: false })),
  more: false,
};

/** An answer the test gives by hand, to land it late. */
function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  let reject: (why: unknown) => void = () => {};
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

/** Let every answer already on its way land before looking at the page. */
const settle = () => act(() => new Promise<void>((done) => setTimeout(done, 0)));

/** Ctrl+S in the editor, as the keyboard sends it. */
function pressSave() {
  act(() => {
    editorView().contentDOM.dispatchEvent(
      new KeyboardEvent("keydown", { key: "s", code: "KeyS", ctrlKey: true, bubbles: true, cancelable: true }),
    );
  });
}

describe("Code tab → Editor", () => {
  beforeEach(() => {
    api.codeFilesList.mockReset();
    api.codeFileRead.mockReset();
    api.codeFileWrite.mockReset();
    api.codeFilesList.mockImplementation((p) => Promise.resolve(p === "src" ? SRC : ROOT));
    api.codeFileRead.mockImplementation((p) =>
      Promise.resolve({ path: p, text: p === "README.md" ? "# Demo\n" : "fn main() {}\n", root: DEMO }),
    );
    api.codeFileWrite.mockImplementation((p, text, root) => Promise.resolve({ path: p, text, root }));
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
    await waitFor(() => expect(api.codeFileWrite).toHaveBeenCalledWith("README.md", "# Demo\nMore text.\n", DEMO));
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
    await waitFor(() => expect(api.codeFileWrite).toHaveBeenCalledWith("README.md", "Draft # Demo\n", DEMO));
  });

  it("shows the backend's refusal for a path outside the project", async () => {
    api.codeFileRead.mockRejectedValueOnce("That file is outside this workspace.");
    render(<CodeEditor />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    expect(await screen.findByText("That file is outside this workspace.")).toBeInTheDocument();
    expect(screen.getByText(EDITOR_EMPTY)).toBeInTheDocument();
  });

  it("sends the project folder the file was opened from, and keeps the edits when the save is refused", async () => {
    const changed = "The project folder changed since this file was opened. Nothing was saved.";
    api.codeFileRead.mockResolvedValue({ path: "README.md", text: "# Project A\n", root: "/home/user/project-a" });
    api.codeFileWrite.mockRejectedValue(changed);
    function Page({ shown }: { shown: boolean }) {
      return (
        <Activity mode={shown ? "visible" : "hidden"}>
          <CodeEditor />
        </Activity>
      );
    }
    const { rerender } = render(<Page shown />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    await waitFor(() => expect(editorText()).toBe("# Project A"));
    editorView().dispatch({ changes: { from: 0, insert: "DRAFT " } });
    // Settings picked another project folder meanwhile.
    rerender(<Page shown={false} />);
    rerender(<Page shown />);
    await waitFor(() => expect(editorText()).toBe("DRAFT # Project A"));
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.codeFileWrite).toHaveBeenCalledTimes(1));
    expect(api.codeFileWrite).toHaveBeenCalledWith("README.md", "DRAFT # Project A\n", "/home/user/project-a");
    expect(await screen.findByRole("status")).toHaveTextContent(changed);
    expect(editorText()).toBe("DRAFT # Project A");
    expect(screen.getByText("· unsaved")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("asks before a slow open replaces edits typed while it was on its way", async () => {
    api.codeFilesList.mockResolvedValue(WIDE);
    render(<CodeEditor />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    await waitFor(() => expect(editorText()).toBe("# Demo"));

    const slow = deferred<CodeFileText>();
    api.codeFileRead.mockReturnValueOnce(slow.promise);
    await userEvent.click(screen.getByRole("button", { name: "slow.txt" }));
    editorView().dispatch({ changes: { from: 0, insert: "typed meanwhile " } });
    await screen.findByText("· unsaved");
    slow.resolve({ path: "slow.txt", text: "slow text\n", root: DEMO });
    await settle();

    expect(editorText()).toBe("typed meanwhile # Demo");
    expect(document.querySelector(".code-editor-path")).toHaveTextContent("README.md · unsaved");
    expect(screen.getByRole("group", { name: EDITOR_DISCARD })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Discard and open slow.txt" }));
    await waitFor(() => expect(editorText()).toBe("fn main() {}"));
    expect(document.querySelector(".code-editor-path")).toHaveTextContent(/^slow\.txt$/);
    expect(api.codeFileWrite).not.toHaveBeenCalled();
  });

  it("the newest pick wins when an older file answers last", async () => {
    api.codeFilesList.mockResolvedValue(WIDE);
    render(<CodeEditor />);
    const slow = deferred<CodeFileText>();
    api.codeFileRead.mockReturnValueOnce(slow.promise);
    await userEvent.click(await screen.findByRole("button", { name: "slow.txt" }));
    api.codeFileRead.mockResolvedValueOnce({ path: "b.rs", text: "fn b() {}\n", root: DEMO });
    await userEvent.click(screen.getByRole("button", { name: "b.rs" }));
    await waitFor(() => expect(editorText()).toBe("fn b() {}"));
    slow.resolve({ path: "slow.txt", text: "slow text\n", root: DEMO });
    await settle();
    expect(editorText()).toBe("fn b() {}");
    expect(document.querySelector(".code-editor-path")).toHaveTextContent(/^b\.rs$/);
    expect(screen.getByRole("button", { name: "b.rs" })).toHaveAttribute("aria-pressed", "true");
  });

  it("a slow open that fails after a newer pick says nothing", async () => {
    api.codeFilesList.mockResolvedValue(WIDE);
    render(<CodeEditor />);
    const slow = deferred<CodeFileText>();
    api.codeFileRead.mockReturnValueOnce(slow.promise);
    await userEvent.click(await screen.findByRole("button", { name: "slow.txt" }));
    api.codeFileRead.mockResolvedValueOnce({ path: "b.rs", text: "fn b() {}\n", root: DEMO });
    await userEvent.click(screen.getByRole("button", { name: "b.rs" }));
    await waitFor(() => expect(editorText()).toBe("fn b() {}"));
    slow.reject("That file is too big to edit here (the limit is 512 KB).");
    await settle();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("a save that lands after another file was opened leaves that file's edits unsaved and guarded", async () => {
    api.codeFilesList.mockResolvedValue(WIDE);
    render(<CodeEditor />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    await waitFor(() => expect(editorText()).toBe("# Demo"));
    editorView().dispatch({ changes: { from: 0, insert: "x" } });
    await screen.findByText("· unsaved");

    const slowSave = deferred<CodeFileText>();
    api.codeFileWrite.mockReturnValueOnce(slowSave.promise);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.codeFileWrite).toHaveBeenCalledWith("README.md", "x# Demo\n", DEMO);
    await userEvent.click(screen.getByRole("button", { name: "b.rs" }));
    await userEvent.click(screen.getByRole("button", { name: "Discard and open b.rs" }));
    await waitFor(() => expect(editorText()).toBe("fn main() {}"));
    editorView().dispatch({ changes: { from: 0, insert: "// mine\n" } });
    await waitFor(() => expect(document.querySelector(".code-editor-path")).toHaveTextContent("b.rs · unsaved"));

    slowSave.resolve({ path: "README.md", text: "x# Demo\n", root: DEMO });
    expect(await screen.findByText("Saved README.md.")).toBeInTheDocument();
    expect(document.querySelector(".code-editor-path")).toHaveTextContent("b.rs · unsaved");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    await userEvent.click(screen.getByRole("button", { name: "slow.txt" }));
    expect(screen.getByRole("group", { name: EDITOR_DISCARD })).toHaveTextContent("b.rs has edits that aren't saved.");
    expect(api.codeFileRead).toHaveBeenCalledTimes(2);
    expect(api.codeFileWrite).toHaveBeenCalledTimes(1);
  });

  it("shows a refused save right under the file's name, above the editor", async () => {
    api.codeFileWrite.mockRejectedValue("That file is too big to edit here (the limit is 512 KB).");
    render(<CodeEditor />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    await waitFor(() => expect(editorText()).toBe("# Demo"));
    editorView().dispatch({ changes: { from: 0, insert: "x" } });
    await userEvent.click(await screen.findByRole("button", { name: "Save" }));
    const note = await screen.findByRole("status");
    expect(note).toHaveTextContent("That file is too big to edit here (the limit is 512 KB).");
    const pane = document.querySelector(".code-editor-pane") as HTMLElement;
    expect(pane.contains(note)).toBe(true);
    expect(note.previousElementSibling).toBe(document.querySelector(".code-editor-bar"));
    const editor = document.querySelector(".code-editor-host") as HTMLElement;
    expect(note.compareDocumentPosition(editor) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("moves focus to Keep editing when the discard card appears", async () => {
    render(<CodeEditor />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    await waitFor(() => expect(editorText()).toBe("# Demo"));
    editorView().dispatch({ changes: { from: 0, insert: "x" } });
    await screen.findByText("· unsaved");
    await userEvent.click(screen.getByRole("button", { name: "src/" }));
    await userEvent.click(await screen.findByRole("button", { name: "main.rs" }));
    expect(screen.getByRole("group", { name: EDITOR_DISCARD })).toBeInTheDocument();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Keep editing" }));
  });

  it("Ctrl+S saves an edit once and does nothing for an unchanged file", async () => {
    render(<CodeEditor />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    await waitFor(() => expect(editorText()).toBe("# Demo"));
    pressSave();
    pressSave();
    await settle();
    expect(api.codeFileWrite).toHaveBeenCalledTimes(0);
    expect(screen.queryByRole("status")).toBeNull();

    editorView().dispatch({ changes: { from: 0, insert: "x" } });
    await screen.findByText("· unsaved");
    pressSave();
    expect(await screen.findByText("Saved README.md.")).toBeInTheDocument();
    pressSave();
    await settle();
    expect(api.codeFileWrite).toHaveBeenCalledTimes(1);
    expect(api.codeFileWrite).toHaveBeenCalledWith("README.md", "x# Demo\n", DEMO);
  });

  it("keeps Windows line endings on save, and an undone edit leaves the file clean", async () => {
    api.codeFileRead.mockResolvedValue({ path: "README.md", text: "one\r\ntwo\r\n", root: DEMO });
    render(<CodeEditor />);
    await userEvent.click(await screen.findByRole("button", { name: "README.md" }));
    await waitFor(() => expect(editorText()).toBe("onetwo"));
    expect(screen.queryByText("· unsaved")).toBeNull();

    const view = editorView();
    view.dispatch({ changes: { from: 0, insert: "X" } });
    await screen.findByText("· unsaved");
    view.dispatch({ changes: { from: 0, to: 1 } });
    await waitFor(() => expect(screen.queryByText("· unsaved")).toBeNull());

    // Pasted text arrives with plain line feeds; the file keeps its own ending.
    view.dispatch({ changes: { from: view.state.doc.length, insert: "three\nX" } });
    await userEvent.click(await screen.findByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.codeFileWrite).toHaveBeenCalledTimes(1));
    expect(api.codeFileWrite).toHaveBeenCalledWith("README.md", "one\r\ntwo\r\nthree\r\nX", DEMO);
    expect(await screen.findByText("Saved README.md.")).toBeInTheDocument();
    expect(screen.queryByText("· unsaved")).toBeNull();
  });

  it("comes back to the folder that was open, and lists it again when asked to refresh", async () => {
    function Page({ shown, refresh }: { shown: boolean; refresh: number }) {
      return (
        <Activity mode={shown ? "visible" : "hidden"}>
          <CodeEditor refresh={refresh} />
        </Activity>
      );
    }
    const { rerender } = render(<Page shown refresh={0} />);
    await userEvent.click(await screen.findByRole("button", { name: "src/" }));
    expect(await screen.findByRole("button", { name: "main.rs" })).toBeInTheDocument();
    expect(api.codeFilesList.mock.calls).toEqual([[""], ["src"]]);

    rerender(<Page shown={false} refresh={0} />);
    rerender(<Page shown refresh={0} />);
    await waitFor(() => expect(api.codeFilesList).toHaveBeenCalledTimes(3));
    await settle();
    expect(api.codeFilesList).toHaveBeenLastCalledWith("src");
    expect(screen.getByRole("button", { name: /up from src/ })).toBeInTheDocument();

    // A file made outside the app shows after Refresh.
    api.codeFilesList.mockResolvedValue({
      path: "src",
      entries: [...SRC.entries, { name: "new.rs", path: "src/new.rs", dir: false }],
      more: false,
    });
    rerender(<Page shown refresh={1} />);
    expect(await screen.findByRole("button", { name: "new.rs" })).toBeInTheDocument();
    expect(api.codeFilesList.mock.calls).toEqual([[""], ["src"], ["src"], ["src"]]);
  });

  it("falls back to the top folder when the open folder is gone", async () => {
    function Page({ shown }: { shown: boolean }) {
      return (
        <Activity mode={shown ? "visible" : "hidden"}>
          <CodeEditor />
        </Activity>
      );
    }
    const { rerender } = render(<Page shown />);
    await userEvent.click(await screen.findByRole("button", { name: "src/" }));
    await screen.findByRole("button", { name: "main.rs" });
    api.codeFilesList.mockImplementation((p) => (p === "src" ? Promise.reject("That folder isn't there.") : Promise.resolve(ROOT)));
    rerender(<Page shown={false} />);
    rerender(<Page shown />);
    expect(await screen.findByRole("button", { name: "src/" })).toBeInTheDocument();
    expect(api.codeFilesList.mock.calls).toEqual([[""], ["src"], ["src"], [""]]);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("says a list error once when the top folder itself can't be read", async () => {
    api.codeFilesList.mockRejectedValue("The project folder can't be read.");
    render(<CodeEditor />);
    expect(await screen.findByRole("status")).toHaveTextContent("The project folder can't be read.");
    expect(api.codeFilesList.mock.calls).toEqual([[""]]);
  });

  it("knows a file whose every line ends in CRLF from one with mixed or other endings", () => {
    expect(usesCrlf("one\r\ntwo\r\n")).toBe(true);
    expect(usesCrlf("one\r\ntwo")).toBe(true);
    expect(usesCrlf("one\r\ntwo\n")).toBe(false);
    expect(usesCrlf("one\rtwo\r\n")).toBe(false);
    expect(usesCrlf("one\ntwo\n")).toBe(false);
    expect(usesCrlf("one line")).toBe(false);
    expect(usesCrlf("")).toBe(false);
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

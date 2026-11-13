import "@testing-library/jest-dom/vitest";
import { Activity } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeTermChunk } from "./api";

// jsdom can't lay out xterm's canvas, so a small stand-in records what the
// component does with it.
const xterm = vi.hoisted(() => {
  const state = {
    written: [] as string[],
    onData: null as ((data: string) => void) | null,
    disposed: 0,
    resets: 0,
    // The newest terminal's options: what it was made with, then what the
    // component set on it since.
    options: {} as Record<string, unknown>,
    rows: 24,
    cols: 80,
  };
  class Terminal {
    options: Record<string, unknown>;
    get rows() {
      return state.rows;
    }
    get cols() {
      return state.cols;
    }
    constructor(options: Record<string, unknown>) {
      this.options = { ...options };
      state.options = this.options;
    }
    loadAddon() {}
    open() {}
    focus() {}
    reset() {
      state.resets += 1;
    }
    write(text: string) {
      state.written.push(text);
    }
    onData(fn: (data: string) => void) {
      state.onData = fn;
    }
    dispose() {
      state.disposed += 1;
    }
  }
  return { state, Terminal };
});

vi.mock("@xterm/xterm", () => ({ Terminal: xterm.Terminal }));
vi.mock("@xterm/xterm/css/xterm.css", () => ({}));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }));

const api = vi.hoisted(() => {
  const listeners: Array<(c: CodeTermChunk) => void> = [];
  return {
    listeners,
    codeTermOpen: vi.fn<(rows: number, cols: number) => Promise<number>>(),
    codeTermWrite: vi.fn<(id: number, data: string) => Promise<void>>(() => Promise.resolve()),
    codeTermResize: vi.fn<(id: number, rows: number, cols: number) => Promise<void>>(() => Promise.resolve()),
    codeTermClose: vi.fn<() => Promise<void>>(() => Promise.resolve()),
    onCodeTerm: vi.fn((fn: (c: CodeTermChunk) => void) => {
      listeners.push(fn);
      return Promise.resolve(() => {});
    }),
  };
});

vi.mock("./api", () => api);

const external = vi.hoisted(() => ({ openExternal: vi.fn<(url: string) => Promise<void>>(() => Promise.resolve()) }));
vi.mock("./external", () => external);

import CodeTerminal, { TERM_ENDED, TERM_NOTE } from "./CodeTerminal";

function emit(chunk: CodeTermChunk) {
  act(() => api.listeners.forEach((fn) => fn(chunk)));
}

/** Let every answer already on its way land before looking at the page. */
const settle = () => act(() => new Promise<void>((done) => setTimeout(done, 0)));

/** The page's theme colors, as the stylesheet sets them on <html>. */
function paint(panel: string, text: string, accent: string) {
  const root = document.documentElement;
  root.style.setProperty("--panel", panel);
  root.style.setProperty("--text", text);
  root.style.setProperty("--accent", accent);
}

/** jsdom has no ResizeObserver: this one fires when the test says the pane changed size. */
const panes: Array<() => void> = [];
class PaneWatch {
  private fire: () => void;
  constructor(fn: () => void) {
    this.fire = fn;
  }
  observe() {
    panes.push(this.fire);
  }
  disconnect() {
    panes.splice(panes.indexOf(this.fire), 1);
  }
}
function resizePane() {
  act(() => panes.forEach((fire) => fire()));
}

describe("Code tab → Terminal", () => {
  beforeEach(() => {
    api.listeners.length = 0;
    api.codeTermOpen.mockReset();
    api.codeTermOpen.mockResolvedValue(7);
    api.codeTermWrite.mockClear();
    api.codeTermClose.mockClear();
    api.onCodeTerm.mockClear();
    xterm.state.written = [];
    xterm.state.onData = null;
    xterm.state.disposed = 0;
    xterm.state.resets = 0;
    xterm.state.options = {};
    xterm.state.rows = 24;
    xterm.state.cols = 80;
    api.codeTermWrite.mockReset();
    api.codeTermWrite.mockResolvedValue();
    api.codeTermResize.mockClear();
    external.openExternal.mockReset();
    external.openExternal.mockResolvedValue();
    panes.length = 0;
    vi.stubGlobal("ResizeObserver", PaneWatch);
    paint("#ffffff", "#1c1c1a", "#1c1c1a");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    document.documentElement.removeAttribute("style");
    document.documentElement.removeAttribute("data-theme");
  });

  it("opens a shell at the terminal's size, shows its output and sends typing", async () => {
    render(<CodeTerminal />);
    expect(screen.getByText(TERM_NOTE)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await waitFor(() => expect(api.codeTermOpen).toHaveBeenCalledWith(24, 80));
    expect(await screen.findByRole("button", { name: "Close terminal" })).toBeInTheDocument();

    emit({ id: 7, text: "$ ", exited: false });
    emit({ id: 3, text: "from an old shell", exited: false });
    expect(xterm.state.written).toEqual(["$ "]);

    xterm.state.onData?.("ls\r");
    expect(api.codeTermWrite).toHaveBeenCalledWith(7, "ls\r");
  });

  it("reports the end of the shell and offers a new one", async () => {
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    emit({ id: 7, text: "logout\r\n", exited: true });
    expect(await screen.findByText(TERM_ENDED)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start a new shell" })).toBeInTheDocument();
    xterm.state.onData?.("echo lost\r");
    expect(api.codeTermWrite).not.toHaveBeenCalled();
  });

  it("Close terminal ends the shell and a new one starts on a clean screen", async () => {
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await userEvent.click(await screen.findByRole("button", { name: "Close terminal" }));
    expect(api.codeTermClose).toHaveBeenCalledTimes(1);
    expect(screen.getByText(TERM_ENDED)).toBeInTheDocument();
    xterm.state.onData?.("echo lost\r");
    expect(api.codeTermWrite).not.toHaveBeenCalled();

    api.codeTermOpen.mockResolvedValueOnce(8);
    await userEvent.click(screen.getByRole("button", { name: "Start a new shell" }));
    await screen.findByRole("button", { name: "Close terminal" });
    expect(xterm.state.resets).toBe(1);
    emit({ id: 7, text: "old", exited: false });
    emit({ id: 8, text: "new", exited: false });
    expect(xterm.state.written).toEqual(["new"]);
    expect(api.onCodeTerm).toHaveBeenCalledTimes(1);
  });

  it("keeps the shell and its output while the Code page is hidden", async () => {
    function Page({ shown }: { shown: boolean }) {
      return (
        <Activity mode={shown ? "visible" : "hidden"}>
          <CodeTerminal />
        </Activity>
      );
    }
    const { rerender } = render(<Page shown />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    rerender(<Page shown={false} />);
    emit({ id: 7, text: "built while away\r\n", exited: false });
    rerender(<Page shown />);
    expect(api.codeTermClose).not.toHaveBeenCalled();
    expect(xterm.state.disposed).toBe(0);
    expect(xterm.state.written).toEqual(["built while away\r\n"]);
    expect(screen.getByRole("button", { name: "Close terminal" })).toBeInTheDocument();
  });

  it("shows why a shell couldn't start", async () => {
    api.codeTermOpen.mockRejectedValueOnce("The shell couldn't start.");
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    expect(await screen.findByText("The shell couldn't start.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open terminal" })).toBeEnabled();
  });

  it("shows the first output of a new shell when it arrives before the shell's id", async () => {
    let answer: (id: number) => void = () => {};
    api.codeTermOpen.mockImplementationOnce(() => new Promise<number>((done) => (answer = done)));
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await waitFor(() => expect(api.codeTermOpen).toHaveBeenCalledTimes(1));
    emit({ id: 7, text: "Last login\r\n", exited: false });
    emit({ id: 3, text: "from an old shell", exited: false });
    emit({ id: 7, text: "$ ", exited: false });
    expect(xterm.state.written).toEqual([]);
    await act(async () => answer(7));
    expect(await screen.findByRole("button", { name: "Close terminal" })).toBeInTheDocument();
    expect(xterm.state.written).toEqual(["Last login\r\n", "$ "]);
    emit({ id: 7, text: "ls\r\n", exited: false });
    emit({ id: 3, text: "still the old shell", exited: false });
    expect(xterm.state.written).toEqual(["Last login\r\n", "$ ", "ls\r\n"]);
  });

  it("a shell that ends before its id arrives is reported as ended", async () => {
    let answer: (id: number) => void = () => {};
    api.codeTermOpen.mockImplementationOnce(() => new Promise<number>((done) => (answer = done)));
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await waitFor(() => expect(api.codeTermOpen).toHaveBeenCalledTimes(1));
    emit({ id: 7, text: "no such shell\r\n", exited: true });
    await act(async () => answer(7));
    expect(await screen.findByText(TERM_ENDED)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start a new shell" })).toBeEnabled();
    expect(xterm.state.written).toEqual(["no such shell\r\n"]);
    xterm.state.onData?.("echo lost\r");
    expect(api.codeTermWrite).not.toHaveBeenCalled();
  });

  it("shows why typing or a paste was refused, and drops the message when input goes through again", async () => {
    const tooMuch = "That is too much to paste into the terminal at once.";
    const busy = "The terminal is busy. Try again when the program is reading input.";
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });

    api.codeTermWrite.mockRejectedValueOnce(tooMuch);
    xterm.state.onData?.("x".repeat(70_000));
    expect(await screen.findByRole("status")).toHaveTextContent(tooMuch);

    api.codeTermWrite.mockRejectedValueOnce(busy);
    xterm.state.onData?.("y");
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent(busy));

    xterm.state.onData?.("z");
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    expect(api.codeTermWrite).toHaveBeenCalledTimes(3);
  });

  it("says the terminal has ended when the desktop refuses input for that reason", async () => {
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    api.codeTermWrite.mockRejectedValueOnce("The terminal has ended.");
    xterm.state.onData?.("ls\r");
    expect(await screen.findByRole("status")).toHaveTextContent("The terminal has ended.");
  });

  it("a refusal for a shell that was closed meanwhile says nothing", async () => {
    let refuse: (why: string) => void = () => {};
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    api.codeTermWrite.mockImplementationOnce(() => new Promise<void>((_, no) => (refuse = no)));
    xterm.state.onData?.("ls\r");
    await userEvent.click(screen.getByRole("button", { name: "Close terminal" }));
    await act(async () => refuse("The terminal has ended."));
    await settle();
    expect(screen.getAllByRole("status").map((el) => el.textContent)).toEqual([TERM_ENDED]);
  });

  it("makes the terminal readable in the theme: a selection that shows, and colors that meet contrast", async () => {
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    expect(xterm.state.options.minimumContrastRatio).toBe(4.5);
    expect(xterm.state.options.theme).toEqual({
      background: "#ffffff",
      foreground: "#1c1c1a",
      cursor: "#1c1c1a",
      selectionBackground: "#1c1c1a47",
    });
  });

  it("a new shell takes the colors of the theme the app is in now", async () => {
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await userEvent.click(await screen.findByRole("button", { name: "Close terminal" }));
    paint("#121215", "#f2f2f4", "#d7263d");
    await userEvent.click(screen.getByRole("button", { name: "Start a new shell" }));
    await screen.findByRole("button", { name: "Close terminal" });
    expect(xterm.state.resets).toBe(1);
    expect(xterm.state.options.theme).toEqual({
      background: "#121215",
      foreground: "#f2f2f4",
      cursor: "#f2f2f4",
      selectionBackground: "#d7263d47",
    });
  });

  it("an open terminal follows a theme change, also one made while the Code page was hidden", async () => {
    function Page({ shown }: { shown: boolean }) {
      return (
        <Activity mode={shown ? "visible" : "hidden"}>
          <CodeTerminal />
        </Activity>
      );
    }
    const { rerender } = render(<Page shown />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    expect((xterm.state.options.theme as { background: string }).background).toBe("#ffffff");

    // Settings → Appearance, with the Code page hidden behind it.
    rerender(<Page shown={false} />);
    paint("#121215", "#f2f2f4", "#d7263d");
    document.documentElement.dataset.theme = "ops-dark";
    rerender(<Page shown />);
    expect(xterm.state.options.theme).toEqual({
      background: "#121215",
      foreground: "#f2f2f4",
      cursor: "#f2f2f4",
      selectionBackground: "#d7263d47",
    });

    // And one made while the terminal is on screen.
    paint("#050506", "#f2f2f4", "#d7263d");
    document.documentElement.dataset.theme = "oled-black";
    await waitFor(() => expect((xterm.state.options.theme as { background: string }).background).toBe("#050506"));
    expect(xterm.state.resets).toBe(0);
  });

  it("follows the system when it flips between light and dark with no theme pinned", async () => {
    const flips: Array<() => void> = [];
    const asked: string[] = [];
    vi.stubGlobal("matchMedia", (query: string) => {
      asked.push(query);
      return {
        matches: false,
        addEventListener: (_kind: string, fn: () => void) => flips.push(fn),
        removeEventListener: (_kind: string, fn: () => void) => flips.splice(flips.indexOf(fn), 1),
      };
    });
    const { unmount } = render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    expect(asked).toEqual(["(prefers-color-scheme: dark)"]);
    expect(flips).toHaveLength(1);
    paint("#121215", "#f2f2f4", "#d7263d");
    act(() => flips.forEach((fn) => fn()));
    expect(xterm.state.options.theme).toEqual({
      background: "#121215",
      foreground: "#f2f2f4",
      cursor: "#f2f2f4",
      selectionBackground: "#d7263d47",
    });
    unmount();
    expect(flips).toHaveLength(0);
  });

  it("opens a link in terminal output through the app, not the browser's own prompt", async () => {
    const confirm = vi.fn(() => true);
    vi.stubGlobal("confirm", confirm);
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    const links = xterm.state.options.linkHandler as { activate: (event: MouseEvent, uri: string) => void };
    links.activate(new MouseEvent("click"), "https://example.com/a");
    expect(external.openExternal).toHaveBeenCalledTimes(1);
    expect(external.openExternal).toHaveBeenCalledWith("https://example.com/a");
    expect(confirm).not.toHaveBeenCalled();

    external.openExternal.mockRejectedValueOnce(new Error("That link can't be opened."));
    links.activate(new MouseEvent("click"), "javascript:alert(1)");
    expect(await screen.findByRole("status")).toHaveTextContent(/^That link can't be opened\.$/);
  });

  it("tells the shell about a new size once, and not at all when the size did not change", async () => {
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    expect(api.codeTermOpen).toHaveBeenCalledWith(24, 80);
    expect(panes).toHaveLength(1);
    resizePane();
    resizePane();
    resizePane();
    expect(api.codeTermResize).toHaveBeenCalledTimes(0);

    xterm.state.rows = 30;
    resizePane();
    resizePane();
    expect(api.codeTermResize.mock.calls).toEqual([[7, 30, 80]]);
    xterm.state.cols = 100;
    resizePane();
    expect(api.codeTermResize.mock.calls).toEqual([
      [7, 30, 80],
      [7, 30, 100],
    ]);
  });

  it("sends a size again when the shell refused it and the pane still has that size", async () => {
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    api.codeTermResize.mockRejectedValueOnce("The terminal couldn't be resized.");
    xterm.state.rows = 30;
    resizePane();
    await settle();
    expect(api.codeTermResize.mock.calls).toEqual([[7, 30, 80]]);

    // The pane changes by a pixel or two: same rows and columns, and the shell still has the old ones.
    resizePane();
    expect(api.codeTermResize.mock.calls).toEqual([
      [7, 30, 80],
      [7, 30, 80],
    ]);
    await settle();
    resizePane();
    resizePane();
    expect(api.codeTermResize).toHaveBeenCalledTimes(2);
  });

  it("a refusal that lands after a newer size was sent does not send the newer size twice", async () => {
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    let refuse: (why: string) => void = () => {};
    api.codeTermResize.mockImplementationOnce(() => new Promise<void>((_, no) => (refuse = no)));
    xterm.state.rows = 30;
    resizePane();
    xterm.state.rows = 40;
    resizePane();
    await act(async () => refuse("The terminal couldn't be resized."));
    await settle();
    resizePane();
    expect(api.codeTermResize.mock.calls).toEqual([
      [7, 30, 80],
      [7, 40, 80],
    ]);
  });

  it("sends the size the pane took while the shell was starting", async () => {
    let answer: (id: number) => void = () => {};
    api.codeTermOpen.mockImplementationOnce(() => new Promise<number>((done) => (answer = done)));
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await waitFor(() => expect(api.codeTermOpen).toHaveBeenCalledWith(24, 80));
    xterm.state.rows = 40;
    resizePane();
    expect(api.codeTermResize).not.toHaveBeenCalled();
    await act(async () => answer(7));
    await screen.findByRole("button", { name: "Close terminal" });
    expect(api.codeTermResize.mock.calls).toEqual([[7, 40, 80]]);
  });

  it("names the terminal pane as a group, outside the tab order", async () => {
    render(<CodeTerminal />);
    await userEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await screen.findByRole("button", { name: "Close terminal" });
    const pane = screen.getByRole("group", { name: "Terminal output" });
    expect(pane).toHaveClass("code-terminal-host");
    expect(pane).not.toHaveAttribute("tabindex");
  });
});

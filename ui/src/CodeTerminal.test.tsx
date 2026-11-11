import "@testing-library/jest-dom/vitest";
import { Activity } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeTermChunk } from "./api";

// jsdom can't lay out xterm's canvas, so a small stand-in records what the
// component does with it.
const xterm = vi.hoisted(() => {
  const state = {
    written: [] as string[],
    onData: null as ((data: string) => void) | null,
    disposed: 0,
    resets: 0,
  };
  class Terminal {
    rows = 24;
    cols = 80;
    constructor(public _options: unknown) {}
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
    codeTermResize: vi.fn(() => Promise.resolve()),
    codeTermClose: vi.fn<() => Promise<void>>(() => Promise.resolve()),
    onCodeTerm: vi.fn((fn: (c: CodeTermChunk) => void) => {
      listeners.push(fn);
      return Promise.resolve(() => {});
    }),
  };
});

vi.mock("./api", () => api);

import CodeTerminal, { TERM_ENDED, TERM_NOTE } from "./CodeTerminal";

function emit(chunk: CodeTermChunk) {
  act(() => api.listeners.forEach((fn) => fn(chunk)));
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
});

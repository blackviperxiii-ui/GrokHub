import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { codeTermClose, codeTermOpen, codeTermResize, codeTermWrite, onCodeTerm } from "./api";

export const TERM_TITLE = "Terminal";
export const TERM_NOTE =
  "Your own shell, started in the project folder. Secret values are hidden in what it shows. Commands typed here, git push included, skip the push guard above.";
export const TERM_ENDED = "The shell ended.";

/** A theme color from the page's own variables, so the terminal matches. */
function themeColor(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

/** The Code tab's terminal: one shell at a time, in the project folder. */
export default function CodeTerminal() {
  const [open, setOpen] = useState(false);
  const [starting, setStarting] = useState(false);
  const [ended, setEnded] = useState(false);
  const [live, setLive] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const host = useRef<HTMLDivElement | null>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const session = useRef<number | null>(null);

  const listening = useRef(false);

  // The shell keeps running while the tab is hidden (the Code page stays
  // mounted, but its effects are paused), so its output listener and the
  // terminal are made on Open, not in an effect. Close, the shell ending, a new
  // Open (which replaces the old shell) and quitting the app end it.
  async function listen() {
    if (listening.current) return;
    listening.current = true;
    try {
      await onCodeTerm((chunk) => {
        // Output for the current shell only; an older session's late text is dropped.
        if (chunk.id !== session.current) return;
        if (chunk.text) term.current?.write(chunk.text);
        if (chunk.exited) {
          session.current = null;
          setLive(false);
          setEnded(true);
        }
      });
    } catch {
      listening.current = false;
    }
  }

  // Keep the shell's size in step with the pane.
  useEffect(() => {
    if (!open || !host.current || typeof ResizeObserver === "undefined") return;
    const watch = new ResizeObserver(() => {
      const t = term.current;
      if (!t) return;
      fit.current?.fit();
      if (session.current !== null) void codeTermResize(session.current, t.rows, t.cols).catch(() => {});
    });
    watch.observe(host.current);
    return () => watch.disconnect();
  }, [open]);

  async function start() {
    if (starting) return;
    setStarting(true);
    setNote(null);
    setEnded(false);
    setOpen(true);
    try {
      // Let the pane show first, so the terminal measures its real size.
      await new Promise<void>((done) => requestAnimationFrame(() => done()));
      if (!host.current) return;
      await listen();
      if (!term.current) {
        const t = new Terminal({
          cursorBlink: true,
          fontFamily: "ui-monospace, monospace",
          fontSize: 13,
          scrollback: 5000,
          theme: {
            background: themeColor("--panel", "#ffffff"),
            foreground: themeColor("--text", "#1c1c1a"),
            cursor: themeColor("--text", "#1c1c1a"),
          },
        });
        const f = new FitAddon();
        t.loadAddon(f);
        t.open(host.current);
        t.onData((data) => {
          if (session.current !== null) void codeTermWrite(session.current, data).catch(() => {});
        });
        term.current = t;
        fit.current = f;
      } else {
        term.current.reset();
      }
      fit.current?.fit();
      const t = term.current;
      session.current = await codeTermOpen(t.rows, t.cols);
      setLive(true);
      t.focus();
    } catch (err) {
      setNote(String(err));
    } finally {
      setStarting(false);
    }
  }

  async function stop() {
    session.current = null;
    setLive(false);
    setEnded(true);
    try {
      await codeTermClose();
    } catch (err) {
      setNote(String(err));
    }
  }

  return (
    <div className="code-terminal">
      <div className="code-editor-bar">
        <h3>{TERM_TITLE}</h3>
        {live ? (
          <button type="button" className="quiet" onClick={() => void stop()}>
            Close terminal
          </button>
        ) : (
          <button type="button" onClick={() => void start()} disabled={starting}>
            {starting ? "Starting…" : ended ? "Start a new shell" : "Open terminal"}
          </button>
        )}
      </div>
      <p className="note">{TERM_NOTE}</p>
      {ended && (
        <p className="note" role="status">
          {TERM_ENDED}
        </p>
      )}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      <div ref={host} className="code-terminal-host" hidden={!open} aria-label="Terminal output" />
    </div>
  );
}

import { useCallback, useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { codeTermClose, codeTermOpen, codeTermResize, codeTermWrite, onCodeTerm, type CodeTermChunk } from "./api";
import { openExternal } from "./external";

export const TERM_TITLE = "Terminal";
export const TERM_NOTE =
  "Your own shell, started in the project folder. Secret values are hidden in what it shows. Commands typed here, git push included, skip the push guard above.";
export const TERM_ENDED = "The shell ended.";

/** A theme color from the page's own variables, so the terminal matches. */
function themeColor(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

/** A `#rgb` or `#rrggbb` color with an alpha added, as `#rrggbbaa`; null for
 * any other form (the terminal can't read `var()` or `color-mix()`). */
function faded(color: string, alpha: number): string | null {
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(color)?.[1];
  if (!hex) return null;
  const full = hex.length === 3 ? [...hex].map((c) => c + c).join("") : hex;
  const strength = Math.round(alpha * 255).toString(16).padStart(2, "0");
  return `#${full}${strength}`;
}

/** The terminal's colors, read from the theme the page is in now. The
 * selection is the accent at the editor's strength: the terminal's own default
 * is white at 30 percent, which shows nothing on a white panel. */
function termTheme(): ITheme {
  const background = themeColor("--panel", "#ffffff");
  const foreground = themeColor("--text", "#1c1c1a");
  return {
    background,
    foreground,
    cursor: foreground,
    selectionBackground: faded(themeColor("--accent", foreground), 0.28) ?? faded(foreground, 0.28) ?? "#1c1c1a47",
  };
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
  // Output that arrives while a shell is starting, before its id is known.
  const early = useRef<CodeTermChunk[] | null>(null);
  // The size the shell was last told, as `rows x cols`.
  const sent = useRef("");
  // True while the note line shows why the shell refused input.
  const refused = useRef(false);

  const listening = useRef(false);

  /** One piece of the current shell's output, or the news that it ended. */
  function take(chunk: CodeTermChunk) {
    if (chunk.text) term.current?.write(chunk.text);
    if (chunk.exited) {
      session.current = null;
      setLive(false);
      setEnded(true);
    }
  }

  // The shell keeps running while the tab is hidden (the Code page stays
  // mounted, but its effects are paused), so its output listener and the
  // terminal are made on Open, not in an effect. Close, the shell ending, a new
  // Open (which replaces the old shell) and quitting the app end it.
  async function listen() {
    if (listening.current) return;
    listening.current = true;
    try {
      await onCodeTerm((chunk) => {
        // Output for the current shell only; an older session's late text is
        // dropped. A new shell's first output can arrive before the answer
        // that carries its id, so it is held until then.
        if (chunk.id !== session.current) {
          early.current?.push(chunk);
          return;
        }
        take(chunk);
      });
    } catch {
      listening.current = false;
    }
  }

  /** Tell the shell the terminal's size, unless it already has it. */
  const syncSize = useCallback(() => {
    const t = term.current;
    const id = session.current;
    if (!t || id === null) return;
    const size = `${t.rows}x${t.cols}`;
    if (size === sent.current) return;
    sent.current = size;
    void codeTermResize(id, t.rows, t.cols).catch(() => {
      // The shell did not take it: the next look at the pane sends it again.
      if (sent.current === size) sent.current = "";
    });
  }, []);

  // Keep the shell's size in step with the pane.
  useEffect(() => {
    if (!open || !host.current || typeof ResizeObserver === "undefined") return;
    const watch = new ResizeObserver(() => {
      if (!term.current) return;
      fit.current?.fit();
      syncSize();
    });
    watch.observe(host.current);
    return () => watch.disconnect();
  }, [open, syncSize]);

  // Follow the app's theme: when the tab shows again (the theme is changed in
  // Settings, with this page hidden), when Settings pins another theme, and
  // when the system flips between light and dark.
  useEffect(() => {
    if (!open) return;
    const apply = () => {
      if (term.current) term.current.options.theme = termTheme();
    };
    apply();
    const pinned = new MutationObserver(apply);
    pinned.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    const system = typeof window.matchMedia === "function" ? window.matchMedia("(prefers-color-scheme: dark)") : null;
    system?.addEventListener("change", apply);
    return () => {
      pinned.disconnect();
      system?.removeEventListener("change", apply);
    };
  }, [open]);

  async function start() {
    if (starting) return;
    setStarting(true);
    refused.current = false;
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
          // The stock colors are made for a dark screen; this keeps each one readable on the panel.
          minimumContrastRatio: 4.5,
          theme: termTheme(),
          // A link in the output opens the way every other link in the app does.
          linkHandler: {
            activate: (_event, uri) => {
              void openExternal(uri).catch((err) => setNote(err instanceof Error ? err.message : String(err)));
            },
          },
        });
        const f = new FitAddon();
        t.loadAddon(f);
        t.open(host.current);
        t.onData((data) => {
          const id = session.current;
          if (id === null) return;
          codeTermWrite(id, data).then(
            () => {
              if (!refused.current) return;
              refused.current = false;
              setNote(null);
            },
            (err) => {
              // The shell said no (busy, too much at once, or ended): say why,
              // unless that shell is already gone from the pane.
              if (session.current !== id) return;
              refused.current = true;
              setNote(String(err));
            },
          );
        });
        term.current = t;
        fit.current = f;
      } else {
        term.current.reset();
        term.current.options.theme = termTheme();
      }
      fit.current?.fit();
      const t = term.current;
      early.current = [];
      sent.current = `${t.rows}x${t.cols}`;
      const id = await codeTermOpen(t.rows, t.cols);
      session.current = id;
      const held = early.current ?? [];
      early.current = null;
      for (const chunk of held) {
        if (chunk.id === id && session.current === id) take(chunk);
      }
      // The shell may have ended already, in what was held.
      if (session.current !== id) return;
      setLive(true);
      // The pane may have changed size while the shell was starting.
      syncSize();
      t.focus();
    } catch (err) {
      setNote(String(err));
    } finally {
      early.current = null;
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
      <div ref={host} className="code-terminal-host" hidden={!open} role="group" aria-label="Terminal output" />
    </div>
  );
}

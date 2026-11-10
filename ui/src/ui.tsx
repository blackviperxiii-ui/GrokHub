// Small shared pieces for the shell's pages: pop-up menus that close like
// menus, windows that keep the keyboard inside, a status line that reads as an
// error when it is one, an inline "are you sure" row for destructive actions,
// and outside links that open in the browser.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode, type RefObject } from "react";
import { externalUrlOk, openExternal } from "./external";

/** Close a pop-up on Escape or on a press outside `ref`. Only listens while open. */
export function useDismiss<T extends HTMLElement>(open: boolean, close: () => void): RefObject<T | null> {
  const ref = useRef<T | null>(null);
  const closeRef = useRef(close);
  useEffect(() => {
    closeRef.current = close;
  });
  useEffect(() => {
    if (!open) return;
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") closeRef.current();
    }
    function onDown(event: PointerEvent | globalThis.MouseEvent) {
      const el = ref.current;
      if (el && event.target instanceof Node && !el.contains(event.target)) closeRef.current();
    }
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onDown);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onDown);
    };
  }, [open]);
  return ref;
}

const TABBABLE = "a[href], button, input, select, textarea, video[controls], [tabindex]";
/** The open windows, newest last. Only the newest holds the keyboard. */
const modals: RefObject<HTMLElement | null>[] = [];

/** Keep the keyboard in the window `box` holds while `open`: Tab wraps from its
 * last control to its first, and closing gives focus back to where it was. */
export function useModalFocus(box: RefObject<HTMLElement | null>, open = true) {
  // A layout effect, so the opener is read before the window's own effect moves focus inside.
  useLayoutEffect(() => {
    if (!open) return;
    const opener = document.activeElement;
    modals.push(box);
    function onKey(event: KeyboardEvent) {
      const el = box.current;
      if (event.key !== "Tab" || !el || modals[modals.length - 1] !== box) return;
      const stops = [...el.querySelectorAll<HTMLElement>(TABBABLE)].filter(
        (s) => s.tabIndex >= 0 && !s.matches(":disabled"),
      );
      const at = document.activeElement;
      const outside = !el.contains(at);
      const edge = event.shiftKey ? stops[0] : stops[stops.length - 1];
      // Shift+Tab from the window itself (focused, but not a stop) would also leave it.
      if (stops.length > 0 && !outside && at !== edge && !(event.shiftKey && at === el)) return;
      event.preventDefault();
      (event.shiftKey ? stops[stops.length - 1] : stops[0])?.focus();
    }
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      modals.splice(modals.indexOf(box), 1);
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [box, open]);
}

/** A page's one-line result: `ok` after something worked, `error` after it didn't. */
export type Note = { text: string; error: boolean } | null;

export function useNote() {
  const [note, setNote] = useState<Note>(null);
  // The same functions every render, so an effect can list them as deps and still run once.
  const set = useMemo(
    () => ({
      ok: (text: string | null | undefined) => setNote(text ? { text, error: false } : null),
      fail: (err: unknown) => setNote({ text: String(err), error: true }),
      clear: () => setNote(null),
    }),
    [],
  );
  return { note, ...set };
}

/** The note under a page's buttons, in the error colour when it is one. */
export function StatusLine({ note, className = "status" }: { note: Note; className?: string }) {
  if (!note) return null;
  return (
    <p className={`${className} ${note.error ? "status--error" : "status--ok"}`} role="status">
      {note.text}
    </p>
  );
}

/** Ask before a destructive action: the question, then the action and Keep. */
export function ConfirmRow({
  question,
  detail,
  action,
  busy = false,
  onConfirm,
  onCancel,
}: {
  question: string;
  detail?: ReactNode;
  action: string;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="card-buttons confirm-row" role="group" aria-label={question}>
      <span>
        {question}
        {detail ? <> {detail}</> : null}
      </span>
      <button type="button" className="danger" disabled={busy} onClick={onConfirm}>
        {action}
      </button>
      <button type="button" className="quiet" onClick={onCancel}>
        Keep
      </button>
    </div>
  );
}

/** Chat errors that Settings can fix: no key, a rejected key or sign-in, no
 * keychain. Tool failures and network trouble don't get the Settings link. */
export function needsSettings(error: string): boolean {
  return /api key|sign in|sign-in|signed in|keychain|HTTP 40[13]\b/i.test(error);
}

/** A link to a web page outside the app: the system browser opens it. */
export function ExternalLink({ href, children }: { href: string; children: ReactNode }) {
  function open(event: MouseEvent<HTMLAnchorElement>) {
    event.preventDefault();
    void openExternal(href).catch(() => {});
  }
  // A middle click or "open in new tab" follows the address itself, so only a web page gets one.
  return (
    <a href={externalUrlOk(href) ? href : undefined} target="_blank" rel="noopener noreferrer" onClick={open}>
      {children}
    </a>
  );
}

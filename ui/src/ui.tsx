// Small shared pieces for the shell's pages: pop-up menus that close like
// menus, a status line that reads as an error when it is one, an inline
// "are you sure" row for destructive actions, and outside links that open in
// the browser.
import { useEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode, type RefObject } from "react";
import { openExternal } from "./external";

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
  return (
    <a href={href} target="_blank" rel="noopener noreferrer" onClick={open}>
      {children}
    </a>
  );
}

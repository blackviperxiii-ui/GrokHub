import { useCallback, useEffect, useRef, useState } from "react";
import {
  pulseLiveAllow,
  pulseLiveClose,
  pulseLiveDeny,
  pulseLiveStop,
  pulseLiveUndo,
  type LiveStepMark,
  type LiveView,
} from "./api";
import { useModalFocus } from "./ui";

export const START_FIX = "Start fix";
export const STOP_FIX = "Stop fix";
export const UNDO_FIX = "Undo fix";
export const ALLOW = "Allow";
export const DENY = "Deny";

/** Words for a checklist row's mark. */
export function markWords(mark: LiveStepMark): string {
  switch (mark) {
    case "pending":
      return "not started";
    case "running":
      return "running";
    case "needsOk":
      return "waiting for your OK";
    case "done":
      return "done";
    case "failed":
      return "didn't work";
    case "skipped":
      return "didn't run";
    default: {
      const _exhaustive: never = mark;
      return _exhaustive;
    }
  }
}

/** The live fix dialog: checklist, Start / Allow, Stop fix, Undo fix. */
export function LiveWindow({
  initial,
  onClose,
}: {
  initial: LiveView;
  onClose: () => void;
}) {
  const [view, setView] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const close = useRef<HTMLButtonElement>(null);
  const box = useRef<HTMLDivElement>(null);
  useModalFocus(box);

  const dismiss = useCallback(async () => {
    try {
      await pulseLiveClose();
    } catch {
      /* closing still drops the dialog */
    }
    onClose();
  }, [onClose]);

  useEffect(() => {
    close.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void dismiss();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dismiss]);

  async function run(act: () => Promise<LiveView>) {
    setBusy(true);
    setError(null);
    try {
      setView(await act());
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  const awaiting = view.phase === "awaiting";
  const primaryLabel = awaiting ? ALLOW : (view.primary ?? START_FIX);

  return (
    <div className="fix-backdrop" onClick={() => void dismiss()}>
      <div
        ref={box}
        className="card fix-window live-window"
        role="dialog"
        aria-modal="true"
        aria-label={view.label}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="fix-head">
          <h2>{view.label}</h2>
          <button
            ref={close}
            type="button"
            className="quiet icon-button"
            aria-label="Close"
            onClick={() => void dismiss()}
          >
            ×
          </button>
        </div>
        <div className="fix-title">{view.title}</div>
        {view.summary && <p className="live-summary">{view.summary}</p>}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <ol className="live-steps" aria-label="Fix steps">
          {view.steps.map((step, i) => (
            <li key={i} data-mark={step.mark}>
              <span className="live-mark" aria-hidden="true">
                {step.mark === "done"
                  ? "✓"
                  : step.mark === "failed"
                    ? "×"
                    : step.mark === "skipped"
                      ? "–"
                      : step.mark === "pending"
                        ? "·"
                        : "→"}
              </span>
              <span className="live-plain">{step.plain}</span>
              <span className="live-status">{markWords(step.mark)}</span>
            </li>
          ))}
        </ol>
        {view.result && <p className="live-result">{view.result}</p>}
        <div className="card-buttons live-actions">
          {awaiting && (
            <button
              type="button"
              className="quiet"
              disabled={busy}
              onClick={() => void run(() => pulseLiveDeny())}
            >
              {DENY}
            </button>
          )}
          {view.stop && (
            <button
              type="button"
              className="danger"
              disabled={busy}
              onClick={() => void run(() => pulseLiveStop())}
            >
              {view.stop}
            </button>
          )}
          {view.primary && view.primary === UNDO_FIX && (
            <button type="button" disabled={busy} onClick={() => void run(() => pulseLiveUndo())}>
              {UNDO_FIX}
            </button>
          )}
          {(view.phase === "ready" || awaiting) && (
            <button type="button" disabled={busy} onClick={() => void run(() => pulseLiveAllow())}>
              {primaryLabel}
            </button>
          )}
          {(view.phase === "done" || view.phase === "stopped") && !view.primary && (
            <button type="button" onClick={() => void dismiss()}>
              Done
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

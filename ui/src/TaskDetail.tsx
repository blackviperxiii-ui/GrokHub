import { useEffect, useRef, useState } from "react";
import { getTaskDetail, type TaskDetail } from "./api";
import { useModalFocus } from "./ui";

export const NO_STEPS = "No steps yet.";
export const NO_RESULT = "No result yet.";

/** A helper's steps and result, read once when the window opens. Both are
 * the helper's own output, so they show as plain text, never as Markdown. */
export function TaskDetailWindow({
  conversationId,
  taskId,
  onClose,
}: {
  conversationId: string;
  taskId: string;
  onClose: () => void;
}) {
  const [detail, setDetail] = useState<TaskDetail | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const close = useRef<HTMLButtonElement>(null);
  const box = useRef<HTMLDivElement>(null);
  useModalFocus(box);

  useEffect(() => {
    let gone = false;
    getTaskDetail(conversationId, taskId).then(
      (next) => {
        if (!gone) setDetail(next);
      },
      (err) => {
        if (!gone) setNote(String(err));
      },
    );
    return () => {
      gone = true;
    };
  }, [conversationId, taskId]);

  useEffect(() => {
    close.current?.focus();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const label = `Helper ${taskId}`;
  return (
    <div className="fix-backdrop" onClick={onClose}>
      <div
        ref={box}
        className="card fix-window"
        role="dialog"
        aria-modal="true"
        aria-label={label}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="fix-head">
          <h2>{label}</h2>
          <button ref={close} type="button" className="quiet" onClick={onClose}>
            Close
          </button>
        </div>
        {note && (
          <p className="status" role="status">
            {note}
          </p>
        )}
        {detail && (
          <>
            {detail.steps.length > 0 ? (
              <ol className="task-steps" aria-label="Steps">
                {detail.steps.map((step, i) => (
                  <li key={i}>{step}</li>
                ))}
              </ol>
            ) : (
              <p className="empty">{NO_STEPS}</p>
            )}
            <pre className="task-result" tabIndex={0} role="region" aria-label="Result">
              {detail.result || NO_RESULT}
            </pre>
          </>
        )}
      </div>
    </div>
  );
}

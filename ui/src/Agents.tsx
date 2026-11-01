import { useEffect, useState } from "react";
import { getQueue, stopTask, type Queue, type TaskStatus } from "./api";

/** How often the page reads the queue again while it is on screen. */
export const POLL_MS = 2000;

export const AGENTS_NO_CHAT =
  "No chat open yet. Send a message in Chat, and its to-dos and background tasks show up here.";

export const AGENTS_EMPTY = "No jobs yet. Queued work from this chat shows up here.";

export const TASK_LABELS: Record<TaskStatus, string> = {
  running: "Running",
  failed: "Failed",
  done: "Done",
  stopped: "Stopped",
};

const TODO_LABELS: Record<string, string> = {
  pending: "To do",
  in_progress: "In progress",
  completed: "Done",
  cancelled: "Cancelled",
};

/** A to-do's status as the page says it; an unknown one shows as written. */
export function todoLabel(status: string): string {
  return TODO_LABELS[status] ?? status;
}

/** The open chat's to-dos and background tasks, each task with a Stop (the
 * cabin's Queue). It reads them every {@link POLL_MS} while it is open and
 * the window is in view. */
export function Agents({
  conversationId,
  onOpenChat,
}: {
  /** The chat the window last heard from; none before the first message. */
  conversationId?: string;
  onOpenChat: () => void;
}) {
  const [queue, setQueue] = useState<Queue | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  // A different chat starts from nothing, not from the last chat's queue.
  const [shownFor, setShownFor] = useState(conversationId);
  if (shownFor !== conversationId) {
    setShownFor(conversationId);
    setQueue(null);
    setNote(null);
  }

  useEffect(() => {
    if (!conversationId) return;
    const id = conversationId;
    let gone = false;
    function read() {
      if (document.hidden) return;
      getQueue(id).then(
        (next) => {
          if (gone) return;
          setQueue(next);
          setNote(null);
        },
        (err) => {
          if (!gone) setNote(String(err));
        },
      );
    }
    read();
    const timer = window.setInterval(read, POLL_MS);
    document.addEventListener("visibilitychange", read);
    return () => {
      gone = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", read);
    };
  }, [conversationId]);

  async function stop(taskId?: string) {
    if (!conversationId) return;
    setBusy(taskId ?? "*");
    try {
      setQueue(await stopTask(conversationId, taskId));
      setNote(null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(null);
    }
  }

  const tasks = queue?.tasks ?? [];
  const todos = queue?.todos ?? [];
  const running = tasks.filter((task) => task.live).length;

  return (
    <section className="agents" aria-label="Agents">
      <div className="card-buttons">
        <button type="button" disabled={!conversationId} onClick={onOpenChat}>
          Open chat
        </button>
        {running > 0 && (
          <button
            type="button"
            className="danger"
            disabled={busy !== null}
            onClick={() => void stop()}
          >
            {busy === "*" ? "Stopping…" : "Stop all"}
          </button>
        )}
      </div>
      <p className="note">Background jobs and to-dos for the open chat.</p>
      {note && (
        <p className="status" role="status">
          {note}
        </p>
      )}
      {!conversationId && <p className="empty">{AGENTS_NO_CHAT}</p>}
      {conversationId && queue === null && !note && <p className="empty">Loading jobs…</p>}
      {queue && tasks.length === 0 && todos.length === 0 && <p className="empty">{AGENTS_EMPTY}</p>}
      {tasks.length > 0 && (
        <>
          <h2>Tasks</h2>
          <ul className="rows" aria-label="Tasks">
            {tasks.map((task) => (
              <li key={task.id} className={`card agent ${task.status}`} aria-label={task.title}>
                <button type="button" className="card-title link" onClick={onOpenChat}>
                  {task.title}
                </button>
                <div className="card-status">
                  {TASK_LABELS[task.status]} · {task.id}
                </div>
                {task.live && (
                  <div className="card-buttons">
                    <button
                      type="button"
                      className="quiet danger"
                      disabled={busy !== null}
                      onClick={() => void stop(task.id)}
                    >
                      {busy === task.id ? "Stopping…" : "Stop"}
                    </button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        </>
      )}
      {todos.length > 0 && (
        <>
          <h2>To-dos</h2>
          <ul className="rows" aria-label="To-dos">
            {todos.map((todo) => (
              <li key={todo.id} className={`card agent ${todo.status}`} aria-label={todo.content}>
                <button type="button" className="card-title link" onClick={onOpenChat}>
                  {todo.content}
                </button>
                <div className="card-status">{todoLabel(todo.status)}</div>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

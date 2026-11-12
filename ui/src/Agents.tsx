import { useEffect, useState } from "react";
import {
  getQueue,
  onEventGap,
  onTasksChanged,
  stopTask,
  type Queue,
  type QueueProgress,
  type TaskStatus,
  type TodoState,
} from "./api";
import { TaskDetailWindow } from "./TaskDetail";

/** How often the page reads the queue again while it is on screen. */
export const POLL_MS = 2000;

/** A live panel reads when it is told of a change; this read is for a change
 * it was never told of. */
export const LIVE_FALLBACK_MS = 15000;

export const AGENTS_NO_CHAT =
  "No chat open yet. Send a message in Chat, and its to-dos and background tasks show up here.";

export const AGENTS_EMPTY = "No jobs yet. Queued work from this chat shows up here.";

/** The Code tab's Tasks panel, beside the agent window. */
export const TASKS_NOTE = "The lead's tasks and helpers for this session.";
export const TASKS_EMPTY = "No tasks yet. The lead's tasks and helpers show up here.";

/** A task's state in the Tasks panel, from its status and its helper. */
export const LEAD_LABELS: Record<TodoState, string> = {
  planned: "Planned",
  running: "Running",
  needsYou: "Needs you",
  review: "Review",
  done: "Done",
  failed: "Failed",
};

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

/** A helper's progress in one line: its last step, its tokens, its time. */
export function taskProgress(p: QueueProgress): string {
  const parts: string[] = [];
  if (p.steps > 0) parts.push(`Step ${p.steps}: ${p.lastStep}`);
  if (p.tokens > 0) parts.push(`${p.tokens < 1000 ? p.tokens : `${(p.tokens / 1000).toFixed(1)}k`} tokens`);
  parts.push(p.seconds < 60 ? `${p.seconds}s` : `${Math.floor(p.seconds / 60)}m ${p.seconds % 60}s`);
  return parts.join(" · ");
}

/** The open chat's to-dos and background tasks, each task with a Stop (the
 * cabin's Queue). It reads them every {@link POLL_MS} while it is open and
 * the window is in view; a live panel reads when the session's tasks change. */
export function Agents({
  conversationId,
  onOpenChat,
  active = true,
  live = false,
}: {
  /** The chat the window last heard from; none before the first message. */
  conversationId?: string;
  /** Opens the chat. Without it this is the Code tab's Tasks panel, which
   * sits beside its own conversation and has nowhere to go. */
  onOpenChat?: () => void;
  /** False while the panel's tab is not the one on screen: nothing is read. */
  active?: boolean;
  /** Read on the session's change event, not on the {@link POLL_MS} timer. */
  live?: boolean;
}) {
  const [queue, setQueue] = useState<Queue | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  /** The helper whose details window is open. */
  const [detail, setDetail] = useState<string | null>(null);
  // A different chat starts from nothing, not from the last chat's queue.
  const [shownFor, setShownFor] = useState(conversationId);
  if (shownFor !== conversationId) {
    setShownFor(conversationId);
    setQueue(null);
    setNote(null);
    setDetail(null);
  }

  useEffect(() => {
    if (!conversationId || !active || live) return;
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
  }, [conversationId, active, live]);

  useEffect(() => {
    if (!conversationId || !active || !live) return;
    const id = conversationId;
    let dropped = false;
    let asked = 0;
    let stop: (() => void) | undefined;
    let stopGap: (() => void) | undefined;
    function read() {
      if (dropped || document.hidden) return;
      // Reads can overlap; only the answer to the newest one is shown.
      const mine = ++asked;
      getQueue(id).then(
        (next) => {
          if (dropped || mine !== asked) return;
          setQueue(next);
          setNote(null);
        },
        (err) => {
          if (!dropped && mine === asked) setNote(String(err));
        },
      );
    }
    // The first read waits for the listener, so no change falls between them.
    onTasksChanged((changed) => {
      if (changed === id) read();
    }).then(
      (unlisten) => {
        if (dropped) {
          unlisten();
          return;
        }
        stop = unlisten;
        read();
      },
      () => read(),
    );
    onEventGap(read).then(
      (unlisten) => {
        if (dropped) unlisten();
        else stopGap = unlisten;
      },
      () => {},
    );
    const timer = window.setInterval(read, LIVE_FALLBACK_MS);
    document.addEventListener("visibilitychange", read);
    return () => {
      dropped = true;
      stop?.();
      stopGap?.();
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", read);
    };
  }, [conversationId, active, live]);

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
  const panel = !onOpenChat;
  /** A helper's progress by its task id; only the Tasks panel shows it. */
  const progressOf = (id?: string) => (panel ? tasks.find((task) => task.id === id)?.progress : undefined);
  // Beside the agent window the lead's tasks come first, then what runs for them.
  const hubTitle = onOpenChat ? "Tasks" : "Helpers and commands";

  return (
    <section className={onOpenChat ? "agents" : "agents embedded"} aria-label={onOpenChat ? "Agents" : "Agent tasks"}>
      <div className="card-buttons">
        {onOpenChat && (
          <button type="button" disabled={!conversationId} onClick={onOpenChat}>
            Open chat
          </button>
        )}
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
      <p className="note">{onOpenChat ? "Background jobs and to-dos for the open chat." : TASKS_NOTE}</p>
      {note && (
        <p className="status" role="status">
          {note}
        </p>
      )}
      {!conversationId && <p className="empty">{AGENTS_NO_CHAT}</p>}
      {conversationId && queue === null && !note && <p className="empty">Loading jobs…</p>}
      {queue && tasks.length === 0 && todos.length === 0 && (
        <p className="empty">{onOpenChat ? AGENTS_EMPTY : TASKS_EMPTY}</p>
      )}
      {!onOpenChat && todos.length > 0 && (
        <>
          <h2>Tasks</h2>
          <ul className="rows" aria-label="Tasks">
            {todos.map((todo) => {
              const progress = progressOf(todo.owner);
              return (
                <li key={todo.id} className={`card agent ${todo.status}`} aria-label={todo.content}>
                  <div className="card-title">{todo.content}</div>
                  <div className="card-status">
                    {LEAD_LABELS[todo.state]}
                    {todo.owner ? ` · helper ${todo.owner}` : ""}
                  </div>
                  {progress && <div className="card-status task-progress">{taskProgress(progress)}</div>}
                </li>
              );
            })}
          </ul>
        </>
      )}
      {tasks.length > 0 && (
        <>
          <h2>{hubTitle}</h2>
          <ul className="rows" aria-label={hubTitle}>
            {tasks.map((task) => {
              const progress = progressOf(task.id);
              return (
                <li key={task.id} className={`card agent ${task.status}`} aria-label={task.title}>
                  {onOpenChat ? (
                    <button type="button" className="card-title link" onClick={onOpenChat}>
                      {task.title}
                    </button>
                  ) : (
                    <div className="card-title">{task.title}</div>
                  )}
                  <div className="card-status">
                    {TASK_LABELS[task.status]} · {task.id}
                  </div>
                  {progress && <div className="card-status task-progress">{taskProgress(progress)}</div>}
                  {(task.live || progress) && (
                    <div className="card-buttons">
                      {task.live && (
                        <button
                          type="button"
                          className="quiet danger"
                          disabled={busy !== null}
                          onClick={() => void stop(task.id)}
                        >
                          {busy === task.id ? "Stopping…" : "Stop"}
                        </button>
                      )}
                      {progress && (
                        <button
                          type="button"
                          className="quiet"
                          aria-label={`Helper ${task.id} details`}
                          onClick={() => setDetail(task.id)}
                        >
                          Details
                        </button>
                      )}
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </>
      )}
      {onOpenChat && todos.length > 0 && (
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
      {panel && conversationId && detail && (
        <TaskDetailWindow conversationId={conversationId} taskId={detail} onClose={() => setDetail(null)} />
      )}
    </section>
  );
}

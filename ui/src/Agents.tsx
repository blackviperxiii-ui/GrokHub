import { useEffect, useRef, useState } from "react";
import {
  discardTaskWork,
  getQueue,
  mergeTaskWork,
  onEventGap,
  onTasksChanged,
  resumeTaskWork,
  stopTask,
  type HelperWork,
  type Queue,
  type QueueProgress,
  type TaskStatus,
  type TodoState,
} from "./api";
import { TaskDetailWindow } from "./TaskDetail";
import { inShell } from "./transport";
import { useModalFocus } from "./ui";

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
  notMerged: "Not merged",
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

export const WORK_TITLE = "Helper work";
export const WORK_NOTE =
  "A helper's work stays in a worktree of its own until you merge it. Merged files are uncommitted changes in the project folder.";
/** A paired browser lists the work and cannot change the project folder. */
export const WORK_DESKTOP_ONLY = "Merge or discard this work in the desktop app.";

/** How many names a row spells out before it counts the rest. */
const NAMES_SHOWN = 5;

function counted(count: number, noun: string): string {
  return count === 1 ? `1 ${noun}` : `${count} ${noun}s`;
}

/** The first few names, and how many more there are of `total`. */
function someNames(names: string[], total = names.length): string {
  const more = total - Math.min(names.length, NAMES_SHOWN);
  const shown = names.slice(0, NAMES_SHOWN).join(", ");
  return more > 0 ? `${shown} and ${more} more` : shown;
}

function workTotal(work: HelperWork): number {
  return work.counts.added + work.counts.changed + work.counts.deleted;
}

/** "1 new, 2 changed, 0 deleted". */
export function workCounts(work: HelperWork): string {
  return `${work.counts.added} new, ${work.counts.changed} changed, ${work.counts.deleted} deleted`;
}

/** What became of a helper's work, in one line; nothing while it runs. On
 * the helper's own row partial work says how much is kept; a row of the
 * Helper work list says so on its Partial line. */
export function workLine(work: HelperWork, listed = false): string | null {
  const files = counted(workTotal(work), "file");
  switch (work.state) {
    case "running":
      return null;
    case "ready":
      return `Ready to merge · ${files}`;
    case "conflict":
      return `Conflict · ${someNames(work.conflicts)}`;
    case "held":
      return work.why === "partial" && !listed ? `Partial · ${files} kept` : `Held · ${work.whyText ?? "not merged"}`;
    case "merged":
      return `Merged · ${files}`;
    case "empty":
      return "No changes";
    case "discarded":
      return "Discarded";
  }
}

/** The ignored paths a helper wrote, which no merge takes. */
export function ignoredNote(ignored: string[]): string {
  const paths = `${counted(ignored.length, "ignored path")} (${someNames(ignored)})`;
  return ignored.length === 1
    ? `${paths} is not merged. It stays in the worktree until you delete it.`
    : `${paths} are not merged. They stay in the worktree until you delete it.`;
}

/** The Merge a row offers: its label, and whether it asks first. None for
 * work that cannot be merged as it is, and for a merge that stopped part
 * way, which is finished or put back. */
export function mergeOffer(work: HelperWork): { label: string; asks: boolean } | null {
  if (work.landing) return null;
  if (work.state === "ready") return { label: "Merge", asks: false };
  if (work.state === "conflict") return { label: "Merge again", asks: false };
  if (work.state !== "held") return null;
  switch (work.why) {
    case "partial":
    case "gitStateChanged":
    case "changedAfterSeal":
      return { label: "Merge partial work", asks: true };
    // The app closed under the helper: the first Merge only reads what it
    // changed, and the row then asks as partial work does.
    case "interrupted":
      return { label: "Merge partial work", asks: false };
    case "projectChanged":
    case "blocked":
    case "mergeFailed":
    case "rolledBack":
      return { label: "Merge", asks: false };
    default:
      return null;
  }
}

/** On work that still waits after a newer helper redid it. Discard stays
 * the person's click. */
const REDONE_NOTE = "A newer helper redid this";
export const MERGE_PARTIAL_QUESTION = "Merge partial work?";
export const DISCARD_QUESTION = "Discard this work?";
export const DELETE_WORKTREE_QUESTION = "Delete this worktree?";

/** What the confirm before a Merge of partial or changed work says. */
export function mergePartialText(work: HelperWork): string {
  const first =
    work.why === "changedAfterSeal"
      ? "This worktree changed after the helper ended."
      : work.partial
        ? `This helper did not finish (${work.ended}).`
        : "This helper changed its worktree's git state.";
  return `${first} Merge its ${counted(workTotal(work), "file")} (${workCounts(work)}) anyway?`;
}

/** What the confirm before a Discard says. Work that was never sealed has
 * no file count, only its worktree. */
export function discardText(work: HelperWork): string {
  const total = workTotal(work);
  return total > 0
    ? `Delete this helper's ${counted(total, "changed file")}? This can't be undone.`
    : "Delete this helper's worktree and everything in it? This can't be undone.";
}

/** What the confirm before Delete worktree says. */
export function deleteWorktreeText(work: HelperWork): string {
  return work.ignored.length > 0
    ? `Delete the worktree and the ${counted(work.ignored.length, "ignored path")} in it?`
    : "Delete the worktree?";
}

type WorkAction = "merge" | "finish" | "putBack" | "discard";

/** A click's label while its call runs. */
const WORKING: Record<WorkAction, string> = {
  merge: "Merging…",
  finish: "Finishing…",
  putBack: "Putting back…",
  discard: "Deleting…",
};

/** What a click did, when the row cannot say so itself because it is gone
 * or no longer what was clicked. */
function workOutcome(before: HelperWork, action: WorkAction, next: Queue): string | null {
  const after = (next.work ?? []).find((work) => work.key === before.key);
  switch (action) {
    case "discard":
      return before.state === "merged" ? "Worktree deleted." : "Work discarded.";
    case "putBack":
      return after?.landing ? null : "Files put back.";
    case "merge":
    case "finish":
      return !after || after.state === "merged"
        ? `Merged ${counted(workTotal(before), "file")} into the project.`
        : null;
  }
}

/** Asks before a step on a helper's work. Escape or the quiet button closes
 * it, and focus starts on the quiet button. */
function WorkConfirm({
  question,
  text,
  yes,
  no,
  danger,
  onConfirm,
  onClose,
}: {
  question: string;
  text: string;
  yes: string;
  no: string;
  danger: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const keep = useRef<HTMLButtonElement>(null);
  const box = useRef<HTMLDivElement>(null);
  useModalFocus(box);
  // The panel reads its queue again while this window is open, and each
  // read hands over a new onClose. Focus is set once, when the window
  // opens, so a read never takes it back from the button the person chose.
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  }, [onClose]);
  useEffect(() => {
    keep.current?.focus();
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") close.current();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <div className="fix-backdrop" onClick={onClose}>
      <div
        ref={box}
        className="card fix-window confirm-window"
        role="alertdialog"
        aria-modal="true"
        aria-label={question}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{question}</h2>
        <p>{text}</p>
        <div className="card-buttons">
          <button type="button" className={danger ? "danger" : undefined} onClick={onConfirm}>
            {yes}
          </button>
          <button ref={keep} type="button" className="quiet" onClick={onClose}>
            {no}
          </button>
        </div>
      </div>
    </div>
  );
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
  /** The helper work a confirm window asks about, and the step it asks for. */
  const [ask, setAsk] = useState<{ key: string; step: "merge" | "discard" } | null>(null);
  /** A refused click, shown on its row. */
  const [refused, setRefused] = useState<{ key: string; text: string } | null>(null);
  /** What the last click on helper work did. */
  const [said, setSaid] = useState<string | null>(null);
  /** The helper work whose click just ended: focus goes back to its row. */
  const [ended, setEnded] = useState<{ key: string } | null>(null);
  const section = useRef<HTMLElement>(null);
  const saidLine = useRef<HTMLParagraphElement>(null);
  // A different chat starts from nothing, not from the last chat's queue.
  const [shownFor, setShownFor] = useState(conversationId);
  if (shownFor !== conversationId) {
    setShownFor(conversationId);
    setQueue(null);
    setNote(null);
    setDetail(null);
    setAsk(null);
    setRefused(null);
    setSaid(null);
    setEnded(null);
  }
  /** The chat on screen now, for a call that answers after a switch. */
  const onScreen = useRef(conversationId);
  useEffect(() => {
    onScreen.current = conversationId;
  }, [conversationId]);

  // A click disables its button, and its row may be gone when the call
  // ends. Focus that was left on nothing goes to the row's first button, or
  // to the line that says what happened.
  useEffect(() => {
    if (!ended) return;
    const at = document.activeElement;
    if (at instanceof HTMLElement && at !== document.body && at.isConnected && !at.matches(":disabled")) return;
    const row = [...(section.current?.querySelectorAll<HTMLElement>("[data-work]") ?? [])].find(
      (el) => el.dataset.work === ended.key,
    );
    (row?.querySelector<HTMLElement>("button:not(:disabled)") ?? saidLine.current)?.focus();
  }, [ended]);

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

  async function act(work: HelperWork, action: WorkAction) {
    if (!conversationId) return;
    const id = conversationId;
    setAsk(null);
    setRefused(null);
    setSaid(null);
    setBusy(`${action}:${work.key}`);
    try {
      const next = await (action === "merge"
        ? mergeTaskWork(id, work.key)
        : action === "discard"
          ? discardTaskWork(id, work.key)
          : resumeTaskWork(id, work.key, action === "finish"));
      if (onScreen.current !== id) return;
      setQueue(next);
      setSaid(workOutcome(work, action, next));
    } catch (err) {
      if (onScreen.current === id) setRefused({ key: work.key, text: String(err) });
    } finally {
      setBusy(null);
      if (onScreen.current === id) setEnded({ key: work.key });
    }
  }

  const tasks = queue?.tasks ?? [];
  const todos = queue?.todos ?? [];
  const work = queue?.work ?? [];
  // Work that waits for a click, from any launch. What a helper of this
  // launch made also shows on its own row.
  const waiting = work.filter(
    (row) =>
      row.state === "ready" || row.state === "conflict" || row.state === "held" || (row.state === "merged" && row.treeKept),
  );
  const asked = ask ? waiting.find((row) => row.key === ask.key) : undefined;
  const desktop = inShell();
  /** The line of the work a helper of this launch made, for its own row. */
  const workOf = (id: string) => {
    const mine = work.filter((row) => row.live && row.task === id).pop();
    return mine ? workLine(mine) : null;
  };
  const working = (action: WorkAction, key: string, label: string) =>
    busy === `${action}:${key}` ? WORKING[action] : label;
  const running = tasks.filter((task) => task.live).length;
  const panel = !onOpenChat;
  /** A helper's progress by its task id; only the Tasks panel shows it. */
  const progressOf = (id?: string) => (panel ? tasks.find((task) => task.id === id)?.progress : undefined);
  // Beside the agent window the lead's tasks come first, then what runs for them.
  const hubTitle = onOpenChat ? "Tasks" : "Helpers and commands";

  return (
    <section
      ref={section}
      className={onOpenChat ? "agents" : "agents embedded"}
      aria-label={onOpenChat ? "Agents" : "Agent tasks"}
    >
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
      {queue && tasks.length === 0 && todos.length === 0 && waiting.length === 0 && !queue.workError && !said && (
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
              const made = workOf(task.id);
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
                  {made && <div className="card-status task-work">{made}</div>}
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
      {(waiting.length > 0 || queue?.workError || said) && (
        <>
          <h2>{WORK_TITLE}</h2>
          {waiting.length > 0 && <p className="note">{desktop ? WORK_NOTE : WORK_DESKTOP_ONLY}</p>}
          {queue?.workError && (
            <p className="error" role="alert">
              The helper work list can't be read: {queue.workError}
            </p>
          )}
          {said && (
            <p ref={saidLine} className="status work-said" role="status" tabIndex={-1}>
              {said}
            </p>
          )}
          {waiting.length > 0 && (
            <ul className="rows" aria-label={WORK_TITLE}>
              {waiting.map((row) => {
                const total = workTotal(row);
                const merge = mergeOffer(row);
                const kept = row.state === "merged";
                return (
                  <li key={row.key} data-work={row.key} className={`card agent work ${row.state}`} aria-label={row.title}>
                    <div className="card-title">{row.title}</div>
                    <div className="card-status task-work">{workLine(row, true)}</div>
                    {row.partial && <div className="card-status work-partial">Partial · {row.ended}</div>}
                    {row.redoOf && <div className="card-status work-detail">Redo of: {row.redoOf}</div>}
                    {row.redone && <div className="card-status work-detail">{REDONE_NOTE}</div>}
                    {total > 0 && <div className="card-status work-detail">{workCounts(row)}</div>}
                    {row.files.length > 0 && (
                      <div className="card-status work-detail">Files: {someNames(row.files, total)}</div>
                    )}
                    {row.skipped.length > 0 && (
                      <div className="card-status work-detail">Left as you changed them: {someNames(row.skipped)}</div>
                    )}
                    {row.treeKept && <div className="card-status work-detail">Worktree: {row.path}</div>}
                    {row.ignored.length > 0 && (
                      <div className="card-status work-detail">{ignoredNote(row.ignored)}</div>
                    )}
                    {refused?.key === row.key && (
                      <p className="error" role="alert">
                        {refused.text}
                      </p>
                    )}
                    {desktop && (
                      <div className="card-buttons">
                        {row.landing && (
                          <>
                            <button type="button" disabled={busy !== null} onClick={() => void act(row, "finish")}>
                              {working("finish", row.key, "Finish merge")}
                            </button>
                            <button
                              type="button"
                              className="quiet"
                              disabled={busy !== null}
                              onClick={() => void act(row, "putBack")}
                            >
                              {working("putBack", row.key, "Put files back")}
                            </button>
                          </>
                        )}
                        {merge && (
                          <button
                            type="button"
                            disabled={busy !== null}
                            onClick={() => (merge.asks ? setAsk({ key: row.key, step: "merge" }) : void act(row, "merge"))}
                          >
                            {working("merge", row.key, merge.label)}
                          </button>
                        )}
                        {!row.landing && (
                          <button
                            type="button"
                            className="quiet danger"
                            disabled={busy !== null}
                            onClick={() => setAsk({ key: row.key, step: "discard" })}
                          >
                            {working("discard", row.key, kept ? "Delete worktree" : "Discard")}
                          </button>
                        )}
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
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
      {asked && ask?.step === "merge" && (
        <WorkConfirm
          question={MERGE_PARTIAL_QUESTION}
          text={mergePartialText(asked)}
          yes="Merge partial work"
          no="Cancel"
          danger={false}
          onConfirm={() => void act(asked, "merge")}
          onClose={() => setAsk(null)}
        />
      )}
      {asked && ask?.step === "discard" && (
        <WorkConfirm
          question={asked.state === "merged" ? DELETE_WORKTREE_QUESTION : DISCARD_QUESTION}
          text={asked.state === "merged" ? deleteWorktreeText(asked) : discardText(asked)}
          yes={asked.state === "merged" ? "Delete worktree" : "Discard"}
          no="Keep"
          danger
          onConfirm={() => void act(asked, "discard")}
          onClose={() => setAsk(null)}
        />
      )}
      {panel && conversationId && detail && (
        <TaskDetailWindow conversationId={conversationId} taskId={detail} onClose={() => setDetail(null)} />
      )}
    </section>
  );
}

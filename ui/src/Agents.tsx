import { useEffect, useRef, useState } from "react";
import {
  codeNewBranch,
  codeTestPlan,
  codeTestRun,
  discardTaskWork,
  getQueue,
  mergeTaskWork,
  onEventGap,
  onTasksChanged,
  resumeTaskWork,
  shipCommit,
  shipStatus,
  shipWaive,
  stopTask,
  type HelperWork,
  type Queue,
  type QueueProgress,
  type Review,
  type ReviewFinding,
  type SecretHit,
  type ShipStatus,
  type TaskStatus,
  type TestsCheck,
  type TodoState,
} from "./api";
import { CodeShip } from "./CodeShip";
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

export const REVIEW_TITLE = "Review";
export const REVIEW_NOTE =
  "A read-only helper checks the change against the plan you approved. A blocker is a finding that must be fixed before the change ships.";
export const NOT_REVIEWED = "Not reviewed";
/** A paired browser lists the findings and starts nothing. */
export const REVIEW_DESKTOP_ONLY = "Start a review in the desktop app.";
/** A paired browser reads no checks, so it cannot say a review is stale. */
export const REVIEW_AGE_DESKTOP_ONLY = "The desktop app says whether this review is still current.";
/** What Review sends the lead, as a message in the session. */
export const REVIEW_ASK = "Review the change: start one helper with type review, then tell me what it found.";
/** What Ask the lead to fix sends. */
export const FIX_ASK = "Fix each blocker the review found through a helper, then review again.";

export const FINDING_LABELS: Record<ReviewFinding["severity"], string> = {
  high: "Blocker",
  medium: "Medium",
  low: "Low",
  info: "Note",
};

/** "Reviewed: 1 blocker, 2 notes". A note is any finding that is not a
 * blocker. A review with no approved plan says so, and so does one whose
 * helper was shown only part of a long change. */
export function reviewLine(review: Review): string {
  const notes = review.findings.length - review.blockers;
  const parts: string[] = [];
  if (review.blockers > 0) parts.push(counted(review.blockers, "blocker"));
  if (notes > 0 || parts.length === 0) parts.push(counted(notes, "note"));
  const plan = review.planGiven ? "" : " · no plan";
  const cut = review.cut ? " · only part of the change was shown" : "";
  const waived = review.waived?.length ? ` · ${review.waived.length} waived` : "";
  return `Reviewed: ${parts.join(", ")}${waived}${plan}${cut}`;
}

/** What the confirm before Waive asks. */
export const WAIVE_QUESTION = "Ship with this finding open?";

/** The review read another change: a file, HEAD or a commit differs now. */
export const REVIEW_STALE = "Stale: files or commits changed since the review";

export const CHECKS_TITLE = "Checks";
export const CHECKS_NOTE =
  "What has to hold before this change can be committed. Reading the checks changes nothing in the project.";
export const CHECKS_BLOCKED = "In the way of a commit:";
export const CHECKS_PASS = "Every check passes.";
/** A test run that wrote to the project does not count for the change. */
export const TESTS_CHANGED_FILES = "The tests changed files; run them again.";
export const TEST_RUN_QUESTION = "Run the tests?";

/** The tests check in one line, with the exact command. */
export function testsLine(tests: TestsCheck): string {
  switch (tests.state) {
    case "noCommand":
      return "No test command is known, so no tests are run.";
    case "notRun":
      return `Not run: ${tests.command}`;
    case "stale":
      return `Files changed since the last run: ${tests.command}`;
    case "failed":
      return `Failed (exit ${tests.exit}): ${tests.command}`;
    case "passed":
      return `Passed: ${tests.command}`;
  }
}

const SECRET_LABELS: Record<SecretHit["kind"], string> = {
  name: "a file that holds secrets",
  held: "one of this project's secrets",
  shape: "looks like a key or a password",
};

/** How many flagged places the Secret scan row lists before it counts the rest. */
const SECRETS_SHOWN = 20;

/** "src/client.ts:12 · looks like a key or a password". The place only,
 * never the line's text. */
export function secretLine(hit: SecretHit): string {
  const commit = hit.commit ? ` · in commit ${hit.commit.slice(0, 8)}` : "";
  const allowed = hit.allowed ? " · allowed" : "";
  return `${secretPlace(hit)} · ${SECRET_LABELS[hit.kind]}${commit}${allowed}`;
}

/** `path:line`, as Allow names a flagged line. */
export function secretPlace(hit: SecretHit): string {
  return hit.line === null ? hit.path : `${hit.path}:${hit.line}`;
}

/** How the app is told a flagged line is allowed: `path:line@<tree id>`.
 * The tree id names every file's content, so the Allow holds only while the
 * folder is what the person looked at (crates/grokhub-code/src/commit.rs). */
export function allowKey(hit: SecretHit, tree: string): string {
  return `${secretPlace(hit)}@${tree}`;
}

/** What the app says about the commit message. They are read again on the
 * Commit click, so a message the person changed since is not held back by
 * what was said about the last one. */
export const MESSAGE_BLOCKS = [
  "Write a commit message.",
  "The first line of the commit message is longer than 200 characters.",
  "The commit message holds a NUL character.",
];

/** What the checks say when the change is committed. */
export const NOTHING_TO_COMMIT = "There is nothing to commit.";

export const COMMIT_TITLE = "Commit";
export const COMMIT_QUESTION = "Commit this change?";

/** "2 commits were made during this session and will be pushed with this one." */
export function sessionCommitsLine(count: number): string {
  return `${counted(count, "commit")} ${count === 1 ? "was" : "were"} made during this session and will be pushed with this one.`;
}

/** What the Commit section says once the change is committed. */
export function committedLine(sha: string): string {
  return `Committed ${sha.slice(0, 8)}. Nothing is left to commit.`;
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

/** A job waiting on the Queue (src-tauri/src/queue.rs `Job`, the cabin's
 * `AgentJob`). Run sends its prompt to Chat as a normal turn. */
export interface QueuedJob {
  title: string;
  status: string;
  prompt: string;
  /** The chat it belongs to; empty for the open one. */
  threadId: string;
}

/** The queued jobs in a read of the queue. A desktop older than this page
 * sends none. */
export function queuedJobs(queue: Queue | null): QueuedJob[] {
  const jobs = (queue as { jobs?: unknown } | null)?.jobs;
  return Array.isArray(jobs) ? (jobs as QueuedJob[]) : [];
}

/** The open chat's to-dos and background tasks, each task with a Stop, and
 * the queued jobs, each with a Run (the cabin's Queue). It reads them every {@link POLL_MS} while it is open and
 * the window is in view; a live panel reads when the session's tasks change. */
export function Agents({
  conversationId,
  onOpenChat,
  onRun,
  active = true,
  live = false,
  onAsk,
  onShipped,
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
  /** Run on a queued job: Chat opens and sends its prompt in its chat. The
   * Code tab's panel has no Run. */
  onRun?: (prompt: string, threadId: string) => void;
  /** Sends a message to the session's lead. With it the Tasks panel has a
   * Review section. */
  onAsk?: (text: string) => void;
  /** Told after a commit, push or pull request made from the Tasks panel, so
   * the Code tab reads the repo again. */
  onShipped?: () => void;
}) {
  const [queue, setQueue] = useState<Queue | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  /** Jobs started from here, by position, shown as running (the cabin's Run). */
  const [started, setStarted] = useState<number[]>([]);
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
  /** The queue a Review click was made on: its buttons wait for the next
   * read, and then for the turn the click started to end. */
  const [askedOn, setAskedOn] = useState<Queue | null>(null);
  /** The checks on the session's change, and the state they were read for. */
  const [ship, setShip] = useState<{ key: string; status: ShipStatus } | null>(null);
  const [shipNote, setShipNote] = useState<string | null>(null);
  /** Counts the reads the person or a test run asked for. */
  const [shipAsk, setShipAsk] = useState(0);
  /** The test command a confirm window asks about. */
  const [testAsk, setTestAsk] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  /** How the last test run from this panel ended. */
  const [testSaid, setTestSaid] = useState<string | null>(null);
  /** The blocker a Waive confirm asks about, by its place in the findings,
   * and the review it was shown in, by that review's time. */
  const [waiveAsk, setWaiveAsk] = useState<{ at: number; atMs: number } | null>(null);
  /** The flagged lines the person allowed, as `path:line@<tree id>`. */
  const [allowed, setAllowed] = useState<string[]>([]);
  /** The commit message the person typed; null for the default. */
  const [message, setMessage] = useState<string | null>(null);
  /** The message changed since the checks last read one. */
  const [messageEdited, setMessageEdited] = useState(false);
  /** The name typed for New branch. */
  const [branchName, setBranchName] = useState("");
  /** The confirm text a Commit confirm window shows. */
  const [commitAsk, setCommitAsk] = useState<string | null>(null);
  const [committing, setCommitting] = useState(false);
  /** What the last Commit or Waive click did. */
  const [commitSaid, setCommitSaid] = useState<string | null>(null);
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
    setAskedOn(null);
    setShip(null);
    setShipNote(null);
    setTestAsk(null);
    setTestSaid(null);
    setWaiveAsk(null);
    setAllowed([]);
    setMessage(null);
    setMessageEdited(false);
    setBranchName("");
    setCommitAsk(null);
    setCommitSaid(null);
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

  // The checks are read when the section shows, when the review, the helper
  // work or the turn changes, and when asked. A snapshot of the project is
  // not taken on every nudge.
  const shipKey =
    !onOpenChat && onAsk && queue && inShell() && (queue.review || queue.reviewError || queue.todos.length > 0)
      ? JSON.stringify([
          queue.review?.atMs ?? null,
          queue.review?.waived ?? [],
          queue.working ?? false,
          (queue.work ?? []).map((row) => [row.key, row.state]),
        ])
      : null;
  const allowedKey = allowed.join("\n");
  useEffect(() => {
    if (!conversationId || !active || shipKey === null) return;
    let gone = false;
    const lines = allowedKey ? allowedKey.split("\n") : [];
    (lines.length > 0 ? shipStatus(conversationId, { allowed: lines }) : shipStatus(conversationId)).then(
      (status) => {
        if (gone) return;
        setShip({ key: shipKey, status });
        setShipNote(null);
        // An Allow given for other files than these holds nothing: drop it.
        const tree = status.commit?.tree;
        setAllowed((now) => {
          const kept = tree ? now.filter((one) => one.endsWith(`@${tree}`)) : [];
          return kept.length === now.length ? now : kept;
        });
      },
      (err) => {
        if (!gone) setShipNote(String(err));
      },
    );
    return () => {
      gone = true;
    };
  }, [conversationId, active, shipKey, shipAsk, allowedKey]);

  /** Commit: the checks are read again for the message and the allowed
   * lines, and their exact confirm text is asked; the yes commits. */
  async function commitNow(confirmed?: string) {
    if (!conversationId || committing || shipKey === null) return;
    const id = conversationId;
    const key = shipKey;
    const text = message ?? ship?.status.commit?.message ?? "";
    setCommitAsk(null);
    setCommitSaid(null);
    setCommitting(true);
    try {
      if (confirmed === undefined) {
        const next = await shipStatus(id, { message: text, allowed });
        if (onScreen.current !== id) return;
        setShip({ key, status: next });
        setMessageEdited(false);
        const confirm = next.commit?.confirm;
        if (next.blocked.length === 0 && confirm) setCommitAsk(confirm);
        return;
      }
      const done = await shipCommit(id, text, allowed, confirmed);
      if (onScreen.current !== id) return;
      setShip({ key, status: done.status });
      setCommitSaid(done.said);
      setMessage(null);
      setMessageEdited(false);
      setAllowed([]);
      onShipped?.();
    } catch (err) {
      if (onScreen.current === id) setCommitSaid(String(err));
    } finally {
      setCommitting(false);
    }
  }

  /** Waive: the person ships with this blocker open. The call names the
   * review the confirm showed, by its tree and its time. */
  async function waiveNow(at: number, atMs: number) {
    const tree = queue?.review?.tree;
    if (!conversationId || tree === undefined) return;
    const id = conversationId;
    setWaiveAsk(null);
    setCommitSaid(null);
    try {
      const next = await shipWaive(id, tree, atMs, at);
      if (onScreen.current === id) setQueue(next);
    } catch (err) {
      if (onScreen.current === id) setCommitSaid(String(err));
    }
  }

  /** New branch: the session's folder moves to a new branch at HEAD, with
   * its files, so the change can be committed there. */
  async function makeBranch() {
    const name = branchName.trim();
    if (!conversationId || !name) return;
    const id = conversationId;
    setCommitSaid(null);
    try {
      const made = await codeNewBranch(name, id);
      if (onScreen.current !== id) return;
      setBranchName("");
      setCommitSaid(`On ${made} now.`);
      setShipAsk((n) => n + 1);
    } catch (err) {
      if (onScreen.current === id) setCommitSaid(String(err));
    }
  }

  /** Runs the session's test command in its folder. The first run of a
   * command asks with the exact command. */
  async function runTests(confirmed?: string) {
    if (!conversationId || testing) return;
    const id = conversationId;
    setTestAsk(null);
    setTestSaid(null);
    setTesting(true);
    try {
      let command = confirmed;
      if (command === undefined) {
        const plan = await codeTestPlan(null, id);
        if (onScreen.current !== id) return;
        if (plan.needsConfirm) {
          setTestAsk(plan.command);
          return;
        }
        command = plan.command;
      }
      const result = await codeTestRun(command, confirmed !== undefined, id);
      if (onScreen.current !== id) return;
      setTestSaid(
        result.changedFiles ? TESTS_CHANGED_FILES : result.exit === 0 ? "Tests passed." : `Tests exited with ${result.exit}.`,
      );
      setShipAsk((n) => n + 1);
    } catch (err) {
      if (onScreen.current === id) setTestSaid(String(err));
    } finally {
      setTesting(false);
    }
  }

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
  const jobs = onRun ? queuedJobs(queue) : [];
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
  const review = queue?.review;
  // A session with a lead and something to review, or a review to show.
  const reviewShown = panel && !!onAsk && !!queue && (!!review || !!queue.reviewError || todos.length > 0);
  const askLead = (text: string) => {
    setAskedOn(queue);
    onAsk?.(text);
  };
  // An ask sent while a turn runs waits behind it, and a second click would
  // wait there too.
  const asking = askedOn === queue || !!queue?.working;
  const checks = ship?.status.checks;
  // Only a read of the state on screen says the review is stale.
  const current = !!ship && ship.key === shipKey;
  const stale = current && checks?.review.state === "stale";
  // A blocker is waived only while the checks say this review's blockers
  // are what is open, and no turn runs.
  const canWaive = desktop && current && checks?.review.state === "blockers" && !queue?.working;
  const waivedAt = new Set(review?.waived ?? []);
  const plan = ship?.status.commit ?? null;
  const shipped = ship?.status.shipped ?? null;
  // What is in the way now: what was said about the last message goes once
  // the person changes it, and once the change is committed, that nothing
  // is left to commit is no hindrance.
  const inTheWay = (ship?.status.blocked ?? []).filter(
    (why) => !(messageEdited && MESSAGE_BLOCKS.includes(why)) && !(shipped && why === NOTHING_TO_COMMIT),
  );
  /** A helper's progress by its task id; only the Tasks panel shows it. */
  const progressOf = (id?: string) => (panel ? tasks.find((task) => task.id === id)?.progress : undefined);
  // Beside the agent window the lead's tasks come first, then what runs for them.
  const hubTitle = onOpenChat ? "Tasks" : "Helpers and commands";

  function run(i: number, job: QueuedJob) {
    setStarted((now) => (now.includes(i) ? now : [...now, i]));
    onRun?.(job.prompt, job.threadId);
  }

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
      {queue && tasks.length === 0 && todos.length === 0 && jobs.length === 0 && waiting.length === 0 && !queue.workError && !said && !reviewShown && (
        <p className="empty">{onOpenChat ? AGENTS_EMPTY : TASKS_EMPTY}</p>
      )}
      {jobs.length > 0 && (
        <>
          <h2>Queued</h2>
          <ul className="rows" aria-label="Queued">
            {jobs.map((job, i) => (
              <li key={`${i}-${job.title}`} className="card agent" aria-label={job.title}>
                <div className="card-title">{job.title}</div>
                <div className="card-status">{started.includes(i) ? "running" : job.status}</div>
                <div className="card-buttons">
                  <button type="button" className="quiet" onClick={() => run(i, job)}>
                    Run
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </>
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
      {reviewShown && (
        <>
          <h2>{REVIEW_TITLE}</h2>
          <p className="note">{REVIEW_NOTE}</p>
          {queue.reviewError && (
            <p className="error" role="alert">
              The review can't be read: {queue.reviewError}
            </p>
          )}
          {!queue.reviewError && (
            <p className="status review-line" role="status">
              {review ? (stale ? REVIEW_STALE : reviewLine(review)) : NOT_REVIEWED}
            </p>
          )}
          {review && (
            <ul className="rows" aria-label="Review findings">
              {review.findings.map((finding, at) => (
                <li key={at} className={`card agent finding ${finding.severity}`}>
                  <div className="card-title">{finding.text}</div>
                  <div className="card-status">
                    {FINDING_LABELS[finding.severity]}
                    {waivedAt.has(at) ? " · waived" : ""}
                  </div>
                  {canWaive && finding.severity === "high" && !waivedAt.has(at) && (
                    <div className="card-buttons">
                      <button
                        type="button"
                        className="quiet"
                        aria-label={`Waive ${finding.text}`}
                        onClick={() => setWaiveAsk({ at, atMs: review.atMs })}
                      >
                        Waive
                      </button>
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
          {desktop ? (
            <div className="card-buttons">
              <button type="button" disabled={asking} onClick={() => askLead(REVIEW_ASK)}>
                {review ? "Review again" : "Review"}
              </button>
              {review && review.blockers > 0 && (
                <button type="button" className="quiet" disabled={asking} onClick={() => askLead(FIX_ASK)}>
                  Ask the lead to fix
                </button>
              )}
            </div>
          ) : (
            <p className="note">
              {REVIEW_DESKTOP_ONLY}
              {review ? ` ${REVIEW_AGE_DESKTOP_ONLY}` : ""}
            </p>
          )}
        </>
      )}
      {reviewShown && desktop && (
        <>
          <h2>{CHECKS_TITLE}</h2>
          <p className="note">{CHECKS_NOTE}</p>
          {shipNote && (
            <p className="error" role="alert">
              The checks can't be read: {shipNote}
            </p>
          )}
          {checks && (
            <ul className="rows" aria-label={CHECKS_TITLE}>
              <li className="card agent" aria-label="Tests">
                <div className="card-title">Tests</div>
                <div className="card-status">{testsLine(checks.tests)}</div>
                {testSaid && (
                  <div className="card-status" role="status">
                    {testSaid}
                  </div>
                )}
                {checks.tests.state !== "noCommand" && (
                  <div className="card-buttons">
                    <button type="button" className="quiet" disabled={testing} onClick={() => void runTests()}>
                      {testing ? "Running…" : "Run"}
                    </button>
                  </div>
                )}
              </li>
              <li className="card agent" aria-label="Secret scan">
                <div className="card-title">Secret scan</div>
                {checks.secrets === null && <div className="card-status">The scan could not run.</div>}
                {checks.secrets?.length === 0 && <div className="card-status">Nothing flagged.</div>}
                {checks.secrets?.slice(0, SECRETS_SHOWN).map((hit, at) => (
                  <div key={at} className="card-status">
                    <span>{secretLine(hit)}</span>
                    {hit.kind === "shape" && hit.commit === null && plan && (
                      <button
                        type="button"
                        className="quiet"
                        aria-label={`${hit.allowed ? "Undo allow" : "Allow"} ${secretPlace(hit)}`}
                        onClick={() => {
                          const key = allowKey(hit, plan.tree);
                          setAllowed((now) =>
                            hit.allowed ? now.filter((one) => one !== key) : [...now.filter((one) => one !== key), key],
                          );
                        }}
                      >
                        {hit.allowed ? "Undo" : "Allow"}
                      </button>
                    )}
                  </div>
                ))}
                {checks.secrets && checks.secrets.length > SECRETS_SHOWN && (
                  <div className="card-status">and {checks.secrets.length - SECRETS_SHOWN} more</div>
                )}
              </li>
              <li className="card agent" aria-label="Branch">
                <div className="card-title">Branch</div>
                <div className="card-status">{checks.branch ?? "Detached HEAD"}</div>
                {checks.newBranch && !shipped && (
                  <div className="code-test-row">
                    <label>
                      New branch
                      <input
                        value={branchName}
                        spellCheck={false}
                        autoComplete="off"
                        placeholder="code/my-change"
                        onChange={(e) => setBranchName(e.target.value)}
                      />
                    </label>
                    <button type="button" onClick={() => void makeBranch()} disabled={!branchName.trim()}>
                      Make branch
                    </button>
                  </div>
                )}
              </li>
            </ul>
          )}
          {ship && inTheWay.length > 0 && (
            <>
              <p className="status" role="status">
                {CHECKS_BLOCKED}
              </p>
              <ul className="rows" aria-label="In the way of a commit">
                {inTheWay.map((why, at) => (
                  <li key={at} className="card agent">
                    <div className="card-status">{why}</div>
                  </li>
                ))}
              </ul>
            </>
          )}
          {ship && inTheWay.length === 0 && !shipped && (
            <p className="status" role="status">
              {CHECKS_PASS}
            </p>
          )}
          <div className="card-buttons">
            <button type="button" className="quiet" onClick={() => setShipAsk((n) => n + 1)}>
              Check again
            </button>
          </div>
          {plan && (plan.files.length > 0 || shipped) && (
            <>
              <h2>{COMMIT_TITLE}</h2>
              {shipped && !commitSaid && (
                <p className="status" role="status">
                  {committedLine(shipped)}
                </p>
              )}
              {plan.files.length > 0 && (
                <>
                  <ul className="rows" aria-label="Changed files">
                    {plan.files.map((file) => (
                      <li key={file.path} className="card agent">
                        <div className="card-status">
                          {file.status} {file.path}
                        </div>
                      </li>
                    ))}
                  </ul>
                  {plan.more > 0 && <p className="note">and {plan.more} more</p>}
                  {plan.commits.length > 0 && (
                    <>
                      <p className="note">{sessionCommitsLine(plan.commits.length)}</p>
                      <ul className="rows" aria-label="Session commits">
                        {plan.commits.map((one) => (
                          <li key={one.sha} className="card agent">
                            <div className="card-status">
                              {one.sha.slice(0, 8)} {one.subject}
                            </div>
                          </li>
                        ))}
                      </ul>
                    </>
                  )}
                  <textarea
                    aria-label="Commit message"
                    rows={3}
                    value={message ?? plan.message}
                    onChange={(e) => {
                      setMessage(e.target.value);
                      setMessageEdited(true);
                    }}
                  />
                  <p className="note">
                    Branch: {checks?.branch ?? "Detached HEAD"}
                    {plan.author ? ` · Author: ${plan.author}` : ""}
                  </p>
                  {plan.notices.map((notice) => (
                    <p key={notice} className="note">
                      {notice}
                    </p>
                  ))}
                  <div className="card-buttons">
                    <button
                      type="button"
                      disabled={!current || !ship || inTheWay.length > 0 || committing}
                      onClick={() => void commitNow()}
                    >
                      {committing ? "Committing…" : "Commit"}
                    </button>
                  </div>
                </>
              )}
            </>
          )}
          {commitSaid && (
            <p className="status" role="status">
              {commitSaid}
            </p>
          )}
          {shipped && conversationId && (
            // The commit made here is pushed by its id, after the app's
            // checks, and every push and PR asks with the exact command.
            <CodeShip
              conversationId={conversationId}
              refreshKey={shipAsk}
              onShipped={() => {
                setShipAsk((n) => n + 1);
                onShipped?.();
              }}
            />
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
      {testAsk !== null && (
        <WorkConfirm
          question={TEST_RUN_QUESTION}
          text={testAsk}
          yes="Run"
          no="Cancel"
          danger={false}
          onConfirm={() => void runTests(testAsk)}
          onClose={() => setTestAsk(null)}
        />
      )}
      {waiveAsk !== null && review?.atMs === waiveAsk.atMs && review.findings[waiveAsk.at] && (
        // Shown only while the review on screen is the one Waive was pressed
        // in: a new review closes it.
        <WorkConfirm
          question={WAIVE_QUESTION}
          text={review.findings[waiveAsk.at].text}
          yes="Waive"
          no="Cancel"
          danger
          onConfirm={() => void waiveNow(waiveAsk.at, waiveAsk.atMs)}
          onClose={() => setWaiveAsk(null)}
        />
      )}
      {commitAsk !== null && (
        <WorkConfirm
          question={COMMIT_QUESTION}
          text={commitAsk}
          yes="Commit"
          no="Cancel"
          danger={false}
          onConfirm={() => void commitNow(commitAsk)}
          onClose={() => setCommitAsk(null)}
        />
      )}
      {panel && conversationId && detail && (
        <TaskDetailWindow conversationId={conversationId} taskId={detail} onClose={() => setDetail(null)} />
      )}
    </section>
  );
}

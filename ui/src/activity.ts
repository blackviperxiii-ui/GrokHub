// The Activity Dock's model. It keeps no store of its own: it reads the chat
// event stream and the Automations runs the shell already reports, and keeps
// the latest state of each one.
import type { BoardPage, ChatEnvelope } from "./api";

/** `queued`: lined up behind the running reply, in order. */
export type ActivityStatus = "running" | "waiting" | "queued" | "done" | "failed";

export interface ActivityItem {
  /** `chat:<conversation>` or `run:<automation id>`. */
  key: string;
  /** `queued`: a message sent while a reply ran, waiting its turn. */
  kind: "chat" | "routine" | "queued";
  title: string;
  status: ActivityStatus;
  /** The latest step, the approval it waits on, or how it ended. */
  detail: string;
  /** Tool steps finished in this turn, for the progress line. */
  steps: number;
  /** When the item last changed state (ms), newest first in the dock. Streamed
   * reply text doesn't count: it would re-render the app on every chunk. */
  at: number;
}

/** Done and failed items stay this long, then leave the dock. */
export const DONE_TTL_MS = 10 * 60 * 1000;
/** The dock lists at most this many items; the rest are counted. */
export const DOCK_MAX = 6;

export const CHAT_TITLE = "Chat reply";
const DETAIL_MAX = 90;

function clip(text: string): string {
  const one = text.replace(/\s+/g, " ").trim();
  return one.length > DETAIL_MAX ? `${one.slice(0, DETAIL_MAX - 1)}…` : one;
}

/** Folds one chat event into the conversation's item. Text and tool events
 * mean the turn is running; a permission ask or a plan/question waits on the
 * user; done and error end it. */
export const SIDE_TITLE = "Answering on the side";

export function chatActivity(
  prev: ActivityItem | undefined,
  { conversationId, event }: ChatEnvelope,
  now: number,
): ActivityItem | undefined {
  const base: ActivityItem = prev ?? {
    key: `chat:${conversationId}`,
    kind: "chat",
    // A side answer (`<chat>~side-<n>`, see Side.tsx) gets its own row.
    title: conversationId.includes("~side-") ? SIDE_TITLE : CHAT_TITLE,
    status: "running",
    detail: "Thinking…",
    steps: 0,
    at: now,
  };
  // A finished turn only comes back to life with new work.
  const fresh = base.status === "done" || base.status === "failed";
  const restart = fresh ? { ...base, steps: 0 } : base;
  switch (event.kind) {
    case "text":
      // More reply text leaves a running item as it is, `at` included (so a
      // streaming chat doesn't move up the dock): returning it unchanged lets
      // the dock skip a render, where a fresh object per streamed chunk
      // re-rendered the whole App on every token.
      if (prev?.status === "running") return prev;
      return { ...restart, status: "running", detail: base.status === "running" ? base.detail : "Writing a reply…", at: now };
    case "tool": {
      const ended = event.status === "completed" || event.status === "failed";
      return {
        ...restart,
        status: "running",
        detail: clip(event.title),
        steps: restart.steps + (ended ? 1 : 0),
        at: now,
      };
    }
    // Thinking, like more reply text, leaves a running item as it is.
    case "thought":
      if (prev?.status === "running") return prev;
      return { ...restart, status: "running", detail: "Thinking…", at: now };
    case "plan":
      return { ...restart, status: "running", detail: "Making a plan…", at: now };
    case "permission":
      return { ...restart, status: "waiting", detail: clip(`Needs your OK: ${event.title}`), at: now };
    // Answered from a notification: the turn carries on.
    case "answered":
      return prev && prev.status === "waiting"
        ? { ...prev, status: "running", detail: "Thinking…", at: now }
        : prev;
    case "elicit":
      return {
        ...restart,
        status: "waiting",
        detail: clip(event.server === "Plan" ? "Plan ready for your OK" : `Asks: ${event.message}`),
        at: now,
      };
    case "done":
      return prev
        ? { ...base, status: "done", detail: event.stop === "halted" ? "Halted" : "Reply finished", at: now }
        : undefined;
    case "error":
      return { ...restart, status: "failed", detail: clip(event.message), at: now };
    case "suggestions":
    case "goal":
      return prev;
  }
}

/** An automation run that started from Run now. */
export function routineStarted(id: string, name: string, now: number): ActivityItem {
  return {
    key: `run:${id}`,
    kind: "routine",
    title: name,
    status: "running",
    detail: "Running now…",
    steps: 0,
    at: now,
  };
}

/** The same run, ended. */
export function routineFinished(
  item: ActivityItem,
  ok: boolean,
  text: string,
  now: number,
): ActivityItem {
  return { ...item, status: ok ? "done" : "failed", detail: clip(text), at: now };
}

const RANK: Record<ActivityStatus, number> = { waiting: 0, running: 1, queued: 2, failed: 3, done: 4 };

/** Still worth showing: unfinished, or finished less than `DONE_TTL_MS` ago. */
function current(item: ActivityItem, now: number): boolean {
  return (item.status !== "done" && item.status !== "failed") || now - item.at < DONE_TTL_MS;
}

/** What the dock shows: waiting first, then running, then recent results,
 * newest first within each; finished items older than `DONE_TTL_MS` drop. */
export function dockItems(items: ActivityItem[], now: number): ActivityItem[] {
  return items.filter((i) => current(i, now)).sort((a, b) => RANK[a.status] - RANK[b.status] || b.at - a.at);
}

/** The same map without the finished items the dock no longer shows, so a
 * long session doesn't keep every past turn. The same object when nothing
 * drops, so React sees no change. */
export function pruneActivity(items: Record<string, ActivityItem>, now: number): Record<string, ActivityItem> {
  const kept = Object.entries(items).filter(([, item]) => current(item, now));
  return kept.length === Object.keys(items).length ? items : Object.fromEntries(kept);
}

/** The collapsed dock's line, e.g. "1 waiting · 1 of 3 running · 1 done".
 * Anything queued reads as "x of y running", where y counts the running work
 * plus what is lined up behind it. */
export function dockSummary(items: ActivityItem[]): string {
  const count = (s: ActivityStatus) => items.filter((i) => i.status === s).length;
  const running = count("running");
  const queued = count("queued");
  const work =
    queued === 0
      ? running > 0
        ? `${running} running`
        : ""
      : running === 0
        ? `${queued} queued`
        : `${running} of ${running + queued} running`;
  const parts = [
    count("waiting") > 0 ? `${count("waiting")} waiting` : "",
    work,
    count("failed") > 0 ? `${count("failed")} failed` : "",
    count("done") > 0 ? `${count("done")} done` : "",
  ];
  return parts.filter(Boolean).join(" · ");
}

/** Where a queued message stands: "Next up", "2nd in line", … */
export function queuePlace(index: number): string {
  if (index === 0) return "Next up";
  const n = index + 1;
  const teen = n % 100 >= 11 && n % 100 <= 13;
  const suffix = teen ? "th" : (["th", "st", "nd", "rd"][n % 10] ?? "th");
  return `${n}${suffix} in line`;
}

const QUEUED_TITLE_MAX = 48;

/** One row per message waiting behind `conversationId`'s reply, in order:
 * its first line, and where it stands. Keys are `queue:<chat>:<index>`. */
export function queuedItems(conversationId: string, texts: readonly string[]): ActivityItem[] {
  return texts.map((text, i) => {
    const first = (text.split("\n").find((line) => line.trim()) ?? "").replace(/\s+/g, " ").trim();
    const title = first.length > QUEUED_TITLE_MAX ? `${first.slice(0, QUEUED_TITLE_MAX - 1).trimEnd()}…` : first;
    return {
      key: `queue:${conversationId}:${i}`,
      kind: "queued",
      title,
      status: "queued",
      detail: queuePlace(i),
      steps: 0,
      // Order within the queue: the first in line sorts first.
      at: -i,
    };
  });
}

/** The chat and place of a queued row's key. */
export function queueSpot(key: string): { conversationId: string; index: number } | null {
  const match = /^queue:(.+):(\d+)$/.exec(key);
  return match ? { conversationId: match[1], index: Number(match[2]) } : null;
}

/** The progress line under a running chat. */
export function stepsLine(item: ActivityItem): string {
  if (item.kind !== "chat" || item.steps === 0) return "";
  return item.steps === 1 ? "1 step done" : `${item.steps} steps done`;
}

/** Which work card each chat works, so a chat's dock item can open its card. */
export function cardsByChat(board: BoardPage): Record<string, string> {
  const out: Record<string, string> = {};
  for (const card of board.columns.flatMap((c) => c.cards)) {
    if (card.chat) out[card.chat] = card.id;
  }
  return out;
}

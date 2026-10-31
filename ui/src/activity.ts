// The Activity Dock's model. It keeps no store of its own: it reads the chat
// event stream and the Automations runs the shell already reports, and keeps
// the latest state of each one.
import type { ChatEnvelope } from "./api";

export type ActivityStatus = "running" | "waiting" | "done" | "failed";

export interface ActivityItem {
  /** `chat:<conversation>` or `run:<automation id>`. */
  key: string;
  kind: "chat" | "routine";
  title: string;
  status: ActivityStatus;
  /** The latest step, the approval it waits on, or how it ended. */
  detail: string;
  /** Tool steps finished in this turn, for the progress line. */
  steps: number;
  /** When the item last changed (ms), newest first in the dock. */
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
export function chatActivity(
  prev: ActivityItem | undefined,
  { conversationId, event }: ChatEnvelope,
  now: number,
): ActivityItem | undefined {
  const base: ActivityItem = prev ?? {
    key: `chat:${conversationId}`,
    kind: "chat",
    title: CHAT_TITLE,
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
    case "plan":
      return { ...restart, status: "running", detail: "Making a plan…", at: now };
    case "permission":
      return { ...restart, status: "waiting", detail: clip(`Needs your OK: ${event.title}`), at: now };
    case "elicit":
      return {
        ...restart,
        status: "waiting",
        detail: clip(event.server === "Plan" ? "Plan ready for your OK" : `Asks: ${event.message}`),
        at: now,
      };
    case "done":
      return prev ? { ...base, status: "done", detail: "Reply finished", at: now } : undefined;
    case "error":
      return { ...restart, status: "failed", detail: clip(event.message), at: now };
    case "suggestions":
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

const RANK: Record<ActivityStatus, number> = { waiting: 0, running: 1, failed: 2, done: 3 };

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

/** The collapsed dock's line, e.g. "1 waiting · 2 running". */
export function dockSummary(items: ActivityItem[]): string {
  const count = (s: ActivityStatus) => items.filter((i) => i.status === s).length;
  const parts = [
    [count("waiting"), "waiting"],
    [count("running"), "running"],
    [count("failed"), "failed"],
    [count("done"), "done"],
  ] as const;
  return parts
    .filter(([n]) => n > 0)
    .map(([n, word]) => `${n} ${word}`)
    .join(" · ");
}

/** The progress line under a running chat. */
export function stepsLine(item: ActivityItem): string {
  if (item.kind !== "chat" || item.steps === 0) return "";
  return item.steps === 1 ? "1 step done" : `${item.steps} steps done`;
}

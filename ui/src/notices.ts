// The Notifications menu's model: the record of things that happened while
// you weren't looking. Work that is running or lined up lives in Activity
// (activity.ts); this keeps what finished, failed or needs a look later.
// Session only, like the window app's bell.

export type NoticeKind = "info" | "success" | "error";

/** The one thing a notification's button does. */
export type NoticeAction =
  /** Opens the chat with this text in the box, ready to send. */
  | { kind: "draft"; label: string; text: string }
  /** Opens a screen: a chat (that conversation when `id` is set), Automations,
   * or a Settings section. */
  | { kind: "chat"; label: string; id?: string }
  | { kind: "automations"; label: string }
  | { kind: "settings"; label: string; section: "about" | "localAi" }
  /** Puts the cards the Workboard tidy archived back in Todo. */
  | { kind: "undoSweep"; label: string };

export interface Notice {
  id: number;
  kind: NoticeKind;
  text: string;
  /** When it last happened (ms). */
  at: number;
  /** The same words in a row fold into one entry, shown "(x2)". */
  count: number;
  action?: NoticeAction;
  /** Where it came from, shown under the words: "Chat: Trip plans", "Workboard". */
  from?: string;
}

export interface Notices {
  items: Notice[];
  unread: number;
  /** The next id. */
  next: number;
}

/** The bell keeps this many; the oldest drop. */
export const NOTICE_CAP = 50;

export const NO_NOTICES: Notices = { items: [], unread: 0, next: 1 };

function sameAction(a?: NoticeAction, b?: NoticeAction): boolean {
  return JSON.stringify(a ?? null) === JSON.stringify(b ?? null);
}

/** Adds a notification, newest first. The same kind, words, place and button
 * as the newest entry fold into it (count + 1) instead of stacking. */
export function pushNotice(
  notices: Notices,
  kind: NoticeKind,
  text: string,
  now: number,
  action?: NoticeAction,
  from?: string,
): Notices {
  const top = notices.items[0];
  if (top && top.kind === kind && top.text === text && top.from === from && sameAction(top.action, action)) {
    const folded = { ...top, at: now, count: top.count + 1 };
    return { ...notices, items: [folded, ...notices.items.slice(1)], unread: Math.max(notices.unread, 1) };
  }
  const notice: Notice = { id: notices.next, kind, text, at: now, count: 1, action, from };
  return {
    items: [notice, ...notices.items].slice(0, NOTICE_CAP),
    unread: Math.min(notices.unread + 1, NOTICE_CAP),
    next: notices.next + 1,
  };
}

/** Opening the menu reads them all. */
export function markRead(notices: Notices): Notices {
  return notices.unread === 0 ? notices : { ...notices, unread: 0 };
}

export function clearNotices(notices: Notices): Notices {
  return { ...NO_NOTICES, next: notices.next };
}

/** A pressed one-time button (Undo) is spent: the entry stays as the record,
 * without it. Buttons that open something stay, since opening twice is fine. */
export function spendAction(notices: Notices, id: number): Notices {
  if (notices.items.find((n) => n.id === id)?.action?.kind !== "undoSweep") return notices;
  return {
    ...notices,
    items: notices.items.map((n) => (n.id === id ? { ...n, action: undefined } : n)),
  };
}

/** The number on the bell: hidden at 0, "99+" past 99. */
export function unreadBadge(unread: number): string {
  if (unread <= 0) return "";
  return unread > 99 ? "99+" : String(unread);
}

/** The entry's words, with how many times it happened. */
export function shownText(notice: Notice): string {
  return notice.count > 1 ? `${notice.text} (x${notice.count})` : notice.text;
}

/** "Chat: Trip plans", or "Chat" when the chat has no name yet. */
export function chatFrom(title?: string): string {
  const name = title?.trim();
  return name ? `Chat: ${name}` : "Chat";
}

/** "just now", "5 min ago", "2 h ago", then the clock time. */
export function noticeAge(at: number, now: number): string {
  const mins = Math.floor(Math.max(0, now - at) / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins} min ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} h ago`;
  return new Date(at).toLocaleString();
}

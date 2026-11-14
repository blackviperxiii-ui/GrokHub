// Find in this chat (Ctrl+F), ported from grokhub_core::chat_view
// (chat_find_rows, chat_find_step, chat_find_label), which stays the source
// of truth. It searches the rows on screen: lines above "Show earlier" are
// not loaded, so they are not searched.
import type { Item } from "./Chat";

/** What a row offers to find: its words, or "" for a row with none. */
export function itemText(item: Item): string {
  switch (item.kind) {
    case "user":
    case "assistant":
    case "tools":
    case "slash":
    case "thought":
      return item.text;
    case "tool":
      return `${item.title}\n${item.detail}`;
    case "permission":
      return `${item.title}\n${item.action}`;
    default:
      return "";
  }
}

/** Rows whose text holds `query`, ignoring case. A blank query finds nothing. */
export function findRows(bodies: readonly string[], query: string): number[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const rows: number[] = [];
  bodies.forEach((body, i) => {
    if (body.toLowerCase().includes(needle)) rows.push(i);
  });
  return rows;
}

/** Step through `len` hits. Wraps at both ends. */
export function findStep(pick: number, len: number, forward: boolean): number {
  if (len === 0) return 0;
  const at = Math.min(pick, len - 1);
  return forward ? (at + 1) % len : (at + len - 1) % len;
}

/** Count beside the find box: "2 of 5", "No matches", or nothing while blank. */
export function findLabel(pick: number, len: number, query: string): string {
  if (!query.trim()) return "";
  if (len === 0) return "No matches";
  return `${Math.min(pick, len - 1) + 1} of ${len}`;
}

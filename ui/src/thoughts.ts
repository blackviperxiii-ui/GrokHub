// How the model's thinking folds in a chat, as the cabin drew it
// (grokhub_core::chat_view). A thought starts open unless Always collapse is
// on or this chat already folded its thoughts. Collapse folds every thought
// in the chat and keeps new ones folded; Expand opens just the one.
import type { Item } from "./Chat";

/** A thought's fold, once the person chose one. No fold follows the chat's start. */
export type Fold = "expanded" | "minimized";

/** Over an open thought. */
export const THOUGHT_LABEL = "Thought process";
/** The one line a folded thought keeps. */
export const THOUGHT_ROW_LABEL = "Thought";
export const COLLAPSE = "Collapse";
export const EXPAND = "Expand";

/** Thoughts start folded when Always collapse is on or this chat folded them. */
export function startsCollapsed(always: boolean, chatFolded: boolean): boolean {
  return always || chatFolded;
}

/** A fold chosen by hand wins; otherwise the chat's start decides. */
export function foldOf(stored: Fold | undefined, startCollapsed: boolean): Fold {
  return stored ?? (startCollapsed ? "minimized" : "expanded");
}

/** Collapse: every thought in the chat folds. */
export function collapseAll(items: Item[]): Item[] {
  return items.map((item) => (item.kind === "thought" ? { ...item, fold: "minimized" } : item));
}

/** Nothing new in the log, only thoughts folded or opened (or the same rows
 * again): the view stays where the person is reading. */
export function onlyFoldsChanged(before: readonly Item[], after: readonly Item[]): boolean {
  return (
    before.length === after.length &&
    after.every((item, i) => {
      const was = before[i];
      return item === was || (item.kind === "thought" && was.kind === "thought" && item.text === was.text);
    })
  );
}

/** Expand: only the thought at `index` opens; the rest keep their fold. */
export function expandOne(items: Item[], index: number): Item[] {
  return items.map((item, i) => (i === index && item.kind === "thought" ? { ...item, fold: "expanded" } : item));
}

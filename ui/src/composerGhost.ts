/** Pure helpers for sentence autocomplete (ghost text) in the composer. */

export const GHOST_DEBOUNCE_MS = 400;
export const MIN_DRAFT_CHARS = 3;

export const GHOST_SYSTEM =
  "Complete the user's partial chat message. Reply with only the continuation (no quotes, no preamble). One short line only. Do not repeat text they already typed.";

/** Whether a draft is worth asking the model for a continuation. */
export function shouldRequest(
  draft: string,
  slashOpen: boolean,
  turnRunning: boolean,
): boolean {
  if (slashOpen || turnRunning) return false;
  const t = draft.trim();
  return !(t.length === 0 || t.startsWith("/") || t.length < MIN_DRAFT_CHARS);
}

/** Cursor at the end of the draft (char index). */
export function cursorAtEnd(draft: string, cursorIndex: number): boolean {
  return cursorIndex >= [...draft].length;
}

/** Strip echoed draft / quotes from a one-line model reply. */
export function normalizeSuggestion(raw: string, draft: string): string {
  const line = (raw.split("\n")[0] ?? "")
    .trim()
    .replace(/^["']+|["']+$/g, "")
    .trim();
  if (!line) return "";
  const draftTrim = draft.trimEnd();
  if (line.startsWith(draftTrim)) {
    return line.slice(draftTrim.length).trimStart();
  }
  return line;
}

/** Append a suggestion to the draft; returns the new draft. */
export function acceptSuggestion(draft: string, suggestion: string): string {
  if (!suggestion) return draft;
  return draft + suggestion;
}

/** Schedule key for debounce: same draft+cursor means one pending ask. */
export function draftKey(draft: string, cursorIndex: number): string {
  return `${cursorIndex}\0${draft}`;
}

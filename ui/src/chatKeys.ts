// The chat's own shortcuts (Ctrl+F find, Ctrl+Shift+A archive) listen on the
// window, so they check here that the key is theirs and that no dialog owns
// the keyboard.

/** `letter` as the layout types it. A key with no Latin letter (a Cyrillic
 * layout, say) falls back to its place on the keyboard, so the shortcut still
 * works there, and never fires for a different letter on Dvorak or AZERTY. */
export function isLetter(event: KeyboardEvent, letter: string): boolean {
  const key = event.key.toLowerCase();
  return /^[a-z]$/.test(key) ? key === letter : event.code === `Key${letter.toUpperCase()}`;
}

/** Ctrl (Cmd on macOS) plus `letter`, with Shift exactly when `shift`, no Alt. */
export function isChatShortcut(event: KeyboardEvent, letter: string, shift: boolean): boolean {
  return (event.ctrlKey || event.metaKey) && !event.altKey && event.shiftKey === shift && isLetter(event, letter);
}

/** A dialog (the palette, the shortcut sheet, a confirm) is over the app. */
export function dialogOpen(): boolean {
  return document.querySelector('[aria-modal="true"]') !== null;
}

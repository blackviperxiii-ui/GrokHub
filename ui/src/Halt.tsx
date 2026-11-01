// Halt: one press stops desktop control and every running prompt, as the
// cabin's Halt does. The button sits in the top bar; Ctrl+Alt+H, the tray's
// Halt and Super+Shift+Esc do the same.
import { PRODUCT_NAME } from "./brand";

export const HALT_KEYS = "Ctrl+Alt+H";
export const HALT_TITLE = `Stop desktop control and every reply now (${HALT_KEYS})`;
export const HALT_DONE = `Halted. Desktop steps stay off until you restart ${PRODUCT_NAME}.`;
export const HALT_FAILED = "Halt didn't go through:";

/** Ctrl+Alt+H, by the key's place so a layout or macOS Option character
 * doesn't hide it. */
export function isHaltKey(event: KeyboardEvent): boolean {
  return event.ctrlKey && event.altKey && !event.shiftKey && !event.metaKey && event.code === "KeyH";
}

/** Shown while desktop control is on or anything is running. */
export function haltShown(desktopOn: boolean, busy: boolean): boolean {
  return desktopOn || busy;
}

export function HaltButton({ onHalt }: { onHalt: () => void }) {
  return (
    <button type="button" className="halt" title={HALT_TITLE} onClick={onHalt}>
      Halt
    </button>
  );
}

/** The line under the top bar after a Halt, or when it failed. */
export function HaltNote({ text, onDismiss }: { text: string; onDismiss: () => void }) {
  return (
    <div className="halt-note" role="status">
      <span>{text}</span>
      <button type="button" className="link" onClick={onDismiss}>
        Dismiss
      </button>
    </div>
  );
}

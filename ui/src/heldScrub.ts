// When a value is held for a chat (a secret typed on a form card), the app
// redacts it from what the model says from then on and from the saved chat.
// This scrubs the text already on the page, which came before it was held.
import type { Item } from "./Chat";

/** What the app writes where a held value was (`redact_held_secrets`). */
export const REDACTED = "[redacted]";
/** Shorter values aren't held: replacing them would mangle words
 * (`held.rs` MIN_CHARS). */
const MIN_CHARS = 4;

/** `items` with `value` replaced in the model's words, its thoughts, and
 * the tool and permission cards. The person's own messages stay as typed.
 * Items without it are returned as they were. */
export function scrubHeld(items: Item[], value: string): Item[] {
  const held = value.trim();
  if ([...held].length < MIN_CHARS) return items;
  const clean = (s: string) => (s.includes(held) ? s.split(held).join(REDACTED) : s);
  let changed = false;
  const out = items.map((item): Item => {
    let next: Item = item;
    switch (item.kind) {
      case "assistant":
      case "thought":
      case "tools":
        next = { ...item, text: clean(item.text) };
        if (next.text === item.text) next = item;
        break;
      case "tool": {
        const [title, status, detail] = [clean(item.title), clean(item.status), clean(item.detail)];
        if (title !== item.title || status !== item.status || detail !== item.detail) {
          next = { ...item, title, status, detail };
        }
        break;
      }
      case "permission": {
        const [title, action, reason] = [clean(item.title), clean(item.action), clean(item.reason)];
        if (title !== item.title || action !== item.action || reason !== item.reason) {
          next = { ...item, title, action, reason };
        }
        break;
      }
      default:
        break;
    }
    if (next !== item) changed = true;
    return next;
  });
  return changed ? out : items;
}

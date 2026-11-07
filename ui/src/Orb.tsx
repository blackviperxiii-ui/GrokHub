// A small dotted orb that shows what's going on while there's nothing to read
// yet: waiting on a reply, a tool running, the mic listening or turning speech
// into words. thinking-orbs (MIT) draws it on a plain 2D canvas, still under
// reduced motion and paused when hidden or scrolled away, so it stays light.
import { useSyncExternalStore } from "react";
import { ThinkingOrb, type OrbState } from "thinking-orbs";

export type { OrbState };

/** Ink for the shell's theme: the pinned light theme takes dark dots, every
 * pinned dark theme light ones, and System follows the OS. */
export function orbTheme(theme: string | undefined): "auto" | "dark" | "light" {
  if (!theme) return "auto";
  return theme === "light" ? "light" : "dark";
}

/** A running tool that looks something up, by its title. */
const LOOKS_UP = /search|web|fetch|brows|look ?up|find/i;

/** The orb for what the chat is doing (`activityOf` in nyx.ts). */
export function orbFor(activity: string): OrbState {
  if (!activity.startsWith("tool:")) return "working";
  return LOOKS_UP.test(activity.slice(5)) ? "searching" : "solving";
}

/** What the orb says to a screen reader, in the same terms. */
export function orbLabel(activity: string): string {
  const state = orbFor(activity);
  if (state === "searching") return "Looking it up";
  if (state === "solving") return "Working on it";
  return "Thinking";
}

function onThemeChange(update: () => void): () => void {
  const watch = new MutationObserver(update);
  watch.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  return () => watch.disconnect();
}

function pinnedTheme(): string | undefined {
  return document.documentElement.dataset.theme;
}

/** `label` names it for a screen reader; leave it out where the control
 * around it already says what's happening. */
export function Orb({ state, label, size = 20 }: { state: OrbState; label?: string; size?: 20 | 32 | 64 }) {
  const theme = useSyncExternalStore(onThemeChange, pinnedTheme, () => undefined);
  return label ? (
    <ThinkingOrb className="orb" state={state} size={size} theme={orbTheme(theme)} aria-label={label} />
  ) : (
    <ThinkingOrb className="orb" state={state} size={size} theme={orbTheme(theme)} aria-hidden="true" />
  );
}

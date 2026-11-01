import type { Theme } from "./api";

/** Themes that pin colors on <html data-theme>. System leaves it to the OS. */
const PINNED: Theme[] = ["dark", "ops-dark", "oled-black", "soft-charcoal", "light"];

/** The last theme Settings applied, kept so the next start paints in it. */
const CACHE_KEY = "shell.theme";

/** Puts Settings → Appearance on the page. Dark maps to Ops Dark tokens. */
export function applyTheme(theme: Theme | undefined) {
  const root = document.documentElement;
  if (theme && PINNED.includes(theme)) root.dataset.theme = theme;
  else delete root.dataset.theme;
  try {
    if (theme && PINNED.includes(theme)) localStorage.setItem(CACHE_KEY, theme);
    else localStorage.removeItem(CACHE_KEY);
  } catch {
    // Storage can be off (private windows); the theme still applies.
  }
}

/** Before the first paint: the theme from last time, until Settings loads. */
export function applyCachedTheme() {
  let cached: string | null;
  try {
    cached = localStorage.getItem(CACHE_KEY);
  } catch {
    return;
  }
  if (cached && (PINNED as string[]).includes(cached)) {
    document.documentElement.dataset.theme = cached;
  }
}

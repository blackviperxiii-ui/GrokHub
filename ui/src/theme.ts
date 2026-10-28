import type { Theme } from "./api";

/** Themes that pin colors on <html data-theme>. System leaves it to the OS. */
const PINNED: Theme[] = ["dark", "ops-dark", "oled-black", "soft-charcoal", "light"];

/** Puts Settings → Appearance on the page. Dark maps to Ops Dark tokens. */
export function applyTheme(theme: Theme | undefined) {
  const root = document.documentElement;
  if (theme && PINNED.includes(theme)) root.dataset.theme = theme;
  else delete root.dataset.theme;
}

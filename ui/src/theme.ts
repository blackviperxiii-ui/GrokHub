import type { Theme } from "./api";

/** Puts Settings → Appearance on the page: dark or light pins the colors,
 * system leaves them to the OS (styles.css). */
export function applyTheme(theme: Theme | undefined) {
  const root = document.documentElement;
  if (theme === "dark" || theme === "light") root.dataset.theme = theme;
  else delete root.dataset.theme;
}

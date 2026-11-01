import { useEffect, useState } from "react";

/** Phone widths, where the views bar sits at the bottom (styles.css). */
export const PHONE_QUERY = "(max-width: 600px)";
/** How far (px) content must keep scrolling one way before the bar moves. */
export const THRESHOLD = 24;
/** Within this many px of the top, the bar always shows. */
export const NEAR_TOP = 8;
/** Scrolls this long (ms) after a wheel, touch or key count as the user's,
 * so a streaming reply that scrolls the chat by itself leaves the bar alone. */
export const USER_WINDOW_MS = 1200;

/** `run` is how far the content has scrolled in the current direction. */
export type BarState = { hidden: boolean; run: number };
export const SHOWN: BarState = { hidden: false, run: 0 };

/** The bar after one scroll: `top` is the new scroll position, `delta` the move. */
export function onScroll(state: BarState, top: number, delta: number): BarState {
  if (top <= NEAR_TOP) return SHOWN;
  if (delta === 0) return state;
  const run = Math.sign(delta) === Math.sign(state.run) ? state.run + delta : delta;
  if (run >= THRESHOLD) return { hidden: true, run };
  if (run <= -THRESHOLD) return { hidden: false, run };
  return { hidden: state.hidden, run };
}

const SHOWS_BAR = "input, textarea, select, [contenteditable], .topbar, .topbar *";

function usePhone(): boolean {
  const [phone, setPhone] = useState(() => window.matchMedia?.(PHONE_QUERY).matches ?? false);
  useEffect(() => {
    const query = window.matchMedia?.(PHONE_QUERY);
    if (!query) return;
    const change = () => setPhone(query.matches);
    query.addEventListener("change", change);
    return () => query.removeEventListener("change", change);
  }, []);
  return phone;
}

/** Whether the views bar should slide away: only on phones, while the user
 * scrolls down, and never while `pinned` (a dialog is open). Focusing a field
 * or the bar itself brings it back, so it stays reachable from the keyboard. */
export function useHidingBar(pinned: boolean): boolean {
  const phone = usePhone();
  const [state, setState] = useState(SHOWN);
  const [wasPhone, setWasPhone] = useState(phone);
  if (phone !== wasPhone) {
    setWasPhone(phone);
    setState(SHOWN);
  }

  useEffect(() => {
    if (!phone) return;
    const last = new WeakMap<Element, number>();
    let userAt = -Infinity;
    const user = () => {
      userAt = performance.now();
    };
    const scroll = (event: Event) => {
      const el = event.target === document ? document.scrollingElement : event.target;
      if (!(el instanceof Element) || el.closest(".topbar")) return;
      const top = el.scrollTop;
      const before = last.get(el) ?? top;
      last.set(el, top);
      if (top > NEAR_TOP && performance.now() - userAt > USER_WINDOW_MS) return;
      setState((now) => onScroll(now, top, top - before));
    };
    const focus = (event: FocusEvent) => {
      if (event.target instanceof Element && event.target.matches(SHOWS_BAR)) setState(SHOWN);
    };
    const inputs = ["wheel", "touchstart", "touchmove", "keydown"] as const;
    for (const name of inputs) document.addEventListener(name, user, { capture: true, passive: true });
    document.addEventListener("scroll", scroll, { capture: true, passive: true });
    document.addEventListener("focusin", focus);
    return () => {
      for (const name of inputs) document.removeEventListener(name, user, { capture: true });
      document.removeEventListener("scroll", scroll, { capture: true });
      document.removeEventListener("focusin", focus);
    };
  }, [phone]);

  return phone && !pinned && state.hidden;
}

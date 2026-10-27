import { useLayoutEffect, type KeyboardEvent, type RefObject } from "react";
import { reducedMotion } from "./rubberBand";

/** How much of the next item a cut-off list shows, at least and at most: a
 * hint that there is more. */
export const PEEK_MIN = 0.3;
export const PEEK_MAX = 0.5;

/** "y" for a column of rows, "x" for a row of chips. */
export type Axis = "x" | "y";

/** Where one item sits along the list, in px from the list's start. */
export interface Span {
  start: number;
  end: number;
}

/** The size to give a list of `spans` that may take up to `max` px, so the
 * last item showing is cut between `min` and `most` of the way in instead of
 * at a clean edge. Where `max` already cuts an item in that band the list
 * keeps all its room. `null` when every item fits. */
export function peekExtent(
  spans: Span[],
  max: number,
  min: number = PEEK_MIN,
  most: number = PEEK_MAX,
): number | null {
  if (spans.length === 0 || spans[spans.length - 1].end <= max) return null;
  let cut: number | null = null;
  for (const span of spans.slice(1)) {
    const size = span.end - span.start;
    if (span.start + min * size > max) break;
    cut = Math.min(max, span.start + most * size);
  }
  return cut ?? max;
}

/** The scroll offset that shows item `index` plus one item of lookahead on
 * each side, moving as little as it can from `scroll`. `dir` is the way the
 * selection moved: when the three items don't fit, the one ahead wins. */
export function revealScroll(scroll: number, viewport: number, spans: Span[], index: number, dir: 1 | -1): number {
  if (spans.length === 0) return scroll;
  const last = spans.length - 1;
  const at = Math.min(Math.max(index, 0), last);
  let lo = spans[Math.max(at - 1, 0)].start;
  let hi = spans[Math.min(at + 1, last)].end;
  if (hi - lo > viewport) {
    if (dir > 0) lo = hi - viewport;
    else hi = lo + viewport;
  }
  let next = scroll;
  if (hi > next + viewport) next = hi - viewport;
  if (lo < next) next = lo;
  return Math.max(0, next);
}

/** The spans of `list`'s children. The list is their offset parent
 * (`position: relative`), so offsets are from its start. */
export function spansOf(list: HTMLElement, axis: Axis): Span[] {
  return Array.from(list.children, (child) => {
    const el = child as HTMLElement;
    const start = axis === "y" ? el.offsetTop : el.offsetLeft;
    return { start, end: start + (axis === "y" ? el.offsetHeight : el.offsetWidth) };
  });
}

/** Scroll `list` so child `index` and its neighbours show. Smooth unless the
 * system asks for reduced motion. */
export function revealItem(list: HTMLElement, index: number, dir: 1 | -1, axis: Axis = "y"): void {
  const scroll = axis === "y" ? list.scrollTop : list.scrollLeft;
  const viewport = axis === "y" ? list.clientHeight : list.clientWidth;
  const next = revealScroll(scroll, viewport, spansOf(list, axis), index, dir);
  if (next === scroll) return;
  const behavior: ScrollBehavior = reducedMotion() ? "auto" : "smooth";
  if (typeof list.scrollTo === "function") {
    list.scrollTo(axis === "y" ? { top: next, behavior } : { left: next, behavior });
  } else if (axis === "y") {
    list.scrollTop = next;
  } else {
    list.scrollLeft = next;
  }
}

/** The room `list` has along `axis`: its parent's content box. */
function room(list: HTMLElement, axis: Axis): number {
  const parent = list.parentElement;
  if (!parent) return 0;
  const style = getComputedStyle(parent);
  return axis === "y"
    ? parent.clientHeight - parseFloat(style.paddingTop || "0") - parseFloat(style.paddingBottom || "0")
    : parent.clientWidth - parseFloat(style.paddingLeft || "0") - parseFloat(style.paddingRight || "0");
}

/** Size `ref`'s list so a cut-off list shows part of its next item. `max`
 * is the most room it may take (its parent's content box by default); a list
 * that fits keeps its stylesheet size. Re-measures on `key` and on resize. */
export function usePeek(
  ref: RefObject<HTMLElement | null>,
  axis: Axis,
  key: unknown,
  max?: () => number,
): void {
  useLayoutEffect(() => {
    const list = ref.current;
    if (!list) return;
    const prop = axis === "y" ? "maxHeight" : "maxWidth";
    const fit = () => {
      list.style[prop] = "";
      const limit = max ? max() : room(list, axis);
      const size = limit > 0 ? peekExtent(spansOf(list, axis), limit) : null;
      if (size !== null) list.style[prop] = `${size}px`;
    };
    fit();
    window.addEventListener("resize", fit);
    return () => window.removeEventListener("resize", fit);
  }, [ref, axis, key, max]);
}

/** The way a flex list runs: a row reads as `x`, anything else as `y`. */
export function axisOf(list: HTMLElement): Axis {
  return getComputedStyle(list).flexDirection.startsWith("row") ? "x" : "y";
}

/** Arrow keys move focus along a row (`x`) or column (`y`) of buttons and
 * reveal the item after the new one early. Stops at the ends. The axis
 * defaults to the way the list runs right now. */
export function onArrowFocus(event: KeyboardEvent<HTMLElement>, axis: Axis = axisOf(event.currentTarget)): void {
  const back = axis === "y" ? "ArrowUp" : "ArrowLeft";
  const ahead = axis === "y" ? "ArrowDown" : "ArrowRight";
  if (event.key !== back && event.key !== ahead) return;
  const list = event.currentTarget;
  const items = Array.from(list.children) as HTMLElement[];
  const from = items.findIndex((item) => item === document.activeElement);
  if (from < 0) return;
  const dir = event.key === ahead ? 1 : -1;
  const to = Math.min(Math.max(from + dir, 0), items.length - 1);
  event.preventDefault();
  if (to === from) return;
  items[to].focus({ preventScroll: true });
  revealItem(list, to, dir, axis);
}

import { useEffect } from "react";

/** How far (px) a pulled view can travel past its edge. */
export const MAX_PULL = 96;
/** Stiffness: higher gives less resistance near the edge. */
export const STIFFNESS = 0.55;
/** The scroll views that stretch when pulled past their top or bottom. */
export const STRETCHY = ".log, .settings-page, .automations, .memory, .connectors, .pulse";

/** iOS-style resistance: the offset for a raw pull of `x` px. It grows more
 * slowly the further you pull and never reaches `d`. Negative pulls mirror. */
export function rubberBand(x: number, d: number = MAX_PULL, c: number = STIFFNESS): number {
  if (d <= 0) return 0;
  const offset = (1 - 1 / ((Math.abs(x) * c) / d + 1)) * d;
  return Math.sign(x) * offset;
}

/** The stretch for a drag of `dy` px (positive is down) on a view scrolled to
 * `top` of `max`: pulling down at the top or up at the bottom stretches it;
 * anything else is a normal scroll. */
export function overscroll(dy: number, top: number, max: number): number {
  if (dy > 0 && top <= 0) return rubberBand(dy);
  if (dy < 0 && top >= max) return rubberBand(dy);
  return 0;
}

function reducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}

/** Stretch the main scroll views when a touch pulls them past an edge, then
 * snap back on release. With reduced motion nothing stretches. */
export function useOverscroll(): void {
  useEffect(() => {
    let el: HTMLElement | null = null;
    let startY = 0;
    const set = (node: HTMLElement, y: number) => {
      node.style.transform = y ? `translateY(${y}px)` : "";
    };
    const start = (event: TouchEvent) => {
      if (event.touches.length !== 1 || reducedMotion()) return;
      const target = event.target instanceof Element ? event.target.closest(STRETCHY) : null;
      el = target instanceof HTMLElement ? target : null;
      startY = event.touches[0].clientY;
      if (el) el.classList.remove("snapping");
    };
    const move = (event: TouchEvent) => {
      if (!el || event.touches.length !== 1) return;
      const y = event.touches[0].clientY;
      const stretch = overscroll(y - startY, el.scrollTop, el.scrollHeight - el.clientHeight);
      // Count the pull from where the view reached its edge, not from the touch.
      if (stretch === 0) startY = y;
      set(el, stretch);
    };
    const end = () => {
      if (!el) return;
      if (el.style.transform) el.classList.add("snapping");
      set(el, 0);
      el = null;
    };
    document.addEventListener("touchstart", start, { passive: true });
    document.addEventListener("touchmove", move, { passive: true });
    document.addEventListener("touchend", end);
    document.addEventListener("touchcancel", end);
    return () => {
      document.removeEventListener("touchstart", start);
      document.removeEventListener("touchmove", move);
      document.removeEventListener("touchend", end);
      document.removeEventListener("touchcancel", end);
    };
  }, []);
}

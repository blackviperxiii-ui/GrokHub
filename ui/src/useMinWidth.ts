// Whether the window is at least `px` wide, as a store: React reads the width
// again once it is listening, so a resize between the first render and the
// listener isn't lost. Without matchMedia (some test setups), innerWidth and
// resize stand in.
import { useCallback, useSyncExternalStore } from "react";

export function useMinWidth(px: number): boolean {
  const query = `(min-width: ${px}px)`;
  const subscribe = useCallback(
    (update: () => void) => {
      if (typeof window.matchMedia === "function") {
        const list = window.matchMedia(query);
        list.addEventListener("change", update);
        return () => list.removeEventListener("change", update);
      }
      window.addEventListener("resize", update);
      return () => window.removeEventListener("resize", update);
    },
    [query],
  );
  const read = useCallback(() => {
    if (typeof window === "undefined") return false;
    if (typeof window.matchMedia === "function") return window.matchMedia(query).matches;
    return window.innerWidth >= px;
  }, [query, px]);
  return useSyncExternalStore(subscribe, read, () => false);
}

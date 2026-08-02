import { useEffect, useState, useSyncExternalStore } from "react";
import { formatRelative } from "@/lib/utils";

/** Shared 15s clock — one interval for all RelativeTime instances */
let tick = 0;
const listeners = new Set<() => void>();
let intervalId: ReturnType<typeof setInterval> | null = null;

function ensureClock() {
  if (intervalId != null) return;
  intervalId = setInterval(() => {
    tick += 1;
    for (const l of listeners) l();
  }, 15_000);
}

function subscribe(cb: () => void) {
  listeners.add(cb);
  ensureClock();
  return () => {
    listeners.delete(cb);
    if (listeners.size === 0 && intervalId != null) {
      clearInterval(intervalId);
      intervalId = null;
    }
  };
}

function getTick() {
  return tick;
}

/** Avoids SSR/client time drift hydration mismatches. */
export function RelativeTime({ ts, className }: { ts: number; className?: string }) {
  const [label, setLabel] = useState("—");
  const clock = useSyncExternalStore(subscribe, getTick, () => 0);

  useEffect(() => {
    setLabel(formatRelative(ts));
  }, [ts, clock]);

  return <span className={className}>{label}</span>;
}

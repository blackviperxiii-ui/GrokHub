/**
 * Visibility-aware polling with exponential backoff when offline / failing.
 */

export type SmartPollOptions = {
  /** Base interval when healthy + visible (ms) */
  intervalMs: number;
  /** Max backoff after consecutive failures (ms) */
  maxBackoffMs?: number;
  /** Called each tick; return false to count as failure */
  tick: () => void | boolean | Promise<void | boolean>;
  /** If true, only tick when document is visible */
  onlyWhenVisible?: boolean;
};

export function startSmartPoll(opts: SmartPollOptions): () => void {
  const maxBackoff = opts.maxBackoffMs ?? opts.intervalMs * 8;
  const onlyWhenVisible = opts.onlyWhenVisible !== false;
  let fails = 0;
  let timer: number | null = null;
  let stopped = false;

  const delay = () => {
    if (fails <= 0) return opts.intervalMs;
    const d = opts.intervalMs * Math.pow(2, Math.min(fails, 4));
    return Math.min(d, maxBackoff);
  };

  const clear = () => {
    if (timer != null) {
      window.clearTimeout(timer);
      timer = null;
    }
  };

  const schedule = (ms: number) => {
    clear();
    if (stopped) return;
    timer = window.setTimeout(() => void run(), ms);
  };

  const run = async () => {
    if (stopped) return;
    if (onlyWhenVisible && typeof document !== "undefined" && document.visibilityState !== "visible") {
      schedule(opts.intervalMs);
      return;
    }
    try {
      const r = await opts.tick();
      if (r === false) fails += 1;
      else fails = 0;
    } catch {
      fails += 1;
    }
    schedule(delay());
  };

  const onVis = () => {
    if (document.visibilityState === "visible") {
      fails = 0;
      void run();
    }
  };

  if (typeof document !== "undefined") {
    document.addEventListener("visibilitychange", onVis);
  }
  // First tick shortly after start (stagger)
  schedule(Math.min(2_000, opts.intervalMs));

  return () => {
    stopped = true;
    clear();
    if (typeof document !== "undefined") {
      document.removeEventListener("visibilitychange", onVis);
    }
  };
}

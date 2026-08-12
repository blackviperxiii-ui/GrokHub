/**
 * Renderer-side ring buffers for diagnostics / field debug.
 * No React deps — safe for store + UI.
 */

const ERROR_CAP = 40;
const STREAM_CAP = 20;

export type RuntimeErrorEntry = {
  ts: string;
  source: string;
  message: string;
};

export type StreamTurnMetrics = {
  ts: string;
  threadId?: string;
  model?: string;
  ttfbMs?: number;
  totalMs?: number;
  rounds?: number;
  chars?: number;
  deltaPaints?: number;
  ok?: boolean;
  error?: string;
};

const errors: RuntimeErrorEntry[] = [];
const streams: StreamTurnMetrics[] = [];
let lastStream: StreamTurnMetrics | null = null;

export function pushRuntimeError(source: string, message: string): void {
  const msg = String(message || "").trim().slice(0, 500);
  if (!msg) return;
  errors.push({
    ts: new Date().toISOString(),
    source: String(source || "app").slice(0, 64),
    message: msg,
  });
  while (errors.length > ERROR_CAP) errors.shift();
}

export function getRuntimeErrors(limit = 20): RuntimeErrorEntry[] {
  const n = Math.max(1, Math.min(ERROR_CAP, limit));
  return errors.slice(-n);
}

export function recordStreamTurn(m: StreamTurnMetrics): void {
  lastStream = m;
  streams.push(m);
  while (streams.length > STREAM_CAP) streams.shift();
}

export function getLastStreamMetrics(): StreamTurnMetrics | null {
  return lastStream;
}

export function getStreamMetricsHistory(limit = 10): StreamTurnMetrics[] {
  const n = Math.max(1, Math.min(STREAM_CAP, limit));
  return streams.slice(-n);
}

export function clearRuntimeMetrics(): void {
  errors.length = 0;
  streams.length = 0;
  lastStream = null;
}

/** True when GROKHUB_DEBUG or localStorage grokhub.debug is on. */
export function isRendererDebug(): boolean {
  try {
    if (typeof localStorage !== "undefined" && localStorage.getItem("grokhub.debug") === "1") {
      return true;
    }
  } catch {
    /* ignore */
  }
  return false;
}

export function perfMark(name: string): void {
  try {
    if (typeof performance !== "undefined" && performance.mark) {
      performance.mark(`gh:${name}`);
    }
  } catch {
    /* ignore */
  }
}

export function perfMeasure(name: string, start: string, end: string): number | undefined {
  try {
    if (typeof performance === "undefined" || !performance.measure) return undefined;
    performance.measure(`gh:${name}`, `gh:${start}`, `gh:${end}`);
    const entries = performance.getEntriesByName(`gh:${name}`);
    const last = entries[entries.length - 1] as PerformanceEntry | undefined;
    return last?.duration;
  } catch {
    return undefined;
  }
}

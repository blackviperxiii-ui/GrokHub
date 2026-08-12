/**
 * One-click diagnostics bundle for crash / support reports.
 */
import { APP_VERSION, APP_NAME } from "./version";
import {
  getLastStreamMetrics,
  getRuntimeErrors,
  getStreamMetricsHistory,
} from "./runtime-metrics";

export type DiagnosticsBundle = {
  app: string;
  version: string;
  ts: string;
  userAgent?: string;
  platform?: string;
  electron?: string | null;
  host?: unknown;
  memoryRoot?: string;
  learning?: string;
  workboardOpen?: number;
  lastErrors?: string[];
  notes?: string;
  context?: {
    percent?: number;
    tokensEst?: number;
    budget?: number;
    shouldCompact?: boolean;
  };
  stream?: unknown;
  streamHistory?: unknown;
  runtimeErrors?: unknown;
  mainMetrics?: unknown;
  logPaths?: unknown;
  logTail?: string;
  stateBytes?: number;
};

export async function buildDiagnostics(extra?: {
  learningLine?: string;
  workboardOpen?: number;
  lastErrors?: string[];
  context?: DiagnosticsBundle["context"];
}): Promise<DiagnosticsBundle> {
  const runtimeErrors = getRuntimeErrors(20);
  const bundle: DiagnosticsBundle = {
    app: APP_NAME,
    version: APP_VERSION,
    ts: new Date().toISOString(),
    userAgent: typeof navigator !== "undefined" ? navigator.userAgent : undefined,
    platform: typeof navigator !== "undefined" ? navigator.platform : undefined,
    electron: null,
    learning: extra?.learningLine,
    workboardOpen: extra?.workboardOpen,
    lastErrors:
      extra?.lastErrors?.slice(0, 20) ||
      runtimeErrors.map((e) => `${e.ts} [${e.source}] ${e.message}`),
    context: extra?.context,
    stream: getLastStreamMetrics(),
    streamHistory: getStreamMetricsHistory(8),
    runtimeErrors,
  };

  if (typeof window !== "undefined" && window.grokhubDesktop) {
    try {
      const host = await window.grokhubDesktop.host?.info?.();
      bundle.host = host
        ? {
            platform: (host as { platform?: string }).platform,
            hostname: (host as { hostname?: string }).hostname,
            home: (host as { home?: string }).home,
          }
        : null;
    } catch {
      /* ignore */
    }
    try {
      const mem = await window.grokhubDesktop.memory?.info?.();
      bundle.memoryRoot = mem?.root;
    } catch {
      /* ignore */
    }
    try {
      // @ts-expect-error optional
      bundle.electron = window.grokhubDesktop.version?.electron || null;
    } catch {
      /* ignore */
    }
    try {
      const st = await window.grokhubDesktop.state?.info?.();
      if (st && typeof (st as { bytes?: number }).bytes === "number") {
        bundle.stateBytes = (st as { bytes: number }).bytes;
      }
    } catch {
      /* ignore */
    }
    try {
      const logs = window.grokhubDesktop as {
        logs?: { tail?: (n?: number) => Promise<unknown>; paths?: () => Promise<unknown> };
        debug?: { metrics?: () => Promise<unknown> };
      };
      if (logs.logs?.paths) {
        bundle.logPaths = await logs.logs.paths();
      }
      if (logs.logs?.tail) {
        const t = (await logs.logs.tail(40)) as { text?: string };
        bundle.logTail = t?.text?.slice(-4000);
      }
      if (logs.debug?.metrics) {
        bundle.mainMetrics = await logs.debug.metrics();
      }
    } catch {
      /* ignore */
    }
  }
  return bundle;
}

export async function copyDiagnostics(extra?: Parameters<typeof buildDiagnostics>[0]): Promise<{
  ok: boolean;
  text?: string;
  error?: string;
}> {
  try {
    const b = await buildDiagnostics(extra);
    const text = JSON.stringify(b, null, 2);
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
    }
    return { ok: true, text };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "copy failed" };
  }
}

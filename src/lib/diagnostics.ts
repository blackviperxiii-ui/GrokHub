/**
 * One-click diagnostics bundle for crash / support reports.
 */
import { APP_VERSION, APP_NAME } from "./version";

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
};

export async function buildDiagnostics(extra?: {
  learningLine?: string;
  workboardOpen?: number;
  lastErrors?: string[];
}): Promise<DiagnosticsBundle> {
  const bundle: DiagnosticsBundle = {
    app: APP_NAME,
    version: APP_VERSION,
    ts: new Date().toISOString(),
    userAgent: typeof navigator !== "undefined" ? navigator.userAgent : undefined,
    platform: typeof navigator !== "undefined" ? navigator.platform : undefined,
    electron: null,
    learning: extra?.learningLine,
    workboardOpen: extra?.workboardOpen,
    lastErrors: extra?.lastErrors?.slice(0, 20),
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

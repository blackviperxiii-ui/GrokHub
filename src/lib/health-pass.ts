/**
 * Lightweight install / session health pass for proactive autonomy.
 * Safe checks only — no destructive actions.
 */
import type { AutonomyConfig } from "./agent-jobs";
import { canFreeRoam, proactiveEnabled } from "./proactive";

export type HealthFinding = {
  id: string;
  severity: "ok" | "info" | "warn";
  title: string;
  detail: string;
  /** Suggested auto-fix key (store applies) */
  fix?:
    | "refresh_oauth"
    | "probe_host"
    | "ensure_memory"
    | "refresh_models"
    | "clear_stream";
};

export type HealthPassInput = {
  autonomy: AutonomyConfig;
  hasOauth: boolean;
  oauthExpiresAt?: number | null;
  hasApiKey: boolean;
  hostConnected: boolean;
  modelsAgeMs?: number | null;
  streamingStuck?: boolean;
  running?: boolean;
  now?: number;
};

export type HealthPassResult = {
  ok: boolean;
  findings: HealthFinding[];
  summary: string;
  autoFixes: HealthFinding[];
};

export async function runHealthPass(input: HealthPassInput): Promise<HealthPassResult> {
  const now = input.now ?? Date.now();
  const findings: HealthFinding[] = [];

  if (!input.hasOauth && !input.hasApiKey) {
    findings.push({
      id: "auth",
      severity: "warn",
      title: "Not signed in",
      detail: "Connect Grok OAuth or an API key in Settings.",
    });
  } else if (input.hasOauth && input.oauthExpiresAt) {
    const left = input.oauthExpiresAt - now;
    if (left < 0) {
      findings.push({
        id: "oauth-expired",
        severity: "warn",
        title: "Session expired",
        detail: "OAuth token expired — re-auth in Settings.",
        fix: "refresh_oauth",
      });
    } else if (left < 45 * 60 * 1000) {
      findings.push({
        id: "oauth-expiring",
        severity: "info",
        title: "Session refresh soon",
        detail: `Token expires in ~${Math.max(1, Math.round(left / 60000))}m — refreshing automatically when proactive.`,
        fix: "refresh_oauth",
      });
    } else {
      findings.push({
        id: "auth-ok",
        severity: "ok",
        title: "Signed in",
        detail: "Grok credentials present.",
      });
    }
  } else {
    findings.push({
      id: "auth-ok",
      severity: "ok",
      title: "Signed in",
      detail: input.hasOauth ? "OAuth active." : "API key set.",
    });
  }

  if (!input.hostConnected) {
    findings.push({
      id: "host",
      severity: "warn",
      title: "Desktop host offline",
      detail: "Shell/file tools need the desktop app bridge.",
      fix: "probe_host",
    });
  } else {
    findings.push({
      id: "host-ok",
      severity: "ok",
      title: "Desktop host live",
      detail: "Unsandboxed tools available.",
    });
  }

  if (input.streamingStuck && !input.running) {
    findings.push({
      id: "stream",
      severity: "warn",
      title: "Stuck stream chrome",
      detail: "A reply still looks like it's streaming — clearing it.",
      fix: "clear_stream",
    });
  }

  if (input.modelsAgeMs != null && input.modelsAgeMs > 12 * 60 * 60 * 1000) {
    findings.push({
      id: "models",
      severity: "info",
      title: "Model catalog stale",
      detail: "Last model list is old — will refresh when online.",
      fix: "refresh_models",
    });
  }

  findings.push({
    id: "memory",
    severity: "info",
    title: "Memory path",
    detail: "~/.config/GrokHub/memory (ensured on boot / self-check).",
    fix: "ensure_memory",
  });

  const autoFixes = findings.filter((f) => {
    if (!f.fix || f.severity === "ok") return false;
    if (!proactiveEnabled(input.autonomy)) return false;
    // Always allow clear_stream / ensure_memory when aware+
    if (f.fix === "clear_stream" || f.fix === "ensure_memory") return true;
    // OAuth/host/models only free-roam L3+
    return canFreeRoam(input.autonomy);
  });

  const warns = findings.filter((f) => f.severity === "warn");
  const summary = warns.length
    ? warns.map((w) => w.title).join(" · ")
    : findings.filter((f) => f.severity === "ok").length
      ? "Health OK"
      : "Health checked";

  return {
    ok: warns.length === 0,
    findings,
    summary,
    autoFixes,
  };
}

/** Format health report for /health slash. */
export function formatHealthMarkdown(r: HealthPassResult): string {
  const lines = [
    "**Health pass**",
    "",
    r.ok ? "All clear." : "Some issues found:",
    "",
    ...r.findings.map((f) => {
      const mark = f.severity === "ok" ? "✓" : f.severity === "warn" ? "!" : "·";
      return `- ${mark} **${f.title}** — ${f.detail}`;
    }),
  ];
  if (r.autoFixes.length) {
    lines.push("", `_Auto-fixes applied when proactive: ${r.autoFixes.map((a) => a.title).join(", ")}_`);
  }
  return lines.join("\n");
}

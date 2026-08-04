/**
 * Turn raw API / host errors into short, user-facing copy.
 * Layer tags (Prompt / Context / Loop / auth / host) help debug without raw dumps.
 */
import { tagErrorLayer } from "./pcl-layers";

export function formatUserError(err: unknown, fallback = "Something went wrong"): string {
  const raw = err instanceof Error ? err.message : String(err || fallback);
  const m = raw.trim();
  if (!m) return fallback;
  const layer = tagErrorLayer(err);
  const tag =
    layer === "unknown"
      ? ""
      : layer === "auth"
        ? "Auth · "
        : layer === "host"
          ? "Host · "
          : layer === "context"
            ? "Context · "
            : layer === "network"
              ? "Network · "
              : layer === "loop"
                ? "Agent loop · "
                : layer === "prompt"
                  ? "Prompt · "
                  : "";

  if (/quota|rate limit|429/i.test(m)) {
    return `${tag}Rate limit or quota hit. Wait a moment, switch to Fast mode, or check your plan.`;
  }
  if (/401|unauthorized|invalid.?token|expired/i.test(m)) {
    return `${tag}Session expired or invalid. Reconnect Grok OAuth or paste a fresh API key in Settings.`;
  }
  if (/403|forbidden|subscription|super.?grok|not entitled/i.test(m)) {
    return `${tag}This model needs a higher plan. Try Fast mode, or upgrade on grok.com.`;
  }
  if (/network|fetch failed|econnrefused|enotfound|offline|failed to fetch/i.test(m)) {
    return `${tag}Network error — check connectivity, then try again.`;
  }
  if (/timeout|timed out|ETIMEDOUT/i.test(m)) {
    return `${tag}Request timed out. Try a shorter prompt or Fast mode.`;
  }
  if (/HOST|host gateway|bridge offline|desktop/i.test(m) && /offline|missing|denied/i.test(m)) {
    return `${tag}Desktop host is offline. Open Settings → Desktop host or restart GrokHub.`;
  }
  if (/safe mode|blocked by host/i.test(m)) {
    return `${tag}Blocked by Host safe mode. Disable it in Settings if you need that command.`;
  }
  if (/context|token.?limit|too large|maximum context/i.test(m)) {
    return `${tag}Context too large. Start a new chat or use /compact, then retry.`;
  }
  // Cap raw technical dumps
  const body = m.length > 280 ? m.slice(0, 280) + "…" : m;
  return tag ? `${tag}${body}` : body;
}

export function friendlyAssistantError(err: unknown): string {
  return `**Couldn't complete that**\n\n${formatUserError(err)}\n\n_Tip: Settings → Connect Grok · check mode · try again._`;
}

/**
 * Turn raw API / host errors into short, user-facing copy.
 */
export function formatUserError(err: unknown, fallback = "Something went wrong"): string {
  const raw = err instanceof Error ? err.message : String(err || fallback);
  const m = raw.trim();
  if (!m) return fallback;

  if (/quota|rate limit|429/i.test(m)) {
    return "Rate limit or quota hit. Wait a moment, switch to Fast mode, or check your plan in Settings.";
  }
  if (/401|unauthorized|invalid.?token|expired/i.test(m)) {
    return "Session expired or invalid. Reconnect Grok OAuth or paste a fresh API key in Settings.";
  }
  if (/403|forbidden|subscription|super.?grok|not entitled/i.test(m)) {
    return "This model needs a higher plan. Try Free / Fast mode, or upgrade on grok.com.";
  }
  if (/network|fetch failed|econnrefused|enotfound|offline|failed to fetch/i.test(m)) {
    return "Network error — check connectivity, then try again.";
  }
  if (/timeout|timed out|ETIMEDOUT/i.test(m)) {
    return "Request timed out. Try a shorter prompt or Fast mode.";
  }
  if (/HOST|host gateway|bridge offline|desktop/i.test(m) && /offline|missing|denied/i.test(m)) {
    return "Desktop host is offline. Open Desktop host or restart GrokHub.";
  }
  if (/safe mode|blocked by host/i.test(m)) {
    return "Blocked by Host safe mode. Disable it in Settings if you need that command.";
  }
  // Cap raw technical dumps
  if (m.length > 280) return m.slice(0, 280) + "…";
  return m;
}

export function friendlyAssistantError(err: unknown): string {
  return `**Couldn't complete that**\n\n${formatUserError(err)}\n\n_Tip: Settings → Connect Grok · check mode · try again._`;
}

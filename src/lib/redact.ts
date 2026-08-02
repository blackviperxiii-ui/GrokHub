/**
 * Redact secrets from logs / activity feed text.
 */
const PATTERNS: Array<[RegExp, string]> = [
  [/Bearer\s+[A-Za-z0-9\-._~+/]+=*/gi, "Bearer [redacted]"],
  [/sso=[A-Za-z0-9%._\-]{8,}/gi, "sso=[redacted]"],
  [/xai-[A-Za-z0-9_\-]{12,}/gi, "xai-[redacted]"],
  [/sk-[A-Za-z0-9_\-]{16,}/gi, "sk-[redacted]"],
  [/ghp_[A-Za-z0-9]{20,}/gi, "ghp_[redacted]"],
  [/github_pat_[A-Za-z0-9_]{20,}/gi, "github_pat_[redacted]"],
  [/"access_token"\s*:\s*"[^"]+"/gi, '"access_token":"[redacted]"'],
  [/"refresh_token"\s*:\s*"[^"]+"/gi, '"refresh_token":"[redacted]"'],
  [/cookie:\s*[^\n]+/gi, "cookie: [redacted]"],
];

export function redactSecrets(input: string | null | undefined): string {
  let s = String(input ?? "");
  for (const [re, rep] of PATTERNS) {
    s = s.replace(re, rep);
  }
  return s;
}

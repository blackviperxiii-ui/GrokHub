/**
 * Host command risk classification for confirm-before-run UX.
 */

const SAFE_PREFIXES = [
  "ls",
  "pwd",
  "whoami",
  "uname",
  "date",
  "hostname",
  "id",
  "echo",
  "cat",
  "head",
  "tail",
  "wc",
  "file",
  "stat",
  "find",
  "grep",
  "rg",
  "which",
  "type",
  "df",
  "du",
  "free",
  "uptime",
  "env",
  "printenv",
  "ps",
  "top",
  "realpath",
  "readlink",
  "tree",
  "git status",
  "git log",
  "git diff",
  "git branch",
  "git show",
  "npm ls",
  "npm list",
  "node -v",
  "python --version",
  "python3 --version",
];

const DESTRUCTIVE_PATTERNS = [
  /\brm\s+(-[a-zA-Z]*f|[^\n]*\s-rf|\s-fr)/i,
  /\brm\s+-[a-zA-Z]*r/i,
  /\bmkfs\b/i,
  /\bdd\s+if=/i,
  /\b(shutdown|reboot|poweroff|halt)\b/i,
  /\bchmod\s+-R\s+777\b/i,
  /\bchown\s+-R\b/i,
  />\s*\/dev\/sd/i,
  /\bcurl\b.*\|\s*(ba)?sh\b/i,
  /\bwget\b.*\|\s*(ba)?sh\b/i,
  /\bsudo\b/i,
  /\bsystemctl\s+(stop|disable|mask)\b/i,
  /\bpacman\s+-R/i,
  /\bapt(-get)?\s+remove\b/i,
  /\bnpm\s+publish\b/i,
  /\bgit\s+push\s+.*--force\b/i,
  /\bgit\s+reset\s+--hard\b/i,
  /\btruncate\b/i,
  /\bshred\b/i,
];

export type HostRisk = "safe" | "moderate" | "destructive";

function firstToken(cmd: string): string {
  const t = cmd.trim().replace(/^\$\s*/, "");
  // strip env assignments PREFIX=val
  const cleaned = t.replace(/^(\w+=\S+\s+)+/, "");
  return cleaned.split(/\s+/)[0]?.toLowerCase() || "";
}

export function classifyHostCommand(cmd: string): HostRisk {
  const raw = cmd.trim();
  if (!raw) return "safe";
  for (const re of DESTRUCTIVE_PATTERNS) {
    if (re.test(raw)) return "destructive";
  }
  const lower = raw.toLowerCase();
  for (const p of SAFE_PREFIXES) {
    if (lower === p || lower.startsWith(p + " ") || lower.startsWith(p + "\t")) {
      return "safe";
    }
  }
  // pipes / redirects other than simple views → moderate
  if (/[|;]|\s>>?|\s<|&&|\|\|/.test(raw) && !/^\s*(ls|cat|head|tail|grep|rg)\b/i.test(raw)) {
    return "moderate";
  }
  const tok = firstToken(raw);
  if (["rm", "mv", "cp", "chmod", "chown", "kill", "pkill", "killall", "dd"].includes(tok)) {
    return "destructive";
  }
  if (["mkdir", "touch", "tee", "sed", "awk", "npm", "pip", "cargo", "make"].includes(tok)) {
    return "moderate";
  }
  return "moderate";
}

export function needsHostConfirm(
  cmds: string[],
  opts: { confirmAll: boolean; confirmDestructive: boolean },
): boolean {
  if (!cmds.length) return false;
  if (opts.confirmAll) return true;
  if (!opts.confirmDestructive) return false;
  return cmds.some((c) => classifyHostCommand(c) !== "safe");
}

export function riskLabel(risk: HostRisk): string {
  if (risk === "destructive") return "destructive";
  if (risk === "moderate") return "writes / side effects";
  return "read-only";
}


/** True if command matches a user "always allow" prefix or exact entry. */
export function isHostAllowlisted(cmd: string, allowlist: string[] | undefined | null): boolean {
  if (!allowlist?.length) return false;
  const c = cmd.trim().replace(/^\$\s*/, "");
  if (!c) return false;
  const lower = c.toLowerCase();
  for (const raw of allowlist) {
    const a = String(raw || "").trim().toLowerCase();
    if (!a) continue;
    if (lower === a || lower.startsWith(a + " ") || lower.startsWith(a)) return true;
  }
  return false;
}

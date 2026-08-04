/**
 * Prompt / Context / Loop (PCL) — three engineering layers for agent reliability.
 * Inspired by Prompt-Context-Loop architecture:
 *   Prompt  = job contract (what kind of work this turn is)
 *   Context = isolated host evidence (bounded, not raw dumps)
 *   Loop    = changed retries (never repeat the same failed strategy)
 */

export type JobKind =
  | "chat"
  | "host_investigate"
  | "code_build"
  | "ops_debug"
  | "reflect"
  | "creative";

export type LoopRetryStrategy =
  | "plan_only"
  | "same_host_cmd"
  | "broad_scan"
  | "unbounded_find"
  | "empty_reply"
  | "meta_excuse";

const HOST_INV =
  /\b(host|system|install|process(?:es)?|pid|journal|systemd|desktop|file(?:s)?|directory|folder|disk|path|scan|audit|debug|investigate|look at|check|probe|ls|find|grep|cat |read_file|exec)\b/i;
const CODE_BUILD =
  /\b(code|refactor|implement|compile|typescript|tsx?|python|bug|fix|pr\b|diff|function|component|build|lint|typecheck)\b/i;
const OPS =
  /\b(cpu|memory|oom|crash|restart|service|port|network|firewall|package|pacman|aur|electron|nitro)\b/i;
const REFLECT =
  /\b(what did we learn|summarize (the )?session|reflect|preferences|remember that)\b/i;
const CREATIVE =
  /\b(imagine|image|video|draw|illustration|story|poem|creative|logo|sprite)\b/i;

export function detectJobKind(
  prompt: string,
  opts?: { hostToolsEnabled?: boolean },
): JobKind {
  const p = String(prompt || "");
  if (CREATIVE.test(p) && !HOST_INV.test(p)) return "creative";
  if (REFLECT.test(p)) return "reflect";
  if (opts?.hostToolsEnabled !== false && (HOST_INV.test(p) || OPS.test(p))) {
    if (CODE_BUILD.test(p) && !/\b(scan|process|journal|install path)\b/i.test(p)) {
      return "code_build";
    }
    return "host_investigate";
  }
  if (CODE_BUILD.test(p)) return "code_build";
  if (OPS.test(p)) return "ops_debug";
  return "chat";
}

export function jobContractPrompt(kind: JobKind): string {
  const base =
    "PCL job contract — follow this for the whole turn. Do not announce the contract.";
  switch (kind) {
    case "host_investigate":
      return [
        base,
        "Job: host_investigate — gather real machine evidence with HOST_CMD before concluding.",
        "Rules: emit own-line HOST_CMD immediately when local data is needed; never plan-only.",
        "After HOST_RESULT: summarize for the user; do not re-run the same command unless options change.",
        "Bound scans (maxdepth, head). Prefer absolute paths under the install or $HOME.",
      ].join("\n");
    case "code_build":
      return [
        base,
        "Job: code_build — inspect real files with HOST_CMD when paths matter; propose concrete edits.",
        "Rules: no multi-agent APIs; single-agent + HOST_CMD. Prefer working diffs over essays.",
      ].join("\n");
    case "ops_debug":
      return [
        base,
        "Job: ops_debug — diagnose runtime with ps/journal/logs via HOST_CMD; state root cause + fix.",
      ].join("\n");
    case "reflect":
      return [
        base,
        "Job: reflect — distill durable preferences/lessons; keep MEMORY concise; no host churn.",
      ].join("\n");
    case "creative":
      return [
        base,
        "Job: creative — use Imagine mode paths when images/video are requested; keep prose tight.",
      ].join("\n");
    default:
      return [
        base,
        "Job: chat — answer clearly. Only use HOST_CMD if the user needs machine-local facts.",
      ].join("\n");
  }
}

function compressLine(line: string, max = 400): string {
  if (line.length <= max) return line;
  return line.slice(0, Math.floor(max * 0.65)) + " … " + line.slice(-Math.floor(max * 0.25));
}

function compressOutput(block: string, maxChars: number): string {
  const s = String(block || "");
  if (s.length <= maxChars) return s;
  const lines = s.split("\n");
  if (lines.length <= 2) return compressLine(s, maxChars);
  const headN = Math.min(40, Math.ceil(lines.length * 0.45));
  const tailN = Math.min(20, Math.ceil(lines.length * 0.2));
  const head = lines.slice(0, headN).map((l) => compressLine(l, 500));
  const tail = lines.slice(-tailN).map((l) => compressLine(l, 500));
  let out = [...head, `… (${lines.length - headN - tailN} lines omitted) …`, ...tail].join("\n");
  if (out.length > maxChars) out = out.slice(0, maxChars - 20) + "\n… [truncated]";
  return out;
}

export function isolateHostResultsForModel(
  outputs: string[],
  maxPer = 4500,
): { modelBlock: string; isolated: boolean; totalRaw: number } {
  const list = (outputs || []).map((o) => String(o || ""));
  const totalRaw = list.reduce((n, s) => n + s.length, 0);
  const compressed = list.map((o, i) => {
    const body = compressOutput(o, maxPer);
    return `### host output ${i + 1}\n${body}`;
  });
  const isolated = list.some((o, i) => compressed[i]!.length + 30 < o.length);
  const modelBlock = [
    "HOST_RESULT (authoritative — use this, do not invent files):",
    compressed.join("\n\n---\n\n"),
    "",
    isolated
      ? "Note: large host output was compressed for context (head/tail). Ask for a narrower path if you need more."
      : "",
    "Summarize these results for the user in plain language.",
    "If a scan timed out or was truncated, say so and suggest a narrower path.",
    "Do not output HOST_CMD again unless you still need a *different* command (changed options/path).",
  ]
    .filter(Boolean)
    .join("\n");
  return { modelBlock, isolated, totalRaw };
}

export function detectTriedStrategies(
  assistantText: string,
  hostOutputs: string[],
): LoopRetryStrategy[] {
  const tried: LoopRetryStrategy[] = [];
  const a = String(assistantText || "");
  if (!a.trim()) tried.push("empty_reply");
  if (
    /\b(let me|i'll|i will|running checks|looking into)\b/i.test(a) &&
    !/HOST_CMD\s*:/i.test(a)
  ) {
    tried.push("plan_only");
  }
  if (/\bonly describing|never output|didn't (run|emit)/i.test(a)) {
    tried.push("meta_excuse");
  }
  const joined = hostOutputs.join("\n");
  if (/find\s+\/\s|find\s+\$HOME/i.test(joined + a)) tried.push("broad_scan");
  if (/unbounded|Argument list too long/i.test(joined)) tried.push("unbounded_find");
  const cmds = [...a.matchAll(/HOST_CMD\s*:\s*(.+)$/gim)].map((m) => m[1]!.trim());
  if (cmds.length >= 2 && cmds[0] === cmds[1]) tried.push("same_host_cmd");
  return [...new Set(tried)];
}

export function buildChangedRetryNudge(
  attempt: number,
  tried: LoopRetryStrategy[],
  opts?: { userPrompt?: string; hostAvailable?: boolean },
): string {
  const avoid = tried.length ? tried.join(", ") : "none recorded";
  const host = opts?.hostAvailable !== false;
  const nextHints: string[] = [];
  if (tried.includes("plan_only") || tried.includes("meta_excuse")) {
    nextHints.push(
      "CHANGED STRATEGY: emit HOST_CMD on its own line in the first 3 lines — no planning prose.",
    );
  }
  if (tried.includes("broad_scan") || tried.includes("unbounded_find")) {
    nextHints.push(
      "CHANGED STRATEGY: narrow scope — use find -maxdepth 3, head -n 80, or a specific directory.",
    );
  }
  if (tried.includes("same_host_cmd")) {
    nextHints.push(
      "CHANGED STRATEGY: do not repeat the same HOST_CMD; change flags, path, or tool.",
    );
  }
  if (!nextHints.length) {
    nextHints.push(
      host
        ? "CHANGED STRATEGY: pick a smaller evidence command or finish with a concrete answer."
        : "CHANGED STRATEGY: finish with a concrete answer without host tools.",
    );
  }
  return [
    "SYSTEM — CHANGED RETRY (PCL loop; do not mention this note):",
    `Attempt ${attempt}. Prior strategies to AVOID: ${avoid}.`,
    ...nextHints,
    "Do not say you will check later — act or conclude now.",
    opts?.userPrompt ? `User goal: ${opts.userPrompt.slice(0, 400)}` : "",
  ]
    .filter(Boolean)
    .join("\n");
}

export function tagErrorLayer(
  err: unknown,
): "prompt" | "context" | "loop" | "auth" | "host" | "network" | "unknown" {
  const m = err instanceof Error ? err.message : String(err || "");
  if (/401|unauthorized|token|oauth|credentials/i.test(m)) return "auth";
  if (/HOST|host gateway|bridge|safe mode/i.test(m)) return "host";
  if (/network|fetch|ECONN|ENOTFOUND|offline/i.test(m)) return "network";
  if (/context|token.?limit|too large|maximum context/i.test(m)) return "context";
  if (/stall|incomplete|retry|loop/i.test(m)) return "loop";
  if (/model|prompt|invalid.?request/i.test(m)) return "prompt";
  return "unknown";
}

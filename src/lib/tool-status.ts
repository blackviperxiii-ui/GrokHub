/** Plain-language tool trail for chat while Grok is working. */

export type ToolKind = "host" | "connector" | "selfmod" | "computer" | "summarize" | "stream" | "tool";

export type ToolUiStatus = {
  kind: ToolKind;
  title: string;
  detail: string;
  phase: "running" | "done" | "error";
};

function shortenCmd(cmd: string, max = 56): string {
  const s = String(cmd || "").replace(/\s+/g, " ").trim();
  if (s.length <= max) return s;
  return `${s.slice(0, max - 1)}…`;
}

export function humanizeHostCommand(command: string): string {
  const c = String(command || "").trim();
  const head = c.split(/[|&;]|&&|\|\|/)[0]?.trim() || c;
  const bin = head.replace(/^sudo\s+/, "").split(/\s+/)[0] || "";
  const base = bin.split("/").pop() || bin;
  switch (base) {
    case "ls":
    case "ll":
    case "tree":
      return "Looking at files";
    case "cat":
    case "head":
    case "tail":
    case "less":
    case "more":
      return "Reading a file";
    case "pwd":
    case "whoami":
    case "id":
    case "uname":
    case "hostname":
    case "date":
    case "uptime":
      return "Checking this machine";
    case "ps":
    case "top":
    case "htop":
      return "Checking running programs";
    case "find":
    case "fd":
    case "rg":
    case "grep":
      return "Searching";
    case "df":
    case "du":
    case "free":
      return "Checking disk or memory";
    case "journalctl":
    case "dmesg":
      return "Reading logs";
    case "systemctl":
      return "Checking a service";
    case "env":
    case "printenv":
      return "Checking environment";
    case "which":
    case "type":
    case "command":
      return "Looking up a program";
    case "stat":
    case "file":
    case "realpath":
    case "readlink":
      return "Checking a path";
    default:
      return "Running a command";
  }
}

export function humanizeComputerCommand(command: string): string {
  const c = String(command || "").trim();
  const op = c.split(/\s+/)[0]?.toLowerCase() || "";
  switch (op) {
    case "screenshot":
      return "Taking a screenshot";
    case "click":
      return "Clicking on the screen";
    case "double_click":
      return "Double-clicking";
    case "type":
      return "Typing";
    case "key":
      return "Pressing a key";
    case "scroll":
      return "Scrolling";
    case "move":
      return "Moving the pointer";
    case "wait":
      return "Waiting a moment";
    default:
      return c ? `Desktop: ${shortenCmd(c, 40)}` : "Using the desktop";
  }
}

function extractHostCmd(status: string): string {
  return status
    .replace(/^Host:\s*/i, "")
    .replace(/^Host parallel\s+\d+\/\d+:\s*/i, "")
    .replace(/\s*\(\d+s\)\s*$/i, "")
    .replace(/…$/, "")
    .trim();
}

export function humanizeStreamStatus(status: string | null | undefined): string {
  if (!status) return "Working…";
  const s = status.trim();
  const parallel = s.match(/running\s+(\d+)\s+read-only/i);
  if (parallel) return `Checking ${parallel[1]} things on your computer…`;
  if (/^Host:/i.test(s) || /^Host parallel/i.test(s)) {
    const cmd = extractHostCmd(s);
    if (!cmd || /running on your desktop/i.test(cmd)) return "On your computer…";
    return `On your computer: ${humanizeHostCommand(cmd).toLowerCase()}`;
  }
  if (/^Computer:/i.test(s)) {
    const cmd = s.replace(/^Computer:\s*/i, "").replace(/…$/, "").trim();
    return `On the desktop: ${humanizeComputerCommand(cmd).toLowerCase()}`;
  }
  if (/Controlling (the )?desktop/i.test(s)) return "Using the desktop…";
  if (/computer use/i.test(s) && /approval/i.test(s)) return "Waiting for you to approve desktop control…";
  if (/Waiting for (host |computer-use )?approval/i.test(s)) return "Waiting for you to approve…";
  if (/^Connector:/i.test(s) || /Running connector/i.test(s)) return "Using a connected service…";
  if (/Self-mod/i.test(s)) return "Changing the app…";
  if (/Summariz/i.test(s) || /compressed for context/i.test(s)) return "Reading the results…";
  if (/Adaptive/i.test(s)) return "Picking a model…";
  if (/Nudging|Starting host investigation/i.test(s)) return "Asking Grok to check your computer…";
  if (/Auto-finish|completing goal/i.test(s)) return "Finishing the answer…";
  if (/Connecting/i.test(s)) return "Connecting…";
  if (/Tool loop|calling model/i.test(s)) return "Reading those results to decide the next step…";
  if (/Host results compressed/i.test(s)) return "Reading the results…";
  if (/Host running/i.test(s)) return "On your computer…";
  if (/Thinking|Waiting/i.test(s)) return /Waiting for you/i.test(s) ? s : "Thinking…";
  if (/Replaying desktop/i.test(s)) return "Replaying a saved desktop skill…";
  if (/Streaming · round/i.test(s)) return "Writing the next step…";
  if (/Streaming|Responding|Context \d+%/i.test(s)) return "Writing a reply…";
  if (/Context compacted/i.test(s)) return "Making room in the conversation…";
  return s.length > 72 ? `${s.slice(0, 71)}…` : s;
}

/** Short label for the streaming pill on a message. */
export function streamStatusPill(status: string | null | undefined): string {
  if (!status) return "working";
  const s = status.trim();
  if (/^Host:/i.test(s) || /Running on your desktop/i.test(s) || /Host parallel/i.test(s)) {
    return "on your computer";
  }
  if (/^Computer:/i.test(s) || /computer use/i.test(s) || /Controlling (the )?desktop/i.test(s)) {
    return "on the desktop";
  }
  if (/^Connector:/i.test(s) || /Running connector/i.test(s)) return "using a service";
  if (/Self-mod/i.test(s)) return "changing the app";
  if (/Summariz/i.test(s)) return "reading results";
  if (/Adaptive/i.test(s)) return "picking a model";
  if (/Connecting/i.test(s)) return "connecting";
  if (/Thinking|Waiting/i.test(s)) return /approval/i.test(s) ? "needs you" : "thinking";
  if (/round/i.test(s)) return "next step";
  if (/Streaming|Responding/i.test(s)) return "writing";
  if (/approval/i.test(s)) return "needs you";
  return "working";
}

export function parseStreamStatus(status: string | null | undefined): ToolUiStatus | null {
  if (!status) return null;
  const s = status.trim();
  const detail = humanizeStreamStatus(s);
  if (/^Host:\s*/i.test(s) || /Running on your desktop/i.test(s) || /Host parallel/i.test(s)) {
    return { kind: "host", title: "On your computer", detail, phase: "running" };
  }
  if (/^Computer:/i.test(s) || /computer use/i.test(s) || /Controlling (the )?desktop/i.test(s)) {
    return { kind: "computer", title: "On the desktop", detail, phase: "running" };
  }
  if (/^Connector:/i.test(s) || /Running connector/i.test(s)) {
    return { kind: "connector", title: "Connected service", detail, phase: "running" };
  }
  if (/Self-mod/i.test(s)) {
    return { kind: "selfmod", title: "App change", detail, phase: "running" };
  }
  if (/Summariz/i.test(s)) {
    return { kind: "summarize", title: "Reading results", detail, phase: "running" };
  }
  if (/approval|Waiting for host/i.test(s)) {
    return { kind: "host", title: "Needs you", detail, phase: "running" };
  }
  if (/Host tool round|Streaming|Thinking|Connecting|Working|Adaptive|Responding/i.test(s)) {
    return {
      kind: "stream",
      title: s.includes("round") ? "Next step" : s.includes("Adaptive") ? "Routing" : "Grok",
      detail,
      phase: "running",
    };
  }
  return { kind: "stream", title: "Working", detail, phase: "running" };
}

function kindTitle(kind: "host" | "connector" | "selfmod" | "computer"): string {
  switch (kind) {
    case "host":
      return "On your computer";
    case "connector":
      return "Connected service";
    case "computer":
      return "On the desktop";
    case "selfmod":
      return "Changing the app";
    default: {
      const _never: never = kind;
      return _never;
    }
  }
}

function whyLine(kind: "host" | "connector" | "selfmod" | "computer", command: string): string {
  switch (kind) {
    case "host":
      return humanizeHostCommand(command);
    case "computer":
      return humanizeComputerCommand(command);
    case "connector":
      return `Using ${shortenCmd(command, 40)}`;
    case "selfmod":
      return shortenCmd(command, 64) || "Updating the app";
    default: {
      const _never: never = kind;
      return _never;
    }
  }
}

export function summarizeToolOutput(raw: string): { label: string; ok: boolean; command: string } {
  const t = String(raw || "");
  const host = t.match(/^\$\s+(.+)$/m);
  if (host) {
    const cmd = host[1]!.trim();
    const exit = t.match(/^exit\s+(\S+)/m);
    const ok = exit ? exit[1] === "0" : !/\[host error\]|failed|timed out/i.test(t);
    return { label: humanizeHostCommand(cmd), ok, command: shortenCmd(cmd) };
  }
  const comp = t.match(/^COMPUTER\s+(.+)$/m);
  if (comp) {
    const cmd = comp[1]!.trim();
    const ok = /\bok\b/i.test(t) && !/\bfailed\b/i.test(t);
    return { label: humanizeComputerCommand(cmd), ok, command: shortenCmd(cmd) };
  }
  const conn = t.match(/^CONNECTOR\s+(.+)$/m);
  if (conn) {
    const cmd = conn[1]!.trim();
    const ok = /\bok\b/i.test(t) && !/\bfailed\b|\[error\]/i.test(t);
    return { label: `Using ${shortenCmd(cmd, 40)}`, ok, command: shortenCmd(cmd) };
  }
  const self = t.match(/^(LIST|READ|WRITE|PATCH|SNAPSHOT)\s+(.+)$/m);
  if (self) {
    const kind = self[1] as "LIST" | "READ" | "WRITE" | "PATCH" | "SNAPSHOT";
    const target = shortenCmd(self[2]!, 40);
    const ok = !/\bfailed\b|error/i.test(t.slice(0, 200));
    let label: string;
    switch (kind) {
      case "LIST":
        label = "Listing app files";
        break;
      case "READ":
        label = "Reading app files";
        break;
      case "WRITE":
        label = "Updating an app file";
        break;
      case "PATCH":
        label = "Patching an app file";
        break;
      case "SNAPSHOT":
        label = "Saving an app snapshot";
        break;
      default: {
        const _never: never = kind;
        label = _never;
      }
    }
    return { label, ok, command: target };
  }
  const first = t.split("\n").find((l) => l.trim()) || "step";
  const ok = !/\bfailed\b|error|timed out/i.test(t.slice(0, 400));
  return { label: shortenCmd(first, 48), ok, command: shortenCmd(first) };
}

export function toolRunningMarkdown(opts: {
  kind: "host" | "connector" | "selfmod" | "computer";
  command: string;
  preface?: string;
  elapsedSec?: number;
  step?: { index: number; total: number };
}): string {
  const stepBit =
    opts.step && opts.step.total > 1 ? ` · ${opts.step.index} of ${opts.step.total}` : "";
  const sec = Math.max(0, Math.floor(opts.elapsedSec || 0));
  const timeBit = sec >= 3 ? ` · ${sec}s` : "";
  return [
    (opts.preface || "").trim(),
    "",
    `**${kindTitle(opts.kind)}**${stepBit}${timeBit}`,
    whyLine(opts.kind, opts.command),
    "",
  ]
    .filter((line, i, arr) => !(line === "" && arr[i - 1] === ""))
    .join("\n")
    .trim();
}

export function toolResultMarkdown(opts: {
  kind: "host" | "connector" | "selfmod" | "computer";
  preface?: string;
  outputs: string[];
  summarizing?: boolean;
}): string {
  const lines = (opts.outputs || []).map((raw) => {
    const s = summarizeToolOutput(raw);
    return `- ${s.label} — ${s.ok ? "done" : "failed"}`;
  });
  return [
    (opts.preface || "").trim(),
    "",
    `**${kindTitle(opts.kind)}**`,
    lines.length ? lines.join("\n") : "- No output",
    "",
    opts.summarizing !== false ? "Reading the results…" : "",
  ]
    .filter((x) => x !== "")
    .join("\n");
}

/** Several safe host commands running together. */
export function toolParallelMarkdown(opts: {
  kind: "host";
  preface?: string;
  commands: string[];
}): string {
  const cmds = opts.commands || [];
  const lines = cmds.map((c) => `- ${humanizeHostCommand(c)}`);
  return [
    (opts.preface || "").trim(),
    "",
    `**${kindTitle(opts.kind)}** · ${cmds.length} at once`,
    lines.length ? lines.join("\n") : "- Working…",
    "",
  ]
    .filter((line, i, arr) => !(line === "" && arr[i - 1] === ""))
    .join("\n")
    .trim();
}

/** Gap filler while waiting for the next model round after tools. */
export function toolLoopWaitMarkdown(preface: string, _round: number): string {
  return [(preface || "").trim(), "", "Reading those results to decide the next step…"]
    .filter((line, i, arr) => !(line === "" && arr[i - 1] === ""))
    .join("\n")
    .trim();
}

/** Visible live trail or raw tool protocol — not a plan-only stall. */
export function hasToolTrailEvidence(text: string): boolean {
  const s = String(text || "");
  if (/HOST_CMD\s*:|CONNECTOR_CMD\s*:|COMPUTER_CMD\s*:|SELF_MOD_CMD\s*:/i.test(s)) return true;
  if (/HOST_RESULT|COMPUTER_RESULT|CONNECTOR_RESULT|SELF_MOD_RESULT/i.test(s)) return true;
  if (/### 🖥️|exit code|```host\b/i.test(s)) return true;
  if (
    /\*\*On your computer\*\*|\*\*On the desktop\*\*|\*\*Connected service\*\*|\*\*Changing the app\*\*/i.test(
      s,
    )
  ) {
    return true;
  }
  if (/Checked your machine\.|Used the desktop\.|Reading those results to decide the next step/i.test(s)) {
    return true;
  }
  return false;
}

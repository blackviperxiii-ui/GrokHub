/** Build readable tool-run cards for chat + parse streamStatus for UI. */

export type ToolKind = "host" | "connector" | "selfmod" | "summarize" | "stream" | "tool";

export type ToolUiStatus = {
  kind: ToolKind;
  title: string;
  detail: string;
  phase: "running" | "done" | "error";
};

/** Short label for the streaming pill on a message. */
export function streamStatusPill(status: string | null | undefined): string {
  if (!status) return "working";
  const s = status.trim();
  if (/^Host:/i.test(s) || /Running on your desktop/i.test(s)) return "tool · host";
  if (/^Connector:/i.test(s) || /Running connector/i.test(s)) return "tool · connector";
  if (/Self-mod/i.test(s)) return "tool · self-mod";
  if (/Summariz/i.test(s)) return "summarizing";
  if (/Adaptive/i.test(s)) return "routing";
  if (/Connecting/i.test(s)) return "connecting";
  if (/Thinking|Waiting/i.test(s)) return "thinking";
  if (/round/i.test(s)) return "tool loop";
  if (/Streaming|Responding/i.test(s)) return "streaming";
  if (/approval/i.test(s)) return "awaiting you";
  return "working";
}

export function parseStreamStatus(status: string | null | undefined): ToolUiStatus | null {
  if (!status) return null;
  const s = status.trim();
  if (/^Host:\s*/i.test(s) || /Running on your desktop/i.test(s)) {
    return {
      kind: "host",
      title: "Desktop host",
      detail: s.replace(/^Host:\s*/i, "").replace(/…$/, "") || "Running command…",
      phase: "running",
    };
  }
  if (/^Connector:/i.test(s) || /Running connector/i.test(s)) {
    return {
      kind: "connector",
      title: "Connector",
      detail: s.replace(/^Connector:\s*/i, "") || "Running tool…",
      phase: "running",
    };
  }
  if (/Self-mod/i.test(s)) {
    return {
      kind: "selfmod",
      title: "App self-mod",
      detail: s,
      phase: "running",
    };
  }
  if (/Summariz/i.test(s)) {
    return {
      kind: "summarize",
      title: "Summarizing results",
      detail: "Turning tool output into a clear reply…",
      phase: "running",
    };
  }
  if (/approval|Waiting for host/i.test(s)) {
    return {
      kind: "host",
      title: "Waiting for approval",
      detail: s,
      phase: "running",
    };
  }
  if (/Host tool round|Streaming|Thinking|Connecting|Working|Adaptive|Responding/i.test(s)) {
    return {
      kind: "stream",
      title: s.includes("round") ? "Tool loop" : s.includes("Adaptive") ? "Routing" : "Grok",
      detail: s,
      phase: "running",
    };
  }
  return {
    kind: "stream",
    title: "Working",
    detail: s,
    phase: "running",
  };
}

export function toolRunningMarkdown(opts: {
  kind: "host" | "connector" | "selfmod";
  command: string;
  preface?: string;
  /** Elapsed seconds while tool is running (keeps stream "alive") */
  elapsedSec?: number;
  /** e.g. 1 of 3 */
  step?: { index: number; total: number };
}): string {
  const title =
    opts.kind === "host"
      ? "Desktop host"
      : opts.kind === "connector"
        ? "Connector tool"
        : "Self-modification";
  const icon = opts.kind === "host" ? "🖥️" : opts.kind === "connector" ? "🔌" : "🛠️";
  const cmd =
    opts.kind === "host"
      ? `$ ${opts.command}`
      : opts.kind === "connector"
        ? opts.command
        : opts.command;
  const sec = Math.max(0, Math.floor(opts.elapsedSec || 0));
  const dots = ".".repeat((sec % 3) + 1).padEnd(3, " ");
  const stepBit =
    opts.step && opts.step.total > 1
      ? ` · step ${opts.step.index}/${opts.step.total}`
      : "";
  const timeBit = sec > 0 ? ` · ${sec}s` : "";
  return [
    (opts.preface || "").trim(),
    "",
    "---",
    "",
    `### ${icon} ${title}${stepBit}`,
    "",
    `> **Running now${dots}**${timeBit} — stream is live, waiting on tools`,
    ">",
    `> \`${cmd}\``,
    "",
    "---",
    "",
  ]
    .filter((line, i, arr) => !(line === "" && arr[i - 1] === ""))
    .join("\n")
    .trimStart();
}

export function toolResultMarkdown(opts: {
  kind: "host" | "connector" | "selfmod";
  preface?: string;
  outputs: string[];
  summarizing?: boolean;
}): string {
  const title =
    opts.kind === "host"
      ? "Desktop results"
      : opts.kind === "connector"
        ? "Connector results"
        : "Self-mod results";
  const lang = opts.kind === "host" ? "shell" : "text";
  const body = opts.outputs.join("\n\n---\n\n") || "(no output)";
  return [
    (opts.preface || "").trim(),
    "",
    `### ${title}`,
    "",
    "```" + lang,
    body,
    "```",
    "",
    opts.summarizing !== false
      ? "_Summarizing results… stream is live — next model tokens coming soon._"
      : "",
  ]
    .filter((x) => x !== "")
    .join("\n");
}

/** Gap filler while waiting for the next model round after tools. */
export function toolLoopWaitMarkdown(preface: string, round: number): string {
  return [
    (preface || "").trim(),
    "",
    "---",
    "",
    `### ⏳ Tool round ${round}`,
    "",
    "> **Working…** feeding results back to Grok for the next step",
    "",
    "---",
    "",
  ]
    .filter((line, i, arr) => !(line === "" && arr[i - 1] === ""))
    .join("\n")
    .trimStart();
}

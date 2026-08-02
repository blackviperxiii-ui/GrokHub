/** Build readable tool-run cards for chat + parse streamStatus for UI. */

export type ToolKind = "host" | "connector" | "selfmod" | "summarize" | "stream";

export type ToolUiStatus = {
  kind: ToolKind;
  title: string;
  detail: string;
  phase: "running" | "done" | "error";
};

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
  if (/Host tool round|Streaming|Thinking|Connecting|Working/i.test(s)) {
    return {
      kind: "stream",
      title: s.includes("round") ? "Tool loop" : "Grok",
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
  return [
    (opts.preface || "").trim(),
    "",
    "---",
    "",
    `### ${icon} ${title}`,
    "",
    `> **Running now** — this may take a few seconds`,
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
    opts.summarizing !== false ? "_Summarizing results…_" : "",
  ]
    .filter((x) => x !== "")
    .join("\n");
}

/**
 * COMPUTER_CMD protocol: screenshot + pointer/keyboard actions for desktop computer-use.
 */

export const MAX_COMPUTER_ACTIONS_PER_TURN = 25;
export const MIN_ACTION_DELAY_MS = 80;
export const SCREENSHOT_MAX_WIDTH = 1280;

export type ComputerOp =
  | "screenshot"
  | "click"
  | "double_click"
  | "type"
  | "key"
  | "scroll"
  | "move"
  | "wait";

export type ComputerStep = {
  op: ComputerOp;
  x?: number;
  y?: number;
  text?: string;
  key?: string;
  direction?: "up" | "down";
  amount?: number;
  ms?: number;
  note?: string;
};

export type ComputerRecipe = {
  version: 1;
  screen: { width: number; height: number };
  steps: ComputerStep[];
  summary: string;
};

export type ComputerRisk = "safe" | "moderate";

const OPS: ComputerOp[] = [
  "screenshot",
  "click",
  "double_click",
  "type",
  "key",
  "scroll",
  "move",
  "wait",
];

function isOp(s: string): s is ComputerOp {
  return (OPS as string[]).includes(s);
}

export function isComputerWriteOp(op: ComputerOp): boolean {
  switch (op) {
    case "screenshot":
    case "wait":
      return false;
    case "click":
    case "double_click":
    case "type":
    case "key":
    case "scroll":
    case "move":
      return true;
    default: {
      const _never: never = op;
      return _never;
    }
  }
}

export function classifyComputerStep(step: ComputerStep): ComputerRisk {
  return isComputerWriteOp(step.op) ? "moderate" : "safe";
}

export function needsComputerConfirm(
  steps: ComputerStep[],
  opts: { confirmAll: boolean; confirmDestructive: boolean },
): boolean {
  if (!steps.length) return false;
  if (opts.confirmAll) return true;
  if (!opts.confirmDestructive) return false;
  return steps.some((s) => classifyComputerStep(s) !== "safe");
}

export function formatComputerCommand(step: ComputerStep): string {
  switch (step.op) {
    case "screenshot":
      return "screenshot";
    case "click":
      return `click ${num(step.x)} ${num(step.y)}`;
    case "double_click":
      return `double_click ${num(step.x)} ${num(step.y)}`;
    case "move":
      return `move ${num(step.x)} ${num(step.y)}`;
    case "type":
      return `type ${step.text || ""}`;
    case "key":
      return `key ${step.key || ""}`;
    case "scroll":
      return `scroll ${step.direction || "down"} ${step.amount ?? 1}`;
    case "wait":
      return `wait ${step.ms ?? 400}`;
    default: {
      const _never: never = step.op;
      return String(_never);
    }
  }
}

function num(n: number | undefined): number {
  return Number.isFinite(n) ? Math.round(Number(n)) : 0;
}

function parseJsonStep(raw: string): ComputerStep | null {
  try {
    const j = JSON.parse(raw) as Record<string, unknown>;
    if (!j || typeof j !== "object") return null;
    const op = String(j.op || "").toLowerCase();
    if (!isOp(op)) return null;
    return normalizeStep({
      op,
      x: toNum(j.x),
      y: toNum(j.y),
      text: typeof j.text === "string" ? j.text : undefined,
      key: typeof j.key === "string" ? j.key : undefined,
      direction: j.direction === "up" || j.direction === "down" ? j.direction : undefined,
      amount: toNum(j.amount),
      ms: toNum(j.ms),
      note: typeof j.note === "string" ? j.note : undefined,
    });
  } catch {
    return null;
  }
}

function toNum(v: unknown): number | undefined {
  if (typeof v === "number" && Number.isFinite(v)) return v;
  if (typeof v === "string" && v.trim() && Number.isFinite(Number(v))) return Number(v);
  return undefined;
}

function unquote(s: string): string {
  const t = s.trim();
  if ((t.startsWith('"') && t.endsWith('"')) || (t.startsWith("'") && t.endsWith("'"))) {
    return t.slice(1, -1);
  }
  return t;
}

export function parseComputerCommand(raw: string): ComputerStep | null {
  const line = String(raw || "")
    .trim()
    .replace(/^COMPUTER_CMD:\s*/i, "");
  if (!line) return null;
  if (line.startsWith("{")) return parseJsonStep(line);
  const m = line.match(/^(\S+)(?:\s+([\s\S]*))?$/);
  if (!m) return null;
  const opRaw = m[1]!.toLowerCase().replace(/-/g, "_");
  const rest = (m[2] || "").trim();
  if (!isOp(opRaw)) return null;
  if (opRaw === "screenshot") return { op: "screenshot" };
  if (opRaw === "wait") {
    const ms = Math.max(0, Math.min(15_000, Number(rest.split(/\s+/)[0]) || 400));
    return { op: "wait", ms };
  }
  if (opRaw === "type") return { op: "type", text: unquote(rest) };
  if (opRaw === "key") return { op: "key", key: unquote(rest) || "Return" };
  if (opRaw === "scroll") {
    const parts = rest.split(/\s+/).filter(Boolean);
    const dir = parts[0] === "up" || parts[0] === "down" ? parts[0] : "down";
    const amount = Math.max(1, Math.min(20, Number(parts[1] || parts[0]) || 1));
    return { op: "scroll", direction: dir, amount };
  }
  if (opRaw === "click" || opRaw === "double_click" || opRaw === "move") {
    const parts = rest.split(/\s+/).filter(Boolean);
    const x = Number(parts[0]);
    const y = Number(parts[1]);
    if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
    return { op: opRaw, x: Math.round(x), y: Math.round(y) };
  }
  return null;
}

function normalizeStep(step: ComputerStep): ComputerStep {
  if (step.op === "wait") {
    return { ...step, ms: Math.max(0, Math.min(15_000, step.ms ?? 400)) };
  }
  if (step.op === "scroll") {
    return {
      ...step,
      direction: step.direction === "up" ? "up" : "down",
      amount: Math.max(1, Math.min(20, step.amount ?? 1)),
    };
  }
  return step;
}

export function extractComputerCommands(text: string): ComputerStep[] {
  const out: ComputerStep[] = [];
  const seen = new Set<string>();
  for (const line of String(text || "").split("\n")) {
    const m = line.match(/^\s*COMPUTER_CMD:\s*(.+?)\s*$/i);
    if (!m?.[1]) continue;
    const step = parseComputerCommand(m[1]);
    if (!step) continue;
    const key = formatComputerCommand(step).toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(step);
  }
  return out.slice(0, MAX_COMPUTER_ACTIONS_PER_TURN);
}

export function stripComputerCommands(text: string): string {
  return String(text || "")
    .split("\n")
    .filter((line) => !/^\s*COMPUTER_CMD:\s*/i.test(line))
    .join("\n")
    .replace(/\s*COMPUTER_CMD:\s*[^\n]+/gi, "")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

export function parseComputerRecipe(raw: unknown): ComputerRecipe | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Partial<ComputerRecipe>;
  if (r.version !== 1 || !Array.isArray(r.steps)) return null;
  const steps: ComputerStep[] = [];
  for (const row of r.steps) {
    if (!row || typeof row !== "object") continue;
    const op = String((row as ComputerStep).op || "");
    if (!isOp(op)) continue;
    const step = normalizeStep({ ...(row as ComputerStep), op });
    steps.push(step);
  }
  if (!steps.length) return null;
  const screen = r.screen && typeof r.screen === "object" ? r.screen : { width: 0, height: 0 };
  return {
    version: 1,
    screen: {
      width: Number(screen.width) || 0,
      height: Number(screen.height) || 0,
    },
    steps,
    summary: String(r.summary || ""),
  };
}

export function computerPromptBlock(enabled: boolean): string {
  if (!enabled) {
    return "NOTE: Computer use (screen/mouse/keyboard) is DISABLED. Do not emit COMPUTER_CMD.";
  }
  return [
    "## Computer use (LIVE)",
    "You can see the Linux desktop and control mouse/keyboard. Coordinates are in the LAST screenshot pixel space (COMPUTER_RESULT lists screenshot WxH and screen WxH).",
    "Own-line commands only:",
    "COMPUTER_CMD: screenshot",
    "COMPUTER_CMD: click 412 880",
    "COMPUTER_CMD: double_click 412 880",
    "COMPUTER_CMD: move 100 200",
    "COMPUTER_CMD: type Hello world",
    "COMPUTER_CMD: key ctrl+t",
    "COMPUTER_CMD: scroll down 3",
    "COMPUTER_CMD: wait 400",
    "A live ffmpeg/grim picture loop is attached after each click/type/key (no screenshot app). Use the latest frame for the next click. Map clicks to the screenshot size, not a guessed resolution.",
    "Do not click the GrokHub window. Prefer the user's other apps (browser, terminals, files).",
    "Requires xAI API key or Grok OAuth (vision). Website-free fallback cannot see the screen.",
    "After COMPUTER_RESULT, continue with more COMPUTER_CMD or summarize. Do not announce control without emitting COMPUTER_CMD in the same reply.",
  ].join("\n");
}

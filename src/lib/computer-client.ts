import type { ComputerRecipe, ComputerStep } from "./computer-protocol";
import { formatComputerCommand } from "./computer-protocol";

export type ComputerInfo = {
  ok: boolean;
  platform?: string;
  session?: string;
  injector?: string | null;
  injectorPath?: string | null;
  display?: string | null;
  waylandDisplay?: string | null;
  electron?: boolean;
  screen?: { width: number; height: number; scaleFactor?: number } | null;
  lastScreenshot?: {
    width: number;
    height: number;
    screenWidth: number;
    screenHeight: number;
  } | null;
  hint?: string;
  error?: string;
  previewing?: boolean;
  capture?: string;
  captureTools?: string[];
};

export type ComputerFrame = {
  dataUrl?: string;
  screen?: { width: number; height: number; scaleFactor?: number };
  screenshot?: { width: number; height: number };
  capture?: string;
};

export type ComputerActResult = {
  ok: boolean;
  op?: string;
  error?: string;
  dataUrl?: string;
  screen?: { width: number; height: number; scaleFactor?: number };
  screenshot?: { width: number; height: number };
  injector?: string;
  mapped?: { x: number; y: number; mapped?: boolean };
  ms?: number;
  capture?: string;
};

type ComputerBridge = {
  info: () => Promise<ComputerInfo>;
  screenshot: () => Promise<ComputerActResult>;
  act: (step: ComputerStep) => Promise<ComputerActResult>;
  beginSession: () => Promise<ComputerInfo>;
  endSession: () => Promise<{ ok: boolean }>;
  stop: () => Promise<{ ok: boolean }>;
  startPreview?: (ms?: number) => Promise<{ ok: boolean; previewing?: boolean }>;
  stopPreview?: () => Promise<{ ok: boolean }>;
  onFrame?: (cb: (frame: ComputerFrame) => void) => () => void;
};

function electronComputer(): ComputerBridge | undefined {
  if (typeof window === "undefined") return undefined;
  return window.grokhubDesktop?.computer;
}

export function computerAvailable(): boolean {
  return Boolean(electronComputer()?.act);
}

export async function computerInfo(): Promise<ComputerInfo> {
  const e = electronComputer();
  if (e?.info) {
    try {
      return await e.info();
    } catch (err) {
      return { ok: false, error: err instanceof Error ? err.message : "computer info failed" };
    }
  }
  return {
    ok: false,
    error:
      "Computer use requires the Electron desktop shell. Relaunch GrokHub from the Arch package.",
  };
}

export async function computerBeginSession(): Promise<ComputerInfo> {
  const e = electronComputer();
  if (e?.beginSession) return e.beginSession();
  return computerInfo();
}

export async function computerEndSession(): Promise<{ ok: boolean; previewing?: boolean }> {
  const e = electronComputer();
  try {
    const r = await e?.endSession?.();
    if (r && typeof r === "object") return r;
  } catch {
    /* ignore */
  }
  return { ok: true, previewing: false };
}

export async function computerStop(): Promise<void> {
  const e = electronComputer();
  try {
    await e?.stop?.();
  } catch {
    /* ignore */
  }
}

export async function computerStartPreview(
  intervalMs = 450,
): Promise<{ ok: boolean; error?: string; capture?: string }> {
  const e = electronComputer();
  if (e?.startPreview) return e.startPreview(intervalMs);
  return { ok: false };
}

export async function computerStopPreview(): Promise<void> {
  const e = electronComputer();
  try {
    await e?.stopPreview?.();
  } catch {
    /* ignore */
  }
}

export function onComputerFrame(cb: (frame: ComputerFrame) => void): () => void {
  const e = electronComputer();
  if (e?.onFrame) return e.onFrame(cb);
  const fn = (ev: Event) => {
    const detail = (ev as CustomEvent<ComputerFrame>).detail;
    if (detail) cb(detail);
  };
  if (typeof window !== "undefined") {
    window.addEventListener("grokhub:computer-frame", fn);
    return () => window.removeEventListener("grokhub:computer-frame", fn);
  }
  return () => {};
}

export async function computerAct(step: ComputerStep): Promise<ComputerActResult> {
  const e = electronComputer();
  if (e?.act) {
    try {
      return await e.act(step);
    } catch (err) {
      return { ok: false, error: err instanceof Error ? err.message : "computer act failed" };
    }
  }
  return {
    ok: false,
    error:
      "Computer use requires the Electron desktop shell. Relaunch GrokHub from the Arch package.",
  };
}

export function formatComputerResult(step: ComputerStep, r: ComputerActResult): string {
  const cmd = formatComputerCommand(step);
  if (!r.ok) {
    return `COMPUTER ${cmd}\nfailed: ${r.error || "error"}`;
  }
  const bits = [`COMPUTER ${cmd}`, "ok"];
  if (r.screenshot && r.screen) {
    bits.push(
      `screenshot ${r.screenshot.width}x${r.screenshot.height} · screen ${r.screen.width}x${r.screen.height}`,
      "Click coordinates are in screenshot pixel space.",
    );
  }
  if (r.mapped) {
    bits.push(`screen coords ${r.mapped.x},${r.mapped.y}${r.mapped.mapped ? " (mapped)" : ""}`);
  }
  if (r.injector) bits.push(`injector ${r.injector}`);
  return bits.join("\n");
}

export async function replayComputerRecipe(
  recipe: ComputerRecipe,
  opts?: { onStep?: (step: ComputerStep, result: ComputerActResult, index: number) => void },
): Promise<{ ok: boolean; error?: string; results: ComputerActResult[] }> {
  const results: ComputerActResult[] = [];
  await computerBeginSession();
  try {
    for (let i = 0; i < recipe.steps.length; i++) {
      const step = recipe.steps[i]!;
      const r = await computerAct(step);
      results.push(r);
      opts?.onStep?.(step, r, i);
      if (!r.ok) {
        return { ok: false, error: r.error || `step ${i + 1} failed`, results };
      }
    }
    return { ok: true, results };
  } finally {
    await computerEndSession();
  }
}

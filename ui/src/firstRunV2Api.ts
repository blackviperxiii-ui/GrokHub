/** First run v2 progress: apps, skills, local model, import, sandbox
 * note, first task and tour. Prefs live in localStorage so the shell does not
 * need a new config file; Settings → Personal can reopen it. */

export type FirstRunV2Step =
  | "apps"
  | "skills"
  | "local"
  | "import"
  | "sandbox"
  | "task"
  | "tour"
  | "done";

export type FirstRunV2Stage = "pending" | "done" | "skipped";

export interface FirstRunV2Progress {
  stage: FirstRunV2Stage;
  step: FirstRunV2Step;
}

export const DEFAULT_FIRST_RUN_V2: FirstRunV2Progress = {
  stage: "pending",
  step: "apps",
};

export const FIRST_RUN_V2_ORDER: FirstRunV2Step[] = [
  "apps",
  "skills",
  "local",
  "import",
  "sandbox",
  "tour",
  "task",
  "done",
];

export const FIRST_RUN_V2_LABELS: Record<FirstRunV2Step, string> = {
  apps: "Apps",
  skills: "Skills",
  local: "Local AI",
  import: "Import",
  sandbox: "Sandbox",
  task: "First task",
  tour: "Tour",
  done: "Done",
};

const STORAGE_KEY = "shell.firstRunV2";

/** Fired after progress is saved so Settings and the wizard stay in sync. */
export const FIRST_RUN_V2_EVENT = "shell-first-run-v2";

/** Up to 8 suggested catalog apps for the connect step (reuse MCP Connectors). */
export const SUGGESTED_APPS: { id: string; name: string; why: string }[] = [
  { id: "notion", name: "Notion", why: "Notes and docs you already use" },
  { id: "linear", name: "Linear", why: "Issues and projects" },
  { id: "github", name: "GitHub", why: "Repos, PRs and Actions" },
  { id: "gmail", name: "Gmail", why: "Mail and drafts" },
  { id: "google-calendar", name: "Google Calendar", why: "Events and invites" },
  { id: "context7", name: "Context7", why: "Up-to-date library docs" },
  { id: "playwright", name: "Playwright", why: "Drive a real browser" },
  { id: "filesystem", name: "Filesystem", why: "Read and write the chat folder" },
];

/** Starter tasks that send a first chat message without opening Settings. */
export const FIRST_TASKS: { id: string; label: string; prompt: string }[] = [
  {
    id: "tour",
    label: "Tour this app",
    prompt:
      "Walk me through this app in a few short steps. Plain words. One step at a time.",
  },
  {
    id: "can",
    label: "What you can do",
    prompt: "Tell me three things you can do on this computer for me. Short and plain.",
  },
  {
    id: "pulse",
    label: "Show Pulse",
    prompt:
      "Explain the Pulse page (For You and Feed) in two sentences, then tell me how to open it.",
  },
];

const STEPS = new Set<string>(FIRST_RUN_V2_ORDER);

function isStep(value: unknown): value is FirstRunV2Step {
  return typeof value === "string" && STEPS.has(value);
}

/** Read progress; a bad or missing value means the wizard still waits. */
export function loadFirstRunV2(): FirstRunV2Progress {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_FIRST_RUN_V2 };
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return { ...DEFAULT_FIRST_RUN_V2 };
    const obj = parsed as Record<string, unknown>;
    const stage: FirstRunV2Stage =
      obj.stage === "done" || obj.stage === "skipped" ? obj.stage : "pending";
    // A saved "layout" step is from the removed Simple/Advanced picker.
    const step = obj.step === "layout" ? "apps" : isStep(obj.step) ? obj.step : "apps";
    return { stage, step };
  } catch {
    return { ...DEFAULT_FIRST_RUN_V2 };
  }
}

/** Persist progress. Storage failures are ignored (private windows). */
export function saveFirstRunV2(progress: FirstRunV2Progress): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(progress));
  } catch {
    // Storage can be off; the in-memory progress still applies.
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent(FIRST_RUN_V2_EVENT, { detail: progress }));
  }
}

export function reopenFirstRunV2(): FirstRunV2Progress {
  const next: FirstRunV2Progress = { stage: "pending", step: "apps" };
  saveFirstRunV2(next);
  return next;
}

export function finishFirstRunV2(skipped: boolean): FirstRunV2Progress {
  const next: FirstRunV2Progress = {
    stage: skipped ? "skipped" : "done",
    step: "done",
  };
  saveFirstRunV2(next);
  return next;
}

export function nextStep(step: FirstRunV2Step): FirstRunV2Step {
  const at = FIRST_RUN_V2_ORDER.indexOf(step);
  return FIRST_RUN_V2_ORDER[Math.min(at + 1, FIRST_RUN_V2_ORDER.length - 1)] ?? "done";
}

export function prevStep(step: FirstRunV2Step): FirstRunV2Step | null {
  const at = FIRST_RUN_V2_ORDER.indexOf(step);
  if (at <= 0) return null;
  return FIRST_RUN_V2_ORDER[at - 1] ?? null;
}


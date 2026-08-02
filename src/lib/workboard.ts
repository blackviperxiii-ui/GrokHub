/**
 * Agent workboard — pin tasks for user review / approve / stage / dismiss.
 *
 * Model lines (stripped from user-visible text):
 *   WORK_PIN: title | optional detail | priority=high|normal|low
 *   WORK_UPDATE: <id|title-fragment> | status=approved|staged|in_progress|done|dismissed
 */

export type WorkItemStatus =
  | "proposed"
  | "approved"
  | "staged"
  | "in_progress"
  | "done"
  | "dismissed";

export type WorkPriority = "low" | "normal" | "high";

export type WorkItem = {
  id: string;
  title: string;
  detail: string;
  status: WorkItemStatus;
  priority: WorkPriority;
  source: "agent" | "user";
  threadId?: string | null;
  projectPath?: string | null;
  tags: string[];
  createdAt: number;
  updatedAt: number;
  /** When agent last mentioned it */
  lastTouchedAt: number;
};

export type WorkboardState = {
  version: 1;
  items: WorkItem[];
  updatedAt: number;
};

const MAX_ITEMS = 80;

export function emptyWorkboard(): WorkboardState {
  return { version: 1, items: [], updatedAt: Date.now() };
}

export function normalizeWorkboard(raw: unknown): WorkboardState {
  const empty = emptyWorkboard();
  if (!raw || typeof raw !== "object") return empty;
  const w = raw as Partial<WorkboardState>;
  if (w.version !== 1 || !Array.isArray(w.items)) return empty;
  return {
    version: 1,
    items: w.items
      .filter((i) => i && typeof i.id === "string" && i.title)
      .slice(0, MAX_ITEMS)
      .map((i) => ({
        id: i.id,
        title: String(i.title).slice(0, 160),
        detail: String(i.detail || "").slice(0, 2000),
        status: (normalizeStatus(i.status) || "proposed") as WorkItemStatus,
        priority: (["low", "normal", "high"].includes(i.priority as string)
          ? i.priority
          : "normal") as WorkPriority,
        source: i.source === "user" ? "user" : "agent",
        threadId: i.threadId || null,
        projectPath: i.projectPath || null,
        tags: Array.isArray(i.tags) ? i.tags.map(String).slice(0, 8) : [],
        createdAt: Number(i.createdAt) || Date.now(),
        updatedAt: Number(i.updatedAt) || Date.now(),
        lastTouchedAt: Number(i.lastTouchedAt) || Number(i.updatedAt) || Date.now(),
      })),
    updatedAt: Number(w.updatedAt) || Date.now(),
  };
}

function normalizeStatus(s: unknown): WorkItemStatus | null {
  const v = String(s || "").toLowerCase().replace(/-/g, "_");
  const map: Record<string, WorkItemStatus> = {
    proposed: "proposed",
    pending: "proposed",
    approved: "approved",
    approve: "approved",
    staged: "staged",
    stage: "staged",
    in_progress: "in_progress",
    progress: "in_progress",
    working: "in_progress",
    done: "done",
    complete: "done",
    completed: "done",
    dismissed: "dismissed",
    dismiss: "dismissed",
    rejected: "dismissed",
  };
  return map[v] || null;
}

function uid(): string {
  return `work-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
}

export function workboardCounts(state: WorkboardState): Record<WorkItemStatus, number> {
  const c: Record<WorkItemStatus, number> = {
    proposed: 0,
    approved: 0,
    staged: 0,
    in_progress: 0,
    done: 0,
    dismissed: 0,
  };
  for (const i of state.items) c[i.status] = (c[i.status] || 0) + 1;
  return c;
}

export function pinWorkItem(
  state: WorkboardState,
  input: {
    title: string;
    detail?: string;
    priority?: WorkPriority;
    source?: "agent" | "user";
    threadId?: string | null;
    projectPath?: string | null;
    tags?: string[];
  },
): { state: WorkboardState; item: WorkItem } {
  const title = input.title.trim().slice(0, 160);
  // Dedupe by similar title among open items
  const open = state.items.filter(
    (i) => !["done", "dismissed"].includes(i.status),
  );
  const existing = open.find(
    (i) => i.title.toLowerCase() === title.toLowerCase(),
  );
  const now = Date.now();
  if (existing) {
    const item: WorkItem = {
      ...existing,
      detail: (input.detail || existing.detail || "").slice(0, 2000),
      priority: input.priority || existing.priority,
      lastTouchedAt: now,
      updatedAt: now,
      threadId: input.threadId ?? existing.threadId,
      projectPath: input.projectPath ?? existing.projectPath,
    };
    return {
      state: {
        version: 1,
        items: state.items.map((i) => (i.id === item.id ? item : i)),
        updatedAt: now,
      },
      item,
    };
  }
  const item: WorkItem = {
    id: uid(),
    title,
    detail: (input.detail || "").slice(0, 2000),
    status: "proposed",
    priority: input.priority || "normal",
    source: input.source || "agent",
    threadId: input.threadId || null,
    projectPath: input.projectPath || null,
    tags: input.tags || [],
    createdAt: now,
    updatedAt: now,
    lastTouchedAt: now,
  };
  return {
    state: {
      version: 1,
      items: [item, ...state.items].slice(0, MAX_ITEMS),
      updatedAt: now,
    },
    item,
  };
}

export function setWorkItemStatus(
  state: WorkboardState,
  idOrTitle: string,
  status: WorkItemStatus,
): WorkboardState {
  const q = idOrTitle.trim().toLowerCase();
  const now = Date.now();
  // Prefer exact id, then exact title, then a single unique title substring — never bulk-update all partial matches
  const byId = state.items.find((i) => i.id === idOrTitle || i.id.toLowerCase() === q);
  const byExactTitle = state.items.find((i) => i.title.toLowerCase() === q);
  const bySub = state.items.filter((i) => i.title.toLowerCase().includes(q));
  const target =
    byId ||
    byExactTitle ||
    (bySub.length === 1 ? bySub[0] : null);
  if (!target) return state;
  return {
    version: 1,
    items: state.items.map((i) =>
      i.id === target.id
        ? { ...i, status, updatedAt: now, lastTouchedAt: now }
        : i,
    ),
    updatedAt: now,
  };
}

export function updateWorkItem(
  state: WorkboardState,
  id: string,
  patch: Partial<Pick<WorkItem, "title" | "detail" | "priority" | "status" | "tags">>,
): WorkboardState {
  const now = Date.now();
  return {
    version: 1,
    items: state.items.map((i) =>
      i.id === id
        ? {
            ...i,
            ...patch,
            title: patch.title != null ? String(patch.title).slice(0, 160) : i.title,
            detail: patch.detail != null ? String(patch.detail).slice(0, 2000) : i.detail,
            updatedAt: now,
            lastTouchedAt: now,
          }
        : i,
    ),
    updatedAt: now,
  };
}

export function removeWorkItem(state: WorkboardState, id: string): WorkboardState {
  return {
    version: 1,
    items: state.items.filter((i) => i.id !== id),
    updatedAt: Date.now(),
  };
}

/** Parse + strip WORK_* lines from model output */
export function extractWorkCommands(text: string): {
  pins: Array<{ title: string; detail: string; priority: WorkPriority }>;
  updates: Array<{ ref: string; status: WorkItemStatus }>;
  cleaned: string;
} {
  const pins: Array<{ title: string; detail: string; priority: WorkPriority }> = [];
  const updates: Array<{ ref: string; status: WorkItemStatus }> = [];
  const lines = String(text || "").split("\n");
  const kept: string[] = [];

  for (const line of lines) {
    const pin = line.match(/^\s*WORK_PIN:\s*(.+)$/i);
    if (pin) {
      const parts = pin[1]!.split("|").map((s) => s.trim());
      let title = parts[0] || "Untitled task";
      let detail = "";
      let priority: WorkPriority = "normal";
      for (let i = 1; i < parts.length; i++) {
        const p = parts[i]!;
        const pr = p.match(/^priority\s*=\s*(low|normal|high)$/i);
        if (pr) {
          priority = pr[1]!.toLowerCase() as WorkPriority;
          continue;
        }
        if (!detail) detail = p;
        else detail += " | " + p;
      }
      pins.push({ title, detail, priority });
      continue;
    }
    const up = line.match(/^\s*WORK_UPDATE:\s*(.+)$/i);
    if (up) {
      const parts = up[1]!.split("|").map((s) => s.trim());
      const ref = parts[0] || "";
      let status: WorkItemStatus = "in_progress";
      for (let i = 1; i < parts.length; i++) {
        const st = parts[i]!.match(/^status\s*=\s*(\w+)$/i);
        if (st) {
          status = normalizeStatus(st[1]) || "in_progress";
        } else {
          const n = normalizeStatus(parts[i]);
          if (n) status = n;
        }
      }
      if (ref) updates.push({ ref, status });
      continue;
    }
    kept.push(line);
  }

  return {
    pins,
    updates,
    cleaned: kept.join("\n").replace(/\n{3,}/g, "\n\n").trim(),
  };
}

export function workboardContextBlock(state: WorkboardState, max = 12): string {
  const open = state.items.filter((i) =>
    ["proposed", "approved", "staged", "in_progress"].includes(i.status),
  );
  if (!open.length) return "";
  const lines = [
    "## Active workboard (user-visible task board)",
    "Pin tasks with WORK_PIN: title | detail | priority=high",
    "Update with WORK_UPDATE: id-or-title | status=approved|staged|in_progress|done|dismissed",
    "Do not invent completion — only mark done when work is finished.",
    "",
  ];
  for (const i of open.slice(0, max)) {
    lines.push(
      `- [${i.status}] (${i.priority}) ${i.title} · id=${i.id}${i.detail ? ` — ${i.detail.slice(0, 100)}` : ""}`,
    );
  }
  return lines.join("\n");
}

export const WORK_STATUS_LABEL: Record<WorkItemStatus, string> = {
  proposed: "Proposed",
  approved: "Approved",
  staged: "Staged",
  in_progress: "In progress",
  done: "Done",
  dismissed: "Dismissed",
};

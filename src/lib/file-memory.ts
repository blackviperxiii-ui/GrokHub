/**
 * Client for M1 file memory (Electron userData/memory/*).
 * Browser preview: in-memory fallback so UI still works.
 */

export type MemoryFileInfo = {
  id: string;
  name: string;
  kind: "core" | "daily";
  bytes: number;
  updatedAt: number;
};

type PinBundle = {
  ok: boolean;
  bundle: string;
  chars: number;
  root?: string;
  hasUser?: boolean;
  hasMemory?: boolean;
  hasToday?: boolean;
};

const browserFiles = new Map<string, string>();

function desktop() {
  return typeof window !== "undefined" ? window.grokhubDesktop?.memory : undefined;
}

function browserEnsure() {
  if (!browserFiles.has("MEMORY.md")) {
    browserFiles.set(
      "MEMORY.md",
      "# Long-term memory\n\nDurable facts and decisions.\n",
    );
  }
  if (!browserFiles.has("USER.md")) {
    browserFiles.set("USER.md", "# User profile\n\n");
  }
}

export async function memoryFsInfo(): Promise<{
  ok: boolean;
  root?: string;
  userData?: string;
  files?: MemoryFileInfo[];
  bytes?: number;
  today?: string;
}> {
  const m = desktop();
  if (m?.info) {
    try {
      const r = await m.info();
      return {
        ...r,
        files: (r.files || []).map((f) => ({
          ...f,
          kind: f.kind === "daily" ? ("daily" as const) : ("core" as const),
        })),
      };
    } catch {
      return { ok: false };
    }
  }
  browserEnsure();
  return {
    ok: true,
    root: "browser-memory",
    files: [...browserFiles.keys()].map((id) => ({
      id,
      name: id,
      kind: id.includes("daily") ? ("daily" as const) : ("core" as const),
      bytes: (browserFiles.get(id) || "").length,
      updatedAt: Date.now(),
    })),
    bytes: [...browserFiles.values()].reduce((a, s) => a + s.length, 0),
    today: new Date().toISOString().slice(0, 10),
  };
}

export async function memoryList(): Promise<MemoryFileInfo[]> {
  const m = desktop();
  if (m?.list) {
    const r = await m.list();
    return (r?.files || []) as MemoryFileInfo[];
  }
  const info = await memoryFsInfo();
  return info.files || [];
}

export async function memoryRead(
  rel: string,
): Promise<{ ok: boolean; content?: string; id?: string; error?: string }> {
  const m = desktop();
  if (m?.read) return m.read(rel);
  browserEnsure();
  const key =
    !rel || rel === "today"
      ? `daily/${new Date().toISOString().slice(0, 10)}.md`
      : rel;
  return { ok: true, id: key, content: browserFiles.get(key) || "" };
}

export async function memoryWrite(
  rel: string,
  content: string,
): Promise<{ ok: boolean; error?: string }> {
  const m = desktop();
  if (m?.write) return m.write(rel, content);
  browserEnsure();
  const key =
    !rel || rel === "today"
      ? `daily/${new Date().toISOString().slice(0, 10)}.md`
      : rel;
  browserFiles.set(key, content);
  return { ok: true };
}

export async function memoryAppend(
  rel: string,
  text: string,
): Promise<{ ok: boolean; error?: string; appended?: string }> {
  const m = desktop();
  if (m?.append) return m.append(rel, text);
  browserEnsure();
  const key =
    rel === "memory"
      ? "MEMORY.md"
      : rel === "user"
        ? "USER.md"
        : !rel || rel === "today"
          ? `daily/${new Date().toISOString().slice(0, 10)}.md`
          : rel;
  const prev = browserFiles.get(key) || "";
  const stamp = new Date().toISOString().slice(0, 16).replace("T", " ");
  browserFiles.set(key, prev.trimEnd() + `\n- ${stamp}: ${text}\n`);
  return { ok: true, appended: text };
}

export async function memoryAppendFacts(
  facts: string[],
  opts?: { target?: string },
): Promise<{ ok: boolean; added?: number }> {
  const m = desktop();
  if (m?.appendFacts) return m.appendFacts(facts, opts);
  let added = 0;
  for (const f of facts) {
    const r = await memoryAppend(opts?.target === "today" ? "today" : "memory", f);
    if (r.ok) added += 1;
  }
  return { ok: true, added };
}

/** Budgeted pin text for context builder (async — disk read). */
export async function loadMemoryPinBundle(): Promise<PinBundle> {
  const m = desktop();
  if (m?.pinBundle) {
    try {
      const r = await m.pinBundle({});
      if (r?.ok) return r as PinBundle;
    } catch {
      /* fall through */
    }
  }
  browserEnsure();
  const user = browserFiles.get("USER.md") || "";
  const mem = browserFiles.get("MEMORY.md") || "";
  const parts = [];
  if (user.trim()) parts.push(`## USER.md\n${user.trim().slice(0, 3000)}`);
  if (mem.trim()) parts.push(`## MEMORY.md\n${mem.trim().slice(0, 6000)}`);
  const bundle = parts.join("\n\n");
  return {
    ok: true,
    bundle,
    chars: bundle.length,
    hasUser: Boolean(user.trim()),
    hasMemory: Boolean(mem.trim()),
  };
}

/** Migrate legacy agentPrefs.memoryNotes into MEMORY.md once. */
export async function migrateNotesToFileMemory(notes: string): Promise<boolean> {
  const text = String(notes || "").trim();
  if (!text) return false;
  const cur = await memoryRead("MEMORY.md");
  const body = cur.content || "";
  if (body.includes("Migrated from app memory notes")) return false;
  if (body.includes(text.slice(0, 40))) return false;
  const merged =
    body.trimEnd() +
    "\n\n## Migrated from app memory notes\n\n" +
    text +
    "\n";
  const r = await memoryWrite("MEMORY.md", merged);
  return Boolean(r.ok);
}


/** Push learning STATUS.md + LEARNINGS.md to disk (host-scannable). */
export async function syncLearningToDisk(payload: {
  statusMarkdown?: string;
  learningsMarkdown?: string;
}): Promise<{ ok: boolean; root?: string }> {
  const m = desktop();
  if (m?.syncLearning) {
    try {
      return await m.syncLearning(payload);
    } catch {
      return { ok: false };
    }
  }
  // browser: keep in memory map
  if (payload.learningsMarkdown) browserFiles.set("LEARNINGS.md", payload.learningsMarkdown);
  if (payload.statusMarkdown) browserFiles.set("STATUS.md", payload.statusMarkdown);
  return { ok: true, root: "browser-memory" };
}

export async function ensureFileMemory(): Promise<{ ok: boolean; root?: string }> {
  const m = desktop();
  if (m?.ensure) {
    try {
      return await m.ensure();
    } catch {
      return { ok: false };
    }
  }
  browserEnsure();
  return { ok: true, root: "browser-memory" };
}

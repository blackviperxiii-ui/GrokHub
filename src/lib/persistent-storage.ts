/**
 * Zustand storage that prefers Electron userData (survives updates),
 * and mirrors to localStorage for browser preview.
 * Writes are debounced to avoid thrashing disk on every keystroke/heartbeat.
 * While streaming, writes are paused and flushed when the turn ends.
 */
import type { StateStorage } from "zustand/middleware";

const MIRROR_PREFIX = "grokhub.persist.";
const DEBOUNCE_MS = 400;
const pending = new Map<string, { value: string; timer: ReturnType<typeof setTimeout> }>();
const lastWritten = new Map<string, string>();
/** When true, setItem only parks values; flush on resume */
let persistPaused = false;
const parked = new Map<string, string>();

function electronState() {
  return typeof window !== "undefined" ? window.grokhubDesktop?.state : undefined;
}

/** Pause disk writes during streaming; call false at end of turn to flush. */
export function setPersistPaused(paused: boolean): void {
  persistPaused = paused;
  if (!paused && parked.size) {
    const entries = [...parked.entries()];
    parked.clear();
    for (const [name, value] of entries) {
      void persistentStorage.setItem(name, value);
    }
  }
}

export function isPersistPaused(): boolean {
  return persistPaused;
}

async function flushWrite(name: string, value: string): Promise<void> {
  if (lastWritten.get(name) === value) return;
  lastWritten.set(name, value);
  try {
    localStorage.setItem(MIRROR_PREFIX + name, value);
    localStorage.setItem(name, value);
  } catch {
    /* quota — electron path still saves */
  }
  const e = electronState();
  if (e?.set) {
    try {
      await e.set(name, value);
    } catch {
      /* ignore */
    }
  }
}

/** Force-flush all pending debounced writes (call on beforeunload). */
export async function flushPersistentStorage(): Promise<void> {
  if (persistPaused) {
    // Promote parked into pending so we flush latest
    for (const [name, value] of parked) {
      const prev = pending.get(name);
      if (prev) clearTimeout(prev.timer);
      pending.set(name, { value, timer: setTimeout(() => {}, 0) });
    }
    parked.clear();
  }
  const jobs: Promise<void>[] = [];
  for (const [name, row] of pending) {
    clearTimeout(row.timer);
    pending.delete(name);
    jobs.push(flushWrite(name, row.value));
  }
  await Promise.all(jobs);
}

export const persistentStorage: StateStorage = {
  getItem: async (name: string): Promise<string | null> => {
    const e = electronState();
    if (e?.get) {
      try {
        const r = await e.get(name);
        if (r?.value != null && r.value !== "") {
          try {
            localStorage.setItem(MIRROR_PREFIX + name, r.value);
          } catch {
            /* quota */
          }
          lastWritten.set(name, r.value);
          return r.value;
        }
      } catch {
        /* fall through */
      }
    }
    try {
      const direct = localStorage.getItem(MIRROR_PREFIX + name) || localStorage.getItem(name);
      if (direct) {
        lastWritten.set(name, direct);
        return direct;
      }
      if (name === "grokhub-memory-v1") {
        for (const legacy of ["grokhub-clean-v4", "grokhub-clean-v3", "grokhub-clean-v2"]) {
          const old = localStorage.getItem(legacy);
          if (old) {
            try {
              localStorage.setItem(MIRROR_PREFIX + name, old);
              if (e?.set) await e.set(name, old);
            } catch {
              /* ignore */
            }
            lastWritten.set(name, old);
            return old;
          }
        }
      }
      return null;
    } catch {
      return null;
    }
  },

  setItem: async (name: string, value: string): Promise<void> => {
    if (lastWritten.get(name) === value) return;
    if (persistPaused) {
      parked.set(name, value);
      return;
    }
    const prev = pending.get(name);
    if (prev) clearTimeout(prev.timer);
    const timer = setTimeout(() => {
      pending.delete(name);
      void flushWrite(name, value);
    }, DEBOUNCE_MS);
    pending.set(name, { value, timer });
  },

  removeItem: async (name: string): Promise<void> => {
    const prev = pending.get(name);
    if (prev) {
      clearTimeout(prev.timer);
      pending.delete(name);
    }
    parked.delete(name);
    lastWritten.delete(name);
    try {
      localStorage.removeItem(MIRROR_PREFIX + name);
      localStorage.removeItem(name);
    } catch {
      /* ignore */
    }
    const e = electronState();
    if (e?.remove) {
      try {
        await e.remove(name);
      } catch {
        /* ignore */
      }
    }
  },
};

export async function memoryInfo(): Promise<{
  path?: string;
  userData?: string;
  bytes?: number;
  updatedAt?: number;
  keys?: string[];
} | null> {
  const e = electronState();
  if (e?.info) {
    try {
      return await e.info();
    } catch {
      return null;
    }
  }
  return { path: "browser localStorage", userData: "browser" };
}

export async function exportMemory(): Promise<{ ok: boolean; json?: string; error?: string }> {
  await flushPersistentStorage();
  const e = electronState();
  if (e?.exportAll) {
    try {
      const r = await e.exportAll();
      return { ok: true, json: JSON.stringify(r, null, 2) };
    } catch (err) {
      return { ok: false, error: err instanceof Error ? err.message : "export failed" };
    }
  }
  try {
    const keys: Record<string, string> = {};
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (!k) continue;
      if (k.startsWith("grokhub") || k.startsWith(MIRROR_PREFIX)) {
        keys[k] = localStorage.getItem(k) || "";
      }
    }
    return {
      ok: true,
      json: JSON.stringify({ exportedAt: Date.now(), keys }, null, 2),
    };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : "export failed" };
  }
}

export async function importMemory(json: string): Promise<{ ok: boolean; error?: string }> {
  const e = electronState();
  if (e?.importAll) {
    try {
      return await e.importAll(json);
    } catch (err) {
      return { ok: false, error: err instanceof Error ? err.message : "import failed" };
    }
  }
  try {
    const body = JSON.parse(json);
    const keys = body?.data?.keys || body?.keys || {};
    for (const [k, v] of Object.entries(keys)) {
      localStorage.setItem(String(k), String(v));
      localStorage.setItem(MIRROR_PREFIX + k, String(v));
    }
    return { ok: true };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : "import failed" };
  }
}

/**
 * Zustand storage that prefers Electron userData (survives updates),
 * and mirrors to localStorage for browser preview.
 */
import type { StateStorage } from "zustand/middleware";

const MIRROR_PREFIX = "grokhub.persist.";

function electronState() {
  return typeof window !== "undefined" ? window.grokhubDesktop?.state : undefined;
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
          return r.value;
        }
      } catch {
        /* fall through */
      }
    }
    try {
      const direct = localStorage.getItem(MIRROR_PREFIX + name) || localStorage.getItem(name);
      if (direct) return direct;
      // One-time migrate from older store keys
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
    try {
      localStorage.setItem(MIRROR_PREFIX + name, value);
      // also write legacy key for older builds
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
  },

  removeItem: async (name: string): Promise<void> => {
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
  const e = electronState();
  if (e?.exportAll) {
    try {
      const r = await e.exportAll();
      return { ok: true, json: JSON.stringify(r, null, 2) };
    } catch (err) {
      return { ok: false, error: err instanceof Error ? err.message : "export failed" };
    }
  }
  // browser: dump localStorage keys
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

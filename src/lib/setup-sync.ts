/**
 * Setup sync tied to Grok OAuth identity.
 *
 * What "sync" means here:
 * 1) **Grok account pull** — profile, models, website connectors (needs OAuth and/or website session).
 * 2) **App setup pack** — desktop prefs, automations, skills, agent personas, mode preference,
 *    openclaw workspace meta — NOT secrets (tokens, API keys, SSO).
 * 3) **Cross-device** — push/pull pack via GitHub Gist when a GitHub token is linked,
 *    keyed by OAuth email/sub so the same Grok sign-in restores setup on another machine.
 */

import type { Agent, Automation, Connector, Skill } from "./types";
import { APP_VERSION } from "./version";

export const SETUP_SYNC_KIND = "grokhub-setup-v1";
export const GIST_DESCRIPTION = "GrokHub setup sync (do not delete) — auto-managed";

export type SetupPack = {
  kind: typeof SETUP_SYNC_KIND;
  version: number;
  appVersion: string;
  /** OAuth identity this pack belongs to */
  account: {
    email?: string;
    name?: string;
    sub?: string;
  };
  exportedAt: number;
  /** App configuration (no secrets) */
  setup: {
    mode?: string;
    desktop?: Record<string, unknown>;
    agents?: Agent[];
    skills?: Skill[];
    automations?: Automation[];
    /** Connector rows without secrets — status/tools/labels only */
    connectors?: Array<
      Pick<Connector, "id" | "name" | "category" | "description" | "status" | "tools" | "accountLabel">
    >;
    openClawWorkspace?: {
      path?: string;
      importedAt?: number;
      skillCount?: number;
    } | null;
  };
};

export type SetupSyncMeta = {
  lastPushAt?: number;
  lastPullAt?: number;
  lastGistId?: string;
  lastAccount?: string;
  lastDetail?: string;
  autoPullOnLogin?: boolean;
  autoPushOnChange?: boolean;
};

export function accountKey(oauth: {
  email?: string;
  name?: string;
  idToken?: string;
  accessToken?: string;
}): string {
  if (oauth.email) return oauth.email.toLowerCase().trim();
  if (oauth.idToken) {
    try {
      const part = oauth.idToken.split(".")[1];
      if (part) {
        const json = JSON.parse(atob(part.replace(/-/g, "+").replace(/_/g, "/")));
        if (json.sub) return String(json.sub);
        if (json.email) return String(json.email).toLowerCase();
      }
    } catch {
      /* ignore */
    }
  }
  if (oauth.name) return `name:${oauth.name.toLowerCase()}`;
  return "anonymous";
}

export function buildSetupPack(input: {
  oauth: { email?: string; name?: string; idToken?: string } | null;
  mode: string;
  desktop: Record<string, unknown>;
  agents: Agent[];
  skills: Skill[];
  automations: Automation[];
  connectors: Connector[];
  openClawWorkspace?: { path?: string; importedAt?: number; files?: unknown[] } | null;
}): SetupPack {
  const acct = input.oauth || {};
  return {
    kind: SETUP_SYNC_KIND,
    version: 1,
    appVersion: APP_VERSION,
    account: {
      email: acct.email,
      name: acct.name,
      sub: accountKey(acct),
    },
    exportedAt: Date.now(),
    setup: {
      mode: input.mode,
      desktop: {
        wayland: input.desktop.wayland,
        tray: input.desktop.tray,
        launchOnLogin: input.desktop.launchOnLogin,
        startMinimized: input.desktop.startMinimized,
        confirmHostCommands: input.desktop.confirmHostCommands,
        confirmDestructiveOnly: input.desktop.confirmDestructiveOnly,
        selfModifyEnabled: input.desktop.selfModifyEnabled,
        unsandboxed: input.desktop.unsandboxed,
        autoStartBridge: input.desktop.autoStartBridge,
      },
      agents: input.agents.map((a) => ({ ...a })),
      skills: input.skills.map((s) => ({ ...s })),
      automations: input.automations.map((a) => ({
        ...a,
        // clear runtime schedule stamps so next machine recomputes
        lastRun: undefined,
        nextRun: undefined,
        runCount: 0,
      })),
      connectors: input.connectors.map((c) => ({
        id: c.id,
        name: c.name,
        category: c.category,
        description: c.description,
        status: c.status === "connected" ? "connected" : "disconnected",
        tools: c.tools,
        accountLabel: c.accountLabel,
      })),
      openClawWorkspace: input.openClawWorkspace
        ? {
            path: input.openClawWorkspace.path,
            importedAt: input.openClawWorkspace.importedAt,
            skillCount: Array.isArray(input.openClawWorkspace.files)
              ? input.openClawWorkspace.files.length
              : undefined,
          }
        : null,
    },
  };
}

export function parseSetupPack(raw: unknown): SetupPack | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Partial<SetupPack>;
  if (o.kind !== SETUP_SYNC_KIND || !o.setup || typeof o.setup !== "object") return null;
  return o as SetupPack;
}

export type ApplySetupResult = {
  ok: boolean;
  detail: string;
  applied: string[];
};

/** Merge pack into store-shaped patch pieces (caller applies). */
export function mergeSetupPack(
  pack: SetupPack,
  current: {
    agents: Agent[];
    skills: Skill[];
    automations: Automation[];
    connectors: Connector[];
    mode: string;
    desktop: Record<string, unknown>;
  },
  opts?: { replaceAutomations?: boolean; replaceSkills?: boolean },
): {
  mode?: string;
  desktop?: Record<string, unknown>;
  agents?: Agent[];
  skills?: Skill[];
  automations?: Automation[];
  connectors?: Connector[];
  applied: string[];
} {
  const applied: string[] = [];
  const s = pack.setup;
  const out: ReturnType<typeof mergeSetupPack> = { applied };

  if (s.mode) {
    out.mode = s.mode;
    applied.push("mode");
  }
  if (s.desktop && typeof s.desktop === "object") {
    out.desktop = { ...current.desktop, ...s.desktop };
    applied.push("desktop prefs");
  }
  if (s.agents?.length) {
    out.agents = s.agents;
    applied.push(`${s.agents.length} agents`);
  }
  if (s.skills?.length) {
    if (opts?.replaceSkills) {
      out.skills = s.skills;
    } else {
      const byId = new Map(current.skills.map((x) => [x.id, x]));
      for (const sk of s.skills) byId.set(sk.id, sk);
      out.skills = [...byId.values()];
    }
    applied.push(`${s.skills.length} skills`);
  }
  if (s.automations?.length) {
    if (opts?.replaceAutomations) {
      out.automations = s.automations;
    } else {
      const byId = new Map(current.automations.map((x) => [x.id, x]));
      for (const a of s.automations) byId.set(a.id, a);
      out.automations = [...byId.values()];
    }
    applied.push(`${s.automations.length} automations`);
  }
  if (s.connectors?.length) {
    const byId = new Map(current.connectors.map((c) => [c.id, c]));
    for (const c of s.connectors) {
      const prev = byId.get(c.id);
      byId.set(c.id, {
        ...(prev || {
          id: c.id,
          name: c.name,
          category: c.category || "Synced",
          description: c.description || "",
          status: "disconnected",
          tools: c.tools || [],
        }),
        name: c.name || prev?.name || c.id,
        category: c.category || prev?.category || "Synced",
        description: c.description || prev?.description || "",
        tools: c.tools?.length ? c.tools : prev?.tools || [],
        accountLabel: c.accountLabel ?? prev?.accountLabel,
        // keep local connected if already connected; otherwise use pack
        status:
          prev?.status === "connected"
            ? "connected"
            : c.status === "connected"
              ? "connected"
              : prev?.status || "disconnected",
      });
    }
    out.connectors = [...byId.values()];
    applied.push(`${s.connectors.length} connectors`);
  }
  return out;
}

// —— GitHub Gist backend (optional, uses user's GitHub token) ——

async function ghFetch(
  path: string,
  token: string,
  init?: RequestInit,
): Promise<Response> {
  return fetch(`https://api.github.com${path}`, {
    ...init,
    headers: {
      accept: "application/vnd.github+json",
      "user-agent": "GrokHub-SetupSync",
      authorization: `Bearer ${token}`,
      "content-type": "application/json",
      ...(init?.headers || {}),
    },
  });
}

export async function findSetupGist(
  token: string,
  account: string,
): Promise<{ id: string; updatedAt?: string } | null> {
  const res = await ghFetch("/gists?per_page=100", token);
  if (!res.ok) return null;
  const list = (await res.json()) as Array<{
    id: string;
    description?: string;
    updated_at?: string;
    files?: Record<string, { filename?: string; content?: string }>;
  }>;
  const needle = `${GIST_DESCRIPTION} · ${account}`;
  const hit =
    list.find((g) => (g.description || "").includes(needle)) ||
    list.find(
      (g) =>
        (g.description || "").includes(GIST_DESCRIPTION) &&
        Object.keys(g.files || {}).some((f) => f.includes("grokhub-setup")),
    );
  return hit ? { id: hit.id, updatedAt: hit.updated_at } : null;
}

export async function pushSetupToGist(
  token: string,
  pack: SetupPack,
  gistId?: string,
  /** If set, file content is this string instead of JSON.stringify(pack) — used for encrypted blobs */
  rawContent?: string,
): Promise<{ ok: boolean; gistId?: string; error?: string; htmlUrl?: string }> {
  const account = pack.account.sub || pack.account.email || "account";
  const filename = `grokhub-setup-${account.replace(/[^a-z0-9._-]+/gi, "_").slice(0, 48)}.json`;
  const content = rawContent || JSON.stringify(pack, null, 2);
  const body = {
    description: `${GIST_DESCRIPTION} · ${account}`,
    public: false,
    files: {
      [filename]: { content },
    },
  };
  try {
    let res: Response;
    if (gistId) {
      res = await ghFetch(`/gists/${gistId}`, token, {
        method: "PATCH",
        body: JSON.stringify(body),
      });
    } else {
      const existing = await findSetupGist(token, account);
      if (existing?.id) {
        res = await ghFetch(`/gists/${existing.id}`, token, {
          method: "PATCH",
          body: JSON.stringify(body),
        });
      } else {
        res = await ghFetch("/gists", token, {
          method: "POST",
          body: JSON.stringify(body),
        });
      }
    }
    if (!res.ok) {
      const t = await res.text();
      return { ok: false, error: `GitHub ${res.status}: ${t.slice(0, 200)}` };
    }
    const data = (await res.json()) as { id?: string; html_url?: string };
    return { ok: true, gistId: data.id, htmlUrl: data.html_url };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "gist push failed" };
  }
}

export async function pullSetupFromGist(
  token: string,
  account: string,
  gistId?: string,
): Promise<{
  ok: boolean;
  pack?: SetupPack;
  raw?: string;
  gistId?: string;
  error?: string;
}> {
  try {
    let id = gistId;
    if (!id) {
      const found = await findSetupGist(token, account);
      id = found?.id;
    }
    if (!id) {
      return {
        ok: false,
        error:
          "No setup gist found for this Grok account yet — push from another device first",
      };
    }
    const res = await ghFetch(`/gists/${id}`, token);
    if (!res.ok) return { ok: false, error: `GitHub ${res.status}` };
    const data = (await res.json()) as {
      id: string;
      files?: Record<string, { content?: string; filename?: string }>;
    };
    const files = Object.values(data.files || {});
    const file =
      files.find((f) => (f.filename || "").includes("grokhub-setup")) || files[0];
    if (!file?.content) return { ok: false, error: "Gist has no setup file" };
    const raw = file.content;
    try {
      const parsed = JSON.parse(raw);
      const pack = parseSetupPack(parsed);
      if (pack) return { ok: true, pack, raw, gistId: data.id };
      // encrypted or other
      return { ok: true, raw, gistId: data.id };
    } catch {
      return { ok: false, error: "Invalid setup file in gist" };
    }
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "gist pull failed" };
  }
}

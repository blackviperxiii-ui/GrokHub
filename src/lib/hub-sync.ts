/**
 * Hub snapshot build / merge. No secrets. Last-write-wins per record.
 */

export const HUB_SNAPSHOT_KIND = "grokhub-hub-v1";

export type HubMemoryFile = { name: string; content: string; updatedAt: number };

export type HubSnapshot = {
  kind: typeof HUB_SNAPSHOT_KIND;
  fromDeviceId: string;
  fromDeviceName: string;
  exportedAt: number;
  threads: unknown[];
  workboard: unknown;
  skills: unknown[];
  automations: unknown[];
  learning: unknown;
  memoryFiles: HubMemoryFile[];
  profile: { displayName?: string | null };
};

type IdRow = { id?: string; updatedAt?: number; createdAt?: number };

function newer(a: IdRow, b: IdRow): IdRow {
  const ta = Number(a.updatedAt || a.createdAt || 0);
  const tb = Number(b.updatedAt || b.createdAt || 0);
  return ta >= tb ? a : b;
}

function mergeById(local: unknown[], remote: unknown[]): unknown[] {
  const map = new Map<string, IdRow>();
  for (const row of [...(local || []), ...(remote || [])] as IdRow[]) {
    if (!row || typeof row !== "object" || !row.id) continue;
    const prev = map.get(row.id);
    map.set(row.id, prev ? (newer(prev, row) as IdRow) : row);
  }
  return Array.from(map.values());
}

export function buildHubSnapshot(input: {
  deviceId: string;
  deviceName: string;
  threads: unknown[];
  workboard: unknown;
  skills: unknown[];
  automations: unknown[];
  learning: unknown;
  memoryFiles: HubMemoryFile[];
  displayName?: string | null;
}): HubSnapshot {
  const threads = (input.threads || []).slice(0, 40).map((t) => {
    const th = t as { messages?: unknown[] };
    return {
      ...(t as object),
      messages: Array.isArray(th.messages) ? th.messages.slice(-80) : [],
    };
  });
  return {
    kind: HUB_SNAPSHOT_KIND,
    fromDeviceId: input.deviceId,
    fromDeviceName: input.deviceName,
    exportedAt: Date.now(),
    threads,
    workboard: input.workboard ?? null,
    skills: (input.skills || []).slice(0, 80),
    automations: (input.automations || []).slice(0, 80),
    learning: input.learning ?? null,
    memoryFiles: (input.memoryFiles || []).map((f) => ({
      name: String(f.name || "").slice(0, 80),
      content: String(f.content || "").slice(0, 200_000),
      updatedAt: Number(f.updatedAt || Date.now()),
    })),
    profile: { displayName: input.displayName ?? null },
  };
}

export function mergeHubSnapshots(local: HubSnapshot, remote: HubSnapshot): HubSnapshot {
  const files = new Map<string, HubMemoryFile>();
  for (const f of [...(local.memoryFiles || []), ...(remote.memoryFiles || [])]) {
    if (!f?.name) continue;
    const prev = files.get(f.name);
    if (!prev || Number(f.updatedAt) >= Number(prev.updatedAt)) files.set(f.name, f);
  }
  const localWb = (local.workboard || {}) as { items?: unknown[] };
  const remoteWb = (remote.workboard || {}) as { items?: unknown[] };
  return {
    kind: HUB_SNAPSHOT_KIND,
    fromDeviceId: local.fromDeviceId,
    fromDeviceName: local.fromDeviceName,
    exportedAt: Math.max(local.exportedAt || 0, remote.exportedAt || 0),
    threads: mergeById(local.threads as unknown[], remote.threads as unknown[]),
    workboard: {
      ...(typeof remoteWb === "object" ? remoteWb : {}),
      ...(typeof localWb === "object" ? localWb : {}),
      items: mergeById(localWb.items || [], remoteWb.items || []),
    },
    skills: mergeById(local.skills as unknown[], remote.skills as unknown[]),
    automations: mergeById(local.automations as unknown[], remote.automations as unknown[]),
    learning: (remote.exportedAt || 0) >= (local.exportedAt || 0) ? remote.learning : local.learning,
    memoryFiles: Array.from(files.values()),
    profile: {
      displayName: local.profile?.displayName || remote.profile?.displayName || null,
    },
  };
}

export function isHubSnapshot(v: unknown): v is HubSnapshot {
  return Boolean(v && typeof v === "object" && (v as HubSnapshot).kind === HUB_SNAPSHOT_KIND);
}

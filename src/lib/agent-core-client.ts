/**
 * Renderer bridge to Electron agent-core (durable queue mirror + notifications).
 */

export type AgentCoreSnapshot = {
  ok: boolean;
  paused?: boolean;
  jobs?: unknown[];
  due?: unknown[];
  detail?: string;
};

function desktop(): {
  agent?: {
    snapshot: () => Promise<AgentCoreSnapshot>;
    enqueue: (job: unknown) => Promise<AgentCoreSnapshot>;
    setPaused: (v: boolean) => Promise<AgentCoreSnapshot>;
    approve: (id: string, grant: boolean) => Promise<AgentCoreSnapshot>;
    sync: (payload: unknown) => Promise<AgentCoreSnapshot>;
    onTick?: (cb: (payload: unknown) => void) => () => void;
  };
} | null {
  if (typeof window === "undefined") return null;
  return (window as unknown as { grokhubDesktop?: ReturnType<typeof desktop> }).grokhubDesktop || null;
}

export function isAgentCoreAvailable(): boolean {
  return Boolean(desktop()?.agent?.snapshot);
}

export async function agentCoreSnapshot(): Promise<AgentCoreSnapshot> {
  const a = desktop()?.agent;
  if (!a) return { ok: false, detail: "not desktop" };
  try {
    return await a.snapshot();
  } catch (e) {
    return { ok: false, detail: e instanceof Error ? e.message : "snapshot failed" };
  }
}

export async function agentCoreEnqueue(job: unknown): Promise<void> {
  const a = desktop()?.agent;
  if (!a) return;
  try {
    await a.enqueue(job);
  } catch {
    /* ignore */
  }
}

export async function agentCoreSetPaused(v: boolean): Promise<void> {
  const a = desktop()?.agent;
  if (!a) return;
  try {
    await a.setPaused(v);
  } catch {
    /* ignore */
  }
}

export async function agentCoreSync(payload: {
  jobs?: unknown[];
  paused?: boolean;
  level?: number;
}): Promise<void> {
  const a = desktop()?.agent;
  if (!a?.sync) return;
  try {
    await a.sync(payload);
  } catch {
    /* ignore */
  }
}

export async function agentCoreApprove(id: string, grant: boolean): Promise<void> {
  const a = desktop()?.agent;
  if (!a?.approve) return;
  try {
    await a.approve(id, grant);
  } catch {
    /* ignore */
  }
}

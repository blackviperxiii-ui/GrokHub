/** Renderer client for the built-in LAN Hub (Electron IPC). */

export type HubPeer = { id: string; name: string; lastSeen?: number; role?: string };
export type HubRemote = { id: string; name: string; url: string; lastSeen?: number };

export type HubStatus = {
  ok: boolean;
  deviceId?: string;
  deviceName?: string;
  sharing?: boolean;
  port?: number;
  urls?: string[];
  pairCode?: string | null;
  pairExpiresAt?: number | null;
  peers?: HubPeer[];
  remotes?: HubRemote[];
  inboxCount?: number;
  lastIncomingAt?: number;
  error?: string;
  already?: boolean;
};

export type HubTask = {
  id: string;
  fromId?: string;
  fromName?: string;
  targetDeviceId?: string;
  title?: string;
  prompt: string;
  status?: string;
  createdAt?: number;
};

function desk() {
  return typeof window !== "undefined" ? window.grokhubDesktop?.hub : undefined;
}

export function isHubDesktop(): boolean {
  return Boolean(desk());
}

export async function hubStatus(): Promise<HubStatus> {
  const d = desk();
  if (!d?.status) return { ok: false, error: "Hub needs the desktop app." };
  return d.status();
}

export async function hubStartShare(): Promise<HubStatus> {
  const d = desk();
  if (!d?.startShare) return { ok: false, error: "Hub needs the desktop app." };
  return d.startShare();
}

export async function hubStopShare(): Promise<HubStatus> {
  const d = desk();
  if (!d?.stopShare) return { ok: false, error: "Hub needs the desktop app." };
  return d.stopShare();
}

export async function hubNewCode(): Promise<HubStatus> {
  const d = desk();
  if (!d?.newPairCode) return { ok: false, error: "Hub needs the desktop app." };
  return d.newPairCode();
}

export async function hubSetName(name: string): Promise<HubStatus> {
  const d = desk();
  if (!d?.setName) return { ok: false, error: "Hub needs the desktop app." };
  return d.setName(name);
}

export async function hubJoin(url: string, code: string): Promise<HubStatus> {
  const d = desk();
  if (!d?.join) return { ok: false, error: "Hub needs the desktop app." };
  return d.join({ url, code });
}

export async function hubLeave(id: string): Promise<HubStatus> {
  const d = desk();
  if (!d?.leave) return { ok: false, error: "Hub needs the desktop app." };
  return d.leave(id);
}

export async function hubForgetPeer(id: string): Promise<HubStatus> {
  const d = desk();
  if (!d?.forgetPeer) return { ok: false, error: "Hub needs the desktop app." };
  return d.forgetPeer(id);
}

export async function hubPushSnapshot(snapshot: unknown): Promise<{ ok: boolean; error?: string }> {
  const d = desk();
  if (!d?.pushSnapshot) return { ok: false, error: "Hub needs the desktop app." };
  return d.pushSnapshot(snapshot);
}

export async function hubPullSnapshot(): Promise<{
  ok: boolean;
  snapshot?: unknown;
  snapshots?: unknown[];
  error?: string;
}> {
  const d = desk();
  if (!d?.pullSnapshot) return { ok: false, error: "Hub needs the desktop app." };
  return d.pullSnapshot();
}

export async function hubSendTask(opts: {
  targetDeviceId: string;
  prompt: string;
  title?: string;
}): Promise<{ ok: boolean; error?: string }> {
  const d = desk();
  if (!d?.sendTask) return { ok: false, error: "Hub needs the desktop app." };
  return d.sendTask(opts);
}

export async function hubClaimInbox(): Promise<{ ok: boolean; tasks?: HubTask[]; error?: string }> {
  const d = desk();
  if (!d?.claimInbox) return { ok: false, error: "Hub needs the desktop app." };
  return d.claimInbox();
}

export async function hubTargets(): Promise<{
  ok: boolean;
  targets?: Array<{ id: string; name: string; self?: boolean }>;
}> {
  const d = desk();
  if (!d?.targets) return { ok: true, targets: [] };
  return d.targets();
}

import { useEffect, useState } from "react";
import {
  hubForgetPeer,
  hubJoin,
  hubLeave,
  hubNewCode,
  hubSetName,
  hubStartShare,
  hubStatus,
  hubStopShare,
  isHubDesktop,
  type HubStatus,
} from "@/lib/hub-client";
import { useGrokHub } from "@/lib/store";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./ui/card";
import { Input } from "./ui/input";

export function DevicesHubPanel({
  sectionHit,
}: {
  sectionHit?: (id: string) => boolean;
}) {
  const syncHubNow = useGrokHub((s) => s.syncHubNow);
  const sendRemoteTask = useGrokHub((s) => s.sendRemoteTask);
  const lastHubSyncAt = useGrokHub((s) => s.lastHubSyncAt);
  const [st, setSt] = useState<HubStatus>({ ok: false });
  const [nameDraft, setNameDraft] = useState("");
  const [joinUrl, setJoinUrl] = useState("");
  const [joinCode, setJoinCode] = useState("");
  const [taskTarget, setTaskTarget] = useState("");
  const [taskPrompt, setTaskPrompt] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const desktop = isHubDesktop();

  const refresh = async () => {
    const next = await hubStatus();
    setSt(next);
    if (next.deviceName && !nameDraft) setNameDraft(next.deviceName);
    if (!taskTarget && next.deviceId) setTaskTarget(next.deviceId);
  };

  useEffect(() => {
    void refresh();
    const t = window.setInterval(() => void refresh(), 8_000);
    return () => window.clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const run = async (fn: () => Promise<unknown>, okMsg: string) => {
    setBusy(true);
    setMsg("");
    try {
      const r = (await fn()) as { ok?: boolean; error?: string; detail?: string };
      if (r && r.ok === false) setMsg(r.error || r.detail || "Failed");
      else setMsg(okMsg);
      await refresh();
    } catch (e) {
      setMsg(e instanceof Error ? e.message : "Failed");
    } finally {
      setBusy(false);
    }
  };

  const hit = (id: string) => (sectionHit ? (sectionHit(id) ? "1" : "0") : "1");

  if (!desktop) {
    return (
      <Card id="sec-hub" data-settings-cat="devices" data-hit={hit("sec-hub")}>
        <CardHeader>
          <CardTitle className="text-sm">Devices</CardTitle>
          <CardDescription>
            Device sync runs in the GrokHub desktop app (not the browser preview).
          </CardDescription>
        </CardHeader>
      </Card>
    );
  }

  const urls = st.urls || [];
  const targets = [
    ...(st.deviceId ? [{ id: st.deviceId, name: `${st.deviceName || "This computer"} (here)` }] : []),
    ...(st.peers || []),
    ...(st.remotes || []).map((r) => ({ id: r.id, name: r.name })),
  ];

  return (
    <>
      <Card id="sec-hub" data-settings-cat="devices" data-hit={hit("sec-hub")}>
        <CardHeader>
          <CardTitle className="text-sm">This computer</CardTitle>
          <CardDescription>
            Pair other GrokHub apps on your network. Separate from Grok sign-in — no
            OAuth required.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-col gap-2 sm:flex-row">
            <Input
              value={nameDraft}
              onChange={(e) => setNameDraft(e.target.value)}
              placeholder="Computer name"
              className="sm:max-w-xs"
            />
            <Button
              size="sm"
              variant="secondary"
              disabled={busy || !nameDraft.trim()}
              onClick={() => void run(() => hubSetName(nameDraft.trim()), "Name saved")}
            >
              Save name
            </Button>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {st.sharing ? (
              <Badge variant="success">Sharing on Wi-Fi</Badge>
            ) : (
              <Badge>Private</Badge>
            )}
            <Button
              size="sm"
              disabled={busy}
              onClick={() =>
                void run(
                  () => (st.sharing ? hubStopShare() : hubStartShare()),
                  st.sharing ? "Sharing stopped" : "This computer is shareable",
                )
              }
            >
              {st.sharing ? "Stop sharing" : "Share this computer"}
            </Button>
          </div>
          {st.sharing && (
            <div className="space-y-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
              <div className="text-[10px] uppercase tracking-wide text-[var(--color-subtle)]">
                Pairing code
              </div>
              <div className="font-mono text-2xl tracking-[0.2em] text-[var(--color-fg)]">
                {st.pairCode || "—"}
              </div>
              <Button
                size="sm"
                variant="secondary"
                disabled={busy}
                onClick={() => void run(() => hubNewCode(), "New code")}
              >
                New code
              </Button>
              <div className="text-[10px] uppercase tracking-wide text-[var(--color-subtle)]">
                Address on this Wi-Fi
              </div>
              {urls.length ? (
                urls.map((u) => (
                  <div key={u} className="font-mono text-xs text-[var(--color-muted)]">
                    {u}
                  </div>
                ))
              ) : (
                <div className="text-xs text-[var(--color-muted)]">No LAN address yet</div>
              )}
              <p className="text-[11px] text-[var(--color-subtle)]">
                On the other computer: Settings → Devices → Join, paste an address and the
                code. Port 18766 must be allowed on this machine.
              </p>
            </div>
          )}
          {(st.peers || []).length > 0 && (
            <div className="space-y-1">
              <div className="text-[10px] uppercase tracking-wide text-[var(--color-subtle)]">
                Paired here
              </div>
              {st.peers!.map((p) => (
                <div
                  key={p.id}
                  className="flex items-center justify-between gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] px-2 py-1.5"
                >
                  <span className="text-sm text-[var(--color-fg)]">{p.name}</span>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => void run(() => hubForgetPeer(p.id), "Removed")}
                  >
                    Forget
                  </Button>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      <Card id="sec-hub-join" data-settings-cat="devices" data-hit={hit("sec-hub-join")}>
        <CardHeader>
          <CardTitle className="text-sm">Join another computer</CardTitle>
          <CardDescription>
            Paste the address and pairing code shown on the computer you want to reach.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <Input
            value={joinUrl}
            onChange={(e) => setJoinUrl(e.target.value)}
            placeholder="http://192.168.1.20:18766"
          />
          <Input
            value={joinCode}
            onChange={(e) => setJoinCode(e.target.value.toUpperCase())}
            placeholder="ABC-234"
            className="font-mono sm:max-w-[10rem]"
          />
          <Button
            size="sm"
            disabled={busy || !joinUrl.trim() || !joinCode.trim()}
            onClick={() =>
              void run(() => hubJoin(joinUrl.trim(), joinCode.trim()), "Paired")
            }
          >
            Join
          </Button>
          {(st.remotes || []).length > 0 && (
            <div className="space-y-1">
              {st.remotes!.map((r) => (
                <div
                  key={r.id}
                  className="flex items-center justify-between gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] px-2 py-1.5"
                >
                  <div>
                    <div className="text-sm">{r.name}</div>
                    <div className="font-mono text-[10px] text-[var(--color-subtle)]">{r.url}</div>
                  </div>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => void run(() => hubLeave(r.id), "Disconnected")}
                  >
                    Leave
                  </Button>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      <Card id="sec-hub-sync" data-settings-cat="devices" data-hit={hit("sec-hub-sync")}>
        <CardHeader>
          <CardTitle className="text-sm">Sync & remote tasks</CardTitle>
          <CardDescription>
            Chats, memory files, skills, and workboard — never API keys or Grok login.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              disabled={busy}
              onClick={() => void run(() => syncHubNow(), "Synced")}
            >
              Sync now
            </Button>
            {lastHubSyncAt ? (
              <span className="self-center text-[11px] text-[var(--color-subtle)]">
                Last sync {new Date(lastHubSyncAt).toLocaleTimeString()}
              </span>
            ) : null}
          </div>
          <textarea
            value={taskPrompt}
            onChange={(e) => setTaskPrompt(e.target.value)}
            rows={3}
            placeholder="Task for another computer… e.g. check disk space and summarize"
            className="w-full rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-bg)] px-3 py-2 text-sm text-[var(--color-fg)]"
          />
          <div className="flex flex-col gap-2 sm:flex-row">
            <select
              className="h-9 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-bg)] px-2 text-xs text-[var(--color-fg)]"
              value={taskTarget}
              onChange={(e) => setTaskTarget(e.target.value)}
            >
              {targets.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                </option>
              ))}
            </select>
            <Button
              size="sm"
              disabled={busy || !taskPrompt.trim() || !taskTarget}
              onClick={() =>
                void run(
                  () => sendRemoteTask(taskTarget, taskPrompt.trim()),
                  "Task sent",
                )
              }
            >
              Send task
            </Button>
          </div>
          {msg ? <p className="text-xs text-[var(--color-muted)]">{msg}</p> : null}
        </CardContent>
      </Card>
    </>
  );
}

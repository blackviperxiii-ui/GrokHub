import { useEffect, useRef, useState } from "react";
import { ExternalLink, FolderInput } from "lucide-react";
import { getModesWithCatalog } from "@/lib/modes";
import { friendlyModelName } from "@/lib/models-catalog";
import { applyUpdate, checkUpdate } from "@/lib/grok-client";
import { useCurrentUserState } from "@/lib/auth/use-current-user";
import { useGrokHub } from "@/lib/store";
import type { GrokModeId } from "@/lib/types";
import type { UpdateStatus } from "@/lib/update";
import { cn } from "@/lib/utils";
import { ProfileAvatar } from "../ProfileAvatar";
import { HostGatewayBanner } from "../HostGatewayBanner";
import { UsageMeterPanel } from "../UsageMeter";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";

export function SettingsView() {
  const mode = useGrokHub((s) => s.mode);
  const setMode = useGrokHub((s) => s.setMode);
  const modelCatalog = useGrokHub((s) => s.modelCatalog);
  const refreshModels = useGrokHub((s) => s.refreshModels);
  const lastModelsFetchAt = useGrokHub((s) => s.lastModelsFetchAt);
  const desktop = useGrokHub((s) => s.desktop);
  const setDesktop = useGrokHub((s) => s.setDesktop);
  const setNav = useGrokHub((s) => s.setNav);
  const resetDemo = useGrokHub((s) => s.resetDemo);
  const apiKey = useGrokHub((s) => s.apiKey);
  const setApiKey = useGrokHub((s) => s.setApiKey);
  const githubToken = useGrokHub((s) => s.githubToken);
  const setGithubToken = useGrokHub((s) => s.setGithubToken);
  const grokConnected = useGrokHub((s) => s.grokConnected);
  const grokStatusDetail = useGrokHub((s) => s.grokStatusDetail);
  const probeGrok = useGrokHub((s) => s.probeGrok);
  const syncFromGrok = useGrokHub((s) => s.syncFromGrok);
  const profile = useGrokHub((s) => s.profile);
  const oauth = useGrokHub((s) => s.oauth);
  const oauthPending = useGrokHub((s) => s.oauthPending);
  const startGrokOAuth = useGrokHub((s) => s.startGrokOAuth);
  const pollGrokOAuth = useGrokHub((s) => s.pollGrokOAuth);
  const clearGrokOAuth = useGrokHub((s) => s.clearGrokOAuth);
  const importOpenClawWorkspace = useGrokHub((s) => s.importOpenClawWorkspace);
  const clearOpenClawWorkspace = useGrokHub((s) => s.clearOpenClawWorkspace);
  const openClawWorkspace = useGrokHub((s) => s.openClawWorkspace);
  const { user } = useCurrentUserState();

  const [keyDraft, setKeyDraft] = useState(apiKey);
  const [ghDraft, setGhDraft] = useState(githubToken);
  const [probing, setProbing] = useState(false);
  const [oauthBusy, setOauthBusy] = useState(false);
  const [oauthErr, setOauthErr] = useState("");
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateLog, setUpdateLog] = useState<string>("");
  const [ocPath, setOcPath] = useState("~/.openclaw/workspace");
  const [ocBusy, setOcBusy] = useState(false);
  const [ocDetail, setOcDetail] = useState("");
  const pollRef = useRef<number | null>(null);

  useEffect(() => {
    setKeyDraft(apiKey);
  }, [apiKey]);
  useEffect(() => {
    setGhDraft(githubToken);
  }, [githubToken]);

  // When OAuth session exists, re-verify against api.x.ai so the status line matches reality
  useEffect(() => {
    if (!oauth?.accessToken) return;
    let cancelled = false;
    void (async () => {
      setProbing(true);
      try {
        await probeGrok();
      } finally {
        if (!cancelled) setProbing(false);
      }
    })();
    return () => {
      cancelled = true;
    };
    // only on mount / oauth identity change
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [oauth?.accessToken, oauth?.email]);

  // Auto-poll device code while pending
  useEffect(() => {
    if (!oauthPending) {
      if (pollRef.current) {
        window.clearInterval(pollRef.current);
        pollRef.current = null;
      }
      return;
    }
    const tick = async () => {
      try {
        const r = await pollGrokOAuth();
        if (r === "ready" || r === "failed") {
          setOauthBusy(false);
        }
      } catch (e) {
        setOauthErr(e instanceof Error ? e.message : "poll failed");
        setOauthBusy(false);
      }
    };
    void tick();
    pollRef.current = window.setInterval(() => void tick(), 5000);
    return () => {
      if (pollRef.current) window.clearInterval(pollRef.current);
    };
  }, [oauthPending, pollGrokOAuth]);

  useEffect(() => {
    void checkUpdate(githubToken || undefined)
      .then(setUpdate)
      .catch((e) =>
        setUpdate({
          currentVersion: "0.2.0",
          currentSha: null,
          remoteSha: null,
          remoteMessage: null,
          updateAvailable: false,
          repo: "blackviperxiii-ui/Grok-Hub",
          branch: "main",
          installRoot: null,
          detail: e instanceof Error ? e.message : "check failed",
        }),
      );
  }, [githubToken]);

  async function onStartOAuth() {
    setOauthErr("");
    setOauthBusy(true);
    try {
      await startGrokOAuth();
      const pending = useGrokHub.getState().oauthPending;
      if (pending?.verificationUriComplete) {
        window.open(pending.verificationUriComplete, "_blank", "noopener,noreferrer");
      } else if (pending?.verificationUri) {
        window.open(pending.verificationUri, "_blank", "noopener,noreferrer");
      }
    } catch (e) {
      setOauthErr(e instanceof Error ? e.message : "Could not start OAuth");
      setOauthBusy(false);
    }
  }

  async function saveAndProbe() {
    setApiKey(keyDraft.trim());
    setProbing(true);
    try {
      const ok = await probeGrok();
      if (ok && user && !user.isDevFallback) {
        await syncFromGrok({
          displayName: user.displayName,
          email: user.primaryEmail,
          imageUrl: user.profileImageUrl,
        });
      }
    } finally {
      setProbing(false);
    }
  }

  async function onCheckUpdate() {
    setUpdateBusy(true);
    setUpdateLog("");
    try {
      setGithubToken(ghDraft.trim());
      const s = await checkUpdate(ghDraft.trim() || undefined);
      setUpdate(s);
      setUpdateLog(s.detail);
    } catch (e) {
      setUpdateLog(e instanceof Error ? e.message : "check failed");
    } finally {
      setUpdateBusy(false);
    }
  }

  async function onInstallUpdate() {
    setUpdateBusy(true);
    setUpdateLog("Installing update from GitHub…");
    try {
      setGithubToken(ghDraft.trim());
      const r = await applyUpdate(ghDraft.trim() || undefined, true);
      if (r.status) {
        setUpdate(r.status);
      } else {
        const s = await checkUpdate(ghDraft.trim() || undefined);
        setUpdate(s);
      }
      const lines = [
        r.ok ? "OK" : "FAILED",
        r.detail,
        r.newVersion ? `Version: v${r.newVersion}` : "",
        r.newSha ? `Commit: ${r.newSha}` : "",
        "",
        ...(r.steps || []),
      ].filter(Boolean);
      if (r.ok && r.restarting) {
        lines.push("", "Restarting GrokHub…");
        setUpdateLog(lines.join("\n"));
        // Desktop shell exits; in browser preview just refresh state
        setTimeout(() => {
          window.location.reload();
        }, 1500);
        return;
      }
      setUpdateLog(lines.join("\n"));
    } catch (e) {
      setUpdateLog(e instanceof Error ? e.message : "update failed");
    } finally {
      setUpdateBusy(false);
    }
  }

  return (
    <div className="content-readable mx-auto space-y-5 pb-8">
      {/* Primary: real xAI Grok OAuth */}
      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Connect to Grok (xAI OAuth)</CardTitle>
          <CardDescription>
            Sign in with your <strong>SuperGrok</strong> or <strong>X Premium+</strong> account
            via xAI device code — same flow as OpenClaw / Grok CLI. No API key required for
            subscription access.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <Badge
              variant={
                oauth && grokConnected
                  ? "success"
                  : oauth
                    ? "info"
                    : grokConnected
                      ? "success"
                      : "default"
              }
            >
              {oauth && grokConnected
                ? "OAuth live"
                : oauth
                  ? "OAuth session"
                  : grokConnected
                    ? "API connected"
                    : "Not connected"}
            </Badge>
            <span className="text-xs text-[var(--color-muted)]">
              {probing
                ? "Verifying with xAI…"
                : oauth && grokStatusDetail.toLowerCase().includes("not connected")
                  ? "Session saved — verifying API access…"
                  : grokStatusDetail}
            </span>
            {oauth && (
              <Button
                variant="secondary"
                size="sm"
                disabled={probing}
                onClick={() => void saveAndProbe()}
              >
                {probing ? "Testing…" : "Test connection"}
              </Button>
            )}
          </div>

          {oauth && (
            <div className="flex flex-wrap items-center gap-3 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-success)_35%,transparent)] bg-[color-mix(in_oklab,var(--color-success)_8%,transparent)] px-3 py-3">
              <ProfileAvatar
                src={oauth.picture}
                name={oauth.name}
                email={oauth.email}
              />
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{oauth.name || "Grok account"}</div>
                <div className="truncate text-xs text-[var(--color-muted)]">
                  {oauth.email || "OAuth session active"}
                </div>
              </div>
              <Button variant="secondary" size="sm" onClick={() => clearGrokOAuth()}>
                Disconnect
              </Button>
            </div>
          )}

          {oauthPending && (
            <div className="space-y-3 rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-elevated)] p-4">
              <div className="text-xs uppercase tracking-wide text-[var(--color-subtle)]">
                Approve this code
              </div>
              <div className="font-mono text-3xl font-semibold tracking-[0.2em] text-[var(--color-fg)]">
                {oauthPending.userCode}
              </div>
              <p className="text-sm text-[var(--color-muted)]">
                Open the link, sign in to xAI / Grok, and enter the code. This window polls
                automatically.
              </p>
              <div className="flex flex-wrap gap-2">
                <Button
                  onClick={() =>
                    window.open(
                      oauthPending.verificationUriComplete || oauthPending.verificationUri,
                      "_blank",
                      "noopener,noreferrer",
                    )
                  }
                >
                  <ExternalLink className="h-4 w-4" />
                  Open accounts.x.ai
                </Button>
                <Button variant="secondary" onClick={() => void pollGrokOAuth()}>
                  Check now
                </Button>
              </div>
              <p className="text-xs text-[var(--color-subtle)]">
                Waiting for approval… {oauthBusy ? "polling" : ""}
              </p>
            </div>
          )}

          {!oauth && !oauthPending && (
            <Button onClick={() => void onStartOAuth()} disabled={oauthBusy}>
              {oauthBusy ? "Starting…" : "Connect with Grok OAuth"}
            </Button>
          )}

          {oauthErr && (
            <p className="text-sm text-[var(--color-danger)]">{oauthErr}</p>
          )}

          <p className="text-xs text-[var(--color-subtle)]">
            Uses xAI public OAuth client (device code). Tokens stay on this device only and are
            never committed to git.
          </p>
        </CardContent>
      </Card>

      <UsageMeterPanel />

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-sm">
            <FolderInput className="h-4 w-4 text-[var(--color-muted)]" />
            Import OpenClaw workspace
          </CardTitle>
          <CardDescription>
            Pull skills, persona, and memory from an OpenClaw agent home (
            <span className="font-mono">~/.openclaw/workspace</span>
            ). Credentials and sqlite sessions are not imported.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {openClawWorkspace ? (
            <div className="rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-success)_35%,transparent)] bg-[color-mix(in_oklab,var(--color-success)_8%,transparent)] px-3 py-2 text-sm">
              <div className="font-medium">
                {openClawWorkspace.identityName || "Workspace linked"}
              </div>
              <div className="truncate font-mono text-xs text-[var(--color-muted)]">
                {openClawWorkspace.root}
              </div>
              <div className="mt-1 text-xs text-[var(--color-subtle)]">
                {openClawWorkspace.filesImported.length} files · imported{" "}
                {new Date(openClawWorkspace.importedAt).toLocaleString()}
              </div>
            </div>
          ) : (
            <p className="text-xs text-[var(--color-subtle)]">
              Default path is scanned automatically if you leave it blank.
            </p>
          )}
          <div className="flex flex-wrap gap-2">
            <Input
              value={ocPath}
              onChange={(e) => setOcPath(e.target.value)}
              placeholder="~/.openclaw/workspace"
              className="min-w-[220px] flex-1 font-mono text-xs"
            />
            <Button
              disabled={ocBusy}
              onClick={() => {
                setOcBusy(true);
                setOcDetail("");
                void importOpenClawWorkspace(ocPath.trim() || undefined).then((r) => {
                  setOcDetail(r.detail);
                  setOcBusy(false);
                });
              }}
            >
              {ocBusy ? "Importing…" : "Import workspace"}
            </Button>
            {openClawWorkspace && (
              <Button variant="secondary" onClick={() => clearOpenClawWorkspace()}>
                Clear import
              </Button>
            )}
          </div>
          {ocDetail && (
            <p className="text-xs text-[var(--color-muted)]">{ocDetail}</p>
          )}
          <p className="text-[11px] text-[var(--color-subtle)]">
            Imports <span className="font-mono">skills/**/SKILL.md</span>, AGENTS/SOUL/USER/IDENTITY,
            HEARTBEAT → automation, and injects context into Agent chat.
          </p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">xAI API key (optional fallback)</CardTitle>
          <CardDescription>
            Pay-per-token console key if you are not using SuperGrok OAuth. From{" "}
            <span className="font-mono">console.x.ai</span>.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <Input
            type="password"
            autoComplete="off"
            value={keyDraft}
            onChange={(e) => setKeyDraft(e.target.value)}
            placeholder="xai-…"
          />
          <div className="flex flex-wrap gap-2">
            <Button onClick={() => void saveAndProbe()} disabled={probing}>
              {probing ? "Testing…" : "Save & test key"}
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                setKeyDraft("");
                setApiKey("");
              }}
            >
              Clear key
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex flex-wrap items-start justify-between gap-2">
            <div>
              <CardTitle className="text-sm">Essential models</CardTitle>
              <CardDescription>
                Polled from xAI · only 4.5 / 4.3 / Fast / Build / Imagine class ids
              </CardDescription>
            </div>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void refreshModels({ force: true })}
            >
              Refresh + reclassify
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="grid gap-2 sm:grid-cols-2">
            {(
              [
                ["fast", "Fast chat"],
                ["balanced", "Balanced"],
                ["smart", "Brains"],
                ["heavy", "Heavy / team"],
                ["build", "Build / code"],
                ["imagine", "Imagine"],
              ] as const
            ).map(([slot, label]) => (
              <div
                key={slot}
                className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2"
              >
                <div className="text-[10px] uppercase tracking-wide text-[var(--color-subtle)]">
                  {label}
                </div>
                <div className="text-sm font-medium text-[var(--color-fg)]">
                  {friendlyModelName(modelCatalog.slots[slot])}
                </div>
                <div className="font-mono text-[10px] text-[var(--color-muted)]">
                  {modelCatalog.slots[slot]}
                </div>
              </div>
            ))}
          </div>
          {modelCatalog.essential.length > 0 && (
            <div className="flex flex-wrap gap-1">
              {modelCatalog.essential.map((m) => (
                <Badge key={m} className="font-mono text-[10px]">
                  {m}
                </Badge>
              ))}
            </div>
          )}
          <div className="text-[10px] text-[var(--color-subtle)]">
            Source: {modelCatalog.source} · slots by{" "}
            {modelCatalog.classifiedBy === "grok" ? "Grok" : "heuristic"}
            {modelCatalog.classifyNotes ? ` · ${modelCatalog.classifyNotes}` : ""}
            {lastModelsFetchAt
              ? ` · last poll ${new Date(lastModelsFetchAt).toLocaleTimeString()}`
              : " · not polled yet"}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Updates (GitHub)</CardTitle>
          <CardDescription>Install the latest clean release from the package repo.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {update && (
            <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-sm">
              <div className="flex flex-wrap items-center gap-2">
                <Badge>v{update.currentVersion}</Badge>
                {update.updateAvailable ? (
                  <Badge variant="info">Update available</Badge>
                ) : (
                  <Badge variant="success">Up to date</Badge>
                )}
              </div>
              <div className="mt-2 space-y-1 font-mono text-xs text-[var(--color-muted)]">
                <div>{update.detail}</div>
                {(update.currentSha || update.remoteSha) && (
                  <div>
                    local {update.currentSha || "?"}
                    {update.remoteSha ? ` · remote ${update.remoteSha}` : ""}
                  </div>
                )}
              </div>
            </div>
          )}
          <Input
            type="password"
            autoComplete="off"
            value={ghDraft}
            onChange={(e) => setGhDraft(e.target.value)}
            placeholder="GitHub token (optional)"
          />
          <div className="flex flex-wrap gap-2">
            <Button variant="secondary" disabled={updateBusy} onClick={() => void onCheckUpdate()}>
              Check for updates
            </Button>
            <Button disabled={updateBusy} onClick={() => void onInstallUpdate()}>
              {updateBusy
                ? "Installing…"
                : update?.updateAvailable
                  ? "Install latest"
                  : "Reinstall / repair"}
            </Button>
          </div>
          {updateLog && (
            <pre className="scroll-panel max-h-48 whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] p-3 font-mono text-xs text-[var(--color-muted)]">
              {updateLog}
            </pre>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Model modes</CardTitle>
          <CardDescription>
            Auto routes each prompt to Fast · 4.3 · 4.5 · Build · Imagine balancing tokens.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {getModesWithCatalog(modelCatalog).map((m) => {
            const selected = m.id === mode;
            return (
              <button
                key={m.id}
                type="button"
                onClick={() => setMode(m.id as GrokModeId)}
                className={cn(
                  "flex w-full items-center justify-between gap-3 rounded-[var(--radius-md)] border px-3 py-3 text-left transition-colors",
                  selected
                    ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)]"
                    : "border-[var(--color-border)] hover:border-[var(--color-border-strong)]",
                )}
              >
                <div>
                  <div className="text-sm font-medium">{m.label}</div>
                  <div className="text-xs text-[var(--color-muted)]">{m.subtitle}</div>
                  {m.id !== "auto" && (
                    <div className="mt-0.5 font-mono text-[10px] text-[var(--color-subtle)]">
                      {m.modelId}
                    </div>
                  )}
                </div>
                {selected && <span className="text-xs text-[var(--color-muted)]">Active</span>}
              </button>
            );
          })}
        </CardContent>
      </Card>

      <HostGatewayBanner variant="card" onOpenDesktop={() => setNav("desktop")} />

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Arch Linux shell preferences</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          {(
            [
              ["wayland", "Prefer Wayland", "Ozone flags"],
              ["tray", "System tray", "Minimize to tray"],
              ["launchOnLogin", "Launch on login", "Autostart"],
              ["startMinimized", "Start minimized", "Tray only"],
              [
                "confirmHostCommands",
                "Confirm host commands",
                "Ask before agent runs shell on your machine",
              ],
              [
                "confirmDestructiveOnly",
                "Only risky commands",
                "Skip confirm for read-only commands (ls, cat, …)",
              ],
            ] as const
          ).map(([key, label, hint]) => (
            <label
              key={key}
              className="flex cursor-pointer items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3"
            >
              <div>
                <div className="text-sm font-medium">{label}</div>
                <div className="text-xs text-[var(--color-muted)]">{hint}</div>
              </div>
              <input
                type="checkbox"
                className="h-4 w-4 accent-[var(--color-fg)]"
                checked={Boolean(desktop[key])}
                onChange={(e) => setDesktop({ [key]: e.target.checked })}
              />
            </label>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Danger zone</CardTitle>
          <CardDescription>
            Wipe local chat history, connectors, and preferences on this device. Does not revoke
            Grok OAuth on xAI servers — disconnect first if you want a full sign-out.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Button
            variant="secondary"
            onClick={() => {
              if (
                typeof window !== "undefined" &&
                window.confirm("Reset GrokHub to a clean install on this device?")
              ) {
                resetDemo();
              }
            }}
          >
            Reset to clean install
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}

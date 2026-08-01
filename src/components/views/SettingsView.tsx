import { useEffect, useRef, useState } from "react";
import { Link } from "@tanstack/react-router";
import { ExternalLink } from "lucide-react";
import { GROK_MODES } from "@/lib/modes";
import { applyUpdate, checkUpdate } from "@/lib/grok-client";
import { GROK_PROVIDERS, authEnabled, signIn, signOut } from "@/lib/auth/client";
import { useCurrentUserState } from "@/lib/auth/use-current-user";
import { useGrokHub } from "@/lib/store";
import type { GrokModeId } from "@/lib/types";
import type { UpdateStatus } from "@/lib/update";
import { cn } from "@/lib/utils";
import { ProfileAvatar } from "../ProfileAvatar";
import { UsageMeterPanel } from "../UsageMeter";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";

export function SettingsView() {
  const mode = useGrokHub((s) => s.mode);
  const setMode = useGrokHub((s) => s.setMode);
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
  const { user, isPending } = useCurrentUserState();

  const [keyDraft, setKeyDraft] = useState(apiKey);
  const [ghDraft, setGhDraft] = useState(githubToken);
  const [probing, setProbing] = useState(false);
  const [oauthBusy, setOauthBusy] = useState(false);
  const [oauthErr, setOauthErr] = useState("");
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateLog, setUpdateLog] = useState<string>("");
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
    <div className="mx-auto max-w-3xl space-y-5 pb-8">
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

      {/* Optional app identity via Grok Build broker */}
      <Card>
        <CardHeader>
          <CardTitle className="text-sm">App account (optional)</CardTitle>
          <CardDescription>
            Grok Build app identity via Google/X (auth.grok.me). Separate from live Grok model
            access above.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {isPending ? (
            <div className="h-10 animate-pulse rounded bg-[var(--color-elevated)]" />
          ) : user && !user.isDevFallback ? (
            <div className="flex flex-wrap items-center gap-3">
              <ProfileAvatar
                src={user.profileImageUrl}
                name={user.displayName}
                email={user.primaryEmail}
              />
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{user.displayName || "Signed in"}</div>
                <div className="truncate text-xs text-[var(--color-muted)]">
                  {user.primaryEmail}
                </div>
              </div>
              {authEnabled && (
                <Button variant="secondary" size="sm" onClick={() => void signOut()}>
                  Sign out
                </Button>
              )}
            </div>
          ) : authEnabled ? (
            <div className="flex flex-wrap gap-2">
              {GROK_PROVIDERS.map((p) => (
                <Button
                  key={p.providerId}
                  variant="secondary"
                  size="sm"
                  onClick={() => void signIn(p.providerId, { callbackURL: "/" })}
                >
                  {p.label}
                </Button>
              ))}
              <Link
                to="/login"
                className="self-center text-xs text-[var(--color-muted)] underline-offset-2 hover:underline"
              >
                Full sign-in page
              </Link>
            </div>
          ) : (
            <p className="text-sm text-[var(--color-muted)]">Auth disabled.</p>
          )}
        </CardContent>
      </Card>

      <UsageMeterPanel />

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

      {profile.models.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="text-sm">Models from your connection</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex flex-wrap gap-1">
              {profile.models.slice(0, 16).map((m) => (
                <Badge key={m} className="font-mono text-[10px]">
                  {m}
                </Badge>
              ))}
            </div>
          </CardContent>
        </Card>
      )}

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
          <CardDescription>Mapped to live xAI models after you connect.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {GROK_MODES.map((m) => {
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
                </div>
                {selected && <span className="text-xs text-[var(--color-muted)]">Active</span>}
              </button>
            );
          })}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Desktop host</CardTitle>
        </CardHeader>
        <CardContent>
          <Button onClick={() => setNav("desktop")}>Open Desktop host</Button>
        </CardContent>
      </Card>

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
                checked={desktop[key]}
                onChange={(e) => setDesktop({ [key]: e.target.checked })}
              />
            </label>
          ))}
        </CardContent>
      </Card>

      <Button variant="secondary" onClick={resetDemo}>
        Reset to clean install
      </Button>
    </div>
  );
}

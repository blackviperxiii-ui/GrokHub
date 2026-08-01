import { useEffect, useState } from "react";
import { Link } from "@tanstack/react-router";
import { GROK_MODES } from "@/lib/modes";
import { applyUpdate, checkUpdate } from "@/lib/grok-client";
import { GROK_PROVIDERS, authEnabled, signIn, signOut } from "@/lib/auth/client";
import { useCurrentUserState } from "@/lib/auth/use-current-user";
import { useGrokHub } from "@/lib/store";
import type { GrokModeId } from "@/lib/types";
import type { UpdateStatus } from "@/lib/update";
import { cn } from "@/lib/utils";
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
  const { user, isPending } = useCurrentUserState();

  const [keyDraft, setKeyDraft] = useState(apiKey);
  const [ghDraft, setGhDraft] = useState(githubToken);
  const [probing, setProbing] = useState(false);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateLog, setUpdateLog] = useState<string>("");

  useEffect(() => {
    setKeyDraft(apiKey);
  }, [apiKey]);
  useEffect(() => {
    setGhDraft(githubToken);
  }, [githubToken]);

  useEffect(() => {
    void checkUpdate(githubToken || undefined)
      .then(setUpdate)
      .catch((e) =>
        setUpdate({
          currentVersion: "0.1",
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
      const r = await applyUpdate(ghDraft.trim() || undefined);
      setUpdateLog([r.detail, "", ...(r.steps || [])].join("\n"));
      if (r.ok) {
        const s = await checkUpdate(ghDraft.trim() || undefined);
        setUpdate(s);
      }
    } catch (e) {
      setUpdateLog(e instanceof Error ? e.message : "update failed");
    } finally {
      setUpdateBusy(false);
    }
  }

  return (
    <div className="mx-auto max-w-3xl space-y-5 pb-8">
      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Grok account (OAuth)</CardTitle>
          <CardDescription>
            Sign in through the Grok Build auth broker (auth.grok.me) with Google or X. Identity is
            pulled after login — nothing personal is preloaded in the app package.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {isPending ? (
            <div className="h-10 animate-pulse rounded bg-[var(--color-elevated)]" />
          ) : user && !user.isDevFallback ? (
            <div className="flex flex-wrap items-center gap-3">
              {user.profileImageUrl ? (
                <img
                  src={user.profileImageUrl}
                  alt=""
                  className="h-10 w-10 rounded-full object-cover"
                />
              ) : (
                <div className="grid h-10 w-10 place-items-center rounded-full bg-[var(--color-elevated)] text-sm font-medium">
                  {(user.displayName || user.primaryEmail || "?").charAt(0).toUpperCase()}
                </div>
              )}
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{user.displayName || "Grok user"}</div>
                <div className="truncate text-xs text-[var(--color-muted)]">
                  {user.primaryEmail || "Signed in via Grok OAuth"}
                </div>
              </div>
              <Badge variant="success">OAuth</Badge>
              {authEnabled && (
                <Button variant="secondary" size="sm" onClick={() => void signOut()}>
                  Sign out
                </Button>
              )}
            </div>
          ) : authEnabled ? (
            <div className="space-y-2">
              {GROK_PROVIDERS.map((p) => (
                <Button
                  key={p.providerId}
                  className="w-full"
                  variant={p.idp === "google" ? "default" : "secondary"}
                  onClick={() => void signIn(p.providerId, { callbackURL: "/" })}
                >
                  Continue with {p.label}
                </Button>
              ))}
              <p className="text-center text-xs text-[var(--color-subtle)]">
                Or open the{" "}
                <Link to="/login" className="underline underline-offset-2">
                  sign-in page
                </Link>
              </p>
            </div>
          ) : (
            <p className="text-sm text-[var(--color-muted)]">Auth is disabled in this build.</p>
          )}
          {profile.models.length > 0 && (
            <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
              <div className="text-[10px] uppercase tracking-wide text-[var(--color-subtle)]">
                Models from xAI
              </div>
              <div className="mt-1 flex flex-wrap gap-1">
                {profile.models.slice(0, 12).map((m) => (
                  <Badge key={m} className="font-mono text-[10px]">
                    {m}
                  </Badge>
                ))}
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <UsageMeterPanel />

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">xAI API key (chat)</CardTitle>
          <CardDescription>
            Agent replies use the live xAI API. Key from console.x.ai — or env{" "}
            <span className="font-mono">XAI_API_KEY</span>. Stored only on this device.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant={grokConnected ? "success" : "default"}>
              {grokConnected ? "API connected" : "API not connected"}
            </Badge>
            <span className="text-xs text-[var(--color-muted)]">{grokStatusDetail}</span>
          </div>
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-[var(--color-muted)]">xAI API key</label>
            <Input
              type="password"
              autoComplete="off"
              value={keyDraft}
              onChange={(e) => setKeyDraft(e.target.value)}
              placeholder="xai-…"
            />
          </div>
          <div className="flex flex-wrap gap-2">
            <Button onClick={() => void saveAndProbe()} disabled={probing}>
              {probing ? "Testing…" : "Save & test"}
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
          <CardTitle className="text-sm">Updates (GitHub)</CardTitle>
          <CardDescription>
            Install the latest clean release from the public package repo.
          </CardDescription>
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
                <div>
                  local {update.currentSha || "—"} → remote {update.remoteSha || "—"}
                </div>
                <div>
                  {update.repo}@{update.branch}
                </div>
                <div>{update.detail}</div>
              </div>
            </div>
          )}
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-[var(--color-muted)]">
              GitHub token (optional, private repo)
            </label>
            <Input
              type="password"
              autoComplete="off"
              value={ghDraft}
              onChange={(e) => setGhDraft(e.target.value)}
              placeholder="ghp_… or github_pat_…"
            />
          </div>
          <div className="flex flex-wrap gap-2">
            <Button variant="secondary" disabled={updateBusy} onClick={() => void onCheckUpdate()}>
              Check for updates
            </Button>
            <Button
              disabled={updateBusy || (update ? !update.updateAvailable : false)}
              onClick={() => void onInstallUpdate()}
            >
              {updateBusy ? "Working…" : "Install latest"}
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
            Auto, Fast, Expert, Heavy, Build — mapped to live xAI Grok models after you connect.
          </CardDescription>
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
                  <div className="flex items-center gap-2 text-sm font-medium">
                    {m.label}
                    {m.id === "build" && <Badge className="text-[10px]">Beta</Badge>}
                  </div>
                  <div className="text-xs text-[var(--color-muted)]">{m.subtitle}</div>
                </div>
                {selected && (
                  <span className="text-xs text-[var(--color-muted)]">Active</span>
                )}
              </button>
            );
          })}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Desktop host</CardTitle>
          <CardDescription>
            Unsandboxed shell, files, and apps. Opens from the Desktop tab.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <Button onClick={() => setNav("desktop")}>Open Desktop host</Button>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Arch Linux desktop shell</CardTitle>
          <CardDescription>Window auto-fits the work area on launch.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {(
            [
              ["wayland", "Prefer Wayland", "Ozone flags for Hyprland / KDE / GNOME"],
              ["tray", "System tray", "Minimize to tray"],
              ["launchOnLogin", "Launch on login", "Autostart entry"],
              ["startMinimized", "Start minimized", "Boot to tray only"],
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

import { useCallback, useEffect, useRef, useState } from "react";
import { ExternalLink, FolderInput, HardDrive, Moon, RefreshCw, Sun, Monitor } from "lucide-react";

const SETTINGS_SECTIONS = [
  { id: "sec-wizard", label: "First-run" },
  { id: "sec-autonomy", label: "Autonomy" },
  { id: "sec-appearance", label: "Appearance" },
  { id: "sec-oauth", label: "Grok OAuth" },
  { id: "sec-free", label: "Free Grok" },
  { id: "sec-usage", label: "Usage" },
  { id: "sec-setup", label: "Setup sync" },
  { id: "sec-api", label: "API key" },
  { id: "sec-models", label: "Models" },
  { id: "sec-model-overrides", label: "Mode pins" },
  { id: "sec-updates", label: "Updates" },
  { id: "sec-modes", label: "Modes" },
  { id: "sec-agent", label: "Agent" },
  { id: "sec-desktop", label: "Desktop shell" },
  { id: "sec-selfmod", label: "Self-mod" },
  { id: "sec-memory", label: "Memory" },
  { id: "sec-project", label: "Project" },
  { id: "sec-learning", label: "Learning" },
  { id: "sec-diagnostics", label: "Diagnostics" },
  { id: "sec-danger", label: "Danger zone" },
] as const;

import { getModesWithCatalog, OVERRIDABLE_MODES, hasModelOverride, type ModelModeOverrides } from "@/lib/modes";
import { friendlyModelName } from "@/lib/models-catalog";
import { applyUpdate, checkUpdate, applyRollback, postUpdateSelfTest } from "@/lib/grok-client";
import { useCurrentUserState } from "@/lib/auth/use-current-user";
import { exportMemory, importMemory, memoryInfo } from "@/lib/persistent-storage";
import {
  memoryFsInfo,
  memoryList,
  memoryRead,
  memoryWrite,
  type MemoryFileInfo,
} from "@/lib/file-memory";
import {
  factoryReinstall,
  selfModInfo,
  selfModRestore,
  selfModSnapshot,
} from "@/lib/self-mod-client";
import { useGrokHub } from "@/lib/store";
import type { GrokModeId } from "@/lib/types";
import type { UpdateStatus } from "@/lib/update";
import { learningSummaryLine } from "@/lib/learning";
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
  const modelOverrides = useGrokHub((s) => s.modelOverrides);
  const setModelOverride = useGrokHub((s) => s.setModelOverride);
  const clearModelOverrides = useGrokHub((s) => s.clearModelOverrides);
  const refreshModels = useGrokHub((s) => s.refreshModels);
  const lastModelsFetchAt = useGrokHub((s) => s.lastModelsFetchAt);
  const desktop = useGrokHub((s) => s.desktop);
  const setDesktop = useGrokHub((s) => s.setDesktop);
  const setNav = useGrokHub((s) => s.setNav);
  const resetDemo = useGrokHub((s) => s.resetDemo);
  const clearQuickAssistMemory = useGrokHub((s) => s.clearQuickAssistMemory);
  const quickAssistMemory = useGrokHub((s) => s.quickAssistMemory);
  const [memInfo, setMemInfo] = useState<{
    path?: string;
    userData?: string;
    bytes?: number;
    updatedAt?: number;
  } | null>(null);
  const [memMsg, setMemMsg] = useState("");
  const importRef = useRef<HTMLInputElement>(null);
  const [selfMsg, setSelfMsg] = useState("");
  const [selfBusy, setSelfBusy] = useState(false);
  const [selfInfo, setSelfInfo] = useState<Awaited<ReturnType<typeof selfModInfo>> | null>(null);

  useEffect(() => {
    void memoryInfo().then(setMemInfo);
    void selfModInfo().then(setSelfInfo);
  }, []);
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
  const preferFreeGrok = useGrokHub((s) => s.preferFreeGrok);
  const setPreferFreeGrok = useGrokHub((s) => s.setPreferFreeGrok);
  const setPlan = useGrokHub((s) => s.setPlan);
  const usagePlan = useGrokHub((s) => s.usage.plan);
  const importOpenClawWorkspace = useGrokHub((s) => s.importOpenClawWorkspace);
  const clearOpenClawWorkspace = useGrokHub((s) => s.clearOpenClawWorkspace);
  const openClawWorkspace = useGrokHub((s) => s.openClawWorkspace);
  const setupSyncMeta = useGrokHub((s) => s.setupSyncMeta);
  const pushSetupSync = useGrokHub((s) => s.pushSetupSync);
  const pullSetupSync = useGrokHub((s) => s.pullSetupSync);
  const syncSetupWithGrokAccount = useGrokHub((s) => s.syncSetupWithGrokAccount);
  const exportSetupPackJson = useGrokHub((s) => s.exportSetupPackJson);
  const importSetupPackJson = useGrokHub((s) => s.importSetupPackJson);
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
  const [setupBusy, setSetupBusy] = useState(false);
  const [setupMsg, setSetupMsg] = useState("");
  const [setupPass, setSetupPass] = useState("");
  const setupImportRef = useRef<HTMLInputElement>(null);
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
    const st = useGrokHub.getState();
    if (st.running) {
      setUpdateLog("Agent is running — stop it before installing an update.");
      return;
    }
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
        lines.push("", "Restarting GrokHub… window will close and reopen.");
        setUpdateLog(lines.join("\n"));
        // Desktop shell exits and relaunches — do NOT reload mid-exit (causes instability)
        if (!window.grokhubDesktop) {
          setTimeout(() => {
            window.location.reload();
          }, 1500);
        }
        return;
      }
      setUpdateLog(lines.join("\n"));
    } catch (e) {
      setUpdateLog(e instanceof Error ? e.message : "update failed");
    } finally {
      setUpdateBusy(false);
    }
  }

  async function onRollback() {
    if (useGrokHub.getState().running) {
      setUpdateLog("Stop the agent before rollback.");
      return;
    }
    if (!window.confirm("Undo last update and restore the previous install? The app will restart.")) {
      return;
    }
    setUpdateBusy(true);
    setUpdateLog("Rolling back…");
    try {
      const r = await applyRollback();
      const lines = [r.ok ? "OK" : "FAILED", r.detail || "", "", ...((r as { steps?: string[] }).steps || [])].filter(Boolean);
      if (r.ok && (r as { restarting?: boolean }).restarting) {
        lines.push("", "Restarting…");
        setUpdateLog(lines.join("\n"));
        return;
      }
      setUpdateLog(lines.join("\n"));
    } catch (e) {
      setUpdateLog(e instanceof Error ? e.message : "rollback failed");
    } finally {
      setUpdateBusy(false);
    }
  }

  async function onSelfTest() {
    setUpdateBusy(true);
    try {
      const r = await postUpdateSelfTest();
      setUpdateLog([r.detail || "", "", ...((r as { checks?: string[] }).checks || [])].join("\n"));
    } catch (e) {
      setUpdateLog(e instanceof Error ? e.message : "self-test failed");
    } finally {
      setUpdateBusy(false);
    }
  }

  const uiTheme = useGrokHub((s) => s.uiTheme);
  const setUiTheme = useGrokHub((s) => s.setUiTheme);
  const [settingsQ, setSettingsQ] = useState("");
  const [factoryPhrase, setFactoryPhrase] = useState("");

  const sectionVisible = (id: string, labels: string) => {
    const q = settingsQ.trim().toLowerCase();
    if (!q) return true;
    return id.toLowerCase().includes(q) || labels.toLowerCase().includes(q);
  };

  return (
    <div className="content-readable mx-auto space-y-5 pb-8">
      <Input
        value={settingsQ}
        onChange={(e) => setSettingsQ(e.target.value)}
        placeholder="Search settings…"
        className="h-9"
        aria-label="Search settings"
      />
      <nav
        aria-label="Settings sections"
        className="sticky top-0 z-20 -mx-1 mb-1 flex gap-1.5 overflow-x-auto scroll-hide rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[color-mix(in_oklab,var(--color-panel)_92%,transparent)] px-2 py-2 backdrop-blur-md"
      >
        {SETTINGS_SECTIONS.filter((s) => sectionVisible(s.id, s.label)).map((s) => (
          <a
            key={s.id}
            href={`#${s.id}`}
            className="shrink-0 rounded-full border border-transparent px-2.5 py-1 text-[11px] font-medium text-[var(--color-muted)] hover:border-[var(--color-border)] hover:bg-[var(--color-elevated)] hover:text-[var(--color-fg)]"
            onClick={(e) => {
              e.preventDefault();
              document.getElementById(s.id)?.scrollIntoView({ behavior: "smooth", block: "start" });
            }}
          >
            {s.label}
          </a>
        ))}
      </nav>

      
      <Card id="sec-autonomy">
        <CardHeader>
          <CardTitle className="text-sm">Autonomy & always-on agent</CardTitle>
          <CardDescription>
            Levels 2+ queue work when busy. Level 3+ claims workboard. Level 4 auto-resumes goals.
            Use tray Pause or the Queue tab kill switch anytime.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <AutonomySettingsPanel />
        </CardContent>
      </Card>

      <Card id="sec-appearance">
        <CardHeader>
          <CardTitle className="text-sm">Appearance</CardTitle>
          <CardDescription>Theme for the GrokHub chrome. Chat content stays high-contrast either way.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          {(
            [
              ["dark", "Dark", Moon],
              ["light", "Light", Sun],
              ["system", "System", Monitor],
            ] as const
          ).map(([id, label, Icon]) => (
            <button
              key={id}
              type="button"
              onClick={() => setUiTheme(id)}
              className={cn(
                "inline-flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border px-3 text-xs font-medium transition-colors",
                uiTheme === id
                  ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]"
                  : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-border-strong)]",
              )}
            >
              <Icon className="h-3.5 w-3.5" />
              {label}
            </button>
          ))}
        </CardContent>
      </Card>

      
      <Card id="sec-wizard" className="border-[color-mix(in_oklab,var(--color-info)_30%,var(--color-border))]">
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">First-run connect</CardTitle>
          <CardDescription>
            Recommended order: Grok OAuth (or free website / API key) → optional GitHub PAT → bind a project folder → install menu entry.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2 pt-0">
          <Button size="sm" onClick={() => document.getElementById("sec-oauth")?.scrollIntoView({ behavior: "smooth" })}>
            1. Grok OAuth
          </Button>
          <Button size="sm" variant="secondary" onClick={() => document.getElementById("sec-free")?.scrollIntoView({ behavior: "smooth" })}>
            2. Free website
          </Button>
          <Button size="sm" variant="secondary" onClick={() => document.getElementById("sec-api")?.scrollIntoView({ behavior: "smooth" })}>
            3. API key
          </Button>
          <Button size="sm" variant="secondary" onClick={() => document.getElementById("sec-project")?.scrollIntoView({ behavior: "smooth" })}>
            4. Project folder
          </Button>
        </CardContent>
      </Card>

{/* Primary: real xAI Grok OAuth */}
      <Card id="sec-oauth">
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


      <Card id="sec-free">
        <CardHeader>
          <CardTitle className="text-sm">Free Grok fallback</CardTitle>
          <CardDescription>
            No SuperGrok? GrokHub can use free-tier models (when an API key has trial credits)
            and/or your free grok.com website session. Paid OAuth/API still falls back to free
            models if a premium model is denied.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant={usagePlan === "free" ? "info" : "default"}>
              Plan: {usagePlan === "free" ? "Free" : usagePlan === "super" ? "SuperGrok" : "Pro"}
            </Badge>
            <Button size="sm" variant="secondary" onClick={() => setPlan("free")}>
              Use Free plan limits
            </Button>
            <Button size="sm" variant="secondary" onClick={() => setPlan("super")}>
              SuperGrok limits
            </Button>
          </div>
          <label className="flex cursor-pointer items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3">
            <div>
              <div className="text-sm font-medium">Allow free website fallback</div>
              <div className="text-xs text-[var(--color-muted)]">
                If API/OAuth fails or is missing, try chat via linked grok.com session
              </div>
            </div>
            <input
              type="checkbox"
              className="h-4 w-4 accent-[var(--color-fg)]"
              checked={preferFreeGrok !== false}
              onChange={(e) => setPreferFreeGrok(e.target.checked)}
            />
          </label>
          <p className="text-[11px] leading-relaxed text-[var(--color-subtle)]">
            1) <strong className="text-[var(--color-muted)]">Link Grok website</strong> (free account
            works) for website free chat · 2) optional free{" "}
            <span className="font-mono">console.x.ai</span> API key with trial credits · 3) SuperGrok
            OAuth when you upgrade.
          </p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-sm">
            <RefreshCw className="h-4 w-4 text-[var(--color-muted)]" />
            Setup sync (Grok account)
          </CardTitle>
          <CardDescription>
            Key setup to your Grok OAuth sign-in. On login we pull profile, models, website
            connectors, and usage. Optionally push/pull full app setup (skills, automations,
            desktop prefs, connector layout) — never tokens or API keys.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 py-2 text-xs text-[var(--color-muted)]">
            <div>
              Account:{" "}
              <span className="font-medium text-[var(--color-fg)]">
                {oauth?.email || oauth?.name || "Sign in with Grok OAuth"}
              </span>
            </div>
            {setupSyncMeta?.lastDetail && (
              <div className="mt-1 truncate">Last: {setupSyncMeta.lastDetail}</div>
            )}
            {setupSyncMeta?.lastPushAt ? (
              <div className="mt-0.5">
                Pushed {new Date(setupSyncMeta.lastPushAt).toLocaleString()}
              </div>
            ) : null}
            {setupSyncMeta?.lastPullAt ? (
              <div className="mt-0.5">
                Pulled {new Date(setupSyncMeta.lastPullAt).toLocaleString()}
              </div>
            ) : null}
          </div>

          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              disabled={!oauth || setupBusy}
              onClick={() => {
                setSetupBusy(true);
                setSetupMsg("");
                void syncSetupWithGrokAccount(
                  setupPass.trim() ? { passphrase: setupPass } : undefined,
                ).then((r) => {
                  setSetupBusy(false);
                  setSetupMsg(r.detail);
                });
              }}
            >
              Sync from Grok now
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={!oauth || setupBusy}
              onClick={() => {
                setSetupBusy(true);
                void pushSetupSync(
                  setupPass.trim() ? { passphrase: setupPass } : undefined,
                ).then((r) => {
                  setSetupBusy(false);
                  setSetupMsg(r.detail);
                });
              }}
            >
              Push setup
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={!oauth || setupBusy}
              onClick={() => {
                setSetupBusy(true);
                void pullSetupSync(
                  setupPass.trim() ? { passphrase: setupPass } : undefined,
                ).then((r) => {
                  setSetupBusy(false);
                  setSetupMsg(r.detail);
                });
              }}
            >
              Pull setup
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={setupBusy}
              onClick={() => {
                setSetupBusy(true);
                void exportSetupPackJson(
                  setupPass.trim() ? { passphrase: setupPass } : undefined,
                ).then((json) => {
                  const blob = new Blob([json], { type: "application/json" });
                  const url = URL.createObjectURL(blob);
                  const a = document.createElement("a");
                  a.href = url;
                  a.download = `grokhub-setup-${oauth?.email || "local"}-${new Date()
                    .toISOString()
                    .slice(0, 10)}.json`;
                  a.click();
                  URL.revokeObjectURL(url);
                  setSetupMsg(
                    setupPass.trim()
                      ? "Encrypted setup pack exported"
                      : "Setup pack exported (no secrets)",
                  );
                  setSetupBusy(false);
                });
              }}
            >
              Export pack
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={setupBusy}
              onClick={() => setupImportRef.current?.click()}
            >
              Import pack
            </Button>
            <input
              ref={setupImportRef}
              type="file"
              accept="application/json,.json"
              className="hidden"
              onChange={(e) => {
                const file = e.target.files?.[0];
                if (!file) return;
                const reader = new FileReader();
                reader.onload = () => {
                  void importSetupPackJson(
                    String(reader.result || ""),
                    setupPass.trim() ? { passphrase: setupPass } : undefined,
                  ).then((r) => setSetupMsg(r.detail));
                };
                reader.readAsText(file);
                e.target.value = "";
              }}
            />
          </div>

          <div className="space-y-1.5">
            <label className="text-xs font-medium text-[var(--color-muted)]">
              Optional pack passphrase (encrypt push/export · decrypt pull/import)
            </label>
            <Input
              type="password"
              value={setupPass}
              onChange={(e) => setSetupPass(e.target.value)}
              placeholder="Leave empty for plain setup packs"
              className="font-mono text-xs"
              autoComplete="new-password"
            />
          </div>

          <label className="flex cursor-pointer items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3">
            <div>
              <div className="text-sm font-medium">Auto-push setup when things change</div>
              <div className="text-xs text-[var(--color-muted)]">
                Debounced push after automations / desktop prefs change (needs OAuth)
              </div>
            </div>
            <input
              type="checkbox"
              className="h-4 w-4 accent-[var(--color-fg)]"
              checked={Boolean(setupSyncMeta?.autoPushOnChange)}
              onChange={(e) =>
                useGrokHub.getState().setSetupSyncMeta({
                  autoPushOnChange: e.target.checked,
                })
              }
            />
          </label>

          <label className="flex cursor-pointer items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3">
            <div>
              <div className="text-sm font-medium">Auto-pull setup on OAuth login</div>
              <div className="text-xs text-[var(--color-muted)]">
                After Grok sign-in, restore this account’s setup pack if one exists
              </div>
            </div>
            <input
              type="checkbox"
              className="h-4 w-4 accent-[var(--color-fg)]"
              checked={setupSyncMeta?.autoPullOnLogin !== false}
              onChange={(e) =>
                useGrokHub.getState().setSetupSyncMeta({
                  autoPullOnLogin: e.target.checked,
                })
              }
            />
          </label>

          <p className="text-[11px] leading-relaxed text-[var(--color-subtle)]">
            <strong className="text-[var(--color-muted)]">Cross-device:</strong> add a GitHub
            token below — Push stores a private Gist keyed to your Grok email. Sign in with the
            same Grok account on another machine and Pull (or auto-pull on login). Without GitHub,
            Push still saves an account vault on this PC. Connector <em>OAuth for Gmail/Notion
            etc.</em> still lives on grok.com — link the website session for those statuses.
          </p>
          {setupMsg && <p className="text-xs text-[var(--color-muted)]">{setupMsg}</p>}
        </CardContent>
      </Card>

      <div id="sec-usage">
      <UsageMeterPanel />
      </div>

      <Card id="sec-setup">
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

      <Card id="sec-api">
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

      <Card id="sec-models">
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
                ["smart", "Brains / Think"],
                ["heavy", "Heavy · Max flagship"],
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

      <Card id="sec-model-overrides">
        <CardHeader>
          <div className="flex flex-wrap items-start justify-between gap-2">
            <div>
              <CardTitle className="text-sm">Mode model overrides</CardTitle>
              <CardDescription>
                Pin a model per mode when auto-classification is wrong. Unset modes keep
                auto-selected models from Refresh + reclassify.
              </CardDescription>
            </div>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => clearModelOverrides()}
              disabled={!Object.keys(modelOverrides || {}).length}
            >
              Clear all pins
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-3">
          {(() => {
            const live = Array.from(
              new Set([
                ...(modelCatalog.all || []),
                ...(modelCatalog.essential || []),
                ...Object.values(modelCatalog.slots || {}),
              ]),
            )
              .filter(Boolean)
              .sort((a, b) => a.localeCompare(b));
            const modes = getModesWithCatalog(modelCatalog, modelOverrides);
            const labels: Record<string, string> = {
              fast: "Fast",
              balanced: "Balanced",
              expert: "Expert / Think",
              heavy: "Heavy",
              max: "Max",
              build: "Build",
            };
            return OVERRIDABLE_MODES.map((id) => {
              const m = modes.find((x) => x.id === id)!;
              const pinned = hasModelOverride(modelOverrides, id);
              const value = pinned
                ? String(modelOverrides[id as keyof ModelModeOverrides] || "")
                : "";
              const inList = value && live.includes(value);
              return (
                <div
                  key={id}
                  className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2.5"
                >
                  <div className="mb-1.5 flex flex-wrap items-center justify-between gap-2">
                    <div>
                      <div className="text-sm font-medium text-[var(--color-fg)]">
                        {labels[id] || id}
                        {pinned ? (
                          <Badge className="ml-2" variant="info">
                            Pinned
                          </Badge>
                        ) : (
                          <Badge className="ml-2" variant="default">
                            Auto
                          </Badge>
                        )}
                      </div>
                      <div className="text-[10px] text-[var(--color-muted)]">
                        Auto: {friendlyModelName(m.modelId)}
                        {!pinned ? ` · ${m.modelId}` : ""}
                      </div>
                    </div>
                    {pinned && (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => setModelOverride(id, null)}
                      >
                        Use auto
                      </Button>
                    )}
                  </div>
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                    <select
                      className="h-9 w-full flex-1 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-bg)] px-2 text-xs text-[var(--color-fg)]"
                      value={pinned && inList ? value : pinned ? "__custom__" : ""}
                      onChange={(e) => {
                        const v = e.target.value;
                        if (!v) setModelOverride(id, null);
                        else if (v === "__custom__") {
                          /* keep current custom text */
                        } else setModelOverride(id, v);
                      }}
                    >
                      <option value="">Auto (catalog)</option>
                      {live.map((mid) => (
                        <option key={mid} value={mid}>
                          {friendlyModelName(mid)} — {mid}
                        </option>
                      ))}
                      {pinned && !inList ? (
                        <option value="__custom__">Custom: {value}</option>
                      ) : (
                        <option value="__custom__">Custom id…</option>
                      )}
                    </select>
                    <Input
                      className="font-mono text-xs sm:max-w-[240px]"
                      placeholder="custom model id"
                      value={pinned && !inList ? value : pinned ? value : ""}
                      onChange={(e) => {
                        const v = e.target.value.trim();
                        setModelOverride(id, v || null);
                      }}
                      onBlur={(e) => {
                        const v = e.target.value.trim();
                        if (!v) setModelOverride(id, null);
                      }}
                    />
                  </div>
                </div>
              );
            });
          })()}
          <p className="text-[10px] text-[var(--color-subtle)]">
            Adaptive still chooses Fast / Think / Deep / Build. When a routed mode has a pin,
            that pin is used; other modes stay on auto catalog slots.
          </p>
        </CardContent>
      </Card>

      <Card id="sec-updates">
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
                {update.writable === false && (
                  <div className="text-[11px] text-amber-400/90">
                    Install path is not writable by your user (e.g.{" "}
                    <span className="font-mono">/usr/lib/grokhub</span>). Updating will ask for admin
                    (pkexec) or fall back to{" "}
                    <span className="font-mono">~/.local/share/grokhub</span>.
                  </div>
                )}
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
            <Button
              variant="secondary"
              disabled={updateBusy}
              onClick={() => {
                setUpdateLog(
                  [
                    "Repair install (preserves chats & secrets):",
                    "",
                    "  git pull",
                    "  ./scripts/repair-install.sh",
                    "",
                    "Or: sudo ./scripts/install-arch.sh",
                    "Windows: .\\scripts\\install-windows.ps1",
                    "",
                    "This rebuilds .output and reinstalls the desktop shell only.",
                  ].join("\n"),
                );
              }}
            >
              Repair install help
            </Button>
            <Button disabled={updateBusy} onClick={() => void onInstallUpdate()}>
              {updateBusy
                ? "Installing…"
                : update?.updateAvailable
                  ? "Install latest"
                  : "Reinstall / repair"}
            </Button>
            <Button variant="secondary" disabled={updateBusy} onClick={() => void onRollback()}>
              Undo last update
            </Button>
            <Button variant="secondary" disabled={updateBusy} onClick={() => void onSelfTest()}>
              Self-test install
            </Button>
          </div>
          <p className="text-[11px] text-[var(--color-subtle)]">
            Updates are blocked while the agent is running. Previous install is kept as{" "}
            <span className="font-mono">.prev</span> for one-shot rollback.
          </p>
          {updateLog && (
            <pre className="scroll-panel max-h-48 whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] p-3 font-mono text-xs text-[var(--color-muted)]">
              {updateLog}
            </pre>
          )}
        </CardContent>
      </Card>

      <Card id="sec-modes">
        <CardHeader>
          <CardTitle className="text-sm">Model modes</CardTitle>
          <CardDescription>
            Adaptive scores complexity, code, and intent — then routes ⚡ Fast · ⚖️ Balanced · 🧠 Think · 🔬 Deep · 🛠️ Build · Imagine.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {getModesWithCatalog(modelCatalog, modelOverrides).map((m) => {
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

      <Card id="sec-agent">
        <CardHeader>
          <CardTitle className="text-sm">Agent controls</CardTitle>
          <CardDescription>
            Temperature, tools, and persistent memory notes (survive restarts).
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <AgentPrefsPanel />
        </CardContent>
      </Card>

      <HostGatewayBanner variant="card" onOpenDesktop={() => setNav("desktop")} />

      <Card id="sec-desktop">
        <CardHeader>
          <CardTitle className="text-sm">Arch Linux shell preferences</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          {(
            [
              ["wayland", "Prefer Wayland", "Ozone flags"],
              ["tray", "System tray", "Minimize to tray"],
              ["launchOnLogin", "Launch on login", "Writes ~/.config/autostart/grokhub.desktop"],
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
              [
                "selfModifyEnabled",
                "Allow self-modification",
                "Agent may edit install files (src/, desktop/, …). Use Factory reinstall if something breaks.",
              ],
              [
                "hostSafeMode",
                "Host safe mode",
                "Block dangerous shell patterns (rm -rf, sudo, pipe-to-shell, …)",
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
                onChange={(e) => {
                  const on = e.target.checked;
                  setDesktop({ [key]: on });
                  if (key === "launchOnLogin") {
                    void window.grokhubDesktop?.desktopEntry?.autostart(on);
                  }
                }}
              />
            </label>
          ))}
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="secondary"
              onClick={() => {
                void window.grokhubDesktop?.desktopEntry?.install().then((r) => {
                  setSelfMsg(
                    r?.ok
                      ? r.detail || `Menu entry installed: ${r.path}`
                      : r?.error || "Menu install needs the desktop app",
                  );
                });
              }}
            >
              Install app menu entry
            </Button>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => {
                void window.grokhubDesktop?.desktopEntry?.status().then((r) => {
                  setSelfMsg(
                    r?.ok
                      ? `Menu: ${r.menuInstalled ? "yes" : "no"} · Autostart: ${
                          r.autostartInstalled ? "yes" : "no"
                        } · exec ${r.exec || "?"}`
                      : "Status unavailable outside desktop",
                  );
                });
              }}
            >
              Check menu status
            </Button>
          </div>
          <p className="text-[11px] leading-relaxed text-[var(--color-subtle)]">
            <strong className="text-[var(--color-muted)]">Taskbar pin:</strong> install the menu
            entry, then pin <em>GrokHub</em> from the app launcher — not a generic Electron icon.
            Pins use <span className="font-mono">/usr/bin/grokhub</span> so they still work after
            you quit. Window class / app id is <span className="font-mono">grokhub</span> (must
            match the desktop file). After updating, unpin + re-pin once if you still see a second
            icon.
          </p>
        </CardContent>
      </Card>

      <Card id="sec-selfmod">
        <CardHeader>
          <CardTitle className="text-sm">Self-mod & factory restore</CardTitle>
          <CardDescription>
            The agent can change GrokHub’s install files when self-modification is enabled. Local
            snapshots and a full GitHub factory reinstall let you roll back. Your chats live in user
            data and survive code reinstall unless you wipe memory.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {selfInfo?.ok && (
            <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 py-2 font-mono text-[11px] text-[var(--color-muted)]">
              <div className="truncate">install: {selfInfo.root || "—"}</div>
              <div className="truncate">snapshots: {selfInfo.selfModDir || "—"}</div>
              <div>
                saved points: {(selfInfo.snapshots || []).length}
                {desktop.selfModifyEnabled ? " · self-mod ON" : " · self-mod OFF"}
              </div>
            </div>
          )}
          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              size="sm"
              disabled={selfBusy}
              onClick={() => {
                setSelfBusy(true);
                setSelfMsg("Creating snapshot…");
                void selfModSnapshot("manual settings").then((r) => {
                  setSelfBusy(false);
                  setSelfMsg(
                    r.ok
                      ? `Snapshot ${(r as { id?: string }).id} (${(r as { fileCount?: number }).fileCount || 0} files)`
                      : (r as { error?: string }).error || "Snapshot failed",
                  );
                  void selfModInfo().then(setSelfInfo);
                });
              }}
            >
              Snapshot install tree
            </Button>
            <Button
              variant="secondary"
              size="sm"
              disabled={selfBusy || !(selfInfo?.snapshots && selfInfo.snapshots[0])}
              onClick={() => {
                const id = selfInfo?.snapshots?.[0]?.id;
                if (!id) return;
                if (
                  !window.confirm(
                    `Restore snapshot ${id}? App code will be rolled back; restart after.`,
                  )
                )
                  return;
                setSelfBusy(true);
                void selfModRestore(id).then((r) => {
                  setSelfBusy(false);
                  setSelfMsg(
                    r.ok
                      ? "Snapshot restored — restart GrokHub to load it"
                      : (r as { error?: string }).error || "Restore failed",
                  );
                });
              }}
            >
              Restore latest snapshot
            </Button>
            <Button
              variant="secondary"
              size="sm"
              disabled={selfBusy}
              onClick={() => {
                if (
                  !window.confirm(
                    "Factory reinstall from GitHub? Stock app code replaces local install. Chats/settings are KEPT.",
                  )
                )
                  return;
                setSelfBusy(true);
                setSelfMsg("Factory reinstall…");
                setUpdateLog("Factory reinstall from GitHub…\n");
                void factoryReinstall({ wipeMemory: false, clearSelfMod: true }).then((r) => {
                  setSelfBusy(false);
                  const steps = (r as { steps?: string[] }).steps || [];
                  setUpdateLog((prev) => prev + steps.join("\n") + "\n");
                  setSelfMsg(
                    (r as { ok?: boolean }).ok !== false
                      ? (r as { detail?: string }).detail || "Factory reinstall done"
                      : (r as { error?: string }).error || "Factory reinstall failed",
                  );
                });
              }}
            >
              Factory reinstall (keep memory)
            </Button>
            <div className="w-full space-y-2 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-danger)_35%,var(--color-border))] p-2">
              <p className="text-[11px] text-[var(--color-muted)]">
                Type <span className="font-mono text-[var(--color-danger)]">WIPE MEMORY</span> to enable full factory reset.
              </p>
              <Input
                value={factoryPhrase}
                onChange={(e) => setFactoryPhrase(e.target.value)}
                placeholder="WIPE MEMORY"
                className="h-8 font-mono text-xs"
              />
              <Button
                variant="danger"
                size="sm"
                disabled={selfBusy || factoryPhrase.trim() !== "WIPE MEMORY"}
                onClick={() => {
                  if (
                    !window.confirm(
                      "FULL factory reset: reinstall from GitHub AND wipe chats, secrets, and local memory?",
                    )
                  )
                    return;
                  setSelfBusy(true);
                  setSelfMsg("Full factory wipe…");
                  void factoryReinstall({ wipeMemory: true, clearSelfMod: true }).then((r) => {
                    setSelfBusy(false);
                    setSelfMsg(
                      (r as { ok?: boolean }).ok !== false
                        ? "Full factory reset done"
                        : (r as { error?: string }).error || "Failed",
                    );
                    if ((r as { ok?: boolean }).ok !== false) {
                      resetDemo();
                    }
                    setFactoryPhrase("");
                  });
                }}
              >
                Factory + wipe memory
              </Button>
            </div>
          </div>
          {selfMsg && <p className="text-xs text-[var(--color-muted)]">{selfMsg}</p>}
          {(selfInfo?.snapshots || []).slice(0, 5).map((s) => (
            <div
              key={s.id}
              className="flex items-center justify-between gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] px-2 py-1.5 text-[11px]"
            >
              <span className="truncate font-mono text-[var(--color-muted)]">
                {s.id}
                {s.note ? ` · ${s.note}` : ""}
              </span>
              <Button
                size="sm"
                variant="ghost"
                className="h-7 shrink-0 text-xs"
                disabled={selfBusy}
                onClick={() => {
                  if (!window.confirm(`Restore ${s.id}?`)) return;
                  setSelfBusy(true);
                  void selfModRestore(s.id).then((r) => {
                    setSelfBusy(false);
                    setSelfMsg(r.ok ? "Restored — restart app" : "Restore failed");
                  });
                }}
              >
                Restore
              </Button>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card id="sec-memory">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-sm">
            <HardDrive className="h-4 w-4 text-[var(--color-muted)]" />
            Memory
          </CardTitle>
          <CardDescription>
            File memory (USER.md, MEMORY.md, daily notes) lives under your user data folder and is
            pinned into every chat under a budget. App state backup is separate. Updates never wipe
            this folder.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <FileMemoryPanel />
          {memInfo && (
            <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 py-2 font-mono text-[11px] text-[var(--color-muted)]">
              <div className="mb-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-subtle)]">
                App state backup
              </div>
              <div className="truncate">path: {memInfo.path || "—"}</div>
              {memInfo.userData && (
                <div className="truncate">userData: {memInfo.userData}</div>
              )}
              <div>
                size:{" "}
                {typeof memInfo.bytes === "number"
                  ? `${(memInfo.bytes / 1024).toFixed(1)} KB`
                  : "—"}
                {memInfo.updatedAt
                  ? ` · saved ${new Date(memInfo.updatedAt).toLocaleString()}`
                  : ""}
              </div>
            </div>
          )}
          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => {
                void exportMemory().then((r) => {
                  if (!r.ok || !r.json) {
                    setMemMsg(r.error || "Export failed");
                    return;
                  }
                  const blob = new Blob([r.json], { type: "application/json" });
                  const url = URL.createObjectURL(blob);
                  const a = document.createElement("a");
                  a.href = url;
                  a.download = `grokhub-memory-${new Date().toISOString().slice(0, 10)}.json`;
                  a.click();
                  URL.revokeObjectURL(url);
                  setMemMsg("App state exported");
                  void memoryInfo().then(setMemInfo);
                });
              }}
            >
              Export backup
            </Button>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => importRef.current?.click()}
            >
              Import backup
            </Button>
            <input
              ref={importRef}
              type="file"
              accept="application/json,.json"
              className="hidden"
              onChange={(e) => {
                const file = e.target.files?.[0];
                if (!file) return;
                const reader = new FileReader();
                reader.onload = () => {
                  void importMemory(String(reader.result || "")).then((r) => {
                    setMemMsg(
                      r.ok
                        ? "Import OK — reload the app to apply"
                        : r.error || "Import failed",
                    );
                    if (r.ok) {
                      void useGrokHub.persist.rehydrate();
                      void memoryInfo().then(setMemInfo);
                    }
                  });
                };
                reader.readAsText(file);
                e.target.value = "";
              }}
            />
          </div>
          {memMsg && <p className="text-xs text-[var(--color-muted)]">{memMsg}</p>}
        </CardContent>
      </Card>

      <Card id="sec-project">
        <CardHeader>
          <CardTitle className="text-sm">Project workspace</CardTitle>
          <CardDescription>
            Bind a local folder so the agent prefers HOST_CMD and file work under that tree.
            Summary is refreshed on bind (README, package.json, AGENTS.md when present).
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ProjectWorkspacePanel />
        </CardContent>
      </Card>

      <Card id="sec-learning">
        <CardHeader>
          <CardTitle className="text-sm">Learning & self-improvement</CardTitle>
          <CardDescription>
            GrokHub learns from successful turns, your 👍/👎 on replies, and explicit prefs. Insights
            pin into chat context and gently bias Adaptive routing. Reflect writes LEARNINGS.md.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <LearningPanel />
        </CardContent>
      </Card>

      <Card id="sec-diagnostics">
        <CardHeader>
          <CardTitle className="text-sm">Diagnostics</CardTitle>
          <CardDescription>
            Copy a support bundle (version, host, learning/workboard counts) for crash reports.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <DiagnosticsPanel />
        </CardContent>
      </Card>

      <Card id="sec-danger">
        <CardHeader>
          <CardTitle className="text-sm">Danger zone</CardTitle>
          <CardDescription>
            Wipe local chat history, connectors, and preferences on this device. Does not revoke
            Grok OAuth on xAI servers — disconnect first if you want a full sign-out.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
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
          <Button
            variant="secondary"
            onClick={() => {
              if (
                typeof window !== "undefined" &&
                window.confirm("Clear learned quick-assist habits?")
              ) {
                clearQuickAssistMemory();
              }
            }}
          >
            Clear chip habits
            {quickAssistMemory.hits.length
              ? ` (${quickAssistMemory.hits.length})`
              : ""}
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}


function ProjectWorkspacePanel() {
  const project = useGrokHub((s) => s.projectWorkspace);
  const bind = useGrokHub((s) => s.bindProjectWorkspace);
  const clear = useGrokHub((s) => s.clearProjectWorkspace);
  const [path, setPath] = useState(project?.path || "");
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  return (
    <div className="space-y-2">
      {project ? (
        <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 py-2 text-xs">
          <div className="font-medium text-[var(--color-fg)]">{project.name}</div>
          <div className="truncate font-mono text-[10px] text-[var(--color-muted)]">{project.path}</div>
          <div className="mt-1 text-[10px] text-[var(--color-subtle)]">
            Bound {new Date(project.boundAt).toLocaleString()}
          </div>
        </div>
      ) : (
        <p className="text-xs text-[var(--color-muted)]">No project bound.</p>
      )}
      <div className="flex flex-wrap gap-2">
        <input
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder="/home/you/projects/app"
          className="min-w-[16rem] flex-1 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-1.5 font-mono text-xs"
        />
        <Button
          size="sm"
          disabled={busy || !path.trim()}
          onClick={() => {
            setBusy(true);
            void bind(path.trim())
              .then((r) => setMsg(r.detail))
              .finally(() => setBusy(false));
          }}
        >
          {busy ? "Binding…" : "Bind"}
        </Button>
        {project ? (
          <Button size="sm" variant="ghost" onClick={() => clear()}>
            Clear
          </Button>
        ) : null}
      </div>
      {msg ? <p className="text-[11px] text-[var(--color-muted)]">{msg}</p> : null}
    </div>
  );
}

function DiagnosticsPanel() {
  const exportDiagnostics = useGrokHub((s) => s.exportDiagnostics);
  const [msg, setMsg] = useState<string | null>(null);
  return (
    <div className="space-y-2">
      <Button
        size="sm"
        variant="secondary"
        onClick={() => {
          void exportDiagnostics().then((r) => {
            setMsg(r.ok ? "Copied diagnostics JSON to clipboard" : r.error || "Failed");
          });
        }}
      >
        Copy diagnostics
      </Button>
      {msg ? <p className="text-[11px] text-[var(--color-muted)]">{msg}</p> : null}
    </div>
  );
}

function LearningPanel() {
  const learning = useGrokHub((s) => s.learning);
  const runSelfImprove = useGrokHub((s) => s.runSelfImprove);
  const clearLearning = useGrokHub((s) => s.clearLearning);
  const flushLearningToDisk = useGrokHub((s) => s.flushLearningToDisk);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [memRoot, setMemRoot] = useState<string>("");
  useEffect(() => {
    void import("@/lib/file-memory").then(({ memoryFsInfo, ensureFileMemory }) => {
      void ensureFileMemory().then(() =>
        memoryFsInfo().then((i) => {
          if (i.root) setMemRoot(i.root);
        }),
      );
    });
    void flushLearningToDisk();
  }, [flushLearningToDisk]);
  const top = [...(learning?.insights || [])]
    .sort((a, b) => b.confidence * b.hits - a.confidence * a.hits)
    .slice(0, 8);
  const routes = Object.entries(learning?.routeStats || {});

  return (
    <div className="space-y-3">
      <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 py-2 text-xs text-[var(--color-muted)]">
        {learningSummaryLine(learning || { insights: [], totalTurns: 0, totalFeedback: 0 } as never)}
        {memRoot ? (
          <div className="mt-1 truncate font-mono text-[10px] text-[var(--color-subtle)]" title={memRoot}>
            disk: {memRoot}
          </div>
        ) : null}
        {learning?.lastReflectionAt ? (
          <div className="mt-1 text-[10px] text-[var(--color-subtle)]">
            Last reflect · {new Date(learning.lastReflectionAt).toLocaleString()}
          </div>
        ) : null}
      </div>
      {routes.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {routes.map(([tier, st]) =>
            st ? (
              <span
                key={tier}
                className="rounded-full border border-[var(--color-border)] px-2 py-0.5 font-mono text-[10px] text-[var(--color-muted)]"
              >
                {tier} {st.success}↑ {st.fail}↓
              </span>
            ) : null,
          )}
        </div>
      )}
      {top.length > 0 ? (
        <ul className="space-y-1 text-xs text-[var(--color-fg)]">
          {top.map((i) => (
            <li key={i.id} className="flex gap-2">
              <span className="shrink-0 font-mono text-[10px] text-[var(--color-subtle)]">
                {Math.round(i.confidence * 100)}%
              </span>
              <span className="text-[var(--color-muted)]">{i.text}</span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-xs text-[var(--color-muted)]">
          No insights yet. Chat normally, rate replies, or run Reflect.
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            void runSelfImprove()
              .then((r) => setMsg(r.detail))
              .finally(() => setBusy(false));
          }}
        >
          {busy ? "Reflecting…" : "Reflect & improve"}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            if (typeof window !== "undefined" && window.confirm("Clear all learning history?")) {
              clearLearning();
              setMsg("Learning cleared");
            }
          }}
        >
          Clear learning
        </Button>
      </div>
      {msg ? <p className="text-[11px] text-[var(--color-muted)]">{msg}</p> : null}
      <p className="text-[11px] text-[var(--color-subtle)]">
        Chat: <span className="font-mono">/learn</span> ·{" "}
        <span className="font-mono">/learn reflect</span> ·{" "}
        <span className="font-mono">/learn note …</span>
      </p>
    </div>
  );
}

function FileMemoryPanel() {
  const [files, setFiles] = useState<MemoryFileInfo[]>([]);
  const [root, setRoot] = useState<string>("");
  const [active, setActive] = useState("MEMORY.md");
  const [body, setBody] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);

  const reload = useCallback(() => {
    void memoryFsInfo().then((info) => {
      if (info.root) setRoot(info.root);
    });
    void memoryList().then((list) => setFiles(list));
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  useEffect(() => {
    void memoryRead(active).then((r) => {
      setBody(r.content || "");
      setDirty(false);
    });
  }, [active]);

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="text-xs font-medium text-[var(--color-fg)]">File memory</div>
        {root ? (
          <div className="max-w-[min(100%,20rem)] truncate font-mono text-[10px] text-[var(--color-subtle)]" title={root}>
            {root}
          </div>
        ) : null}
      </div>
      <div className="flex flex-wrap gap-1">
        {(["USER.md", "MEMORY.md", "today"] as const).map((id) => (
          <button
            key={id}
            type="button"
            className={cn(
              "rounded-full border px-2.5 py-0.5 text-[11px] font-medium",
              active === id || (id === "today" && active.startsWith("daily/"))
                ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]"
                : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-border-strong)]",
            )}
            onClick={() => setActive(id === "today" ? "today" : id)}
          >
            {id === "today" ? "Today" : id}
          </button>
        ))}
        {files
          .filter((f) => f.kind === "daily")
          .slice(0, 5)
          .map((f) => (
            <button
              key={f.id}
              type="button"
              className={cn(
                "rounded-full border px-2 py-0.5 font-mono text-[10px]",
                active === f.id
                  ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)]"
                  : "border-[var(--color-border)] text-[var(--color-subtle)]",
              )}
              onClick={() => setActive(f.id)}
            >
              {f.name.replace(/\.md$/, "")}
            </button>
          ))}
      </div>
      <textarea
        value={body}
        onChange={(e) => {
          setBody(e.target.value);
          setDirty(true);
        }}
        rows={10}
        spellCheck={false}
        className="w-full rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] p-2 font-mono text-xs leading-relaxed"
        placeholder="# Memory…"
      />
      <div className="flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          disabled={!dirty}
          onClick={() => {
            void memoryWrite(active, body).then((r) => {
              setStatus(r.ok ? "Saved" : r.error || "Save failed");
              setDirty(!r.ok);
              reload();
            });
          }}
        >
          Save file
        </Button>
        <Button size="sm" variant="ghost" onClick={reload}>
          Reload
        </Button>
        {status ? (
          <span className="text-[11px] text-[var(--color-muted)]">{status}</span>
        ) : null}
      </div>
      <p className="text-[11px] text-[var(--color-muted)]">
        Chat: <span className="font-mono">/memory note</span> ·{" "}
        <span className="font-mono">/memory show</span> ·{" "}
        <span className="font-mono">/memory user …</span> ·{" "}
        <span className="font-mono">/memory today …</span>
      </p>
    </div>
  );
}

function AgentPrefsPanel() {
  const agentPrefs = useGrokHub((s) => s.agentPrefs);
  const setAgentPrefs = useGrokHub((s) => s.setAgentPrefs);
  const hostAllowlist = useGrokHub((s) => s.hostAllowlist);
  const removeHostAllow = useGrokHub((s) => s.removeHostAllow);
  const addHostAllow = useGrokHub((s) => s.addHostAllow);
  const [notes, setNotes] = useState(agentPrefs.memoryNotes || "");
  const [allowDraft, setAllowDraft] = useState("");
  return (
    <div className="space-y-4">
      <label className="block space-y-1.5">
        <div className="flex items-center justify-between text-sm">
          <span>Temperature</span>
          <span className="font-mono text-xs text-[var(--color-subtle)]">
            {agentPrefs.temperature.toFixed(2)}
          </span>
        </div>
        <input
          type="range"
          min={0}
          max={1}
          step={0.05}
          value={agentPrefs.temperature}
          onChange={(e) => setAgentPrefs({ temperature: Number(e.target.value) })}
          className="w-full accent-[var(--color-info)]"
        />
        <p className="text-[11px] text-[var(--color-muted)]">
          Lower = focused · Higher = creative
        </p>
      </label>
      <label className="flex cursor-pointer items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[var(--color-border)] px-3 py-3">
        <div>
          <div className="text-sm font-medium">Host tools (HOST_CMD)</div>
          <div className="text-xs text-[var(--color-muted)]">
            Let the agent run shell on this machine
          </div>
        </div>
        <input
          type="checkbox"
          className="h-4 w-4 accent-[var(--color-fg)]"
          checked={agentPrefs.hostToolsEnabled}
          onChange={(e) => setAgentPrefs({ hostToolsEnabled: e.target.checked })}
        />
      </label>
      <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] p-3 space-y-2">
        <div className="text-sm font-medium">Always-allow host prefixes</div>
        <p className="text-xs text-[var(--color-muted)]">
          Matching commands skip the approval prompt (still blocked by safe mode).
        </p>
        <div className="flex flex-wrap gap-1">
          {(hostAllowlist || []).map((p) => (
            <button
              key={p}
              type="button"
              className="rounded-full border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-0.5 font-mono text-[10px] hover:border-[var(--color-danger)]"
              title="Remove"
              onClick={() => removeHostAllow(p)}
            >
              {p} ×
            </button>
          ))}
          {!hostAllowlist?.length && (
            <span className="text-[11px] text-[var(--color-subtle)]">None yet</span>
          )}
        </div>
        <div className="flex gap-2">
          <Input
            value={allowDraft}
            onChange={(e) => setAllowDraft(e.target.value)}
            placeholder="e.g. ls or git status"
            className="h-8 font-mono text-xs"
          />
          <Button
            size="sm"
            variant="secondary"
            onClick={() => {
              if (allowDraft.trim()) {
                addHostAllow(allowDraft.trim());
                setAllowDraft("");
              }
            }}
          >
            Add
          </Button>
        </div>
      </div>
      <label className="flex cursor-pointer items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[var(--color-border)] px-3 py-3">
        <div>
          <div className="text-sm font-medium">Connector tools</div>
          <div className="text-xs text-[var(--color-muted)]">
            GitHub and website connector commands
          </div>
        </div>
        <input
          type="checkbox"
          className="h-4 w-4 accent-[var(--color-fg)]"
          checked={agentPrefs.connectorToolsEnabled}
          onChange={(e) => setAgentPrefs({ connectorToolsEnabled: e.target.checked })}
        />
      </label>
      <label className="block space-y-1.5">
        <div className="text-sm font-medium">Legacy sticky notes</div>
        <textarea
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          onBlur={() => setAgentPrefs({ memoryNotes: notes })}
          rows={3}
          placeholder="Also mirrored into MEMORY.md on next chat…"
          className="w-full rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] p-2 text-sm"
        />
        <p className="text-[11px] text-[var(--color-muted)]">
          Prefer Settings → Memory files or <span className="font-mono">/memory</span>
        </p>
      </label>
    </div>
  );
}


function AutonomySettingsPanel() {
  const autonomy = useGrokHub((s) => s.autonomy);
  const setAutonomy = useGrokHub((s) => s.setAutonomy);
  const pauseAutonomy = useGrokHub((s) => s.pauseAutonomy);
  const stats = useGrokHub((s) => s.agentQueue);
  const setNav = useGrokHub((s) => s.setNav);
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap gap-2">
        {([0, 1, 2, 3, 4] as const).map((lv) => (
          <button
            key={lv}
            type="button"
            onClick={() => setAutonomy({ level: lv })}
            className={
              "rounded-full border px-3 py-1 text-xs font-medium " +
              (autonomy.level === lv
                ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)]"
                : "border-[var(--color-border)] text-[var(--color-muted)]")
            }
          >
            {lv}
          </button>
        ))}
      </div>
      <label className="flex items-center justify-between gap-3 text-sm">
        <span>Pause all autonomy</span>
        <input
          type="checkbox"
          checked={autonomy.paused}
          onChange={(e) => pauseAutonomy(e.target.checked)}
        />
      </label>
      <label className="flex items-center justify-between gap-3 text-sm">
        <span>Auto-claim workboard (level 3+)</span>
        <input
          type="checkbox"
          checked={autonomy.autoClaimWorkboard}
          onChange={(e) => setAutonomy({ autoClaimWorkboard: e.target.checked })}
        />
      </label>
      <label className="flex items-center justify-between gap-3 text-sm">
        <span>Auto goal resume (level 4)</span>
        <input
          type="checkbox"
          checked={autonomy.autoGoalResume}
          onChange={(e) => setAutonomy({ autoGoalResume: e.target.checked })}
        />
      </label>
      <label className="block space-y-1 text-sm">
        <span>Daily unit budget (0 = unlimited)</span>
        <Input
          type="number"
          min={0}
          value={autonomy.dailyUnitBudget}
          onChange={(e) => setAutonomy({ dailyUnitBudget: Number(e.target.value) || 0 })}
          className="h-8"
        />
      </label>
      <p className="text-xs text-[var(--color-muted)]">
        Queue depth {stats.jobs.length} · spent today {autonomy.spentUnitsToday}
        {autonomy.dailyUnitBudget ? ` / ${autonomy.dailyUnitBudget}` : ""}
      </p>
      <Button size="sm" variant="secondary" onClick={() => setNav("queue")}>
        Open agent queue
      </Button>
      <p className="text-[11px] text-[var(--color-subtle)]">
        Arch always-on: enable packaging/systemd/grokhub-agent.service (user unit) or launch with{" "}
        <span className="font-mono">grokhub --agent</span>.
      </p>
    </div>
  );
}

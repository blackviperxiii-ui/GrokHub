import { useEffect, useRef, useState } from "react";
import {
  ExternalLink,
  FolderInput,
  Moon,
  RefreshCw,
  Sun,
  Monitor,
  UserRound,
  AppWindow,
  Search,
} from "lucide-react";

const SETTINGS_CATEGORIES = [
  {
    id: "account",
    label: "Account",
    hint: "Sign in to Grok",
    sections: ["sec-wizard", "sec-oauth", "sec-setup-sync", "sec-setup", "sec-api"],
  },
  {
    id: "devices",
    label: "Devices",
    hint: "Phones and other computers",
    sections: ["sec-hub", "sec-hub-join", "sec-hub-sync"],
  },
  {
    id: "app",
    label: "App",
    hint: "Theme, updates, reset",
    sections: ["sec-appearance", "sec-updates", "sec-diagnostics", "sec-danger"],
  },
] as const;

const SECTION_CAT: Record<string, (typeof SETTINGS_CATEGORIES)[number]["id"]> = {
  "sec-wizard": "account",
  "sec-oauth": "account",
  "sec-setup-sync": "account",
  "sec-setup": "account",
  "sec-api": "account",
  "sec-hub": "devices",
  "sec-hub-join": "devices",
  "sec-hub-sync": "devices",
  "sec-appearance": "app",
  "sec-updates": "app",
  "sec-diagnostics": "app",
  "sec-danger": "app",
};

const SECTION_SEARCH: Record<string, string> = {
  "sec-wizard": "first-run connect welcome setup",
  "sec-oauth": "oauth login sign in grok xai super",
  "sec-setup-sync": "setup sync grok account profile models skills",
  "sec-setup": "sync pack export import",
  "sec-api": "api key token xai",
  "sec-hub": "devices pair hub share lan sync remote computer",
  "sec-hub-join": "join pair code address wifi",
  "sec-hub-sync": "sync history memory remote task send",
  "sec-appearance": "theme dark light appearance",
  "sec-updates": "update github release install",
  "sec-diagnostics": "diagnostics debug export",
  "sec-danger": "danger reset wipe clean",
};

import { applyUpdate, checkUpdate, applyRollback, postUpdateSelfTest } from "@/lib/grok-client";
import { useCurrentUserState } from "@/lib/auth/use-current-user";
import { startGrokOAuthAndOpenBrowser } from "@/lib/begin-grok-oauth";
import {
  settingsSectionEventName,
  takePendingSettingsSection,
  type SettingsCat,
} from "@/lib/settings-nav";
import { resolveSettingsCat } from "@/lib/locked-settings";
import { useGrokHub } from "@/lib/store";
import type { UpdateStatus } from "@/lib/update";
import { cn } from "@/lib/utils";
import { ProfileAvatar } from "../ProfileAvatar";
import { HostGatewayBanner } from "../HostGatewayBanner";
import { DevicesHubPanel } from "../DevicesHubPanel";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";

export function SettingsView() {
  const setNav = useGrokHub((s) => s.setNav);
  const resetDemo = useGrokHub((s) => s.resetDemo);
  const clearQuickAssistMemory = useGrokHub((s) => s.clearQuickAssistMemory);
  const quickAssistMemory = useGrokHub((s) => s.quickAssistMemory);
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
  const pollGrokOAuth = useGrokHub((s) => s.pollGrokOAuth);
  const clearGrokOAuth = useGrokHub((s) => s.clearGrokOAuth);
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
      await startGrokOAuthAndOpenBrowser();
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
  const [settingsCat, setSettingsCat] =
    useState<(typeof SETTINGS_CATEGORIES)[number]["id"]>("account");

  useEffect(() => {
    const apply = (intent: { cat: SettingsCat; sectionId?: string }) => {
      setSettingsCat(resolveSettingsCat(intent.cat));
      setSettingsQ("");
      if (!intent.sectionId) return;
      window.requestAnimationFrame(() => {
        document.getElementById(intent.sectionId!)?.scrollIntoView({
          behavior: "smooth",
          block: "start",
        });
      });
    };
    const pending = takePendingSettingsSection();
    if (pending) apply(pending);
    const onEvt = (e: Event) => {
      const d = (e as CustomEvent<{ cat?: SettingsCat; sectionId?: string }>).detail;
      if (d?.cat) apply({ cat: d.cat, sectionId: d.sectionId });
    };
    window.addEventListener(settingsSectionEventName(), onEvt);
    return () => window.removeEventListener(settingsSectionEventName(), onEvt);
  }, []);

  const qNorm = settingsQ.trim().toLowerCase();
  const searching = qNorm.length > 0;

  const sectionHit = (id: string) => {
    // First-run only when not connected (unless searching for it)
    if (
      id === "sec-wizard" &&
      (grokConnected || oauth?.accessToken || apiKey) &&
      !(searching && qNorm.includes("first"))
    ) {
      return false;
    }
    if (searching) {
      const blob = `${id} ${SECTION_SEARCH[id] || ""}`.toLowerCase();
      return blob.includes(qNorm);
    }
    // Category pane: only sections in active category
    return SECTION_CAT[id] === settingsCat;
  };

  const catMeta = SETTINGS_CATEGORIES.find((c) => c.id === settingsCat) || SETTINGS_CATEGORIES[0]!;
  const catIcon = (id: string) => {
    if (id === "account") return UserRound;
    if (id === "devices") return Monitor;
    return AppWindow;
  };

  return (
    <div className="content-readable mx-auto pb-10">
      <header className="mb-4 space-y-1">
        <h1 className="text-lg font-semibold tracking-tight text-[var(--color-fg)]">Settings</h1>
        <p className="text-sm text-[var(--color-muted)]">
          {searching ? `Matches for “${settingsQ.trim()}”` : catMeta.hint}
        </p>
      </header>

      <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center">
        <div className="relative min-w-0 flex-1">
          <Search
            className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--color-subtle)]"
            aria-hidden
          />
          <Input
            value={settingsQ}
            onChange={(e) => setSettingsQ(e.target.value)}
            placeholder="Search settings…"
            className="h-10 pl-8"
            aria-label="Search settings"
          />
        </div>
        {searching ? (
          <Button size="sm" variant="secondary" onClick={() => setSettingsQ("")}>
            Clear
          </Button>
        ) : null}
      </div>

      <div className="flex flex-col gap-4 lg:flex-row lg:items-start">
        <nav
          aria-label="Settings categories"
          className="flex shrink-0 gap-1.5 overflow-x-auto scroll-hide lg:sticky lg:top-2 lg:w-44 lg:flex-col lg:overflow-visible"
        >
          {SETTINGS_CATEGORIES.map((c) => {
            const Icon = catIcon(c.id);
            const active = !searching && settingsCat === c.id;
            const hits = searching
              ? c.sections.filter((id) => sectionHit(id)).length
              : 0;
            if (searching && hits === 0) return null;
            return (
              <button
                key={c.id}
                type="button"
                onClick={() => {
                  setSettingsCat(c.id);
                  setSettingsQ("");
                }}
                className={cn(
                  "flex min-h-10 shrink-0 items-center gap-2 rounded-[var(--radius-md)] border px-3 py-2 text-left transition-colors lg:w-full",
                  active
                    ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]"
                    : "border-transparent bg-[var(--color-surface)] text-[var(--color-muted)] hover:border-[var(--color-border)] hover:text-[var(--color-fg)]",
                )}
              >
                <Icon className="h-4 w-4 shrink-0 opacity-80" aria-hidden />
                <span className="min-w-0 flex-1">
                  <span className="block text-sm font-medium leading-tight">{c.label}</span>
                  <span className="hidden text-[10px] leading-snug text-[var(--color-subtle)] lg:block">
                    {searching ? `${hits} match${hits === 1 ? "" : "es"}` : c.hint}
                  </span>
                </span>
              </button>
            );
          })}
        </nav>

        <div
          className="settings-stack min-w-0 flex-1 space-y-4"
          data-mode={searching ? "search" : settingsCat}
        >
          <style>{`
            .settings-stack > [data-settings-cat] { display: none !important; }
            .settings-stack > [data-settings-cat][data-hit="1"] { display: flex !important; flex-direction: column; }
          `}</style>
      <Card id="sec-appearance" data-settings-cat="app" data-hit={sectionHit("sec-appearance") ? "1" : "0"}>
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

      
      <Card id="sec-wizard" data-settings-cat="account" data-hit={sectionHit("sec-wizard") ? "1" : "0"} className="border-[color-mix(in_oklab,var(--color-info)_30%,var(--color-border))]">
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Get started</CardTitle>
          <CardDescription>
            Sign in with Grok first. An API key is optional.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2 pt-0">
          <Button size="sm" onClick={() => document.getElementById("sec-oauth")?.scrollIntoView({ behavior: "smooth" })}>
            1. Connect Grok
          </Button>
          <Button size="sm" variant="secondary" onClick={() => document.getElementById("sec-api")?.scrollIntoView({ behavior: "smooth" })}>
            2. API key (optional)
          </Button>
        </CardContent>
      </Card>

{/* Primary: real xAI Grok OAuth */}
      <Card id="sec-oauth" data-settings-cat="account" data-hit={sectionHit("sec-oauth") ? "1" : "0"}>
        <CardHeader>
          <CardTitle className="text-sm">Connect to Grok (xAI OAuth)</CardTitle>
          <CardDescription>
            Sign in with SuperGrok or X Premium+. No API key required for subscription access.
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


      <Card id="sec-setup-sync" data-settings-cat="account" data-hit={sectionHit("sec-setup-sync") ? "1" : "0"}>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-sm">
            <RefreshCw className="h-4 w-4 text-[var(--color-muted)]" />
            Setup sync (Grok account)
          </CardTitle>
          <CardDescription>
            Key setup to your Grok OAuth sign-in. On login we pull profile and models. Optionally push/pull full app setup (skills, automations,
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
<Card id="sec-setup" data-settings-cat="account" data-hit={sectionHit("sec-setup") ? "1" : "0"}>
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

      <Card id="sec-api" data-settings-cat="account" data-hit={sectionHit("sec-api") ? "1" : "0"}>
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

      <DevicesHubPanel sectionHit={sectionHit} />

      <Card id="sec-updates" data-settings-cat="app" data-hit={sectionHit("sec-updates") ? "1" : "0"}>
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
                  <div className="text-[11px] text-[var(--color-muted)]">
                    Install path is not writable by your user (e.g.{" "}
                    <span className="font-mono">/usr/lib/grokhub</span>). Updating will ask for admin
                    (pkexec) or fall back to{" "}
                    <span className="font-mono">~/.local/lib/grokhub</span>.
                  </div>
                )}
                {Boolean((update as { dualInstall?: boolean }).dualInstall) && (
                  <div className="mt-2 rounded-[var(--radius-sm)] border border-[color-mix(in_oklab,var(--color-warn)_40%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-warn)_10%,transparent)] px-2 py-1.5 text-[11px] text-[var(--color-fg)]">
                    <div className="font-medium text-[var(--color-warn)]">Dual install detected</div>
                    <div className="mt-0.5 text-[var(--color-muted)]">
                      Active tree:{" "}
                      <span className="font-mono text-[10px]">
                        {(update as { installRoot?: string }).installRoot || "user"}
                      </span>
                      . A stale system package at{" "}
                      <span className="font-mono text-[10px]">/usr/lib/grokhub</span> is ignored.
                      Remove it when ready:
                    </div>
                    <pre className="mt-1 overflow-x-auto rounded bg-[var(--color-elevated)] px-2 py-1 font-mono text-[10px] text-[var(--color-muted)]">
                      sudo rm -rf /usr/lib/grokhub /usr/bin/grokhub
                    </pre>
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

      <div
        data-settings-cat="account"
        data-hit={sectionHit("sec-oauth") || settingsCat === "account" ? "1" : "0"}
      >
        <HostGatewayBanner variant="card" />
      </div>

      <Card id="sec-diagnostics" data-settings-cat="app" data-hit={sectionHit("sec-diagnostics") ? "1" : "0"}>
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

      <Card id="sec-danger" data-settings-cat="app" data-hit={sectionHit("sec-danger") ? "1" : "0"}>
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
      </div>
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

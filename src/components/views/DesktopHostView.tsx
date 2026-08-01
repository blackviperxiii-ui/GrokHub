import {
  AppWindow,
  ChevronRight,
  Folder,
  FolderOpen,
  Play,
  RefreshCw,
  ShieldAlert,
  Terminal,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { HostApp, HostExecResult, HostFileEntry, HostInfo } from "@/lib/host-types";
import { useGrokHub } from "@/lib/store";
import { cn } from "@/lib/utils";
import { HostGatewayBanner } from "../HostGatewayBanner";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";

type Tab = "cli" | "files" | "apps";

type HostApi = typeof import("@/lib/host-client");

const QUICK_CMDS = [
  "uname -a && whoami && pwd",
  "ls -la",
  "df -h | head -12",
  "ps aux --sort=-%mem | head -12",
  "env | sort | head -40",
];

export function DesktopHostView() {
  const recordUsage = useGrokHub((s) => s.recordUsage);
  const [api, setApi] = useState<HostApi | null>(null);
  const [tab, setTab] = useState<Tab>("cli");
  const [info, setInfo] = useState<HostInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const [cmd, setCmd] = useState("uname -a && whoami && pwd");
  const [cwd, setCwd] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<HostExecResult | null>(null);
  const [history, setHistory] = useState<string[]>([]);

  const [dirPath, setDirPath] = useState("");
  const [entries, setEntries] = useState<HostFileEntry[]>([]);
  const [filePreview, setFilePreview] = useState<{ path: string; content: string } | null>(null);

  const [apps, setApps] = useState<HostApp[]>([]);
  const [appQ, setAppQ] = useState("");
  const [isShell, setIsShell] = useState(false);
  const probed = useRef(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const mod = await import("@/lib/host-client");
        if (cancelled) return;
        setApi(mod);
        setIsShell(mod.isDesktopShell());
        setLoading(true);
        const i = await mod.hostInfo();
        if (cancelled) return;
        setInfo(i);
        setCwd(i.homedir || i.cwd);
        setDirPath(i.homedir || i.cwd);
        if (i.bridge === "none" || !i.unsandboxed) {
          setError(
            "Desktop host bridge is offline. Fully quit and relaunch GrokHub from the Arch package (Electron shell). Browser-only preview has limited host access.",
          );
        }
        if (!probed.current && i.bridge !== "none") {
          probed.current = true;
          try {
            const r = await mod.hostExec(
              "uname -a && whoami && pwd && echo --- && ls -la | head -20",
              i.homedir || i.cwd,
            );
            if (!cancelled) {
              setResult(r);
              setHistory(["uname -a && whoami && pwd && echo --- && ls -la | head -20"]);
              if (!r.ok) {
                setError(r.stderr || `probe exit ${r.code}`);
              }
            }
          } catch (e) {
            if (!cancelled) {
              setError(e instanceof Error ? e.message : "host probe failed");
            }
          }
        }
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : "host bridge failed");
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [recordUsage]);

  const refreshInfo = useCallback(async () => {
    if (!api) return;
    setLoading(true);
    setError(null);
    try {
      const i = await api.hostInfo();
      setInfo(i);
      setCwd((c) => c || i.cwd);
      setDirPath((p) => p || i.homedir || i.cwd);
    } catch (e) {
      setError(e instanceof Error ? e.message : "host bridge failed");
    } finally {
      setLoading(false);
    }
  }, [api]);

  async function runCmd(command?: string) {
    if (!api) return;
    const c = (command ?? cmd).trim();
    if (!c) return;
    const bill = recordUsage("host");
    if (!bill.ok) {
      setError("Subscription unit quota exceeded — reset period or switch plan in Settings.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const r = await api.hostExec(c, cwd || undefined);
      setResult(r);
      setHistory((h) => [c, ...h.filter((x) => x !== c)].slice(0, 12));
    } catch (e) {
      setError(e instanceof Error ? e.message : "exec failed");
    } finally {
      setBusy(false);
    }
  }

  async function loadDir(p?: string) {
    if (!api) return;
    setBusy(true);
    setError(null);
    setFilePreview(null);
    try {
      const res = await api.hostListDir(p || dirPath);
      setDirPath(res.path);
      setEntries(res.entries);
    } catch (e) {
      setError(e instanceof Error ? e.message : "list dir failed");
    } finally {
      setBusy(false);
    }
  }

  async function openEntry(e: HostFileEntry) {
    if (!api) return;
    if (e.isDir) {
      await loadDir(e.path);
      return;
    }
    setBusy(true);
    try {
      const f = await api.hostReadFile(e.path);
      setFilePreview({ path: f.path, content: f.content });
    } catch (err) {
      setError(err instanceof Error ? err.message : "read failed");
    } finally {
      setBusy(false);
    }
  }

  async function loadApps() {
    if (!api) return;
    setBusy(true);
    setError(null);
    try {
      setApps(await api.hostListApps());
    } catch (e) {
      setError(e instanceof Error ? e.message : "list apps failed");
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    if (!api) return;
    if (tab === "files" && entries.length === 0 && dirPath) void loadDir(dirPath);
    if (tab === "apps" && apps.length === 0) void loadApps();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, api]);

  const filteredApps = apps.filter((a) =>
    a.name.toLowerCase().includes(appQ.trim().toLowerCase()),
  );

  return (
    <div className="space-y-4">
      <HostGatewayBanner variant="card" />

      <Card>
        <CardHeader className="gap-2">
          <CardTitle className="flex items-center gap-2 text-sm">
            <ShieldAlert className="h-4 w-4 text-[var(--color-warn)]" />
            Desktop host · session
          </CardTitle>
          <CardDescription>
            Full host CLI / files / apps. Shell commands bill 0.25 units against your plan.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap items-center gap-2">
          {loading && <span className="text-xs text-[var(--color-subtle)]">Probing host…</span>}
          {info && (
            <>
              <Badge variant={info.unsandboxed ? "warn" : "default"}>
                {info.unsandboxed ? "unsandboxed" : "limited"}
              </Badge>
              <Badge variant="info">{info.bridge}</Badge>
              <Badge className="font-mono">
                {info.platform}/{info.arch}
              </Badge>
              <span className="text-xs text-[var(--color-muted)]">
                {info.user}@{info.hostname} · {info.shell}
              </span>
              {isShell && <Badge variant="success">Electron shell</Badge>}
            </>
          )}
          <Button
            size="sm"
            variant="secondary"
            className="ml-auto"
            onClick={() => void refreshInfo()}
            disabled={!api}
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Refresh
          </Button>
        </CardContent>
      </Card>

      {error && (
        <div className="rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-danger)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-danger)_10%,transparent)] px-3 py-2 text-sm text-[var(--color-danger)]">
          {error}
        </div>
      )}

      <div className="flex flex-wrap gap-1.5">
        {(
          [
            ["cli", "CLI", Terminal],
            ["files", "Files", FolderOpen],
            ["apps", "Apps", AppWindow],
          ] as const
        ).map(([id, label, Icon]) => (
          <button
            key={id}
            type="button"
            onClick={() => setTab(id)}
            className={cn(
              "inline-flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border px-3 text-sm",
              tab === id
                ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]"
                : "border-[var(--color-border)] text-[var(--color-muted)]",
            )}
          >
            <Icon className="h-3.5 w-3.5" />
            {label}
          </button>
        ))}
      </div>

      {tab === "cli" && (
        <Card>
          <CardHeader>
            <CardTitle className="text-sm">Shell</CardTitle>
            <CardDescription>Full command access · 0.25u per run</CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="grid gap-2 sm:grid-cols-[1fr_160px]">
              <Input
                value={cwd}
                onChange={(e) => setCwd(e.target.value)}
                placeholder="Working directory"
                className="font-mono text-xs"
              />
              <Button
                variant="secondary"
                disabled={!api || busy}
                onClick={() => void runCmd("pwd && ls -la")}
              >
                pwd + ls
              </Button>
            </div>
            <form
              className="flex gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void runCmd();
              }}
            >
              <Input
                value={cmd}
                onChange={(e) => setCmd(e.target.value)}
                placeholder="e.g. systemctl --user status · pacman -Q electron"
                className="font-mono text-xs"
                disabled={busy || !api}
              />
              <Button type="submit" disabled={busy || !api || !cmd.trim()}>
                <Play className="h-3.5 w-3.5" />
                Run
              </Button>
            </form>
            <div className="flex flex-wrap gap-1.5">
              {QUICK_CMDS.map((h) => (
                <button
                  key={h}
                  type="button"
                  className="rounded-full border border-[var(--color-border)] px-2 py-0.5 font-mono text-[10px] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]"
                  onClick={() => {
                    setCmd(h);
                    void runCmd(h);
                  }}
                >
                  {h.slice(0, 42)}
                  {h.length > 42 ? "…" : ""}
                </button>
              ))}
            </div>
            {result && (
              <div className="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[#0c0c0e]">
                <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-2 text-[10px] text-[var(--color-subtle)]">
                  <span className="font-mono">$ {result.command}</span>
                  <span>
                    exit {result.code ?? "?"} · {result.ms}ms
                  </span>
                </div>
                <pre className="max-h-[360px] overflow-auto p-3 font-mono text-xs leading-relaxed text-[var(--color-fg)] whitespace-pre-wrap">
                  {result.stdout || ""}
                  {result.stderr ? `\n[stderr]\n${result.stderr}` : ""}
                  {!result.stdout && !result.stderr ? "(no output)" : ""}
                </pre>
              </div>
            )}
          </CardContent>
        </Card>
      )}

      {tab === "files" && (
        <div className="grid gap-3 lg:grid-cols-[1fr_1fr]">
          <Card>
            <CardHeader className="gap-2">
              <CardTitle className="text-sm">Filesystem</CardTitle>
              <div className="flex gap-2">
                <Input
                  value={dirPath}
                  onChange={(e) => setDirPath(e.target.value)}
                  className="font-mono text-xs"
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void loadDir(dirPath);
                  }}
                />
                <Button size="sm" variant="secondary" disabled={!api} onClick={() => void loadDir(dirPath)}>
                  Open
                </Button>
              </div>
            </CardHeader>
            <CardContent>
              <ul className="max-h-[420px] space-y-0.5 overflow-auto">
                <li>
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-[var(--color-muted)] hover:bg-[var(--color-elevated)]"
                    onClick={() => {
                      const parent = dirPath.replace(/\/[^/]+\/?$/, "") || "/";
                      void loadDir(parent || "/");
                    }}
                  >
                    <Folder className="h-3.5 w-3.5" />
                    ..
                  </button>
                </li>
                {entries.map((e) => (
                  <li key={e.path}>
                    <button
                      type="button"
                      onClick={() => void openEntry(e)}
                      className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm hover:bg-[var(--color-elevated)]"
                    >
                      {e.isDir ? (
                        <FolderOpen className="h-3.5 w-3.5 text-[var(--color-muted)]" />
                      ) : (
                        <ChevronRight className="h-3.5 w-3.5 text-[var(--color-subtle)]" />
                      )}
                      <span className="min-w-0 flex-1 truncate">{e.name}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Preview</CardTitle>
              <CardDescription className="font-mono text-[10px]">
                {filePreview?.path || "Select a file"}
              </CardDescription>
            </CardHeader>
            <CardContent>
              <pre className="max-h-[420px] overflow-auto rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[#0c0c0e] p-3 font-mono text-xs text-[var(--color-muted)] whitespace-pre-wrap">
                {filePreview?.content || "—"}
              </pre>
            </CardContent>
          </Card>
        </div>
      )}

      {tab === "apps" && (
        <Card>
          <CardHeader className="gap-2 sm:flex-row sm:items-end sm:justify-between">
            <div>
              <CardTitle className="text-sm">Installed apps (.desktop)</CardTitle>
              <CardDescription>Launch with gtk-launch / xdg-open as your user</CardDescription>
            </div>
            <div className="flex gap-2">
              <Input
                value={appQ}
                onChange={(e) => setAppQ(e.target.value)}
                placeholder="Filter apps"
                className="sm:w-48"
              />
              <Button size="sm" variant="secondary" disabled={!api} onClick={() => void loadApps()}>
                Reload
              </Button>
            </div>
          </CardHeader>
          <CardContent>
            <div className="grid max-h-[480px] gap-2 overflow-auto sm:grid-cols-2 xl:grid-cols-3">
              {filteredApps.map((a) => (
                <div
                  key={a.id}
                  className="flex items-center justify-between gap-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2"
                >
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium">{a.name}</div>
                    <div className="truncate font-mono text-[10px] text-[var(--color-subtle)]">
                      {a.exec}
                    </div>
                  </div>
                  <Button
                    size="sm"
                    disabled={!api}
                    onClick={() =>
                      void api?.hostOpenApp({ desktopFile: a.desktopFile, exec: a.exec })
                    }
                  >
                    Open
                  </Button>
                </div>
              ))}
              {filteredApps.length === 0 && (
                <p className="text-sm text-[var(--color-muted)]">No .desktop apps found yet.</p>
              )}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

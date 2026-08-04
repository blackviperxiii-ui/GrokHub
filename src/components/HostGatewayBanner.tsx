import { HardDrive, Loader2, Plug, RefreshCw, ShieldAlert, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import type { HostInfo } from "@/lib/host-types";
import { useGrokHub } from "@/lib/store";
import { cn } from "@/lib/utils";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./ui/card";

type Props = {
  /** compact = thin strip; card = full settings/desktop panel */
  variant?: "compact" | "card";
  className?: string;
  onOpenDesktop?: () => void;
};

/**
 * Desktop host / gateway connector.
 * When the unsandboxed bridge is offline, this is the CTA to reconnect so Grok
 * can run shell, files, and apps on the user's machine.
 */
export function HostGatewayBanner({ variant = "card", className, onOpenDesktop }: Props) {
  const setNav = useGrokHub((s) => s.setNav);
  const connectConnector = useGrokHub((s) => s.connectConnector);
  const [info, setInfo] = useState<HostInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [isElectron, setIsElectron] = useState(false);

  const probe = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const mod = await import("@/lib/host-client");
      setIsElectron(mod.isDesktopShell());
      const i = await mod.hostInfo();
      setInfo(i);
      if (i.bridge !== "none" && i.unsandboxed) {
        // Keep connector roster in sync (never toggle off)
        useGrokHub.setState((s) => ({
          connectors: s.connectors.map((c) =>
            c.id === "desktop-host"
              ? { ...c, status: "connected" as const, lastUsed: Date.now() }
              : c,
          ),
        }));
      }
      return i;
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Host probe failed";
      setError(msg);
      setInfo(null);
      return null;
    } finally {
      setBusy(false);
    }
  }, [connectConnector]);

  useEffect(() => {
    void probe();
  }, [probe]);

  const online = Boolean(info && info.bridge !== "none" && info.unsandboxed);

  async function connectGateway() {
    setBusy(true);
    setError(null);
    try {
      const i = await probe();
      if (i && i.bridge !== "none" && i.unsandboxed) {
        useGrokHub.setState((s) => ({
          connectors: s.connectors.map((c) =>
            c.id === "desktop-host"
              ? { ...c, status: "connected" as const, lastUsed: Date.now() }
              : c,
          ),
        }));
        return;
      }
      // Still offline — guide the user
      if (typeof window !== "undefined" && window.grokhubDesktop) {
        setError(
          "Electron shell is present but host IPC failed. Fully quit GrokHub (tray too) and relaunch.",
        );
      } else {
        setError(
          "No desktop gateway in this window. Launch the Arch package: grokhub (Electron). Web-only preview cannot control your machine.",
        );
      }
    } finally {
      setBusy(false);
    }
  }

  if (variant === "compact") {
    if (online) return null; // quiet when healthy
    return (
      <div
        className={cn(
          "flex flex-wrap items-center justify-between gap-2 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-warn)_45%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_10%,transparent)] px-3 py-2",
          className,
        )}
      >
        <div className="flex min-w-0 items-center gap-2 text-xs text-[var(--color-warn)]">
          <ShieldAlert className="h-3.5 w-3.5 shrink-0" />
          <span className="truncate">
            Desktop gateway offline — agent cannot run shell/files on your machine.
          </span>
        </div>
        <div className="flex shrink-0 gap-1.5">
          <Button size="sm" variant="secondary" disabled={busy} onClick={() => void connectGateway()}>
            {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Plug className="h-3.5 w-3.5" />}
            Connect host
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              try {
                onOpenDesktop?.();
              } catch {
                /* ignore */
              }
              setNav("settings");
            }}
          >
            Settings
          </Button>
        </div>
      </div>
    );
  }

  return (
    <Card className={className}>
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <CardTitle className="flex items-center gap-2 text-sm">
              <HardDrive className="h-4 w-4" />
              Desktop host gateway
            </CardTitle>
            <CardDescription>
              Unsandboxed access so Grok can run CLI commands, read/write files, and open apps on
              this machine on your behalf.
            </CardDescription>
          </div>
          <Badge variant={online ? "success" : "warn"}>
            {online ? "connected" : "offline"}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {online && info ? (
          <div className="flex items-start gap-2 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-success)_35%,transparent)] bg-[color-mix(in_oklab,var(--color-success)_8%,transparent)] px-3 py-2.5 text-sm">
            <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-[var(--color-success)]" />
            <div className="min-w-0">
              <div className="font-medium text-[var(--color-fg)]">
                {info.user}@{info.hostname}
              </div>
              <div className="font-mono text-xs text-[var(--color-muted)]">
                {info.bridge} · {info.platform}/{info.arch} · {info.homedir}
              </div>
              <div className="mt-1 text-xs text-[var(--color-subtle)]">
                Agent can use <span className="font-mono">$</span> shell in chat and host tools from Settings.
              </div>
            </div>
          </div>
        ) : (
          <div className="space-y-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3 text-sm text-[var(--color-muted)]">
            <p>
              No live desktop gateway. Without it, Grok cannot control your PC — only chat.
            </p>
            <ol className="list-decimal space-y-1 pl-4 text-xs text-[var(--color-subtle)]">
              <li>
                Install/run the Arch package:{" "}
                <span className="font-mono text-[var(--color-fg)]">grokhub</span>
              </li>
              <li>Use the Electron window (not a plain browser tab)</li>
              <li>Click Connect below to probe host IPC / API</li>
            </ol>
          </div>
        )}

        {error && (
          <div className="rounded-[var(--radius-sm)] border border-[color-mix(in_oklab,var(--color-danger)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-danger)_10%,transparent)] px-3 py-2 text-xs text-[var(--color-danger)]">
            {error}
          </div>
        )}

        <div className="flex flex-wrap gap-2">
          {!online ? (
            <Button disabled={busy} onClick={() => void connectGateway()}>
              {busy ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  Connecting…
                </>
              ) : (
                <>
                  <Plug className="h-4 w-4" />
                  Connect desktop host
                </>
              )}
            </Button>
          ) : (
            <Button
              onClick={() => {
                try {
                  onOpenDesktop?.();
                } catch {
                  /* ignore */
                }
                setNav("settings");
              }}
            >
              Host settings
            </Button>
          )}
          <Button variant="secondary" size="default" disabled={busy} onClick={() => void probe()}>
            <RefreshCw className={cn("h-4 w-4", busy && "animate-spin")} />
            Reprobe
          </Button>
          {isElectron && (
            <Badge variant="info" className="self-center">
              Electron shell
            </Badge>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

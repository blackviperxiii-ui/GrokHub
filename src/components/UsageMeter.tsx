import type { CSSProperties } from "react";
import { Gauge, RefreshCw, Link2 } from "lucide-react";
import {
  formatUnits,
  PLAN_LIMITS,
  usagePercent,
  usageTone,
} from "@/lib/usage";
import { formatResetAt, formatUsdFromCents } from "@/lib/grok-website-usage";
import { useGrokHub } from "@/lib/store";
import { cn } from "@/lib/utils";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./ui/card";
import { Input } from "./ui/input";
import { useState } from "react";

function barColor(tone: "ok" | "warn" | "danger") {
  if (tone === "danger") return "bg-[var(--color-danger)]";
  if (tone === "warn") return "bg-[var(--color-warn)]";
  return "bg-[var(--color-info)]";
}

const PRODUCT_COLORS = [
  "bg-[var(--color-info)]",
  "bg-[var(--color-success)]",
  "bg-[var(--color-warn)]",
  "bg-[var(--color-danger)]",
  "bg-[var(--color-muted)]",
];

/** Compact titlebar chip — shows website weekly % when available */
export function UsageMeterChip({ className }: { className?: string }) {
  const usage = useGrokHub((s) => s.usage);
  const setNav = useGrokHub((s) => s.setNav);
  const refreshUsage = useGrokHub((s) => s.refreshUsage);
  const web = usage.website;
  const pct =
    web?.creditUsagePercent != null && (usage.source === "website" || web.creditUsagePercent > 0)
      ? web.creditUsagePercent
      : usagePercent(usage);
  const tone = usageTone(pct);
  const label = web?.planLabel || PLAN_LIMITS[usage.plan].label;
  const noDrag = { WebkitAppRegion: "no-drag" } as CSSProperties;
  const stale =
    !usage.lastPolledAt || Date.now() - usage.lastPolledAt > 5 * 60_000;
  const err = web?.error;

  return (
    <button
      type="button"
      style={noDrag}
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        void refreshUsage();
        setNav("settings");
      }}
      title={
        err
          ? `Usage error: ${err}`
          : web && usage.source === "website"
            ? `${label}: ${Math.round(pct)}% weekly · resets ${formatResetAt(web.periodEnd)}`
            : `${label}: ${Math.round(pct)}% (local meter) · link grok.com in Settings for live weekly %`
      }
      className={cn(
        "flex min-w-0 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-1 text-left transition-colors hover:border-[var(--color-border-strong)]",
        err && "border-[color-mix(in_oklab,var(--color-warn)_40%,var(--color-border))]",
        className,
      )}
    >
      <Gauge
        className={cn(
          "h-3.5 w-3.5 shrink-0",
          err
            ? "text-[var(--color-warn)]"
            : tone === "danger"
              ? "text-[var(--color-danger)]"
              : tone === "warn"
                ? "text-[var(--color-warn)]"
                : "text-[var(--color-muted)]",
        )}
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-2">
          <span className="truncate text-[10px] font-medium text-[var(--color-fg)]">
            {usage.source === "website" ? label : "Usage"}
          </span>
          <span className="tabular text-[10px] text-[var(--color-subtle)]">
            {Math.round(pct)}%
            {stale ? " ·" : ""}
          </span>
        </div>
        <div className="mt-0.5 h-1 overflow-hidden rounded-full bg-[var(--color-border)]">
          <div
            className={cn("h-full rounded-full transition-all duration-300", barColor(tone))}
            style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
          />
        </div>
      </div>
    </button>
  );
}

/** Full panel matching Grok website Settings → Usage */
export function UsageMeterPanel({ compact }: { compact?: boolean }) {
  const usage = useGrokHub((s) => s.usage);
  const ssoCookie = useGrokHub((s) => s.ssoCookie);
  const setSsoCookie = useGrokHub((s) => s.setSsoCookie);
  const linkWebsite = useGrokHub((s) => s.linkGrokWebsiteSession);
  const refreshUsage = useGrokHub((s) => s.refreshUsage);
  const [ssoDraft, setSsoDraft] = useState("");
  const [linkBusy, setLinkBusy] = useState(false);
  const [linkDetail, setLinkDetail] = useState<string | null>(null);

  const web = usage.website;
  const pct = web?.creditUsagePercent ?? usagePercent(usage);
  const tone = usageTone(pct);
  const planTitle =
    web?.periodType === "weekly"
      ? `Weekly ${web.planLabel || "SuperGrok"} Limit`
      : web?.planLabel
        ? `${web.planLabel} usage`
        : "Subscription usage";

  const products = (web?.productUsage || []).filter((p) => p.usagePercent > 0);

  async function onLink() {
    setLinkBusy(true);
    setLinkDetail(null);
    const r = await linkWebsite();
    setLinkDetail(r.detail);
    setLinkBusy(false);
  }

  return (
    <Card>
      <CardHeader className={compact ? "pb-2" : undefined}>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div>
            <CardTitle className="flex items-center gap-2 text-sm">
              <Gauge className="h-4 w-4 text-[var(--color-muted)]" />
              Usage
            </CardTitle>
            <CardDescription>
              Live from Grok website Settings → Usage · polled every minute
            </CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <Badge
              variant={
                usage.source === "website"
                  ? "success"
                  : tone === "danger"
                    ? "danger"
                    : "default"
              }
            >
              {usage.source === "website" ? "grok.com" : usage.source}
            </Badge>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void refreshUsage()}
              title="Refresh now"
            >
              <RefreshCw className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* Weekly pool — website style */}
        <div className="rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="mb-1 text-xs font-medium text-[var(--color-muted)]">
            {planTitle}
          </div>
          <div className="mb-2 flex flex-wrap items-end justify-between gap-2">
            <div className="text-2xl font-semibold tabular tracking-tight">
              {Math.round(pct)}%
              <span className="ml-1.5 text-sm font-normal text-[var(--color-muted)]">
                used
              </span>
            </div>
            <div className="text-xs text-[var(--color-subtle)]">
              Resets {formatResetAt(web?.periodEnd || usage.periodEnd)}
            </div>
          </div>
          {/* Stacked product bar */}
          <div className="mb-2 flex h-2.5 overflow-hidden rounded-full bg-[var(--color-border)]">
            {products.length > 0 ? (
              products.map((p, i) => (
                <div
                  key={p.product + i}
                  className={cn("h-full", PRODUCT_COLORS[i % PRODUCT_COLORS.length])}
                  style={{ width: `${Math.min(100, p.usagePercent)}%` }}
                  title={`${p.label}: ${Math.round(p.usagePercent)}%`}
                />
              ))
            ) : (
              <div
                className={cn("h-full rounded-full transition-all", barColor(tone))}
                style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
              />
            )}
          </div>
          {products.length > 0 && (
            <div className="flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-[var(--color-muted)]">
              {products.map((p, i) => (
                <span key={p.product + i} className="inline-flex items-center gap-1.5">
                  <span
                    className={cn(
                      "inline-block h-1.5 w-1.5 rounded-full",
                      PRODUCT_COLORS[i % PRODUCT_COLORS.length],
                    )}
                  />
                  {p.label}{" "}
                  <span className="tabular text-[var(--color-fg)]">
                    {Math.round(p.usagePercent)}%
                  </span>
                </span>
              ))}
            </div>
          )}
          {web?.error && (
            <p className="mt-2 text-xs text-[var(--color-warn)]">{web.error}</p>
          )}
          {!web && (
            <p className="mt-2 text-xs text-[var(--color-subtle)]">
              Link your Grok website session below to show the same weekly limit as
              grok.com (Build / App Builder / Chat breakdown).
            </p>
          )}
        </div>

        {/* Extra credits */}
        <div className="rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="mb-2 text-xs font-medium text-[var(--color-muted)]">
            Extra Usage Credits
          </div>
          <div className="flex items-center justify-between gap-3">
            <div>
              <div className="text-lg font-semibold tabular">
                {formatUsdFromCents(web?.prepaidBalanceCents ?? 0)}
              </div>
              <div className="text-xs text-[var(--color-subtle)]">Additional Credits</div>
            </div>
          </div>
        </div>

        {/* Link website session */}
        {!compact && (
          <div className="space-y-2 rounded-[var(--radius-lg)] border border-[var(--color-border)] p-3">
            <div className="text-xs font-medium text-[var(--color-fg)]">
              Grok website session
            </div>
            <p className="text-[11px] text-[var(--color-subtle)]">
              Opens an in-app sign-in window. Complete login until the Grok chat UI appears, then
              click <span className="font-medium text-[var(--color-fg)]">Use this session</span> in
              the bar at the bottom of that window. Or paste the{" "}
              <span className="font-mono">sso</span> cookie from your browser DevTools.
            </p>
            <div className="flex flex-wrap gap-2">
              <Button size="sm" onClick={() => void onLink()} disabled={linkBusy}>
                <Link2 className="h-3.5 w-3.5" />
                {linkBusy ? "Waiting for sign-in…" : "Link Grok website"}
              </Button>
              {ssoCookie ? (
                <Badge variant="success">Linked</Badge>
              ) : (
                <Badge>Not linked</Badge>
              )}
              {ssoCookie ? (
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => {
                    setSsoCookie("");
                    setLinkDetail("Website session cleared");
                  }}
                >
                  Unlink
                </Button>
              ) : null}
            </div>
            {linkDetail && (
              <p
                className={
                  linkDetail.toLowerCase().includes("linked") ||
                  linkDetail.toLowerCase().includes("saved")
                    ? "text-[11px] text-[var(--color-success)]"
                    : "text-[11px] text-[var(--color-muted)]"
                }
              >
                {linkDetail}
              </p>
            )}
            <div className="flex gap-2">
              <Input
                value={ssoDraft}
                onChange={(e) => setSsoDraft(e.target.value)}
                placeholder="Paste sso=… or full Cookie header from grok.com"
                className="font-mono text-xs"
              />
              <Button
                size="sm"
                variant="secondary"
                disabled={!ssoDraft.trim()}
                onClick={() => {
                  setSsoCookie(ssoDraft.trim());
                  setSsoDraft("");
                  setLinkDetail("Cookie saved & injected — refreshing usage…");
                  void refreshUsage();
                }}
              >
                Save
              </Button>
            </div>
          </div>
        )}

        {usage.lastPolledAt > 0 && (
          <div className="text-[10px] text-[var(--color-subtle)]">
            Last poll {new Date(usage.lastPolledAt).toLocaleTimeString()}
            {usage.source === "website" ? " · source grok.com" : ""}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

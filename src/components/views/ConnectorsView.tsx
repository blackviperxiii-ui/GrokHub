import { Cable, Check, ExternalLink, Link2Off, Loader2, RefreshCw, Settings } from "lucide-react";
import { useMemo, useState } from "react";
import { useGrokHub } from "@/lib/store";
import { RelativeTime } from "../RelativeTime";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";
import { cn } from "@/lib/utils";

type Tier = "live" | "website" | "planned";

function tierOf(id: string, liveTools?: boolean, source?: string): Tier {
  if (id === "grok-xai" || id === "desktop-host" || id === "github" || liveTools) return "live";
  if (source === "website" || ["gmail", "gdrive", "google-calendar", "notion", "outlook", "teams", "linear", "vercel", "stripe"].includes(id))
    return "website";
  return "planned";
}

export function ConnectorsView() {
  const connectors = useGrokHub((s) => s.connectors);
  const connectConnector = useGrokHub((s) => s.connectConnector);
  const syncWebsiteConnectors = useGrokHub((s) => s.syncWebsiteConnectors);
  const setNav = useGrokHub((s) => s.setNav);
  const oauth = useGrokHub((s) => s.oauth);
  const ssoCookie = useGrokHub((s) => s.ssoCookie);
  const [q, setQ] = useState("");
  const [busyId, setBusyId] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);

  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase();
    if (!needle) return connectors;
    return connectors.filter(
      (c) =>
        c.name.toLowerCase().includes(needle) ||
        c.category.toLowerCase().includes(needle) ||
        c.description.toLowerCase().includes(needle),
    );
  }, [connectors, q]);

  const groups = useMemo(() => {
    const live: typeof filtered = [];
    const website: typeof filtered = [];
    const planned: typeof filtered = [];
    for (const c of filtered) {
      const t = tierOf(c.id, c.liveTools, c.source);
      if (t === "live") live.push(c);
      else if (t === "website") website.push(c);
      else planned.push(c);
    }
    // Connected first within group
    const sort = (a: (typeof filtered)[0], b: (typeof filtered)[0]) => {
      if (a.status === "connected" && b.status !== "connected") return -1;
      if (b.status === "connected" && a.status !== "connected") return 1;
      return a.name.localeCompare(b.name);
    };
    live.sort(sort);
    website.sort(sort);
    planned.sort(sort);
    return { live, website, planned };
  }, [filtered]);

  const connected = connectors.filter((c) => c.status === "connected").length;

  async function onToggle(id: string) {
    setBusyId(id);
    try {
      await connectConnector(id);
    } finally {
      setBusyId(null);
    }
  }

  async function onSyncAll() {
    setSyncing(true);
    try {
      await syncWebsiteConnectors();
    } finally {
      setSyncing(false);
    }
  }

  function renderCard(c: (typeof connectors)[0]) {
    const live = tierOf(c.id, c.liveTools, c.source) === "live";
    const on = c.status === "connected";
    const err = c.status === "error";
    const busy = busyId === c.id;
    return (
      <Card key={c.id} className={cn("card-quiet flex flex-col", on && "border-[color-mix(in_oklab,var(--color-success)_35%,var(--color-border))]")}>
        <CardHeader className="pb-2">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <CardTitle className="text-sm">{c.name}</CardTitle>
              <CardDescription className="mt-0.5">{c.category}</CardDescription>
            </div>
            <Badge variant={on ? "success" : err ? "danger" : live ? "default" : "info"}>
              {on
                ? live
                  ? "Live tools"
                  : "Status only"
                : err
                  ? "Error"
                  : live
                    ? "Ready"
                    : "Status sync"}
            </Badge>
          </div>
        </CardHeader>
        <CardContent className="mt-auto flex flex-1 flex-col gap-3 pt-0">
          <p className="text-xs leading-relaxed text-[var(--color-muted)]">{c.description}</p>
          {!live && (
            <p className="rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[10px] leading-snug text-[var(--color-subtle)]">
              Website link shows connection status only. The agent cannot call Gmail/Drive/etc.
              tools from this app yet — use live connectors (Grok, Desktop Host, GitHub) for
              executable tools.
            </p>
          )}
          {c.accountLabel && (
            <p className="truncate font-mono text-[10px] text-[var(--color-subtle)]">{c.accountLabel}</p>
          )}
          {c.lastUsed && (
            <p className="text-[10px] text-[var(--color-subtle)]">
              Last used <RelativeTime ts={c.lastUsed} />
            </p>
          )}
          <div className="mt-auto flex flex-wrap gap-2">
            <Button size="sm" disabled={busy} onClick={() => void onToggle(c.id)}>
              {busy ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : on ? (
                <Check className="h-3.5 w-3.5" />
              ) : (
                <Cable className="h-3.5 w-3.5" />
              )}
              {on ? "Refresh" : "Connect"}
            </Button>
            {c.id === "grok-xai" && (
              <Button size="sm" variant="secondary" onClick={() => setNav("settings")}>
                <Settings className="h-3.5 w-3.5" />
                OAuth
              </Button>
            )}
            {c.id === "desktop-host" && (
              <Button size="sm" variant="secondary" onClick={() => setNav("settings")}>
                Host
              </Button>
            )}
            {!live && !on && (
              <Button
                size="sm"
                variant="secondary"
                onClick={() => {
                  if (!ssoCookie) setNav("settings");
                  else void onToggle(c.id);
                }}
              >
                <ExternalLink className="h-3.5 w-3.5" />
                {ssoCookie ? "Sync" : "Link site"}
              </Button>
            )}
            {on && (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => void onToggle(c.id)}
                title="Disconnect"
              >
                <Link2Off className="h-3.5 w-3.5" />
              </Button>
            )}
          </div>
        </CardContent>
      </Card>
    );
  }

  function Section({
    title,
    hint,
    items,
  }: {
    title: string;
    hint: string;
    items: typeof filtered;
  }) {
    if (!items.length) return null;
    return (
      <section className="space-y-2">
        <div className="flex items-end justify-between gap-2 px-0.5">
          <div>
            <h3 className="text-xs font-semibold uppercase tracking-wide text-[var(--color-subtle)]">
              {title}
            </h3>
            <p className="text-[11px] text-[var(--color-muted)]">{hint}</p>
          </div>
          <span className="text-[10px] tabular text-[var(--color-subtle)]">{items.length}</span>
        </div>
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">{items.map(renderCard)}</div>
      </section>
    );
  }

  return (
    <div className="space-y-5">
      <Card>
        <CardHeader className="gap-3 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <CardTitle className="flex items-center gap-2">
              <Cable className="h-4 w-4" />
              Connectors
            </CardTitle>
            <CardDescription>
              {connected} connected
              {oauth?.email ? ` · Grok as ${oauth.email}` : ""}
              {ssoCookie ? " · Website session linked" : " · Link website for Gmail/Drive/…"}
            </CardDescription>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Input
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="Search connectors"
              className="sm:max-w-xs"
            />
            <Button
              size="sm"
              variant="secondary"
              disabled={syncing || !ssoCookie}
              onClick={() => void onSyncAll()}
              title={!ssoCookie ? "Link Grok website in Settings first" : "Sync from grok.com"}
            >
              {syncing ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <RefreshCw className="h-3.5 w-3.5" />}
              Sync website
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setNav("settings")}>
              <Settings className="h-3.5 w-3.5" />
              Settings
            </Button>
          </div>
        </CardHeader>
      </Card>

      <Section
        title="Live in GrokHub"
        hint="Actions run here — Grok API, desktop host, GitHub tools."
        items={groups.live}
      />
      <Section
        title="Website-linked"
        hint="Status from grok.com after you link a website session."
        items={groups.website}
      />
      <Section
        title="Coming soon"
        hint="Listed for parity — connect on the website when available."
        items={groups.planned}
      />
    </div>
  );
}

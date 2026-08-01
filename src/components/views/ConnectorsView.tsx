import { Cable, Check, Link2Off } from "lucide-react";
import { useMemo, useState } from "react";
import { useGrokClaw } from "@/lib/store";
import { RelativeTime } from "../RelativeTime";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";

export function ConnectorsView() {
  const connectors = useGrokClaw((s) => s.connectors);
  const toggleConnector = useGrokClaw((s) => s.toggleConnector);
  const [q, setQ] = useState("");

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

  const connected = connectors.filter((c) => c.status === "connected").length;

  return (
    <div className="space-y-5">
      <Card>
        <CardHeader className="gap-3 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <CardTitle className="flex items-center gap-2">
              <Cable className="h-4 w-4" />
              Grok Connectors
            </CardTitle>
            <CardDescription>
              OAuth tools Grok can use in chat — email, files, code, CRM, and custom MCP.
              {` ${connected} connected.`}
            </CardDescription>
          </div>
          <Input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Search connectors"
            className="sm:max-w-xs"
          />
        </CardHeader>
      </Card>

      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {filtered.map((c) => {
          const on = c.status === "connected";
          return (
            <Card key={c.id} className="flex flex-col">
              <CardHeader>
                <div className="flex items-start justify-between gap-2">
                  <div>
                    <CardTitle className="text-sm">{c.name}</CardTitle>
                    <CardDescription className="mt-1">{c.category}</CardDescription>
                  </div>
                  <Badge variant={on ? "success" : "default"}>
                    {on ? "connected" : "offline"}
                  </Badge>
                </div>
              </CardHeader>
              <CardContent className="mt-auto flex flex-1 flex-col gap-4">
                <p className="text-sm text-[var(--color-muted)]">{c.description}</p>
                <div className="flex flex-wrap gap-1.5">
                  {c.tools.slice(0, 4).map((t) => (
                    <span
                      key={t}
                      className="rounded-full border border-[var(--color-border)] px-2 py-0.5 font-mono text-[10px] text-[var(--color-subtle)]"
                    >
                      {t}
                    </span>
                  ))}
                </div>
                {c.lastUsed && (
                  <p className="text-xs text-[var(--color-subtle)]">
                    Last used <RelativeTime ts={c.lastUsed} />
                  </p>
                )}
                <Button
                  variant={on ? "secondary" : "default"}
                  size="sm"
                  className="mt-auto w-full"
                  onClick={() => toggleConnector(c.id)}
                >
                  {on ? (
                    <>
                      <Link2Off className="h-3.5 w-3.5" />
                      Disconnect
                    </>
                  ) : (
                    <>
                      <Check className="h-3.5 w-3.5" />
                      Connect
                    </>
                  )}
                </Button>
              </CardContent>
            </Card>
          );
        })}
      </div>
    </div>
  );
}

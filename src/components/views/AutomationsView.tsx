import { Play, Plus, TimerReset } from "lucide-react";
import { useState } from "react";
import { useGrokHub } from "@/lib/store";
import type { AutomationSchedule } from "@/lib/types";
import { RelativeTime } from "../RelativeTime";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";

const SCHEDULES: AutomationSchedule[] = [
  "once",
  "daily",
  "weekdays",
  "weekly",
  "monthly",
];

export function AutomationsView() {
  const automations = useGrokHub((s) => s.automations);
  const connectors = useGrokHub((s) => s.connectors);
  const toggleAutomation = useGrokHub((s) => s.toggleAutomation);
  const runAutomation = useGrokHub((s) => s.runAutomation);
  const addAutomation = useGrokHub((s) => s.addAutomation);
  const running = useGrokHub((s) => s.running);

  const [name, setName] = useState("");
  const [instructions, setInstructions] = useState("");
  const [schedule, setSchedule] = useState<AutomationSchedule>("daily");
  const [time, setTime] = useState("09:00");

  function onCreate() {
    if (!name.trim() || !instructions.trim()) return;
    addAutomation({
      name: name.trim(),
      instructions: instructions.trim(),
      schedule,
      time: time.trim() || "09:00",
    });
    setName("");
    setInstructions("");
    setSchedule("daily");
    setTime("09:00");
  }

  return (
    <div className="space-y-5">
      <div className="grid gap-4 lg:grid-cols-[1fr_340px]">
        <div className="space-y-3">
          {automations.map((a) => (
            <Card key={a.id}>
              <CardHeader className="gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div className="min-w-0">
                  <CardTitle className="flex flex-wrap items-center gap-2 text-sm">
                    <TimerReset className="h-4 w-4 text-[var(--color-muted)]" />
                    {a.name}
                    <Badge variant={a.enabled ? "success" : "default"}>
                      {a.enabled ? "enabled" : "paused"}
                    </Badge>
                  </CardTitle>
                  <CardDescription className="mt-1">
                    {a.schedule} · {a.time} · {a.runCount} runs
                    {a.lastRun ? (
                      <>
                        {" "}
                        · last <RelativeTime ts={a.lastRun} />
                      </>
                    ) : null}
                  </CardDescription>
                </div>
                <div className="flex shrink-0 gap-2">
                  <Button size="sm" variant="secondary" onClick={() => toggleAutomation(a.id)}>
                    {a.enabled ? "Pause" : "Enable"}
                  </Button>
                  <Button
                    size="sm"
                    disabled={!a.enabled || running}
                    onClick={() => void runAutomation(a.id)}
                  >
                    <Play className="h-3.5 w-3.5" />
                    Run now
                  </Button>
                </div>
              </CardHeader>
              <CardContent className="space-y-3">
                <p className="text-sm text-[var(--color-muted)]">{a.instructions}</p>
                <div className="flex flex-wrap gap-1.5">
                  {a.connectorIds.map((id) => {
                    const c = connectors.find((x) => x.id === id);
                    return (
                      <Badge key={id} variant={c?.status === "connected" ? "info" : "default"}>
                        {c?.name ?? id}
                      </Badge>
                    );
                  })}
                  {a.skillIds.map((id) => (
                    <Badge key={id}>{id}</Badge>
                  ))}
                </div>
              </CardContent>
            </Card>
          ))}
        </div>

        <Card className="h-fit lg:sticky lg:top-20">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-sm">
              <Plus className="h-4 w-4" />
              New automation
            </CardTitle>
            <CardDescription>
              Local scheduler (every 30s while the app is open). Due jobs run through the agent
              with live Grok when connected.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-[var(--color-muted)]">Name</label>
              <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Evening cash check" />
            </div>
            <div className="grid grid-cols-2 gap-2">
              <div className="space-y-1.5">
                <label className="text-xs font-medium text-[var(--color-muted)]">Schedule</label>
                <select
                  value={schedule}
                  onChange={(e) => setSchedule(e.target.value as AutomationSchedule)}
                  className="flex h-10 w-full rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-sm text-[var(--color-fg)]"
                >
                  {SCHEDULES.map((s) => (
                    <option key={s} value={s}>
                      {s}
                    </option>
                  ))}
                </select>
              </div>
              <div className="space-y-1.5">
                <label className="text-xs font-medium text-[var(--color-muted)]">Time</label>
                <Input value={time} onChange={(e) => setTime(e.target.value)} placeholder="18:00" />
              </div>
            </div>
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-[var(--color-muted)]">Instructions</label>
              <Textarea
                value={instructions}
                onChange={(e) => setInstructions(e.target.value)}
                placeholder="@Gmail summarize unpaid bills and draft replies…"
              />
            </div>
            <Button
              className="w-full"
              onClick={onCreate}
              disabled={!name.trim() || !instructions.trim()}
            >
              Create automation
            </Button>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

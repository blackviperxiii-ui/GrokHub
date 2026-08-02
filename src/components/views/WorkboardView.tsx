import { useMemo, useState } from "react";
import {
  Check,
  ClipboardList,
  Layers,
  Play,
  Plus,
  ThumbsUp,
  Trash2,
  X,
} from "lucide-react";
import { useGrokHub } from "@/lib/store";
import {
  WORK_STATUS_LABEL,
  type WorkItemStatus,
  type WorkPriority,
} from "@/lib/workboard";
import { cn } from "@/lib/utils";
import { Button } from "../ui/button";
import { Badge } from "../ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";

const COLUMNS: WorkItemStatus[] = [
  "proposed",
  "approved",
  "staged",
  "in_progress",
  "done",
  "dismissed",
];

const COLUMN_HINT: Record<WorkItemStatus, string> = {
  proposed: "Agent pinned — review",
  approved: "You approved",
  staged: "Ready to run",
  in_progress: "Actively working",
  done: "Finished",
  dismissed: "Won’t do",
};

export function WorkboardView() {
  const items = useGrokHub((s) => s.workboard.items);
  const setStatus = useGrokHub((s) => s.setWorkItemStatus);
  const pin = useGrokHub((s) => s.pinWorkItem);
  const remove = useGrokHub((s) => s.removeWorkItem);
  const setNav = useGrokHub((s) => s.setNav);
  const selectThread = useGrokHub((s) => s.selectThread);
  const [title, setTitle] = useState("");
  const [detail, setDetail] = useState("");
  const [priority, setPriority] = useState<WorkPriority>("normal");
  const [filter, setFilter] = useState<"open" | "all">("open");

  const visible = useMemo(() => {
    if (filter === "all") return items;
    return items.filter((i) => !["done", "dismissed"].includes(i.status));
  }, [items, filter]);

  const byCol = useMemo(() => {
    const m = Object.fromEntries(COLUMNS.map((c) => [c, [] as typeof items])) as Record<
      WorkItemStatus,
      typeof items
    >;
    for (const i of visible) m[i.status].push(i);
    return m;
  }, [visible]);

  return (
    <div className="mx-auto flex w-full max-w-[1600px] flex-col gap-4">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="flex items-center gap-2 text-lg font-semibold tracking-tight">
            <ClipboardList className="h-5 w-5 text-[var(--color-info)]" />
            Workboard
          </h1>
          <p className="mt-1 max-w-xl text-sm text-[var(--color-muted)]">
            Agent-pinned tasks you can approve, stage, track, or dismiss. The agent can also pin
            with <span className="font-mono text-xs">WORK_PIN:</span> lines.
          </p>
        </div>
        <div className="flex gap-2">
          <Button
            size="sm"
            variant={filter === "open" ? "default" : "secondary"}
            onClick={() => setFilter("open")}
          >
            Open
          </Button>
          <Button
            size="sm"
            variant={filter === "all" ? "default" : "secondary"}
            onClick={() => setFilter("all")}
          >
            All
          </Button>
        </div>
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Add task</CardTitle>
          <CardDescription>Pin work for yourself or the agent to pick up later.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-2 sm:flex-row sm:items-end">
          <label className="min-w-0 flex-1 space-y-1">
            <span className="text-[11px] text-[var(--color-muted)]">Title</span>
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              className="w-full rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-1.5 text-sm"
              placeholder="e.g. Fix update HOME env"
            />
          </label>
          <label className="min-w-0 flex-[1.2] space-y-1">
            <span className="text-[11px] text-[var(--color-muted)]">Detail</span>
            <input
              value={detail}
              onChange={(e) => setDetail(e.target.value)}
              className="w-full rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-1.5 text-sm"
              placeholder="Optional notes"
            />
          </label>
          <label className="space-y-1">
            <span className="text-[11px] text-[var(--color-muted)]">Priority</span>
            <select
              value={priority}
              onChange={(e) => setPriority(e.target.value as WorkPriority)}
              className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-1.5 text-sm"
            >
              <option value="low">Low</option>
              <option value="normal">Normal</option>
              <option value="high">High</option>
            </select>
          </label>
          <Button
            size="sm"
            onClick={() => {
              if (!title.trim()) return;
              pin({ title, detail, priority, source: "user" });
              setTitle("");
              setDetail("");
            }}
          >
            <Plus className="mr-1 h-3.5 w-3.5" />
            Pin
          </Button>
        </CardContent>
      </Card>

      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-6">
        {COLUMNS.map((col) => (
          <div
            key={col}
            className="flex min-h-[12rem] flex-col rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-panel)]"
          >
            <div className="border-b border-[var(--color-border)] px-3 py-2">
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs font-semibold">{WORK_STATUS_LABEL[col]}</span>
                <Badge variant="default" className="font-mono text-[10px]">
                  {byCol[col].length}
                </Badge>
              </div>
              <p className="text-[10px] text-[var(--color-subtle)]">{COLUMN_HINT[col]}</p>
            </div>
            <div className="flex flex-1 flex-col gap-2 p-2">
              {byCol[col].length === 0 ? (
                <p className="px-1 py-4 text-center text-[11px] text-[var(--color-subtle)]">Empty</p>
              ) : (
                byCol[col].map((item) => (
                  <div
                    key={item.id}
                    className={cn(
                      "rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] p-2 shadow-sm",
                      item.priority === "high" &&
                        "border-[color-mix(in_oklab,var(--color-danger)_35%,var(--color-border))]",
                    )}
                  >
                    <div className="text-xs font-medium leading-snug text-[var(--color-fg)]">
                      {item.title}
                    </div>
                    {item.detail ? (
                      <p className="mt-1 line-clamp-3 text-[11px] text-[var(--color-muted)]">
                        {item.detail}
                      </p>
                    ) : null}
                    <div className="mt-1.5 flex flex-wrap items-center gap-1 text-[9px] text-[var(--color-subtle)]">
                      <span className="rounded bg-[var(--color-panel)] px-1 font-mono">
                        {item.priority}
                      </span>
                      <span>{item.source}</span>
                    </div>
                    <div className="mt-2 flex flex-wrap gap-1">
                      {col === "proposed" && (
                        <IconBtn
                          title="Approve"
                          onClick={() => setStatus(item.id, "approved")}
                        >
                          <ThumbsUp className="h-3 w-3" />
                        </IconBtn>
                      )}
                      {(col === "proposed" || col === "approved") && (
                        <IconBtn title="Stage" onClick={() => setStatus(item.id, "staged")}>
                          <Layers className="h-3 w-3" />
                        </IconBtn>
                      )}
                      {(col === "approved" || col === "staged") && (
                        <IconBtn
                          title="Start"
                          onClick={() => setStatus(item.id, "in_progress")}
                        >
                          <Play className="h-3 w-3" />
                        </IconBtn>
                      )}
                      {col !== "done" && col !== "dismissed" && (
                        <IconBtn title="Done" onClick={() => setStatus(item.id, "done")}>
                          <Check className="h-3 w-3" />
                        </IconBtn>
                      )}
                      {col !== "dismissed" && col !== "done" && (
                        <IconBtn
                          title="Dismiss"
                          onClick={() => setStatus(item.id, "dismissed")}
                        >
                          <X className="h-3 w-3" />
                        </IconBtn>
                      )}
                      {item.threadId && (
                        <IconBtn
                          title="Open chat"
                          onClick={() => {
                            selectThread(item.threadId!);
                            setNav("chat");
                          }}
                        >
                          <ClipboardList className="h-3 w-3" />
                        </IconBtn>
                      )}
                      <IconBtn title="Remove" onClick={() => remove(item.id)}>
                        <Trash2 className="h-3 w-3" />
                      </IconBtn>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function IconBtn({
  children,
  title,
  onClick,
}: {
  children: React.ReactNode;
  title: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className="inline-flex h-6 w-6 items-center justify-center rounded border border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]"
    >
      {children}
    </button>
  );
}

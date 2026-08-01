import { History, MessageSquarePlus, Pencil, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useGrokHub } from "@/lib/store";
import { RelativeTime } from "../RelativeTime";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { cn } from "@/lib/utils";

export function HistoryView() {
  const threads = useGrokHub((s) => s.threads);
  const activeThreadId = useGrokHub((s) => s.activeThreadId);
  const selectThread = useGrokHub((s) => s.selectThread);
  const deleteThread = useGrokHub((s) => s.deleteThread);
  const renameThread = useGrokHub((s) => s.renameThread);
  const newThread = useGrokHub((s) => s.newThread);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  const sorted = [...threads].sort((a, b) => b.updatedAt - a.updatedAt);

  useEffect(() => {
    if (renamingId) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [renamingId]);

  function startRename(id: string, title: string) {
    setRenamingId(id);
    setDraft(title);
  }

  function commitRename() {
    if (!renamingId) return;
    const next = draft.trim();
    if (next) renameThread(renamingId, next);
    setRenamingId(null);
  }

  return (
    <div className="content-readable mx-auto space-y-4">
      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-3">
          <div>
            <CardTitle className="flex items-center gap-2 text-sm">
              <History className="h-4 w-4" />
              History
            </CardTitle>
            <CardDescription>
              Past chats — select to resume, rename, or delete.
            </CardDescription>
          </div>
          <Button size="sm" onClick={() => newThread()}>
            <MessageSquarePlus className="h-4 w-4" />
            New chat
          </Button>
        </CardHeader>
        <CardContent className="space-y-1.5 p-3 pt-0">
          {sorted.length === 0 && (
            <p className="px-2 py-8 text-center text-sm text-[var(--color-muted)]">
              No chats yet. Start one from Agent.
            </p>
          )}
          {sorted.map((t) => {
            const active = t.id === activeThreadId;
            const preview =
              [...t.messages].reverse().find((m) => m.role === "user" || m.role === "assistant")
                ?.content || "Empty chat";
            const isRenaming = renamingId === t.id;
            return (
              <div
                key={t.id}
                className={cn(
                  "group flex items-start gap-2 rounded-[var(--radius-md)] border px-3 py-2.5 transition-colors",
                  active
                    ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)]"
                    : "border-[var(--color-border)] hover:border-[var(--color-border-strong)]",
                )}
              >
                {isRenaming ? (
                  <div className="min-w-0 flex-1">
                    <input
                      ref={inputRef}
                      value={draft}
                      onChange={(e) => setDraft(e.target.value)}
                      onBlur={commitRename}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          commitRename();
                        }
                        if (e.key === "Escape") setRenamingId(null);
                      }}
                      className="w-full rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2 py-1.5 text-sm outline-none focus:ring-1 focus:ring-[var(--color-accent)]"
                      aria-label="Rename chat"
                    />
                  </div>
                ) : (
                  <button
                    type="button"
                    className="min-w-0 flex-1 text-left"
                    onClick={() => selectThread(t.id)}
                  >
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="truncate text-sm font-medium">{t.title}</span>
                      {active && (
                        <Badge className="text-[10px]" variant="info">
                          Open
                        </Badge>
                      )}
                    </div>
                    <p className="mt-0.5 line-clamp-2 text-xs text-[var(--color-muted)]">
                      {preview.replace(/\s+/g, " ").slice(0, 140)}
                    </p>
                    <div className="mt-1 text-[10px] text-[var(--color-subtle)]">
                      <RelativeTime ts={t.updatedAt} />
                      {t.mode ? ` · ${t.mode}` : ""}
                    </div>
                  </button>
                )}
                <div className="flex shrink-0 gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                  <button
                    type="button"
                    className="rounded p-1.5 text-[var(--color-subtle)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"
                    aria-label="Rename chat"
                    onClick={() => startRename(t.id, t.title)}
                  >
                    <Pencil className="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    className="rounded p-1.5 text-[var(--color-subtle)] hover:bg-[var(--color-surface)] hover:text-[var(--color-danger)]"
                    aria-label="Delete chat"
                    onClick={() => deleteThread(t.id)}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </div>
              </div>
            );
          })}
        </CardContent>
      </Card>
    </div>
  );
}

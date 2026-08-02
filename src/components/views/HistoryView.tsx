import { Folder, History, MessageSquarePlus, Pencil, Pin, PinOff, Search, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useGrokHub } from "@/lib/store";
import { RelativeTime } from "../RelativeTime";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";
import { cn } from "@/lib/utils";

function dateBucket(ts: number, nowMs: number): string {
  const d = new Date(ts);
  const now = new Date(nowMs);
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const startYesterday = startToday - 86400000;
  if (ts >= startToday) return "Today";
  if (ts >= startYesterday) return "Yesterday";
  const weekAgo = startToday - 6 * 86400000;
  if (ts >= weekAgo) return "This week";
  return d.toLocaleDateString(undefined, { month: "short", year: "numeric" });
}

export function HistoryView() {
  const threads = useGrokHub((s) => s.threads);
  const activeThreadId = useGrokHub((s) => s.activeThreadId);
  const selectThread = useGrokHub((s) => s.selectThread);
  const deleteThread = useGrokHub((s) => s.deleteThread);
  const renameThread = useGrokHub((s) => s.renameThread);
  const pinThread = useGrokHub((s) => s.pinThread);
  const setThreadFolder = useGrokHub((s) => s.setThreadFolder);
  const newThread = useGrokHub((s) => s.newThread);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [q, setQ] = useState("");
  const [folderFilter, setFolderFilter] = useState<string | "all" | "pinned">("all");
  const [folderDraftId, setFolderDraftId] = useState<string | null>(null);
  const [folderDraft, setFolderDraft] = useState("");
  // Stable "now" only after mount — avoids SSR/client Today/Yesterday mismatches
  const [nowMs, setNowMs] = useState(0);
  useEffect(() => {
    setNowMs(Date.now());
  }, []);
  const inputRef = useRef<HTMLInputElement>(null);

  const folders = useMemo(() => {
    const set = new Set<string>();
    for (const t of threads) {
      if (t.folder) set.add(t.folder);
    }
    return [...set].sort((a, b) => a.localeCompare(b));
  }, [threads]);

  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase();
    let list = [...threads];
    if (folderFilter === "pinned") list = list.filter((t) => t.pinned);
    else if (folderFilter !== "all") list = list.filter((t) => t.folder === folderFilter);
    if (needle) {
      list = list.filter((t) => {
        if (t.title.toLowerCase().includes(needle)) return true;
        if (t.folder?.toLowerCase().includes(needle)) return true;
        return t.messages.some((m) => m.content.toLowerCase().includes(needle));
      });
    }
    list.sort((a, b) => {
      if (Boolean(a.pinned) !== Boolean(b.pinned)) return a.pinned ? -1 : 1;
      return b.updatedAt - a.updatedAt;
    });
    return list;
  }, [threads, q, folderFilter]);

  const grouped = useMemo(() => {
    const map = new Map<string, typeof filtered>();
    const clock = nowMs || 0;
    for (const t of filtered) {
      const key = t.pinned
        ? "Pinned"
        : clock
          ? dateBucket(t.updatedAt, clock)
          : "Recent";
      const list = map.get(key) || [];
      list.push(t);
      map.set(key, list);
    }
    // Pinned first, then Today, Yesterday, This week, then months
    const order = ["Pinned", "Today", "Yesterday", "This week", "Recent"];
    const keys = [...map.keys()].sort((a, b) => {
      const ia = order.indexOf(a);
      const ib = order.indexOf(b);
      if (ia >= 0 || ib >= 0) return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib);
      return a.localeCompare(b);
    });
    return keys.map((k) => [k, map.get(k)!] as const);
  }, [filtered, nowMs]);

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
        <CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <CardTitle className="flex items-center gap-2 text-sm">
              <History className="h-4 w-4" />
              History
            </CardTitle>
            <CardDescription>
              Search, pin, and folder chats. Select to resume exactly where you left off.
            </CardDescription>
          </div>
          <Button size="sm" onClick={() => newThread()}>
            <MessageSquarePlus className="h-4 w-4" />
            New chat
          </Button>
        </CardHeader>
        <CardContent className="space-y-3 p-3 pt-0">
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--color-subtle)]" />
            <Input
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="Search title or message text…"
              className="pl-8"
            />
          </div>
          <div className="flex flex-wrap gap-1.5">
            {(
              [
                ["all", "All"],
                ["pinned", "Pinned"],
                ...folders.map((f) => [f, f] as const),
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                type="button"
                onClick={() => setFolderFilter(id)}
                className={cn(
                  "rounded-full border px-2.5 py-0.5 text-[11px] transition-colors",
                  folderFilter === id
                    ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]"
                    : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-border-strong)]",
                )}
              >
                {label}
              </button>
            ))}
          </div>

          {filtered.length === 0 && (
            <p className="px-2 py-8 text-center text-sm text-[var(--color-muted)]">
              {threads.length === 0 ? "No chats yet. Start one from Agent." : "No matches."}
            </p>
          )}
          {grouped.map(([group, list]) => (
            <div key={group} className="space-y-2">
              <div className="px-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-subtle)]">
                {group}
              </div>
              {list.map((t) => {
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
                <button
                  type="button"
                  className="min-w-0 flex-1 text-left"
                  onClick={() => selectThread(t.id)}
                >
                  <div className="flex flex-wrap items-center gap-1.5">
                    {t.pinned && <Pin className="h-3 w-3 text-[var(--color-info)]" />}
                    {isRenaming ? (
                      <input
                        ref={inputRef}
                        value={draft}
                        onClick={(e) => e.stopPropagation()}
                        onChange={(e) => setDraft(e.target.value)}
                        onBlur={commitRename}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") commitRename();
                          if (e.key === "Escape") setRenamingId(null);
                        }}
                        className="w-full max-w-xs rounded border border-[var(--color-border-strong)] bg-[var(--color-surface)] px-2 py-0.5 text-sm"
                      />
                    ) : (
                      <span className="truncate text-sm font-medium">{t.title}</span>
                    )}
                    {t.folder && (
                      <Badge className="text-[10px]">
                        <Folder className="mr-0.5 h-2.5 w-2.5" />
                        {t.folder}
                      </Badge>
                    )}
                    {t.mode && <Badge className="font-mono text-[10px]">{t.mode}</Badge>}
                  </div>
                  <p className="mt-0.5 line-clamp-2 text-xs text-[var(--color-muted)]">{preview}</p>
                  <div className="mt-1 text-[10px] text-[var(--color-subtle)]">
                    <RelativeTime ts={t.updatedAt} /> · {t.messages.length} messages
                  </div>
                </button>
                <div className="flex shrink-0 flex-col gap-0.5 opacity-70 group-hover:opacity-100">
                  <Button
                    size="icon"
                    variant="ghost"
                    className="h-7 w-7"
                    title={t.pinned ? "Unpin" : "Pin"}
                    onClick={() => pinThread(t.id)}
                  >
                    {t.pinned ? <PinOff className="h-3.5 w-3.5" /> : <Pin className="h-3.5 w-3.5" />}
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="h-7 w-7"
                    title="Rename"
                    onClick={() => startRename(t.id, t.title)}
                  >
                    <Pencil className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="h-7 w-7"
                    title="Folder"
                    onClick={() => {
                      setFolderDraftId(t.id);
                      setFolderDraft(t.folder || "");
                    }}
                  >
                    <Folder className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="h-7 w-7 text-[var(--color-danger)]"
                    title="Delete"
                    onClick={() => {
                      if (window.confirm("Delete this chat?")) deleteThread(t.id);
                    }}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
            );
          })}
            </div>
          ))}

          {folderDraftId && (
            <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
              <Card className="w-full max-w-sm">
                <CardHeader>
                  <CardTitle className="text-sm">Set folder</CardTitle>
                  <CardDescription>Group chats (e.g. Work, Arch, Imagine).</CardDescription>
                </CardHeader>
                <CardContent className="space-y-3">
                  <Input
                    value={folderDraft}
                    onChange={(e) => setFolderDraft(e.target.value)}
                    placeholder="Folder name (empty to clear)"
                    autoFocus
                  />
                  <div className="flex justify-end gap-2">
                    <Button size="sm" variant="secondary" onClick={() => setFolderDraftId(null)}>
                      Cancel
                    </Button>
                    <Button
                      size="sm"
                      onClick={() => {
                        setThreadFolder(folderDraftId, folderDraft.trim() || null);
                        setFolderDraftId(null);
                      }}
                    >
                      Save
                    </Button>
                  </div>
                </CardContent>
              </Card>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

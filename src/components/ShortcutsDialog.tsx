import { SHORTCUTS } from "@/lib/shortcuts";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "./ui/dialog";

export function ShortcutsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const scopes = [...new Set(SHORTCUTS.map((s) => s.scope))];
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <DialogDescription>Speed up everyday GrokHub workflows.</DialogDescription>
        </DialogHeader>
        <div className="scroll-panel max-h-[min(60dvh,28rem)] space-y-4 pr-1">
          {scopes.map((scope) => (
            <div key={scope}>
              <div className="mb-1.5 text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]">
                {scope}
              </div>
              <ul className="space-y-1">
                {SHORTCUTS.filter((s) => s.scope === scope).map((s) => (
                  <li
                    key={s.keys + s.action}
                    className="flex items-center justify-between gap-3 rounded-[var(--radius-sm)] px-1 py-1 text-sm"
                  >
                    <span className="text-[var(--color-muted)]">{s.action}</span>
                    <kbd className="shrink-0 rounded border border-[var(--color-border)] bg-[var(--color-elevated)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--color-fg)]">
                      {s.keys}
                    </kbd>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}

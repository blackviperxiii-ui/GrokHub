import { useEffect } from "react";
import { useGrokHub } from "@/lib/store";
import { Button } from "./ui/button";

export function UndoToast() {
  const undo = useGrokHub((s) => s.undoBuffer);
  const undoLastDelete = useGrokHub((s) => s.undoLastDelete);
  const clearUndo = useGrokHub((s) => s.clearUndoBuffer);

  useEffect(() => {
    if (!undo) return;
    const left = Math.max(500, undo.expiresAt - Date.now());
    const t = window.setTimeout(() => clearUndo(), left);
    return () => window.clearTimeout(t);
  }, [undo, clearUndo]);

  if (!undo || undo.expiresAt < Date.now()) return null;

  return (
    <div className="pointer-events-auto fixed bottom-4 left-1/2 z-[80] flex -translate-x-1/2 items-center gap-3 rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-panel)] px-3 py-2 shadow-[var(--shadow-soft)]">
      <span className="text-sm text-[var(--color-fg)]">{undo.label}</span>
      <Button size="sm" variant="secondary" onClick={() => undoLastDelete()}>
        Undo
      </Button>
      <Button size="sm" variant="ghost" onClick={() => clearUndo()}>
        Dismiss
      </Button>
    </div>
  );
}

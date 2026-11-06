import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { chipOutcome, quickChips, type ChipSurface, type QuickChip, type SessionMode } from "./api";
import { onArrowFocus, usePeek } from "./peek";

/** At most this many chips in a row, as in the cabin. */
export const CHIP_ROW_MAX = 3;
/** How long the box sits still before its row is asked for again. */
export const DRAFT_SETTLE_MS = 250;
export const HIDE_TIP = "Hide this suggestion";

/** The chip's tip: why it is here, and its key. */
export function chipTip(chip: QuickChip, slot: number): string {
  const why = chip.hint.trim() ? `Why this? ${chip.hint.trim()}` : chip.value;
  return `${why}  (Alt+${slot + 1})`;
}

/** The row's key for Alt+1, Alt+2 and Alt+3; layouts that type a symbol
 * with Alt still report the digit key. */
export function chipSlotOf(event: { altKey: boolean; ctrlKey: boolean; metaKey: boolean; code: string }): number | null {
  if (!event.altKey || event.ctrlKey || event.metaKey) return null;
  const m = /^Digit([1-9])$/.exec(event.code);
  if (!m) return null;
  const slot = Number(m[1]) - 1;
  return slot < CHIP_ROW_MAX ? slot : null;
}

/** One row of quick chips: the same look on every page. Each chip has its
 * tap and its ×, Left and Right move along the row, and Alt+1 to Alt+3 tap
 * a chip while the row is on screen (`keys`). */
export function ChipRow({
  chips,
  label = "Suggestions",
  busy = false,
  keys = true,
  onTap,
  onHide,
}: {
  chips: QuickChip[];
  label?: string;
  busy?: boolean;
  keys?: boolean;
  onTap: (chip: QuickChip) => void;
  onHide: (chip: QuickChip) => void;
}) {
  const row = useRef<HTMLDivElement>(null);
  const shown = chips.slice(0, CHIP_ROW_MAX);
  usePeek(row, "x", shown.length);
  // A layout effect, so Alt+N works from the moment the row is on screen.
  useLayoutEffect(() => {
    if (!keys || busy || shown.length === 0) return;
    const onKey = (event: KeyboardEvent) => {
      const slot = chipSlotOf(event);
      if (slot === null || !shown[slot]) return;
      event.preventDefault();
      onTap(shown[slot]);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [keys, busy, shown, onTap]);
  if (shown.length === 0) return null;
  return (
    <div className="chip-row" role="group" aria-label={label}>
      <div ref={row} className="chip-row-items" onKeyDown={(e) => onArrowFocus(e, "x")}>
        {shown.flatMap((chip, slot) => [
          <button
            key={`${chip.id}-tap`}
            type="button"
            className={chip.primary ? "quick-chip primary" : "quick-chip"}
            title={chipTip(chip, slot)}
            disabled={busy}
            onClick={() => onTap(chip)}
          >
            {chip.label}
          </button>,
          <button
            key={`${chip.id}-hide`}
            type="button"
            className="quick-chip-hide"
            aria-label={`Hide ${chip.label}`}
            title={HIDE_TIP}
            onClick={() => onHide(chip)}
          >
            ×
          </button>,
        ])}
      </div>
    </div>
  );
}

/** A surface's row from the shell's chip engine. It is asked for again when
 * the chat, its box (once typing settles), its mode or `turn` changes; while
 * `busy` or not `active` the row is empty. `picked` and `hide` teach the
 * ranker, and either one asks for a fresh row. */
export function useChipRow({
  surface,
  conversationId,
  draft = "",
  mode,
  turn = 0,
  busy = false,
  active = true,
}: {
  surface: ChipSurface;
  conversationId?: string;
  draft?: string;
  mode?: SessionMode;
  turn?: number;
  busy?: boolean;
  active?: boolean;
}) {
  const [chips, setChips] = useState<QuickChip[]>([]);
  const [nonce, setNonce] = useState(0);
  const wanted = active && !busy;
  useEffect(() => {
    if (!wanted) return;
    let live = true;
    const ask = () =>
      quickChips(surface, conversationId, draft, mode).then(
        (row) => {
          if (live) setChips(row);
        },
        () => {
          if (live) setChips([]);
        },
      );
    const timer = setTimeout(() => void ask(), draft ? DRAFT_SETTLE_MS : 0);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [wanted, surface, conversationId, draft, mode, turn, nonce]);

  function picked(chip: QuickChip) {
    setChips([]);
    void chipOutcome(surface, chip.id, true).then(
      () => setNonce((n) => n + 1),
      () => setNonce((n) => n + 1),
    );
  }

  function hide(chip: QuickChip) {
    setChips((now) => now.filter((c) => c.id !== chip.id));
    void chipOutcome(surface, chip.id, false).then(
      () => setNonce((n) => n + 1),
      () => setNonce((n) => n + 1),
    );
  }

  return { chips: wanted ? chips : [], picked, hide };
}

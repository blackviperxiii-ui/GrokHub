import type { ReplyChipData } from "./api";

/** Predicted replies on a card that waits on you (Order 733, the cabin's
 * reply chips): one tap answers the card, fills its reply box, or sets it
 * aside. The shell ranks them from your past picks; this only paints them.
 * What a tap does is the card's business (`onPick`). */
export function ReplyChips({
  chips,
  onPick,
}: {
  chips: ReplyChipData[];
  onPick: (chip: ReplyChipData) => void;
}) {
  if (chips.length === 0) return null;
  return (
    <div className="reply-chips" role="group" aria-label="Suggested replies">
      {chips.map((chip) => (
        <button
          key={chip.key}
          type="button"
          className={chip.act === "dismiss" ? "chip reply-chip quiet" : "chip reply-chip"}
          onClick={() => onPick(chip)}
        >
          {chip.label}
        </button>
      ))}
    </div>
  );
}

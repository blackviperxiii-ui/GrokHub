import { useEffect, useState } from "react";
import { useDismiss } from "./ui";
import { DOCK_MAX, dockItems, dockSummary, stepsLine, type ActivityItem } from "./activity";

const STATUS_WORD: Record<ActivityItem["status"], string> = {
  running: "Running",
  waiting: "Waiting on you",
  done: "Done",
  failed: "Failed",
};

/** One dock in the views bar, so it follows every view: what the agent is
 * running, what waits on the user's OK, and what just finished. Chat items
 * open the chat (where the approval card is), and their work card when the
 * chat works one; routine runs open Automations with the result. Hidden while there is nothing to show. */
export function ActivityDock({
  items,
  onOpenChat,
  onOpenAutomations,
  cardFor,
  onOpenCard,
  now: fixedNow,
}: {
  items: ActivityItem[];
  onOpenChat: () => void;
  onOpenAutomations: () => void;
  /** The work card a chat works, if any (keyed by conversation id). */
  cardFor?: (conversationId: string) => string | undefined;
  onOpenCard?: (cardId: string) => void;
  /** Tests pin the clock; the app ticks it so finished items age out. */
  now?: number;
}) {
  const [open, setOpen] = useState(false);
  const dock = useDismiss<HTMLElement>(open, () => setOpen(false));
  const [tick, setTick] = useState(() => Date.now());
  useEffect(() => {
    if (fixedNow !== undefined) return;
    const timer = setInterval(() => setTick(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, [fixedNow]);
  const now = fixedNow ?? Math.max(tick, ...items.map((i) => i.at));
  const shown = dockItems(items, now);
  if (shown.length === 0) return null;
  const waiting = shown.some((i) => i.status === "waiting");
  const listed = shown.slice(0, DOCK_MAX);
  const more = shown.length - listed.length;

  return (
    <aside
      ref={dock}
      className={waiting ? "activity-dock waiting" : "activity-dock"}
      aria-label="Activity"
    >
      <button
        type="button"
        className="activity-toggle"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <span className={`activity-dot ${shown[0].status}`} aria-hidden="true" />
        <span className="activity-label">Activity</span>
        <span className="activity-summary">{dockSummary(shown)}</span>
      </button>
      {open && (
        <ul className="activity-list">
          {listed.map((item) => (
            <li key={item.key} className={`activity-item ${item.status}`} aria-label={item.title}>
              <div className="activity-head">
                <span className="activity-title">{item.title}</span>
                <span className="activity-status">{STATUS_WORD[item.status]}</span>
              </div>
              <div className="activity-detail">{item.detail}</div>
              {item.status === "running" && (
                <div className="activity-progress" aria-hidden="true">
                  <i />
                </div>
              )}
              {stepsLine(item) && <div className="activity-steps">{stepsLine(item)}</div>}
              <button
                type="button"
                className="quiet"
                onClick={() => {
                  setOpen(false);
                  if (item.kind === "chat") onOpenChat();
                  else onOpenAutomations();
                }}
              >
                {item.kind === "chat"
                  ? item.status === "waiting"
                    ? "Review"
                    : "Open chat"
                  : "Open result"}
              </button>
              {(() => {
                const card = item.kind === "chat" ? cardFor?.(item.key.slice("chat:".length)) : undefined;
                return card && onOpenCard ? (
                  <button
                    type="button"
                    className="quiet"
                    onClick={() => {
                      setOpen(false);
                      onOpenCard(card);
                    }}
                  >
                    Open card
                  </button>
                ) : null;
              })()}
            </li>
          ))}
          {more > 0 && <li className="activity-more">{more} more</li>}
        </ul>
      )}
    </aside>
  );
}

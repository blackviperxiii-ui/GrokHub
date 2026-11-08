import { useState } from "react";
import { useDismiss } from "./ui";
import { noticeAge, shownText, unreadBadge, type Notice, type Notices } from "./notices";

/** The Notifications menu in the views bar, beside Activity: the record of
 * what finished, failed or wants a look, so nothing pops up over the page.
 * The number counts what you haven't read; opening the menu reads them. */
export function NoticeBell({
  notices,
  onOpen,
  onClear,
  onAction,
  now: fixedNow,
}: {
  notices: Notices;
  /** Opening the menu marks everything read. */
  onOpen: () => void;
  onClear: () => void;
  onAction: (notice: Notice) => void;
  /** Tests pin the clock. */
  now?: number;
}) {
  const [open, setOpen] = useState(false);
  const bell = useDismiss<HTMLDivElement>(open, () => setOpen(false));
  const badge = unreadBadge(notices.unread);
  // The clock for "5 min ago", read each time the menu opens.
  const [clock, setClock] = useState(() => Date.now());
  const now = fixedNow ?? clock;
  const label = badge ? `Notifications, ${badge} new` : "Notifications";

  return (
    <div ref={bell} className="notice-bell">
      <button
        type="button"
        className="notice-toggle"
        aria-label={label}
        title={label}
        aria-expanded={open}
        onClick={() => {
          if (!open) {
            setClock(Date.now());
            onOpen();
          }
          setOpen((o) => !o);
        }}
      >
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
          <path
            d="M12 3a6 6 0 0 0-6 6v3.2L4.6 15a1 1 0 0 0 .9 1.5h13a1 1 0 0 0 .9-1.5L18 12.2V9a6 6 0 0 0-6-6Zm-2.5 15a2.5 2.5 0 0 0 5 0"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
        {badge && (
          <span className="notice-badge" aria-hidden="true">
            {badge}
          </span>
        )}
      </button>
      {open && (
        <div className="notice-menu" role="region" aria-label="Notifications">
          <div className="notice-menu-head">
            <span className="activity-title">Notifications</span>
            {notices.items.length > 0 && (
              <button type="button" className="link" onClick={onClear}>
                Clear all
              </button>
            )}
          </div>
          {notices.items.length === 0 ? (
            <p className="activity-detail">Nothing new. Finished work and anything that needs a look shows up here.</p>
          ) : (
            <ul className="notice-list">
              {notices.items.map((n) => (
                <li key={n.id} className={`notice-item ${n.kind}`}>
                  <span className="notice-text">{shownText(n)}</span>
                  <span className="activity-detail">
                    {n.from ? `${n.from} · ${noticeAge(n.at, now)}` : noticeAge(n.at, now)}
                  </span>
                  {n.action && (
                    <button
                      type="button"
                      className="quiet"
                      onClick={() => {
                        setOpen(false);
                        onAction(n);
                      }}
                    >
                      {n.action.label}
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

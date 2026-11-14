// The left rail: the main pages as icons, More for the rest (through the
// palette, scoped to pages), and Settings at the foot. A page opened from
// More, a tile or a notice gets a slot of its own while it is open, so the
// rail always marks where you are.
import type { KeyboardEvent, MouseEvent } from "react";
import type { Capabilities } from "./api";
import { railViews, VIEW_LABELS, type View } from "./Palette";
import { TabIcon } from "./TabIcon";

const STEP: Record<string, 1 | -1> = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 };

/** Arrow keys move between the rail's buttons, wrapping; Home and End jump to
 * either end. Tab still walks them in order too. */
function onArrows(event: KeyboardEvent<HTMLElement>) {
  const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button")];
  const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
  if (at < 0) return;
  let next: number;
  if (event.key in STEP) next = (at + STEP[event.key] + buttons.length) % buttons.length;
  else if (event.key === "Home") next = 0;
  else if (event.key === "End") next = buttons.length - 1;
  else return;
  event.preventDefault();
  buttons[next].focus();
}

export function Rail({
  caps,
  view,
  moreOpen,
  onOpen,
  onMore,
  onChatMenu,
}: {
  caps: Capabilities;
  view: View;
  moreOpen: boolean;
  onOpen: (view: View) => void;
  onMore: () => void;
  /** Chat's right-click menu (Clear chat), at the pointer; desktops only. */
  onChatMenu: (at: { x: number; y: number }) => void;
}) {
  const rail = railViews(caps);
  const top = rail.filter((name) => name !== "settings");
  const slot = rail.includes(view) ? null : view;

  function item(name: View) {
    const chatMenu =
      name === "chat" && !caps.remote
        ? (event: MouseEvent) => {
            event.preventDefault();
            onChatMenu({ x: event.clientX, y: event.clientY });
          }
        : undefined;
    return (
      <button
        key={name}
        type="button"
        title={VIEW_LABELS[name]}
        aria-current={view === name ? "page" : undefined}
        onClick={() => onOpen(name)}
        onContextMenu={chatMenu}
      >
        <TabIcon view={name} size={20} />
        <span className="rail-label">{VIEW_LABELS[name]}</span>
      </button>
    );
  }

  return (
    <nav className="rail" aria-label="Views" onKeyDown={onArrows}>
      {top.map(item)}
      {slot && item(slot)}
      <div className="rail-end">
        <button
          type="button"
          className="rail-more"
          title="More"
          aria-haspopup="dialog"
          aria-expanded={moreOpen}
          onClick={onMore}
        >
          <TabIcon view="more" size={20} />
          <span className="rail-label">More</span>
        </button>
        {rail.includes("settings") && item("settings")}
      </div>
    </nav>
  );
}

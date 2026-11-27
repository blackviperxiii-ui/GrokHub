// The left rail lists every page in collapsible groups, with pins at the top.
// Settings stays at the foot. A page that is not in the groups still gets a
// temporary slot while it is open so the rail marks where you are.
import { type KeyboardEvent, type MouseEvent } from "react";
import type { Capabilities, Profile } from "./api";
import { railSections, railViews, VIEW_LABELS, type RailSectionId, type View } from "./Palette";
import { Avatar } from "./Profile";
import { TabIcon } from "./TabIcon";

const STEP: Record<string, 1 | -1> = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 };

/** Page buttons on the rail; section headers stay on Tab, not arrows. */
function pageButtons(nav: HTMLElement): HTMLButtonElement[] {
  return [...nav.querySelectorAll<HTMLButtonElement>("button")].filter(
    (b) => getComputedStyle(b).display !== "none" && !b.classList.contains("rail-section-toggle"),
  );
}

/** Arrow keys move between the rail's page buttons, wrapping; Home and End
 * jump to either end. Tab still walks section headers too. */
function onArrows(event: KeyboardEvent<HTMLElement>) {
  const buttons = pageButtons(event.currentTarget);
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
  pinned,
  collapsed,
  onOpen,
  onChatMenu,
  onPin,
  onToggleSection,
  profile = null,
  onProfile,
}: {
  caps: Capabilities;
  view: View;
  pinned: readonly View[];
  collapsed: readonly RailSectionId[];
  onOpen: (view: View) => void;
  /** Chat's right-click menu (Clear chat), at the pointer; desktops only. */
  onChatMenu: (at: { x: number; y: number }) => void;
  onPin: (view: View, pinned: boolean) => void;
  onToggleSection: (id: RailSectionId) => void;
  /** Your name and picture, at the foot; with `onProfile` it opens Settings → Account. */
  profile?: Profile | null;
  onProfile?: () => void;
}) {
  const opts = { pinned: [...pinned] };
  const rail = railViews(caps, opts);
  const top = rail.filter((name) => name !== "settings");
  const slot = rail.includes(view) ? null : view;
  const sections = railSections(caps, opts);
  const pinnedSet = new Set(pinned);
  const folded = new Set(collapsed);

  function item(name: View) {
    function onContext(event: MouseEvent) {
      // Chat keeps Clear chat on desktop; Shift+right-click pins instead.
      if (name === "chat" && !caps.remote && !event.shiftKey) {
        event.preventDefault();
        onChatMenu({ x: event.clientX, y: event.clientY });
        return;
      }
      if (name === "settings") return;
      event.preventDefault();
      onPin(name, !pinnedSet.has(name));
    }
    const pinHint =
      name === "settings"
        ? undefined
        : pinnedSet.has(name)
          ? "Pinned. Right-click to unpin."
          : "Right-click to pin.";
    return (
      <button
        key={name}
        type="button"
        title={VIEW_LABELS[name]}
        aria-description={pinHint}
        aria-current={view === name ? "page" : undefined}
        onClick={() => onOpen(name)}
        onContextMenu={onContext}
      >
        <TabIcon view={name} size={20} />
        <span className="rail-label">{VIEW_LABELS[name]}</span>
      </button>
    );
  }

  const topSet = new Set<string>(top);
  const pinnedTop = pinned.filter((name) => topSet.has(name));

  return (
    <nav className="rail rail-advanced" aria-label="Views" onKeyDown={onArrows}>
      {pinnedTop.map((name) => item(name))}
      {sections.map((section) => {
        const open = !folded.has(section.id);
        return (
          <div key={section.id} className="rail-section" data-section={section.id}>
            <button
              type="button"
              className="rail-section-toggle"
              aria-expanded={open}
              onClick={() => onToggleSection(section.id)}
            >
              <span aria-hidden="true">{open ? "▾" : "▸"}</span>
              <span className="rail-section-label">{section.label}</span>
            </button>
            {open && section.views.map((name) => item(name))}
          </div>
        );
      })}
      {slot && item(slot)}
      <div className="rail-end">
        {rail.includes("settings") && item("settings")}
        {profile &&
          (onProfile ? (
            <button
              type="button"
              className="rail-avatar"
              title={profile.shownName}
              aria-label={`${profile.shownName}, Account settings`}
              onClick={onProfile}
            >
              <Avatar profile={profile} className="rail-avatar-face" />
            </button>
          ) : (
            <span className="rail-avatar" role="img" title={profile.shownName} aria-label={profile.shownName}>
              <Avatar profile={profile} className="rail-avatar-face" />
            </span>
          ))}
      </div>
    </nav>
  );
}

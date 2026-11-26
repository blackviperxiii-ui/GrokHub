// The left rail: Simple keeps the chat-first icons; Advanced shows every page
// in collapsible groups (Work / Make / You / …) with pin/unpin. More stays the
// overflow list. A page opened from More, a tile or a notice still gets a
// temporary slot while it is open so the rail marks where you are. Narrow
// screens always use the Simple layout so the phone tab bar stays usable.
import { useEffect, useState, type KeyboardEvent, type MouseEvent } from "react";
import type { Capabilities, Profile } from "./api";
import {
  moreViews,
  railSections,
  railViews,
  VIEW_LABELS,
  type RailMode,
  type RailSectionId,
  type View,
} from "./Palette";
import { Avatar } from "./Profile";
import { TabIcon } from "./TabIcon";

const NARROW = "(max-width: 600px)";

const STEP: Record<string, 1 | -1> = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 };

/** Page buttons on the rail; mode and section chrome stay on Tab, not arrows. */
function pageButtons(nav: HTMLElement): HTMLButtonElement[] {
  return [...nav.querySelectorAll<HTMLButtonElement>("button")].filter(
    (b) =>
      getComputedStyle(b).display !== "none" &&
      !b.classList.contains("rail-mode-btn") &&
      !b.classList.contains("rail-section-toggle"),
  );
}

/** Arrow keys move between the rail's page buttons, wrapping; Home and End
 * jump to either end. Tab still walks mode and section chrome too. */
function onArrows(event: KeyboardEvent<HTMLElement>) {
  // A button the layout hides (the avatar in the phone tab bar) is skipped.
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
  mode,
  pinned,
  collapsed,
  moreOpen,
  onOpen,
  onMore,
  onChatMenu,
  onMode,
  onPin,
  onToggleSection,
  profile = null,
  onProfile,
}: {
  caps: Capabilities;
  view: View;
  mode: RailMode;
  pinned: readonly View[];
  collapsed: readonly RailSectionId[];
  moreOpen: boolean;
  onOpen: (view: View) => void;
  onMore: () => void;
  /** Chat's right-click menu (Clear chat), at the pointer; desktops only. */
  onChatMenu: (at: { x: number; y: number }) => void;
  onMode: (mode: RailMode) => void;
  onPin: (view: View, pinned: boolean) => void;
  onToggleSection: (id: RailSectionId) => void;
  /** Your name and picture, at the foot; with `onProfile` it opens Settings → Account. */
  profile?: Profile | null;
  onProfile?: () => void;
}) {
  const [narrow, setNarrow] = useState(() => window.matchMedia?.(NARROW).matches ?? false);
  useEffect(() => {
    const mq = window.matchMedia?.(NARROW);
    if (!mq) return;
    const sync = () => setNarrow(mq.matches);
    sync();
    mq.addEventListener("change", sync);
    return () => mq.removeEventListener("change", sync);
  }, []);
  // Phone tab bar: always the Simple set (+ pins), even when Advanced is on.
  const layoutMode: RailMode = narrow ? "simple" : mode;
  const opts = { mode: layoutMode, pinned: [...pinned] };
  const rail = railViews(caps, opts);
  const overflow = moreViews(caps, opts);
  const top = rail.filter((name) => name !== "settings");
  const slot = rail.includes(view) ? null : view;
  const advanced = layoutMode === "advanced";
  const sections = advanced ? railSections(caps, opts) : [];
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
  const simpleTop = advanced ? pinnedTop : top;

  return (
    <nav
      className={advanced ? "rail rail-advanced" : "rail"}
      aria-label="Views"
      data-rail-mode={layoutMode}
      onKeyDown={onArrows}
    >
      <div className="rail-mode" role="group" aria-label="Sidebar mode">
        <button
          type="button"
          className="rail-mode-btn"
          aria-pressed={mode === "simple"}
          title="Simple"
          aria-description="Core pages on the rail"
          onClick={() => onMode("simple")}
        >
          Simple
        </button>
        <button
          type="button"
          className="rail-mode-btn"
          aria-pressed={mode === "advanced"}
          title="Advanced"
          aria-description="Every page grouped on the rail"
          onClick={() => onMode("advanced")}
        >
          Advanced
        </button>
      </div>
      {simpleTop.map((name) => item(name))}
      {advanced &&
        sections.map((section) => {
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
        {overflow.length > 0 && (
          <button
            type="button"
            className="rail-more"
            title="More"
            aria-description={`${overflow.length} page${overflow.length === 1 ? "" : "s"} not on the rail`}
            aria-haspopup="dialog"
            aria-expanded={moreOpen}
            onClick={onMore}
          >
            <TabIcon view="more" size={20} />
            <span className="rail-label">More</span>
            {advanced && <span className="rail-more-count">{overflow.length}</span>}
          </button>
        )}
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

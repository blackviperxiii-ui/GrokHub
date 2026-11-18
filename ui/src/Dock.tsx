import { Fragment, useRef, useState } from "react";
import type { DragEvent, PointerEvent, ReactNode } from "react";
import type { DockLayout, DockPane } from "./api";
import { VIEW_LABELS, type View } from "./Palette";

/**
 * Settings → Behavior → Dockable panels: the cabin's dock as a web-native
 * split. The main pages become tabs in side-by-side panes; a page is a tab
 * in exactly one pane, and the focused pane's active tab is the page the
 * window is on (the cabin's focused-leaf rule). The layout is plain data in
 * shell.json (src-tauri/src/settings.rs); this module owns its rules.
 */

/** The pages the dock holds: the cabin's six main pages, in cabin order. */
export const DOCKABLE: View[] = ["chat", "imagine", "automations", "skills", "workboard", "pulse"];

export function isDockable(page: View): boolean {
  return DOCKABLE.includes(page);
}

/** The layout before anything is saved: one pane holding Chat. */
export function emptyLayout(): DockLayout {
  return { panes: [{ tabs: ["chat"], active: 0, share: 1000, id: "pane-1" }], focused: 0 };
}

function sameLayout(a: DockLayout, b: DockLayout): boolean {
  return (
    a.focused === b.focused &&
    a.panes.length === b.panes.length &&
    a.panes.every((pane, i) => {
      const other = b.panes[i];
      return (
        pane.id === other.id &&
        pane.active === other.active &&
        pane.share === other.share &&
        pane.tabs.length === other.tabs.length &&
        pane.tabs.every((tab, j) => tab === other.tabs[j])
      );
    })
  );
}

/** The first `pane-<n>` name no pane in the row is using. */
function freshId(panes: DockPane[]): string {
  const used = new Set(panes.map((pane) => pane.id));
  let n = 1;
  while (used.has(`pane-${n}`)) n += 1;
  return `pane-${n}`;
}

/**
 * A saved layout made true for this window: tabs limited to the dockable
 * pages this platform has, a page kept in its first pane only, available
 * pages missing everywhere folded into the focused pane (in the cabin every
 * page always had a tab), no empty panes, indexes in range. Returns the
 * same object when nothing had to change.
 */
export function cleanLayout(layout: DockLayout, available: View[]): DockLayout {
  const usable = DOCKABLE.filter((page) => available.includes(page));
  const seen = new Set<string>();
  const panes: DockPane[] = [];
  for (const pane of layout.panes) {
    const tabs = pane.tabs.filter(
      (tab) => usable.includes(tab as View) && !seen.has(tab) && seen.add(tab),
    );
    if (tabs.length === 0) continue;
    panes.push({
      id: pane.id ?? "",
      tabs,
      active: Math.min(Math.max(pane.active, 0), tabs.length - 1),
      share: pane.share > 0 && Number.isFinite(pane.share) ? pane.share : 1000,
    });
  }
  if (panes.length === 0) {
    return {
      panes: [{ id: "pane-1", tabs: usable.length > 0 ? [...usable] : ["chat"], active: 0, share: 1000 }],
      focused: 0,
    };
  }
  const focused = Math.min(Math.max(layout.focused, 0), panes.length - 1);
  const missing = usable.filter((page) => !seen.has(page));
  if (missing.length > 0) panes[focused] = { ...panes[focused], tabs: [...panes[focused].tabs, ...missing] };
  // Panes keep their names; a missing or repeated name gets a fresh one, so
  // a pane's identity survives its tabs changing.
  const named = new Set<string>();
  const identified = panes.map((pane) => {
    if (pane.id && !named.has(pane.id)) {
      named.add(pane.id);
      return pane;
    }
    const id = freshId(panes.filter((p) => named.has(p.id)));
    named.add(id);
    return { ...pane, id };
  });
  const next: DockLayout = { panes: identified, focused };
  return sameLayout(layout, next) ? layout : next;
}

/** Focus the pane holding `page` and make it that pane's active tab. A page
 * with no tab yet joins the focused pane; a non-dockable page changes nothing. */
export function focusPage(layout: DockLayout, page: View): DockLayout {
  if (!isDockable(page)) return layout;
  const at = layout.panes.findIndex((pane) => pane.tabs.includes(page));
  if (at < 0) {
    const focused = layout.panes[layout.focused] ?? layout.panes[0];
    if (!focused) return layout;
    const panes = layout.panes.map((pane, i) =>
      i === layout.focused
        ? { ...pane, tabs: [...pane.tabs, page], active: pane.tabs.length }
        : pane,
    );
    return { panes, focused: layout.focused };
  }
  const tab = layout.panes[at].tabs.indexOf(page);
  if (at === layout.focused && layout.panes[at].active === tab) return layout;
  const panes = layout.panes.map((pane, i) => (i === at ? { ...pane, active: tab } : pane));
  return { panes, focused: at };
}

/** Where a dropped tab lands: in a pane, or split off to its left or right. */
export type DropZone = "in" | "before" | "after";

/** Move `page`'s tab to another pane, or split it into a new pane beside one.
 * The pane it leaves disappears when it held no other tab. */
export function moveTab(layout: DockLayout, page: View, toPane: number, zone: DropZone): DockLayout {
  const from = layout.panes.findIndex((pane) => pane.tabs.includes(page));
  if (from < 0 || toPane < 0 || toPane >= layout.panes.length) return layout;
  if (zone === "in" && from === toPane) return focusPage(layout, page);
  if (zone !== "in" && from === toPane && layout.panes[from].tabs.length === 1) return layout;
  const leaving = layout.panes[from].tabs.filter((tab) => tab !== page);
  const panes: DockPane[] = [];
  layout.panes.forEach((pane, i) => {
    if (i === from) {
      if (leaving.length > 0) {
        panes.push({ ...pane, tabs: leaving, active: Math.min(pane.active, leaving.length - 1) });
      }
      return;
    }
    panes.push(pane);
  });
  // The target's place in the new row: itself, unless the pane that was
  // removed stood before it.
  const target = layout.panes[toPane];
  let at = panes.indexOf(target);
  if (at < 0) return layout;
  if (zone === "in") {
    const tabs = [...target.tabs.filter((tab) => tab !== page), page];
    panes[at] = { ...target, tabs, active: tabs.length - 1 };
    return { panes, focused: at };
  }
  const half = Math.max(1, Math.floor(target.share / 2));
  panes[at] = { ...target, share: target.share - half };
  if (zone === "after") at += 1;
  panes.splice(at, 0, { id: freshId(panes), tabs: [page], active: 0, share: half });
  return { panes, focused: at };
}

/** The next pane over, wrapping around: what F6 walks through. */
export function cycleFocus(layout: DockLayout, dir: 1 | -1): DockLayout {
  if (layout.panes.length < 2) return layout;
  const focused = (layout.focused + dir + layout.panes.length) % layout.panes.length;
  return { ...layout, focused };
}

/** The page the window is on under this layout: the focused pane's active tab. */
export function focusedPage(layout: DockLayout): View | null {
  const pane = layout.panes[layout.focused];
  const tab = pane?.tabs[pane.active];
  return tab ? (tab as View) : null;
}

const DRAG_TYPE = "application/x-grokhub-dock-page";

/**
 * The dock itself: a row of panes with tab strips, drag-to-move tabs,
 * edge-drop splits and draggable dividers. Page bodies come from the App,
 * which owns what each page renders; `active` marks the focused pane, the
 * one whose page drives the window.
 */
export function Dock({
  layout,
  onLayout,
  onFocusPage,
  renderPage,
}: {
  layout: DockLayout;
  onLayout: (next: DockLayout) => void;
  onFocusPage: (page: View) => void;
  renderPage: (page: View, active: boolean) => ReactNode;
}) {
  const rowRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<number[] | null>(null);
  const [dropAt, setDropAt] = useState<{ pane: number; zone: DropZone } | null>(null);
  const [dragShares, setDragShares] = useState<number[] | null>(null);

  const zoneFor = (event: DragEvent, pane: number): DropZone => {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const x = (event.clientX - rect.left) / Math.max(rect.width, 1);
    if (layout.panes.length > 1 || layout.panes[pane].tabs.length > 1) {
      if (x < 0.25) return "before";
      if (x > 0.75) return "after";
    }
    return "in";
  };

  const startResize = (divider: number) => (event: PointerEvent) => {
    const row = rowRef.current;
    if (!row) return;
    event.preventDefault();
    const startX = event.clientX;
    const width = Math.max(row.getBoundingClientRect().width, 1);
    const start = layout.panes.map((pane) => pane.share);
    const pair = start[divider] + start[divider + 1];
    const move = (moveEvent: globalThis.PointerEvent) => {
      const delta = ((moveEvent.clientX - startX) / width) * pair * 2;
      const left = Math.min(Math.max(start[divider] + delta, 100), pair - 100);
      const shares = [...start];
      shares[divider] = Math.round(left);
      shares[divider + 1] = Math.round(pair - left);
      dragRef.current = shares;
      setDragShares(shares);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      const shares = dragRef.current;
      dragRef.current = null;
      setDragShares(null);
      if (shares) {
        onLayout({
          ...layout,
          panes: layout.panes.map((pane, i) => ({ ...pane, share: shares[i] ?? pane.share })),
        });
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const shares = dragShares ?? layout.panes.map((p) => p.share);
  return (
    <div className="dock" ref={rowRef}>
      {layout.panes.map((pane, i) => {
        const page = pane.tabs[pane.active] as View | undefined;
        return (
          <Fragment key={pane.id || pane.tabs.join("|")}>
            <section
              style={{ flexGrow: shares[i] }}
              className="dock-pane"
              data-focused={i === layout.focused || undefined}
              data-drop={dropAt?.pane === i ? dropAt.zone : undefined}
              aria-label={page ? `${VIEW_LABELS[page]} pane` : "Pane"}
              onDragOver={(event) => {
                if (!event.dataTransfer.types.includes(DRAG_TYPE)) return;
                event.preventDefault();
                setDropAt({ pane: i, zone: zoneFor(event, i) });
              }}
              onDragLeave={() => setDropAt((now) => (now?.pane === i ? null : now))}
              onDrop={(event) => {
                const dragged = event.dataTransfer.getData(DRAG_TYPE) as View;
                const zone = zoneFor(event, i);
                setDropAt(null);
                if (!isDockable(dragged)) return;
                event.preventDefault();
                onLayout(moveTab(layout, dragged, i, zone));
                onFocusPage(dragged);
              }}
            >
              <div className="dock-tabs" role="tablist" aria-label="Pane pages">
                {pane.tabs.map((tab) => {
                  const tabPage = tab as View;
                  return (
                    <button
                      key={tab}
                      type="button"
                      role="tab"
                      aria-selected={tab === pane.tabs[pane.active]}
                      className="dock-tab"
                      data-active={tab === pane.tabs[pane.active] || undefined}
                      draggable
                      onDragStart={(event) => {
                        event.dataTransfer.setData(DRAG_TYPE, tab);
                        event.dataTransfer.effectAllowed = "move";
                      }}
                      onDragEnd={() => setDropAt(null)}
                      onClick={() => onFocusPage(tabPage)}
                    >
                      {VIEW_LABELS[tabPage]}
                    </button>
                  );
                })}
              </div>
              <div
                className="dock-page"
                onMouseDown={() => {
                  if (page && i !== layout.focused) onFocusPage(page);
                }}
              >
                {page ? renderPage(page, i === layout.focused) : null}
              </div>
            </section>
            {i < layout.panes.length - 1 && (
              <div
                className="dock-divider"
                role="separator"
                aria-orientation="vertical"
                aria-label="Resize panes"
                onPointerDown={startResize(i)}
              />
            )}
          </Fragment>
        );
      })}
    </div>
  );
}

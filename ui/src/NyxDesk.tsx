import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { COMPANION_NAME } from "./brand";
import { NyxCard } from "./NyxCard";
import {
  desktopNyxDock,
  desktopNyxSet,
  desktopNyxSetOutfit,
  desktopNyxDrag,
  desktopNyxSetHit,
  type DesktopNyxHitBox,
} from "./desktopNyxApi";
import { useDesktopNyx } from "./useDesktopNyx";

/** Every outfit picture, including gaming (the chat dock leaves that out). */
const ART = import.meta.glob<string>(["../../brand/nyx/**/*.jpg", "../../brand/nyx-*.jpg"], {
  query: "?url",
  import: "default",
  eager: true,
});

export function deskArtUrl(path: string): string | null {
  return Object.hasOwn(ART, `../../${path}`) ? ART[`../../${path}`] : null;
}

/** How far a press on her moves before it drags her window. */
const DRAG_AFTER_PX = 4;

/** The desktop companion page: pedestal + figure, wardrobe, dock back. */
export function NyxDesk() {
  // The dock's wardrobe, Labs and the tray change her too.
  const [state, setState] = useDesktopNyx();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const artRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLUListElement>(null);
  const press = useRef<{ x: number; y: number } | null>(null);

  // A click on the desktop beside her passes through, so the page hears no
  // click: leaving the window closes the menu too.
  useEffect(() => {
    function onDoc() {
      setMenu(null);
    }
    document.addEventListener("click", onDoc);
    window.addEventListener("blur", onDoc);
    return () => {
      document.removeEventListener("click", onDoc);
      window.removeEventListener("blur", onDoc);
    };
  }, []);

  // Her pedestal and figure (the stage's children: the stage itself fills
  // the window) and an open menu take the mouse; the app lets clicks
  // through everywhere else, watching the cursor itself (a window that lets
  // clicks through hears no pointer). Sent again whenever they change size.
  useEffect(() => {
    const parts = () => [...(artRef.current?.children ?? []), ...(menuRef.current ? [menuRef.current] : [])];
    function report() {
      const boxes: DesktopNyxHitBox[] = parts().map((el) => {
        const r = el.getBoundingClientRect();
        return { x: r.left, y: r.top, w: r.width, h: r.height };
      });
      void desktopNyxSetHit(boxes);
    }
    report();
    window.addEventListener("resize", report);
    const sizes = typeof ResizeObserver === "function" ? new ResizeObserver(report) : null;
    for (const el of parts()) sizes?.observe(el);
    return () => {
      window.removeEventListener("resize", report);
      sizes?.disconnect();
    };
  }, [state, menu]);

  // A press on her closes her menu; moving a few pixels with the button
  // down drags her window (a press alone stays a click, since the system
  // keeps the release once a drag starts).
  function onPointerDown(event: ReactPointerEvent) {
    if (event.button !== 0) return;
    if (!artRef.current?.contains(event.target as Node)) return;
    setMenu(null);
    press.current = { x: event.clientX, y: event.clientY };
    // The release comes back here even off her, so a stale press can't
    // drag her on a later hover.
    try {
      event.currentTarget.setPointerCapture(event.pointerId);
    } catch {
      // No capture (an old webview): the release over her still clears it.
    }
  }

  function onPointerMove(event: ReactPointerEvent) {
    const from = press.current;
    if (!from) return;
    if (Math.hypot(event.clientX - from.x, event.clientY - from.y) < DRAG_AFTER_PX) return;
    press.current = null;
    void desktopNyxDrag();
  }

  function onPointerUp() {
    press.current = null;
  }

  if (!state) {
    return <div className="nyx-desk" aria-label={COMPANION_NAME} />;
  }

  const url = deskArtUrl(state.outfitFile);
  const name = state.companion || COMPANION_NAME;

  return (
    <div
      className="nyx-desk"
      aria-label={name}
      onContextMenu={(event) => {
        event.preventDefault();
        setMenu({ x: event.clientX, y: event.clientY });
      }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    >
      <div className="nyx-desk-stage" ref={artRef}>
        <div className="nyx-desk-pedestal" aria-hidden="true">
          <div className="nyx-desk-cone" />
          <div className="nyx-desk-disc" />
          <div className="nyx-desk-ring nyx-desk-ring-outer" />
          <div className="nyx-desk-ring nyx-desk-ring-inner" />
        </div>
        {url && (
          <NyxCard
            className="nyx-desk-figure"
            url={url}
            path={state.outfitFile}
            draggable={false}
            onClick={() => {
              /* open main chat later; click focuses her for now */
            }}
          />
        )}
      </div>
      {menu && (
        <ul
          ref={menuRef}
          className="nyx-desk-menu"
          style={{ left: menu.x, top: menu.y }}
          role="menu"
          onClick={(e) => e.stopPropagation()}
        >
          <li role="none">
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                setMenu(null);
                void desktopNyxDock().then(setState);
              }}
            >
              Bring her back
            </button>
          </li>
          <li role="none">
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                setMenu(null);
                void desktopNyxSet({ behind: !state.behind }).then(setState);
              }}
            >
              {state.behind ? "Float above windows" : "Stay behind windows"}
            </button>
          </li>
          <li className="nyx-desk-menu-label" role="presentation">
            Change outfit
          </li>
          {state.outfits.map((o) => (
            <li key={o.id} role="none">
              <button
                type="button"
                role="menuitem"
                aria-checked={state.outfit === o.id}
                onClick={() => {
                  setMenu(null);
                  void desktopNyxSetOutfit(o.id).then(setState);
                }}
              >
                {o.label}
              </button>
            </li>
          ))}
          <li role="none">
            <button
              type="button"
              role="menuitem"
              aria-checked={state.outfit == null}
              onClick={() => {
                setMenu(null);
                void desktopNyxSetOutfit(null).then(setState);
              }}
            >
              Pick for me
            </button>
          </li>
        </ul>
      )}
    </div>
  );
}

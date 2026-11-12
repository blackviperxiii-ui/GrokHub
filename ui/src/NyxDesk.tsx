import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { COMPANION_NAME } from "./brand";
import {
  desktopNyxDock,
  desktopNyxGet,
  desktopNyxSavePos,
  desktopNyxSet,
  desktopNyxSetOutfit,
  desktopNyxSetPassthrough,
  type DesktopNyxState,
} from "./desktopNyxApi";

/** Every outfit picture, including gaming (the chat dock leaves that out). */
const ART = import.meta.glob<string>(["../../brand/nyx/**/*.jpg", "../../brand/nyx-*.jpg"], {
  query: "?url",
  import: "default",
  eager: true,
});

export function deskArtUrl(path: string): string | null {
  return Object.hasOwn(ART, `../../${path}`) ? ART[`../../${path}`] : null;
}

const POS_SETTLE_MS = 600;

/** The desktop companion page: pedestal + figure, wardrobe, dock back. */
export function NyxDesk() {
  const [state, setState] = useState<DesktopNyxState | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const artRef = useRef<HTMLDivElement>(null);
  const settle = useRef<number | null>(null);
  const dragging = useRef(false);

  // Mount load only. Inlined like Memory / NyxDock (no refresh wrapper).
  useEffect(() => {
    void desktopNyxGet().then(setState, () => setState(null));
  }, []);

  useEffect(() => {
    function onDoc() {
      setMenu(null);
    }
    document.addEventListener("click", onDoc);
    return () => document.removeEventListener("click", onDoc);
  }, []);

  // Click-through when the pointer is over empty pixels.
  useEffect(() => {
    function onMove(event: PointerEvent) {
      const root = artRef.current;
      if (!root || typeof document.elementFromPoint !== "function") return;
      const hit = document.elementFromPoint(event.clientX, event.clientY);
      const over = !!hit && root.contains(hit);
      void desktopNyxSetPassthrough(!over);
    }
    window.addEventListener("pointermove", onMove);
    return () => window.removeEventListener("pointermove", onMove);
  }, []);

  function scheduleSavePos() {
    if (settle.current != null) window.clearTimeout(settle.current);
    settle.current = window.setTimeout(() => {
      settle.current = null;
      void desktopNyxSavePos(
        window.screenX,
        window.screenY,
        window.screen.width,
        window.screen.height,
      ).then(setState);
    }, POS_SETTLE_MS);
  }

  function onPointerDown(event: ReactPointerEvent) {
    if (event.button !== 0) return;
    dragging.current = true;
    void desktopNyxSetPassthrough(false);
  }

  function onPointerUp() {
    if (!dragging.current) return;
    dragging.current = false;
    scheduleSavePos();
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
          <img
            className="nyx-desk-figure"
            src={url}
            alt=""
            draggable={false}
            onClick={() => {
              /* open main chat later; click focuses her for now */
            }}
          />
        )}
      </div>
      {menu && (
        <ul
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

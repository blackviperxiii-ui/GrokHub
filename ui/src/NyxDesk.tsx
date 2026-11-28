import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { COMPANION_NAME } from "./brand";
import { NyxCard } from "./NyxCard";
import {
  desktopNyxDock,
  desktopNyxSet,
  desktopNyxSetOutfit,
  desktopNyxDrag,
  desktopNyxSetHit,
  onDesktopNyxTalk,
  type DesktopNyxHitBox,
  type DesktopNyxTalk,
} from "./desktopNyxApi";
import { FRAME_MS, SMOOTH_FRAMES, smoothLevelAt, TALK_HOP, talkHop } from "./lipsync";
import { Menu, MenuItem } from "./Menu";
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

/** Her float (px) and breath (share of her height, and of her width) in
 * styles.css `nyx-desk-bob` and `nyx-desk-breath`. */
const FLOAT_PX = 3;
const BREATH_LIFT = 0.012;
const BREATH_SWELL = 0.006;

const tenth = (v: number) => Math.round(v * 10) / 10;

/** Her figure's box grown by everywhere her float, breath and talk hop can
 * take it, so a box measured mid-breath or mid-hop still covers her at the
 * top of the next one. */
export function figureReach(r: DesktopNyxHitBox): DesktopNyxHitBox {
  const hop = TALK_HOP * r.h;
  const up = FLOAT_PX + BREATH_LIFT * r.h + hop;
  const side = (BREATH_SWELL * r.w) / 2;
  return { x: tenth(r.x - side), y: tenth(r.y - up), w: tenth(r.w + 2 * side), h: tenth(r.h + up + FLOAT_PX + hop) };
}

/** The desktop companion page: pedestal + figure, wardrobe, dock back. */
export function NyxDesk() {
  // The dock's wardrobe, Labs and the tray change her too.
  const [state, setState] = useDesktopNyx();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const artRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const press = useRef<{ x: number; y: number } | null>(null);

  // Reduce motion stills her breath, float and flicker, and her figure
  // shows its picture instead of the clip.
  const still = state?.still;
  useEffect(() => {
    if (still !== undefined) document.documentElement.dataset.reduceMotion = still ? "true" : "false";
  }, [still]);

  // While the chat speaks she hops up with its voice, one frame at a time,
  // until the reply stops or its last level has faded out.
  const stillNow = useRef(still);
  useEffect(() => {
    stillNow.current = still;
  }, [still]);
  useEffect(() => {
    let talk: DesktopNyxTalk | null = null;
    let frame = 0;
    const hop = (px: number) => artRef.current?.style.setProperty("--nyx-hop", `${px}px`);
    function tick() {
      frame = 0;
      const figure = artRef.current?.querySelector<HTMLElement>(".nyx-desk-figure");
      const at = talk ? Date.now() - talk.startedAt : 0;
      if (!talk || at > (talk.levels.length + SMOOTH_FRAMES) * FRAME_MS) {
        talk = null;
        hop(0);
        return;
      }
      hop(talkHop(figure?.offsetHeight ?? 0, smoothLevelAt(talk.levels, at), stillNow.current === true));
      frame = requestAnimationFrame(tick);
    }
    const off = onDesktopNyxTalk((next) => {
      talk = next;
      if (!frame) frame = requestAnimationFrame(tick);
    });
    return () => {
      cancelAnimationFrame(frame);
      void off.then((stop) => stop());
    };
  }, []);

  // A click on the desktop beside her passes through, so the page hears no
  // click: leaving the window, or Escape, closes the menu too.
  useEffect(() => {
    function onDoc() {
      setMenu(null);
    }
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") setMenu(null);
    }
    document.addEventListener("click", onDoc);
    document.addEventListener("keydown", onKey);
    window.addEventListener("blur", onDoc);
    return () => {
      document.removeEventListener("click", onDoc);
      document.removeEventListener("keydown", onKey);
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
        const box = { x: r.left, y: r.top, w: r.width, h: r.height };
        return el.classList.contains("nyx-desk-figure") ? figureReach(box) : box;
      });
      void desktopNyxSetHit(boxes);
    }
    const sizes = typeof ResizeObserver === "function" ? new ResizeObserver(report) : null;
    function watch() {
      sizes?.disconnect();
      for (const el of parts()) sizes?.observe(el);
      report();
    }
    watch();
    // Her figure swaps its own element (clip, still, offline), with no new
    // state here: watch the new one.
    const swaps = new MutationObserver(watch);
    if (artRef.current) swaps.observe(artRef.current, { childList: true, subtree: true });
    window.addEventListener("resize", report);
    return () => {
      window.removeEventListener("resize", report);
      swaps.disconnect();
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
      data-look={state.look}
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
        <Menu
          ref={menuRef}
          className="nyx-desk-menu"
          style={{ left: menu.x, top: menu.y }}
          aria-label={`${COMPANION_NAME} desk`}
          onClose={() => setMenu(null)}
          onClick={(e) => e.stopPropagation()}
        >
          <MenuItem
            onClick={() => {
              setMenu(null);
              void desktopNyxDock().then(setState);
            }}
          >
            Bring her back
          </MenuItem>
          <MenuItem
            onClick={() => {
              setMenu(null);
              void desktopNyxSet({ behind: !state.behind }).then(setState);
            }}
          >
            {state.behind ? "Float above windows" : "Stay behind windows"}
          </MenuItem>
          <p className="nyx-desk-menu-label" role="presentation">
            Change outfit
          </p>
          {state.outfits.map((o) => (
            <MenuItem
              key={o.id}
              aria-checked={state.outfit === o.id}
              onClick={() => {
                setMenu(null);
                void desktopNyxSetOutfit(o.id).then(setState);
              }}
            >
              {o.label}
            </MenuItem>
          ))}
          <MenuItem
            aria-checked={state.outfit == null}
            onClick={() => {
              setMenu(null);
              void desktopNyxSetOutfit(null).then(setState);
            }}
          >
            Pick for me
          </MenuItem>
        </Menu>
      )}
    </div>
  );
}

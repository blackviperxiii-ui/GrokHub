import { memo, useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { COMPANION_NAME } from "./brand";
import {
  desktopNyxDock,
  desktopNyxGet,
  desktopNyxSetOutfit,
  desktopNyxUndock,
  type DesktopNyxState,
} from "./desktopNyxApi";
import { MOOD_LABEL, type Mood } from "./nyx";
import { inShell } from "./transport";

/** Narrower windows keep the whole width for the chat. Keep in step with the
 * `.chat.has-nyx` rule in styles.css. */
export const DOCK_MIN_WIDTH = 900;

const QUERY = `(min-width: ${DOCK_MIN_WIDTH}px)`;

function isWide(): boolean {
  if (typeof window === "undefined") return false;
  if (typeof window.matchMedia === "function") return window.matchMedia(QUERY).matches;
  return window.innerWidth >= DOCK_MIN_WIDTH;
}

function onWidthChange(update: () => void): () => void {
  if (typeof window.matchMedia === "function") {
    const query = window.matchMedia(QUERY);
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }
  window.addEventListener("resize", update);
  return () => window.removeEventListener("resize", update);
}

/** True while the window is at least DOCK_MIN_WIDTH wide. A store, not state
 * plus an effect: React reads the width again once it is listening, so a
 * resize between the first render and the listener isn't lost. */
export function useWide(): boolean {
  return useSyncExternalStore(onWidthChange, isWide, () => false);
}

/** Nyx docked at the chat's right, in the pose for what the chat is doing.
 * A new pose fades in (instantly with reduced motion). When Labs → Desktop
 * companion is on, Put her on the desktop / Bring her back and the wardrobe. */
export const NyxDock = memo(function NyxDock({ url, mood }: { url: string; mood: Mood }) {
  const wide = useWide();
  const [desk, setDesk] = useState<DesktopNyxState | null>(null);
  const refresh = useCallback(() => {
    if (!inShell()) {
      setDesk(null);
      return;
    }
    void desktopNyxGet().then(setDesk, () => setDesk(null));
  }, []);
  useEffect(() => {
    refresh();
  }, [refresh]);

  if (!wide) return null;
  const doing = MOOD_LABEL[mood] ?? MOOD_LABEL["hanging-out"];
  const desktopOn = desk?.enabled === true;

  return (
    <aside className="card nyx-dock" aria-label={COMPANION_NAME}>
      <img key={url} src={url} alt={`${COMPANION_NAME}: ${doing}`} draggable={false} />
      <p className="note">{doing}</p>
      {desktopOn && (
        <div className="nyx-dock-desktop">
          {desk.undocked ? (
            <button
              type="button"
              className="ghost"
              onClick={() => void desktopNyxDock().then(setDesk)}
            >
              Bring her back
            </button>
          ) : (
            <button
              type="button"
              className="ghost"
              onClick={() => void desktopNyxUndock().then(setDesk)}
            >
              Put her on the desktop
            </button>
          )}
          <label className="nyx-dock-outfit">
            Outfit
            <select
              value={desk.outfit ?? ""}
              onChange={(e) => {
                const id = e.target.value;
                void desktopNyxSetOutfit(id === "" ? null : id).then(setDesk);
              }}
            >
              <option value="">Pick for me</option>
              {desk.outfits.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>
        </div>
      )}
    </aside>
  );
});

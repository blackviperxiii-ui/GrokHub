import { memo, useEffect, useState } from "react";
import { COMPANION_NAME } from "./brand";
import {
  desktopNyxDock,
  desktopNyxGet,
  desktopNyxSetOutfit,
  desktopNyxUndock,
  type DesktopNyxState,
} from "./desktopNyxApi";
import { NyxCard } from "./NyxCard";
import { MOOD_LABEL, type Mood } from "./nyx";
import { inShell } from "./transport";
import { useMinWidth } from "./useMinWidth";

/** Narrower windows keep the whole width for the chat. Keep in step with the
 * `.chat-side` rule in styles.css. */
export const DOCK_MIN_WIDTH = 900;

/** From this width the column also holds Chat's side cards (ChatSide.tsx).
 * Keep in step with the wider `.chat-side` rule in styles.css. */
export const SIDE_MIN_WIDTH = 1280;

/** True while the window is at least DOCK_MIN_WIDTH wide. */
export function useWide(): boolean {
  return useMinWidth(DOCK_MIN_WIDTH);
}

/** Nyx docked at the chat's right, in the pose for what the chat is doing.
 * A new pose fades in (instantly with reduced motion). When Labs → Desktop
 * companion is on, Put her on the desktop / Bring her back and the wardrobe. */
export const NyxDock = memo(function NyxDock({
  url,
  path,
  mood,
}: {
  url: string;
  path?: string;
  mood: Mood;
}) {
  const wide = useWide();
  const [desk, setDesk] = useState<DesktopNyxState | null>(null);

  // Mount load only. Inlined like Memory: no refresh wrapper that setStates
  // from the effect (react-hooks/set-state-in-effect). Off the shell, desk
  // stays null; button handlers refresh via their own .then(setDesk).
  useEffect(() => {
    if (!inShell()) return;
    void desktopNyxGet().then(setDesk, () => setDesk(null));
  }, []);

  if (!wide) return null;
  const doing = MOOD_LABEL[mood] ?? MOOD_LABEL["hanging-out"];
  const desktopOn = desk?.enabled === true;

  return (
    <aside className="card nyx-dock" aria-label={COMPANION_NAME}>
      <NyxCard key={url} url={url} path={path} alt={`${COMPANION_NAME}: ${doing}`} draggable={false} />
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

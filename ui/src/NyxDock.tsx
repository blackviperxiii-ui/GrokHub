import { memo, useEffect, useState } from "react";
import { COMPANION_NAME } from "./brand";
import { MOOD_LABEL, type Mood } from "./nyx";

/** Narrower windows keep the whole width for the chat. Keep in step with the
 * `.chat.has-nyx` rule in styles.css. */
export const DOCK_MIN_WIDTH = 900;

const QUERY = `(min-width: ${DOCK_MIN_WIDTH}px)`;

function isWide(): boolean {
  if (typeof window === "undefined") return false;
  if (typeof window.matchMedia === "function") return window.matchMedia(QUERY).matches;
  return window.innerWidth >= DOCK_MIN_WIDTH;
}

/** True while the window is at least DOCK_MIN_WIDTH wide. */
export function useWide(): boolean {
  const [wide, setWide] = useState(isWide);
  useEffect(() => {
    const update = () => setWide(isWide());
    if (typeof window.matchMedia === "function") {
      const query = window.matchMedia(QUERY);
      query.addEventListener("change", update);
      return () => query.removeEventListener("change", update);
    }
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);
  return wide;
}

/** Nyx docked at the chat's right, in the pose for what the chat is doing.
 * A new pose fades in (instantly with reduced motion). */
export const NyxDock = memo(function NyxDock({ url, mood }: { url: string; mood: Mood }) {
  const wide = useWide();
  if (!wide) return null;
  const doing = MOOD_LABEL[mood] ?? MOOD_LABEL["hanging-out"];
  return (
    <aside className="card nyx-dock" aria-label={COMPANION_NAME}>
      <img key={url} src={url} alt={`${COMPANION_NAME}: ${doing}`} draggable={false} />
      <p className="note">{doing}</p>
    </aside>
  );
});

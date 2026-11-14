import { useEffect, useState } from "react";
import { desktopNyxGet, onDesktopNyx, type DesktopNyxState } from "./desktopNyxApi";

/** The desktop companion's state, kept current: read when `active` turns
 * true, then every change from any page or the tray replaces it. The setter
 * is for a command's own reply. A failed read calls `onError`, or clears the
 * state when there is none. */
export function useDesktopNyx(
  active = true,
  onError?: (e: unknown) => void,
): [DesktopNyxState | null, (state: DesktopNyxState | null) => void] {
  const [state, setState] = useState<DesktopNyxState | null>(null);

  useEffect(() => {
    if (!active) return;
    let stop: (() => void) | undefined;
    let dropped = false;
    // A change heard while the read is out is newer than the read's answer.
    let heard = false;
    void onDesktopNyx((next) => {
      heard = true;
      setState(next);
    }).then(
      (unlisten) => {
        if (dropped) unlisten();
        else stop = unlisten;
      },
      () => {},
    );
    void desktopNyxGet().then(
      (read) => {
        if (!dropped && !heard) setState(read);
      },
      (e: unknown) => {
        if (dropped) return;
        if (onError) onError(e);
        else setState(null);
      },
    );
    return () => {
      dropped = true;
      stop?.();
    };
  }, [active, onError]);

  return [state, setState];
}

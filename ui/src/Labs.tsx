import { COMPANION_NAME } from "./brand";
import { desktopNyxSet } from "./desktopNyxApi";
import { Diagnostics } from "./Diagnostics";
import { Toggle } from "./Toggle";
import { inShell } from "./transport";
import { useDesktopNyx } from "./useDesktopNyx";
import { useNote } from "./ui";

export const PLUGINS_MOVED = "Plugins are on the Marketplace page now, beside skills and connectors.";
export const LABS_NOTE = "Early features. They work, but they may change.";
export const CHANNEL_NOTE = "Switch between beta and stable in About, under Updates.";
export const CHANNEL_NOTE_REMOTE =
  "Switch between beta and stable on your computer, in About, under Updates.";
export const DESKTOP_NYX_TIP = `Puts a pop-out on ${COMPANION_NAME}'s dock. Popped out, she stands on a holo pedestal in a small see-through window over your desktop.`;
export const DESKTOP_NYX_GAME = "On the desktop, hidden while you game.";
export const DESKTOP_NYX_WAYLAND =
  "On this desktop, a see-through always-on-top window may not be honoured (common on Wayland). She still opens; the glass and float may not.";

/** Settings → Labs: early features. Plugin bundles moved to the Marketplace page. */
export function Labs() {
  return (
    <div className="labs" role="group" aria-label="Labs">
      <h2>Labs</h2>
      <p className="note">{LABS_NOTE}</p>
      <p className="note">{inShell() ? CHANNEL_NOTE : CHANNEL_NOTE_REMOTE}</p>
      {inShell() && <DesktopCompanion />}
      <p className="note">{PLUGINS_MOVED}</p>
      <Diagnostics />
    </div>
  );
}

/** Settings → Labs → Desktop companion. Off by default. */
function DesktopCompanion() {
  const { fail } = useNote();
  // The tray and the desk page change it too; their events land here.
  const [desk, setDesk] = useDesktopNyx(true, fail);
  if (!desk) return null;
  const effectsOk = desk.effects.transparent && desk.effects.onTop;
  return (
    <div className="labs-desktop-nyx" role="group" aria-label={`Desktop ${desk.companion}`}>
      <h3>
        Desktop {desk.companion}{" "}
        <span className="badge" title={DESKTOP_NYX_TIP}>
          Preview
        </span>
      </h3>
      <Toggle
        checked={desk.enabled}
        onChange={(on) => void desktopNyxSet({ enabled: on }).then(setDesk, fail)}
      >
        Desktop companion
      </Toggle>
      <p className="note">{DESKTOP_NYX_TIP}</p>
      {!effectsOk && <p className="note warn">{DESKTOP_NYX_WAYLAND}</p>}
      {desk.enabled && (
        <>
          <Toggle
            checked={desk.behind}
            onChange={(behind) => void desktopNyxSet({ behind }).then(setDesk, fail)}
          >
            Stay behind windows
          </Toggle>
          <p className="note">
            On: she sits on the desktop and your apps cover her. Off: she floats above them.
          </p>
          <Toggle
            checked={desk.adult}
            onChange={(adult) => void desktopNyxSet({ adult }).then(setDesk, fail)}
          >
            Allow 18+ outfits on the desktop
          </Toggle>
          <p className="note">
            Off by default: the desktop is visible to anyone at the screen. Needs adults-only
            personalities on.
          </p>
          <p className="note">
            {!desk.undocked
              ? "Docked in the chat. Use Put her on the desktop on her dock."
              : desk.hiddenForGame
                ? DESKTOP_NYX_GAME
                : "On the desktop. Use Bring her back on her dock or right-click menu."}
          </p>
        </>
      )}
    </div>
  );
}

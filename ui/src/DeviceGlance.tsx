// The chat's device glance, as the cabin had it: on an empty chat, one small
// row says "Sharing" while this computer's hub is on, or "Last frame" with a
// thumbnail of the newest screenshot. Tapping it opens Devices.
import { useEffect, useState } from "react";
import { getDeviceGlance, type DeviceGlance } from "./api";

/** How often the row re-reads, so a share started elsewhere shows. */
export const GLANCE_MS = 15_000;

export function glanceLine(glance: DeviceGlance): string {
  return glance.sharing ? "Sharing" : "Last frame";
}

/** Only an inline image is drawn; anything else shows no thumbnail. */
export function glanceThumb(glance: DeviceGlance): string | null {
  const frame = glance.frame?.trim() ?? "";
  return /^data:image\/[a-z0-9.+-]+;base64,/i.test(frame) ? frame : null;
}

export function DeviceGlanceRow({ onOpen }: { onOpen: () => void }) {
  const [glance, setGlance] = useState<DeviceGlance | null>(null);

  useEffect(() => {
    let live = true;
    function read() {
      // A build with no hub (the phone app, the web app) refuses the call:
      // the row stays hidden.
      Promise.resolve()
        .then(() => getDeviceGlance())
        .then(
          (next) => {
            if (live) setGlance(next ?? null);
          },
          () => {
            if (live) setGlance(null);
          },
        );
    }
    read();
    const timer = window.setInterval(read, GLANCE_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, []);

  if (!glance) return null;
  const line = glanceLine(glance);
  const thumb = glanceThumb(glance);
  return (
    <button type="button" className="device-glance" aria-label={`${line}. Open Devices`} onClick={onOpen}>
      <span>{line}</span>
      {thumb && <img src={thumb} alt="" width={16} height={16} />}
    </button>
  );
}

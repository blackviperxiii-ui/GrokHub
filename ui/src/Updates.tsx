import { useEffect, useState } from "react";
import {
  checkShellUpdate,
  getUpdateStatus,
  installShellUpdate,
  onUpdateAvailable,
  setUpdateChannel,
  type UpdateChannel,
  type UpdateView,
} from "./api";
import { PRODUCT_NAME } from "./brand";

export const CHANNEL_NOTE =
  `Beta gets every new build first. The channel is shared with the ${PRODUCT_NAME} cabin, so both follow the same builds.`;
export const VERIFY_NOTE =
  "Updates come from GitHub and install only after their signature checks out.";
export const UP_TO_DATE = "You're up to date.";

/** Settings → About → Updates: the channel, Check now, Install and restart. */
export function Updates() {
  const [view, setView] = useState<UpdateView | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState<"check" | "install" | null>(null);

  useEffect(() => {
    getUpdateStatus().then(setView, (err) => setNote(String(err)));
    const off = onUpdateAvailable((found) => setView(found));
    return () => {
      void off.then((stop) => stop());
    };
  }, []);

  async function pick(channel: UpdateChannel) {
    setNote(null);
    try {
      setView(await setUpdateChannel(channel));
    } catch (err) {
      setNote(String(err));
    }
  }

  async function check() {
    setBusy("check");
    setNote(null);
    try {
      const found = await checkShellUpdate();
      setView(found);
      if (!found.available) setNote(UP_TO_DATE);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(null);
    }
  }

  async function install() {
    setBusy("install");
    setNote(null);
    try {
      await installShellUpdate();
    } catch (err) {
      setNote(String(err));
      setBusy(null);
    }
  }

  if (!view) return note ? <p className="note">{note}</p> : null;
  return (
    <div className="updates" role="group" aria-label="Updates">
      <h3>Updates</h3>
      <fieldset className="channel">
        <legend>Channel</legend>
        {(["stable", "beta"] as const).map((channel) => (
          <label key={channel}>
            <input
              type="radio"
              name="update-channel"
              checked={view.channel === channel}
              onChange={() => void pick(channel)}
            />
            {channel === "stable" ? "Stable" : "Beta"}
          </label>
        ))}
      </fieldset>
      <p className="note">{CHANNEL_NOTE}</p>
      {view.updatable ? (
        <>
          <p className="note">{VERIFY_NOTE}</p>
          {view.available ? (
            <button type="button" onClick={() => void install()} disabled={busy !== null}>
              {busy === "install" ? "Installing…" : `Install ${view.available} and restart`}
            </button>
          ) : (
            <button type="button" onClick={() => void check()} disabled={busy !== null}>
              {busy === "check" ? "Checking…" : "Check now"}
            </button>
          )}
        </>
      ) : (
        view.note && <p className="note">{view.note}</p>
      )}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}

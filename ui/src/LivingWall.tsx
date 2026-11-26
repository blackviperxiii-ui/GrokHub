import { useEffect, useState } from "react";
import { getLivingWall, setLivingWall } from "./api";
import { PRODUCT_NAME } from "./brand";
import { Toggle } from "./Toggle";

/** The cabin's words for Settings → Behavior → Living wall. */
export const LIVING_WALL_TITLE = "Living wall";
export const LIVING_WALL_HELP = `Every few hours ${PRODUCT_NAME} paints a new cover for Library → Creations, two low-quality stills each. Twenty live. Oldest leaves first. Off by default.`;

/**
 * Living wall switch in Settings → Behavior. Same `app.json` `imagineWall` key
 * as the cabin. An app with no wall (a phone, a paired browser) refuses the
 * read, and the row stays out.
 */
export function LivingWall() {
  const [on, setOn] = useState<boolean | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getLivingWall().then(
      (view) => setOn(view.on),
      () => setOn(null),
    );
  }, []);

  async function change(next: boolean) {
    setBusy(true);
    try {
      setOn((await setLivingWall(next)).on);
      setNote(null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (on === null) return null;
  return (
    <div role="group" aria-label={LIVING_WALL_TITLE}>
      <h2>{LIVING_WALL_TITLE}</h2>
      <Toggle checked={on} disabled={busy} onChange={(next) => void change(next)}>
        {LIVING_WALL_TITLE}
      </Toggle>
      <p className="note">{LIVING_WALL_HELP}</p>
      {note && (
        <p className="note warn" role="status">
          {note}
        </p>
      )}
    </div>
  );
}

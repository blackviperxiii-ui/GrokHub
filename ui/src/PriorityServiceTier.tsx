import { useEffect, useState } from "react";
import { getPriorityServiceTier, setPriorityServiceTier } from "./api";
import { Toggle } from "./Toggle";

export const PRIORITY_TIER_TITLE = "Priority service tier";
export const PRIORITY_TIER_HELP =
  "Off by default. When on, xAI /v1/responses uses Priority processing. That is a higher token cost than the default tier.";

/**
 * Settings → Account. Same `app.json` `priorityServiceTier` key the chat
 * client reads. Off until switched on.
 */
export function PriorityServiceTier() {
  const [on, setOn] = useState<boolean | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getPriorityServiceTier().then(
      (view) => setOn(view == null ? null : view.on),
      () => setOn(null),
    );
  }, []);

  async function change(next: boolean) {
    setBusy(true);
    try {
      setOn((await setPriorityServiceTier(next)).on);
      setNote(null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (on === null) return null;
  return (
    <div role="group" aria-label={PRIORITY_TIER_TITLE}>
      <h2>{PRIORITY_TIER_TITLE}</h2>
      <Toggle checked={on} disabled={busy} onChange={(next) => void change(next)}>
        {PRIORITY_TIER_TITLE}
      </Toggle>
      <p className="note">{PRIORITY_TIER_HELP}</p>
      {note && (
        <p className="note warn" role="status">
          {note}
        </p>
      )}
    </div>
  );
}
